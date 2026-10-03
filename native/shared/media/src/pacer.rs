//! O vídeo sobe espalhado no tempo, e não de uma vez.
//!
//! Um quadro-chave de 1080p a 10 Mb/s são uns 300 KB — duzentos e tantos pacotes — e eles
//! saíam na velocidade da placa de rede, todos juntos. O roteador de casa segura uma fila
//! curta na subida, e o que não cabe nela some: o servidor mediu 16% de perda assim em
//! 30/09, e cada pacote perdido é um quadro que quem assiste não monta. O WebRTC do
//! navegador, que o app em React usava, solta o vídeo a 2,5× a taxa dele: o quadro-chave
//! atravessa em ~100 ms sem transbordar a fila. Aqui é o mesmo.
//!
//! O vídeo e os reenvios passam por aqui; o áudio sai na hora — é pouco e pequeno, e esperar
//! atrás de um quadro-chave custaria mais do que o espaço que ele ocupa. O reenvio vai na
//! frente da fila, mas no mesmo ritmo: o NACK de um quadro-chave perdido pede de 100 a 200
//! pacotes, e saindo todos de uma vez estouravam de novo a fila do roteador de quem tem upload
//! fraco — perda de novo, quadro-chave de novo, em ciclo.

use std::collections::VecDeque;
use std::io::ErrorKind;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bytes::Bytes;

/// O ritmo antes de alguém dizer a taxa do vídeo: 2,5× os 10 Mb/s da qualidade padrão.
const STARTING_RATE: u64 = 25_000_000;

/// O ritmo nunca fica abaixo disto, em bits por segundo: com o governador no piso de uma
/// qualidade baixa, 2,5× daria um ritmo que o próprio pico do VBR passaria.
const FLOOR: u64 = 4_000_000;

/// O que pode sair de uma vez com a fila parada. Um quadro comum a 10 Mb/s tem ~20 KB e sai
/// sem esperar nada; só o que passa disso — o quadro-chave — é espalhado.
const BURST: Duration = Duration::from_millis(20);

/// O menor fôlego de uma vez, em bytes: dez pacotes cheios.
const MOST_BURST_FLOOR: f64 = 12_000.0;

/// A fila mais longa, em pacotes. Com o ritmo acima da taxa do vídeo ela só enche num
/// quadro-chave; passar disto é o uplink que não leva nem o vídeo, e segurar mais só
/// atrasaria a imagem de quem assiste. O mais velho sai, contado como largado.
const MOST_QUEUED: usize = 4_096;

/// Os reenvios esperando a vez: o histórico do remetente inteiro, que é o que o servidor pode
/// pedir de volta.
const MOST_REPAIRS: usize = 1_024;

/// O balde do ritmo: enche com o tempo, na taxa pedida, até o fôlego de uma vez.
#[derive(Debug)]
struct Bucket {
    budget: f64,
    last: Instant,
}

impl Bucket {
    fn new(now: Instant) -> Self {
        Self { budget: MOST_BURST_FLOOR, last: now }
    }

    /// `None` quando o pacote pode sair agora (e já sai do balde); senão, quanto falta.
    fn take(&mut self, size: usize, rate: u64, now: Instant) -> Option<Duration> {
        let bytes_per_second = rate as f64 / 8.0;
        let most = (bytes_per_second * BURST.as_secs_f64()).max(MOST_BURST_FLOOR);

        self.budget = (self.budget + bytes_per_second * now.saturating_duration_since(self.last).as_secs_f64()).min(most);
        self.last = now;

        let size = size as f64;

        if self.budget >= size {
            self.budget -= size;

            return None;
        }

        Some(Duration::from_secs_f64((size - self.budget) / bytes_per_second))
    }
}

#[derive(Default)]
struct Queue {
    /// Saem antes dos `packets`: quem assiste já está parado esperando por eles.
    repairs: VecDeque<Bytes>,
    packets: VecDeque<Bytes>,
    closed: bool,
}

impl Queue {
    fn front(&self) -> Option<&Bytes> {
        self.repairs.front().or_else(|| self.packets.front())
    }

    fn pop_front(&mut self) -> Option<Bytes> {
        self.repairs.pop_front().or_else(|| self.packets.pop_front())
    }
}

struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
    rate: AtomicU64,
    sent_bytes: AtomicU64,
    dropped: AtomicU64,
}

pub(crate) struct Pacer {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Pacer {
    pub(crate) fn start(socket: UdpSocket) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            queue: Mutex::default(),
            ready: Condvar::new(),
            rate: AtomicU64::new(STARTING_RATE),
            sent_bytes: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
        });
        let thread = std::thread::Builder::new().name("unkvoid-ritmo".into()).spawn({
            let shared = Arc::clone(&shared);

            move || run(&shared, &socket)
        })?;

        Ok(Self { shared, thread: Some(thread) })
    }

    pub(crate) fn push(&self, packet: Bytes) {
        let mut queue = lock(&self.shared.queue);

        if queue.packets.len() == MOST_QUEUED {
            queue.packets.pop_front();
            self.shared.dropped.fetch_add(1, Ordering::Relaxed);
        }

        queue.packets.push_back(packet);
        drop(queue);
        self.shared.ready.notify_one();
    }

    /// Um pacote pedido de volta pelo servidor: sai antes do vídeo novo.
    pub(crate) fn push_repair(&self, packet: Bytes) {
        let mut queue = lock(&self.shared.queue);

        if queue.repairs.len() == MOST_REPAIRS {
            queue.repairs.pop_front();
            self.shared.dropped.fetch_add(1, Ordering::Relaxed);
        }

        queue.repairs.push_back(packet);
        drop(queue);
        self.shared.ready.notify_one();
    }

    /// A taxa do vídeo, que o governador decide: o ritmo anda 2,5× à frente dela.
    pub(crate) fn follow(&self, video_bitrate: u64) {
        self.shared.rate.store((video_bitrate * 5 / 2).max(FLOOR), Ordering::Relaxed);
    }

    pub(crate) fn sent_bytes(&self) -> u64 {
        self.shared.sent_bytes.load(Ordering::Relaxed)
    }

    pub(crate) fn dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Relaxed)
    }
}

impl Drop for Pacer {
    fn drop(&mut self) {
        lock(&self.shared.queue).closed = true;
        self.shared.ready.notify_one();

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(shared: &Shared, socket: &UdpSocket) {
    let mut bucket = Bucket::new(Instant::now());

    loop {
        let packet = {
            let mut queue = lock(&shared.queue);

            loop {
                if queue.closed {
                    return;
                }

                let Some(front) = queue.front() else {
                    queue = shared.ready.wait(queue).unwrap_or_else(PoisonError::into_inner);

                    continue;
                };

                match bucket.take(front.len(), shared.rate.load(Ordering::Relaxed), Instant::now()) {
                    None => break queue.pop_front(),
                    // Dormir sem o cadeado: quem codifica continua empilhando enquanto isso.
                    Some(wait) => {
                        drop(queue);
                        std::thread::sleep(wait);
                        queue = lock(&shared.queue);
                    }
                }
            }
        };

        let Some(packet) = packet else {
            continue;
        };

        match socket.send(&packet) {
            Ok(written) => {
                shared.sent_bytes.fetch_add(written as u64, Ordering::Relaxed);
            }
            // Buffer do sistema cheio mesmo no ritmo: é o uplink que não leva nem o vídeo.
            // O pacote vai embora; o servidor o pede de volta e o reenvio sai do histórico.
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                shared.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(error) => {
                if shared.dropped.fetch_add(1, Ordering::Relaxed) == 0 {
                    tracing::warn!(%error, "ritmo: pacote de vídeo não saiu");
                }
            }
        }
    }
}

fn lock(queue: &Mutex<Queue>) -> MutexGuard<'_, Queue> {
    queue.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKET: usize = 1_200;

    #[test]
    fn an_ordinary_frame_goes_out_at_once() {
        let now = Instant::now();
        let mut bucket = Bucket::new(now);

        // Dez pacotes cheios cabem no fôlego inicial, sem esperar nada.
        for _ in 0..10 {
            assert_eq!(bucket.take(PACKET, STARTING_RATE, now), None);
        }
    }

    #[test]
    fn a_keyframe_is_spread_at_the_pace_rate() {
        let now = Instant::now();
        let mut bucket = Bucket::new(now);
        let mut clock = now;
        let mut sent = 0;

        // 250 pacotes (300 KB) a 25 Mb/s: uns 96 ms, e não de uma vez.
        while sent < 250 {
            match bucket.take(PACKET, STARTING_RATE, clock) {
                None => sent += 1,
                Some(wait) => clock += wait,
            }
        }

        let spread = clock - now;

        assert!(spread >= Duration::from_millis(85), "saiu depressa demais: {spread:?}");
        assert!(spread <= Duration::from_millis(110), "segurou demais: {spread:?}");
    }

    #[test]
    fn a_quiet_line_does_not_bank_more_than_one_burst() {
        let now = Instant::now();
        let mut bucket = Bucket::new(now);
        let later = now + Duration::from_secs(10);
        let mut at_once = 0;

        // Dez segundos parado não viram um fôlego de dez segundos: o quadro-chave seguinte
        // ainda sai espalhado.
        while bucket.take(PACKET, STARTING_RATE, later).is_none() {
            at_once += 1;
        }

        assert!(at_once <= 55, "{at_once} pacotes de uma vez");
    }

    #[test]
    fn a_repair_goes_out_before_the_video_waiting_in_line() {
        let mut queue = Queue::default();

        queue.packets.push_back(Bytes::from_static(b"video"));
        queue.repairs.push_back(Bytes::from_static(b"repair"));

        assert_eq!(queue.pop_front().as_deref(), Some(&b"repair"[..]));
        assert_eq!(queue.pop_front().as_deref(), Some(&b"video"[..]));
    }

    #[test]
    fn the_rate_follows_the_video_and_never_drops_below_the_floor() {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("socket");
        let pacer = Pacer::start(socket).expect("pacer");

        pacer.follow(10_000_000);
        assert_eq!(pacer.shared.rate.load(Ordering::Relaxed), 25_000_000);

        pacer.follow(500_000);
        assert_eq!(pacer.shared.rate.load(Ordering::Relaxed), FLOOR);
    }
}
