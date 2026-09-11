//! A ponte entre a interface e a captura nativa.
//!
//! A interface nunca fala com o sistema operacional: ela chama estes comandos, e o
//! crate `capture` cuida do que muda de plataforma para plataforma.

mod broadcast;
mod logbook;

use std::sync::atomic::{AtomicBool, Ordering};

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

/// A interface manda para cá o mesmo diagnóstico que mostra na janela de logs.
///
/// Sem isto ele vivia só na memória da webview, com teto de linhas: o app caía e o
/// diagnóstico do que aconteceu caía junto, que é exatamente o momento em que ele
/// importa.
#[tauri::command]
fn log_line(line: String) {
    logbook::write(&line);
}

/// Onde o arquivo mora, para a janela de diagnóstico dizer à pessoa o que anexar.
#[tauri::command]
fn log_path() -> String {
    logbook::path().to_string_lossy().into_owned()
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
    audio: bool,
    mute_calls: bool,
) -> Result<(), String> {
    let mut active = state.0.lock().await;

    if active.is_some() {
        return Err("a stream is already in progress".into());
    }

    // Registrado ANTES de chamar. No Windows a captura e o encoder são COM e Direct3D:
    // quando um deles derruba o processo não há erro para devolver nem pânico para o
    // hook pegar, e a única prova do que estava acontecendo é a linha já em disco.
    tracing::info!(
        %quality,
        fps,
        source = source.as_deref().unwrap_or("primary"),
        audio,
        mute_calls,
        "broadcast: ligando captura e encoder"
    );

    let started = Broadcast::start(
        quality_from(&quality),
        fps,
        source_from(source.as_deref()),
        audio,
        mute_calls,
    );

    match started {
        Ok(broadcast) => {
            *active = Some(broadcast);
            tracing::info!("broadcast: no ar");

            Ok(())
        }
        Err(error) => {
            tracing::error!(error = %error, "broadcast: não subiu");

            Err(error.to_string())
        }
    }
}

/// O que mandar ao servidor para abrir o ingest puro: codec, SSRC e a chave SRTP.
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

/// Chave SRTP nova antes de republicar num servidor que reiniciou.
///
/// Ver `Broadcast::renew_sfu_key`: reapontar o destino recomeça a numeração dos pacotes,
/// e repetir a chave com o contador zerado repetiria o keystream.
#[tauri::command]
async fn renew_sfu_key(state: State<'_, ActiveBroadcast>) -> Result<(), String> {
    let mut active = state.0.lock().await;

    active
        .as_mut()
        .ok_or_else(|| "no active stream".to_string())?
        .renew_sfu_key();

    Ok(())
}

/// Aponta a transmissão para a porta que o servidor devolveu no `producePlain`.
///
/// `server_key` é a chave SRTP de SAÍDA do servidor, que vem na mesma resposta. É com ela
/// que este lado abre o caminho de volta e enxerga o pedido de quadro-chave — sem ela a
/// transmissão sobe igual, só demora mais a se recompor de uma perda.
#[tauri::command]
async fn use_sfu(
    state: State<'_, ActiveBroadcast>,
    address: String,
    server_key: Option<String>,
) -> Result<(), String> {
    let active = state.0.lock().await;

    let key = server_key
        .map(|value| {
            use base64::Engine;

            base64::engine::general_purpose::STANDARD
                .decode(value)
                .map_err(|error| format!("chave do servidor ilegível: {error}"))
        })
        .transpose()?;

    active
        .as_ref()
        .ok_or_else(|| "no active stream".to_string())?
        .use_sfu(address, key)
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

/// Procura, baixa e instala a atualização antes de abrir o app, como o Discord faz.
/// Falha de rede não impede a abertura: sem servidor a pessoa não usa o app de todo
/// jeito, mas travar na tela de atualização seria pior do que avisar.
///
/// O prazo é o que cumpre essa promessa: sem ele, um endereço que aceita a conexão e
/// nunca responde segura a tela de abertura até o sistema desistir sozinho.
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

/// Tamanho da janela depois que o app está pronto para uso.
const APP_SIZE: (f64, f64) = (1280.0, 800.0);

/// Cresce a janela quando a abertura termina.
///
/// Ela nasce pequena de propósito: procurar atualização numa janela de 1280 por 800
/// vazia parece um app travado, e não um app carregando.
#[tauri::command]
fn expand_window(window: tauri::Window) -> Result<(), String> {
    use tauri::LogicalSize;

    let paint = |erro: tauri::Error| erro.to_string();

    // Sem piso de tamanho aqui. Definir um mínimo enquanto a janela ainda é a pequena
    // fazia o macOS crescê-la até o próprio mínimo e engolir este `set_size`: ela
    // parava em 940x600 em vez de 1280x800, nas duas ordens possíveis. O piso é um
    // luxo; abrir do tamanho certo não é.
    window
        .set_size(LogicalSize::new(APP_SIZE.0, APP_SIZE.1))
        .map_err(paint)?;

    // O `center()` usa o tamanho que a janela tem na hora da chamada, e no macOS o
    // redimensionamento ainda não terminou aqui — centralizava pelo tamanho antigo e a
    // janela ficava para o canto. A conta pelo monitor não depende desse tempo.
    center_on_monitor(&window, APP_SIZE);

    Ok(())
}

/// Põe a janela no meio do monitor, pelo tamanho que ela **vai** ter.
///
/// Ler `outer_size()` aqui não serve: no macOS o redimensionamento ainda não terminou,
/// então a conta saía com o tamanho da janela pequena e a janela ia parar no canto. O
/// tamanho alvo é constante e conhecido, então ele não depende desse tempo.
fn center_on_monitor(window: &tauri::Window, size: (f64, f64)) {
    use tauri::PhysicalPosition;

    let (Ok(Some(monitor)), Ok(scale)) = (window.primary_monitor(), window.scale_factor()) else {
        // Sem monitor legível não dá para calcular; o `center` do sistema ainda é
        // melhor do que deixar onde está.
        let _ = window.center();

        return;
    };

    let screen = monitor.size();
    let origin = monitor.position();
    let width = size.0 * scale;
    let height = size.1 * scale;

    let x = origin.x + ((screen.width as f64 - width) / 2.0).max(0.0) as i32;
    let y = origin.y + ((screen.height as f64 - height) / 2.0).max(0.0) as i32;

    let _ = window.set_position(PhysicalPosition::new(x, y));
}

/// Se a bandeja existe nesta máquina. É o que decide se fechar a janela esconde ou sai.
struct HasTray(AtomicBool);

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

/// Liga o WebRTC do WebKitGTK.
///
/// No Linux a janela do app é WebKitGTK, e ele entrega WebRTC **desligado**: sem isto
/// `RTCPeerConnection` não existe na página, o mediasoup-client falha ao montar o
/// transporte, e a pessoa não consegue nem assistir. Nada disso aparece como erro de
/// permissão ou de rede — a API simplesmente não está lá.
///
/// `enable_media_stream` vai junto porque é o que libera `getUserMedia` e as faixas de
/// mídia; ligar um sem o outro deixa a metade do caminho aberta.
#[cfg(target_os = "linux")]
fn enable_webrtc(app: &tauri::AppHandle) {
    use tauri::Manager;
    use webkit2gtk::{SettingsExt, WebViewExt};

    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    // O `with_webview` enfileira a closure no laço do GTK, que só começa a rodar
    // depois do `setup`. Ou seja: isto acontece DEPOIS de a página já ter nascido, e a
    // página que nasceu sem WebRTC continua sem ele — a configuração vale para a
    // próxima carga. Por isso a interface recarrega uma vez quando não acha o
    // `RTCPeerConnection`, e por isso estas linhas de log existem: sem elas não há como
    // saber, de fora, se o problema foi a ordem ou o WebKit da distro.
    let outcome = window.with_webview(|webview| match WebViewExt::settings(&webview.inner()) {
        Some(settings) => {
            settings.set_enable_webrtc(true);
            settings.set_enable_media_stream(true);

            tracing::info!(
                webrtc = settings.enables_webrtc(),
                media_stream = settings.enables_media_stream(),
                "configuração do WebKitGTK aplicada",
            );
        }
        None => tracing::error!("a webview do WebKitGTK não devolveu configuração"),
    });

    if let Err(failure) = outcome {
        tracing::error!(failure = %failure, "não deu para falar com a webview do WebKitGTK");
    }
}

#[cfg(not(target_os = "linux"))]
fn enable_webrtc(_app: &tauri::AppHandle) {}

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
    logbook::init();

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
            renew_sfu_key,
            use_sfu,
            stop_broadcast,
            broadcast_stats,
            expand_window,
            log_line,
            log_path
        ])
        .manage(HasTray(AtomicBool::new(false)))
        .setup(|app| {
            use tauri::Manager;

            // A bandeja não pode derrubar o app. No GNOME sem a extensão de
            // AppIndicator ela simplesmente não existe, e propagar o erro daqui fazia o
            // `run` entrar em pânico: nenhuma janela, nenhuma mensagem, nada.
            match build_tray(app) {
                Ok(()) => app.state::<HasTray>().0.store(true, Ordering::Relaxed),
                Err(failure) => tracing::error!(failure = %failure, "sem ícone na bandeja"),
            }

            enable_webrtc(app.handle());

            Ok(())
        })
        .on_window_event(|window, event| {
            use tauri::Manager;

            // Fechar esconde, porque matar o processo derrubaria a transmissão junto —
            // mas só quando existe bandeja para trazer a janela de volta. Sem ela,
            // esconder deixava um processo invisível que só morria no `kill`.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if ! window.state::<HasTray>().0.load(Ordering::Relaxed) {
                    return;
                }

                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error starting the app");
}
