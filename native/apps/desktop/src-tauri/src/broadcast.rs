//! Os comandos do Tauri que expõem a transmissão à janela.
//!
//! A lógica mora em `core_app::sharing`: ela é a mesma nos três sistemas e nas quatro
//! interfaces, e aqui só há a casca que o Tauri precisa.

use std::sync::atomic::{AtomicBool, Ordering};

use capture::CaptureSource;
use media::Source;
use tauri::{Emitter, State};

pub use core_app::sharing::*;

/// O microfone padrão do sistema, pelo Rust. Enquanto ele está aberto sai o evento
/// `voice:level` com `{ level }`: o RMS linear de 0 a 1, uns dez por segundo, mutado ou não.
///
/// ponytail: sempre o `@DEFAULT_SOURCE@`; um seletor de microfone traria o `device`.
#[tauri::command]
pub async fn start_voice(app: tauri::AppHandle, state: State<'_, ActiveSession>) -> Result<(), String> {
    let mut session = state.0.lock().await;

    if session.voice.is_some() {
        return Ok(());
    }

    let voice = start_native(&session, CaptureSource::Microphone, None, Some(Source::Mic))?;
    let emit_failed = AtomicBool::new(false);

    voice.on_level(move |level| {
        if let Err(error) = app.emit("voice:level", serde_json::json!({ "level": level }))
            && ! emit_failed.swap(true, Ordering::Relaxed)
        {
            tracing::warn!(error = %error, "voz: o nível do microfone não chegou à interface");
        }
    });

    session.voice = Some(voice);

    Ok(())
}

#[tauri::command]
pub async fn stop_voice(state: State<'_, ActiveSession>) -> Result<(), String> {
    let mut session = state.0.lock().await;

    if let Some(mut voice) = session.voice.take() {
        tokio::task::block_in_place(|| voice.stop()).map_err(|error| error.to_string())?;
    }

    session.release_if_idle();

    Ok(())
}

#[tauri::command]
pub async fn set_voice_muted(state: State<'_, ActiveSession>, muted: bool) -> Result<(), String> {
    if let Some(voice) = state.0.lock().await.voice.as_ref() {
        voice.set_muted(muted);
    }

    Ok(())
}

/// O índice de um `id` que `list_cameras` devolveu (`/dev/video<n>`).
pub fn camera_index(device: &str) -> Result<u32, String> {
    device
        .trim_start_matches("/dev/video")
        .parse()
        .map_err(|_| format!("câmera desconhecida: {device}"))
}

/// A câmera, pelo Rust. Pedir outra com uma já ligada troca: a que estava para antes.
#[tauri::command]
pub async fn start_camera(state: State<'_, ActiveSession>, device: String) -> Result<(), String> {
    let source = CaptureSource::Camera(camera_index(&device)?);
    let mut session = state.0.lock().await;

    match session.camera.take() {
        Some(camera) if camera.source == source => {
            session.camera = Some(camera);

            return Ok(());
        }
        Some(mut camera) => tokio::task::block_in_place(|| camera.stop()).map_err(|error| error.to_string())?,
        None => {}
    }

    let camera = start_native(&session, source, Some(Source::Camera), None)?;

    session.camera = Some(camera);

    Ok(())
}

#[tauri::command]
pub async fn stop_camera(state: State<'_, ActiveSession>) -> Result<(), String> {
    let mut session = state.0.lock().await;

    if let Some(mut camera) = session.camera.take() {
        tokio::task::block_in_place(|| camera.stop()).map_err(|error| error.to_string())?;
    }

    session.release_if_idle();

    Ok(())
}
