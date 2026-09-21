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
    pub fn push(&mut self, packet: &[u8]) -> Option<AccessUnit> {
        let packet = Packet::unmarshal(&mut Bytes::copy_from_slice(packet)).ok()?;
        let sequence = packet.header.sequence_number;

        if self.last_sequence.is_some_and(|last| last.wrapping_add(1) != sequence) {
            self.damaged = true;
            self.waiting_keyframe = true;
        }

        self.last_sequence = Some(sequence);

        match self.h264.depacketize(&packet.payload) {
            Ok(nals) => self.frame.extend_from_slice(&nals),
            Err(_) => self.damaged = true,
        }

        if !packet.header.marker {
            return None;
        }

        let data = std::mem::take(&mut self.frame);
        let damaged = std::mem::take(&mut self.damaged);
        let keyframe = is_keyframe(&data);

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

fn is_keyframe(annex_b: &[u8]) -> bool {
    nals(annex_b).iter().any(|nal| nal.first().is_some_and(|header| matches!(header & 0x1F, NAL_IDR | NAL_SPS)))
}

/// Opus de um pacote RTP para PCM `f32` estéreo intercalado a 48 kHz.
pub struct AudioUnpacker {
    decoder: Decoder,
}

impl AudioUnpacker {
    pub fn new() -> Result<Self> {
        let decoder =
            Decoder::new(SAMPLE_RATE, Channels::Stereo).map_err(|error| anyhow!("o decodificador Opus não abriu: {error}"))?;

        Ok(Self { decoder })
    }

    pub fn push(&mut self, packet: &[u8]) -> Option<Vec<f32>> {
        let packet = Packet::unmarshal(&mut Bytes::copy_from_slice(packet)).ok()?;
        let mut samples = vec![0.0; LONGEST_OPUS_BLOCK];
        let frames = self.decoder.decode_float(&packet.payload, &mut samples, false).ok()?;

        samples.truncate(frames * 2);

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
}
