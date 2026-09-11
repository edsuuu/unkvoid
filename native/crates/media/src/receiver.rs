//! O outro sentido do RTP puro: receber a transmissão de alguém numa porta UDP.
//!
//! Existe para o app sem WebRTC na janela (o Linux). O servidor manda SRTP para o
//! endereço de onde veio o primeiro pacote — por isso o primeiro ato aqui é mandar um
//! pacote válido, para o roteador de casa abrir o caminho de volta. Depois é só abrir o
//! que chega e repassar, já em RTP limpo, para quem decodifica na própria máquina.
//!
//! ponytail: sem RTCP de volta (sem NACK nem PLI). Perda de pacote é imagem quebrada
//! até o próximo keyframe periódico; o receptor pede um ao retomar o consumer.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use rtc::srtp::context::Context as SrtpContext;
use rtc::srtp::protection_profile::ProtectionProfile;

const KEY_LEN: usize = 16;
const SALT_LEN: usize = 14;

/// Entre um pacote de manutenção e o outro. Roteadores de casa esquecem um mapeamento
/// UDP em trinta segundos de silêncio; aqui o silêncio nunca chega a cinco.
const KEEPALIVE: Duration = Duration::from_secs(5);

pub struct PlainReceiver {
    stop: Arc<AtomicBool>,
    /// Mudo é não repassar o áudio: o decodificador só vê silêncio e retoma quando volta.
    muted: Arc<AtomicBool>,
    packets: Arc<AtomicU64>,
    local: SocketAddr,
}

impl PlainReceiver {
    /// `key` é a chave deste lado (a que foi ao servidor), `server_key` a dele. O que
    /// chegar com `video_payload` vai para `video_to`, com `audio_payload` para
    /// `audio_to` — dois destinos locais, um por decodificador.
    pub fn start(
        server: impl ToSocketAddrs,
        key: &[u8],
        server_key: &[u8],
        video: Option<(u8, SocketAddr)>,
        audio: Option<(u8, SocketAddr)>,
    ) -> Result<Self> {
        let server = server
            .to_socket_addrs()
            .context("could not resolve the SFU address")?
            .next()
            .ok_or_else(|| anyhow!("the SFU address resolved to nothing"))?;

        let socket = UdpSocket::bind(if server.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" })
            .context("could not open the UDP socket for the SFU")?;

        socket.connect(server).context("could not point the socket at the SFU")?;
        socket.set_read_timeout(Some(Duration::from_millis(500)))?;

        let mut outgoing = context(key)?;
        let mut incoming = context(server_key)?;
        let relay = UdpSocket::bind("127.0.0.1:0").context("could not open the local relay socket")?;
        let stop = Arc::new(AtomicBool::new(false));
        let muted = Arc::new(AtomicBool::new(false));
        let packets = Arc::new(AtomicU64::new(0));
        let local = socket.local_addr()?;

        let stop_thread = Arc::clone(&stop);
        let muted_thread = Arc::clone(&muted);
        let packets_thread = Arc::clone(&packets);

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

                let target = match (video, audio) {
                    (Some((wanted, to)), _) if wanted == payload_type => to,
                    (_, Some((wanted, to))) if wanted == payload_type => {
                        if muted_thread.load(Ordering::Relaxed) {
                            continue;
                        }

                        to
                    }
                    _ => continue,
                };

                if relay.send_to(&plain, target).is_ok() {
                    packets_thread.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        Ok(Self { stop, muted, packets, local })
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    pub fn packets(&self) -> u64 {
        self.packets.load(Ordering::Relaxed)
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local
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
}
