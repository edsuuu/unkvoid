//! Assistir no Linux: a fila de mídia do `Room` do núcleo, com o GStreamer decodificando.
//!
//! O `Room` recebe, recupera o pacote que se perdeu e remonta o quadro H.264 — o mesmo
//! caminho do macOS e do Windows. Aqui cada tela ganha um `gst-launch` que lê o quadro pela
//! entrada padrão e devolve RGB cru na saída, e cada som um que toca direto na saída do
//! sistema.
//!
//! O vídeo sai do GStreamer como RGB cru, e a janela o desenha como textura. Cru, e não
//! JPEG: quem lê está no mesmo computador, e um quadro que atravessa um cano local não
//! precisa ser comprimido de novo só para ser aberto de novo logo depois.
//!
//! ponytail: o cartão é 720p fixo e o decodificador é o de software (`avdec_h264`). Teto:
//! uma tela 1080p60 custa um núcleo de quem assiste. A saída é o pipeline dentro do processo
//! (`gstreamer-rs`) com o decodificador da placa, sem passar por cano nenhum.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use core_app::speaking::Speaking;
use core_app::watching::{Media, MediaKind, Stalled, WORTH_TELLING};

/// O tamanho do cartão. Fixo para o quadro ter sempre o mesmo número de bytes: é isso que
/// permite ler a saída do GStreamer sem procurar separador nenhum.
pub const TILE: (u32, u32) = (1280, 720);

const FRAME_BYTES: usize = TILE.0 as usize * TILE.1 as usize * 3;

/// De quanto em quanto tempo a thread olha se mandaram parar, e se alguém calou.
const PATIENCE: Duration = Duration::from_millis(100);

/// Quantos blocos de som esperam o tocador. Passou disso, ele não está acompanhando.
const WAITING_SOUND: usize = 16;

/// Quantos quadros esperam o horário deles: meio segundo a 60 fps — a espera mais longa do
/// `Playout` — com folga. Passou disso, o decodificador não está acompanhando.
const WAITING_VIDEO: usize = 48;

/// Tocador de som sem nada chegando há mais que isto é de quem saiu ou pausou.
const IDLE: Duration = Duration::from_secs(3);

/// Tela sem quadro há mais que isto solta o `gst-launch`: o producer fechou, ou quem transmite
/// está com a tela parada (o Windows fica até um minuto sem mandar quadro) — aí ela renasce no
/// próximo quadro-chave, que é pedido. Com os 3 s do som, a tela parada que voltava a mexer
/// ficava congelada até o quadro-chave periódico.
const SCREEN_IDLE: Duration = Duration::from_secs(20);

/// O quadro mais novo de uma transmissão. A janela desenha este e larga o que ficou para
/// trás: quadro atrasado não interessa a ninguém.
type LatestFrame = Arc<Mutex<Option<Vec<u8>>>>;

/// O pedido de quadro-chave ao servidor, para a tela que quebrou do lado de cá.
type AskKeyframe = Box<dyn Fn(&str) + Send>;

/// A espera de quadro-chave de cada tela.
type Stalls = HashMap<String, Stalled>;

/// Um `gst-launch` por transmissão, com a thread que escreve na entrada dele. No vídeo cada
/// quadro vai com o horário de entrar.
struct Player {
    child: Child,
    feed: SyncSender<(Option<Instant>, Vec<u8>)>,
    frame: Option<LatestFrame>,
    /// Vídeo que perdeu um bloco na fila espera o próximo keyframe: quadro P sem o que veio
    /// antes só desenharia lixo.
    waiting_keyframe: bool,
    last: Instant,
    /// O "jitter buffer" do navegador: o horário de cada quadro sai do relógio do RTP de quem
    /// transmite, e um bolo de quadros segurado por um reenvio entra no decodificador
    /// espaçado, e não de uma vez — antes a imagem andava aos trancos.
    playout: media::Playout,
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

type Frames = Arc<Mutex<HashMap<String, LatestFrame>>>;

pub struct Watch {
    frames: Frames,
    /// Trocar a saída de áudio: os tocadores de som renascem na saída nova.
    reopen_sound: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Watch {
    /// `on_speaking` recebe o producer e se ele começou (`true`) ou parou de falar.
    /// `ask_keyframe` pede ao servidor um keyframe da transmissão cuja tela quebrou aqui.
    pub fn start(
        queue: Receiver<Media>,
        on_speaking: impl Fn(&str, bool) + Send + 'static,
        ask_keyframe: impl Fn(&str) + Send + 'static,
    ) -> Self {
        let (frames, reopen_sound, stop) = (Frames::default(), Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
        let ask_keyframe: AskKeyframe = Box::new(ask_keyframe);
        let thread = std::thread::Builder::new()
            .name("unkvoid-assistir".into())
            .spawn({
                let (frames, reopen_sound, stop) = (frames.clone(), reopen_sound.clone(), stop.clone());

                move || route(&queue, &frames, (&reopen_sound, &stop), (&on_speaking, &ask_keyframe))
            })
            .ok();

        Self { frames, reopen_sound, stop, thread }
    }

    /// O que chegou desde a última vez, por producer. Quem desenha chama isto no relógio
    /// dele, e nunca recebe o mesmo quadro duas vezes.
    pub fn fresh_frames(&self) -> Vec<(String, Vec<u8>)> {
        lock(&self.frames)
            .iter()
            .filter_map(|(producer, frame)| Some((producer.clone(), lock(frame).take()?)))
            .collect()
    }

    pub fn reopen_sound(&self) {
        self.reopen_sound.store(true, Ordering::Relaxed);
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn route(
    queue: &Receiver<Media>,
    frames: &Frames,
    (reopen_sound, stop): (&AtomicBool, &AtomicBool),
    (on_speaking, ask_keyframe): (&impl Fn(&str, bool), &AskKeyframe),
) {
    let mut players: HashMap<String, Player> = HashMap::new();
    let mut stalls = Stalls::new();
    let mut speaking = Speaking::default();

    while !stop.load(Ordering::Relaxed) {
        match queue.recv_timeout(PATIENCE) {
            Ok(item) => match item.kind {
                MediaKind::Video { keyframe, timestamp } => play(
                    (&mut players, frames),
                    (&mut stalls, ask_keyframe.as_ref()),
                    &item.producer_id,
                    item.data,
                    Some((keyframe, timestamp)),
                ),
                MediaKind::Audio => {
                    if speaking.heard(&item.producer_id, &pcm(&item.data), Instant::now()) {
                        on_speaking(&item.producer_id, true);
                    }

                    play((&mut players, frames), (&mut stalls, ask_keyframe.as_ref()), &item.producer_id, item.data, None);
                }
            },
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        for producer in speaking.quiet(Instant::now()) {
            on_speaking(&producer, false);
        }

        if reopen_sound.swap(false, Ordering::Relaxed) {
            players.retain(|_, player| player.frame.is_some());
        }

        let now = Instant::now();
        let before = players.len();

        players.retain(|_, player| now.duration_since(player.last) < if player.frame.is_some() { SCREEN_IDLE } else { IDLE });

        if players.len() != before {
            let alive: Vec<String> = players.keys().cloned().collect();

            lock(frames).retain(|producer, _| alive.contains(producer));
        }
    }
}

/// Um bloco de uma transmissão para o tocador dela. `video` é `Some((keyframe, timestamp))` no
/// vídeo: o tocador só nasce num keyframe, e o que se perdeu na fila espera o próximo. Enquanto
/// a imagem espera, o quadro-chave é pedido ao servidor — na hora e de novo a cada segundo —,
/// em vez de ela ficar parada até o periódico; e a parada que passa de `WORTH_TELLING` vai para
/// o log com a duração.
fn play(
    (players, frames): (&mut HashMap<String, Player>, &Frames),
    (stalls, ask_keyframe): (&mut Stalls, &dyn Fn(&str)),
    producer: &str,
    data: Vec<u8>,
    video: Option<(bool, u32)>,
) {
    let now = Instant::now();
    let keyframe = video.is_some_and(|(keyframe, _)| keyframe);
    let waiting = |stalls: &mut Stalls| {
        if stalls.entry(producer.to_owned()).or_default().waiting(now) {
            ask_keyframe(producer);
        }
    };

    if !players.contains_key(producer) {
        if video.is_some() && !keyframe {
            waiting(stalls);

            return;
        }

        match open(video.is_some()) {
            Ok(player) => {
                if let Some(frame) = &player.frame {
                    lock(frames).insert(producer.to_owned(), Arc::clone(frame));
                }

                players.insert(producer.to_owned(), player);
            }
            Err(failure) => {
                tracing::warn!(failure = %format!("{failure:#}"), producer, "assistir: o gst-launch não abriu");

                return;
            }
        }
    }

    let Some(player) = players.get_mut(producer) else {
        return;
    };

    player.last = now;

    if player.waiting_keyframe && !keyframe {
        waiting(stalls);

        return;
    }

    let due = video.map(|(_, timestamp)| player.playout.due(timestamp, now));

    match player.feed.try_send((due, data)) {
        Ok(()) => {
            player.waiting_keyframe = false;

            if video.is_some()
                && let Some(lasted) = stalls.entry(producer.to_owned()).or_default().flowing(now)
                && lasted >= WORTH_TELLING
            {
                tracing::error!(producer, seconds = lasted.as_secs_f32(), "assistir: a imagem ficou parada esperando um quadro-chave");
            }
        }
        Err(TrySendError::Full(_)) => {
            if video.is_some() {
                if !player.waiting_keyframe {
                    tracing::warn!(producer, "assistir: o decodificador não acompanha, largando até o próximo quadro-chave");
                }

                player.waiting_keyframe = true;
                waiting(stalls);
            }
        }
        Err(TrySendError::Disconnected(_)) => {
            players.remove(producer);
        }
    }
}

fn open(video: bool) -> Result<Player> {
    let mut child = Command::new("gst-launch-1.0")
        .arg("-q")
        .args(pipeline(video).split_whitespace())
        .stdin(Stdio::piped())
        .stdout(if video { Stdio::piped() } else { Stdio::null() })
        .stderr(Stdio::null())
        .spawn()
        .context("gst-launch-1.0 não abriu; instale gstreamer1.0-tools e os plugins good/libav")?;
    let stdin = child.stdin.take().context("o gst-launch abriu sem entrada")?;
    let (feed, blocks) = sync_channel(if video { WAITING_VIDEO } else { WAITING_SOUND });

    write_blocks(stdin, blocks);

    let frame = match (video, child.stdout.take()) {
        (true, Some(stdout)) => {
            let frame = LatestFrame::default();

            read_frames(stdout, Arc::clone(&frame));

            Some(frame)
        }
        _ => None,
    };

    Ok(Player { child, feed, frame, waiting_keyframe: false, last: Instant::now(), playout: media::Playout::default() })
}

/// Um toque do app pela saída de som do PulseAudio — a que a pessoa escolheu, que o seletor
/// de som põe como padrão. O `pacat` vem no mesmo `pulseaudio-utils` do `pactl`.
pub fn chime(samples: Vec<f32>) {
    std::thread::spawn(move || {
        let child = std::process::Command::new("pacat")
            .args(["--raw", "--format=float32le", "--rate=48000", "--channels=2"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(failure) => {
                tracing::warn!(%failure, "som: o toque não tocou");

                return;
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();

            let _ = std::io::Write::write_all(&mut stdin, &bytes);
        }

        let _ = child.wait();
    });
}

/// O vídeo entra como H.264 Annex-B e sai RGB no tamanho do cartão; o som entra como PCM
/// `f32` estéreo a 48 kHz e vai direto para a saída do sistema. O `typefind` é o que dá
/// ao `h264parse` o tipo que a entrada padrão não traz.
fn pipeline(video: bool) -> String {
    let (width, height) = TILE;

    if video {
        return format!(
            "fdsrc fd=0 ! typefind ! h264parse ! avdec_h264 thread-type=slice ! videoconvert ! videoscale \
             ! video/x-raw,format=RGB,width={width},height={height},pixel-aspect-ratio=1/1 ! fdsink fd=1 sync=false"
        );
    }

    "fdsrc fd=0 do-timestamp=true ! audio/x-raw,format=F32LE,rate=48000,channels=2,layout=interleaved \
     ! audioconvert ! audioresample ! pulsesink sync=false buffer-time=40000 latency-time=10000"
        .to_owned()
}

/// A entrada do tocador numa thread só dela: um decodificador lento não trava a fila de
/// todo mundo, só enche a própria. O quadro com horário espera por ele; o que já passou da
/// hora entra na hora — é assim que quem ficou para trás alcança o presente, e a janela só
/// desenha o mais novo.
fn write_blocks(mut stdin: ChildStdin, blocks: Receiver<(Option<Instant>, Vec<u8>)>) {
    std::thread::spawn(move || {
        for (due, block) in blocks {
            if let Some(wait) = due.and_then(|due| due.checked_duration_since(Instant::now())) {
                std::thread::sleep(wait);
            }

            if stdin.write_all(&block).is_err() {
                break;
            }
        }
    });
}

/// Lê quadro por quadro. O tamanho é fixo (o pipeline força largura, altura e formato), o
/// que dispensa procurar marcador de fim: cada `FRAME_BYTES` é um quadro inteiro.
fn read_frames(mut stdout: impl Read + Send + 'static, frame: LatestFrame) {
    std::thread::spawn(move || {
        loop {
            let mut pixels = vec![0_u8; FRAME_BYTES];

            if stdout.read_exact(&mut pixels).is_err() {
                break;
            }

            *lock(&frame) = Some(pixels);
        }
    });
}

/// O som chega como bytes de `f32` little-endian, estéreo intercalado.
fn pcm(bytes: &[u8]) -> Vec<f32> {
    bytes.as_chunks::<4>().0.iter().map(|sample| f32::from_le_bytes(*sample)).collect()
}

fn lock<T>(cell: &Mutex<T>) -> MutexGuard<'_, T> {
    cell.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_only_handed_over_once() {
        let frame: LatestFrame = Arc::default();
        let (reader, mut writer) = std::io::pipe().expect("um cano");

        read_frames(reader, Arc::clone(&frame));

        writer.write_all(&vec![7_u8; FRAME_BYTES]).expect("um quadro");

        let deadline = Instant::now() + Duration::from_secs(5);

        while lock(&frame).is_none() {
            assert!(Instant::now() < deadline, "o quadro nunca chegou");
            std::thread::sleep(Duration::from_millis(10));
        }

        let taken = lock(&frame).take().expect("um quadro inteiro");

        assert_eq!(taken.len(), FRAME_BYTES);
        assert!(lock(&frame).is_none(), "o mesmo quadro sairia duas vezes");
    }

    #[test]
    fn the_video_pipeline_hands_the_window_raw_pixels_of_a_known_size() {
        let video = pipeline(true);

        assert!(video.contains("format=RGB,width=1280,height=720"), "{video}");
        assert!(video.starts_with("fdsrc fd=0 ! typefind ! h264parse"), "{video}");
        assert!(!video.contains("jpeg"), "recodificar aqui é trabalho que ninguém pediu");

        // O som não passa pela janela: vai do GStreamer para a saída do sistema.
        assert!(pipeline(false).contains("pulsesink"));
    }

    /// O quadro P sem tocador não abre nada e pede o quadro-chave — uma vez, não um pedido por
    /// quadro: o próximo só depois de um segundo.
    #[test]
    fn a_screen_only_opens_its_player_at_a_keyframe_and_asks_for_one() {
        let (mut players, frames, mut stalls) = (HashMap::new(), Frames::default(), HashMap::new());
        let asked = std::cell::Cell::new(0);
        let ask = |_: &str| asked.set(asked.get() + 1);

        play((&mut players, &frames), (&mut stalls, &ask), "tela", vec![0, 0, 0, 1, 0x09], Some((false, 0)));
        play((&mut players, &frames), (&mut stalls, &ask), "tela", vec![0, 0, 0, 1, 0x09], Some((false, 3_000)));

        assert!(players.is_empty(), "um quadro P abriu tocador");
        assert_eq!(asked.get(), 1, "o quadro-chave não foi pedido, ou foi pedido a cada quadro");
    }

    /// Contra o GStreamer de verdade: o arquivo de teste do `media` entra pela entrada padrão
    /// e sai como seis quadros do tamanho do cartão.
    #[test]
    #[ignore]
    fn a_real_stream_comes_out_as_tile_sized_frames() {
        let stream = include_bytes!("../../../shared/media/tests/fixtures/testsrc-320x240.h264");
        let player = open(true).expect("o gst-launch abriu");
        let frame = player.frame.clone().expect("vídeo tem quadro");

        player.feed.send((None, stream.to_vec())).expect("entrou");

        let deadline = Instant::now() + Duration::from_secs(10);

        while lock(&frame).is_none() {
            assert!(Instant::now() < deadline, "nenhum quadro saiu");
            std::thread::sleep(Duration::from_millis(20));
        }

        assert_eq!(lock(&frame).as_ref().map(Vec::len), Some(FRAME_BYTES));
    }
}
