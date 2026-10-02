//! Assistir no Windows: a fila do núcleo esvaziada numa thread que só distribui — o som vai
//! direto para o alto-falante e cada tela vai para uma thread própria, com o decodificador
//! dela. É o `MediaRouter` do macOS escrito em Rust.
//!
//! Nada aqui passa pela thread da janela: sessenta quadros por segundo decodificados nela
//! travariam a interface. A janela só busca, no tique dela, o último quadro de cada tela.
//!
//! Antes era uma thread só para tudo, e cada quadro custava ~15 ms (decodificar e converter
//! para RGB, na CPU). Duas telas 1080p60 pedem 120 quadros por segundo: medido em 01/10 na
//! sala de produção, a fila do núcleo enchia em quatro segundos, a imagem ficava quatro
//! segundos atrás e o que chegava depois era largado — tela travada até o próximo keyframe.
//! O som, na mesma fila, atrasava junto.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use core_app::speaking::Speaking;
use core_app::watching::{Media, MediaKind};
use slint::{Rgb8Pixel, SharedPixelBuffer};

use crate::sound::Speaker;

/// De quanto em quanto tempo a thread olha se mandaram parar, e se alguém calou.
const PATIENCE: Duration = Duration::from_millis(100);

/// Quadros de uma tela esperando a thread dela: dois segundos a 60 fps. Com a conversão
/// pulada nos quadros que não vão aparecer, ela só fica para trás se nem decodificar der
/// conta; aí o quadro é largado e o keyframe é pedido na hora.
const SCREEN_QUEUE: usize = 120;

/// O quadro mais novo de cada tela que a janela ainda não desenhou.
type Fresh = Arc<Mutex<HashMap<String, SharedPixelBuffer<Rgb8Pixel>>>>;

/// Quantos quadros cada tela decodificou desde a última pergunta, e a altura do último.
type Drawn = Arc<Mutex<HashMap<String, (u32, u32)>>>;

/// O aviso de quadro novo, dividido entre as threads das telas.
type OnFrame = Arc<Mutex<Box<dyn Fn() + Send>>>;

/// O pedido de keyframe ao servidor, para a tela que quebrou do lado de cá.
type AskKeyframe = Arc<dyn Fn(&str) + Send + Sync>;

pub struct Watch {
    fresh: Fresh,
    drawn: Drawn,
    speaker: Arc<Speaker>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Watch {
    /// `on_speaking` recebe o producer e se ele começou (`true`) ou parou de falar.
    /// `on_frame` avisa que há quadro novo. É aviso e não relógio: arrastar a janela no
    /// Windows prende o laço de eventos num laço modal do sistema, e relógio nenhum bate
    /// ali — a fila de eventos, sim, continua sendo despachada.
    /// `ask_keyframe` pede ao servidor um keyframe da transmissão cuja tela quebrou aqui.
    pub fn start(
        queue: Receiver<Media>,
        speaker: Arc<Speaker>,
        on_speaking: impl Fn(&str, bool) + Send + 'static,
        on_frame: impl Fn() + Send + 'static,
        ask_keyframe: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        let (fresh, drawn, stop) = (Fresh::default(), Drawn::default(), Arc::new(AtomicBool::new(false)));
        let on_frame: OnFrame = Arc::new(Mutex::new(Box::new(on_frame)));
        let ask_keyframe: AskKeyframe = Arc::new(ask_keyframe);
        let thread = std::thread::Builder::new()
            .name("unkvoid-assistir".into())
            .spawn({
                let (fresh, drawn, speaker, stop) = (fresh.clone(), drawn.clone(), speaker.clone(), stop.clone());

                move || route(&queue, (&fresh, &drawn), &speaker, &stop, (&on_speaking, &on_frame, &ask_keyframe))
            })
            .ok();

        Self { fresh, drawn, speaker, stop, thread }
    }

    /// Quadros decodificados de cada tela desde a última pergunta, e a altura do último.
    /// Perguntado de segundo em segundo, é o fps.
    pub fn drawn(&self) -> HashMap<String, (u32, u32)> {
        std::mem::take(&mut *lock(&self.drawn))
    }

    /// O último quadro de cada tela desde a última pergunta. Quadro que a janela não chegou
    /// a desenhar é substituído pelo mais novo: atrasar a imagem para mostrar tudo é pior.
    pub fn fresh(&self) -> Vec<(String, SharedPixelBuffer<Rgb8Pixel>)> {
        lock(&self.fresh).drain().collect()
    }

    pub fn speaker(&self) -> &Arc<Speaker> {
        &self.speaker
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
    (fresh, drawn): (&Fresh, &Drawn),
    speaker: &Speaker,
    stop: &Arc<AtomicBool>,
    (on_speaking, on_frame, ask_keyframe): (&impl Fn(&str, bool), &OnFrame, &AskKeyframe),
) {
    let mut screens: HashMap<String, Screen> = HashMap::new();
    let mut speaking = Speaking::default();

    while !stop.load(Ordering::Relaxed) {
        match queue.recv_timeout(PATIENCE) {
            Ok(item) => match item.kind {
                MediaKind::Video { keyframe, .. } => {
                    let screen = match screens.entry(item.producer_id.clone()) {
                        Entry::Occupied(entry) => entry.into_mut(),
                        Entry::Vacant(entry) => match Screen::start(entry.key(), (fresh, drawn), stop, (on_frame, ask_keyframe)) {
                            Some(screen) => entry.insert(screen),
                            None => continue,
                        },
                    };

                    screen.push(item, keyframe);
                }
                MediaKind::Audio => {
                    let samples = pcm(&item.data);

                    speaker.play(&item.producer_id, &samples);

                    if speaking.heard(&item.producer_id, &samples, Instant::now()) {
                        on_speaking(&item.producer_id, true);
                    }
                }
            },
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        for producer in speaking.quiet(Instant::now()) {
            on_speaking(&producer, false);
        }
    }
}

/// A fila de uma tela e a thread que a decodifica.
struct Screen {
    producer: String,
    frames: Option<SyncSender<Media>>,
    thread: Option<JoinHandle<()>>,
    /// Largou um quadro: até o próximo keyframe nada entra, porque quadro P depois de um
    /// buraco só desenharia lixo. O keyframe é pedido na hora em que quebra.
    broken: bool,
    ask_keyframe: AskKeyframe,
}

impl Screen {
    fn start(producer: &str, (fresh, drawn): (&Fresh, &Drawn), stop: &Arc<AtomicBool>, (on_frame, ask_keyframe): (&OnFrame, &AskKeyframe)) -> Option<Self> {
        let (frames, queue) = sync_channel(SCREEN_QUEUE);
        let thread = std::thread::Builder::new().name("unkvoid-tela".into()).spawn({
            let (producer, fresh, drawn, stop, on_frame, ask_keyframe) =
                (producer.to_owned(), fresh.clone(), drawn.clone(), stop.clone(), on_frame.clone(), ask_keyframe.clone());

            move || decode_screen(&producer, &queue, (&fresh, &drawn), &stop, (&on_frame, &ask_keyframe))
        });

        match thread {
            Ok(thread) => Some(Self {
                producer: producer.to_owned(),
                frames: Some(frames),
                thread: Some(thread),
                broken: false,
                ask_keyframe: ask_keyframe.clone(),
            }),
            Err(failure) => {
                tracing::warn!(%failure, producer, "assistir: a thread da tela não abriu");

                None
            }
        }
    }

    fn push(&mut self, item: Media, keyframe: bool) {
        if self.broken && !keyframe {
            return;
        }

        let Some(frames) = &self.frames else {
            return;
        };

        let broken = match frames.try_send(item) {
            Ok(()) => false,
            Err(TrySendError::Full(_)) => {
                if !self.broken {
                    tracing::warn!(producer = self.producer, "assistir: a tela não acompanha, largando até o próximo keyframe");
                    (self.ask_keyframe)(&self.producer);
                }

                true
            }
            Err(TrySendError::Disconnected(_)) => true,
        };

        self.broken = broken;
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        drop(self.frames.take());

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A thread de uma tela: decodifica tudo o que chegou e converte para imagem só o mais novo.
/// Quem acompanha recebe um quadro por vez e converte todos; quem ficou para trás junta
/// vários na fila, decodifica-os sem converter e alcança o presente.
fn decode_screen(producer: &str, queue: &Receiver<Media>, (fresh, drawn): (&Fresh, &Drawn), stop: &AtomicBool, (on_frame, ask_keyframe): (&OnFrame, &AskKeyframe)) {
    let mut decoder = None;
    let mut batch = Vec::new();

    while !stop.load(Ordering::Relaxed) {
        match queue.recv_timeout(PATIENCE) {
            Ok(item) => batch.push(item),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        }

        batch.extend(queue.try_iter());

        let newest = batch.len() - 1;

        for (index, item) in batch.drain(..).enumerate() {
            let MediaKind::Video { keyframe, timestamp } = item.kind else {
                continue;
            };

            let open = decoder.is_some();
            let shown = show(&mut decoder, fresh, producer, &item.data, (keyframe, timestamp), index == newest);

            if open && decoder.is_none() {
                ask_keyframe(producer);
            }

            let Some(height) = shown else {
                continue;
            };

            let mut drawn = lock(drawn);
            let counted = drawn.entry(producer.to_owned()).or_default();

            *counted = (counted.0 + 1, height);
            drop(drawn);
            (lock(on_frame))();
        }
    }
}

/// Um quadro de uma tela. O decodificador só nasce num keyframe: quadro P sem o I de antes
/// só desenharia lixo. Se ele falhar, morre e renasce no próximo keyframe. Devolve a altura
/// do quadro que virou imagem — com `convert` falso, nenhum vira.
fn show(
    decoder: &mut Option<media::H264Decoder>,
    fresh: &Fresh,
    producer: &str,
    data: &[u8],
    (keyframe, timestamp): (bool, u32),
    convert: bool,
) -> Option<u32> {
    if decoder.is_none() {
        if !keyframe {
            return None;
        }

        match media::H264Decoder::new() {
            Ok(opened) => *decoder = Some(opened),
            Err(failure) => {
                tracing::warn!(%failure, producer, "assistir: o decodificador não abriu");

                return None;
            }
        }
    }

    let opened = decoder.as_mut()?;
    let decoded = if convert { opened.decode(data, timestamp).map(|frames| frames.into_iter().last()) } else { opened.skip(data, timestamp).map(|()| None) };

    match decoded {
        Ok(frame) => {
            let frame = frame?;
            let buffer = SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(&frame.rgb, frame.width, frame.height);

            lock(fresh).insert(producer.to_owned(), buffer);

            Some(frame.height)
        }
        Err(failure) => {
            tracing::warn!(failure = %format!("{failure:#}"), producer, "assistir: quadro recusado, esperando o próximo keyframe");
            *decoder = None;

            None
        }
    }
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
    fn the_sound_bytes_become_the_samples_they_were() {
        let samples = [0.5_f32, -0.25, 1.0, 0.0];
        let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();

        assert_eq!(pcm(&bytes), samples);
    }

    #[test]
    fn a_torn_last_sample_is_left_out() {
        let mut bytes = 0.5_f32.to_le_bytes().to_vec();

        bytes.push(7);

        assert_eq!(pcm(&bytes), [0.5]);
    }

    /// Contra a pilha no ar e alguém transmitindo na sala: prova que o Windows assiste — o
    /// quadro chega pelo `Room`, o Media Foundation decodifica e ele vira imagem para a janela.
    /// Sem janela nenhuma, então roda com quem estiver na frente do computador jogando:
    ///
    /// `UNKVOID_ROOM=<código> cargo test -p unkvoid-windows a_live_screen -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn a_live_screen_becomes_images_for_the_window() {
        let code = std::env::var("UNKVOID_ROOM").expect("UNKVOID_ROOM com o código de uma sala transmitindo");
        let url = std::env::var("UNKVOID_SFU").unwrap_or_else(|_| "ws://127.0.0.1:3000/sfu".into());
        let runtime = tokio::runtime::Runtime::new().expect("o tokio subiu");
        let identity: core_app::Identity = Arc::new({
            let code = code.clone();

            move || {
                let identity = core_app::models::RoomIdentity::Guest {
                    room: code.clone(),
                    name: "teste do windows".into(),
                    install_id: "teste-do-windows".into(),
                };

                Box::pin(async move { Ok(identity) }) as _
            }
        });
        let (updates, _heard) = std::sync::mpsc::channel();
        let (room, media) = runtime
            .block_on(core_app::room::Room::enter(&url, &code, identity, updates))
            .expect("entrou na sala");
        let watch = Watch::start(media, Arc::new(Speaker::start(None)), |_, _| {}, || {}, |_| {});
        let (mut frames, mut size) = (0_u32, (0, 0));
        let until = Instant::now() + Duration::from_secs(10);

        while Instant::now() < until {
            for (_, buffer) in watch.fresh() {
                frames += 1;
                size = (buffer.width(), buffer.height());
            }

            std::thread::sleep(Duration::from_millis(16));
        }

        runtime.block_on(room.leave());
        println!("{frames} imagens prontas em 10 s, de {}x{}", size.0, size.1);

        assert!(frames > 100, "só {frames} imagens em 10 s");
        assert!(size.0 >= 640 && size.1 >= 360, "a imagem saiu {size:?}");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn a_screen_only_starts_drawing_at_a_keyframe() {
        let mut decoder = None;
        let fresh = Fresh::default();

        let drawn = show(&mut decoder, &fresh, "tela", &[0, 0, 0, 1, 0x09, 0x10], (false, 0), true);

        assert!(drawn.is_none());

        assert!(decoder.is_none(), "um quadro P abriu decodificador");
        assert!(lock(&fresh).is_empty());
    }
}
