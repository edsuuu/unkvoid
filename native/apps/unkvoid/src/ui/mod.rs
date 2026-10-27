//! O desenho da janela. Cada módulo desenha um pedaço e anota em `app.actions` o que o clique
//! pediu; quem muda o estado é o `app.rs`, depois do quadro.

mod call;
mod channel_settings;
mod chat;
mod clips;
pub(crate) mod go_live;
mod home;
mod icons;
mod login;
mod members;
mod modals;
mod rail;
mod server_settings;
mod settings;
mod voice_settings;
mod sidebar;
pub mod theme;
pub(super) mod voice_debug;
pub(crate) mod widgets;

use egui::{Align2, Color32, CornerRadius, Frame, Rect, Sense, Ui, pos2, vec2};
use egui_phosphor::regular as phosphor;

use crate::app::{Action, Phase, Unkvoid};

/// A borda em que o cursor redimensiona a janela sem moldura.
const RESIZE_BORDER: f32 = 5.0;
const RESIZE_CORNER: f32 = 12.0;

pub fn show(app: &mut Unkvoid, ui: &mut Ui) {
    if call::full_stream(app, ui) {
        // Sem barra na tela cheia: o alto da transmissão não arrasta a janela.
        #[cfg(target_os = "windows")]
        crate::shell::set_caption(None);
        return;
    }

    // Os Clips moram no hub e na sala: em outra tela (a conta saiu, a sala acabou) eles fecham.
    if app.clips_open && !matches!(app.phase, Phase::Hub | Phase::Room) {
        close_clips(app);
    }

    titlebar(app, ui);

    match app.phase.clone() {
        Phase::Starting(status) => starting(ui, &status),
        Phase::Offline => offline(app, ui),
        Phase::SignIn => login::show(app, ui),
        // A sala e a chamada continuam no ar por baixo; só a tela é dos Clips.
        Phase::Hub | Phase::Room if app.clips_open => {
            clips::show(app, ui);
            modals::show(app, ui);
        }
        Phase::Hub => hub(app, ui),
        Phase::Room => {
            if let Some(code) = app.call.seat.as_ref().map(|seat| seat.channel.clone()) {
                egui::CentralPanel::no_frame()
                    .frame(Frame::new().fill(theme::MAIN))
                    .show(ui, |ui| call::room(app, ui, &code));
                modals::show(app, ui);
            }
        }
    }

    toasts(app, ui);
    window_resize(ui);
}

/// O hub no molde do Discord: o trilho de servidores e a coluna de canais (ou de conversas, na
/// Home) com o cartão da conta por baixo dos dois; a barra do canal por cima do chat e da lista
/// de membros; o chat no centro.
fn hub(app: &mut Unkvoid, ui: &mut Ui) {
    // A borda da coluna de canais se arrasta, como no Discord; o trilho fica do mesmo tamanho.
    egui::Panel::left("navigation")
        .default_size(theme::RAIL_WIDTH + theme::SIDEBAR_WIDTH)
        .size_range(
            (theme::RAIL_WIDTH + theme::SIDEBAR_RANGE.start())
                ..=(theme::RAIL_WIDTH + theme::SIDEBAR_RANGE.end()),
        )
        .resizable(true)
        .show_separator_line(false)
        .frame(Frame::new().fill(theme::RAIL))
        .show(ui, |ui| {
            sidebar::account(app, ui);
            rail::show(app, ui);
            sidebar::show(app, ui);
        });

    let in_call = app.reading.clone().filter(|reading| {
        app.call
            .seat
            .as_ref()
            .is_some_and(|seat| seat.channel == reading.id)
    });

    // Na chamada o palco ocupa a largura toda, como no Discord, e tem a barra dele.
    if in_call.is_none() && app.opened.is_some() {
        if let Some(reading) = app.reading.clone() {
            chat::header(app, ui, &reading);
        }

        // Na janela estreita a lista de membros sai e o chat fica com o espaço, como no Discord;
        // a escolha da pessoa (`show_members`) continua guardada para quando a janela crescer.
        let room = ui.available_width() >= theme::MEMBERS_WIDTH + 480.0;
        app.members_room = room;
        let mut members_open = if room { app.show_members } else { app.members_narrow };
        egui::Panel::right("members")
            .exact_size(theme::MEMBERS_WIDTH)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(theme::MAIN))
            .show_collapsible(ui, &mut members_open, |ui| {
                let rect = ui.max_rect();

                ui.painter().vline(
                    rect.left() + 0.5,
                    rect.y_range(),
                    egui::Stroke::new(1.0, theme::BORDER),
                );
                members::contents(app, ui);
            });

        if room {
            app.show_members = members_open;
        } else {
            app.members_narrow = members_open;
        }
    }

    egui::CentralPanel::no_frame()
        .frame(Frame::new().fill(theme::MAIN))
        .show(ui, |ui| {
            if let Some(channel) = in_call {
                call::show(app, ui, &channel);
            } else if app.opened.is_some() {
                chat::channel(app, ui, false);
            } else {
                home::show(app, ui);
            }
        });

    modals::show(app, ui);
}

/// A barra de cima, desenhada por nós como a do Discord: o nome do lugar no meio, o botão verde
/// da atualização e os botões da janela. Arrastar move; clique duplo maximiza.
fn titlebar(app: &mut Unkvoid, ui: &mut Ui) {
    egui::Panel::top("titlebar")
        .exact_size(theme::TITLEBAR_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(theme::RAIL))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            // No Windows quem arrasta é o sistema (`shell::drag_by_caption`), com os botões fora.
            #[cfg(not(target_os = "windows"))]
            {
                // A barra se arrasta, mas não é botão: a seta de sempre, não a mãozinha.
                let drag = ui
                    .interact(rect, ui.id().with("titlebar-drag"), Sense::click_and_drag())
                    .on_hover_cursor(egui::CursorIcon::Default);

                if drag.double_clicked() {
                    let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));

                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                } else if drag.drag_started() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
            }

            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                place_name(app),
                theme::bold(13.0),
                theme::MUTED,
            );

            let controls = window_controls(ui, rect);
            // O que não arrasta: os botões da janela e os desenhados aqui.
            let mut buttons = vec![Rect::from_min_max(pos2(controls, rect.top()), rect.right_bottom())];
            // `on` é a parte aberta: o fundo fica aceso, como a aba de antes.
            let mut button = |ui: &mut Ui, left: f32, icon: &str, color: Color32, on: bool, tip: &str| {
                let button = Rect::from_min_size(pos2(left, rect.top() + 3.0), vec2(28.0, 26.0));
                buttons.push(button);
                let response = ui.interact(button, ui.id().with(icon), Sense::click()).on_hover_text(tip);

                if on || response.hovered() {
                    ui.painter().rect_filled(button, CornerRadius::same(4), if on { theme::SELECTED } else { theme::HOVER });
                }

                crate::ui::icons::text(ui.painter(), button.center(), Align2::CENTER_CENTER, icon, theme::icon(18.0), if on { theme::BRIGHT } else { color });
                response.clicked()
            };

            if let Some(version) = app.update.clone()
                && button(ui, controls - 36.0, phosphor::DOWNLOAD_SIMPLE, theme::SUCCESS, false, &format!("A versão {version} está pronta. Clique para reiniciar e instalar."))
            {
                app.actions.push(Action::InstallUpdate);
            }

            // O começo e o Clips na esquerda, só onde há janela para abrir: na entrada e na tela de
            // carregar não.
            if matches!(app.phase, Phase::Room | Phase::Hub) {
                // Na sala por código o começo é a entrada, e a sala continua no ar.
                if button(ui, rect.left() + 8.0, phosphor::HOUSE, theme::MUTED, false, "Voltar para o início") {
                    close_clips(app);
                    app.actions.push(if app.phase == Phase::Room { Action::ShowEntry } else { Action::OpenHome });
                }

                let clips = rect.left() + 44.0;

                if button(ui, clips, phosphor::FILM_STRIP, theme::MUTED, app.clips_open, "Unkvoid Clips") {
                    if app.clips_open {
                        close_clips(app);
                    } else {
                        app.clips_open = true;
                    }
                }

                // O ponto vermelho é o replay gravando: dá para ver de qualquer tela.
                #[cfg(target_os = "windows")]
                if app.clips_runtime.settings.enabled && app.clips_runtime.health().recording {
                    ui.painter().circle_filled(pos2(clips + 23.0, rect.top() + 8.0), 3.5, theme::DANGER);
                }
            }

            #[cfg(target_os = "windows")]
            {
                let scale = ui.ctx().pixels_per_point();
                let physical = |rect: Rect| [rect.left(), rect.top(), rect.right(), rect.bottom()].map(|value| (value * scale).round() as i32);
                let maximized = ui.input(|input| {
                    input.viewport().maximized.unwrap_or(false) || input.viewport().fullscreen.unwrap_or(false)
                });
                // Fora da maximizada, a beira de cima e as laterais redimensionam (`window_resize`).
                let area = if maximized {
                    rect
                } else {
                    Rect::from_min_max(rect.min + vec2(RESIZE_BORDER, RESIZE_BORDER), pos2(rect.right() - RESIZE_BORDER, rect.bottom()))
                };

                crate::shell::set_caption(Some((physical(area), buttons.into_iter().map(physical).collect())));
            }
        });
}

/// Fecha os Clips, e o player junto: ele é uma janela do Windows por cima da nossa, e sobraria
/// na frente do hub.
fn close_clips(app: &mut Unkvoid) {
    app.clips_open = false;
    #[cfg(target_os = "windows")]
    app.clips_runtime.close_player();
}

/// O nome que a barra de cima mostra: os Clips, o servidor aberto, a conversa ou o Unkvoid.
fn place_name(app: &Unkvoid) -> String {
    if app.clips_open {
        return "Unkvoid Clips".to_owned();
    }

    if app.phase == Phase::Room
        && let Some(seat) = &app.call.seat
    {
        return format!("Sala {}", seat.channel);
    }

    if let Some(server) = app.opened.and_then(|server| app.server_summary(server)) {
        return server.name.clone();
    }

    match &app.home {
        crate::app::Home::Direct(person) if app.phase == Phase::Hub => person.name.clone(),
        crate::app::Home::Friends(_) if app.phase == Phase::Hub => "Amigos".to_owned(),
        _ => crate::NAME.to_owned(),
    }
}

/// Minimizar, maximizar e fechar, na direita. Devolve onde eles começam.
fn window_controls(ui: &mut Ui, bar: Rect) -> f32 {
    let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
    let buttons = [
        (phosphor::MINUS, "minimize", theme::HOVER),
        (
            if maximized {
                phosphor::CORNERS_IN
            } else {
                phosphor::SQUARE
            },
            "maximize",
            theme::HOVER,
        ),
        (phosphor::X, "close", theme::DANGER),
    ];
    let width = 46.0;
    let left = bar.right() - width * buttons.len() as f32;

    for (index, (icon, name, hover)) in buttons.into_iter().enumerate() {
        let rect = Rect::from_min_size(
            pos2(left + width * index as f32, bar.top()),
            vec2(width, bar.height()),
        );
        let response = ui.interact(rect, ui.id().with(name), Sense::click());

        if response.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::ZERO, hover);
        }

        let tint = if response.hovered() {
            Color32::WHITE
        } else {
            theme::MUTED
        };

        crate::ui::icons::text(ui.painter(), rect.center(), Align2::CENTER_CENTER, icon, theme::icon(15.0), tint);

        if response.clicked() {
            let command = match name {
                "minimize" => egui::ViewportCommand::Minimized(true),
                "maximize" => egui::ViewportCommand::Maximized(!maximized),
                _ => egui::ViewportCommand::Close,
            };

            ui.ctx().send_viewport_cmd(command);
        }
    }

    left
}

/// A janela não tem moldura do sistema: a borda dela redimensiona.
fn window_resize(ui: &mut Ui) {
    let maximized = ui.input(|input| {
        input.viewport().maximized.unwrap_or(false) || input.viewport().fullscreen.unwrap_or(false)
    });

    if maximized {
        return;
    }

    let Some(position) = ui.input(|input| input.pointer.hover_pos()) else {
        return;
    };
    let Some((direction, cursor)) = resize_direction(ui.ctx().content_rect(), position) else {
        return;
    };

    ui.ctx().set_cursor_icon(cursor);

    if ui.input(|input| input.pointer.primary_pressed()) {
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
    }
}

fn resize_direction(
    window: Rect,
    position: egui::Pos2,
) -> Option<(egui::ResizeDirection, egui::CursorIcon)> {
    use egui::{CursorIcon as Cursor, ResizeDirection as Direction};

    if !window.contains(position) {
        return None;
    }

    let near = |distance: f32, reach: f32| distance <= reach;
    let (left, right, top, bottom) = (
        position.x - window.left(),
        window.right() - position.x,
        position.y - window.top(),
        window.bottom() - position.y,
    );
    let mut x = i8::from(near(right, RESIZE_BORDER)) - i8::from(near(left, RESIZE_BORDER));
    let mut y = i8::from(near(bottom, RESIZE_BORDER)) - i8::from(near(top, RESIZE_BORDER));

    // Perto do canto, o canto vale mesmo que só uma das bordas esteja sob o cursor.
    if y != 0 {
        x = i8::from(near(right, RESIZE_CORNER)) - i8::from(near(left, RESIZE_CORNER));
    }

    if x != 0 {
        y = i8::from(near(bottom, RESIZE_CORNER)) - i8::from(near(top, RESIZE_CORNER));
    }

    match (x, y) {
        (-1, -1) => Some((Direction::NorthWest, Cursor::ResizeNwSe)),
        (1, -1) => Some((Direction::NorthEast, Cursor::ResizeNeSw)),
        (-1, 1) => Some((Direction::SouthWest, Cursor::ResizeNeSw)),
        (1, 1) => Some((Direction::SouthEast, Cursor::ResizeNwSe)),
        (-1, 0) => Some((Direction::West, Cursor::ResizeHorizontal)),
        (1, 0) => Some((Direction::East, Cursor::ResizeHorizontal)),
        (0, -1) => Some((Direction::North, Cursor::ResizeVertical)),
        (0, 1) => Some((Direction::South, Cursor::ResizeVertical)),
        _ => None,
    }
}

/// A abertura: o logo, o que está acontecendo e a roda girando.
fn starting(ui: &mut Ui, status: &str) {
    egui::CentralPanel::no_frame()
        .frame(Frame::new().fill(theme::RAIL))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() / 2.0 - 60.0).max(0.0));
                ui.label(
                    egui::RichText::new("UNKVOID")
                        .font(theme::bold(28.0))
                        .color(theme::ACCENT_TEXT),
                );
                ui.add_space(16.0);
                ui.add(egui::Spinner::new().size(22.0).color(theme::ACCENT));
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(status)
                        .font(theme::regular(14.0))
                        .color(theme::MUTED),
                );
            });
        });
}

fn offline(app: &mut Unkvoid, ui: &mut Ui) {
    egui::CentralPanel::no_frame()
        .frame(Frame::new().fill(theme::RAIL))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() / 2.0 - 80.0).max(0.0));
                crate::ui::icons::label(ui, phosphor::WIFI_SLASH, theme::icon(40.0), theme::DIM);
                ui.add_space(12.0);
                ui.label(egui::RichText::new("Servidor sem resposta").font(theme::bold(20.0)));
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Confira a sua internet. Se ela estiver de pé, o problema é do nosso lado.",
                    )
                    .color(theme::MUTED),
                );
                ui.add_space(18.0);

                if widgets::primary_button(ui, "Tentar de novo", true, 180.0) {
                    app.actions.push(Action::Retry);
                }
            });
        });
}

/// Os avisos no canto de baixo, que somem sozinhos; clicar dispensa.
fn toasts(app: &mut Unkvoid, ui: &mut Ui) {
    let screen = ui.ctx().content_rect();
    // Acima da caixa de mensagem (68 px com a margem), para o aviso não cobrir os botões dela.
    let mut bottom = screen.bottom() - 88.0;

    for toast in app.toasts.iter().rev() {
        let galley =
            ui.painter()
                .layout(toast.text.clone(), theme::regular(14.0), theme::TEXT, 320.0);
        let size = if toast.action_path.is_some() {
            vec2((galley.size().x + 168.0).max(360.0), 46.0)
        } else {
            galley.size() + vec2(48.0, 22.0)
        };
        let rect = Rect::from_min_size(pos2(screen.right() - 20.0 - size.x, bottom - size.y), size);
        let response = egui::Area::new(egui::Id::new(("toast", toast.id)))
            .order(egui::Order::Tooltip)
            .fixed_pos(rect.min)
            .show(ui.ctx(), |ui| {
                let (frame, response) = ui.allocate_exact_size(size, Sense::click());
                let accent = if toast.error {
                    theme::DANGER
                } else {
                    theme::ACCENT
                };

                ui.painter()
                    .rect_filled(frame, CornerRadius::same(8), theme::MENU);
                ui.painter().rect_filled(
                    Rect::from_min_size(frame.min, vec2(4.0, frame.height())),
                    CornerRadius {
                        nw: 8,
                        sw: 8,
                        ne: 0,
                        se: 0,
                    },
                    accent,
                );
                crate::ui::icons::text(ui.painter(), pos2(frame.left() + 16.0, frame.center().y), Align2::LEFT_CENTER, if toast.error {
                        phosphor::WARNING_CIRCLE
                    } else {
                        phosphor::INFO
                    }, theme::icon(16.0), accent);
                ui.painter().galley(
                    pos2(frame.left() + 38.0, frame.center().y - galley.size().y * 0.5),
                    galley,
                    theme::TEXT,
                );

                let action = toast.action_path.as_ref().map(|_| {
                    let button_rect = Rect::from_min_size(
                        pos2(frame.right() - 112.0, frame.top() + 7.0),
                        vec2(102.0, frame.height() - 14.0),
                    );
                    let button = ui.interact(
                        button_rect,
                        egui::Id::new(("toast-action", toast.id)),
                        Sense::click(),
                    );
                    ui.painter().rect_filled(
                        button_rect,
                        CornerRadius::same(6),
                        if button.hovered() { theme::BUTTON_HOVER } else { theme::BUTTON },
                    );
                    ui.painter().text(
                        button_rect.center(),
                        Align2::CENTER_CENTER,
                        "Abrir pasta",
                        theme::regular(12.0),
                        theme::TEXT,
                    );
                    button
                });

                (response, action)
            })
            .inner;

        if response.1.is_some_and(|action| action.clicked()) {
            #[cfg(target_os = "windows")]
            if let Some(path) = &toast.action_path {
                crate::shell::open_folder(path);
            }
            app.actions.push(Action::Dismiss(toast.id));
        } else if response.0.clicked() {
            app.actions.push(Action::Dismiss(toast.id));
        }

        bottom -= size.y + 8.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_border_and_the_corners_resize_and_the_middle_does_not() {
        let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));

        assert!(
            resize_direction(window, pos2(400.0, 300.0)).is_none(),
            "o meio da janela não redimensiona"
        );
        assert_eq!(
            resize_direction(window, pos2(2.0, 300.0)).map(|found| found.0),
            Some(egui::ResizeDirection::West)
        );
        assert_eq!(
            resize_direction(window, pos2(798.0, 598.0)).map(|found| found.0),
            Some(egui::ResizeDirection::SouthEast)
        );
        assert_eq!(
            resize_direction(window, pos2(8.0, 2.0)).map(|found| found.0),
            Some(egui::ResizeDirection::NorthWest),
            "perto do canto vale o canto"
        );
    }
}
