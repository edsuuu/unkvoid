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
    /// O tamanho de saída: a largura da qualidade e a altura na proporção da origem —
    /// ver `Quality::fit`. É o que o encoder recebe; a qualidade sozinha não basta.
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub bitrate: u32,
}

impl EncoderConfig {
    /// Quadros por segundo aceitos. Abaixo de 30 a tela parece travada; acima de 60 o
    /// ganho não se vê e o custo é real, em banda e em GPU.
    pub const FPS_MIN: u32 = 30;
    pub const FPS_MAX: u32 = 60;

    /// `source` é o tamanho do que está sendo capturado, em pixels.
    pub fn new(quality: Quality, frame_rate: u32, source: (u32, u32)) -> Self {
        let (width, height) = quality.fit(source);

        // Os mesmos valores do app web, onde já foram calibrados.
        let bitrate = match quality {
            // Medidos depois que o controle de taxa passou a ser respeitado de verdade.
            // Antes o MFT do Windows ignorava o alvo e entregava 11 Mb/s com 7 pedidos;
            // quando a taxa passou a valer, 1080p60 caiu para 6,5 Mb/s reais e a imagem
            // ficou visivelmente pior — o excesso estava tapando um teto baixo demais.
            // Jogo a 60 quadros é o pior caso do H.264: cena inteira mudando toda vez.
            Quality::Hd720 => 5_000_000,
            Quality::Hd1080 => 10_000_000,
            // 1440p tem 1,78 vez os pixels do 1080p e recebia só 1,6 vez os bits: cena em
            // movimento quebrava em bloco. 4K60 em H.264 fica no piso do que se recomenda
            // para envio ao vivo nessa resolução.
            Quality::Qhd1440 => 20_000_000,
            Quality::Uhd2160 => 40_000_000,
        };

        let frame_rate = frame_rate.clamp(Self::FPS_MIN, Self::FPS_MAX);

        Self {
            quality,
            width,
            height,
            // Metade dos quadros custa perto de metade da banda: um teto pensado para
            // 60 sobra em 30, e sobra vira bitrate gasto à toa.
            bitrate: bitrate * frame_rate / Self::FPS_MAX,
            frame_rate: f64::from(frame_rate),
        }
    }
}

/// Deixa passar um quadro a cada `1 / frame_rate` segundo, pelo relógio da captura.
///
/// No Windows 10 a captura não aceita teto de quadros (o intervalo mínimo só veio no 11
/// 24H2) e chega na frequência do monitor: num de 144 Hz a placa comprimia 144 quadros com
/// o bitrate pensado para 60, e cada um saía com menos da metade dos bits. O que sobra
/// volta como `NeedsMoreInput` antes de custar qualquer trabalho.
#[cfg(any(target_os = "windows", test))]
pub(crate) struct FramePacer {
    interval_ns: u64,
    next_ns: u64,
}

#[cfg(any(target_os = "windows", test))]
impl FramePacer {
    pub(crate) fn new(frame_rate: f64) -> Self {
        Self {
            interval_ns: (1e9 / frame_rate.max(1.0)) as u64,
            next_ns: 0,
        }
    }

    pub(crate) fn admit(&mut self, timestamp_ns: u64) -> bool {
        // Um quarto de quadro de folga: a captura a 60 Hz não chega a cada 16 666 µs
        // exatos, e sem folga o quadro que devia passar chegava um tico adiantado e ficava
        // de fora — a transmissão caía para 20 fps.
        if timestamp_ns + self.interval_ns / 4 < self.next_ns {
            return false;
        }

        // Tela parada não gera quadro. Voltando depois de mais de um intervalo, o relógio
        // recomeça daqui em vez de soltar uma rajada para alcançar o tempo perdido.
        let base = if timestamp_ns > self.next_ns + self.interval_ns {
            timestamp_ns
        } else {
            self.next_ns
        };

        self.next_ns = base + self.interval_ns;

        true
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
        let baixo = EncoderConfig::new(Quality::Hd720, 60, (3840, 2160)).bitrate;
        let medio = EncoderConfig::new(Quality::Hd1080, 60, (3840, 2160)).bitrate;
        let alto = EncoderConfig::new(Quality::Qhd1440, 60, (3840, 2160)).bitrate;
        let uhd = EncoderConfig::new(Quality::Uhd2160, 60, (3840, 2160)).bitrate;

        assert!(baixo < medio && medio < alto && alto < uhd);
    }

    #[test]
    fn half_the_frames_cost_about_half_the_bandwidth() {
        let cheio = EncoderConfig::new(Quality::Hd1080, 60, (3840, 2160));
        let metade = EncoderConfig::new(Quality::Hd1080, 30, (3840, 2160));

        assert_eq!(cheio.frame_rate, 60.0);
        assert_eq!(metade.frame_rate, 30.0);
        assert_eq!(metade.bitrate, cheio.bitrate / 2);
    }

    #[test]
    fn fps_out_of_range_is_clamped() {
        // A interface oferece 30 a 60, mas quem chama é o Rust: um valor solto vindo de
        // fora não pode virar captura de 1 fps nem encoder pedindo 240.
        assert_eq!(EncoderConfig::new(Quality::Hd1080, 5, (3840, 2160)).frame_rate, 30.0);
        assert_eq!(EncoderConfig::new(Quality::Hd1080, 500, (3840, 2160)).frame_rate, 60.0);
    }

    #[test]
    fn the_pacer_turns_a_jittery_144_hz_capture_into_60_fps() {
        let mut pacer = FramePacer::new(60.0);
        let admitted = (0..1440_u64)
            .filter(|frame| {
                // A captura nunca chega a cada 6,94 ms exatos: meio milissegundo para cada lado.
                let jitter = if frame % 2 == 0 { 500_000 } else { 0 };

                pacer.admit(frame * 6_944_444 + 500_000 - jitter)
            })
            .count();

        assert!((590..=610).contains(&admitted), "{admitted} quadros em 10 s");
    }

    #[test]
    fn the_pacer_lets_a_60_hz_capture_through_at_60_fps() {
        let mut pacer = FramePacer::new(60.0);
        let admitted = (0..600_u64)
            .filter(|frame| {
                let jitter = if frame % 2 == 0 { 1_000_000 } else { 0 };

                pacer.admit(frame * 16_666_667 + 1_000_000 - jitter)
            })
            .count();

        assert_eq!(admitted, 600, "no Windows 11 a captura já vem no teto e nada pode cair");
    }

    #[test]
    fn the_pacer_does_not_burst_after_a_still_screen() {
        let mut pacer = FramePacer::new(30.0);

        assert!(pacer.admit(0));
        // Tela parada por um segundo; volta a 60 Hz.
        assert!(pacer.admit(1_000_000_000));
        assert!(!pacer.admit(1_016_666_667), "sem rajada para alcançar o segundo perdido");
        assert!(pacer.admit(1_033_333_333));
    }
}
