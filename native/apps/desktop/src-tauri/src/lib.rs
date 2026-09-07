//! Bridge between the interface and native capture.
//!
//! The interface never talks to the operating system: it calls these commands, and
//! the `capture` crate handles what varies by platform.

use std::sync::Mutex;

mod broadcast;

use broadcast::Broadcast;
use capture::{CaptureConfig, CaptureEvent, PlatformCapturer, Quality};
use serde::Serialize;
use tauri::{Emitter, State};

#[derive(Default)]
struct ActiveCapture(Mutex<Option<PlatformCapturer>>);

#[derive(Default)]
struct ActiveBroadcast(tokio::sync::Mutex<Option<Broadcast>>);

fn quality_from(name: &str) -> Quality {
    match name {
        "720" => Quality::Hd720,
        "1440" => Quality::Qhd1440,
        _ => Quality::Hd1080,
    }
}

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

/// Starts broadcasting. Capture and the encoder start here; connections are created
/// one per viewer in `offer_to`.
#[tauri::command]
async fn start_broadcast(
    app: tauri::AppHandle,
    state: State<'_, ActiveBroadcast>,
    quality: String,
    ice_servers: Vec<String>,
) -> Result<(), String> {
    let mut active = state.0.lock().await;

    if active.is_some() {
        return Err("a stream is already in progress".into());
    }

    let (broadcast, mut signals) =
        Broadcast::start(quality_from(&quality), ice_servers).map_err(|error| error.to_string())?;

    let handle = app.clone();

    tokio::spawn(async move {
        while let Some((peer_id, media::Signal::Candidate(json))) = signals.recv().await {
            let _ = handle.emit("p2p:signal", (peer_id, json));
        }
    });

    *active = Some(broadcast);

    Ok(())
}

/// Offer for a specific viewer. One connection per person, one encoder only.
#[tauri::command]
async fn offer_to(state: State<'_, ActiveBroadcast>, peer_id: String) -> Result<String, String> {
    let active = state.0.lock().await;

    active
        .as_ref()
        .ok_or_else(|| "no active stream".to_string())?
        .offer_to(peer_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn accept_answer(
    state: State<'_, ActiveBroadcast>,
    peer_id: String,
    sdp: String,
) -> Result<(), String> {
    let active = state.0.lock().await;

    active
        .as_ref()
        .ok_or_else(|| "no active stream".to_string())?
        .accept_answer(&peer_id, sdp)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn add_candidate(
    state: State<'_, ActiveBroadcast>,
    peer_id: String,
    candidate: String,
) -> Result<(), String> {
    let active = state.0.lock().await;

    active
        .as_ref()
        .ok_or_else(|| "no active stream".to_string())?
        .add_candidate(&peer_id, candidate)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn drop_viewer(state: State<'_, ActiveBroadcast>, peer_id: String) -> Result<(), String> {
    let active = state.0.lock().await;

    if let Some(broadcast) = active.as_ref() {
        broadcast.drop_peer(&peer_id).await;
    }

    Ok(())
}

/// What to send the server to open the plain ingest: codec, SSRC and the SRTP key.
#[tauri::command]
async fn sfu_offer(
    state: State<'_, ActiveBroadcast>,
    kind: String,
) -> Result<serde_json::Value, String> {
    let active = state.0.lock().await;

    Ok(active
        .as_ref()
        .ok_or_else(|| "no active stream".to_string())?
        .sfu_offer(&kind))
}

/// Moves the broadcast onto the server. Called when the room outgrows what direct
/// connections can carry — from here the upload no longer depends on the audience.
#[tauri::command]
async fn use_sfu(state: State<'_, ActiveBroadcast>, address: String) -> Result<(), String> {
    let active = state.0.lock().await;

    active
        .as_ref()
        .ok_or_else(|| "no active stream".to_string())?
        .use_sfu(address)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn broadcast_stats(state: State<'_, ActiveBroadcast>) -> Result<(u64, usize), String> {
    let active = state.0.lock().await;

    match active.as_ref() {
        Some(broadcast) => Ok((broadcast.frames(), broadcast.viewers().await)),
        None => Ok((0, 0)),
    }
}

#[tauri::command]
async fn stop_broadcast(state: State<'_, ActiveBroadcast>) -> Result<u64, String> {
    let mut active = state.0.lock().await;

    let Some(mut broadcast) = active.take() else {
        return Ok(0);
    };

    let frames = broadcast.frames();

    broadcast.stop().await.map_err(|error| error.to_string())?;

    Ok(frames)
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
        .map_err(|_| "corrupted capture state".to_string())?;

    if active.is_some() {
        return Err("a capture is already in progress".into());
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
        // For now the interface only needs to know that it is alive. The media
        // path (encoder + WebRTC) is added after this point.
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
        .map_err(|_| "corrupted capture state".to_string())?;

    if let Some(mut capturer) = active.take() {
        capturer.stop().map_err(|error| error.to_string())?;
    }

    Ok(())
}

/// Checks for, downloads, and installs updates before opening the app — as Discord
/// does. A network failure does not block startup: without a server the user cannot
/// use the app anyway, but blocking on the update screen would be worse than warning them.
///
/// The timeout is what keeps that promise: without it an endpoint that accepts the
/// connection and never answers holds the splash screen until the OS gives up.
#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_updater::UpdaterExt;

    let updater = app
        .updater_builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|error| error.to_string())?;

    let Some(update) = updater.check().await.map_err(|error| error.to_string())? else {
        return Ok(None);
    };

    let version = update.version.clone();

    update
        .download_and_install(|_baixado, _total| {}, || {})
        .await
        .map_err(|error| error.to_string())?;

    Ok(Some(version))
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
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .manage(ActiveCapture::default())
        .manage(ActiveBroadcast::default())
        .invoke_handler(tauri::generate_handler![
            list_displays,
            list_windows,
            start_capture,
            capture_stats,
            stop_capture,
            check_update,
            restart,
            start_broadcast,
            offer_to,
            accept_answer,
            add_candidate,
            drop_viewer,
            broadcast_stats,
            sfu_offer,
            use_sfu,
            stop_broadcast
        ])
        .run(tauri::generate_context!())
        .expect("error starting the app");
}
