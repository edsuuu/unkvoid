use apple_cf::iosurface::IOSurface;
use videotoolbox::prelude::*;

use crate::{EncodedFrame, EncoderConfig, EncoderError};

/// Encoder H.264 por hardware. No Apple Silicon roda no media engine — a CPU só
/// entrega o buffer e recebe os bytes de volta.
pub struct VideoToolboxEncoder {
    session: CompressionSession,
    frame_rate: f64,
    frames: u64,
}

impl VideoToolboxEncoder {
    pub fn new(config: &EncoderConfig) -> Result<Self, EncoderError> {
        let (width, height) = config.quality.dimensions();

        let session = CompressionSession::builder(width as i32, height as i32, Codec::H264)
            // Tempo real: prioriza latência baixa sobre taxa de compressão.
            .with_real_time(true)
            // Sem B-frames. Eles comprimem melhor, mas exigem reordenar quadros, o
            // que adiciona latência — inaceitável numa chamada.
            .with_allow_frame_reordering(false)
            .with_average_bit_rate(config.bitrate as i32)
            .with_expected_frame_rate(config.frame_rate)
            // Keyframe a cada 2s: quem entra no meio da transmissão não espera muito,
            // e não gasta banda mandando quadro completo toda hora.
            .with_max_keyframe_interval((config.frame_rate * 2.0) as i32)
            .build()
            .map_err(|error| EncoderError::Start(error.to_string()))?;

        Ok(Self {
            session,
            frame_rate: config.frame_rate,
            frames: 0,
        })
    }

    /// Codifica um quadro. `surface` vem direto da captura, sem passar pela CPU.
    pub fn encode(
        &mut self,
        surface: &IOSurface,
        timestamp_ns: u64,
    ) -> Result<EncodedFrame, EncoderError> {
        let escala = self.frame_rate as i64;
        let apresentacao = (self.frames as i64, escala as i32);

        self.frames += 1;

        let codificado = self
            .session
            .encode(surface, apresentacao)
            .map_err(|error| EncoderError::Encode(error.to_string()))?;

        Ok(EncodedFrame {
            keyframe: is_keyframe(&codificado.data),
            data: codificado.data,
            timestamp_ns,
        })
    }
}

/// Um quadro-chave em H.264 carrega SPS (tipo 7), PPS (8) ou IDR (5). Olhar o tipo
/// do primeiro NAL é suficiente e não custa nada.
fn is_keyframe(data: &[u8]) -> bool {
    let mut posicao = 0;

    while posicao + 4 < data.len() {
        let tamanho = u32::from_be_bytes([
            data[posicao],
            data[posicao + 1],
            data[posicao + 2],
            data[posicao + 3],
        ]) as usize;
        let tipo = data.get(posicao + 4).map(|byte| byte & 0x1F);

        if matches!(tipo, Some(5 | 7 | 8)) {
            return true;
        }

        posicao += 4 + tamanho;
    }

    false
}
