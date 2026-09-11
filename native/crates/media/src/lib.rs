//! Codificação de vídeo e transporte.
//!
//! Quem codifica é o **chip de mídia**, não o processador: o quadro sai da captura como
//! buffer de GPU e vai direto para o encoder, sem cópia. É isso que permite 1440p60 sem
//! sobrecarregar a máquina de quem transmite.

use capture::Quality;

mod audio;
mod plain;
mod receiver;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

pub use audio::{AudioEncoder, FRAME_MS};
pub use plain::PlainSender;
pub use receiver::PlainReceiver;

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
pub type GpuSurface = capture::GpuSurface;

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
    /// Quadros por segundo aceitos. Abaixo de 30 a tela parece travada; acima de 60 o
    /// ganho não se vê e o custo é real, em banda e em GPU.
    pub const FPS_MIN: u32 = 30;
    pub const FPS_MAX: u32 = 60;

    pub fn new(quality: Quality, frame_rate: u32) -> Self {
        // Os mesmos valores do app web, onde já foram calibrados.
        let bitrate = match quality {
            // Medidos depois que o controle de taxa passou a ser respeitado de verdade.
            // Antes o MFT do Windows ignorava o alvo e entregava 11 Mb/s com 7 pedidos;
            // quando a taxa passou a valer, 1080p60 caiu para 6,5 Mb/s reais e a imagem
            // ficou visivelmente pior — o excesso estava tapando um teto baixo demais.
            // Jogo a 60 quadros é o pior caso do H.264: cena inteira mudando toda vez.
            Quality::Hd720 => 5_000_000,
            Quality::Hd1080 => 10_000_000,
            Quality::Qhd1440 => 16_000_000,
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

/// No Linux quem codifica é o x264 dentro do GStreamer, na captura. O que chega aqui já
/// é H.264 Annex-B, e este encoder só o repassa — com a mesma forma dos outros para o
/// `broadcast.rs` não saber a diferença.
#[cfg(target_os = "linux")]
pub struct PlatformEncoder;

#[cfg(target_os = "linux")]
impl PlatformEncoder {
    pub fn new(_config: &EncoderConfig) -> Result<Self, EncoderError> {
        Ok(Self)
    }

    /// ponytail: o x264 no pipe não recebe pedidos; o keyframe periódico (1 s) cobre.
    pub fn request_keyframe(&mut self) {}

    pub fn encode(
        &mut self,
        surface: &GpuSurface,
        timestamp_ns: u64,
    ) -> Result<EncodedFrame, EncoderError> {
        Ok(EncodedFrame {
            data: surface.data.clone(),
            keyframe: surface.keyframe,
            timestamp_ns,
        })
    }
}

/// Fora dos três sistemas não há encoder. O stub tem a **mesma forma** da implementação
/// real para o app compilar e falhar com mensagem clara em vez de não compilar.
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub struct PlatformEncoder;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
impl PlatformEncoder {
    pub fn new(_config: &EncoderConfig) -> Result<Self, EncoderError> {
        Err(EncoderError::Unsupported)
    }

    pub fn request_keyframe(&mut self) {}

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
    fn bitrate_rises_with_resolution() {
        let baixo = EncoderConfig::new(Quality::Hd720, 60).bitrate;
        let medio = EncoderConfig::new(Quality::Hd1080, 60).bitrate;
        let alto = EncoderConfig::new(Quality::Qhd1440, 60).bitrate;

        assert!(baixo < medio && medio < alto);
    }

    #[test]
    fn half_the_frames_cost_about_half_the_bandwidth() {
        let cheio = EncoderConfig::new(Quality::Hd1080, 60);
        let metade = EncoderConfig::new(Quality::Hd1080, 30);

        assert_eq!(cheio.frame_rate, 60.0);
        assert_eq!(metade.frame_rate, 30.0);
        assert_eq!(metade.bitrate, cheio.bitrate / 2);
    }

    #[test]
    fn fps_out_of_range_is_clamped() {
        // A interface oferece 30 a 60, mas quem chama é o Rust: um valor solto vindo de
        // fora não pode virar captura de 1 fps nem encoder pedindo 240.
        assert_eq!(EncoderConfig::new(Quality::Hd1080, 5).frame_rate, 30.0);
        assert_eq!(EncoderConfig::new(Quality::Hd1080, 500).frame_rate, 60.0);
    }
}
