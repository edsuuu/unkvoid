//! O outro sentido do RTP puro: receber as transmissões numa porta UDP.
//!
//! Existe para o app sem WebRTC na janela (o Linux). O servidor manda SRTP para o
//! endereço de onde veio o primeiro pacote — por isso o primeiro ato aqui é mandar um
//! pacote válido, para o roteador de casa abrir o caminho de volta. Depois é só abrir o
//! que chega e repassar, já em RTP limpo, para quem decodifica na própria máquina.
//!
//! É UM socket por sessão no servidor: o mediasoup tem um transporte de saída por peer, e
//! por ele chegam a tela, a câmera e o microfone de todo mundo, cada um com o seu SSRC.
//! Um socket por producer não funcionaria — o `comedia` aprende um endereço só e
//! descarta o resto. Então o que separa os fluxos aqui é o SSRC, e cada um vai para a
//! porta local do decodificador que o pediu.
//!
//! No vídeo, o que se perde é pedido de novo: o `recovery.rs` segura quem chegou
//! adiantado, o NACK volta ao servidor, a retransmissão chega pelo RTX e o quadro sai
//! inteiro. Só quando a espera passa do prazo o buraco é largado, e aí vai um PLI no lugar
//! de esperar o keyframe periódico.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use rtc::srtp::context::Context as SrtpContext;
use rtc::srtp::protection_profile::ProtectionProfile;

use crate::recovery::{self, Counters, Recovery};

const KEY_LEN: usize = 16;
const SALT_LEN: usize = 14;

/// Entre um pacote de manutenção e o outro. Roteadores de casa esquecem um mapeamento
/// UDP em trinta segundos de silêncio; aqui o silêncio nunca chega a vinte.
const KEEPALIVE: Duration = Duration::from_secs(20);

/// De quanto em quanto tempo o laço acorda sem pacote nenhum, para pedir reenvio e largar
/// buraco no prazo. Mais longo e o pedido de reenvio atrasaria mais que a própria rede.
const TICK: Duration = Duration::from_millis(10);

/// O intervalo mínimo entre dois pedidos de keyframe do mesmo fluxo. Um keyframe custa o
/// quadro mais caro do encoder, e pedir de novo antes de ele chegar só gera outro.
const PLI_INTERVAL: Duration = Duration::from_millis(300);

/// A retransmissão de um fluxo, como o servidor a anuncia no `consumePlain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rtx {
    pub ssrc: u32,
    pub payload_type: u8,
}

/// Uma transmissão pedida ao receptor: de quem, para onde, e como recuperá-la.
#[derive(Debug, Clone)]
pub struct Stream {
    pub id: String,
    pub payload_type: u8,
    pub to: SocketAddr,
    /// O que o servidor devolveu no `consumePlain`; sem ele, aprendido no primeiro pacote.
    pub ssrc: Option<u32>,
    /// Vídeo ganha a recuperação: esperar o que atrasou, pedir de novo o que faltou e o
    /// keyframe quando não há o que esperar. No som, o Opus refaz o pedaço sozinho.
    pub video: bool,
    pub rtx: Option<Rtx>,
}

/// Para onde vai o que chega de um producer: a porta local do decodificador dele.
struct Route {
    id: String,
    payload_type: u8,
    to: SocketAddr,
    /// O SSRC que o servidor devolveu no `consumePlain`; sem ele, aprendido no primeiro
    /// pacote do mesmo tipo de payload.
    ssrc: Option<u32>,
    /// Mudo é não repassar: o decodificador só vê silêncio e retoma quando volta.
    muted: bool,
    rtx: Option<Rtx>,
    /// Só no vídeo.
    recovery: Option<Recovery>,
    last_pli: Option<Instant>,
}

#[derive(Default)]
struct Routes {
    active: Vec<Route>,
    /// SSRCs de producers já fechados. O servidor ainda manda um resto deles depois do
    /// `closeConsumer`, e sem esta lista esse resto era "aprendido" pela próxima rota
    /// sem SSRC — a câmera nova passava a receber a tela velha.
    retired: Vec<u32>,
}

pub struct PlainReceiver {
    stop: Arc<AtomicBool>,
    packets: Arc<AtomicU64>,
    routes: Arc<Mutex<Routes>>,
    local: SocketAddr,
    server: SocketAddr,
}

impl PlainReceiver {
    /// `key` é a chave deste lado (a que foi ao servidor), `server_key` a dele. Os
    /// destinos entram depois, um por producer, em `route`.
    pub fn start(server: impl ToSocketAddrs, key: &[u8], server_key: &[u8]) -> Result<Self> {
        let server = resolve(server)?;

        let socket = UdpSocket::bind(if server.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" })
            .context("could not open the UDP socket for the SFU")?;

        socket.connect(server).context("could not point the socket at the SFU")?;
        socket.set_read_timeout(Some(TICK))?;

        let mut outgoing = context(key)?;
        let mut incoming = context(server_key)?;
        let relay = UdpSocket::bind("127.0.0.1:0").context("could not open the local relay socket")?;
        let stop = Arc::new(AtomicBool::new(false));
        let packets = Arc::new(AtomicU64::new(0));
        let routes: Arc<Mutex<Routes>> = Arc::default();
        let local = socket.local_addr()?;
        // `UNKVOID_LOSS=3` joga fora 3% do RTP que chega, de propósito: é como se prova a
        // recuperação contra o servidor de verdade numa máquina onde a rede não perde nada.
        let loss = std::env::var("UNKVOID_LOSS").ok().and_then(|percent| percent.parse::<f64>().ok()).unwrap_or(0.0);

        let stop_thread = Arc::clone(&stop);
        let packets_thread = Arc::clone(&packets);
        let routes_thread = Arc::clone(&routes);

        std::thread::spawn(move || {
            let ssrc: u32 = rand::random();
            let mut sequence: u16 = rand::random();
            let mut punched = Instant::now() - KEEPALIVE;
            let mut buffer = [0_u8; 2048];

            while ! stop_thread.load(Ordering::Relaxed) {
                if punched.elapsed() >= KEEPALIVE {
                    if let Ok(packet) = outgoing.encrypt_rtp(&punch(ssrc, sequence)) {
                        let _ = socket.send(&packet);
                    }

                    sequence = sequence.wrapping_add(1);
                    punched = Instant::now();
                }

                let now = Instant::now();

                if let Ok(size) = socket.recv(&mut buffer)
                    // RTCP (relatórios do servidor) tem o segundo byte entre 200 e 207.
                    && size >= 12
                    && !(192..=223).contains(&buffer[1])
                    && (loss <= 0.0 || rand::random::<f64>() * 100.0 >= loss)
                    && let Ok(plain) = incoming.decrypt_rtp(&buffer[..size])
                    && let Ok(mut routes) = routes_thread.lock()
                {
                    let payload_type = plain[1] & 0x7f;
                    let ssrc = u32::from_be_bytes([plain[8], plain[9], plain[10], plain[11]]);

                    for (target, packet) in deliver(&mut routes, plain.to_vec(), ssrc, payload_type, now) {
                        if relay.send_to(&packet, target).is_ok() {
                            packets_thread.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }

                let Ok(mut routes) = routes_thread.lock() else {
                    continue;
                };

                for route in &mut routes.active {
                    let (Some(recovery), Some(media)) = (route.recovery.as_mut(), route.ssrc) else {
                        continue;
                    };
                    let due = recovery.due(now);

                    for packet in due.released {
                        if relay.send_to(&packet, route.to).is_ok() {
                            packets_thread.fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    if !due.nack.is_empty()
                        && let Ok(feedback) = outgoing.encrypt_rtcp(&recovery::nack(ssrc, media, &due.nack))
                    {
                        let _ = socket.send(&feedback);
                    }

                    let pli_allowed = route.last_pli.is_none_or(|last| now.duration_since(last) >= PLI_INTERVAL);

                    if due.pli
                        && pli_allowed
                        && let Ok(feedback) = outgoing.encrypt_rtcp(&recovery::pli(ssrc, media))
                    {
                        let _ = socket.send(&feedback);
                        route.last_pli = Some(now);
                    }
                }
            }
        });

        Ok(Self { stop, packets, routes, local, server })
    }

    /// O que chegar com o SSRC da transmissão vai para `to`. Sem SSRC, vai o primeiro fluxo
    /// do mesmo tipo de payload que ninguém reclamou — ver `pick_route`.
    pub fn route(&self, stream: Stream) {
        let Stream { id, payload_type, to, ssrc, video, rtx } = stream;

        if let Ok(mut routes) = self.routes.lock() {
            routes.active.retain(|route| route.id != id);
            routes.active.push(Route {
                id,
                payload_type,
                to,
                ssrc,
                muted: false,
                rtx,
                recovery: video.then(Recovery::default),
                last_pli: None,
            });
        }
    }

    /// O que aconteceu com o vídeo de uma transmissão: recebidos, recuperados e perdidos.
    pub fn counters(&self, id: &str) -> Option<Counters> {
        let routes = self.routes.lock().ok()?;

        routes.active.iter().find(|route| route.id == id)?.recovery.as_ref().map(Recovery::counters)
    }

    pub fn unroute(&self, id: &str) {
        if let Ok(mut routes) = self.routes.lock() {
            let (gone, kept): (Vec<Route>, Vec<Route>) =
                std::mem::take(&mut routes.active).into_iter().partition(|route| route.id == id);

            routes.active = kept;
            routes.retired.extend(gone.into_iter().filter_map(|route| route.ssrc));
        }
    }

    pub fn set_muted(&self, id: &str, muted: bool) {
        if let Ok(mut routes) = self.routes.lock()
            && let Some(route) = routes.active.iter_mut().find(|route| route.id == id)
        {
            route.muted = muted;
        }
    }

    pub fn packets(&self) -> u64 {
        self.packets.load(Ordering::Relaxed)
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    pub fn server(&self) -> SocketAddr {
        self.server
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for PlainReceiver {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Para onde vai um pacote que chegou, e o que ele destrava. O de retransmissão volta a
/// ser o original antes de tudo; o de vídeo passa pela recuperação, que pode segurá-lo ou
/// soltar junto os que esperavam por ele.
fn deliver(routes: &mut Routes, packet: Vec<u8>, ssrc: u32, payload_type: u8, now: Instant) -> Vec<(SocketAddr, Vec<u8>)> {
    let repaired = routes.active.iter().position(|route| route.rtx.is_some_and(|rtx| rtx.ssrc == ssrc));

    let (index, packet) = match repaired {
        Some(index) => {
            let route = &routes.active[index];
            let Some(original) = route.ssrc.and_then(|media| recovery::unwrap_rtx(&packet, media, route.payload_type)) else {
                return Vec::new();
            };

            (index, original)
        }
        None => {
            let Some(index) = pick_route(routes, ssrc, payload_type) else {
                return Vec::new();
            };

            (index, packet)
        }
    };

    let route = &mut routes.active[index];

    if route.muted {
        return Vec::new();
    }

    let to = route.to;

    match (route.recovery.as_mut(), recovery::sequence_of(&packet)) {
        (Some(recovery), Some(sequence)) => recovery.arrive(sequence, packet, now).into_iter().map(|ready| (to, ready)).collect(),
        _ => vec![(to, packet)],
    }
}

/// A rota de um pacote: a que tem este SSRC, senão a mais antiga do mesmo tipo de
/// payload que ainda não aprendeu o seu — e ela aprende agora, a menos que o SSRC seja
/// o resto de um producer já fechado.
///
/// O caminho de aprender existe para servidor antigo, que não devolve o SSRC no
/// `consumePlain`; com ele devolvido a rota casa exato e nunca troca de lugar.
fn pick_route(routes: &mut Routes, ssrc: u32, payload_type: u8) -> Option<usize> {
    let index = routes.active.iter().position(|route| route.ssrc == Some(ssrc)).or_else(|| {
        if routes.retired.contains(&ssrc) {
            return None;
        }

        routes
            .active
            .iter()
            .position(|route| route.ssrc.is_none() && route.payload_type == payload_type)
    })?;

    routes.active[index].ssrc = Some(ssrc);

    Some(index)
}

pub fn resolve(server: impl ToSocketAddrs) -> Result<SocketAddr> {
    server
        .to_socket_addrs()
        .context("could not resolve the SFU address")?
        .next()
        .ok_or_else(|| anyhow!("the SFU address resolved to nothing"))
}

fn context(key: &[u8]) -> Result<SrtpContext> {
    if key.len() != KEY_LEN + SALT_LEN {
        return Err(anyhow!("SRTP key must be {} bytes, got {}", KEY_LEN + SALT_LEN, key.len()));
    }

    SrtpContext::new(
        &key[..KEY_LEN],
        &key[KEY_LEN..],
        ProtectionProfile::Aes128CmHmacSha1_80,
        None,
        None,
    )
    .map_err(|error| anyhow!("could not start SRTP: {error}"))
}

/// Um pacote RTP mínimo e válido. O servidor só aprende o endereço de quem manda depois
/// de abrir o pacote, então ele tem de ser SRTP de verdade — não basta um datagrama.
fn punch(ssrc: u32, sequence: u16) -> Vec<u8> {
    let mut packet = vec![0x80, 96];

    packet.extend(sequence.to_be_bytes());
    packet.extend(0_u32.to_be_bytes());
    packet.extend(ssrc.to_be_bytes());
    packet.extend([0, 0, 0, 0]);

    packet
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_punch_survives_a_round_trip_through_srtp() {
        let key = [7_u8; KEY_LEN + SALT_LEN];
        let mut sender = context(&key).expect("context");
        let mut receiver = context(&key).expect("context");

        let encrypted = sender.encrypt_rtp(&punch(1234, 1)).expect("encrypt");
        let plain = receiver.decrypt_rtp(&encrypted).expect("decrypt");

        assert_eq!(&plain[..], &punch(1234, 1)[..]);
        assert_eq!(plain[1] & 0x7f, 96);
    }

    fn to(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn route(id: &str, payload_type: u8, port: u16, ssrc: Option<u32>) -> Route {
        Route {
            id: id.into(),
            payload_type,
            to: to(port),
            ssrc,
            muted: false,
            rtx: None,
            recovery: None,
            last_pli: None,
        }
    }

    fn picked(routes: &mut Routes, ssrc: u32, payload_type: u8) -> Option<SocketAddr> {
        pick_route(routes, ssrc, payload_type).map(|index| routes.active[index].to)
    }

    /// Tela e câmera chegam com o mesmo tipo de payload; só o SSRC os separa.
    #[test]
    fn a_route_learns_its_ssrc_on_the_first_packet_and_keeps_it() {
        let mut routes = Routes {
            active: vec![
                route("screen", 101, 1, None),
                route("camera", 101, 2, None),
                route("mic", 100, 3, None),
            ],
            retired: vec![0xEE],
        };

        // O resto de um producer fechado não é aprendido por ninguém.
        assert!(picked(&mut routes, 0xEE, 101).is_none());
        assert_eq!(picked(&mut routes, 0xAA, 101), Some(to(1)));
        assert_eq!(picked(&mut routes, 0xBB, 101), Some(to(2)));
        assert_eq!(picked(&mut routes, 0xBB, 101), Some(to(2)));
        assert_eq!(picked(&mut routes, 0xAA, 101), Some(to(1)));
        assert_eq!(picked(&mut routes, 0xCC, 100), Some(to(3)));
        // Um terceiro vídeo que ninguém pediu não tem para onde ir.
        assert!(picked(&mut routes, 0xDD, 101).is_none());
    }

    fn rtp(sequence: u16, ssrc: u32, payload_type: u8, payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0x80, 0x80 | payload_type];

        packet.extend(sequence.to_be_bytes());
        packet.extend(u32::from(sequence).to_be_bytes());
        packet.extend(ssrc.to_be_bytes());
        packet.extend(payload);

        packet
    }

    fn sequence_arrived(decoder: &UdpSocket) -> (u16, u32, u8) {
        let mut buffer = [0_u8; 1_500];
        let size = decoder.recv(&mut buffer).expect("o pacote chegou ao decodificador");

        assert!(size >= 12);

        (
            u16::from_be_bytes([buffer[2], buffer[3]]),
            u32::from_be_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]),
            buffer[1] & 0x7f,
        )
    }

    /// A corrente inteira com SRTP de verdade: o servidor manda 1, 2 e 4; o receptor entrega
    /// 1 e 2, segura o 4, pede o 3 num NACK cifrado, e quando o 3 volta pelo RTX o entrega
    /// desembrulhado — com o SSRC e o tipo do fluxo principal — antes do 4.
    #[test]
    fn a_lost_packet_is_asked_again_and_the_retransmission_comes_out_in_order() {
        let (client_key, server_key) = ([1_u8; KEY_LEN + SALT_LEN], [2_u8; KEY_LEN + SALT_LEN]);
        let server = UdpSocket::bind("127.0.0.1:0").expect("o servidor de mentira");
        let decoder = UdpSocket::bind("127.0.0.1:0").expect("o decodificador de mentira");

        server.set_read_timeout(Some(Duration::from_secs(3))).expect("prazo");
        decoder.set_read_timeout(Some(Duration::from_secs(3))).expect("prazo");

        let receiver = PlainReceiver::start(server.local_addr().expect("porta"), &client_key, &server_key).expect("o receptor abriu");

        receiver.route(Stream {
            id: "tela".into(),
            payload_type: 96,
            to: decoder.local_addr().expect("porta"),
            ssrc: Some(0x1111),
            video: true,
            rtx: Some(Rtx { ssrc: 0x2222, payload_type: 97 }),
        });

        let mut buffer = [0_u8; 1_500];
        let (_, client) = server.recv_from(&mut buffer).expect("o pacote que abre o caminho");
        let mut sending = context(&server_key).expect("contexto");
        let mut feedback = context(&client_key).expect("contexto");

        for sequence in [1, 2, 4] {
            let packet = sending.encrypt_rtp(&rtp(sequence, 0x1111, 96, &[sequence as u8])).expect("cifrou");

            server.send_to(&packet, client).expect("mandou");
        }

        assert_eq!(sequence_arrived(&decoder).0, 1);
        assert_eq!(sequence_arrived(&decoder).0, 2);

        let asked = loop {
            let size = server.recv(&mut buffer).expect("o NACK chegou ao servidor");

            if let Ok(plain) = feedback.decrypt_rtcp(&buffer[..size])
                && plain[1] == 205
            {
                break plain.to_vec();
            }
        };

        assert_eq!(u32::from_be_bytes([asked[8], asked[9], asked[10], asked[11]]), 0x1111, "o NACK nomeia o fluxo");
        assert_eq!(u16::from_be_bytes([asked[12], asked[13]]), 3, "o NACK pede o 3");

        let repaired = sending.encrypt_rtp(&rtp(500, 0x2222, 97, &[0, 3, 3])).expect("cifrou");

        server.send_to(&repaired, client).expect("mandou o reenvio");

        assert_eq!(sequence_arrived(&decoder), (3, 0x1111, 96));
        assert_eq!(sequence_arrived(&decoder).0, 4);

        let counters = receiver.counters("tela").expect("o vídeo tem contagem");

        assert_eq!((counters.received, counters.recovered, counters.lost), (4, 1, 0));
    }

    /// Com o SSRC devolvido pelo servidor a rota casa exato, mesmo que outra do mesmo
    /// tipo esteja livre para aprender.
    #[test]
    fn a_known_ssrc_matches_exactly_and_never_steals_a_learning_route() {
        let mut routes = Routes {
            active: vec![route("screen", 101, 1, None), route("camera", 101, 2, Some(0xBB))],
            retired: Vec::new(),
        };

        assert_eq!(picked(&mut routes, 0xBB, 101), Some(to(2)));
        assert_eq!(picked(&mut routes, 0xAA, 101), Some(to(1)));
    }
}
