//! A transmissão pelo SFU, em vez de direto para cada espectador.
//!
//! Conexão direta custa menos latência — o servidor está nos EUA e as pessoas no Brasil
//! — mas o upload de quem transmite multiplica pelo número de espectadores. Passando de
//! um punhado, é esse upload, e não a CPU, que quebra a transmissão: o quadro continua
//! sendo codificado uma vez, e passa a ser enviado N vezes.
//!
//! Aqui ele sobe **uma vez**, para o servidor, que replica. O caminho é RTP puro sobre
//! UDP (o PlainTransport do mediasoup) em vez de uma conexão WebRTC inteira: não há ICE
//! nem DTLS a negociar, porque o servidor já sabe o que vem — este lado escolheu o SSRC,
//! o tipo de payload e a chave SRTP, e anunciou os três ao pedir o transporte. O
//! `comedia` do lado de lá faz o servidor aprender este endereço no primeiro pacote,
//! então nada aqui precisa ser alcançável de fora.
//!
//! O SRTP não é opcional: sem ele a tela atravessa a internet aberta.

use std::io::ErrorKind;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

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

/// Os SSRCs que o servidor conhece antes do primeiro pacote.
pub const SSRC_VIDEO: u32 = 0x2234_5678;
pub const SSRC_AUDIO: u32 = 0x2234_5679;

/// Tipos de payload. 96+ é a faixa dinâmica, e o servidor devolve o que for declarado.
pub const PAYLOAD_VIDEO: u8 = 96;
pub const PAYLOAD_AUDIO: u8 = 111;

/// 90 kHz é o relógio RTP do vídeo, fixado pelo formato de payload do H.264.
const VIDEO_CLOCK: u32 = 90_000;

/// Abaixo da MTU de 1500 bytes da Ethernet, com folga para IP, UDP e a etiqueta do
/// SRTP. Passar disso significa fragmentação de IP, e um só fragmento perdido custa o
/// quadro inteiro.
const MTU: usize = 1200;

/// A única suíte criptográfica combinada com o servidor. Do lado do mediasoup ela é
/// `AES_CM_128_HMAC_SHA1_80`: 16 bytes de chave e 14 de sal, trocados em base64 juntos.
const KEY_LEN: usize = 16;
const SALT_LEN: usize = 14;

pub struct PlainSender {
    socket: UdpSocket,
    server: SocketAddr,
    srtp: SrtpContext,
    video: Box<dyn Packetizer>,
    audio: Box<dyn Packetizer>,

    /// Quando o quadro anterior foi capturado. O relógio RTP anda com o tempo de
    /// verdade, não com o fps nominal.
    last_video_ns: Option<u64>,

    /// Pacotes largados por buffer de saída cheio. Uplink saturado é diferente de erro
    /// de rede, e sem este número os dois viram a mesma linha muda no diagnóstico.
    dropped: u64,

    /// Bytes que saíram de verdade, já protegidos. É daqui que a barra tira os Mb/s:
    /// contar pacotes não diz nada quando um quadro parado custa 1 KB e um keyframe 300.
    sent_bytes: u64,
}

impl PlainSender {
    pub const CRYPTO_SUITE: &'static str = "AES_CM_128_HMAC_SHA1_80";

    /// Uma chave nova para esta transmissão. Ela só sai deste processo dentro do
    /// WebSocket autenticado que pede o transporte ao servidor.
    pub fn generate_key() -> [u8; KEY_LEN + SALT_LEN] {
        std::array::from_fn(|_| rand::random())
    }

    /// `key` é o que o `generate_key` produziu e o que o servidor recebeu; `server` é o
    /// endereço que ele respondeu.
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

        // Ligar na porta 0 do endereço não especificado deixa o sistema escolher. O
        // servidor aprende para onde responder no primeiro pacote que chega.
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

        grow_send_buffer(&socket);

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
            last_video_ns: None,
            dropped: 0,
            sent_bytes: 0,
        })
    }

    pub fn server(&self) -> SocketAddr {
        self.server
    }

    /**
     * O que o servidor precisa saber antes do primeiro pacote: qual codec, qual tipo de
     * payload, qual SSRC. É montado aqui e não na interface porque são as mesmas
     * constantes que o empacotador acima usa — descrever isso em dois lugares é como uma
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
                // Sem nack e pli, quem assiste e perde um pacote fica com a imagem
                // quebrada até o próximo keyframe — dois segundos de lixo.
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

    /// Um quadro codificado vira vários pacotes RTP — um keyframe de 1440p é bem maior
    /// que uma MTU. O avanço do relógio RTP é o que diz ao outro lado quando exibir.
    pub fn send_frame(&mut self, frame: &EncodedFrame, frame_rate: f64) -> Result<()> {
        // Todo quadro que a captura ou o encoder não entregam abre um buraco no tempo.
        // Avançar sempre `90000/fps` roubava esse buraco do vídeo enquanto o áudio
        // seguia em amostras reais: uma tela parada trinta segundos deixava a
        // transmissão trinta segundos fora de sincronia, sem volta.
        //
        // O avanço vai ANTES de empacotar. O packetizer soma depois de emitir, então
        // passar o intervalo lá dentro carimbaria o quadro seguinte com o buraco deste.
        let advance = match self.last_video_ns {
            Some(previous) if frame.timestamp_ns > previous => {
                let elapsed = u128::from(frame.timestamp_ns - previous);

                (elapsed * u128::from(VIDEO_CLOCK) / 1_000_000_000).min(u128::from(u32::MAX))
                    as u32
            }
            // Sem quadro anterior não há tempo decorrido. Se a captura não carimba a
            // hora, o fps nominal é o melhor palpite que existe.
            Some(_) => (VIDEO_CLOCK as f64 / frame_rate.max(1.0)).round() as u32,
            None => 0,
        };

        self.last_video_ns = Some(frame.timestamp_ns);
        self.video.skip_samples(advance);

        // Campos emprestados separadamente para o empacotador e o contexto SRTP poderem
        // ser mutáveis ao mesmo tempo — são campos distintos da mesma struct.
        Self::send(
            &self.socket,
            &mut self.srtp,
            self.video.as_mut(),
            Bytes::copy_from_slice(&frame.data),
            0,
            &mut self.dropped,
            &mut self.sent_bytes,
        )
    }

    /// Pacotes largados porque o buffer de saída estava cheio.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Bytes protegidos que o socket aceitou, vídeo e áudio somados.
    pub fn sent_bytes(&self) -> u64 {
        self.sent_bytes
    }

    /// O Opus chega em blocos fixos de 20 ms, então o relógio anda sempre o mesmo tanto.
    pub fn send_audio(&mut self, opus: &[u8]) -> Result<()> {
        let samples = SAMPLE_RATE / 1000 * FRAME_MS;

        Self::send(
            &self.socket,
            &mut self.srtp,
            self.audio.as_mut(),
            Bytes::copy_from_slice(opus),
            samples,
            &mut self.dropped,
            &mut self.sent_bytes,
        )
    }

    fn send(
        socket: &UdpSocket,
        srtp: &mut SrtpContext,
        packetizer: &mut dyn Packetizer,
        payload: Bytes,
        samples: u32,
        dropped: &mut u64,
        sent_bytes: &mut u64,
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

            match socket.send(&protected) {
                Ok(written) => *sent_bytes += written as u64,
                // Só chega aqui com o buffer do socket cheio, e depois do `SO_SNDBUF`
                // ampliado isso é uplink saturado de verdade. Largar o pacote é o preço
                // certo para vídeo ao vivo: dormir aqui segurava a thread da captura,
                // que é justamente quem produz o próximo quadro, e segurava junto o
                // mutex do destino.
                Err(error) if error.kind() == ErrorKind::WouldBlock => *dropped += 1,
                Err(error) => return Err(error).context("could not send RTP to the SFU"),
            }
        }

        Ok(())
    }
}

/// Quanto o socket pode ter em voo antes de recusar. Um quadro a 1080p60 sai em umas dez
/// mensagens, e um quadro-chave em algumas centenas; o padrão do Windows para datagrama
/// é 8 KB, ou seja, menos de um quadro. Era isso que largava 14% dos pacotes com o
/// uplink praticamente vazio, e o que travava a imagem de quem assistia a cada movimento
/// na tela. 4 MB é limite, não reserva: o sistema só usa o que precisa.
const SEND_BUFFER: usize = 4 * 1024 * 1024;

/// Amplia o buffer de saída do socket, se o sistema deixar.
///
/// Não é fatal: o sistema pode aparar o pedido, e transmitir com o buffer padrão é pior
/// do que com ele grande, mas ainda é melhor do que não transmitir. O tamanho que ficou
/// vai para o log porque é ele, e não o pedido, que explica perda de pacote depois.
fn grow_send_buffer(socket: &UdpSocket) {
    let socket = socket2::SockRef::from(socket);

    if let Err(error) = socket.set_send_buffer_size(SEND_BUFFER) {
        tracing::warn!(error = %error, "transporte: o buffer de saída ficou no padrão");

        return;
    }

    match socket.send_buffer_size() {
        Ok(size) => tracing::info!(bytes = size, "transporte: buffer de saída"),
        Err(error) => tracing::warn!(error = %error, "transporte: buffer de saída desconhecido"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um socket local no lugar do SFU, para o caminho inteiro — empacotar, proteger,
    /// enviar — ser exercitado de verdade em vez de simulado.
    fn listener() -> (UdpSocket, SocketAddr) {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("could not bind the listener");
        let address = socket.local_addr().expect("socket without an address");

        socket
            .set_read_timeout(Some(std::time::Duration::from_millis(500)))
            .expect("could not set the timeout");

        (socket, address)
    }

    #[test]
    fn key_must_match_the_suite_size() {
        let (_servidor, address) = listener();

        assert!(PlainSender::connect(address, &[0; 10]).is_err());
        assert!(PlainSender::connect(address, &PlainSender::generate_key()).is_ok());
    }

    #[test]
    fn generated_keys_do_not_repeat() {
        assert_ne!(PlainSender::generate_key(), PlainSender::generate_key());
    }

    #[test]
    fn a_big_frame_becomes_several_protected_packets() {
        let (server_socket, address) = listener();
        let key = PlainSender::generate_key();
        let mut sender = PlainSender::connect(address, &key).expect("could not connect");

        // Uma unidade NAL bem maior que a MTU: o empacotador precisa quebrá-la, e cada
        // pedaço tem de chegar protegido.
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

        while let Ok(size) = server_socket.recv(&mut buffer) {
            recebidos += 1;

            assert!(size <= MTU + 64, "packet above the MTU: {size}");
            // Payload protegido: o texto puro não pode aparecer na rede.
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
        // A barra da transmissão lê os Mb/s daqui: um contador parado mostraria 0,0 Mb/s
        // enquanto a tela sobe inteira.
        assert!(
            sender.sent_bytes() >= data.len() as u64,
            "sent bytes ({}) below the frame itself ({})",
            sender.sent_bytes(),
            data.len()
        );
    }

    #[test]
    fn audio_fits_one_packet_and_advances_the_clock() {
        let (server_socket, address) = listener();
        let mut sender =
            PlainSender::connect(address, &PlainSender::generate_key()).expect("could not connect");

        sender
            .send_audio(&[0x7F; 160])
            .expect("could not send audio");
        sender
            .send_audio(&[0x7F; 160])
            .expect("could not send audio");

        let mut buffer = [0u8; 2048];
        let mut timestamps = Vec::new();

        while let Ok(size) = server_socket.recv(&mut buffer) {
            // O carimbo de tempo do RTP fica nos bytes 4..8 e não é cifrado — o
            // cabeçalho viaja aberto para o outro lado reordenar antes de decifrar.
            timestamps.push(u32::from_be_bytes([
                buffer[4], buffer[5], buffer[6], buffer[7],
            ]));
            assert!(size > 160, "packet without header or auth tag: {size}");
        }

        assert_eq!(timestamps.len(), 2, "each 20 ms block is one packet");
        assert_eq!(
            timestamps[1].wrapping_sub(timestamps[0]),
            SAMPLE_RATE / 1000 * FRAME_MS,
            "the clock has to advance exactly one 20 ms block"
        );
    }

    /// O buraco de um quadro perdido tem de aparecer no relógio.
    ///
    /// Enquanto o avanço era fixo em `90000/fps`, todo quadro que não saía roubava
    /// 16,6 ms do vídeo e o áudio seguia em frente: uma tela parada por trinta segundos
    /// deixava a transmissão trinta segundos fora de sincronia, para sempre.
    #[test]
    fn dropped_frame_opens_a_gap_in_the_video_clock() {
        let (server_socket, address) = listener();
        let mut sender =
            PlainSender::connect(address, &PlainSender::generate_key()).expect("could not connect");

        let make_frame = |timestamp_ns| EncodedFrame {
            data: vec![0, 0, 0, 1, 0x41, 0xAB],
            keyframe: false,
            timestamp_ns,
        };

        // Três quadros de 60 fps de intervalo entre o primeiro e o segundo: dois deles
        // não chegaram a ser codificados.
        sender.send_frame(&make_frame(1_000_000_000), 60.0).expect("1");
        sender.send_frame(&make_frame(1_050_000_000), 60.0).expect("2");

        let mut buffer = [0u8; 2048];
        let mut timestamps = Vec::new();

        while server_socket.recv(&mut buffer).is_ok() {
            timestamps.push(u32::from_be_bytes([
                buffer[4], buffer[5], buffer[6], buffer[7],
            ]));
        }

        assert_eq!(timestamps.len(), 2);
        assert_eq!(
            timestamps[1].wrapping_sub(timestamps[0]),
            50 * VIDEO_CLOCK / 1000,
            "o relógio precisa andar os 50 ms de verdade, não um quadro nominal",
        );
    }
}
