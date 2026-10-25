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

#[cfg(target_os = "windows")]
mod windows_camera;

#[cfg(target_os = "windows")]
pub mod windows_duplication;

#[cfg(target_os = "windows")]
pub use windows_camera::{CameraFrame, CameraReader, SurfaceUploader};

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
mod linux_audio;

#[cfg(target_os = "macos")]
pub use macos::MacCapturer as PlatformCapturer;

#[cfg(target_os = "windows")]
pub use windows::WindowsCapturer as PlatformCapturer;

#[cfg(target_os = "linux")]
pub use linux::LinuxCapturer as PlatformCapturer;

/// Se quem escolhe a tela é o seletor do próprio sistema (o portal do Wayland) e não a
/// lista do app: aí `displays()` devolve um item só, sem tamanho, e `windows()` nenhum.
#[cfg(target_os = "linux")]
pub use linux::uses_system_picker;

/// O encoder do pipeline de vídeo no ar: o `media` pede o keyframe e troca a taxa por ele.
#[cfg(target_os = "linux")]
pub use linux::EncoderControl;

#[cfg(not(target_os = "linux"))]
pub fn uses_system_picker() -> bool {
    false
}

/// Uma câmera que esta captura abre: `index` é o que vai em `CaptureSource::Camera`, e `name`
/// é o que a pessoa reconhece ("HD Webcam C920").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Camera {
    pub index: u32,
    pub name: String,
}

/// Se esta captura abre câmera neste sistema. Só o Linux a captura aqui (o GStreamer lê a
/// webcam e já entrega H.264); no macOS a câmera vem pronta da interface, e no Windows ela
/// ainda não existe. É isto que a tela pergunta, e não o sistema em que foi compilada.
pub fn captures_cameras() -> bool {
    cfg!(target_os = "linux")
}

/// As câmeras disponíveis, na ordem do sistema; a primeira é a câmera padrão.
pub fn cameras() -> Vec<Camera> {
    #[cfg(target_os = "linux")]
    return linux::LinuxCapturer::cameras()
        .into_iter()
        .filter_map(|(path, name)| Some(Camera { index: path.strip_prefix("/dev/video")?.parse().ok()?, name }))
        .collect();

    #[cfg(target_os = "windows")]
    return windows_camera::cameras();

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    Vec::new()
}

/// Se a janela escolhida ainda existe. A captura calada pode ser a janela que fechou — o jogo
/// que saiu —, e aí refazê-la só falharia de novo a cada espera do vigia: quem transmite tem de
/// parar e saber por quê. Monitor e o resto: sempre.
pub fn source_exists(source: CaptureSource) -> bool {
    #[cfg(target_os = "windows")]
    return windows::window_alive(source);

    #[cfg(not(target_os = "windows"))]
    {
        let _ = source;

        true
    }
}

/// Se a janela escolhida está minimizada. Minimizada ela não entrega quadro nenhum, e refazer a
/// captura ali só custa engasgos ao jogo que está por cima.
pub fn source_minimized(source: CaptureSource) -> bool {
    #[cfg(target_os = "windows")]
    return windows::window_minimized(source);

    #[cfg(not(target_os = "windows"))]
    {
        let _ = source;

        false
    }
}

/// O que tem de acontecer antes de `source_size` e `start` e pode esperar pela pessoa:
/// no Wayland é aqui que o seletor do sistema abre, uma vez só, e o que ela escolher fica
/// guardado para os dois. Bloqueia; quem chama não pode estar com cadeado nenhum na mão.
/// Nos outros sistemas, e no X11, não faz nada.
pub fn prepare(config: &CaptureConfig) -> Result<Prepared, CaptureError> {
    #[cfg(target_os = "linux")]
    linux::prepare(config)?;

    #[cfg(not(target_os = "linux"))]
    let _ = config;

    Ok(Prepared(()))
}

/// O que o `prepare` deixou separado. Largado sem que um `start` tenha consumido, fecha
/// o que abriu: sem isto um encoder que não abre deixaria a tela "sendo compartilhada"
/// no indicador do sistema, sem transmissão nenhuma.
#[must_use]
pub struct Prepared(());

impl Drop for Prepared {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        linux::discard_prepared();
    }
}

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

/// No Linux o quadro já chega comprimido: o x264 roda dentro do GStreamer, do lado da
/// captura. O "buffer" que vai para o encoder é o H.264 pronto.
#[cfg(target_os = "linux")]
pub type GpuSurface = linux::EncodedVideo;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub type GpuSurface = ();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Hd720,
    Hd1080,
    Qhd1440,
    Uhd2160,
}

/// A maior largura que sai: o H.264 da placa para em 4096.
const MAX_WIDTH: u64 = 3840;

impl Quality {
    /// A qualidade escolhe só a altura; a largura vem da proporção da origem.
    pub fn height(self) -> u32 {
        match self {
            Self::Hd720 => 720,
            Self::Hd1080 => 1080,
            Self::Qhd1440 => 1440,
            Self::Uhd2160 => 2160,
        }
    }

    /// O tamanho de saída para uma origem de `source` pixels: sempre a altura da qualidade —
    /// quem assiste recebe o 1080p pedido mesmo de um ultrawide (3440x1440 sai 2580x1080) ou
    /// de um monitor menor (1366x768 sobe para 1920x1080) — e a largura que mantém a
    /// proporção, até `MAX_WIDTH`; as duas pares, como o H.264 em 4:2:0 exige.
    ///
    /// Sem a proporção um ultrawide 21:9 e um monitor em pé saíam espremidos em 16:9.
    pub fn fit(self, source: (u32, u32)) -> (u32, u32) {
        let (source_width, source_height) = (u64::from(source.0.max(2)), u64::from(source.1.max(2)));
        let height = u64::from(self.height());
        let width = height * source_width / source_height;
        let (width, height) = if width > MAX_WIDTH {
            (MAX_WIDTH, MAX_WIDTH * source_height / source_width)
        } else {
            (width, height)
        };

        (width.max(2) as u32 & !1, height.max(2) as u32 & !1)
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

    /// A câmera `/dev/video<n>`. Só no Linux: nos outros sistemas o webview a captura.
    Camera(u32),

    /// O microfone padrão do sistema. Só no Linux, pelo mesmo motivo.
    Microphone,
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
    /// o app de chamada para conversar quase nunca quer a conversa junto, mas quem está
    /// mostrando o próprio app de chamada para alguém quer.
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
    pub const EXCLUDE_OWN_AUDIO: bool = true;

    /// Aplicativos cujo som nunca sobe junto com a tela, identificados pelo bundle.
    ///
    /// Quem compartilha aqui quase sempre está falando pelo app de chamada ao mesmo tempo. Sem
    /// isto, a voz de todo mundo da chamada de lá entra na transmissão: quem está nos
    /// dois lugares ouve cada pessoa duas vezes, a segunda com o atraso do salto pelo
    /// servidor. O sistema filtra por processo, que é mais confiável do que tentar
    /// adivinhar aqui de onde veio cada som.
    ///
    /// No macOS o filtro do ScreenCaptureKit é um só para vídeo e áudio, então a janela
    /// do app silenciado também sai da imagem. Para o app de chamada isso é ganho duplo: a
    /// conversa privada não vaza para a sala.
    ///
    /// No Windows não há bundle, e quem vale é `MUTED_EXECUTABLES`.
    pub const MUTED_APPS: &'static [&'static str] = &[
        "com.hnc.Discord",
        "com.hnc.DiscordPTB",
        "com.hnc.DiscordCanary",
    ];

    /// Os mesmos aplicativos no Windows, pelo nome do executável, sem diferenciar
    /// maiúsculas. O app de chamada toca a chamada num processo filho com o mesmo nome; o
    /// `DiscordSystemHelper.exe` nasce fora da árvore dele e precisa vir pelo nome.
    ///
    /// Os clientes alternativos entram pelo nome próprio: a chamada é a mesma, num processo
    /// que não se chama assim. O app de chamada aberto no navegador não tem nome que o separe
    /// do resto do navegador, e continua entrando.
    pub const MUTED_EXECUTABLES: &'static [&'static str] = &[
        "Discord.exe",
        "DiscordPTB.exe",
        "DiscordCanary.exe",
        "DiscordSystemHelper.exe",
        "Vesktop.exe",
        "ArmCord.exe",
        "Legcord.exe",
        "WebCord.exe",
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
    /// tipo lá dentro muda: textura do Direct3D no Windows, buffer do ScreenCaptureKit
    /// no macOS, e no Linux o H.264 que o GStreamer já entregou pronto.
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

    /// A pessoa fechou o seletor de tela do sistema sem escolher nada. Não é defeito, e
    /// a interface precisa poder distinguir.
    #[error("screen picker closed without choosing a source")]
    Cancelled,

    #[error("capture failed: {0}")]
    Platform(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_is_always_the_chosen_height_in_the_source_aspect() {
        assert_eq!(Quality::Hd1080.fit((1920, 1080)), (1920, 1080));
        assert_eq!(Quality::Hd720.fit((3840, 2160)), (1280, 720));
        // Ultrawides descem para 1080 de altura, com a largura toda.
        assert_eq!(Quality::Hd1080.fit((3440, 1440)), (2580, 1080));
        assert_eq!(Quality::Hd1080.fit((2560, 1080)), (2560, 1080));
        // Monitor menor sobe para 1080.
        assert_eq!(Quality::Hd1080.fit((1366, 768)), (1920, 1080));
        assert_eq!(Quality::Hd1080.fit((1280, 720)), (1920, 1080));
        // Monitor em pé continua em pé.
        assert_eq!(Quality::Hd1080.fit((1080, 1920)), (606, 1080));
        // Largo demais para o H.264 da placa: a largura para no teto e a altura acompanha.
        assert_eq!(Quality::Uhd2160.fit((5120, 1440)), (3840, 1080));
        // Janela ímpar sai par.
        assert_eq!(Quality::Hd1080.fit((1001, 601)), (1798, 1080));
        // Origem desconhecida não divide por zero.
        assert_eq!(Quality::Hd1080.fit((0, 0)), (1080, 1080));
    }

    #[test]
    fn default_captures_system_audio() {
        let config = CaptureConfig::default();

        assert!(config.capture_audio);
        assert!(config.mute_listed_apps, "o áudio de chamada fica de fora até alguém pedir o contrário");
        assert_eq!(config.quality, Quality::Hd1080);
        assert_eq!(config.frame_rate, 60);
    }
}
