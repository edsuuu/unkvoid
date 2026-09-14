//! Assistir sem WebRTC na janela: a mídia chega por RTP puro (`PlainReceiver`), o
//! GStreamer decodifica, e o quadro entra no app como MJPEG numa `<img>` — um
//! servidorzinho HTTP em 127.0.0.1 que o webview lê. O som vai direto para a saída.
//!
//! É o caminho do Linux, onde o WebKitGTK das distros vem sem WebRTC. Nos outros
//! sistemas o webview assiste sozinho e isto nunca é chamado.
//!
//! Um pipeline por producer (a tela, a câmera, o microfone de cada pessoa), todos
//! alimentados por UM `PlainReceiver`: o servidor manda tudo pelo mesmo transporte, e o
//! que separa é o SSRC — ver `receiver.rs`.
//!
//! ponytail: MJPEG custa CPU (decodifica H.264 e recodifica JPEG) e não tem som na
//! `<img>`. O passo seguinte é fMP4 por MSE, sem recodificar, se a CPU pesar.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use media::PlainReceiver;

pub struct Watch {
    player: Child,
    /// Porta do MJPEG em 127.0.0.1, para a `<img>` do cartão. Zero para áudio.
    port: u16,
    stop: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct Watches {
    /// A chave SRTP deste lado, uma só para o app inteiro. Só protege o pacote que abre
    /// o caminho no roteador; o receptor sorteia o próprio SSRC.
    key: Option<[u8; 30]>,
    /// O socket da sessão, aberto no primeiro producer e fechado com o último.
    receiver: Option<PlainReceiver>,
    /// Por producer.
    active: HashMap<String, Watch>,
}

impl Watches {
    pub fn key(&mut self) -> [u8; 30] {
        *self.key.get_or_insert_with(|| std::array::from_fn(|_| rand::random()))
    }

    /// `ssrc` é o que o servidor devolveu no `consumePlain`; sem ele o receptor aprende
    /// no primeiro pacote (servidor antigo).
    pub fn start(
        &mut self,
        producer_id: String,
        kind: &str,
        address: &str,
        server_key: &[u8],
        payload_type: u8,
        ssrc: Option<u32>,
    ) -> Result<u16> {
        // Outro endereço é outra sessão no servidor: o que estava aberto já morreu lá —
        // inclusive um producer de mesmo id que ainda conste como ativo.
        if self.receiver.as_ref().is_some_and(|receiver| media::resolve(address).ok() != Some(receiver.server())) {
            self.stop(None);
        }

        if let Some(watch) = self.active.get(&producer_id) {
            return Ok(watch.port);
        }

        let key = self.key();

        if self.receiver.is_none() {
            self.receiver = Some(PlainReceiver::start(address, &key, server_key).map_err(|error| anyhow!("{error}"))?);
        }

        let to = free_port()?;
        let mut pipeline: Vec<String> = vec!["-q".into()];

        if kind == "video" {
            // `colorimetry=2:4:7:1` é BT.601 de faixa cheia, que é o que JFIF quer dizer;
            // o atalho `bt709` é faixa limitada e saía lavado. `thread-type=slice`
            // decodifica sem esperar o quadro seguinte. O cartão é desenhado em 720p a
            // 30 quadros: recodificar JPEG em 1080p60 era o que pesava na CPU.
            //
            // ponytail: MJPEG em 1280x720@30 é o teto deste caminho; fMP4 por MSE, sem
            // recodificar, é o passo seguinte se a CPU ainda pesar.
            pipeline.extend(
                format!(
                    "udpsrc address=127.0.0.1 port={} caps=application/x-rtp,media=video,encoding-name=H264,clock-rate=90000,payload={payload_type} \
                     ! rtpjitterbuffer latency=80 do-lost=true ! rtph264depay ! h264parse ! avdec_h264 thread-type=slice \
                     ! videorate ! video/x-raw,framerate=30/1 ! videoscale ! video/x-raw,width=1280,height=720,pixel-aspect-ratio=1/1 \
                     ! videoconvert ! video/x-raw,colorimetry=2:4:7:1 ! jpegenc quality=70 ! fdsink fd=1 sync=false",
                    to.port()
                )
                .split_whitespace()
                .map(String::from),
            );
        } else {
            // Opus com FEC e PLC recompõe o que o jitter buffer marcou como perdido
            // (`do-lost`); o `pulsesink` com 40 ms de buffer é o que segura a latência
            // baixa sem estalar.
            pipeline.extend(
                format!(
                    "udpsrc address=127.0.0.1 port={} caps=application/x-rtp,media=audio,encoding-name=OPUS,clock-rate=48000,payload={payload_type} \
                     ! rtpjitterbuffer latency=80 do-lost=true ! rtpopusdepay ! opusdec plc=true use-inband-fec=true \
                     ! audioconvert ! audioresample ! pulsesink buffer-time=40000 latency-time=10000",
                    to.port()
                )
                .split_whitespace()
                .map(String::from),
            );
        }

        let mut player = Command::new("gst-launch-1.0")
            .args(&pipeline)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("gst-launch-1.0 não abriu; instale gstreamer1.0-tools e os plugins good/libav")?;

        let stop = Arc::new(AtomicBool::new(false));
        let port = match (kind, player.stdout.take()) {
            ("video", Some(stdout)) => serve_mjpeg(stdout, Arc::clone(&stop)),
            ("video", None) => Err(anyhow!("o gst-launch abriu sem stdout")),
            _ => Ok(0),
        };

        let port = match port {
            Ok(port) => port,
            Err(error) => {
                let _ = player.kill();

                // O socket foi aberto para este producer; sem ele, não fica ninguém.
                if self.active.is_empty() {
                    self.receiver = None;
                }

                return Err(error);
            }
        };

        if let Some(receiver) = self.receiver.as_ref() {
            receiver.route(producer_id.clone(), payload_type, to, ssrc);
        }

        tracing::info!(producer = %producer_id, kind, %address, port, "assistindo por RTP puro");

        self.active.insert(producer_id, Watch { player, port, stop });

        Ok(port)
    }

    pub fn stop(&mut self, producer_id: Option<&str>) {
        let keys: Vec<String> = match producer_id {
            Some(producer_id) => vec![producer_id.to_string()],
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

        if self.active.is_empty() {
            self.receiver = None;
        }
    }

    pub fn set_muted(&self, producer_id: &str, muted: bool) {
        if let Some(receiver) = self.receiver.as_ref() {
            receiver.set_muted(producer_id, muted);
        }
    }

    /// Pacotes repassados na sessão inteira: é um socket só, e o que interessa ao
    /// diagnóstico é se algo chega.
    pub fn packets(&self) -> u64 {
        self.receiver.as_ref().map(PlainReceiver::packets).unwrap_or(0)
    }
}

/// Os JPEGs do `jpegenc` viram uma resposta HTTP multipart que nunca acaba, um quadro
/// por parte. Um cliente por vez: é a `<img>` do cartão. Quem conectar depois toma o
/// lugar.
///
/// As partes são montadas aqui, e não pelo `multipartmux`, por causa do
/// `Content-Length`: com ele o WebKit desenha o quadro assim que chega em vez de esperar
/// o separador do seguinte — um quadro inteiro de atraso a menos.
fn serve_mjpeg(mut stdout: impl Read + Send + 'static, stop: Arc<AtomicBool>) -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("could not open the MJPEG port")?;
    let port = listener.local_addr()?.port();
    let client: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));

    listener.set_nonblocking(true)?;

    let client_accept = Arc::clone(&client);
    let stop_accept = Arc::clone(&stop);

    std::thread::spawn(move || {
        while ! stop_accept.load(Ordering::Relaxed) {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            };

            let mut request = [0_u8; 1024];
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            // Quadro pequeno não espera o Nagle juntar com o próximo.
            let _ = stream.set_nodelay(true);
            let _ = stream.read(&mut request);

            let headers = "HTTP/1.1 200 OK\r\n\
                Content-Type: multipart/x-mixed-replace; boundary=unkvoid\r\n\
                Cache-Control: no-cache\r\nConnection: close\r\n\r\n";

            if stream.write_all(headers.as_bytes()).is_ok()
                && let Ok(mut current) = client_accept.lock()
            {
                *current = Some(stream);
            }
        }
    });

    std::thread::spawn(move || {
        let mut chunk = vec![0_u8; 64 * 1024];
        let mut pending = Vec::new();

        loop {
            let read = match stdout.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };

            if stop.load(Ordering::Relaxed) {
                break;
            }

            pending.extend_from_slice(&chunk[..read]);

            while let Some(jpeg) = take_jpeg(&mut pending) {
                if let Ok(mut current) = client.lock()
                    && let Some(stream) = current.as_mut()
                    && stream.write_all(&mjpeg_part(&jpeg)).is_err()
                {
                    *current = None;
                }
            }
        }
    });

    Ok(port)
}

/// Um JPEG inteiro do começo do buffer, até o marcador de fim (`FF D9`), que dentro dos
/// dados comprimidos não aparece: todo `FF` ali vem seguido de `00` ou de um `RSTn`.
fn take_jpeg(pending: &mut Vec<u8>) -> Option<Vec<u8>> {
    let end = pending.windows(2).position(|pair| pair == [0xFF, 0xD9])? + 2;

    Some(pending.drain(..end).collect())
}

fn mjpeg_part(jpeg: &[u8]) -> Vec<u8> {
    let mut part = format!(
        "--unkvoid\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
        jpeg.len()
    )
    .into_bytes();

    part.extend_from_slice(jpeg);
    part.extend_from_slice(b"\r\n");

    part
}

/// Uma porta local livre para o `udpsrc`. Abrir e fechar tem uma janela de corrida
/// teórica; na prática ninguém mais pega uma porta efêmera neste milissegundo.
fn free_port() -> Result<SocketAddr> {
    Ok(UdpSocket::bind("127.0.0.1:0")?.local_addr()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpegs_are_split_on_the_end_marker_and_framed_with_a_length() {
        let mut pending = vec![0xFF, 0xD8, 0xFF, 0x00, 0xFF, 0xD9, 0xFF, 0xD8, 0x01];

        let first = take_jpeg(&mut pending).expect("um JPEG fechado");

        assert_eq!(first, [0xFF, 0xD8, 0xFF, 0x00, 0xFF, 0xD9]);
        assert!(take_jpeg(&mut pending).is_none(), "o segundo ainda está aberto");
        assert_eq!(pending, [0xFF, 0xD8, 0x01]);

        let part = mjpeg_part(&first);
        let head = "--unkvoid\r\nContent-Type: image/jpeg\r\nContent-Length: 6\r\n\r\n";

        assert!(part.starts_with(head.as_bytes()));
        assert!(part.ends_with(b"\xFF\xD9\r\n"));
    }
}
