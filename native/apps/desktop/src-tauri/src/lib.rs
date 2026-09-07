//! Ponte entre a interface e a captura nativa.
//!
//! A interface nunca fala com o sistema operacional: ela chama estes comandos, e a
//! crate `capture` cuida do que muda por plataforma.

use std::sync::Mutex;

use capture::{CaptureConfig, CaptureEvent, PlatformCapturer, Quality};
use serde::Serialize;
use tauri::{Emitter, State};

#[derive(Default)]
struct ActiveCapture(Mutex<Option<PlatformCapturer>>);

#[derive(Serialize)]
struct DisplayInfo {
    id: u32,
    width: u32,
    height: u32,
}

#[derive(Serialize)]
struct WindowInfo {
    id: u32,
    title: String,
    application: String,
}

#[derive(Serialize, Clone)]
struct CaptureStats {
    frames: u64,
    audio_chunks: u64,
    width: u32,
    height: u32,
}

#[tauri::command]
fn list_displays() -> Result<Vec<DisplayInfo>, String> {
    PlatformCapturer::displays()
        .map(|displays| {
            displays
                .into_iter()
                .map(|display| DisplayInfo {
                    id: display.id,
                    width: display.width,
                    height: display.height,
                })
                .collect()
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_windows() -> Result<Vec<WindowInfo>, String> {
    PlatformCapturer::windows()
        .map(|windows| {
            windows
                .into_iter()
                .map(|window| WindowInfo {
                    id: window.id,
                    title: window.title,
                    application: window.application,
                })
                .collect()
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_capture(
    app: tauri::AppHandle,
    state: State<'_, ActiveCapture>,
    quality: String,
) -> Result<(), String> {
    let mut active = state
        .0
        .lock()
        .map_err(|_| "estado de captura corrompido".to_string())?;

    if active.is_some() {
        return Err("já existe uma captura em andamento".into());
    }

    let config = CaptureConfig {
        quality: match quality.as_str() {
            "720" => Quality::Hd720,
            "1440" => Quality::Qhd1440,
            _ => Quality::Hd1080,
        },
        ..CaptureConfig::default()
    };

    let handle = app.clone();

    let capturer = PlatformCapturer::start(&config, move |event| {
        // Por enquanto a interface só precisa saber que está vivo. O caminho de
        // mídia (encoder + WebRTC) entra depois deste ponto.
        if let CaptureEvent::Video(frame) = event {
            let _ = handle.emit("capture:frame", (frame.width, frame.height));
        }
    })
    .map_err(|error| error.to_string())?;

    *active = Some(capturer);

    Ok(())
}

#[tauri::command]
fn capture_stats(state: State<'_, ActiveCapture>) -> Option<CaptureStats> {
    let active = state.0.lock().ok()?;
    let capturer = active.as_ref()?;

    Some(CaptureStats {
        frames: capturer.frames_captured(),
        audio_chunks: capturer.audio_chunks_captured(),
        width: 0,
        height: 0,
    })
}

#[tauri::command]
fn stop_capture(state: State<'_, ActiveCapture>) -> Result<(), String> {
    let mut active = state
        .0
        .lock()
        .map_err(|_| "estado de captura corrompido".to_string())?;

    if let Some(mut capturer) = active.take() {
        capturer.stop().map_err(|error| error.to_string())?;
    }

    Ok(())
}

/// Procura, baixa e instala atualização antes de liberar o app — do jeito que o
/// Discord faz. Falha de rede não trava a abertura: sem servidor a pessoa não vai
/// conseguir usar mesmo, mas travar na tela de update seria pior que entrar e avisar.
#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_updater::UpdaterExt;

    let updater = app.updater().map_err(|error| error.to_string())?;

    let Some(update) = updater.check().await.map_err(|error| error.to_string())? else {
        return Ok(None);
    };

    let versao = update.version.clone();

    update
        .download_and_install(|_baixado, _total| {}, || {})
        .await
        .map_err(|error| error.to_string())?;

    Ok(Some(versao))
}

#[tauri::command]
fn restart(app: tauri::AppHandle) {
    app.restart();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt().with_env_filter("info").init();

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(ActiveCapture::default())
        .invoke_handler(tauri::generate_handler![
            list_displays,
            list_windows,
            start_capture,
            capture_stats,
            stop_capture,
            check_update,
            restart
        ])
        .run(tauri::generate_context!())
        .expect("erro ao subir o app");
}
