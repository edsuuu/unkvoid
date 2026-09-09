use capture::AudioChunk;
use opus::{Application, Channels, Encoder};

use crate::EncoderError;

/// Opus at 48 kHz stereo — what WebRTC expects and capture provides.
pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;

/// 20 ms per packet: WebRTC's standard. Smaller adds header overhead; larger
/// increases perceived conversation latency.
pub const FRAME_MS: u32 = 20;
const FRAME_SAMPLES: usize = (SAMPLE_RATE as usize / 1000) * FRAME_MS as usize * CHANNELS as usize;

/// Compresses system audio into Opus.
///
/// Capture already provides audio **without our own app's output** — the operating
/// system filters it per process. Without this, sharing audio would send back a
/// caller's voice and create feedback.
pub struct AudioEncoder {
    encoder: Encoder,
    pending: Vec<f32>,
}

impl AudioEncoder {
    pub fn new(bitrate: i32) -> Result<Self, EncoderError> {
        let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Stereo, Application::Audio)
            .map_err(|error| EncoderError::Start(error.to_string()))?;

        encoder
            .set_bitrate(opus::Bitrate::Bits(bitrate))
            .map_err(|error| EncoderError::Start(error.to_string()))?;

        Ok(Self {
            encoder,
            pending: Vec::with_capacity(FRAME_SAMPLES * 2),
        })
    }

    /// Opus accepts only fixed-duration blocks, but capture provides variable-sized
    /// chunks. Remainders are saved for the next block.
    pub fn push(&mut self, chunk: &AudioChunk) -> Result<Vec<Vec<u8>>, EncoderError> {
        self.pending.extend_from_slice(&chunk.samples);

        let mut packets = Vec::new();

        while self.pending.len() >= FRAME_SAMPLES {
            let block: Vec<f32> = self.pending.drain(..FRAME_SAMPLES).collect();
            let mut out = vec![0u8; 4_000];

            let size = self
                .encoder
                .encode_float(&block, &mut out)
                .map_err(|error| EncoderError::Encode(error.to_string()))?;

            out.truncate(size);
            packets.push(out);
        }

        Ok(packets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_until_a_20ms_block_closes() {
        let mut encoder = AudioEncoder::new(64_000).expect("encoder");

        // Half a block produces no packet: Opus requires an exact duration.
        let metade = AudioChunk {
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
            samples: vec![0.0; FRAME_SAMPLES / 2],
        };

        assert!(encoder.push(&metade).expect("push").is_empty());

        // The other half completes the block and produces a packet.
        assert_eq!(encoder.push(&metade).expect("push").len(), 1);
    }

    #[test]
    fn a_big_block_becomes_several_packets() {
        let mut encoder = AudioEncoder::new(64_000).expect("encoder");

        let grande = AudioChunk {
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
            samples: vec![0.0; FRAME_SAMPLES * 3],
        };

        assert_eq!(encoder.push(&grande).expect("push").len(), 3);
    }
}
