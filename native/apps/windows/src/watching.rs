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

use std::collections::{HashMap, VecDeque};
use std::collections::hash_map::Entry;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use core_app::speaking::Speaking;
use core_app::watching::{Media, MediaKind, Stalled, WORTH_TELLING};
use slint::{Rgba8Pixel, SharedPixelBuffer};

use crate::sound::Speaker;

/// De quanto em quanto tempo a thread olha se mandaram parar, e se alguém calou.
const PATIENCE: Duration = Duration::from_millis(100);

/// Quadros de uma tela esperando a thread dela: dois segundos a 60 fps. Com a conversão
/// pulada nos quadros que não vão aparecer, ela só fica para trás se nem decodificar der
/// conta; aí o quadro é largado e o keyframe é pedido na hora.
const SCREEN_QUEUE: usize = 120;

/// Quadros esperando o horário deles: meio segundo a 60 fps, a espera mais longa do `Playout`.
/// Esperam ainda comprimidos — KB cada, e não os 8 MB de uma imagem 1080p (33 MB em 4K) que
/// esperavam antes, até 750 MB por tela 4K numa rede com perda.
const MOST_WAITING: usize = 30;

/// Tela sem quadro por isto sai, com a thread, o decodificador e as imagens dela: o producer
/// fechou, ou quem transmite está com a tela parada — aí ela renasce no próximo quadro, que
/// pede o quadro-chave.
const SCREEN_IDLE: Duration = Duration::from_secs(20);

/// O quadro mais novo de cada tela que a janela ainda não desenhou.
type Fresh = Arc<Mutex<HashMap<String, SharedPixelBuffer<Rgba8Pixel>>>>;

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
    pub fn fresh(&self) -> Vec<(String, SharedPixelBuffer<Rgba8Pixel>)> {
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

        screens.retain(|producer, screen| {
            let keep = screen.frames.is_some() && screen.last.elapsed() < SCREEN_IDLE;

            if !keep {
                lock(fresh).remove(producer);
                lock(drawn).remove(producer);
            }

            keep
        });
    }
}

/// A fila de uma tela e a thread que a decodifica.
struct Screen {
    producer: String,
    frames: Option<SyncSender<Media>>,
    thread: Option<JoinHandle<()>>,
    /// Largou um quadro: até o próximo keyframe nada entra, porque quadro P depois de um
    /// buraco só desenharia lixo. O keyframe é pedido na hora em que quebra, e de novo a cada
    /// segundo enquanto não vem.
    broken: bool,
    stalled: Stalled,
    /// Os quadros largados desde que quebrou, para o log dizer o tamanho do estrago.
    dropped: u32,
    /// O último quadro que chegou.
    last: Instant,
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
                stalled: Stalled::default(),
                dropped: 0,
                last: Instant::now(),
                ask_keyframe: ask_keyframe.clone(),
            }),
            Err(failure) => {
                tracing::warn!(%failure, producer, "assistir: a thread da tela não abriu");

                None
            }
        }
    }

    fn push(&mut self, item: Media, keyframe: bool) {
        let now = Instant::now();

        self.last = now;

        let Some(frames) = &self.frames else {
            return;
        };

        let sent = if self.broken && !keyframe { Err(TrySendError::Full(item)) } else { frames.try_send(item) };

        match sent {
            Ok(()) => {
                if let Some(lasted) = self.stalled.flowing(now) {
                    tracing::error!(producer = self.producer, seconds = lasted.as_secs_f32(), dropped = self.dropped, "assistir: este PC não acompanhou a tela e largou quadros até o quadro-chave");
                }

                self.broken = false;
                self.dropped = 0;
            }
            Err(TrySendError::Full(_)) => {
                if !self.broken {
                    tracing::warn!(producer = self.producer, "assistir: a tela não acompanha, largando até o próximo keyframe");
                }

                self.broken = true;
                self.dropped += 1;

                if self.stalled.waiting(now) {
                    (self.ask_keyframe)(&self.producer);
                }
            }
            // A thread da tela morreu: a tela sai, e renasce no próximo quadro.
            Err(TrySendError::Disconnected(_)) => self.frames = None,
        }
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

/// Um quadro que chegou, ainda comprimido, com o horário de aparecer.
struct Pending {
    due: Instant,
    keyframe: bool,
    timestamp: u32,
    data: Vec<u8>,
}

/// A thread de uma tela: guarda cada quadro com o horário dele, que o `Playout` tira do relógio
/// do RTP de quem transmite — é o "jitter buffer" do navegador: um bolo de quadros segurado por
/// um reenvio sai espaçado, e não de uma vez. Na hora, os que venceram passam pelo
/// decodificador na ordem e só o último vira imagem: é assim que quem ficou para trás alcança o
/// presente.
fn decode_screen(producer: &str, queue: &Receiver<Media>, (fresh, drawn): (&Fresh, &Drawn), stop: &AtomicBool, (on_frame, ask_keyframe): (&OnFrame, &AskKeyframe)) {
    let _timer = FineTimer::start();
    let mut decoder = None;
    let mut playout = media::Playout::default();
    let mut waiting: VecDeque<Pending> = VecDeque::new();
    // Quadro chegando e decodificador fechado: a espera do quadro-chave, que é pedido na hora e
    // de novo a cada segundo. Inclui a primeira imagem, que assim não espera o GOP de 4 s.
    let mut stalled = Stalled::default();

    while !stop.load(Ordering::Relaxed) {
        let wait = waiting
            .front()
            .map_or(PATIENCE, |pending| pending.due.saturating_duration_since(Instant::now()).min(PATIENCE));
        let first = match queue.recv_timeout(wait) {
            Ok(item) => Some(item),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let mut arrived = false;

        for item in first.into_iter().chain(queue.try_iter()) {
            let MediaKind::Video { keyframe, timestamp } = item.kind else {
                continue;
            };

            arrived = true;
            waiting.push_back(Pending { due: playout.due(timestamp, Instant::now()), keyframe, timestamp, data: item.data });
        }

        // Atrás demais: os mais velhos passam pelo decodificador sem virar imagem, porque todo
        // quadro P precisa do anterior.
        while waiting.len() > MOST_WAITING {
            if let Some(pending) = waiting.pop_front() {
                show(&mut decoder, producer, &pending, false);
            }
        }

        let now = Instant::now();
        let ready = waiting.iter().take_while(|pending| pending.due <= now).count();
        let mut image = None;

        for index in 0..ready {
            let Some(pending) = waiting.pop_front() else {
                break;
            };

            image = show(&mut decoder, producer, &pending, index + 1 == ready);
        }

        if arrived && decoder.is_none() && !waiting.iter().any(|pending| pending.keyframe) && stalled.waiting(now) {
            ask_keyframe(producer);
        }

        let Some(image) = image else {
            continue;
        };

        if let Some(lasted) = stalled.flowing(now)
            && lasted >= WORTH_TELLING
        {
            tracing::error!(producer, seconds = lasted.as_secs_f32(), "assistir: a imagem ficou parada aqui, o decodificador recusou um quadro e esperou o quadro-chave");
        }

        let height = image.height();

        lock(fresh).insert(producer.to_owned(), image);

        let mut drawn = lock(drawn);
        let counted = drawn.entry(producer.to_owned()).or_default();

        *counted = (counted.0 + 1, height);
        drop(drawn);
        (lock(on_frame))();
    }
}

/// O relógio do Windows acorda de 15,6 em 15,6 ms por padrão, mais que um quadro a 60 fps: os
/// horários do `Playout` cairiam em pares e a imagem pularia. Enquanto alguma tela está aberta,
/// ele acorda de milissegundo em milissegundo, como faz o navegador tocando vídeo.
struct FineTimer;

impl FineTimer {
    fn start() -> Self {
        #[cfg(target_os = "windows")]
        unsafe {
            windows::Win32::Media::timeBeginPeriod(1);
        }

        Self
    }
}

impl Drop for FineTimer {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        unsafe {
            windows::Win32::Media::timeEndPeriod(1);
        }
    }
}

/// Um quadro de uma tela. O decodificador só nasce num keyframe: quadro P sem o I de antes
/// só desenharia lixo. Se ele falhar, morre e renasce no próximo keyframe. Devolve a imagem
/// do quadro, em RGBA escrito direto nela — com `convert` falso, ele passa pelo decodificador e
/// nenhuma imagem sai.
fn show(decoder: &mut Option<media::H264Decoder>, producer: &str, pending: &Pending, convert: bool) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    if decoder.is_none() {
        if !pending.keyframe {
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
    let decoded = if convert {
        let mut image = None::<SharedPixelBuffer<Rgba8Pixel>>;
        let drawn = opened.decode_into(&pending.data, pending.timestamp, |width, height| {
            image.insert(SharedPixelBuffer::new(width, height)).make_mut_bytes()
        });

        drawn.map(|drawn| image.filter(|_| drawn))
    } else {
        opened.skip(&pending.data, pending.timestamp).map(|()| None)
    };

    match decoded {
        Ok(image) => image,
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

        // O que a thread da tela pôs na tela, contado por ela: a espera de 16 ms acima vira 31 num
        // processo sem janela no Windows 11, e a pergunta da janela sozinha mediria o relógio.
        let shown: u32 = watch.drawn().values().map(|(count, _)| count).sum();

        runtime.block_on(room.leave());
        println!("{frames} imagens prontas em 10 s, {shown} postas na tela pela thread, de {}x{}", size.0, size.1);

        assert!(frames > 100, "só {frames} imagens em 10 s");
        assert!(size.0 >= 640 && size.1 >= 360, "a imagem saiu {size:?}");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn a_screen_only_starts_drawing_at_a_keyframe() {
        let mut decoder = None;
        let pending = Pending { due: Instant::now(), keyframe: false, timestamp: 0, data: vec![0, 0, 0, 1, 0x09, 0x10] };
        let drawn = show(&mut decoder, "tela", &pending, true);

        assert!(drawn.is_none());

        assert!(decoder.is_none(), "um quadro P abriu decodificador");
    }
}
