//! Assistir sem WebRTC: a mídia chega por RTP puro e o GStreamer decodifica.
//!
//! O WebKitGTK das distribuições vem sem WebRTC — é por isso que este app existe. Então o
//! caminho de recepção é todo nativo: um `PlainReceiver` por sessão (o servidor manda tela,
//! câmera e microfone de todo mundo pelo MESMO transporte, e o que os separa é o SSRC) e um
//! `gst-launch` por producer.
//!
//! O vídeo sai do GStreamer como RGB cru na saída padrão, e a janela o desenha como
//! textura. Cru, e não JPEG: quem lê está no mesmo computador, e um quadro que atravessa um
//! cano local não precisa ser comprimido de novo só para ser aberto de novo logo depois.
//!
//! ponytail: o cartão é 720p30 fixo, que é o teto deste caminho. A saída é o pipeline
//! dentro do processo (`gstreamer-rs`), que entrega o quadro sem passar por cano nenhum.

use std::collections::HashMap;
use std::io::Read;
use std::net::{SocketAddr, UdpSocket};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use media::PlainReceiver;

/// O tamanho do cartão. Fixo para o quadro ter sempre o mesmo número de bytes: é isso que
/// permite ler a saída do GStreamer sem procurar separador nenhum.
pub const TILE: (u32, u32) = (1280, 720);

const FRAME_BYTES: usize = TILE.0 as usize * TILE.1 as usize * 3;

/// O quadro mais novo de uma transmissão. A janela desenha este e larga o que ficou para
/// trás: quadro atrasado não interessa a ninguém, e segurá-los faria a fila crescer para
/// sempre quando a janela não acompanhasse.
type LatestFrame = Arc<Mutex<Option<Vec<u8>>>>;

/// O que o `consumePlain` respondeu sobre uma transmissão: por onde ela vem, com que chave
/// abrir e com que SSRC separá-la das outras que chegam pelo mesmo socket.
pub struct Incoming<'a> {
    pub producer_id: String,
    pub kind: &'a str,
    pub address: &'a str,
    pub server_key: &'a [u8],
    pub payload_type: u8,
    /// O que o servidor devolveu; sem ele o receptor aprende no primeiro pacote.
    pub ssrc: Option<u32>,
    /// Som de tela compartilhada, que chega mudo por regra.
    pub always_muted: bool,
}

struct Watch {
    player: Child,
    stop: Arc<AtomicBool>,
    frame: LatestFrame,
    /// A receita, para reabrir o tocador na saída de áudio nova sem mexer na rota.
    pipeline: String,
    video: bool,
    /// Som de tela compartilhada, que chega mudo por regra e continua mudo ao desensurdecer.
    always_muted: bool,
}

#[derive(Default)]
pub struct Watching {
    /// A chave SRTP deste lado, uma só para o app inteiro. Só protege o pacote que abre o
    /// caminho no roteador; o receptor sorteia o próprio SSRC.
    key: Option<[u8; 30]>,
    /// O socket da sessão, aberto no primeiro producer e fechado com o último.
    receiver: Option<PlainReceiver>,
    active: HashMap<String, Watch>,
    deafened: bool,
}

impl Watching {
    /// A chave que vai ao servidor no `consumePlain`.
    pub fn key(&mut self) -> [u8; 30] {
        *self.key.get_or_insert_with(media::PlainSender::generate_key)
    }

    pub fn is_watching(&self, producer_id: &str) -> bool {
        self.active.contains_key(producer_id)
    }

    /// Abre o tocador de uma transmissão e passa a encaminhar o que chegar dela.
    pub fn start(&mut self, incoming: Incoming<'_>) -> Result<()> {
        let Incoming { producer_id, kind, address, server_key, payload_type, ssrc, always_muted } =
            incoming;

        // Outro endereço é outra sessão no servidor: o que estava aberto já morreu lá.
        if self
            .receiver
            .as_ref()
            .is_some_and(|receiver| media::resolve(address).ok() != Some(receiver.server()))
        {
            self.stop(None);
        }

        if self.active.contains_key(&producer_id) {
            return Ok(());
        }

        let key = self.key();

        if self.receiver.is_none() {
            self.receiver = Some(
                PlainReceiver::start(address, &key, server_key).map_err(|error| anyhow!("{error}"))?,
            );
        }

        let to = free_port()?;
        let video = kind == "video";
        let recipe = pipeline(video, to.port(), payload_type);
        let mut player = spawn(&recipe, video)?;
        let stop = Arc::new(AtomicBool::new(false));
        let frame: LatestFrame = Arc::default();

        if video {
            let Some(stdout) = player.stdout.take() else {
                let _ = player.kill();

                return Err(anyhow!("o gst-launch abriu sem saída padrão"));
            };

            read_frames(stdout, Arc::clone(&frame), Arc::clone(&stop));
        }

        if let Some(receiver) = self.receiver.as_ref() {
            receiver.route(producer_id.clone(), payload_type, to, ssrc);
        }

        if always_muted || self.deafened {
            self.set_muted(&producer_id, true);
        }

        tracing::info!(producer = %producer_id, kind, "assistindo por RTP puro");
        self.active
            .insert(producer_id, Watch { player, stop, frame, pipeline: recipe, video, always_muted });

        Ok(())
    }

    /// `None` fecha a sessão inteira. O socket só sai nesse caso: o SFU manda para o
    /// endereço que o `comedia` aprendeu, e um socket novo numa porta nova não recebe mais
    /// nada naquela sala.
    pub fn stop(&mut self, producer_id: Option<&str>) {
        let keys: Vec<String> = match producer_id {
            Some(producer_id) => vec![producer_id.to_owned()],
            None => self.active.keys().cloned().collect(),
        };

        for key in keys {
            if let Some(mut watch) = self.active.remove(&key) {
                watch.stop.store(true, Ordering::Relaxed);
                let _ = watch.player.kill();
                let _ = watch.player.wait();
            }

            if let Some(receiver) = self.receiver.as_ref() {
                receiver.unroute(&key);
            }
        }

        if producer_id.is_none() {
            self.receiver = None;
        }
    }

    /// Mudo é não repassar o pacote: o decodificador só vê silêncio.
    pub fn set_muted(&self, producer_id: &str, muted: bool) {
        if let Some(receiver) = self.receiver.as_ref() {
            receiver.set_muted(producer_id, muted);
        }
    }

    /// Ensurdecer cala só o áudio: pausar o vídeo faria esperar keyframe na volta.
    pub fn deafen(&mut self, deafened: bool) {
        self.deafened = deafened;

        for (producer_id, watch) in &self.active {
            if !watch.video {
                self.set_muted(producer_id, deafened || watch.always_muted);
            }
        }
    }

    pub fn is_deafened(&self) -> bool {
        self.deafened
    }

    /// Reabre os tocadores para eles pegarem a saída de áudio escolhida agora. A rota não
    /// muda: o `udpsrc` volta na mesma porta, e o servidor nem fica sabendo.
    pub fn use_new_output(&mut self) {
        for watch in self.active.values_mut().filter(|watch| !watch.video) {
            let _ = watch.player.kill();
            let _ = watch.player.wait();

            match spawn(&watch.pipeline, false) {
                Ok(player) => watch.player = player,
                Err(failure) => tracing::warn!(%failure, "o tocador não voltou na saída nova"),
            }
        }
    }

    /// O que chegou desde a última vez, por producer. Quem desenha chama isto no relógio
    /// dele, e nunca recebe o mesmo quadro duas vezes.
    pub fn fresh_frames(&self) -> Vec<(String, Vec<u8>)> {
        self.active
            .iter()
            .filter_map(|(producer_id, watch)| {
                let frame = watch.frame.lock().ok()?.take()?;

                Some((producer_id.clone(), frame))
            })
            .collect()
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        // Cada producer é um `gst-launch` filho, e a janela fechar não o mata sozinha.
        self.stop(None);
    }
}

/// O vídeo sai em RGB no tamanho do cartão; o som vai direto para a saída do sistema.
///
/// `do-lost` avisa o decodificador do que o jitter buffer perdeu: no Opus o PLC e o FEC
/// recompõem o pedaço, e no vídeo o quadro quebrado se refaz no keyframe seguinte.
fn pipeline(video: bool, port: u16, payload_type: u8) -> String {
    let (width, height) = TILE;

    if video {
        return format!(
            "udpsrc address=127.0.0.1 port={port} caps=application/x-rtp,media=video,encoding-name=H264,clock-rate=90000,payload={payload_type} \
             ! rtpjitterbuffer latency=80 do-lost=true ! rtph264depay ! h264parse ! avdec_h264 thread-type=slice \
             ! videorate ! video/x-raw,framerate=30/1 ! videoscale ! videoconvert \
             ! video/x-raw,format=RGB,width={width},height={height},pixel-aspect-ratio=1/1 ! fdsink fd=1 sync=false"
        );
    }

    format!(
        "udpsrc address=127.0.0.1 port={port} caps=application/x-rtp,media=audio,encoding-name=OPUS,clock-rate=48000,payload={payload_type} \
         ! rtpjitterbuffer latency=80 do-lost=true ! rtpopusdepay ! opusdec plc=true use-inband-fec=true \
         ! audioconvert ! audioresample ! pulsesink buffer-time=40000 latency-time=10000"
    )
}

fn spawn(pipeline: &str, video: bool) -> Result<Child> {
    Command::new("gst-launch-1.0")
        .arg("-q")
        .args(pipeline.split_whitespace())
        .stdin(Stdio::null())
        .stdout(if video { Stdio::piped() } else { Stdio::null() })
        .stderr(Stdio::inherit())
        .spawn()
        .context("gst-launch-1.0 não abriu; instale gstreamer1.0-tools e os plugins good/libav")
}

/// Lê quadro por quadro. O tamanho é fixo (o pipeline força largura, altura e formato), o
/// que dispensa procurar marcador de fim: cada `FRAME_BYTES` é um quadro inteiro.
fn read_frames(mut stdout: impl Read + Send + 'static, frame: LatestFrame, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            let mut pixels = vec![0_u8; FRAME_BYTES];

            if stdout.read_exact(&mut pixels).is_err() {
                break;
            }

            if let Ok(mut latest) = frame.lock() {
                *latest = Some(pixels);
            }
        }
    });
}

/// Uma porta local livre para o `udpsrc`. Abrir e fechar tem uma janela de corrida teórica;
/// na prática ninguém mais pega uma porta efêmera neste milissegundo.
fn free_port() -> Result<SocketAddr> {
    Ok(UdpSocket::bind("127.0.0.1:0")?.local_addr()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_only_handed_over_once() {
        let frame: LatestFrame = Arc::default();
        let (reader, mut writer) = std::io::pipe().expect("um cano");

        read_frames(reader, Arc::clone(&frame), Arc::new(AtomicBool::new(false)));

        std::io::Write::write_all(&mut writer, &vec![7_u8; FRAME_BYTES]).expect("um quadro");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);

        while frame.lock().expect("o quadro").is_none() {
            assert!(std::time::Instant::now() < deadline, "o quadro nunca chegou");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let taken = frame.lock().expect("o quadro").take().expect("um quadro inteiro");

        assert_eq!(taken.len(), FRAME_BYTES);
        assert!(frame.lock().expect("o quadro").is_none(), "o mesmo quadro sairia duas vezes");
    }

    #[test]
    fn the_video_pipeline_hands_the_window_raw_pixels_of_a_known_size() {
        let video = pipeline(true, 5004, 96);

        assert!(video.contains("format=RGB,width=1280,height=720"), "{video}");
        assert!(video.contains("payload=96"), "{video}");
        assert!(!video.contains("jpeg"), "recodificar aqui é trabalho que ninguém pediu");

        // O som não passa pela janela: vai do GStreamer para a saída do sistema.
        assert!(pipeline(false, 5004, 111).contains("pulsesink"));
    }
}
