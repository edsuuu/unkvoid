//! Assistir no Windows: a fila do núcleo esvaziada numa thread só, o vídeo indo para um
//! decodificador por transmissão e o som indo para o alto-falante. É o `MediaRouter` do
//! macOS escrito em Rust.
//!
//! Nada aqui passa pela thread da janela: sessenta quadros por segundo decodificados nela
//! travariam a interface. A janela só busca, no tique dela, o último quadro de cada tela.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use core_app::speaking::Speaking;
use core_app::watching::{Media, MediaKind};
use slint::{Rgb8Pixel, SharedPixelBuffer};

use crate::sound::Speaker;

/// De quanto em quanto tempo a thread olha se mandaram parar, e se alguém calou.
const PATIENCE: Duration = Duration::from_millis(100);

/// O quadro mais novo de cada tela que a janela ainda não desenhou.
type Fresh = Arc<Mutex<HashMap<String, SharedPixelBuffer<Rgb8Pixel>>>>;

/// Quantos quadros cada tela decodificou desde a última pergunta, e a altura do último.
type Drawn = Arc<Mutex<HashMap<String, (u32, u32)>>>;

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
    pub fn start(
        queue: Receiver<Media>,
        speaker: Arc<Speaker>,
        on_speaking: impl Fn(&str, bool) + Send + 'static,
        on_frame: impl Fn() + Send + 'static,
    ) -> Self {
        let (fresh, drawn, stop) = (Fresh::default(), Drawn::default(), Arc::new(AtomicBool::new(false)));
        let thread = std::thread::Builder::new()
            .name("unkvoid-assistir".into())
            .spawn({
                let (fresh, drawn, speaker, stop) = (fresh.clone(), drawn.clone(), speaker.clone(), stop.clone());

                move || route(&queue, (&fresh, &drawn), &speaker, &stop, (&on_speaking, &on_frame))
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
    stop: &AtomicBool,
    (on_speaking, on_frame): (&impl Fn(&str, bool), &impl Fn()),
) {
    let mut screens: HashMap<String, media::H264Decoder> = HashMap::new();
    let mut speaking = Speaking::default();

    while !stop.load(Ordering::Relaxed) {
        match queue.recv_timeout(PATIENCE) {
            Ok(item) => match item.kind {
                MediaKind::Video { keyframe, timestamp } => {
                    if let Some(height) = show(&mut screens, fresh, &item.producer_id, &item.data, (keyframe, timestamp)) {
                        let mut drawn = lock(drawn);
                        let counted = drawn.entry(item.producer_id.clone()).or_default();

                        *counted = (counted.0 + 1, height);
                        drop(drawn);
                        on_frame();
                    }
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

/// Um quadro de uma tela. O decodificador só nasce num keyframe: quadro P sem o I de antes
/// só desenharia lixo. Se ele falhar, morre e renasce no próximo keyframe. Devolve a altura
/// do quadro que ficou pronto.
fn show(
    screens: &mut HashMap<String, media::H264Decoder>,
    fresh: &Fresh,
    producer: &str,
    data: &[u8],
    (keyframe, timestamp): (bool, u32),
) -> Option<u32> {
    if !screens.contains_key(producer) {
        if !keyframe {
            return None;
        }

        match media::H264Decoder::new() {
            Ok(decoder) => {
                screens.insert(producer.to_owned(), decoder);
            }
            Err(failure) => {
                tracing::warn!(%failure, producer, "assistir: o decodificador não abriu");

                return None;
            }
        }
    }

    let decoder = screens.get_mut(producer)?;

    match decoder.decode(data, timestamp) {
        Ok(frames) => {
            let frame = frames.into_iter().last()?;
            let buffer = SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(&frame.rgb, frame.width, frame.height);

            lock(fresh).insert(producer.to_owned(), buffer);

            Some(frame.height)
        }
        Err(failure) => {
            tracing::warn!(failure = %format!("{failure:#}"), producer, "assistir: quadro recusado, esperando o próximo keyframe");
            screens.remove(producer);

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
        let watch = Watch::start(media, Arc::new(Speaker::start(None)), |_, _| {}, || {});
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
        let mut screens = HashMap::new();
        let fresh = Fresh::default();

        let drawn = show(&mut screens, &fresh, "tela", &[0, 0, 0, 1, 0x09, 0x10], (false, 0));

        assert!(drawn.is_none());

        assert!(screens.is_empty(), "um quadro P abriu decodificador");
        assert!(lock(&fresh).is_empty());
    }
}
