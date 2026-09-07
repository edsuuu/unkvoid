use capture::AudioChunk;
use opus::{Application, Channels, Encoder};

use crate::EncoderError;

/// Opus a 48 kHz estéreo — o que o WebRTC espera e o que a captura entrega.
pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;

/// 20 ms por pacote: o padrão do WebRTC. Menor gera overhead de cabeçalho, maior
/// aumenta a latência percebida na conversa.
pub const FRAME_MS: u32 = 20;
const FRAME_SAMPLES: usize = (SAMPLE_RATE as usize / 1000) * FRAME_MS as usize * CHANNELS as usize;

/// Comprime o áudio do sistema em Opus.
///
/// A captura já entrega o som **sem o que sai do nosso próprio app** — quem filtra é
/// o sistema operacional, por processo. Sem isso, compartilhar áudio devolveria a
/// voz de quem está na chamada e criaria realimentação.
pub struct AudioEncoder {
    encoder: Encoder,
    pendente: Vec<f32>,
}

impl AudioEncoder {
    pub fn new(bitrate: i32) -> Result<Self, EncoderError> {
        let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Stereo, Application::Audio)
            .map_err(|erro| EncoderError::Start(erro.to_string()))?;

        encoder
            .set_bitrate(opus::Bitrate::Bits(bitrate))
            .map_err(|erro| EncoderError::Start(erro.to_string()))?;

        Ok(Self {
            encoder,
            pendente: Vec::with_capacity(FRAME_SAMPLES * 2),
        })
    }

    /// O Opus só aceita blocos de duração fixa, mas a captura entrega pedaços de
    /// tamanho variável. Sobra fica guardada para o próximo bloco.
    pub fn push(&mut self, chunk: &AudioChunk) -> Result<Vec<Vec<u8>>, EncoderError> {
        self.pendente.extend_from_slice(&chunk.samples);

        let mut pacotes = Vec::new();

        while self.pendente.len() >= FRAME_SAMPLES {
            let bloco: Vec<f32> = self.pendente.drain(..FRAME_SAMPLES).collect();
            let mut saida = vec![0u8; 4_000];

            let tamanho = self
                .encoder
                .encode_float(&bloco, &mut saida)
                .map_err(|erro| EncoderError::Encode(erro.to_string()))?;

            saida.truncate(tamanho);
            pacotes.push(saida);
        }

        Ok(pacotes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acumula_ate_fechar_um_bloco_de_20ms() {
        let mut encoder = AudioEncoder::new(64_000).expect("encoder");

        // Meio bloco não produz pacote: o Opus exige duração exata.
        let metade = AudioChunk {
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
            samples: vec![0.0; FRAME_SAMPLES / 2],
        };

        assert!(encoder.push(&metade).expect("push").is_empty());

        // A outra metade fecha o bloco e sai um pacote.
        assert_eq!(encoder.push(&metade).expect("push").len(), 1);
    }

    #[test]
    fn um_bloco_grande_vira_varios_pacotes() {
        let mut encoder = AudioEncoder::new(64_000).expect("encoder");

        let grande = AudioChunk {
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
            samples: vec![0.0; FRAME_SAMPLES * 3],
        };

        assert_eq!(encoder.push(&grande).expect("push").len(), 3);
    }
}
