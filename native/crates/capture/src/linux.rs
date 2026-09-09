use crate::{CaptureConfig, CaptureError, CaptureEvent, Display, Window};

/// A captura no Linux passa pelo portal XDG (`org.freedesktop.portal.ScreenCast`),
/// que devolve um nó do PipeWire. É o único caminho que funciona sob Wayland.
///
/// Estado: o diálogo e a listagem do portal respondem, mas consumir os quadros do
/// PipeWire ainda não está ligado — então `start` recusa em vez de fingir que captura.
/// users see a clear error, not a black screen.
pub struct LinuxCapturer;

impl LinuxCapturer {
    /// Sem miniatura fora do macOS ainda. Devolver vazio em vez de erro deixa o
    /// seletor abrir listando os nomes — pior que com preview, melhor que quebrado.
    pub fn preview(_source: crate::CaptureSource) -> Result<Vec<u8>, CaptureError> {
        Ok(Vec::new())
    }

    pub fn displays() -> Result<Vec<Display>, CaptureError> {
        // O portal não expõe a lista antes da escolha: quem mostra o seletor é ele.
        // Devolver lista vazia deixa a interface abrir o diálogo do sistema.
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
