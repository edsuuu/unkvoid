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

use std::collections::VecDeque;
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

/// Quantos pacotes de vídeo ficam guardados para reenvio. É a janela anti-repetição que o
/// mediasoup dá ao SRTP: pacote mais velho que isso ele recusaria de qualquer jeito. Em
/// 1080p60 cobre perto de meio segundo, e o pedido chega numa ida e volta (~140 ms).
const HISTORY: usize = 1024;

pub struct PlainSender {
    socket: UdpSocket,
    server: SocketAddr,
    srtp: SrtpContext,

    /// Para ABRIR o que o servidor manda de volta, que vem com a chave dele e não com a
    /// nossa. Sem isto o caminho de retorno era ruído: o pedido de quadro-chave chegava
    /// em todo buraco de pacote e ninguém conseguia sequer saber que ele existia.
    incoming: Option<SrtpContext>,
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

    /// Os últimos pacotes de vídeo já cifrados, com o número de sequência, para reenviar
    /// o que o servidor disser que não chegou.
    history: VecDeque<(u16, Bytes)>,
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
    pub fn connect(server: impl ToSocketAddrs, key: &[u8], server_key: Option<&[u8]>) -> Result<Self> {
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

        // Chave do servidor ausente é cliente falando com servidor antigo: a transmissão
        // sobe igual, só não há recuperação rápida de perda.
        let incoming = server_key.and_then(|key| {
            SrtpContext::new(
                &key[..KEY_LEN],
                &key[KEY_LEN..],
                ProtectionProfile::Aes128CmHmacSha1_80,
                None,
                None,
            )
            .ok()
        });

        Ok(Self {
            socket,
            server,
            srtp,
            incoming,
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
            history: VecDeque::with_capacity(HISTORY),
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
        let sent = Self::send(
            &self.socket,
            &mut self.srtp,
            self.video.as_mut(),
            Bytes::copy_from_slice(&frame.data),
            0,
            &mut self.dropped,
            &mut self.sent_bytes,
        )?;

        for packet in sent {
            if self.history.len() == HISTORY {
                self.history.pop_front();
            }

            self.history.push_back(packet);
        }

        Ok(())
    }

    /// Lê o que o servidor devolveu: reenvia na hora os pacotes de vídeo que ele diz não
    /// ter recebido, e diz se ele pediu um quadro-chave.
    ///
    /// O caminho até o servidor é a internet aberta, do Brasil aos EUA, e 1% de perda ali
    /// congelava quem assiste por segundos: o servidor pedia o pacote de volta (o `nack`
    /// de `rtp_parameters`), ninguém respondia, e cada buraco esperava um quadro-chave —
    /// que em algumas placas nem sai na hora. Reenviando, o buraco fecha numa ida e volta
    /// e o decodificador de quem assiste nem percebe. Os bytes são os mesmos já cifrados:
    /// mesmo índice e mesmo texto não reusam keystream.
    ///
    /// Não bloqueia: o socket é não-bloqueante e quem chama é a thread da captura, que
    /// não pode esperar por nada. Lê o que já chegou e volta.
    pub fn read_feedback(&mut self) -> bool {
        let Some(incoming) = self.incoming.as_mut() else {
            return false;
        };

        let mut buffer = [0_u8; 1500];
        let mut asked = false;

        while let Ok(size) = self.socket.recv(&mut buffer) {
            // Falha ao abrir é pacote de outra pessoa ou lixo da rede. Ignorar é o certo:
            // é justamente a autenticação do SRTCP que impede um estranho de nos fazer
            // gastar quadro-chave a cada pacote forjado.
            let Ok(plain) = incoming.decrypt_rtcp(&buffer[..size]) else {
                continue;
            };

            asked |= wants_keyframe(&plain);

            // ponytail: busca linear no histórico a cada pacote perdido, até 1024 passos.
            // Índice por número de sequência se isto aparecer no custo por quadro.
            for sequence in lost_video_packets(&plain) {
                if let Some((_, packet)) = self.history.iter().find(|(stored, _)| *stored == sequence)
                    && let Ok(written) = self.socket.send(packet)
                {
                    self.sent_bytes += written as u64;
                }
            }
        }

        asked
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
        .map(|_| ())
    }

    /// Devolve cada pacote que saiu, já cifrado e com o número de sequência — inclusive o
    /// que o buffer cheio largou, que é justamente o que o servidor vai pedir de volta.
    fn send(
        socket: &UdpSocket,
        srtp: &mut SrtpContext,
        packetizer: &mut dyn Packetizer,
        payload: Bytes,
        samples: u32,
        dropped: &mut u64,
        sent_bytes: &mut u64,
    ) -> Result<Vec<(u16, Bytes)>> {
        let packets = packetizer
            .packetize(&payload, samples)
            .map_err(|error| anyhow!("could not packetize: {error}"))?;

        let mut sent = Vec::with_capacity(packets.len());

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

            sent.push((packet.header.sequence_number, protected.freeze()));
        }

        Ok(sent)
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

/// Procura um pedido de quadro-chave num RTCP composto.
///
/// PLI e FIR são as duas formas de dizer a mesma coisa, e navegadores diferentes mandam
/// uma ou outra — atender só uma deixaria metade das pessoas congelada. O laço anda pelo
/// campo de comprimento de cada sub-pacote porque o pedido quase nunca vem sozinho: ele
/// costuma vir atrás de um relatório de recepção, no mesmo datagrama.
fn wants_keyframe(rtcp: &[u8]) -> bool {
    /// Payload-specific feedback, onde mora o PLI.
    const PSFB: u8 = 206;
    /// Full Intra Request no formato antigo, sozinho num pacote só dele.
    const LEGACY_FIR: u8 = 192;
    const PLI: u8 = 1;
    const FIR: u8 = 4;

    let mut rest = rtcp;

    while rest.len() >= 4 {
        let format = rest[0] & 0x1F;
        let kind = rest[1];
        // O campo conta palavras de 32 bits sem contar a primeira, então o pacote inteiro
        // tem (length + 1) * 4 bytes.
        let size = (usize::from(u16::from_be_bytes([rest[2], rest[3]])) + 1) * 4;

        if kind == LEGACY_FIR || (kind == PSFB && (format == PLI || format == FIR)) {
            return true;
        }

        if size == 0 || size > rest.len() {
            return false;
        }

        rest = &rest[size..];
    }

    false
}

/// Os números de sequência que um NACK genérico diz terem faltado no vídeo.
///
/// Cada entrada é o primeiro perdido e uma máscara de 16 bits com os seguintes: o bit `i`
/// ligado quer dizer que `primeiro + i + 1` também não chegou.
fn lost_video_packets(rtcp: &[u8]) -> Vec<u16> {
    /// Transport-layer feedback, onde mora o NACK.
    const RTPFB: u8 = 205;
    const GENERIC_NACK: u8 = 1;

    let mut lost = Vec::new();
    let mut rest = rtcp;

    while rest.len() >= 4 {
        let size = (usize::from(u16::from_be_bytes([rest[2], rest[3]])) + 1) * 4;

        if size > rest.len() {
            break;
        }

        let packet = &rest[..size];

        if packet[1] == RTPFB
            && packet[0] & 0x1F == GENERIC_NACK
            && size >= 16
            && u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]) == SSRC_VIDEO
        {
            for entry in packet[12..].as_chunks::<4>().0 {
                let first = u16::from_be_bytes([entry[0], entry[1]]);
                let mask = u16::from_be_bytes([entry[2], entry[3]]);

                lost.push(first);
                lost.extend(
                    (0..16_u16)
                        .filter(|bit| mask & (1 << bit) != 0)
                        .map(|bit| first.wrapping_add(bit + 1)),
                );
            }
        }

        rest = &rest[size..];
    }

    lost
}

#[cfg(test)]
mod tests {

    /// Um NACK atrás de um relatório de recepção, com máscara: é assim que o mediasoup
    /// pede. Errar a conta da máscara reenviaria o pacote errado e o buraco continuaria.
    #[test]
    fn a_nack_lists_the_lost_video_packets() {
        // RR vazio, e o NACK: FMT 1, PT 205, comprimento 3 (16 bytes no total).
        let mut packet = vec![0x80, 201, 0x00, 0x01, 0, 0, 0, 1];
        packet.extend_from_slice(&[0x81, 205, 0x00, 0x03, 0, 0, 0, 1]);
        packet.extend_from_slice(&SSRC_VIDEO.to_be_bytes());
        // Primeiro perdido 100, bits 0 e 2 ligados: faltaram também 101 e 103.
        packet.extend_from_slice(&[0, 100, 0, 0b101]);

        assert_eq!(lost_video_packets(&packet), vec![100, 101, 103]);

        packet[16..20].copy_from_slice(&SSRC_AUDIO.to_be_bytes());

        assert!(lost_video_packets(&packet).is_empty(), "NACK do áudio não é do vídeo");
    }

    /// O caminho inteiro da perda: o servidor manda o NACK cifrado com a chave dele, e o
    /// pacote que faltou volta idêntico ao que saiu.
    #[test]
    fn a_nacked_packet_is_sent_again() {
        let (server_socket, address) = listener();
        let server_key = PlainSender::generate_key();
        let mut sender = PlainSender::connect(address, &PlainSender::generate_key(), Some(&server_key))
            .expect("could not connect");

        sender
            .send_frame(
                &EncodedFrame {
                    data: vec![0, 0, 0, 1, 0x65, 0xAB],
                    keyframe: true,
                    timestamp_ns: 0,
                },
                60.0,
            )
            .expect("could not send the frame");

        let mut buffer = [0u8; 2048];
        let size = server_socket.recv(&mut buffer).expect("the frame did not arrive");
        let original = buffer[..size].to_vec();
        let sequence = u16::from_be_bytes([original[2], original[3]]);

        let mut nack = vec![0x81, 205, 0x00, 0x03, 0, 0, 0, 1];
        nack.extend_from_slice(&SSRC_VIDEO.to_be_bytes());
        nack.extend_from_slice(&sequence.to_be_bytes());
        nack.extend_from_slice(&[0, 0]);

        let protected = SrtpContext::new(
            &server_key[..KEY_LEN],
            &server_key[KEY_LEN..],
            ProtectionProfile::Aes128CmHmacSha1_80,
            None,
            None,
        )
        .expect("could not start the server SRTP")
        .encrypt_rtcp(&nack)
        .expect("could not protect the NACK");

        let port = sender.socket.local_addr().expect("sender without an address").port();

        server_socket
            .send_to(&protected, ("127.0.0.1", port))
            .expect("could not send the NACK");
        std::thread::sleep(std::time::Duration::from_millis(50));

        assert!(!sender.read_feedback(), "um NACK não é pedido de quadro-chave");

        let size = server_socket.recv(&mut buffer).expect("the lost packet was not sent again");

        assert_eq!(buffer[..size], original[..]);
    }

    /// Um PLI atrás de um relatório de recepção, que é como ele chega de verdade. Se o
    /// laço não andar pelo comprimento, este caso passa batido e a travada continua.
    #[test]
    fn a_pli_behind_a_receiver_report_is_found() {
        // RR vazio: versão 2, sem blocos, PT 201, comprimento 1 (8 bytes no total).
        let mut packet = vec![0x80, 201, 0x00, 0x01, 0, 0, 0, 1];
        // PLI: versão 2, FMT 1, PT 206, comprimento 2 (12 bytes no total).
        packet.extend_from_slice(&[0x81, 206, 0x00, 0x02, 0, 0, 0, 1, 0, 0, 0, 2]);

        assert!(wants_keyframe(&packet), "PLI depois de um RR não foi encontrado");
        assert!(!wants_keyframe(&packet[..8]), "um RR sozinho não pede quadro-chave");
    }

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

        assert!(PlainSender::connect(address, &[0; 10], None).is_err());
        assert!(PlainSender::connect(address, &PlainSender::generate_key(), None).is_ok());
    }

    #[test]
    fn generated_keys_do_not_repeat() {
        assert_ne!(PlainSender::generate_key(), PlainSender::generate_key());
    }

    #[test]
    fn a_big_frame_becomes_several_protected_packets() {
        let (server_socket, address) = listener();
        let key = PlainSender::generate_key();
        let mut sender = PlainSender::connect(address, &key, None).expect("could not connect");

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
            PlainSender::connect(address, &PlainSender::generate_key(), None).expect("could not connect");

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
            PlainSender::connect(address, &PlainSender::generate_key(), None).expect("could not connect");

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
