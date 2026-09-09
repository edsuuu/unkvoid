//! Bridge between the interface and native capture.
//!
//! The interface never talks to the operating system: it calls these commands, and
//! the `capture` crate handles what varies by platform.

mod broadcast;

use broadcast::Broadcast;
use capture::{CaptureSource, PlatformCapturer, Quality};
use serde::Serialize;
use tauri::{Emitter, State};

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

/// Liga a captura e o encoder. A tela sobe uma vez só, para o servidor.
///
/// `source` vem como `display:<id>` ou `window:<id>`; ausente é o monitor principal.
#[tauri::command]
async fn start_broadcast(
    state: State<'_, ActiveBroadcast>,
    quality: String,
    fps: u32,
    source: Option<String>,
) -> Result<(), String> {
    let mut active = state.0.lock().await;

    if active.is_some() {
        return Err("a stream is already in progress".into());
    }

    *active = Some(
        Broadcast::start(quality_from(&quality), fps, source_from(source.as_deref()))
            .map_err(|error| error.to_string())?,
    );

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

/// Aponta a transmissão para a porta que o servidor devolveu no `producePlain`.
#[tauri::command]
async fn use_sfu(state: State<'_, ActiveBroadcast>, address: String) -> Result<(), String> {
    let active = state.0.lock().await;

    active
        .as_ref()
        .ok_or_else(|| "no active stream".to_string())?
        .use_sfu(address)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn stop_broadcast(state: State<'_, ActiveBroadcast>) -> Result<u64, String> {
    let mut active = state.0.lock().await;

    let Some(mut broadcast) = active.take() else {
        return Ok(0);
    };

    let frames = broadcast.frames();

    broadcast.stop().map_err(|error| error.to_string())?;

    Ok(frames)
}

#[tauri::command]
async fn broadcast_stats(state: State<'_, ActiveBroadcast>) -> Result<serde_json::Value, String> {
    let active = state.0.lock().await;

    Ok(active
        .as_ref()
        .map(Broadcast::stats)
        .unwrap_or_else(|| serde_json::json!({ "active": false })))
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

#[tauri::command]
fn app_version(app: tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Ícone na bandeja: fechar a janela esconde o app em vez de matá-lo.
///
/// Sair de verdade é uma escolha explícita no menu do botão direito. Sem isto, fechar a
/// janela para voltar ao jogo mataria o processo e derrubaria a transmissão junto.
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
        // Uma cópia só. Abrir o app de novo traz a janela que já existe para a frente,
        // em vez de subir um segundo processo que disputaria a mesma captura.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(ActiveBroadcast::default())
        .invoke_handler(tauri::generate_handler![
            list_displays,
            list_windows,
            source_preview,
            app_version,
            check_update,
            restart,
            start_broadcast,
            sfu_offer,
            use_sfu,
            stop_broadcast,
            broadcast_stats
        ])
        .setup(|app| {
            build_tray(app)?;

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
