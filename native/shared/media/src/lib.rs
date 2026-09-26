//! Codificação de vídeo e transporte.
//!
//! Quem codifica é o **chip de mídia**, não o processador: o quadro sai da captura como
//! buffer de GPU e vai direto para o encoder, sem cópia. É isso que permite 1440p60 sem
//! sobrecarregar a máquina de quem transmite.

use capture::Quality;

mod audio;
mod governor;
mod plain;
mod receiver;
mod recovery;
mod unpack;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
mod windows_decoder;

pub use audio::{AudioEncoder, FRAME_MS};
pub use governor::BitrateGovernor;
pub use plain::{Feedback, PlainSender, Source};
pub use receiver::{PlainReceiver, Rtx, Stream, resolve};
pub use recovery::Counters;
pub use unpack::{AccessUnit, AudioUnpacker, VideoUnpacker, nals};

#[cfg(target_os = "macos")]
pub use macos::VideoToolboxEncoder as PlatformEncoder;

#[cfg(target_os = "windows")]
pub use windows::MediaFoundationEncoder as PlatformEncoder;

#[cfg(target_os = "windows")]
pub use windows_decoder::H264Decoder;

/// Um quadro decodificado, pronto para desenhar: RGB de 8 bits, sem padding entre as linhas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// Fora do Windows quem assiste decodifica pelo sistema dele — VideoToolbox no macOS,
/// GStreamer no Linux —, e este existe só para o app do Windows compilar em qualquer lugar.
#[cfg(not(target_os = "windows"))]
pub struct H264Decoder;

#[cfg(not(target_os = "windows"))]
impl H264Decoder {
    pub fn new() -> anyhow::Result<Self> {
        Err(anyhow::anyhow!("o decodificador de H.264 do media é só do Windows"))
    }

    pub fn decode(&mut self, _annex_b: &[u8], _timestamp: u32) -> anyhow::Result<Vec<DecodedFrame>> {
        Err(anyhow::anyhow!("o decodificador de H.264 do media é só do Windows"))
    }
}

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
    /// Quadros por segundo aceitos. 15 é a válvula de escape de 720p para máquina
    /// fraca (a captura já aceita de 1 a 60; o menu continua só oferecendo 30 e 60).
    /// Acima de 60 o ganho não se vê e o custo é real, em banda e em GPU.
    pub const FPS_MIN: u32 = 15;
    pub const FPS_MAX: u32 = 60;

    /// `source` é o tamanho do que está sendo capturado, em pixels.
    pub fn new(quality: Quality, frame_rate: u32, source: (u32, u32)) -> Self {
        let (width, height) = quality.fit(source);

        let bitrate = match quality {
            // Medidos depois que o controle de taxa passou a ser respeitado de verdade.
            // Antes o MFT do Windows ignorava o alvo e entregava 11 Mb/s com 7 pedidos;
            // quando a taxa passou a valer, 1080p60 caiu para 6,5 Mb/s reais e a imagem
            // ficou visivelmente pior — o excesso estava tapando um teto baixo demais.
            // Jogo a 60 quadros é o pior caso do H.264: cena inteira mudando toda vez.
            Quality::Hd720 => 5_000_000,
            Quality::Hd1080 => 10_000_000,
            Quality::Qhd1440 => 20_000_000,
            // 4K60 em H.264 fica no piso do que se recomenda para envio ao vivo.
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

    /// O teto quando não há encoder na placa: 720p, no máximo 30 fps, e a taxa de 720p30.
    ///
    /// Comprimir no processador é disputar com o jogo o que ele precisa, e 1080p60 por
    /// software custa o quadro inteiro. O dono decidiu que transmitir pior é melhor do que
    /// não transmitir, e este é o "pior" que ainda deixa o jogo rodando.
    pub fn for_cpu(&self) -> Self {
        Self::new(Quality::Hd720, (self.frame_rate as u32).min(30), (self.width, self.height))
    }
}

/// `UNKVOID_ENCODER=cpu` pula o encoder da placa. Sem isto o degrau do processador só roda
/// em máquina sem placa, e ninguém que desenvolve tem uma à mão.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn cpu_forced() -> bool {
    std::env::var("UNKVOID_ENCODER").is_ok_and(|value| value == "cpu")
}

/// Deixa passar um quadro a cada `1 / frame_rate` segundo, pelo relógio da captura.
///
/// É assim que o encoder por processador fica em 30 fps sem reabrir a captura: o quadro
/// que sobra volta como `NeedsMoreInput` antes de custar qualquer trabalho.
///
/// No Windows 10 vale para qualquer encoder, e não só para o do processador: a captura
/// não aceita teto de quadros (o intervalo mínimo só veio no 11 24H2) e chega na
/// frequência do monitor — num de 144 Hz a placa comprimia 144 quadros com o bitrate
/// pensado para 60, e cada um saía com menos da metade dos bits.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub(crate) struct FramePacer {
    interval_ns: u64,
    next_ns: u64,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
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
pub struct PlatformEncoder {
    bitrate: u32,
}

#[cfg(target_os = "linux")]
impl PlatformEncoder {
    pub fn new(config: &EncoderConfig) -> Result<Self, EncoderError> {
        Ok(Self { bitrate: config.bitrate })
    }

    /// ponytail: o x264 no pipe não recebe pedidos; o keyframe periódico (1 s) cobre.
    pub fn request_keyframe(&mut self) {}

    /// A taxa com que o encoder abriu, que é o teto de quem a ajusta.
    pub fn bitrate(&self) -> u32 {
        self.bitrate
    }

    /// ponytail: sem efeito, e diz que recusou para o governador parar de tentar. O encoder
    /// é o `gst-launch` filho, com a taxa escrita na linha de comando: o teto é a taxa fixa
    /// de hoje. A saída é o pipeline dentro do processo (`gstreamer-rs`), onde `bitrate` é
    /// propriedade que o x264enc e o nvh264enc aceitam com o pipeline no ar.
    pub fn set_bitrate(&mut self, _bitrate: u32) -> bool {
        false
    }

    /// Se o H.264 sai da placa. Quem escolhe o encoder é a captura, que monta o pipeline.
    pub fn hardware(&self) -> bool {
        capture::PlatformCapturer::video_encoder() != "x264enc"
    }

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

    pub fn bitrate(&self) -> u32 {
        0
    }

    pub fn set_bitrate(&mut self, _bitrate: u32) -> bool {
        false
    }

    pub fn hardware(&self) -> bool {
        false
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

    const FULL_HD: (u32, u32) = (1920, 1080);

    #[test]
    fn bitrate_rises_with_resolution() {
        let baixo = EncoderConfig::new(Quality::Hd720, 60, FULL_HD).bitrate;
        let medio = EncoderConfig::new(Quality::Hd1080, 60, FULL_HD).bitrate;
        let alto = EncoderConfig::new(Quality::Qhd1440, 60, FULL_HD).bitrate;

        let uhd = EncoderConfig::new(Quality::Uhd2160, 60, FULL_HD).bitrate;

        assert!(baixo < medio && medio < alto && alto < uhd);
    }

    #[test]
    fn half_the_frames_cost_about_half_the_bandwidth() {
        let cheio = EncoderConfig::new(Quality::Hd1080, 60, FULL_HD);
        let metade = EncoderConfig::new(Quality::Hd1080, 30, FULL_HD);

        assert_eq!(cheio.frame_rate, 60.0);
        assert_eq!(metade.frame_rate, 30.0);
        assert_eq!(metade.bitrate, cheio.bitrate / 2);
    }

    #[test]
    fn fps_out_of_range_is_clamped() {
        // A interface oferece 30 a 60, mas quem chama é o Rust: um valor solto vindo de
        // fora não pode virar captura de 1 fps nem encoder pedindo 240.
        assert_eq!(EncoderConfig::new(Quality::Hd1080, 5, FULL_HD).frame_rate, 15.0);
        assert_eq!(EncoderConfig::new(Quality::Hd1080, 500, FULL_HD).frame_rate, 60.0);
    }

    #[test]
    fn the_cpu_ceiling_is_720p30_in_the_source_aspect() {
        let ultrawide = EncoderConfig::new(Quality::Hd1080, 60, (3440, 1440)).for_cpu();

        assert_eq!((ultrawide.width, ultrawide.height), (1280, 534));
        assert_eq!(ultrawide.frame_rate, 30.0);
        assert_eq!(ultrawide.bitrate, EncoderConfig::new(Quality::Hd720, 30, FULL_HD).bitrate);

        // Pedido abaixo do teto fica como está: o teto não sobe ninguém.
        assert_eq!(EncoderConfig::new(Quality::Hd720, 15, FULL_HD).for_cpu().frame_rate, 15.0);
    }

    #[test]
    fn the_pacer_turns_a_jittery_60_hz_capture_into_30_fps() {
        let mut pacer = FramePacer::new(30.0);
        let admitted = (0..600_u64)
            .filter(|frame| {
                let jitter = if frame % 2 == 0 { 1_000_000 } else { 0 };

                pacer.admit(frame * 16_666_667 + 1_000_000 - jitter)
            })
            .count();

        assert_eq!(admitted, 300);
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

    #[test]
    fn the_output_follows_the_source_aspect() {
        let ultrawide = EncoderConfig::new(Quality::Hd1080, 60, (3440, 1440));

        assert_eq!((ultrawide.width, ultrawide.height), (1920, 802));
    }
}
