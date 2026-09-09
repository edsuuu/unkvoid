//! Broadcasting through the SFU instead of directly to each viewer.
//!
//! Direct connections are cheaper in latency — the server is in the US, the people are
//! in Brazil — but the broadcaster's upload multiplies by the number of viewers. Past a
//! handful of viewers that upload, not the CPU, is what breaks the stream: the frame is
//! still encoded once, but it is sent N times.
//!
//! Here it is sent **once**, to the server, which fans it out. The path is plain RTP over
//! UDP (mediasoup's PlainTransport) rather than a full WebRTC connection: there is no ICE
//! and no DTLS to negotiate, because the server already knows what is coming — this side
//! picked the SSRC, the payload type and the SRTP key, and announced them when it asked
//! for the transport. `comedia` on the server means it learns this side's address from
//! the first packet, so nothing here needs to be reachable from outside.
//!
//! SRTP is not optional: without it a screen share crosses the internet in the clear.

use std::io::ErrorKind;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use rtc::rtp::codec::h264::H264Payloader;
use rtc::rtp::codec::opus::OpusPayloader;
use rtc::rtp::packetizer::Payloader;
use rtc::rtp::packetizer::{new_packetizer, Packetizer};
use rtc::rtp::sequence::{new_random_sequencer, Sequencer};
use rtc::shared::marshal::Marshal;
use rtc::srtp::context::Context as SrtpContext;
use rtc::srtp::protection_profile::ProtectionProfile;

use crate::{audio::CHANNELS, audio::SAMPLE_RATE, EncodedFrame, FRAME_MS};

/// SSRCs the server is told about before a single packet is sent.
pub const SSRC_VIDEO: u32 = 0x2234_5678;
pub const SSRC_AUDIO: u32 = 0x2234_5679;

/// Payload types. 96+ is the dynamic range; the server echoes whatever is declared.
pub const PAYLOAD_VIDEO: u8 = 96;
pub const PAYLOAD_AUDIO: u8 = 111;

/// 90 kHz is the RTP clock for video, fixed by the H.264 payload format.
const VIDEO_CLOCK: u32 = 90_000;

/// Under the 1500-byte Ethernet MTU with room for IP, UDP and the SRTP tag. Going over
/// it means IP fragmentation, and a single lost fragment costs the whole frame.
const MTU: usize = 1200;

/// A full local UDP buffer is transient: retrying briefly preserves a complete H.264
/// frame, while a permanent network error still returns immediately.
const SEND_RETRIES: usize = 256;
const SEND_RETRY_DELAY: Duration = Duration::from_millis(2);

/// The one crypto suite negotiated with the server. `AES_CM_128_HMAC_SHA1_80` on the
/// mediasoup side: a 16-byte key plus a 14-byte salt, exchanged base64 as one blob.
const KEY_LEN: usize = 16;
const SALT_LEN: usize = 14;

pub struct PlainSender {
    socket: UdpSocket,
    server: SocketAddr,
    srtp: SrtpContext,
    video: Box<dyn Packetizer>,
    audio: Box<dyn Packetizer>,
}

impl PlainSender {
    pub const CRYPTO_SUITE: &'static str = "AES_CM_128_HMAC_SHA1_80";

    /// A fresh key for this broadcast. It never leaves this process except inside the
    /// authenticated WebSocket that asks the server for the transport.
    pub fn generate_key() -> [u8; KEY_LEN + SALT_LEN] {
        std::array::from_fn(|_| rand::random())
    }

    /// `key` is what `generate_key` produced and what the server was told; `server` is
    /// the address it answered with.
    pub fn connect(server: impl ToSocketAddrs, key: &[u8]) -> Result<Self> {
        if key.len() != KEY_LEN + SALT_LEN {
            return Err(anyhow!(
                "SRTP key must be {} bytes, got {}",
                KEY_LEN + SALT_LEN,
                key.len()
            ));
        }

        let server = server
            .to_socket_addrs()
            .context("could not resolve the SFU address")?
            .next()
            .ok_or_else(|| anyhow!("the SFU address resolved to nothing"))?;

        // Binding to port 0 on the unspecified address lets the OS choose. The server
        // learns where to answer from the first packet it receives.
        let socket = UdpSocket::bind(if server.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })
        .context("could not open the UDP socket for the SFU")?;

        socket
            .connect(server)
            .context("could not point the socket at the SFU")?;

        // Quem manda é a thread da captura. Se o buffer do socket encher, bloquear ali
        // seguraria o próximo quadro — e para vídeo ao vivo perder um pacote custa muito
        // menos do que perder fps.
        socket
            .set_nonblocking(true)
            .context("could not put the SFU socket in non-blocking mode")?;

        let srtp = SrtpContext::new(
            &key[..KEY_LEN],
            &key[KEY_LEN..],
            ProtectionProfile::Aes128CmHmacSha1_80,
            None,
            None,
        )
        .map_err(|error| anyhow!("could not start SRTP: {error}"))?;

        Ok(Self {
            socket,
            server,
            srtp,
            video: Box::new(new_packetizer(
                MTU,
                PAYLOAD_VIDEO,
                SSRC_VIDEO,
                Box::<H264Payloader>::default() as Box<dyn Payloader>,
                Box::new(new_random_sequencer()) as Box<dyn Sequencer>,
                VIDEO_CLOCK,
            )),
            audio: Box::new(new_packetizer(
                MTU,
                PAYLOAD_AUDIO,
                SSRC_AUDIO,
                Box::<OpusPayloader>::default() as Box<dyn Payloader>,
                Box::new(new_random_sequencer()) as Box<dyn Sequencer>,
                SAMPLE_RATE,
            )),
        })
    }

    pub fn server(&self) -> SocketAddr {
        self.server
    }

    /**
     * What the server needs to know before the first packet: which codec, which payload
     * type, which SSRC. It is built here and not in the UI because these are the same
     * constants the packetizer above uses — describing them in two places is how a
     * broadcast ends up arriving as noise.
     */
    pub fn rtp_parameters(kind: &str) -> serde_json::Value {
        if kind == "audio" {
            return serde_json::json!({
                "codecs": [{
                    "mimeType": "audio/opus",
                    "payloadType": PAYLOAD_AUDIO,
                    "clockRate": SAMPLE_RATE,
                    "channels": CHANNELS,
                    "parameters": { "useinbandfec": 1, "usedtx": 1 },
                    "rtcpFeedback": [],
                }],
                "encodings": [{ "ssrc": SSRC_AUDIO }],
            });
        }

        serde_json::json!({
            "codecs": [{
                "mimeType": "video/H264",
                "payloadType": PAYLOAD_VIDEO,
                "clockRate": VIDEO_CLOCK,
                "parameters": {
                    "packetization-mode": 1,
                    "level-asymmetry-allowed": 1,
                    "profile-level-id": "42e01f",
                },
                // Without nack and pli a viewer that loses a packet stays with a broken
                // image until the next keyframe — two seconds of garbage.
                "rtcpFeedback": [
                    { "type": "nack" },
                    { "type": "nack", "parameter": "pli" },
                    { "type": "ccm", "parameter": "fir" },
                    { "type": "goog-remb" },
                ],
            }],
            "encodings": [{ "ssrc": SSRC_VIDEO }],
        })
    }

    /// One encoded frame becomes several RTP packets — an H.264 keyframe at 1440p is far
    /// bigger than an MTU. `samples` is how far the RTP clock advances, which is what
    /// tells the far side when to display the frame.
    pub fn send_frame(&mut self, frame: &EncodedFrame, frame_rate: f64) -> Result<()> {
        let samples = (VIDEO_CLOCK as f64 / frame_rate.max(1.0)).round() as u32;

        // Fields borrowed separately so the packetizer and the SRTP context can both be
        // mutable at once — they are different fields of the same struct.
        Self::send(
            &self.socket,
            &mut self.srtp,
            self.video.as_mut(),
            Bytes::copy_from_slice(&frame.data),
            samples,
        )
    }

    /// Opus arrives in fixed 20 ms blocks, so the clock always advances by the same amount.
    pub fn send_audio(&mut self, opus: &[u8]) -> Result<()> {
        let samples = SAMPLE_RATE / 1000 * FRAME_MS;

        Self::send(
            &self.socket,
            &mut self.srtp,
            self.audio.as_mut(),
            Bytes::copy_from_slice(opus),
            samples,
        )
    }

    fn send(
        socket: &UdpSocket,
        srtp: &mut SrtpContext,
        packetizer: &mut dyn Packetizer,
        payload: Bytes,
        samples: u32,
    ) -> Result<()> {
        let packets = packetizer
            .packetize(&payload, samples)
            .map_err(|error| anyhow!("could not packetize: {error}"))?;

        for packet in packets {
            let plain = packet
                .marshal()
                .map_err(|error| anyhow!("could not serialize RTP: {error}"))?;

            let protected = srtp
                .encrypt_rtp(&plain)
                .map_err(|error| anyhow!("could not protect RTP: {error}"))?;

            for attempt in 0..=SEND_RETRIES {
                match socket.send(&protected) {
                    Ok(_) => break,
                    Err(error)
                        if error.kind() == ErrorKind::WouldBlock && attempt < SEND_RETRIES =>
                    {
                        thread::sleep(SEND_RETRY_DELAY);
                    }
                    Err(error) => {
                        return Err(error).context("could not send RTP to the SFU");
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A local socket standing in for the SFU, so the whole path — packetize, protect,
    /// send — is exercised for real instead of mocked.
    fn ouvinte() -> (UdpSocket, SocketAddr) {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("could not bind the listener");
        let address = socket.local_addr().expect("socket without an address");

        socket
            .set_read_timeout(Some(std::time::Duration::from_millis(500)))
            .expect("could not set the timeout");

        (socket, address)
    }

    #[test]
    fn chave_precisa_ter_o_tamanho_da_suite() {
        let (_servidor, address) = ouvinte();

        assert!(PlainSender::connect(address, &[0; 10]).is_err());
        assert!(PlainSender::connect(address, &PlainSender::generate_key()).is_ok());
    }

    #[test]
    fn chaves_geradas_nao_se_repetem() {
        assert_ne!(PlainSender::generate_key(), PlainSender::generate_key());
    }

    #[test]
    fn um_quadro_grande_vira_varios_pacotes_protegidos() {
        let (servidor, address) = ouvinte();
        let key = PlainSender::generate_key();
        let mut sender = PlainSender::connect(address, &key).expect("could not connect");

        // A NAL unit far larger than the MTU: the payloader has to split it, and every
        // piece has to arrive protected.
        let mut data = vec![0u8, 0, 0, 1, 0x65];
        data.extend(std::iter::repeat_n(0xAB, MTU * 3));

        sender
            .send_frame(
                &EncodedFrame {
                    data: data.clone(),
                    keyframe: true,
                    timestamp_ns: 0,
                },
                60.0,
            )
            .expect("could not send the frame");

        let mut recebidos = 0;
        let mut buffer = [0u8; 2048];

        while let Ok(size) = servidor.recv(&mut buffer) {
            recebidos += 1;

            assert!(size <= MTU + 64, "packet above the MTU: {size}");
            // Protected payload: the plaintext must not appear on the wire.
            assert!(
                !buffer[..size]
                    .windows(16)
                    .any(|janela| janela == [0xAB; 16]),
                "the payload went out in the clear"
            );
        }

        assert!(
            recebidos > 1,
            "a frame above the MTU produced {recebidos} packet(s)"
        );
    }

    #[test]
    fn audio_cabe_em_um_pacote_e_avanca_o_relogio() {
        let (servidor, address) = ouvinte();
        let mut sender =
            PlainSender::connect(address, &PlainSender::generate_key()).expect("could not connect");

        sender
            .send_audio(&[0x7F; 160])
            .expect("could not send audio");
        sender
            .send_audio(&[0x7F; 160])
            .expect("could not send audio");

        let mut buffer = [0u8; 2048];
        let mut carimbos = Vec::new();

        while let Ok(size) = servidor.recv(&mut buffer) {
            // The RTP timestamp lives in bytes 4..8 and is not encrypted — the header
            // travels in the clear so the far side can reorder before decrypting.
            carimbos.push(u32::from_be_bytes([
                buffer[4], buffer[5], buffer[6], buffer[7],
            ]));
            assert!(size > 160, "packet without header or auth tag: {size}");
        }

        assert_eq!(carimbos.len(), 2, "each 20 ms block is one packet");
        assert_eq!(
            carimbos[1].wrapping_sub(carimbos[0]),
            SAMPLE_RATE / 1000 * FRAME_MS,
            "the clock has to advance exactly one 20 ms block"
        );
    }
}
