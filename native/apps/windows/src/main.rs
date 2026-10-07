//! O Unkvoid no Windows: Slint por cima do `shared/core`, sem ponte nenhuma no meio.
//!
//! A janela é a pilha das cinco telas; quem diz qual está valendo é o núcleo. Este arquivo
//! só abre a janela, liga os cliques ao núcleo e entrega o laço de eventos ao Slint.
//!
//! No Windows ele também liga os Clips, o replay instantâneo, antes do núcleo: o replay grava
//! desde o logon, com ou sem internet, e o app mora na bandeja.

// Sem console atrás da janela no Windows. Em `debug` ele fica: é onde o `tracing` aparece.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod bridge;
#[cfg(target_os = "windows")]
mod clips;
mod devices;
mod frame;
#[cfg(target_os = "windows")]
mod logbook;
#[cfg(test)]
mod sharing;
mod sound;
mod stage;
mod watching;

use slint::ComponentHandle;

use bridge::Bridge;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    let opened = run();

    // Sem console atrás da janela, o erro que o `main` devolve vai para um stderr que não
    // existe: o app sumia deixando no log só a linha "abrindo".
    if let Err(error) = &opened {
        tracing::error!(error = %format!("{error:#}"), "abertura: o app não abriu");
    }

    opened
}

fn run() -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    let Some(show_request) = start_windows()? else {
        return Ok(());
    };

    #[cfg(not(target_os = "windows"))]
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    #[cfg(target_os = "windows")]
    allow_borderless_capture();

    let window = AppWindow::new()?;

    // Os Clips primeiro: o núcleo espera o servidor responder, e o replay não pode esperar.
    #[cfg(target_os = "windows")]
    let in_tray = match clips::start(&window) {
        Ok(()) => true,
        Err(error) => {
            tracing::error!(error = %format!("{error:#}"), "clips: não ligaram");

            false
        }
    };

    let bridge = Bridge::new(window.as_weak())?;

    frame::wire(&window);
    bridge.wire(&window);
    bridge.start();

    #[cfg(target_os = "windows")]
    listen_for_show(show_request, window.as_weak())?;

    #[cfg(target_os = "windows")]
    if in_tray {
        // Fechar esconde: o replay segue gravando, e a bandeja traz a janela de volta. A
        // chamada não: fechar a janela sempre foi sair da sala, e o microfone aberto sem janela
        // nenhuma na tela seria pior que o replay parado.
        let closing = bridge.clone();

        window.window().on_close_requested(move || {
            closing.hang_up();
            clips::window_closed();

            slint::CloseRequestResponse::HideWindow
        });

        // O X da moldura nossa faz o mesmo que o Alt+F4, e não sair do app (o `frame::wire` o
        // liga ao `quit`, que continua valendo sem os Clips).
        let (closing, hidden) = (bridge.clone(), window.as_weak());

        window.global::<Ui>().on_close_window(move || {
            closing.hang_up();
            clips::window_closed();

            if let Some(window) = hidden.upgrade() {
                let _ = window.hide();
            }
        });

        if !clips::shell::started_in_background() {
            clips::show_window();
        }

        slint::run_event_loop_until_quit()?;
        closed();
    }

    window.run()?;

    closed()
}

#[cfg(not(target_os = "windows"))]
fn closed() -> anyhow::Result<()> {
    Ok(())
}

/// Sai sem destrutor nenhum. O estado do app mora num `thread_local`, e o Rust roda esses
/// destrutores dentro do `ExitProcess`, com as outras threads já mortas: o `Runtime` do tokio da
/// `Bridge` caía ali e esperava para sempre threads que não existiam mais. O processo ficava vivo
/// com o ícone e a instância única, e todo clique depois só sinalizava ele: o app "não abria".
/// O `std::process::exit` passa pelo mesmo `ExitProcess`. O log é síncrono e não perde a última
/// linha; a sala e a voz caem com o socket, como já caíam.
#[cfg(target_os = "windows")]
fn closed() -> ! {
    use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};

    tracing::info!("Unkvoid fechando");
    clips::remove_tray();

    unsafe {
        let _ = TerminateProcess(GetCurrentProcess(), 0);
    }

    std::process::abort()
}

/// A borda amarela que o Windows pinta em volta do que está sendo capturado — o replay dos
/// Clips e a tela transmitida — aparecia até para quem assiste. O `windows-capture` já pede a
/// captura sem borda, mas app de fora da loja só é atendido depois de pedir licença, uma vez
/// por processo; sem ela o Windows ignora o pedido calado. Tem de vir antes da primeira
/// captura. No Windows 10 o pedido é recusado, e o monitor inteiro sai pelo Desktop
/// Duplication (`capture::windows_duplication`), que não tem borda.
///
/// A resposta vem da configuração de privacidade do Windows, sem janela nenhuma; o prazo existe
/// só para uma resposta que nunca chegue não prender a abertura do app.
#[cfg(target_os = "windows")]
fn allow_borderless_capture() {
    use windows::Graphics::Capture::{GraphicsCaptureAccess, GraphicsCaptureAccessKind};

    let (answer, answered) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let _ = answer.send(GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless).and_then(|asked| asked.join()));
    });

    match answered.recv_timeout(std::time::Duration::from_secs(3)) {
        Ok(Ok(status)) => tracing::info!(?status, "captura: licença para tirar a borda amarela"),
        Ok(Err(failure)) => tracing::info!(%failure, "captura: este Windows não tira a borda amarela"),
        Err(_) => tracing::warn!("captura: o Windows não respondeu a licença da borda"),
    }
}

/// O log num arquivo (sem console atrás da janela, é onde o que aconteceu fica) e a instância
/// única. `None` quando o app já está aberto: ele recebe o pedido de mostrar a janela.
#[cfg(target_os = "windows")]
fn start_windows() -> anyhow::Result<Option<windows::Win32::Foundation::HANDLE>> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let log = logbook::DailyLog::open(&clips::shell::local_folder());

    tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::sync::Mutex::new(log)).with_ansi(false).init();

    std::panic::set_hook(Box::new(|information| tracing::error!("pânico: {information}")));
    tracing::info!(version = env!("CARGO_PKG_VERSION"), pid = std::process::id(), background = clips::shell::started_in_background(), "Unkvoid abrindo");

    if clips::shell::elevate() {
        return Ok(None);
    }

    clips::shell::single_instance()
}

/// Abrir o app de novo (atalho, menu Iniciar) enquanto ele já roda só mostra a janela. Também
/// sem os Clips: a instância única vale para o app inteiro.
#[cfg(target_os = "windows")]
fn listen_for_show(request: windows::Win32::Foundation::HANDLE, window: slint::Weak<AppWindow>) -> anyhow::Result<()> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};

    let request = request.0 as isize;

    std::thread::Builder::new().name("show-request".into()).spawn(move || {
        loop {
            unsafe {
                WaitForSingleObject(HANDLE(request as *mut _), INFINITE);
            }

            let _ = window.upgrade_in_event_loop(|window| {
                let _ = window.show();

                clips::show_window();
            });
        }
    })?;

    Ok(())
}
