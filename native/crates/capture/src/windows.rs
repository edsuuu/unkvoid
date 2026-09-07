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

use crate::{CaptureConfig, CaptureError, CaptureEvent, Display, VideoFrame, Window};

type EventSink = Arc<dyn Fn(CaptureEvent) + Send + Sync>;

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

        (self.on_event)(CaptureEvent::Video(VideoFrame {
            width: frame.width(),
            height: frame.height(),
            timestamp_ns: self.started_at.elapsed().as_nanos() as u64,
            // No hardware encoder on Windows yet: the frame is counted, not
            // encoded. The Media Foundation path is still missing.
            surface: None,
        }));

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
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
                    id: 0,
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
        let monitor = Monitor::primary().map_err(|_| CaptureError::NoDisplay)?;
        let frames = Arc::new(AtomicU64::new(0));
        let sink: EventSink = Arc::new(on_event);

        let settings = Settings::new(
            monitor,
            if config.show_cursor {
                CursorCaptureSettings::WithCursor
            } else {
                CursorCaptureSettings::WithoutCursor
            },
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            (sink, frames.clone()),
        );

        let control = Sink::start_free_threaded(settings)
            .map_err(|error| CaptureError::Platform(error.to_string()))?;

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
