//! Bridge between the interface and native capture.
//!
//! The interface never talks to the operating system: it calls these commands, and
//! the `capture` crate handles what varies by platform.

use std::sync::Mutex;

mod broadcast;
mod settings;

use broadcast::Broadcast;
use capture::{CaptureConfig, CaptureEvent, CaptureSource, PlatformCapturer, Quality};
use serde::Serialize;
use settings::Settings;
use tauri::{Emitter, State};

#[derive(Default)]
struct ActiveCapture(Mutex<Option<PlatformCapturer>>);

#[derive(Default)]
struct ActiveBroadcast(tokio::sync::Mutex<Option<Broadcast>>);

/// A interface manda `display:<id>` ou `window:<id>`; qualquer outra coisa é o monitor
/// principal, que é o caso em que ninguém escolheu nada.
fn source_from(escolha: Option<&str>) -> CaptureSource {
    match escolha.and_then(|texto| texto.split_once(':')) {
        Some(("display", id)) => id.parse().map(CaptureSource::Display).unwrap_or_default(),
        Some(("window", id)) => id.parse().map(CaptureSource::Window).unwrap_or_default(),
        _ => CaptureSource::PrimaryDisplay,
    }
}

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
    id: u64,
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

/// Miniatura do que será transmitido, como data URL para a interface mostrar.
///
/// Vazio quando a plataforma ainda não sabe gerar: o seletor abre sem imagem em vez de
/// não abrir.
#[tauri::command]
fn source_preview(source: String) -> Result<String, String> {
    let bytes =
        PlatformCapturer::preview(source_from(Some(&source))).map_err(|error| error.to_string())?;

    if bytes.is_empty() {
        return Ok(String::new());
    }

    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
    ))
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
///
/// `source` vem como `display:<id>` ou `window:<id>`; ausente é o monitor principal.
#[tauri::command]
async fn start_broadcast(
    app: tauri::AppHandle,
    state: State<'_, ActiveBroadcast>,
    quality: String,
    source: Option<String>,
    ice_servers: Vec<String>,
) -> Result<(), String> {
    let mut active = state.0.lock().await;

    if active.is_some() {
        return Err("a stream is already in progress".into());
    }

    let (broadcast, mut signals) = Broadcast::start(
        quality_from(&quality),
        source_from(source.as_deref()),
        ice_servers,
    )
    .map_err(|error| error.to_string())?;

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

    // O progresso vai para a tela. Um download de 12 MB numa conexão ruim leva minutos,
    // e sem número nenhum a tela de atualização é indistinguível de um app travado —
    // que foi exatamente a primeira reclamação que este app recebeu.
    let handle = app.clone();
    let mut baixado = 0_usize;

    update
        .download_and_install(
            move |pedaco, total| {
                baixado += pedaco;
                let _ = handle.emit("update:progress", (baixado as u64, total));
            },
            || {},
        )
        .await
        .map_err(|error| error.to_string())?;

    Ok(Some(version))
}

#[tauri::command]
fn restart(app: tauri::AppHandle) {
    app.restart();
}

/// Configurações desta máquina: atalho, monitor, iniciar com o sistema.
#[tauri::command]
fn get_setting(settings: State<'_, Settings>, key: String) -> Result<Option<String>, String> {
    settings.get(&key).map_err(|error| error.to_string())
}

#[tauri::command]
fn set_setting(settings: State<'_, Settings>, key: String, value: String) -> Result<(), String> {
    settings
        .set(&key, &value)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn all_settings(settings: State<'_, Settings>) -> Result<Vec<(String, String)>, String> {
    settings.all().map_err(|error| error.to_string())
}

/// Iniciar com o sistema. No Windows isto é a chave `Run` do registro; no macOS um
/// LaunchAgent; no Linux um `.desktop` no autostart. O plugin cuida de cada um.
#[tauri::command]
fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;

    let manager = app.autolaunch();

    if enabled {
        manager.enable().map_err(|error| error.to_string())
    } else {
        manager.disable().map_err(|error| error.to_string())
    }
}

#[tauri::command]
fn autostart_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;

    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

/// Ícone na bandeja, como o Discord: fechar a janela esconde o app em vez de matá-lo.
///
/// Sair de verdade é uma escolha explícita no menu do botão direito. Um app de voz que
/// morre ao fechar a janela derruba a chamada de quem só queria tirar a janela da frente.
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let open = MenuItem::with_id(app, "open", "Open Unkvoid", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Unkvoid", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().ok_or_else(|| {
            tauri::Error::Anyhow(anyhow::anyhow!("the bundle has no icon for the tray"))
        })?)
        .tooltip("Unkvoid")
        .menu(&menu)
        // false: no Windows o clique esquerdo abriria o menu, e o esperado é abrir o app.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    use tauri::Manager;

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt().with_env_filter("info").init();

    tauri::Builder::default()
        // Precisa ser o PRIMEIRO plugin. Sem ele, o navegador devolvendo `discord2://`
        // faz o sistema abrir uma SEGUNDA cópia do app, que começa do zero na tela de
        // atualização por cima de quem acabou de fazer login. Com ele, a segunda cópia
        // entrega o link para a que já está aberta e sai.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        // `--minimized` é o que o autostart passa: subir com o sistema não pode jogar
        // uma janela na cara de quem acabou de ligar o computador.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .manage(ActiveCapture::default())
        .manage(ActiveBroadcast::default())
        .invoke_handler(tauri::generate_handler![
            list_displays,
            list_windows,
            source_preview,
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
            stop_broadcast,
            get_setting,
            set_setting,
            all_settings,
            set_autostart,
            autostart_enabled
        ])
        .setup(|app| {
            use tauri::Manager;

            // Registra o esquema em tempo de execução: sem isto o sistema continua
            // entregando `discord2://` para a cópia que registrou primeiro — uma pasta
            // de build, um DMG montado — em vez desta.
            #[cfg(desktop)]
            {
                use tauri_plugin_deep_link::DeepLinkExt;

                let _ = app.deep_link().register_all();
            }

            let banco = app.path().app_data_dir()?.join("settings.db");

            app.manage(Settings::open(banco)?);

            build_tray(app)?;

            // Iniciado pelo sistema: fica só na bandeja. Quem abriu no clique quer ver
            // a janela; quem acabou de ligar o computador, não.
            if std::env::args().any(|argument| argument == "--minimized")
                && let Some(window) = app.get_webview_window("main")
            {
                let _ = window.hide();
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // Fechar esconde; quem quer sair usa o menu da bandeja. Sem o prevent_close
            // o processo morre e a chamada cai junto.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error starting the app");
}
