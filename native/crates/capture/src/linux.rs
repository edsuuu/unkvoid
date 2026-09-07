use crate::{CaptureConfig, CaptureError, CaptureEvent, Display, Window};

/// Linux capture goes through the XDG portal (`org.freedesktop.portal.ScreenCast`),
/// which returns a PipeWire node. It is the only path that works under Wayland.
///
/// Status: the portal dialog and listing respond, but consuming PipeWire frames is
/// not connected yet — so `start` refuses instead of pretending to capture. Linux
/// users see a clear error, not a black screen.
pub struct LinuxCapturer;

impl LinuxCapturer {
    /// Sem miniatura fora do macOS ainda. Devolver vazio em vez de erro deixa o
    /// seletor abrir listando os nomes — pior que com preview, melhor que quebrado.
    pub fn preview(_source: crate::CaptureSource) -> Result<Vec<u8>, CaptureError> {
        Ok(Vec::new())
    }

    pub fn displays() -> Result<Vec<Display>, CaptureError> {
        // The portal does not expose the list before the user chooses: it displays
        // the selector. Returning an empty list lets the interface open the dialog.
        Ok(Vec::new())
    }

    pub fn windows() -> Result<Vec<Window>, CaptureError> {
        Ok(Vec::new())
    }

    pub fn start<F>(_config: &CaptureConfig, _on_event: F) -> Result<Self, CaptureError>
    where
        F: Fn(CaptureEvent) + Send + Sync + 'static,
    {
        Err(CaptureError::Platform(
            "Linux capture is not implemented yet — PipeWire node consumption is missing".into(),
        ))
    }

    pub fn frames_captured(&self) -> u64 {
        0
    }

    pub fn audio_chunks_captured(&self) -> u64 {
        0
    }

    pub fn stop(&mut self) -> Result<(), CaptureError> {
        Ok(())
    }
}
