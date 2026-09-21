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
//! ponytail: sem RTCP de volta (sem NACK nem PLI). Perda de pacote é imagem quebrada
//! até o próximo keyframe periódico; o receptor pede um ao retomar o consumer.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use rtc::srtp::context::Context as SrtpContext;
use rtc::srtp::protection_profile::ProtectionProfile;

const KEY_LEN: usize = 16;
const SALT_LEN: usize = 14;

/// Entre um pacote de manutenção e o outro. Roteadores de casa esquecem um mapeamento
/// UDP em trinta segundos de silêncio; aqui o silêncio nunca chega a vinte.
const KEEPALIVE: Duration = Duration::from_secs(20);

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
        socket.set_read_timeout(Some(Duration::from_millis(500)))?;

        let mut outgoing = context(key)?;
        let mut incoming = context(server_key)?;
        let relay = UdpSocket::bind("127.0.0.1:0").context("could not open the local relay socket")?;
        let stop = Arc::new(AtomicBool::new(false));
        let packets = Arc::new(AtomicU64::new(0));
        let routes: Arc<Mutex<Routes>> = Arc::default();
        let local = socket.local_addr()?;

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

                let Ok(size) = socket.recv(&mut buffer) else {
                    continue;
                };

                // RTCP (relatórios do servidor) tem o segundo byte entre 200 e 207.
                if size < 12 || (192..=223).contains(&buffer[1]) {
                    continue;
                }

                let Ok(plain) = incoming.decrypt_rtp(&buffer[..size]) else {
                    continue;
                };

                let payload_type = plain[1] & 0x7f;
                let ssrc = u32::from_be_bytes([plain[8], plain[9], plain[10], plain[11]]);

                let target = routes_thread.lock().ok().and_then(|mut routes| {
                    let route = pick_route(&mut routes, ssrc, payload_type)?;

                    (! route.muted).then_some(route.to)
                });

                if let Some(target) = target
                    && relay.send_to(&plain, target).is_ok()
                {
                    packets_thread.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        Ok(Self { stop, packets, routes, local, server })
    }

    /// O que chegar com `ssrc` vai para `to`. Sem SSRC, vai o primeiro fluxo de
    /// `payload_type` que ninguém reclamou — ver `pick_route`.
    pub fn route(&self, id: String, payload_type: u8, to: SocketAddr, ssrc: Option<u32>) {
        if let Ok(mut routes) = self.routes.lock() {
            routes.active.retain(|route| route.id != id);
            routes.active.push(Route { id, payload_type, to, ssrc, muted: false });
        }
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

/// A rota de um pacote: a que tem este SSRC, senão a mais antiga do mesmo tipo de
/// payload que ainda não aprendeu o seu — e ela aprende agora, a menos que o SSRC seja
/// o resto de um producer já fechado.
///
/// O caminho de aprender existe para servidor antigo, que não devolve o SSRC no
/// `consumePlain`; com ele devolvido a rota casa exato e nunca troca de lugar.
fn pick_route(routes: &mut Routes, ssrc: u32, payload_type: u8) -> Option<&Route> {
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

    Some(&routes.active[index])
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
        Route { id: id.into(), payload_type, to: to(port), ssrc, muted: false }
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
        assert!(pick_route(&mut routes, 0xEE, 101).is_none());
        assert_eq!(pick_route(&mut routes, 0xAA, 101).map(|route| route.to), Some(to(1)));
        assert_eq!(pick_route(&mut routes, 0xBB, 101).map(|route| route.to), Some(to(2)));
        assert_eq!(pick_route(&mut routes, 0xBB, 101).map(|route| route.to), Some(to(2)));
        assert_eq!(pick_route(&mut routes, 0xAA, 101).map(|route| route.to), Some(to(1)));
        assert_eq!(pick_route(&mut routes, 0xCC, 100).map(|route| route.to), Some(to(3)));
        // Um terceiro vídeo que ninguém pediu não tem para onde ir.
        assert!(pick_route(&mut routes, 0xDD, 101).is_none());
    }

    /// Com o SSRC devolvido pelo servidor a rota casa exato, mesmo que outra do mesmo
    /// tipo esteja livre para aprender.
    #[test]
    fn a_known_ssrc_matches_exactly_and_never_steals_a_learning_route() {
        let mut routes = Routes {
            active: vec![route("screen", 101, 1, None), route("camera", 101, 2, Some(0xBB))],
            retired: Vec::new(),
        };

        assert_eq!(pick_route(&mut routes, 0xBB, 101).map(|route| route.to), Some(to(2)));
        assert_eq!(pick_route(&mut routes, 0xAA, 101).map(|route| route.to), Some(to(1)));
    }
}
