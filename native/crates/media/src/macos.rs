use apple_cf::iosurface::IOSurface;
use videotoolbox::prelude::*;

use crate::{EncodedFrame, EncoderConfig, EncoderError};

/// Hardware H.264 encoder. On Apple Silicon it runs on the media engine — the CPU
/// only supplies the buffer and receives the bytes back.
pub struct VideoToolboxEncoder {
    session: CompressionSession,
    frame_rate: f64,
    frames: u64,
}

impl VideoToolboxEncoder {
    pub fn new(config: &EncoderConfig) -> Result<Self, EncoderError> {
        let (width, height) = config.quality.dimensions();

        let session = CompressionSession::builder(width as i32, height as i32, Codec::H264)
            // Real time: prioritize low latency over compression ratio.
            .with_real_time(true)
            // No B-frames. They compress better, but require reordering frames, which
            // adds latency — unacceptable in a call.
            .with_allow_frame_reordering(false)
            .with_average_bit_rate(config.bitrate as i32)
            .with_expected_frame_rate(config.frame_rate)
            // Keyframe every 2s: someone joining mid-broadcast does not wait long, and
            // bandwidth is not wasted sending a full frame constantly.
            .with_max_keyframe_interval((config.frame_rate * 2.0) as i32)
            .build()
            .map_err(|error| EncoderError::Start(error.to_string()))?;

        Ok(Self {
            session,
            frame_rate: config.frame_rate,
            frames: 0,
        })
    }

    /// Encodes a frame. `surface` comes directly from capture without passing through the CPU.
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

/// An H.264 keyframe carries SPS (type 7), PPS (8), or IDR (5). Checking the
/// first NAL type is sufficient and costs nothing.
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
