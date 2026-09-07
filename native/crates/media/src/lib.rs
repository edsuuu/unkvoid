//! Codificação de vídeo e transporte.
//!
//! A codificação roda no **chip de mídia**, não na CPU: o quadro sai da captura já
//! como buffer de GPU e vai direto para o encoder, sem cópia. É o que permite
//! 1440p60 sem a máquina de quem compartilha derreter.

use capture::Quality;

mod audio;
mod peer;

#[cfg(target_os = "macos")]
mod macos;

pub use audio::{AudioEncoder, FRAME_MS};
pub use peer::{PeerLink, Signal};

#[cfg(target_os = "macos")]
pub use macos::VideoToolboxEncoder as PlatformEncoder;

/// O buffer de GPU que o encoder recebe. No macOS é uma `IOSurface` de verdade; nas
/// outras plataformas é um marcador, até existir encoder por lá.
#[cfg(target_os = "macos")]
pub type GpuSurface = apple_cf::iosurface::IOSurface;

#[cfg(not(target_os = "macos"))]
pub type GpuSurface = ();

/// Um quadro já comprimido, pronto para virar pacote RTP.
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
        // Mesmos números do app web, onde já foram calibrados.
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
    #[error("o encoder por hardware recusou iniciar: {0}")]
    Start(String),

    #[error("falha ao codificar o quadro: {0}")]
    Encode(String),

    #[error("o quadro veio sem buffer de GPU — nada para codificar")]
    NoSurface,

    #[error("codificação de vídeo ainda não implementada nesta plataforma")]
    Unsupported,
}

/// Fora do macOS ainda não há encoder por hardware. O stub existe com a **mesma
/// forma** do real para o app compilar e falhar com mensagem clara, em vez de não
/// compilar — assim o `.msi` e o `.deb` saem e o resto do app funciona.
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
