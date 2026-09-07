//! Captura de tela e áudio do sistema.
//!
//! O motivo de existir: no navegador o compartilhamento sempre carrega a barra do
//! Chrome, e no macOS o WKWebView nem oferece `getDisplayMedia`. Aqui a captura é
//! nativa, então não há barra nenhuma e o áudio do sistema funciona em qualquer OS.

use std::fmt;

mod source;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

pub use source::{Display, Window};

#[cfg(target_os = "macos")]
pub use macos::MacCapturer as PlatformCapturer;

#[cfg(target_os = "windows")]
pub use windows::WindowsCapturer as PlatformCapturer;

#[cfg(target_os = "linux")]
pub use linux::LinuxCapturer as PlatformCapturer;

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
    /// Teto de quadros. O piso real é responsabilidade do encoder e do transporte.
    pub frame_rate: u32,
    pub capture_audio: bool,
    pub show_cursor: bool,
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

/// O que sai da captura. Vídeo e áudio chegam separados de propósito: o encoder de
/// vídeo e o de áudio são independentes, e misturar aqui só atrapalharia.
pub enum CaptureEvent {
    Video(VideoFrame),
    Audio(AudioChunk),
}

pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    /// Nanossegundos desde o início da captura.
    pub timestamp_ns: u64,
}

pub struct AudioChunk {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: usize,
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
    #[error("nenhuma tela disponível para capturar")]
    NoDisplay,

    #[error("permissão de gravação de tela negada — libere em Ajustes do Sistema")]
    PermissionDenied,

    #[error("falha na captura: {0}")]
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
        // É o motivo de existir do app nativo: no navegador isso depende de OS e
        // versão. Se alguém desligar por engano, o teste acusa.
        let config = CaptureConfig::default();

        assert!(config.capture_audio);
        assert_eq!(config.quality, Quality::Hd1080);
        assert_eq!(config.frame_rate, 60);
    }
}
