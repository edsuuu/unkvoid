use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::path::PathBuf;

use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::ImageFormat;
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window as CaptureWindow;

use ::windows::Win32::Foundation::HWND;
use ::windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

use crate::windows_audio::{AudioScope, SystemAudio};
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
    let settings = Settings::new(
        target,
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

    Sink::start_free_threaded(settings).map_err(|error| {
        tracing::error!(error = %error, "Windows Graphics Capture recusou a captura");
        CaptureError::Platform(error.to_string())
    })
}

/// Capture via Windows Graphics Capture. Requires Windows 10 1903 or newer.
pub struct WindowsCapturer {
    control: Option<windows_capture::capture::CaptureControl<Sink, CaptureFailure>>,
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
        tracing::warn!("captura de tela encerrada pelo Windows");
        Ok(())
    }
}

impl WindowsCapturer {
    /// Captura um quadro curto para a pessoa confirmar a tela ou janela escolhida.
    pub fn preview(source: crate::CaptureSource) -> Result<Vec<u8>, CaptureError> {
        match source {
            crate::CaptureSource::Window(id) => capture_preview(
                CaptureWindow::from_raw_hwnd(hwnd_from_id(id)),
            ),
            crate::CaptureSource::Display(id) => capture_preview(
                Monitor::enumerate()
                    .map_err(|error| CaptureError::Platform(error.to_string()))?
                    .into_iter()
                    .nth(id as usize)
                    .ok_or(CaptureError::NoDisplay)?,
            ),
            crate::CaptureSource::PrimaryDisplay => capture_preview(
                Monitor::enumerate()
                    .map_err(|error| CaptureError::Platform(error.to_string()))?
                    .into_iter()
                    .next()
                    .ok_or(CaptureError::NoDisplay)?,
            ),
        }
    }
}

fn capture_preview<T>(target: T) -> Result<Vec<u8>, CaptureError>
where
    T: TryInto<windows_capture::settings::GraphicsCaptureItemType> + Send + 'static,
{
        let path = std::env::temp_dir().join(format!("unkvoid-preview-{}.jpg", std::process::id()));
        let (enviado, recebido) = mpsc::channel();
        let settings = Settings::new(
            target,
            CursorCaptureSettings::WithoutCursor,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Custom(std::time::Duration::from_millis(100)),
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
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
        let frames = Arc::new(AtomicU64::new(0));
        let sink: EventSink = Arc::new(on_event);

        tracing::info!(source = ?config.source, "captura: abrindo o Graphics Capture");

        // Monitor e janela são tipos diferentes, mas `iniciar` é genérico e devolve o
        // mesmo controle para os dois.
        let control = match config.source {
            // Quem escolheu compartilhar só o jogo não pode ter o e-mail junto: antes
            // isto era ignorado e ia sempre o monitor principal inteiro.
            CaptureSource::Window(id) => {
                let window_target = CaptureWindow::from_raw_hwnd(hwnd_from_id(id));

                // A janela pode ter sido fechada entre escolher e transmitir.
                if !window_target.is_valid() {
                    return Err(CaptureError::NoDisplay);
                }

                start_capture(window_target, config, sink.clone(), frames.clone())?
            }
            source => {
                let monitor = match source {
                    // `displays()` numera a partir de zero; `from_index` conta de um.
                    CaptureSource::Display(index) => Monitor::from_index(index as usize + 1),
                    _ => Monitor::primary(),
                }
                .map_err(|_| CaptureError::NoDisplay)?;

                start_capture(monitor, config, sink.clone(), frames.clone())?
            }
        };

        let audio_chunks = Arc::new(AtomicU64::new(0));

        // Compartilhar uma janela com o áudio de chamada de fora vira a pergunta do
        // avesso: em vez de excluir o Discord — o que o Windows não deixa fazer junto
        // com excluir a nós mesmos — grava-se só a árvore do processo daquela janela.
        // Entra o som do jogo, e Discord, navegador e nós ficamos de fora de graça.
        let scope = match config.source {
            CaptureSource::Window(id) if config.mute_listed_apps => {
                let mut pid = 0_u32;

                unsafe { GetWindowThreadProcessId(HWND(hwnd_from_id(id)), Some(&mut pid)) };

                if pid == 0 {
                    // Janela sem dono legível: melhor gravar tudo menos nós do que nada.
                    AudioScope::ExcludeSelf
                } else {
                    AudioScope::OnlyProcess(pid)
                }
            }
            _ => AudioScope::ExcludeSelf,
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

        if let Some(control) = self.control.take() {
            control
                .stop()
                .map_err(|error| CaptureError::Platform(error.to_string()))?;
        }

        Ok(())
    }
}
