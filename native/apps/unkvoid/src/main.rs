//! O Unkvoid: um app só, em Rust, desenhado com egui por cima do `shared/core`.
//!
//! Este arquivo abre o log, garante uma instância só, pede ao Windows a captura sem a borda
//! amarela e entrega a janela ao eframe. O estado mora em `app.rs`, o desenho em `ui/` e o que
//! espera a rede em `backend.rs`.

// Sem console atrás da janela no Windows. Em `debug` ele fica: é onde o `tracing` aparece.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod backend;
mod camera;
#[cfg(target_os = "windows")]
mod clips;
#[cfg(target_os = "windows")]
mod clips_player;
// ponytail: o microfone, a saída de som, o palco, o assistir e a cola dos Clips voltam a ser chamados
// nas fases da voz e dos Clips; até lá só o toque e a abertura os usam, e os testes deles seguem
// rodando. Teto: aviso de código morto calado nestes módulos. A saída é ligar a voz (fase 2) e os
// Clips, e tirar cada llow.
#[allow(dead_code, unused_imports)]
mod devices;
#[cfg(target_os = "windows")]
mod logbook;
#[cfg(target_os = "windows")]
#[allow(dead_code)]
mod shell;
#[cfg(test)]
mod sharing;
#[allow(dead_code)]
mod sound;
#[allow(dead_code)]
mod stage;
mod system;
#[cfg(target_os = "windows")]
mod toast;
#[cfg(target_os = "windows")]
mod tray;
mod ui;
mod voice;
#[allow(dead_code)]
mod watching;
mod words;

use app::Unkvoid;

/// O Unkvoid Beta (`--features beta`) convive com o app instalado sem tocar em nada dele.
pub const BETA: bool = cfg!(feature = "beta");
pub const NAME: &str = if BETA { "Unkvoid Beta" } else { "Unkvoid" };

/// O menor tamanho em que as quatro colunas do hub ainda cabem.
const MIN_SIZE: [f32; 2] = [940.0, 540.0];

fn main() -> anyhow::Result<()> {
    let opened = run();

    // Sem console atrás da janela, o erro que o `main` devolve vai para um stderr que não
    // existe: o app sumiria deixando no log só a linha "abrindo".
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
    start_logbook();

    #[cfg(target_os = "windows")]
    allow_borderless_capture();

    let viewport = egui::ViewportBuilder::default()
        .with_title(NAME)
        .with_app_id(if BETA { "unkvoid-beta" } else { "com.unkvoid.desktop" })
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size(MIN_SIZE)
        .with_decorations(false)
        .with_icon(icon());
    let options = eframe::NativeOptions { viewport, centered: true, ..eframe::NativeOptions::default() };

    eframe::run_native(
        NAME,
        options,
        Box::new(move |creation| {
            ui::theme::install(&creation.egui_ctx);

            #[cfg(target_os = "windows")]
            if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(creation)
                && let raw_window_handle::RawWindowHandle::Win32(handle) = handle.as_raw()
            {
                shell::drag_by_caption(windows::Win32::Foundation::HWND(handle.hwnd.get() as *mut _));
            }

            #[cfg(target_os = "windows")]
            listen_for_show(show_request, creation.egui_ctx.clone())?;

            Ok(Box::new(Window {
                app: Unkvoid::new(&creation.egui_ctx)?,
                #[cfg(target_os = "windows")]
                tray: InTray::start(&creation.egui_ctx),
            }))
        }),
    )
    .map_err(|failure| anyhow::anyhow!("a janela não abriu: {failure}"))?;

    tracing::info!("Unkvoid fechando");

    Ok(())
}

struct Window {
    app: Unkvoid,
    /// `None` quando o ícone não entrou na bandeja: aí o X fecha o app, como fora do Windows.
    #[cfg(target_os = "windows")]
    tray: Option<InTray>,
}

/// O app que mora na bandeja: o X só esconde a janela, e o replay segue gravando.
#[cfg(target_os = "windows")]
struct InTray {
    icon: tray::Tray,
    events: std::sync::mpsc::Receiver<tray::TrayEvent>,
    /// Aberto pelo logon: a janela some no primeiro quadro. Antes dele o eframe a mostraria de
    /// novo, porque ela nasce escondida até o primeiro desenho.
    hide_at_start: bool,
    quitting: bool,
}

#[cfg(target_os = "windows")]
impl InTray {
    fn start(context: &egui::Context) -> Option<Self> {
        let (sender, events) = std::sync::mpsc::channel();
        let context = context.clone();
        let started = tray::Tray::start(NAME, move |event| {
            let _ = sender.send(event);
            context.request_repaint();
        });

        match started {
            Ok(icon) => Some(Self { icon, events, hide_at_start: shell::started_in_background(), quitting: false }),
            Err(error) => {
                tracing::error!(error = %format!("{error:#}"), "bandeja: o ícone não entrou");

                None
            }
        }
    }

    fn logic(&mut self, app: &mut Unkvoid, context: &egui::Context) {
        let runtime = &app.clips_runtime;

        self.icon.set_tip(&format!("{NAME} · {}", clips::status_line(&runtime.settings, &runtime.health())));

        if std::mem::take(&mut self.hide_at_start) {
            context.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        while let Ok(event) = self.events.try_recv() {
            match event {
                tray::TrayEvent::Open => show_window(context),
                tray::TrayEvent::Save => app.clips_runtime.request_save(),
                // Com um replay sendo salvo, não: o arquivo ficaria pela metade.
                tray::TrayEvent::Quit if app.clips_runtime.saving() => {
                    toast::show("Ainda salvando o replay", "Espere terminar para sair do Unkvoid.");
                }
                tray::TrayEvent::Quit => {
                    self.quitting = true;
                    context.send_viewport_cmd(egui::ViewportCommand::Close);
                    context.request_repaint();
                }
            }
        }

        // O X da moldura e o Alt+F4 chegam aqui; o "Sair" da bandeja passa direto.
        if !self.quitting && context.input(|input| input.viewport().close_requested()) {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            context.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            app.hidden();
        }
    }
}

impl eframe::App for Window {
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.receive();

        #[cfg(target_os = "windows")]
        if let Some(tray) = &mut self.tray {
            tray.logic(&mut self.app, context);
        }

        if let Some(wait) = self.app.next_wakeup() {
            context.request_repaint_after(wait);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui::show(&mut self.app, ui);
        pointing_hand(ui.ctx());

        let acted = !self.app.actions.is_empty();

        self.app.apply_actions();

        if let Some(text) = self.app.take_copied() {
            ui.ctx().copy_text(text);
        }

        // O que o clique mudou aparece já, e não no próximo movimento do mouse.
        if acted {
            ui.ctx().request_repaint();
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        ui::theme::RAIL.to_normalized_gamma_f32()
    }
}

/// A mãozinha em tudo que se clica, como no Discord, num lugar só: o egui só a põe no `Button`, e
/// cada canal, linha, cartão e ícone desenhado à mão ficava com a seta. Quem pediu outro cursor
/// (o texto de um campo) manda; o que também se arrasta (a barra de título, o texto selecionável)
/// fica com o dele.
fn pointing_hand(context: &egui::Context) {
    if context.output(|output| output.cursor_icon) != egui::CursorIcon::Default {
        return;
    }

    let hovered = context.interaction_snapshot(|snapshot| snapshot.hovered.clone());
    let clickable = hovered.iter().any(|id| {
        context.read_response(*id).is_some_and(|response| {
            response.enabled() && response.sense.senses_click() && !response.sense.senses_drag()
        })
    });

    if clickable {
        context.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

/// O ícone da janela e da barra de tarefas, o mesmo do instalador.
fn icon() -> egui::IconData {
    let decoded = image::load_from_memory_with_format(include_bytes!("../icons/128x128.png"), image::ImageFormat::Png)
        .map(|image| image.to_rgba8())
        .ok();

    match decoded {
        Some(image) => egui::IconData { width: image.width(), height: image.height(), rgba: image.into_raw() },
        None => egui::IconData::default(),
    }
}

/// A borda amarela que o Windows pinta em volta do que está sendo capturado aparecia até para
/// quem assiste. O `windows-capture` já pede a captura sem borda, mas app de fora da loja só é
/// atendido depois de pedir licença, uma vez por processo; sem ela o Windows ignora o pedido
/// calado. No Windows 10 o pedido é recusado, e o monitor inteiro sai pelo Desktop Duplication.
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
    let log = logbook::DailyLog::open(&shell::local_folder());

    tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::sync::Mutex::new(log)).with_ansi(false).init();

    std::panic::set_hook(Box::new(|information| tracing::error!("pânico: {information}")));
    tracing::info!(version = env!("CARGO_PKG_VERSION"), pid = std::process::id(), beta = BETA, "Unkvoid abrindo");

    // O build de depuração convive com o app instalado: a tarefa da elevação abre o `.exe`
    // instalado, e a trava da instância única mandaria o de depuração mostrar aquele e sair. O
    // beta tem trava própria, mas também não se eleva pela tarefa, que é a do instalado.
    if cfg!(debug_assertions) && !BETA {
        return Ok(Some(windows::Win32::Foundation::HANDLE::default()));
    }

    // O beta aberto pelo `UNKVOID_BETA_ABRIR` é o das fotos de comparação: ele abre ao lado do
    // beta em que o dono está testando, em vez de só trazer o dele para a frente e sair.
    if BETA && std::env::var_os("UNKVOID_BETA_ABRIR").is_some() {
        return Ok(Some(windows::Win32::Foundation::HANDLE::default()));
    }

    if !BETA && shell::elevate() {
        return Ok(None);
    }

    shell::single_instance()
}

/// Fora do Windows o log vai para a pasta de estado do XDG, e o terminal recebe as mesmas linhas.
#[cfg(not(target_os = "windows"))]
fn start_logbook() {
    use tracing_subscriber::fmt::writer::MakeWriterExt;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let Some(folder) = system::log_folder() else {
        tracing_subscriber::fmt().with_env_filter(filter).init();

        return;
    };
    let log = std::sync::Mutex::new(core_app::logbook::DailyLog::open(&folder));

    tracing_subscriber::fmt().with_env_filter(filter).with_writer(log.and(std::io::stderr)).with_ansi(false).init();
    std::panic::set_hook(Box::new(|information| tracing::error!("pânico: {information}")));
    tracing::info!(version = env!("CARGO_PKG_VERSION"), folder = %folder.display(), "Unkvoid abrindo");
}

/// Abrir o app de novo (atalho, menu Iniciar) enquanto ele já roda só traz a janela para a frente.
#[cfg(target_os = "windows")]
fn listen_for_show(request: windows::Win32::Foundation::HANDLE, context: egui::Context) -> anyhow::Result<()> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};

    // O build de depuração não tem a trava, e esperar num evento nulo seria um laço girando.
    if request.is_invalid() {
        return Ok(());
    }

    let request = request.0 as isize;

    std::thread::Builder::new().name("show-request".into()).spawn(move || {
        loop {
            // SAFETY: o evento é da instância única e vive até o processo acabar.
            unsafe {
                WaitForSingleObject(HANDLE(request as *mut _), INFINITE);
            }

            show_window(&context);
        }
    })?;

    Ok(())
}

/// A janela de volta: da bandeja, do atalho do menu Iniciar com o app já aberto.
#[cfg(target_os = "windows")]
fn show_window(context: &egui::Context) {
    context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    context.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    context.send_viewport_cmd(egui::ViewportCommand::Focus);
}
