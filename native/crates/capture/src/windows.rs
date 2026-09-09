use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window as CaptureWindow;

use crate::{
    CaptureConfig, CaptureError, CaptureEvent, CaptureSource, Display, GpuSurface, VideoFrame,
    Window,
};

type EventSink = Arc<dyn Fn(CaptureEvent) + Send + Sync>;

/// `HWND` é ponteiro, e o identificador que atravessa a interface é número. A Microsoft
/// garante que handles cabem em 32 bits com sinal estendido justamente para poderem
/// atravessar fronteiras de 32/64 bits, então a ida e a volta são seguras — e só valem
/// dentro deste processo, que é onde a escolha é feita e usada.
fn hwnd_para_id(hwnd: *mut std::ffi::c_void) -> u64 {
    hwnd as usize as u64
}

fn id_para_hwnd(id: u64) -> *mut std::ffi::c_void {
    id as usize as *mut std::ffi::c_void
}

/// Liga a captura no alvo escolhido. Genérica porque monitor e janela são tipos
/// distintos para o `windows-capture`, mas produzem o mesmo controle.
fn iniciar<T>(
    alvo: T,
    config: &CaptureConfig,
    sink: EventSink,
    frames: Arc<AtomicU64>,
) -> Result<windows_capture::capture::CaptureControl<Sink, CaptureFailure>, CaptureError>
where
    T: TryInto<windows_capture::settings::GraphicsCaptureItemType> + Send + 'static,
{
    let settings = Settings::new(
        alvo,
        if config.show_cursor {
            CursorCaptureSettings::WithCursor
        } else {
            CursorCaptureSettings::WithoutCursor
        },
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        // O teto de quadros começa aqui: pedir 30 e deixar a captura entregar 60 faria o
        // encoder jogar metade fora depois de já ter pago por ela.
        MinimumUpdateIntervalSettings::Custom(std::time::Duration::from_secs_f64(
            1.0 / f64::from(config.frame_rate.max(1)),
        )),
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        (sink, frames),
    );

    Sink::start_free_threaded(settings).map_err(|error| CaptureError::Platform(error.to_string()))
}

/// Capture via Windows Graphics Capture. Requires Windows 10 1903 or newer.
pub struct WindowsCapturer {
    control: Option<windows_capture::capture::CaptureControl<Sink, CaptureFailure>>,
    frames: Arc<AtomicU64>,
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

        // A textura é da rotação interna da captura: vale enquanto este callback roda,
        // e o encoder copia dela antes de devolver. Clonar aqui só soma uma referência.
        (self.on_event)(CaptureEvent::Video(VideoFrame {
            width: frame.width(),
            height: frame.height(),
            timestamp_ns: self.started_at.elapsed().as_nanos() as u64,
            surface: Some(GpuSurface {
                texture: frame.as_raw_texture().clone(),
                device: frame.device().clone(),
                context: frame.device_context().clone(),
            }),
        }));

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl WindowsCapturer {
    /// Sem miniatura fora do macOS ainda. Devolver vazio em vez de erro deixa o
    /// seletor abrir listando os nomes — pior que com preview, melhor que quebrado.
    pub fn preview(_source: crate::CaptureSource) -> Result<Vec<u8>, CaptureError> {
        Ok(Vec::new())
    }

    pub fn displays() -> Result<Vec<Display>, CaptureError> {
        let monitors =
            Monitor::enumerate().map_err(|error| CaptureError::Platform(error.to_string()))?;

        Ok(monitors
            .into_iter()
            .enumerate()
            .map(|(index, monitor)| Display {
                id: index as u32,
                width: monitor.width().unwrap_or(0),
                height: monitor.height().unwrap_or(0),
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
                    id: hwnd_para_id(window.as_raw_hwnd()),
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
        let frames = Arc::new(AtomicU64::new(0));
        let sink: EventSink = Arc::new(on_event);

        // Monitor e janela são tipos diferentes, mas `iniciar` é genérico e devolve o
        // mesmo controle para os dois.
        let control = match config.source {
            // Quem escolheu compartilhar só o jogo não pode ter o e-mail junto: antes
            // isto era ignorado e ia sempre o monitor principal inteiro.
            CaptureSource::Window(id) => {
                let janela = CaptureWindow::from_raw_hwnd(id_para_hwnd(id));

                // A janela pode ter sido fechada entre escolher e transmitir.
                if !janela.is_valid() {
                    return Err(CaptureError::NoDisplay);
                }

                iniciar(janela, config, sink, frames.clone())?
            }
            source => {
                let monitor = match source {
                    // `displays()` numera a partir de zero; `from_index` conta de um.
                    CaptureSource::Display(index) => Monitor::from_index(index as usize + 1),
                    _ => Monitor::primary(),
                }
                .map_err(|_| CaptureError::NoDisplay)?;

                iniciar(monitor, config, sink, frames.clone())?
            }
        };

        Ok(Self {
            control: Some(control),
            frames,
        })
    }

    pub fn frames_captured(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    /// System audio on Windows comes through WASAPI loopback, not Graphics Capture.
    /// It joins the media path.
    pub fn audio_chunks_captured(&self) -> u64 {
        0
    }

    pub fn stop(&mut self) -> Result<(), CaptureError> {
        if let Some(control) = self.control.take() {
            control
                .stop()
                .map_err(|error| CaptureError::Platform(error.to_string()))?;
        }

        Ok(())
    }
}
