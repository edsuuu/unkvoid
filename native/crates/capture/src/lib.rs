//! Screen and system audio capture.
//!
//! The reason this exists: browser sharing always includes Chrome's bar, and on macOS
//! WKWebView does not even provide `getDisplayMedia`. Capture is native here, so
//! there is no bar and system audio works on every OS.

use std::fmt;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
pub use macos::MacCapturer as PlatformCapturer;

#[cfg(target_os = "windows")]
pub use windows::WindowsCapturer as PlatformCapturer;

#[cfg(target_os = "linux")]
pub use linux::LinuxCapturer as PlatformCapturer;

/// A full screen available for capture.
#[derive(Debug, Clone)]
pub struct Display {
    pub id: u32,
    pub width: u32,
    pub height: u32,
}

/// A specific window. Sharing a window avoids showing what should remain private.
#[derive(Debug, Clone)]
pub struct Window {
    pub id: u32,
    pub title: String,
    pub application: String,
}

/// Platform-specific GPU buffer.
#[cfg(target_os = "macos")]
pub type GpuSurface = apple_cf::iosurface::IOSurface;

#[cfg(not(target_os = "macos"))]
pub type GpuSurface = ();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Hd720,
    Hd1080,
    Qhd1440,
}

impl Quality {
    pub fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Hd720 => (1280, 720),
            Self::Hd1080 => (1920, 1080),
            Self::Qhd1440 => (2560, 1440),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub quality: Quality,
    /// Frame-rate ceiling. The actual floor is the encoder and transport's responsibility.
    pub frame_rate: u32,
    pub capture_audio: bool,
    pub show_cursor: bool,
}

impl CaptureConfig {
    /// Output from our own app **never** enters the capture.
    ///
    /// Without this, sharing system audio would capture the voice of someone in
    /// the call and send it back to them — the classic feedback loop. The
    /// operating system handles this by filtering per process, more reliably
    /// than trying to guess in our code where the sound came from.
    pub const EXCLUI_AUDIO_DO_APP: bool = true;
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            quality: Quality::Hd1080,
            frame_rate: 60,
            capture_audio: true,
            show_cursor: true,
        }
    }
}

/// Output from capture. Video and audio are intentionally separate: their
/// encoders are independent, and combining them here would only get in the way.
pub enum CaptureEvent {
    Video(VideoFrame),
    Audio(AudioChunk),
}

pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    /// Nanoseconds since capture began.
    pub timestamp_ns: u64,

    /// GPU buffer containing the frame. It goes directly to the hardware encoder
    /// without a CPU copy — this is what makes 1440p60 possible without overload.
    ///
    /// This field exists on every platform so the app compiles everywhere; only
    /// the type inside it changes. Outside macOS it is always empty for now.
    pub surface: Option<GpuSurface>,
}

pub struct AudioChunk {
    pub sample_rate: u32,
    pub channels: u16,
    /// Amostras intercaladas (L, R, L, R...) em ponto flutuante, como o WebRTC quer.
    pub samples: Vec<f32>,
}

impl AudioChunk {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }
}

impl fmt::Debug for VideoFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "VideoFrame({}x{} @{}ns)",
            self.width, self.height, self.timestamp_ns
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("no screen available to capture")]
    NoDisplay,

    #[error("screen recording permission denied — enable it in System Settings")]
    PermissionDenied,

    #[error("capture failed: {0}")]
    Platform(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualidade_mapeia_para_as_resolucoes_combinadas() {
        assert_eq!(Quality::Hd720.dimensions(), (1280, 720));
        assert_eq!(Quality::Hd1080.dimensions(), (1920, 1080));
        assert_eq!(Quality::Qhd1440.dimensions(), (2560, 1440));
    }

    #[test]
    fn padrao_captura_audio_do_sistema() {
        // This is why the native app exists: in a browser this depends on the OS
        // and version. If someone disables it accidentally, the test reports it.
        let config = CaptureConfig::default();

        assert!(config.capture_audio);
        assert_eq!(config.quality, Quality::Hd1080);
        assert_eq!(config.frame_rate, 60);
    }
}
