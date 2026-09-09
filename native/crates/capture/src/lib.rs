//! Captura de tela e do som do sistema.
//!
//! O motivo de existir: compartilhar pelo navegador leva sempre a barra do Chrome
//! junto, e no macOS o WKWebView nem oferece `getDisplayMedia`. Aqui a captura é
//! nativa, então não há barra e o som do sistema funciona em todo sistema.

use std::fmt;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
mod windows_audio;

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

/// Uma janela específica. Compartilhar só ela evita mostrar o que era para ficar privado.
#[derive(Debug, Clone)]
pub struct Window {
    /// Identificador do sistema. É `u64` porque no Windows ele é um `HWND`, que é um
    /// ponteiro — no macOS é um `CGWindowID` de 32 bits e sobra espaço.
    pub id: u64,
    pub title: String,
    pub application: String,
}

/// Platform-specific GPU buffer.
#[cfg(target_os = "macos")]
pub type GpuSurface = apple_cf::iosurface::IOSurface;

/// No Windows o quadro não é um buffer solto: é uma textura do Direct3D que pertence à
/// rotação interna da captura, e só o device que a criou sabe lê-la. Por isso os três
/// andam juntos — o encoder precisa do device e do contexto para copiar a textura antes
/// que o próximo quadro a reaproveite.
///
/// O `::` na frente não é enfeite: este crate tem um módulo chamado `windows`, e sem ele
/// o caminho acha o módulo local em vez da crate da Microsoft.
#[cfg(target_os = "windows")]
#[derive(Clone)]
pub struct GpuSurface {
    pub texture: ::windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    pub device: ::windows::Win32::Graphics::Direct3D11::ID3D11Device,
    pub context: ::windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

/// O que transmitir.
///
/// A escolha vem da interface, e não é detalhe: transmitir a tela inteira quando a
/// pessoa queria só o jogo mostra e-mail, senha e conversa para a sala toda.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CaptureSource {
    /// O monitor principal — o que a maioria quer, e o que não exige escolher nada.
    #[default]
    PrimaryDisplay,
    Display(u32),
    Window(u64),
}

#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub quality: Quality,
    pub source: CaptureSource,
    /// Teto de quadros por segundo. O piso de verdade é problema do encoder e do transporte.
    pub frame_rate: u32,
    pub capture_audio: bool,

    /// Se o som dos aplicativos de `MUTED_APPS` deve ficar de fora da transmissão.
    ///
    /// É escolha de quem transmite, no momento de escolher o que compartilhar: quem usa
    /// o Discord para conversar quase nunca quer a conversa junto, mas quem está
    /// mostrando o próprio Discord para alguém quer.
    pub mute_listed_apps: bool,
    pub show_cursor: bool,
}

impl CaptureConfig {
    /// O som que o próprio app toca **nunca** entra na captura.
    ///
    /// Sem isto, compartilhar o áudio do sistema gravaria a voz de quem está na
    /// chamada e a devolveria para ela — a realimentação clássica. Quem resolve é o
    /// sistema operacional, filtrando por processo, o que é mais confiável do que
    /// tentar adivinhar aqui de onde veio cada som.
    pub const EXCLUI_AUDIO_DO_APP: bool = true;

    /// Aplicativos cujo som nunca sobe junto com a tela, identificados pelo bundle.
    ///
    /// Quem compartilha aqui quase sempre está falando pelo Discord ao mesmo tempo. Sem
    /// isto, a voz de todo mundo da chamada de lá entra na transmissão: quem está nos
    /// dois lugares ouve cada pessoa duas vezes, a segunda com o atraso do salto pelo
    /// servidor. O sistema filtra por processo, que é mais confiável do que tentar
    /// adivinhar aqui de onde veio cada som.
    ///
    /// No macOS o filtro do ScreenCaptureKit é um só para vídeo e áudio, então a janela
    /// do app silenciado também sai da imagem. Para o Discord isso é ganho duplo: a
    /// conversa privada não vaza para a sala.
    ///
    /// No Windows esta lista ainda não vale: o laço por processo do WASAPI exclui uma
    /// árvore só, e ela já é a nossa — ver o cabeçalho de `windows_audio.rs`.
    pub const MUTED_APPS: &'static [&'static str] = &[
        "com.hnc.Discord",
        "com.hnc.DiscordPTB",
        "com.hnc.DiscordCanary",
    ];
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            source: CaptureSource::PrimaryDisplay,
            quality: Quality::Hd1080,
            frame_rate: 60,
            capture_audio: true,
            mute_listed_apps: true,
            show_cursor: true,
        }
    }
}

/// O que sai da captura. Vídeo e áudio são separados de propósito: os encoders dos
/// dois são independentes, e juntá-los aqui só atrapalharia.
pub enum CaptureEvent {
    Video(VideoFrame),
    Audio(AudioChunk),
}

pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    /// Nanoseconds since capture began.
    pub timestamp_ns: u64,

    /// O buffer de GPU com o quadro. Ele vai direto para o encoder de hardware, sem
    /// cópia pela CPU — é isso que torna 1440p60 possível sem sobrecarregar a máquina.
    ///
    /// O campo existe em todos os sistemas para o app compilar em qualquer um; só o
    /// tipo lá dentro muda. Fora do macOS ele ainda vem sempre vazio.
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
    fn quality_maps_to_the_agreed_resolutions() {
        assert_eq!(Quality::Hd720.dimensions(), (1280, 720));
        assert_eq!(Quality::Hd1080.dimensions(), (1920, 1080));
        assert_eq!(Quality::Qhd1440.dimensions(), (2560, 1440));
    }

    #[test]
    fn default_captures_system_audio() {
        // É por isto que o app nativo existe: no navegador isso depende do sistema e
        // da versão. Se alguém desligar sem querer, o teste avisa.
        let config = CaptureConfig::default();

        assert!(config.capture_audio);
        assert!(config.mute_listed_apps, "o áudio de chamada fica de fora até alguém pedir o contrário");
        assert_eq!(config.quality, Quality::Hd1080);
        assert_eq!(config.frame_rate, 60);
    }
}
