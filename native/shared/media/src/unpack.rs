//! O caminho de volta do `plain.rs`: pacote RTP limpo vira quadro H.264 inteiro, e Opus
//! vira PCM.
//!
//! Existe para a interface que decodifica na própria máquina sem GStreamer (macOS e
//! Windows): o `PlainReceiver` entrega RTP aberto, isto entrega o que o decodificador do
//! sistema come. Nada aqui toca rede nem decodifica vídeo — só remonta.
//!
//! ponytail: sem fila de reordenação. Pacote fora de ordem conta como perda, o quadro cai
//! e a imagem espera o próximo keyframe. Se rede ruim pesar, entra um jitter buffer aqui.

use anyhow::{Result, anyhow};
use bytes::Bytes;
use opus::{Channels, Decoder};
use rtc::rtp::codec::h264::H264Packet;
use rtc::rtp::packet::Packet;
use rtc::rtp::packetizer::Depacketizer;
use rtc::shared::marshal::Unmarshal;

use crate::FRAME_MS;
use crate::audio::SAMPLE_RATE;

const NAL_IDR: u8 = 5;
const NAL_SPS: u8 = 7;

/// O maior bloco que um pacote Opus carrega: 120 ms em estéreo a 48 kHz.
const LONGEST_OPUS_BLOCK: usize = 5_760 * 2;

/// Um quadro completo em Annex-B, pronto para o decodificador.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessUnit {
    pub data: Vec<u8>,
    /// No relógio de 90 kHz do RTP.
    pub timestamp: u32,
    pub keyframe: bool,
}

#[derive(Default)]
pub struct VideoUnpacker {
    h264: H264Packet,
    frame: Vec<u8>,
    last_sequence: Option<u16>,
    /// Faltou pacote no quadro que está sendo montado.
    damaged: bool,
    /// Depois de uma perda, quadro que depende do anterior só desenharia lixo.
    waiting_keyframe: bool,
}

impl VideoUnpacker {
    /// Devolve o quadro quando o pacote que o fecha chega (o bit `marker`).
    ///
    /// O depacotador da `rtc` guarda o NAL fragmentado até o pedaço final, e só o larga quando
    /// ele chega: perdido o final, o resto grudava no próximo NAL fragmentado — que depois de um
    /// buraco costuma ser o IDR do quadro-chave pedido. Por isso ele recomeça a cada buraco e a
    /// cada início de fragmento.
    pub fn push(&mut self, packet: &[u8]) -> Option<AccessUnit> {
        let packet = Packet::unmarshal(&mut Bytes::copy_from_slice(packet)).ok()?;
        let sequence = packet.header.sequence_number;

        let gap = self.last_sequence.is_some_and(|last| last.wrapping_add(1) != sequence);

        if gap || starts_a_fragmented_nal(&packet.payload) {
            self.h264 = H264Packet::default();
        }

        if gap {
            self.damaged = true;
            self.waiting_keyframe = true;
        }

        self.last_sequence = Some(sequence);

        match whole_aggregate(&packet.payload).then(|| self.h264.depacketize(&packet.payload)) {
            Some(Ok(nals)) => self.frame.extend_from_slice(&nals),
            Some(Err(_)) | None => self.damaged = true,
        }

        if !packet.header.marker {
            return None;
        }

        let data = std::mem::take(&mut self.frame);
        let damaged = std::mem::take(&mut self.damaged);
        let keyframe = is_keyframe(&data);

        // O quadro furado não sai, e o seguinte depende dele.
        self.waiting_keyframe |= damaged;

        if damaged || data.is_empty() || (self.waiting_keyframe && !keyframe) {
            return None;
        }

        self.waiting_keyframe = false;

        Some(AccessUnit { data, timestamp: packet.header.timestamp, keyframe })
    }

    /// A imagem está parada à espera de um keyframe: é a hora de pedir um ao servidor.
    pub fn waiting_keyframe(&self) -> bool {
        self.waiting_keyframe
    }
}

/// Os NALs de um trecho em Annex-B, sem os códigos de início. Um NAL nunca termina em
/// zero (o RBSP fecha com um bit 1), então zero no fim é sobra do código de 4 bytes seguinte.
pub fn nals(annex_b: &[u8]) -> Vec<&[u8]> {
    let starts: Vec<usize> =
        annex_b.windows(3).enumerate().filter(|(_, window)| *window == [0, 0, 1]).map(|(index, _)| index).collect();

    starts
        .iter()
        .enumerate()
        .map(|(position, &start)| {
            let end = starts.get(position + 1).copied().unwrap_or(annex_b.len());
            let nal = &annex_b[start + 3..end];
            let padding = nal.iter().rev().take_while(|byte| **byte == 0).count();

            &nal[..nal.len() - padding]
        })
        .collect()
}

const NAL_STAP_A: u8 = 24;
const NAL_FU_A: u8 = 28;

/// O primeiro pedaço de um NAL fragmentado (FU-A com o bit de início).
fn starts_a_fragmented_nal(payload: &[u8]) -> bool {
    payload.len() >= 2 && payload[0] & 0x1F == NAL_FU_A && payload[1] & 0x80 != 0
}

/// Se cada NAL de um STAP-A cabe no pacote. O depacotador da `rtc` lê o comprimento seguinte
/// sem conferir que ele existe, e um pacote cortado (de um cliente modificado: o SFU repassa
/// sem olhar) derrubava a thread de quem assiste. O que não é STAP-A passa.
fn whole_aggregate(payload: &[u8]) -> bool {
    if payload.first().is_none_or(|header| header & 0x1F != NAL_STAP_A) {
        return true;
    }

    let mut rest = &payload[1..];

    while !rest.is_empty() {
        let Some(&[high, low]) = rest.first_chunk::<2>() else {
            return false;
        };
        let Some(after) = rest.get(2 + usize::from(u16::from_be_bytes([high, low]))..) else {
            return false;
        };

        rest = after;
    }

    true
}

fn is_keyframe(annex_b: &[u8]) -> bool {
    nals(annex_b).iter().any(|nal| nal.first().is_some_and(|header| matches!(header & 0x1F, NAL_IDR | NAL_SPS)))
}

/// Um bloco de 20 ms em estéreo, o que cada pacote do app carrega.
const OPUS_BLOCK: usize = (SAMPLE_RATE / 1000 * FRAME_MS) as usize * 2;

/// Até quantos blocos perdidos seguidos o Opus inventa. Mais que isso é quem manda que parou
/// (o fluxo pausou, a pessoa saiu), e som inventado por mais de 100 ms soa pior que silêncio.
const MOST_CONCEALED: u16 = 5;

/// Opus de um pacote RTP para PCM `f32` estéreo intercalado a 48 kHz.
///
/// Pacote que não chegou vira som estimado pelo próprio Opus, com o que veio antes, em vez de
/// um buraco: o buraco esvaziava a fila de quem toca, e cada volta dela é um estalo. É o que o
/// WebRTC do navegador faz no som de quem fala.
pub struct AudioUnpacker {
    decoder: Decoder,
    last: Option<u16>,
}

impl AudioUnpacker {
    pub fn new() -> Result<Self> {
        let decoder =
            Decoder::new(SAMPLE_RATE, Channels::Stereo).map_err(|error| anyhow!("o decodificador Opus não abriu: {error}"))?;

        Ok(Self { decoder, last: None })
    }

    pub fn push(&mut self, packet: &[u8]) -> Option<Vec<f32>> {
        let packet = Packet::unmarshal(&mut Bytes::copy_from_slice(packet)).ok()?;
        let sequence = packet.header.sequence_number;
        let missing = self.last.map_or(0, |last| sequence.wrapping_sub(last).wrapping_sub(1));

        // Atrasado ou repetido: o lugar dele já tocou, estimado.
        if missing >= u16::MAX / 2 {
            return None;
        }

        self.last = Some(sequence);

        let mut samples = Vec::new();

        if missing <= MOST_CONCEALED {
            for gap in 1..=missing {
                let mut block = vec![0.0; OPUS_BLOCK];
                // O último buraco pode vir de verdade dentro deste pacote (o FEC do Opus, quando
                // quem manda o liga); sem ele o decodificador estima, como nos outros.
                let carried: &[u8] = if gap == missing { &packet.payload } else { &[] };

                if let Ok(frames) = self.decoder.decode_float(carried, &mut block, gap == missing) {
                    block.truncate(frames * 2);
                    samples.extend(block);
                }
            }
        }

        let mut block = vec![0.0; LONGEST_OPUS_BLOCK];
        let frames = self.decoder.decode_float(&packet.payload, &mut block, false).ok()?;

        block.truncate(frames * 2);
        samples.extend(block);

        Some(samples)
    }
}

#[cfg(test)]
mod tests {
    use rtc::rtp::codec::h264::H264Payloader;
    use rtc::rtp::header::Header;
    use rtc::rtp::packetizer::Payloader;
    use rtc::shared::marshal::Marshal;

    use super::*;

    const MTU: usize = 1200;

    fn frame(nal_type: u8, size: usize) -> Vec<u8> {
        let mut data = vec![0, 0, 0, 1, 0x67, 1, 2, 3, 0, 0, 0, 1, 0x68, 4, 5, 0, 0, 0, 1, nal_type];

        data.extend((0..size).map(|index| (index % 251) as u8 + 1));

        data
    }

    fn packets(payloader: &mut H264Payloader, annex_b: &[u8], first_sequence: u16, timestamp: u32) -> Vec<Vec<u8>> {
        let payloads = payloader.payload(MTU, &Bytes::copy_from_slice(annex_b)).expect("payload");
        let last = payloads.len() - 1;

        payloads
            .into_iter()
            .enumerate()
            .map(|(index, payload)| {
                let packet = Packet {
                    header: Header {
                        version: 2,
                        marker: index == last,
                        payload_type: 102,
                        sequence_number: first_sequence.wrapping_add(index as u16),
                        timestamp,
                        ssrc: 7,
                        ..Header::default()
                    },
                    payload,
                };

                packet.marshal().expect("marshal").to_vec()
            })
            .collect()
    }

    #[test]
    fn a_fragmented_keyframe_comes_back_whole() {
        let sent = frame(0x65, 5_000);
        let mut unpacker = VideoUnpacker::default();
        let sent_packets = packets(&mut H264Payloader::default(), &sent, 65_534, 9_000);

        assert!(sent_packets.len() > 3, "o quadro tem de ter sido fragmentado");

        let got: Vec<AccessUnit> = sent_packets.iter().filter_map(|packet| unpacker.push(packet)).collect();

        assert_eq!(got.len(), 1);
        assert!(got[0].keyframe);
        assert_eq!(got[0].timestamp, 9_000);
        assert_eq!(nals(&got[0].data), nals(&sent));
    }

    #[test]
    fn after_a_lost_packet_the_picture_waits_for_the_next_keyframe() {
        let mut payloader = H264Payloader::default();
        let mut unpacker = VideoUnpacker::default();
        let mut damaged = packets(&mut payloader, &frame(0x65, 5_000), 10, 0);
        let next = 10 + damaged.len() as u16;

        damaged.remove(2);

        assert!(damaged.iter().filter_map(|packet| unpacker.push(packet)).next().is_none(), "quadro furado não sai");
        assert!(unpacker.waiting_keyframe());

        let delta = packets(&mut payloader, &[0, 0, 0, 1, 0x41, 9, 9, 9], next, 3_000);

        assert!(delta.iter().filter_map(|packet| unpacker.push(packet)).next().is_none(), "quadro que depende do furado não sai");

        let key = packets(&mut payloader, &frame(0x65, 100), next + 1, 6_000);

        assert!(key.iter().filter_map(|packet| unpacker.push(packet)).next().is_some_and(|unit| unit.keyframe));
        assert!(!unpacker.waiting_keyframe());
    }

    /// O fim de um NAL fragmentado se perde e a recuperação desiste: o pedaço que ficou não pode
    /// grudar no NAL do quadro-chave que vem depois. Grudado, o quadro-chave saía como quadro-chave
    /// mas com o IDR podre, e a imagem ficava em lixo até o periódico seguinte.
    #[test]
    fn a_lost_fragment_end_does_not_rot_the_next_keyframe() {
        let mut payloader = H264Payloader::default();
        let mut unpacker = VideoUnpacker::default();
        let mut big_delta = packets(&mut payloader, &[[0, 0, 0, 1, 0x41].as_slice(), &[7; 4_000]].concat(), 100, 0);
        let next = 100 + big_delta.len() as u16;

        big_delta.pop();

        assert!(big_delta.iter().filter_map(|packet| unpacker.push(packet)).next().is_none());

        let small_delta = packets(&mut payloader, &[0, 0, 0, 1, 0x41, 9, 9, 9], next, 3_000);

        assert!(small_delta.iter().filter_map(|packet| unpacker.push(packet)).next().is_none(), "o quadro depois do buraco não sai");

        let sent = frame(0x65, 5_000);
        let key = packets(&mut payloader, &sent, next + 1, 6_000);
        let got = key.iter().filter_map(|packet| unpacker.push(packet)).next().expect("o quadro-chave saiu");

        assert!(got.keyframe);
        assert_eq!(nals(&got.data), nals(&sent), "o quadro-chave saiu com o resto do NAL perdido grudado");
    }

    /// Um STAP-A com o comprimento cortado no meio fazia o depacotador da `rtc` ler além do pacote
    /// e derrubar a thread de quem assiste: a tela daquela pessoa parava para sempre.
    #[test]
    fn a_torn_aggregate_is_a_damaged_frame_and_not_a_panic() {
        let mut unpacker = VideoUnpacker::default();
        let torn = Packet {
            header: Header { version: 2, marker: true, payload_type: 102, sequence_number: 1, ssrc: 7, ..Header::default() },
            payload: Bytes::from_static(&[0x78, 0x00, 0x01, 0x65, 0x00]),
        };

        assert!(unpacker.push(&torn.marshal().expect("marshal")).is_none());

        let key = packets(&mut H264Payloader::default(), &frame(0x65, 100), 2, 3_000);

        assert!(key.iter().filter_map(|packet| unpacker.push(packet)).next().is_some_and(|unit| unit.keyframe), "depois do pacote torto o quadro-chave sai");
    }

    #[test]
    fn opus_comes_back_as_stereo_pcm() {
        let mut encoder = crate::AudioEncoder::new(48_000).expect("encoder");
        let tone: Vec<f32> = (0..1920).map(|index| (index as f32 * 0.05).sin() * 0.5).collect();
        let block = capture::AudioChunk { sample_rate: 48_000, channels: 2, samples: tone };
        let opus = encoder.push(&block).expect("push").pop().expect("um pacote por bloco de 20 ms");

        let packet = Packet {
            header: Header { version: 2, payload_type: 111, ..Header::default() },
            payload: Bytes::from(opus),
        };

        let samples = AudioUnpacker::new().expect("decoder").push(&packet.marshal().expect("marshal")).expect("pcm");

        assert_eq!(samples.len(), 1920, "20 ms em estéreo");
    }

    /// A voz sai com FEC, e o bloco que se perdeu volta refeito pelo pacote seguinte em vez de
    /// virar silêncio.
    #[test]
    fn a_lost_voice_packet_comes_back_from_the_next_one() {
        let tone: Vec<f32> = (0..1920 * 4).map(|index| (index as f32 * 0.03).sin() * 0.5).collect();
        let block = capture::AudioChunk { sample_rate: 48_000, channels: 2, samples: tone };
        let packets = |mut encoder: crate::AudioEncoder| -> Vec<Vec<u8>> {
            encoder
                .push(&block)
                .expect("push")
                .into_iter()
                .enumerate()
                .map(|(index, opus)| {
                    let header = Header { version: 2, payload_type: 111, sequence_number: index as u16, ..Header::default() };

                    Packet { header, payload: Bytes::from(opus) }.marshal().expect("marshal").to_vec()
                })
                .collect()
        };
        let energy = |samples: &[f32]| samples.iter().map(|sample| sample * sample).sum::<f32>();
        let refilled = |packets: &[Vec<u8>]| {
            let mut unpacker = AudioUnpacker::new().expect("decoder");

            unpacker.push(&packets[0]).expect("pcm");
            unpacker.push(&packets[1]).expect("pcm");

            let pcm = unpacker.push(&packets[3]).expect("pcm");

            energy(&pcm[..1920])
        };
        let voice = refilled(&packets(crate::AudioEncoder::for_voice(48_000).expect("encoder")));

        assert!(voice > 1.0, "o bloco perdido voltou mudo: energia {voice}");
    }

    /// O bloco que não chegou sai estimado junto com o seguinte, e o atrasado não toca de novo.
    #[test]
    fn a_lost_opus_packet_is_filled_and_a_late_one_is_dropped() {
        let mut encoder = crate::AudioEncoder::new(48_000).expect("encoder");
        let mut unpacker = AudioUnpacker::new().expect("decoder");
        let tone: Vec<f32> = (0..1920 * 3).map(|index| (index as f32 * 0.05).sin() * 0.5).collect();
        let block = capture::AudioChunk { sample_rate: 48_000, channels: 2, samples: tone };
        let packets: Vec<Vec<u8>> = encoder
            .push(&block)
            .expect("push")
            .into_iter()
            .enumerate()
            .map(|(index, opus)| {
                let header = Header { version: 2, payload_type: 111, sequence_number: 65_535_u16.wrapping_add(index as u16), ..Header::default() };

                Packet { header, payload: Bytes::from(opus) }.marshal().expect("marshal").to_vec()
            })
            .collect();

        assert_eq!(unpacker.push(&packets[0]).expect("pcm").len(), 1920);
        assert_eq!(unpacker.push(&packets[2]).expect("pcm").len(), 1920 * 2, "o perdido e o que chegou");
        assert!(unpacker.push(&packets[1]).is_none(), "o atrasado já tocou estimado");
    }
}
