use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::path::PathBuf;

use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::ImageFormat;
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::{GraphicsCaptureApi, InternalCaptureControl};
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window as CaptureWindow;

use ::windows::Win32::Foundation::{HWND, RECT};
use ::windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    GetWindowPlacement, GetWindowThreadProcessId, IsIconic, IsWindow, WINDOWPLACEMENT,
    WPF_RESTORETOMAXIMIZED,
};

use crate::windows_audio::{AudioScope, SystemAudio};
use crate::windows_duplication::{self, Duplication};
use crate::{
    CaptureConfig, CaptureError, CaptureEvent, CaptureSource, Display, GpuSurface, VideoFrame,
    Window,
};

type EventSink = Arc<dyn Fn(CaptureEvent) + Send + Sync>;

/// `HWND` é ponteiro, e o identificador que atravessa a interface é número. A Microsoft
/// garante que handles cabem em 32 bits com sinal estendido justamente para poderem
/// atravessar fronteiras de 32/64 bits, então a ida e a volta são seguras — e só valem
/// dentro deste processo, que é onde a escolha é feita e usada.
fn id_from_hwnd(hwnd: *mut std::ffi::c_void) -> u64 {
    hwnd as usize as u64
}

fn hwnd_from_id(id: u64) -> *mut std::ffi::c_void {
    id as usize as *mut std::ffi::c_void
}

/// O id de um monitor é o número que o Windows deu a ele (`\\.\DISPLAY2` é 2), e não a posição
/// na lista: desligar a TV ou o segundo monitor, ou voltar da suspensão, reordena a lista, e a
/// posição passava a transmitir **outro** monitor, com o que estivesse nele. Somado a
/// `DISPLAY_IDS` para nunca bater com um id antigo, de posição, guardado antes desta versão: esse
/// falha em vez de adivinhar.
const DISPLAY_IDS: u32 = 1_000;

fn display_id(monitor: &Monitor) -> Option<u32> {
    monitor.index().ok().and_then(|number| u32::try_from(number).ok()).map(|number| DISPLAY_IDS + number)
}

/// O monitor de um id, ou nenhum: o que sumiu não vira outro.
fn monitor_by_id(id: u32) -> Result<Monitor, CaptureError> {
    Monitor::enumerate()
        .map_err(|error| CaptureError::Platform(error.to_string()))?
        .into_iter()
        .find(|monitor| display_id(monitor) == Some(id))
        .ok_or(CaptureError::NoDisplay)
}

pub fn window_alive(source: CaptureSource) -> bool {
    match source {
        CaptureSource::Window(id) => unsafe { IsWindow(Some(HWND(hwnd_from_id(id)))) }.as_bool(),
        _ => true,
    }
}

pub fn window_minimized(source: CaptureSource) -> bool {
    match source {
        CaptureSource::Window(id) => unsafe { IsIconic(HWND(hwnd_from_id(id))) }.as_bool(),
        _ => false,
    }
}

/// O tamanho da janela para o encoder.
///
/// Minimizada, o `GetWindowRect` devolve o retângulo do ícone (160×28 em -32000,-32000), e
/// nenhum encoder de H.264 abre nisso. Vale o tamanho para o qual ela volta: o normal, ou a
/// área útil do monitor quando ela volta maximizada — o encoder estica a fonte na saída
/// inteira, e abrir com a proporção errada deformaria a imagem na volta.
///
/// Minimizada é o caso comum, não o raro: jogo em tela cheia exclusiva minimiza sozinho
/// quando a pessoa vem ao app escolher o que compartilhar. A Graphics Capture abre, não
/// entrega quadro nenhum enquanto a janela está embaixo (medido: 0 em 3 s) e volta a
/// entregar, já no tamanho certo, quando a pessoa volta ao jogo. Recusar o início aqui
/// tornaria esse jogo impossível de compartilhar por janela.
fn window_size(shown: (i32, i32), restored: Option<Restored>) -> (u32, u32) {
    let (width, height) = match restored {
        Some(restored) => restored.work_area.unwrap_or(restored.normal),
        None => shown,
    };

    (width.max(0) as u32, height.max(0) as u32)
}

/// Para onde a janela minimizada volta.
struct Restored {
    normal: (i32, i32),

    /// A área útil do monitor dela, só quando ela volta maximizada.
    work_area: Option<(i32, i32)>,
}

fn extent(rect: RECT) -> (i32, i32) {
    (rect.right - rect.left, rect.bottom - rect.top)
}

/// `None` para janela que não está minimizada.
fn restored_placement(hwnd: HWND) -> Option<Restored> {
    unsafe {
        if !IsIconic(hwnd).as_bool() {
            return None;
        }

        let mut placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };

        GetWindowPlacement(hwnd, &mut placement).ok()?;

        let work_area = placement.flags.contains(WPF_RESTORETOMAXIMIZED).then(|| {
            let mut monitor = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };

            // Pelo retângulo normal, não pela janela: minimizada ela mora em -32000, e o
            // monitor "mais perto" disso não é o dela.
            GetMonitorInfoW(
                MonitorFromRect(&placement.rcNormalPosition, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            )
            .as_bool()
            .then(|| extent(monitor.rcWork))
        });

        Some(Restored { normal: extent(placement.rcNormalPosition), work_area: work_area.flatten() })
    }
}

/// Liga a captura no alvo escolhido. Genérica porque monitor e janela são tipos
/// distintos para o `windows-capture`, mas produzem o mesmo controle.
fn start_capture<T>(
    target: T,
    config: &CaptureConfig,
    sink: EventSink,
    frames: Arc<AtomicU64>,
) -> Result<windows_capture::capture::CaptureControl<Sink, CaptureFailure>, CaptureError>
where
    T: TryInto<windows_capture::settings::GraphicsCaptureItemType> + Send + 'static,
{
    let settings = supported_settings(
        target,
        config.show_cursor,
        // O teto de quadros começa aqui: pedir 30 e deixar a captura entregar 60 faria o
        // encoder jogar metade fora depois de já ter pago por ela.
        std::time::Duration::from_secs_f64(1.0 / f64::from(config.frame_rate.max(1))),
        (sink, frames),
    );

    Sink::start_free_threaded(settings).map_err(|error| {
        tracing::error!(error = %error, "Windows Graphics Capture recusou a captura");
        CaptureError::Platform(error.to_string())
    })
}

/// A crate recusa a captura inteira quando recebe uma chave que este Windows não tem: a
/// borda só dá para desligar do Windows 11 em diante, o intervalo mínimo veio no 11 24H2
/// e o cursor no 10 2004. O que faltar fica no padrão do sistema — a borda amarela
/// aparece, e a captura chega na frequência do monitor.
fn supported_settings<T, Flags>(
    target: T,
    show_cursor: bool,
    interval: std::time::Duration,
    flags: Flags,
) -> Settings<Flags, T>
where
    T: TryInto<windows_capture::settings::GraphicsCaptureItemType>,
{
    let cursor = match GraphicsCaptureApi::is_cursor_settings_supported() {
        Ok(true) if show_cursor => CursorCaptureSettings::WithCursor,
        Ok(true) => CursorCaptureSettings::WithoutCursor,
        _ => CursorCaptureSettings::Default,
    };

    let border = if GraphicsCaptureApi::is_border_settings_supported().unwrap_or(false) {
        DrawBorderSettings::WithoutBorder
    } else {
        DrawBorderSettings::Default
    };

    let update_interval = if GraphicsCaptureApi::is_minimum_update_interval_supported().unwrap_or(false) {
        MinimumUpdateIntervalSettings::Custom(interval)
    } else {
        MinimumUpdateIntervalSettings::Default
    };

    Settings::new(
        target,
        cursor,
        border,
        SecondaryWindowSettings::Default,
        update_interval,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        flags,
    )
}

/// O que está capturando: o Graphics Capture, ou o Desktop Duplication no monitor do Windows
/// que não tira a borda amarela.
enum Running {
    Graphics(windows_capture::capture::CaptureControl<Sink, CaptureFailure>),
    Duplication(Duplication),
}

/// Capture via Windows Graphics Capture. Requires Windows 10 1903 or newer.
pub struct WindowsCapturer {
    control: Option<Running>,
    frames: Arc<AtomicU64>,

    /// O som não vem junto com a imagem aqui: o Graphics Capture só entrega quadros, e
    /// o áudio do sistema é uma API à parte.
    audio: Option<SystemAudio>,
}

pub struct CaptureFailure(String);

impl std::fmt::Debug for CaptureFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl std::fmt::Display for CaptureFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl std::error::Error for CaptureFailure {}

struct PreviewSink {
    path: PathBuf,
    result: mpsc::Sender<Result<(), String>>,
}

impl GraphicsCaptureApiHandler for PreviewSink {
    type Flags = (PathBuf, mpsc::Sender<Result<(), String>>);
    type Error = CaptureFailure;

    fn new(context: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { path: context.flags.0, result: context.flags.1 })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let result = frame
            .save_as_image(&self.path, ImageFormat::Jpeg)
            .map_err(|error| error.to_string());

        let _ = self.result.send(result);
        control.stop();
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        tracing::warn!("prévia de captura encerrada pelo Windows");
        Ok(())
    }
}

struct Sink {
    on_event: EventSink,
    frames: Arc<AtomicU64>,
    started_at: std::time::Instant,
}

impl GraphicsCaptureApiHandler for Sink {
    type Flags = (EventSink, Arc<AtomicU64>);
    type Error = CaptureFailure;

    fn new(context: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            on_event: context.flags.0,
            frames: context.flags.1,
            started_at: std::time::Instant::now(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        self.frames.fetch_add(1, Ordering::Relaxed);

        // A hora em que o quadro foi composto, e não a em que este callback rodou: o atraso de
        // agendamento — o jogo segurando a CPU — virava variação no relógio do RTP, e quem
        // assiste aumentava a espera do jitter buffer por um tranco que a rede nem teve.
        let timestamp_ns = frame
            .timestamp()
            .ok()
            .and_then(|composed| u64::try_from(composed.Duration).ok())
            .map_or_else(|| self.started_at.elapsed().as_nanos() as u64, |hundreds| hundreds * 100);

        // A textura é da rotação interna da captura: vale enquanto este callback roda,
        // e o encoder copia dela antes de devolver. Clonar aqui só soma uma referência.
        (self.on_event)(CaptureEvent::Video(VideoFrame {
            width: frame.width(),
            height: frame.height(),
            timestamp_ns,
            surface: Some(GpuSurface {
                texture: frame.as_raw_texture().clone(),
                device: frame.device().clone(),
                context: frame.device_context().clone(),
            }),
        }));

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        tracing::warn!("captura de tela encerrada pelo Windows");
        Ok(())
    }
}

impl WindowsCapturer {
    /// Captura um quadro curto para a pessoa confirmar a tela ou janela escolhida.
    pub fn preview(source: crate::CaptureSource) -> Result<Vec<u8>, CaptureError> {
        match source {
            // Janela minimizada não tem quadro para mostrar: sem isto cada uma segurava uma
            // captura aberta por 5 s até o tempo esgotar. Vazio é "sem imagem" no seletor.
            crate::CaptureSource::Window(id) if unsafe { IsIconic(HWND(hwnd_from_id(id))) }.as_bool() => {
                Ok(Vec::new())
            }
            crate::CaptureSource::Window(id) => capture_preview(
                CaptureWindow::from_raw_hwnd(hwnd_from_id(id)),
            ),
            crate::CaptureSource::Display(id) => monitor_preview(monitor_by_id(id)?),
            crate::CaptureSource::PrimaryDisplay => monitor_preview(
                Monitor::enumerate()
                    .map_err(|error| CaptureError::Platform(error.to_string()))?
                    .into_iter()
                    .next()
                    .ok_or(CaptureError::NoDisplay)?,
            ),
            crate::CaptureSource::Camera(_) | crate::CaptureSource::Microphone => Err(
                CaptureError::Platform("no preview for camera or microphone here".into()),
            ),
        }
    }
}

/// A prévia do monitor: pelo Desktop Duplication onde a borda amarela não sai, para ela não
/// piscar no monitor cada vez que o seletor abre.
fn monitor_preview(monitor: Monitor) -> Result<Vec<u8>, CaptureError> {
    if !Duplication::needed() {
        return capture_preview(monitor);
    }

    let path = std::env::temp_dir().join(format!("unkvoid-preview-{}.jpg", std::process::id()));

    if let Err(failure) = windows_duplication::preview(monitor, &path) {
        tracing::info!(%failure, "prévia: o Desktop Duplication recusou, vai pelo Graphics Capture");

        return capture_preview(monitor);
    }

    let bytes = std::fs::read(&path).map_err(|error| CaptureError::Platform(error.to_string()))?;
    let _ = std::fs::remove_file(path);

    Ok(bytes)
}

/// O monitor pelo Desktop Duplication, com o mesmo relógio e o mesmo evento do Graphics
/// Capture: para o encoder, é só uma textura de outro device.
fn start_duplication(
    monitor: Monitor,
    config: &CaptureConfig,
    sink: EventSink,
    frames: Arc<AtomicU64>,
) -> Result<Duplication, String> {
    Duplication::start(monitor, config.frame_rate, config.show_cursor, move |frame| {
        frames.fetch_add(1, Ordering::Relaxed);

        // A hora em que a tela foi apresentada, no QPC — o mesmo relógio do Graphics Capture.
        // Contada da abertura desta captura, ela recomeçava do zero a cada troca de qualidade, e
        // o RTP de quem assiste andava um quadro só no lugar do tempo que a troca levou.
        sink(CaptureEvent::Video(VideoFrame {
            width: frame.width,
            height: frame.height,
            timestamp_ns: frame.timestamp_ns,
            surface: Some(GpuSurface {
                texture: frame.texture.clone(),
                device: frame.device.clone(),
                context: frame.context.clone(),
            }),
        }));

        ControlFlow::Continue(())
    })
}

fn capture_preview<T>(target: T) -> Result<Vec<u8>, CaptureError>
where
    T: TryInto<windows_capture::settings::GraphicsCaptureItemType> + Send + 'static,
{
        let path = std::env::temp_dir().join(format!("unkvoid-preview-{}.jpg", std::process::id()));
        let (enviado, recebido) = mpsc::channel();
        let settings = supported_settings(
            target,
            false,
            std::time::Duration::from_millis(100),
            (path.clone(), enviado),
        );

        let _control = PreviewSink::start_free_threaded(settings)
            .map_err(|error| CaptureError::Platform(error.to_string()))?;
        recebido
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| CaptureError::Platform(error.to_string()))?
            .map_err(CaptureError::Platform)?;

        let bytes = std::fs::read(&path).map_err(|error| CaptureError::Platform(error.to_string()))?;
        let _ = std::fs::remove_file(path);
        Ok(bytes)
}

impl WindowsCapturer {
    /// O tamanho da origem, para a altura da saída seguir a proporção dela.
    pub fn source_size(source: CaptureSource) -> Result<(u32, u32), CaptureError> {
        // Monitor e janela têm cada um o seu tipo de erro na crate; a mensagem basta.
        fn platform(error: impl std::fmt::Display) -> CaptureError {
            CaptureError::Platform(error.to_string())
        }

        match source {
            CaptureSource::Window(id) => {
                let window = CaptureWindow::from_raw_hwnd(hwnd_from_id(id));
                let shown = (window.width().map_err(platform)?, window.height().map_err(platform)?);

                Ok(window_size(shown, restored_placement(HWND(hwnd_from_id(id)))))
            }
            CaptureSource::Display(id) => {
                let monitor = monitor_by_id(id)?;

                Ok((monitor.width().map_err(platform)?, monitor.height().map_err(platform)?))
            }
            CaptureSource::PrimaryDisplay => {
                let monitor = Monitor::primary().map_err(|_| CaptureError::NoDisplay)?;

                Ok((monitor.width().map_err(platform)?, monitor.height().map_err(platform)?))
            }
            CaptureSource::Camera(_) | CaptureSource::Microphone => Ok((0, 0)),
        }
    }

    pub fn displays() -> Result<Vec<Display>, CaptureError> {
        let monitors =
            Monitor::enumerate().map_err(|error| CaptureError::Platform(error.to_string()))?;

        Ok(monitors
            .into_iter()
            .filter_map(|monitor| {
                Some(Display {
                    id: display_id(&monitor)?,
                    width: monitor.width().unwrap_or(0),
                    height: monitor.height().unwrap_or(0),
                })
            })
            .collect())
    }

    pub fn windows() -> Result<Vec<Window>, CaptureError> {
        let windows = CaptureWindow::enumerate()
            .map_err(|error| CaptureError::Platform(error.to_string()))?;

        Ok(windows
            .into_iter()
            .filter_map(|window| {
                let title = window.title().ok()?;

                (!title.is_empty()).then(|| Window {
                    // O HWND é a identidade. Antes vinha `0` para todas, e escolher a
                    // segunda janela da lista transmitia a primeira — ou o monitor.
                    id: id_from_hwnd(window.as_raw_hwnd()),
                    application: window.process_name().unwrap_or_default(),
                    title,
                })
            })
            .collect())
    }

    pub fn start<F>(config: &CaptureConfig, on_event: F) -> Result<Self, CaptureError>
    where
        F: Fn(CaptureEvent) + Send + Sync + 'static,
    {
        if matches!(config.source, CaptureSource::Camera(_) | CaptureSource::Microphone) {
            return Err(CaptureError::Platform("camera and microphone go through the webview here".into()));
        }

        let frames = Arc::new(AtomicU64::new(0));
        let sink: EventSink = Arc::new(on_event);

        tracing::info!(source = ?config.source, "captura: abrindo o Graphics Capture");

        let control = match config.source {
            // Quem escolheu compartilhar só o jogo não pode ter o e-mail junto: antes
            // isto era ignorado e ia sempre o monitor principal inteiro.
            CaptureSource::Window(id) => {
                let window_target = CaptureWindow::from_raw_hwnd(hwnd_from_id(id));

                // A janela pode ter sido fechada entre escolher e transmitir.
                if !window_target.is_valid() {
                    return Err(CaptureError::NoDisplay);
                }

                Running::Graphics(start_capture(window_target, config, sink.clone(), frames.clone())?)
            }
            source => {
                let monitor = match source {
                    CaptureSource::Display(id) => monitor_by_id(id)?,
                    _ => Monitor::primary().map_err(|_| CaptureError::NoDisplay)?,
                };

                // A borda amarela aparecia para quem assistia. Sem a duplicação (outra placa
                // de vídeo num notebook híbrido, por exemplo), a tela vai com ela, mas vai.
                let duplicated = Duplication::needed()
                    .then(|| start_duplication(monitor, config, sink.clone(), frames.clone()))
                    .and_then(|started| {
                        started
                            .inspect_err(|failure| {
                                tracing::warn!(%failure, "captura: o Desktop Duplication não abriu, a tela vai com a borda");
                            })
                            .ok()
                    });

                match duplicated {
                    Some(duplication) => Running::Duplication(duplication),
                    None => Running::Graphics(start_capture(monitor, config, sink.clone(), frames.clone())?),
                }
            }
        };

        let audio_chunks = Arc::new(AtomicU64::new(0));

        let scope = match config.source {
            _ if !config.mute_listed_apps => AudioScope::ExcludeSelf,
            CaptureSource::Window(id) => {
                let mut pid = 0_u32;

                unsafe { GetWindowThreadProcessId(HWND(hwnd_from_id(id)), Some(&mut pid)) };

                if pid == 0 {
                    // Janela sem dono legível: a mistura ainda deixa o app de chamada de fora.
                    AudioScope::ExceptMuted
                } else {
                    AudioScope::OnlyProcess(pid)
                }
            }
            _ => AudioScope::ExceptMuted,
        };

        // O áudio não derruba a transmissão: sem permissão ou em Windows antigo, o vídeo
        // continua e a falha fica no log. Ficar mudo é ruim; não transmitir é pior.
        tracing::info!(scope = ?scope, "captura: imagem no ar, abrindo o áudio do sistema");

        let audio = if config.capture_audio {
            match SystemAudio::start(sink, audio_chunks, scope) {
                Ok(audio) => Some(audio),
                Err(erro) => {
                    tracing::error!(erro = %erro, "sem áudio do sistema: a transmissão sai muda");

                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            control: Some(control),
            frames,
            audio,
        })
    }

    pub fn frames_captured(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    /// Erro da captura consultável depois de `start`. Só o Linux tem, porque só lá a
    /// captura é outro processo que pode morrer em silêncio.
    pub fn error(&self) -> Option<String> {
        None
    }

    /// O som do sistema no Windows vem pelo laço do WASAPI, não pelo Graphics
    /// Capture. Ele entra no caminho da mídia mais adiante.
    pub fn audio_chunks_captured(&self) -> u64 {
        self.audio.as_ref().map_or(0, SystemAudio::chunks_captured)
    }

    pub fn stop(&mut self) -> Result<(), CaptureError> {
        if let Some(mut audio) = self.audio.take() {
            audio.stop();
        }

        match self.control.take() {
            Some(Running::Graphics(control)) => {
                control.stop().map_err(|error| CaptureError::Platform(error.to_string()))?;
            }
            Some(Running::Duplication(mut duplication)) => duplication.stop(),
            None => {}
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DISPLAY_IDS, Monitor, Restored, display_id, monitor_by_id, window_size};

    /// Cada monitor da máquina volta pelo id dele, e um id antigo, de posição, não acha nenhum.
    #[test]
    fn a_monitor_comes_back_by_its_own_id_and_an_old_position_finds_none() {
        for monitor in Monitor::enumerate().expect("os monitores listaram") {
            let id = display_id(&monitor).expect("o monitor tem número");

            assert!(id > DISPLAY_IDS);
            assert_eq!(monitor_by_id(id).expect("voltou").as_raw_hmonitor(), monitor.as_raw_hmonitor());
        }

        assert!(monitor_by_id(0).is_err(), "o id de posição achou um monitor");
    }

    #[test]
    fn a_minimized_window_is_sized_by_where_it_comes_back_to() {
        let icon = (160, 28);

        // À mostra, vale o que está na tela.
        assert_eq!(window_size((1280, 720), None), (1280, 720));

        let restored = Restored { normal: (1920, 1080), work_area: None };
        assert_eq!(window_size(icon, Some(restored)), (1920, 1080));

        let restored = Restored { normal: (1000, 800), work_area: Some((1920, 1040)) };
        assert_eq!(window_size(icon, Some(restored)), (1920, 1040));

        // Retângulo invertido não vira número gigante ao perder o sinal.
        assert_eq!(window_size((-5, 10), None), (0, 10));
    }
}
