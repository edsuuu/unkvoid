//! Video encoding and transport.
//!
//! Encoding runs on the **media chip**, not the CPU: the frame leaves capture as a
//! GPU buffer and goes directly to the encoder without a copy. This is what enables
//! 1440p60 without overloading the broadcaster's machine.

use capture::Quality;

mod audio;
mod peer;
mod plain;

#[cfg(target_os = "macos")]
mod macos;

pub use audio::{AudioEncoder, FRAME_MS};
pub use peer::{PeerLink, Signal};
pub use plain::PlainSender;

#[cfg(target_os = "macos")]
pub use macos::VideoToolboxEncoder as PlatformEncoder;

/// The GPU buffer received by the encoder. On macOS this is a real `IOSurface`;
/// on other platforms it is a marker until an encoder exists there.
#[cfg(target_os = "macos")]
pub type GpuSurface = apple_cf::iosurface::IOSurface;

#[cfg(not(target_os = "macos"))]
pub type GpuSurface = ();

/// An already-compressed frame, ready to become an RTP packet.
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub keyframe: bool,
    pub timestamp_ns: u64,
}

#[derive(Debug, Clone)]
pub struct EncoderConfig {
    pub quality: Quality,
    pub frame_rate: f64,
    pub bitrate: u32,
}

impl EncoderConfig {
    pub fn for_quality(quality: Quality) -> Self {
        // Same values as the web app, where they have already been calibrated.
        let bitrate = match quality {
            Quality::Hd720 => 4_000_000,
            Quality::Hd1080 => 7_000_000,
            Quality::Qhd1440 => 12_000_000,
        };

        Self {
            quality,
            frame_rate: 60.0,
            bitrate,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EncoderError {
    #[error("hardware encoder refused to start: {0}")]
    Start(String),

    #[error("failed to encode frame: {0}")]
    Encode(String),

    #[error("frame arrived without a GPU buffer — nothing to encode")]
    NoSurface,

    #[error("video encoding is not implemented on this platform yet")]
    Unsupported,
}

/// Outside macOS there is no hardware encoder yet. The stub has the **same shape**
/// as the real implementation so the app compiles and fails with a clear message
/// instead of failing to compile — `.msi` and `.deb` can ship and the rest works.
#[cfg(not(target_os = "macos"))]
pub struct PlatformEncoder;

#[cfg(not(target_os = "macos"))]
impl PlatformEncoder {
    pub fn new(_config: &EncoderConfig) -> Result<Self, EncoderError> {
        Err(EncoderError::Unsupported)
    }

    pub fn encode(
        &mut self,
        _surface: &GpuSurface,
        _timestamp_ns: u64,
    ) -> Result<EncodedFrame, EncoderError> {
        Err(EncoderError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_sobe_junto_com_a_resolucao() {
        let baixo = EncoderConfig::for_quality(Quality::Hd720).bitrate;
        let medio = EncoderConfig::for_quality(Quality::Hd1080).bitrate;
        let alto = EncoderConfig::for_quality(Quality::Qhd1440).bitrate;

        assert!(baixo < medio && medio < alto);
    }

    #[test]
    fn encoder_mira_60_fps() {
        assert_eq!(EncoderConfig::for_quality(Quality::Hd1080).frame_rate, 60.0);
    }
}
