//! Captura de tela e áudio do sistema.
//!
//! O motivo de existir: no navegador o compartilhamento sempre carrega a barra do
//! Chrome, e no macOS o WKWebView nem oferece `getDisplayMedia`. Aqui a captura é
//! nativa, então não há barra nenhuma e o áudio do sistema funciona em qualquer OS.

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

/// Uma tela inteira disponível para captura.
#[derive(Debug, Clone)]
pub struct Display {
    pub id: u32,
    pub width: u32,
    pub height: u32,
}

/// Uma janela específica. Compartilhar janela evita mostrar o que não devia.
#[derive(Debug, Clone)]
pub struct Window {
    pub id: u32,
    pub title: String,
    pub application: String,
}

/// Buffer de GPU específico da plataforma.
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
    /// Teto de quadros. O piso real é responsabilidade do encoder e do transporte.
    pub frame_rate: u32,
    pub capture_audio: bool,
    pub show_cursor: bool,
}

impl CaptureConfig {
    /// O que sai do nosso próprio app **nunca** entra na captura.
    ///
    /// Sem isso, compartilhar áudio do sistema capturaria a voz de quem está na
    /// chamada e devolveria para eles — o clássico loop de realimentação. Quem
    /// resolve é o sistema operacional, filtrando por processo: mais confiável que
    /// tentar adivinhar no nosso código de onde o som veio.
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

    /// Buffer da GPU com o quadro. Vai direto para o encoder por hardware, sem
    /// cópia para a CPU — é o que permite 1440p60 sem derreter a máquina.
    ///
    /// O campo existe em toda plataforma para o app compilar em todas; só o tipo
    /// dentro dele muda. Fora do macOS ainda vem sempre vazio.
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
