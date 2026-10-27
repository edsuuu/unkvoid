//! O painel do Alt+Z: os botões do replay por cima do jogo, no alto do monitor principal.
//!
//! São duas janelas. O escuro translúcido que cobre o monitor é uma janela do Windows em camadas,
//! como o `toast.rs`: não pega o foco e fecha o painel com um clique. O painel é uma viewport do
//! egui SÓLIDA, recortada com `SetWindowRgn` nos dois cartões. A transparência por pixel não serve:
//! a configuração do OpenGL nasce sem alfa, e no app de antes ela cobria o jogo de preto sólido no
//! driver da NVIDIA. E o quadro de configurações não pode ter opacidade nenhuma.
//!
//! O painel é desenhado pelo `ClipsRuntime::poll`, que roda no `logic` do eframe. Com a janela
//! escondida na bandeja o eframe continua rodando o quadro inteiro (ele só pula o desenho com a
//! janela minimizada, e mesmo aí volta a desenhar enquanto houver uma viewport filha na tela).

use std::cell::{Cell, RefCell};
use std::time::Instant;

use egui::{Align, Align2, Color32, CornerRadius, Layout, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, EnumThreadWindows, GetWindowTextW, HWND_TOPMOST, IDC_ARROW,
    LWA_ALPHA, LoadCursorW, MA_NOACTIVATE, RegisterClassW, SW_HIDE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetWindowPos, ShowWindow,
    WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_RBUTTONDOWN, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{BOOL, PCWSTR, w};

use crate::clips::{ClipsSettings, FRAME_RATES, REPLAY_MINUTES};
use crate::ui::{theme, widgets};

const BACKDROP_CLASS: PCWSTR = w!("unkvoid-clips-backdrop");
/// O título da janela do painel: é por ele que o HWND é achado entre as janelas desta thread.
const PANEL_TITLE: &str = "Unkvoid Clips";
const BACKDROP_OPACITY: f32 = 190.0;
const SLIDE_SECONDS: f32 = 0.26;

/// A janela tem sempre o tamanho dos dois cartões abertos; o recorte decide o que existe dela.
const WINDOW_SIZE: [f32; 2] = [480.0, 640.0];
const TOP: f32 = 24.0;
const GAP: f32 = 12.0;
const CARD_WIDTH: f32 = 432.0;
const BUTTON_WIDTH: f32 = 120.0;
const BUTTON_SPACING: f32 = 12.0;

thread_local! {
    static BACKDROP: Cell<Option<isize>> = const { Cell::new(None) };
    static CLICKED: Cell<bool> = const { Cell::new(false) };
    /// O clique no escuro chega pelo laço de mensagens, fora do egui: é este contexto que acorda
    /// o eframe para fechar o painel.
    static CONTEXT: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
}

/// O que um clique no painel pede ao `ClipsRuntime`.
pub enum Action {
    None,
    Close,
    Replay,
    Gallery,
    Settings(ClipsSettings),
    PickFolder,
}

pub struct OverlayState {
    opened_at: Instant,
    /// A janela da frente quando o painel abriu (o jogo): volta para ela, e o clipe é dela.
    pub previous: HWND,
    /// O monitor do painel e do escuro, em pixels físicos.
    pub monitor: (i32, i32, i32, i32),
    settings_open: bool,
    panel: Option<HWND>,
    placed: Option<(i32, i32, i32, i32)>,
    shape: Vec<crate::shell::RoundedRect>,
    /// A altura dos cartões no quadro anterior, de onde a descida parte.
    content_height: f32,
    settled: bool,
}

impl OverlayState {
    pub fn new(previous: HWND) -> Self {
        // Sempre no monitor principal, que é onde o jogo roda, e não no da janela da frente (o
        // Discord no segundo monitor, por exemplo). O beta abre no da janela da frente: os testes
        // rodam no monitor da direita, longe de quem está usando o PC.
        let monitor = if crate::BETA {
            crate::shell::window_monitor_rect(previous)
        } else {
            crate::shell::primary_monitor_rect()
        };

        Self {
            opened_at: Instant::now(),
            previous,
            monitor,
            settings_open: false,
            panel: None,
            placed: None,
            shape: Vec::new(),
            content_height: 200.0,
            settled: false,
        }
    }

    /// Apaga o escuro; o painel some sozinho, porque a viewport deixa de ser mostrada. Com
    /// `return_focus` o jogo volta para a frente (e para a tela, se ele se minimizou ao perder o
    /// foco). Devolve a janela do jogo, de onde sai a pasta do clipe.
    pub fn close(self, return_focus: bool) -> HWND {
        backdrop_hide();

        if return_focus {
            crate::shell::restore_if_minimized(self.previous);
            crate::shell::bring_to_front(self.previous);
        }

        self.previous
    }
}

/// Desce do alto em 260 ms, desacelerando no fim: de 0 (tudo acima da tela) a 1 (no lugar).
fn eased(seconds: f32) -> f32 {
    let progress = (seconds / SLIDE_SECONDS).clamp(0.0, 1.0);

    1.0 - (1.0 - progress).powi(3)
}

/// Desenha o painel por um quadro. Precisa ser chamado a cada quadro enquanto ele está aberto.
pub fn draw(context: &egui::Context, state: &mut OverlayState, settings: &ClipsSettings, saving: bool) -> Action {
    // O construtor fica igual o tempo todo: o egui transforma cada diferença em comando para a
    // janela. Ela nasce escondida e na origem do monitor principal, que é sempre (0, 0), e só
    // aparece depois de posta no lugar e recortada: nada pisca, e ela não pula de DPI.
    let builder = egui::ViewportBuilder::default()
        .with_title(PANEL_TITLE)
        .with_decorations(false)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_visible(false)
        .with_position([0.0, 0.0])
        .with_inner_size(WINDOW_SIZE);

    let mut action = Action::None;

    // Um quadro pode ter uma segunda passada (`request_discard`), e só a primeira recebe o
    // clique: a resposta dela não pode ser apagada pela outra.
    context.show_viewport_immediate(egui::ViewportId::from_hash_of("clips-overlay"), builder, |ui, _| {
        match panel(ui, state, settings, saving) {
            Action::None => {}
            clicked => action = clicked,
        }
    });

    action
}

fn panel(ui: &mut Ui, state: &mut OverlayState, settings: &ClipsSettings, saving: bool) -> Action {
    let scale = ui.ctx().pixels_per_point();
    let eased = eased(state.opened_at.elapsed().as_secs_f32());
    // A janela fica parada no alto do monitor, e quem desce é o conteúdo, dentro dela.
    let offset = -(TOP + state.content_height) * (1.0 - eased);
    let mut action = Action::None;

    let buttons = card(ui, pos2((WINDOW_SIZE[0] - CARD_WIDTH) / 2.0, TOP + offset), 22.0, 24, |ui| {
        ui.set_width(CARD_WIDTH - 48.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = BUTTON_SPACING;

            let caption = if saving {
                "Salvando…".to_owned()
            } else if settings.enabled {
                format!("Clique para salvar {} min", settings.replay_minutes)
            } else {
                "Desligado · clique para ligar".to_owned()
            };

            if round_button(ui, egui_phosphor::regular::RECORD, "Replay", &caption, settings.enabled, !saving) {
                action = Action::Replay;
            }

            if round_button(ui, egui_phosphor::regular::FILM_STRIP, "Galeria", "Ver os clipes", false, true) {
                action = Action::Gallery;
            }

            if round_button(ui, egui_phosphor::regular::GEAR, "Configurações", "Tempo, qualidade, pasta", state.settings_open, true) {
                state.settings_open = !state.settings_open;
            }
        });
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(format!("Esc ou {} para voltar ao jogo", settings.overlay_hotkey.label()))
                    .font(theme::regular(12.0))
                    .color(theme::MUTED),
            );
        });
    });
    let mut cards = vec![(buttons, 22.0)];

    if state.settings_open {
        let top = buttons.bottom() + GAP;
        let block = card(ui, pos2(buttons.left(), top), 18.0, 20, |ui| {
            ui.set_width(CARD_WIDTH - 40.0);

            if let Some(changed) = settings_block(ui, settings) {
                action = changed;
            }
        });

        cards.push((block, 18.0));
    }

    state.content_height = cards.last().map_or(0.0, |(rect, _)| rect.bottom() - offset - TOP);
    place(state, scale, &cards);

    if eased < 1.0 {
        backdrop_opacity((BACKDROP_OPACITY * eased) as u8);
        ui.ctx().request_repaint();
    } else if !state.settled {
        // Parou de descer: o escuro de novo logo abaixo do painel, porque alguém pode ter subido
        // no meio.
        backdrop_opacity(BACKDROP_OPACITY as u8);
        state.settled = true;

        if let Some(panel) = state.panel {
            keep_below(panel);
        }
    }

    // O Alt+F4 no painel chega como pedido de fechar desta viewport: fecha como o Esc.
    if ui.input(|input| input.key_pressed(egui::Key::Escape) || input.viewport().close_requested()) {
        action = Action::Close;
    }

    action
}

/// Um cartão sólido com o canto de cima-esquerda em `at`. Devolve onde ele ficou.
fn card(ui: &mut Ui, at: egui::Pos2, radius: f32, margin: i8, add: impl FnOnce(&mut Ui)) -> Rect {
    let area = Rect::from_min_size(at, vec2(CARD_WIDTH, WINDOW_SIZE[1]));

    ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
        egui::Frame::new()
            .fill(theme::MAIN)
            .stroke(Stroke::new(1.0, theme::MENU_BORDER))
            .corner_radius(CornerRadius::same(radius as u8))
            .inner_margin(egui::Margin::same(margin))
            .show(ui, add)
            .response
            .rect
    })
    .inner
}

/// Põe a janela no alto e no meio do monitor principal e a recorta nos cartões, em pixels físicos
/// da própria janela. O Windows só é chamado quando algo mudou.
fn place(state: &mut OverlayState, scale: f32, cards: &[(Rect, f32)]) {
    let Some(panel) = state.panel.or_else(find_panel) else { return };
    let physical = |length: f32| (length * scale).round() as i32;
    let (left, top, width, _) = state.monitor;
    let size = (physical(WINDOW_SIZE[0]), physical(WINDOW_SIZE[1]));
    let placed = (left + (width - size.0) / 2, top, size.0, size.1);
    let shape: Vec<_> = cards
        .iter()
        .map(|(rect, radius)| {
            (physical(rect.left()), physical(rect.top()), physical(rect.width()), physical(rect.height()), physical(*radius))
        })
        .collect();

    state.panel = Some(panel);

    if state.shape != shape {
        crate::shell::shape_window(panel, &shape);
        state.shape = shape;
    }

    if state.placed != Some(placed) {
        let first = state.placed.is_none();

        unsafe {
            let _ = SetWindowPos(panel, Some(HWND_TOPMOST), placed.0, placed.1, placed.2, placed.3, SWP_SHOWWINDOW);
        }

        state.placed = Some(placed);

        // A primeira vez na tela: fora da barra de tarefas e do Alt+Tab, sem a moldura clássica
        // que o Windows desenha em janela recortada, com o foco (para o Esc chegar) e acima do
        // escuro.
        if first {
            crate::shell::hide_from_taskbar(panel);
            crate::shell::suppress_frame(panel);
            crate::shell::bring_to_front(panel);
        }

        keep_below(panel);
    }
}

/// Liga e desliga, tempo, qualidade, fps e pasta: o mesmo que a página dos Clips, em pouco espaço.
fn settings_block(ui: &mut Ui, settings: &ClipsSettings) -> Option<Action> {
    let mut changed = settings.clone();
    let mut action = None;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Replay instantâneo").font(theme::bold(16.0)).color(theme::BRIGHT));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::switch(ui, changed.enabled).clicked() {
                changed.enabled = !changed.enabled;
            }
        });
    });
    ui.label(
        egui::RichText::new(if settings.enabled {
            "Gravando o tempo todo. Só vira arquivo quando você salvar."
        } else {
            "Ligue para começar a gravar."
        })
        .font(theme::regular(13.0))
        .color(theme::DESCRIPTION),
    );

    let minutes = REPLAY_MINUTES.map(|minutes| format!("{minutes} min"));

    section(ui, "Duração do replay");
    if let Some(index) = segmented(ui, "minutes", &minutes, REPLAY_MINUTES.iter().position(|&minutes| minutes == changed.replay_minutes)) {
        changed.replay_minutes = REPLAY_MINUTES[index];
    }

    section(ui, "Qualidade");
    // Os mesmos nomes da página dos Clips.
    let qualities = ["Eficiente", "Equilibrada", "Alta"].map(str::to_owned);
    if let Some(index) = segmented(ui, "quality", &qualities, Some(changed.quality.min(qualities.len() - 1))) {
        changed.quality = index;
    }

    section(ui, "Quadros por segundo");
    let rates = FRAME_RATES.map(|rate| format!("{rate} fps"));
    if let Some(index) = segmented(ui, "fps", &rates, FRAME_RATES.iter().position(|&rate| rate == changed.frame_rate)) {
        changed.frame_rate = FRAME_RATES[index];
    }

    section(ui, "Salvar em");
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 38.0), Layout::right_to_left(Align::Center), |ui| {
        if widgets::secondary_button(ui, "Alterar") {
            action = Some(Action::PickFolder);
        }

        // O caminho começa na margem, debaixo do título, e corta antes do botão: encostado no
        // botão ele ficava solto no meio da linha.
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(settings.folder.to_string_lossy()).font(theme::regular(13.0)).color(theme::TEXT),
                )
                .truncate(),
            );
        });
    });

    action.or_else(|| (changed != *settings).then_some(Action::Settings(changed)))
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(egui::RichText::new(title).font(theme::bold(13.0)).color(theme::SECONDARY));
    ui.add_space(4.0);
}

/// As opções lado a lado, de mesma largura, a escolhida acesa. O `pill_tabs` guarda lugar para um
/// ícone antes do texto, que aqui não tem. Devolve a clicada, se não era a escolhida.
fn segmented(ui: &mut Ui, id: &str, options: &[String], selected: Option<usize>) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
    let inner = rect.shrink(3.0);
    #[allow(clippy::cast_precision_loss)]
    let each = inner.width() / options.len() as f32;
    let mut chosen = None;

    ui.painter().rect_filled(rect, theme::RADIUS, theme::FIELD);

    for (position, name) in options.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let option = Rect::from_min_size(pos2(inner.left() + each * position as f32, inner.top()), vec2(each, inner.height()));
        let response = ui
            .interact(option, ui.id().with((id, position)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let on = selected == Some(position);

        if on {
            ui.painter().rect_filled(option, CornerRadius::same(6), theme::SELECTED);
        } else if response.hovered() {
            ui.painter().rect_filled(option, CornerRadius::same(6), theme::HOVER);
        }

        let tint = if on || response.hovered() { theme::BRIGHT } else { theme::SECONDARY };

        ui.painter().text(option.center(), Align2::CENTER_CENTER, name, theme::bold(13.0), tint);

        if response.clicked() && !on {
            chosen = Some(position);
        }
    }

    chosen
}

/// O botão redondo: o ícone num círculo (roxo quando aceso), o nome e a legenda embaixo.
fn round_button(ui: &mut Ui, icon: &str, label: &str, caption: &str, on: bool, enabled: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(BUTTON_WIDTH, 104.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let center = pos2(rect.center().x, rect.top() + 30.0);
    let fill = match (on, response.hovered() && enabled) {
        (true, _) => theme::ACCENT,
        (false, true) => theme::BUTTON_HOVER,
        (false, false) => theme::BUTTON,
    };
    let painter = ui.painter();

    painter.circle_filled(center, 28.0, fill);
    painter.text(center, Align2::CENTER_CENTER, icon, theme::icon(24.0), Color32::WHITE);
    painter.text(pos2(rect.center().x, rect.top() + 74.0), Align2::CENTER_CENTER, label, theme::bold(14.0), theme::BRIGHT);
    painter.text(pos2(rect.center().x, rect.top() + 94.0), Align2::CENTER_CENTER, caption, theme::regular(11.0), theme::MUTED);

    enabled && response.clicked()
}

/// A janela do painel entre as desta thread, pelo título. O eframe não entrega o HWND de uma
/// viewport filha.
fn find_panel() -> Option<HWND> {
    unsafe extern "system" fn visit(window: HWND, found: LPARAM) -> BOOL {
        let mut title = [0_u16; 64];
        let length = unsafe { GetWindowTextW(window, &mut title) } as usize;

        if String::from_utf16_lossy(&title[..length]) == PANEL_TITLE {
            unsafe { *(found.0 as *mut isize) = window.0 as isize };

            return BOOL(0);
        }

        BOOL(1)
    }

    let mut found = 0_isize;

    unsafe {
        let _ = EnumThreadWindows(GetCurrentThreadId(), Some(visit), LPARAM(&raw mut found as isize));
    }

    (found != 0).then_some(HWND(found as *mut _))
}

/// Cobre o monitor (pixels físicos) com o preto, ainda apagado: a descida o acende.
pub fn backdrop_show(x: i32, y: i32, width: i32, height: i32, context: &egui::Context) {
    CONTEXT.set(Some(context.clone()));
    CLICKED.set(false);

    let Some(window) = backdrop() else { return };

    unsafe {
        let _ = SetLayeredWindowAttributes(window, COLORREF(0), 0, LWA_ALPHA);
        let _ = SetWindowPos(window, Some(HWND_TOPMOST), x, y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

pub fn backdrop_hide() {
    if let Some(window) = BACKDROP.get() {
        unsafe {
            let _ = ShowWindow(HWND(window as *mut _), SW_HIDE);
        }
    }
}

/// Se alguém clicou no escuro, fora dos cartões, desde a última pergunta.
pub fn take_backdrop_click() -> bool {
    CLICKED.take()
}

fn backdrop_opacity(opacity: u8) {
    if let Some(window) = BACKDROP.get() {
        unsafe {
            let _ = SetLayeredWindowAttributes(HWND(window as *mut _), COLORREF(0), opacity, LWA_ALPHA);
        }
    }
}

/// O painel e o escuro são "sempre no topo", e entre eles vale o último que subiu: o painel sobe,
/// e o escuro fica logo abaixo dele.
fn keep_below(panel: HWND) {
    let Some(window) = BACKDROP.get() else { return };

    unsafe {
        let _ = SetWindowPos(panel, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        let _ = SetWindowPos(HWND(window as *mut _), Some(panel), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// A janela do escuro é criada uma vez e reaproveitada: o Alt+Z abre e fecha o tempo todo. Na
/// thread da interface, cujo laço de mensagens (o do winit) a atende.
fn backdrop() -> Option<HWND> {
    if let Some(window) = BACKDROP.get() {
        return Some(HWND(window as *mut _));
    }

    unsafe {
        let instance = GetModuleHandleW(None).ok()?;

        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            lpszClassName: BACKDROP_CLASS,
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            // Sem cursor na classe, o do jogo (às vezes escondido) fica por cima do escuro todo.
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        });

        let window = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
            BACKDROP_CLASS,
            w!("Unkvoid"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;

        BACKDROP.set(Some(window.0 as isize));

        Some(window)
    }
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        // Clicar no escuro não tira o foco do painel (nem o dá a esta janela).
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
            CLICKED.set(true);
            CONTEXT.with_borrow(|context| {
                if let Some(context) = context {
                    context.request_repaint();
                }
            });

            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_slides_from_above_and_stops_in_place() {
        assert!(eased(0.0).abs() < f32::EPSILON);
        assert!(eased(SLIDE_SECONDS / 2.0) > 0.5, "desacelera no fim, não no começo");
        assert!((eased(SLIDE_SECONDS) - 1.0).abs() < f32::EPSILON);
        assert!((eased(5.0) - 1.0).abs() < f32::EPSILON);
    }
}
