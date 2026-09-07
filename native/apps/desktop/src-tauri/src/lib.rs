//! Ponte entre a interface e a captura nativa.
//!
//! A interface nunca fala com o sistema operacional: ela chama estes comandos, e a
//! crate `capture` cuida do que muda por plataforma.

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

fn quality_from(nome: &str) -> Quality {
    match nome {
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

/// Começa a transmitir. A captura e o encoder sobem aqui; as conexões nascem uma
/// por espectador em `offer_to`.
#[tauri::command]
async fn start_broadcast(
    app: tauri::AppHandle,
    state: State<'_, ActiveBroadcast>,
    quality: String,
    ice_servers: Vec<String>,
) -> Result<(), String> {
    let mut ativo = state.0.lock().await;

    if ativo.is_some() {
        return Err("já existe uma transmissão em andamento".into());
    }

    let (transmissao, mut sinais) =
        Broadcast::start(quality_from(&quality), ice_servers).map_err(|erro| erro.to_string())?;

    let handle = app.clone();

    tokio::spawn(async move {
        while let Some((peer_id, media::Signal::Candidate(json))) = sinais.recv().await {
            let _ = handle.emit("p2p:signal", (peer_id, json));
        }
    });

    *ativo = Some(transmissao);

    Ok(())
}

/// Oferta para um espectador específico. Uma conexão por pessoa, um encoder só.
#[tauri::command]
async fn offer_to(state: State<'_, ActiveBroadcast>, peer_id: String) -> Result<String, String> {
    let ativo = state.0.lock().await;

    ativo
        .as_ref()
        .ok_or_else(|| "nenhuma transmissão ativa".to_string())?
        .offer_to(peer_id)
        .await
        .map_err(|erro| erro.to_string())
}

#[tauri::command]
async fn accept_answer(
    state: State<'_, ActiveBroadcast>,
    peer_id: String,
    sdp: String,
) -> Result<(), String> {
    let ativo = state.0.lock().await;

    ativo
        .as_ref()
        .ok_or_else(|| "nenhuma transmissão ativa".to_string())?
        .accept_answer(&peer_id, sdp)
        .await
        .map_err(|erro| erro.to_string())
}

#[tauri::command]
async fn add_candidate(
    state: State<'_, ActiveBroadcast>,
    peer_id: String,
    candidate: String,
) -> Result<(), String> {
    let ativo = state.0.lock().await;

    ativo
        .as_ref()
        .ok_or_else(|| "nenhuma transmissão ativa".to_string())?
        .add_candidate(&peer_id, candidate)
        .await
        .map_err(|erro| erro.to_string())
}

#[tauri::command]
async fn drop_viewer(state: State<'_, ActiveBroadcast>, peer_id: String) -> Result<(), String> {
    let ativo = state.0.lock().await;

    if let Some(transmissao) = ativo.as_ref() {
        transmissao.drop_peer(&peer_id).await;
    }

    Ok(())
}

#[tauri::command]
async fn broadcast_stats(state: State<'_, ActiveBroadcast>) -> Result<(u64, usize), String> {
    let ativo = state.0.lock().await;

    match ativo.as_ref() {
        Some(transmissao) => Ok((transmissao.frames(), transmissao.viewers().await)),
        None => Ok((0, 0)),
    }
}

#[tauri::command]
async fn stop_broadcast(state: State<'_, ActiveBroadcast>) -> Result<u64, String> {
    let mut ativo = state.0.lock().await;

    let Some(mut transmissao) = ativo.take() else {
        return Ok(0);
    };

    let quadros = transmissao.frames();

    transmissao.stop().await.map_err(|erro| erro.to_string())?;

    Ok(quadros)
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
            stop_broadcast
        ])
        .run(tauri::generate_context!())
        .expect("erro ao subir o app");
}
