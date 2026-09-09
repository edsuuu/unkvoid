//! Video encoding and transport.
//!
//! Encoding runs on the **media chip**, not the CPU: the frame leaves capture as a
//! GPU buffer and goes directly to the encoder without a copy. This is what enables
//! 1440p60 without overloading the broadcaster's machine.

use capture::Quality;

mod audio;
mod plain;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

pub use audio::{AudioEncoder, FRAME_MS};
pub use plain::PlainSender;

#[cfg(target_os = "macos")]
pub use macos::VideoToolboxEncoder as PlatformEncoder;

#[cfg(target_os = "windows")]
pub use windows::MediaFoundationEncoder as PlatformEncoder;

/// O buffer de GPU que o encoder recebe. No macOS é um `IOSurface`; no Windows é a
/// textura do Direct3D com o device que a criou. Em ambos, quem o produz é a captura —
/// o quadro nunca desce para a memória do processador.
#[cfg(target_os = "macos")]
pub type GpuSurface = apple_cf::iosurface::IOSurface;

#[cfg(target_os = "windows")]
pub type GpuSurface = capture::GpuSurface;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
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
    /// Quadros por segundo aceitos. Abaixo de 30 a tela parece travada; acima de 60 o
    /// ganho não se vê e o custo é real, em banda e em GPU.
    pub const FPS_MIN: u32 = 30;
    pub const FPS_MAX: u32 = 60;

    pub fn new(quality: Quality, frame_rate: u32) -> Self {
        // Same values as the web app, where they have already been calibrated.
        let bitrate = match quality {
            Quality::Hd720 => 4_000_000,
            Quality::Hd1080 => 7_000_000,
            Quality::Qhd1440 => 12_000_000,
        };

        let frame_rate = frame_rate.clamp(Self::FPS_MIN, Self::FPS_MAX);

        Self {
            quality,
            // Metade dos quadros custa perto de metade da banda: um teto pensado para
            // 60 sobra em 30, e sobra vira bitrate gasto à toa.
            bitrate: bitrate * frame_rate / Self::FPS_MAX,
            frame_rate: f64::from(frame_rate),
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

    /// Encoder de hardware tem fila: os primeiros quadros entram sem nada sair ainda.
    #[error("the encoder has not produced a frame yet")]
    NeedsMoreInput,

    #[error("video encoding is not implemented on this platform yet")]
    Unsupported,
}

/// Fora do macOS e do Windows não há encoder de hardware. O stub tem a **mesma forma**
/// da implementação real para o app compilar e falhar com mensagem clara em vez de não
/// compilar — o `.deb` sai, e o resto do app funciona.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub struct PlatformEncoder;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
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
        let baixo = EncoderConfig::new(Quality::Hd720, 60).bitrate;
        let medio = EncoderConfig::new(Quality::Hd1080, 60).bitrate;
        let alto = EncoderConfig::new(Quality::Qhd1440, 60).bitrate;

        assert!(baixo < medio && medio < alto);
    }

    #[test]
    fn metade_dos_quadros_custa_perto_de_metade_da_banda() {
        let cheio = EncoderConfig::new(Quality::Hd1080, 60);
        let metade = EncoderConfig::new(Quality::Hd1080, 30);

        assert_eq!(cheio.frame_rate, 60.0);
        assert_eq!(metade.frame_rate, 30.0);
        assert_eq!(metade.bitrate, cheio.bitrate / 2);
    }

    #[test]
    fn fps_fora_da_faixa_e_puxado_para_dentro() {
        // A interface oferece 30 a 60, mas quem chama é o Rust: um valor solto vindo de
        // fora não pode virar captura de 1 fps nem encoder pedindo 240.
        assert_eq!(EncoderConfig::new(Quality::Hd1080, 5).frame_rate, 30.0);
        assert_eq!(EncoderConfig::new(Quality::Hd1080, 500).frame_rate, 60.0);
    }
}
