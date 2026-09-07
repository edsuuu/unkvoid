use crate::{CaptureConfig, CaptureError, CaptureEvent, Display, Window};

/// Captura no Linux passa pelo portal XDG (`org.freedesktop.portal.ScreenCast`),
/// que devolve um nó do PipeWire. É o único caminho que funciona sob Wayland.
///
/// Estado: o diálogo do portal e a listagem já respondem, mas o consumo dos quadros
/// do PipeWire ainda não está ligado — por isso `start` recusa em vez de fingir que
/// capturou. Quem usa Linux vê um erro claro, não uma tela preta.
pub struct LinuxCapturer;

impl LinuxCapturer {
    pub fn displays() -> Result<Vec<Display>, CaptureError> {
        // O portal não expõe a lista antes do usuário escolher: é ele quem mostra o
        // seletor. Devolver vazio faz a interface pular direto para o diálogo.
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
            "captura no Linux ainda não implementada — falta consumir o nó do PipeWire".into(),
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
