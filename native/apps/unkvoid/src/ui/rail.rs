//! O trilho de servidores, à esquerda: a Home, um ícone por servidor e o "+".
//!
//! O ícone é redondo e vira quadrado arredondado no hover e no aberto; a pílula branca na
//! borda diz qual está aberto (alta), qual está sob o cursor (média) e qual tem não lidas
//! (bolinha) — o mesmo código visual do Discord.

use egui::{Align2, Color32, CornerRadius, Frame, Rect, Sense, Ui, pos2, vec2};
use egui_phosphor::regular as icons;

use super::{theme, widgets};
use crate::app::{Action, AddServerStep, Modal, Unkvoid};

const ICON: f32 = 40.0;

pub fn show(app: &mut Unkvoid, ui: &mut Ui) {
    egui::Panel::left("rail")
        .exact_size(theme::RAIL_WIDTH)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(theme::RAIL)
                .inner_margin(egui::Margin::symmetric(0, 8)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;

                        let home_unread = app
                            .conversations
                            .iter()
                            .map(|conversation| conversation.unread)
                            .sum::<i64>();

                        if slot(
                            ui,
                            "home",
                            app.opened.is_none(),
                            home_unread > 0,
                            "Mensagens diretas",
                            |ui, rect, lit| {
                                let fill = if lit { theme::ACCENT } else { theme::ELEVATED };

                                ui.painter().rect_filled(rect, shape(lit), fill);
                                super::icons::paint_glyph(ui, rect, icons::CHATS_CIRCLE, Color32::WHITE, 26.0);

                                if home_unread > 0 {
                                    widgets::paint_badge(
                                        ui,
                                        rect.right_bottom() - vec2(6.0, 6.0),
                                        home_unread,
                                        theme::RAIL,
                                    );
                                }
                            },
                        ) {
                            app.actions.push(Action::OpenHome);
                        }

                        let unread_conversations: Vec<_> = app
                            .conversations
                            .iter()
                            .filter(|conversation| conversation.unread > 0)
                            .cloned()
                            .collect();
                        for conversation in &unread_conversations {
                            if slot(
                                ui,
                                &format!("direct-{}", conversation.user.id),
                                false,
                                true,
                                &conversation.user.name,
                                |ui, rect, _| {
                                    if let Some(url) = &conversation.user.avatar_url {
                                        egui::Image::new(url.clone())
                                            .corner_radius(CornerRadius::same(12))
                                            .paint_at(ui, rect);
                                    } else {
                                        ui.painter().rect_filled(
                                            rect,
                                            CornerRadius::same(12),
                                            widgets::color_for(&conversation.user.name),
                                        );
                                        ui.painter().text(
                                            rect.center(),
                                            Align2::CENTER_CENTER,
                                            acronym(&conversation.user.name),
                                            theme::bold(16.0),
                                            Color32::WHITE,
                                        );
                                    }
                                    widgets::paint_badge(
                                        ui,
                                        rect.right_bottom() - vec2(2.0, 2.0),
                                        conversation.unread,
                                        theme::RAIL,
                                    );
                                },
                            ) {
                                app.actions
                                    .push(Action::OpenDirect(conversation.user.clone()));
                            }
                        }

                        separator(ui);

                        let servers = app.servers.clone();

                        for server in &servers {
                            let opened = app.opened == Some(server.id);
                            let unread = app.unread_of(server);
                            let clicked = slot(
                                ui,
                                &format!("server-{}", server.id),
                                opened,
                                unread > 0,
                                &server.name,
                                |ui, rect, lit| match &server.icon_url {
                                    Some(url) => {
                                        egui::Image::new(url.clone())
                                            .corner_radius(shape(lit))
                                            .paint_at(ui, rect);
                                    }
                                    None => {
                                        let fill =
                                            if lit { theme::ACCENT } else { theme::ELEVATED };

                                        ui.painter().rect_filled(rect, shape(lit), fill);
                                        ui.painter().text(
                                            rect.center(),
                                            Align2::CENTER_CENTER,
                                            acronym(&server.name),
                                            theme::bold(16.0),
                                            theme::TEXT,
                                        );
                                    }
                                },
                            );

                            if clicked {
                                app.actions.push(Action::OpenServer(server.id));
                            }
                        }

                        if slot(
                            ui,
                            "add",
                            false,
                            false,
                            "Adicionar um servidor",
                            |ui, rect, lit| {
                                let fill = if lit { theme::SUCCESS } else { theme::ELEVATED };
                                let tint = if lit { Color32::WHITE } else { theme::SUCCESS };

                                ui.painter().rect_filled(rect, shape(lit), fill);
                                super::icons::paint_glyph(ui, rect, icons::PLUS, tint, 22.0);
                            },
                        ) {
                            app.actions
                                .push(Action::Show(Modal::AddServer(AddServerStep::Choose)));
                        }

                        // A sala por código também é de quem tem conta: criar uma ou entrar na
                        // de alguém, com o nome da conta. Acesa enquanto a pessoa está numa.
                        let in_code_room = app.call.seat.as_ref().is_some_and(|seat| seat.guest.is_some());

                        if slot(
                            ui,
                            "code-room",
                            false,
                            false,
                            "Sala por código",
                            |ui, rect, lit| {
                                let lit = lit || in_code_room;
                                let fill = if lit { theme::ACCENT } else { theme::ELEVATED };
                                let tint = if lit { Color32::WHITE } else { theme::ACCENT_TEXT };

                                ui.painter().rect_filled(rect, shape(lit), fill);
                                super::icons::paint_glyph(ui, rect, icons::HASH, tint, 22.0);
                            },
                        ) {
                            // Dentro de uma, o # é o caminho de volta para ela.
                            app.actions.push(if in_code_room { Action::BackToRoom } else { Action::Show(Modal::CodeRoom) });
                        }
                    });
                });
        });
}

/// Um lugar do trilho: o desenho de dentro, a pílula da borda e a dica com o nome.
fn slot(
    ui: &mut Ui,
    id: &str,
    opened: bool,
    unread: bool,
    tip: &str,
    paint: impl FnOnce(&Ui, Rect, bool),
) -> bool {
    let (row, _) = ui.allocate_exact_size(vec2(theme::RAIL_WIDTH, ICON), Sense::hover());
    let rect = Rect::from_center_size(row.center(), vec2(ICON, ICON));
    let response = ui
        .interact(rect, ui.id().with(id), Sense::click())
        .on_hover_text(tip);
    let lit = opened || response.hovered();

    paint(ui, rect, lit);

    let pill = match (opened, response.hovered(), unread) {
        (true, _, _) => Some(40.0),
        (false, true, _) => Some(20.0),
        (false, false, true) => Some(8.0),
        _ => None,
    };

    if let Some(height) = pill {
        let bar = Rect::from_center_size(pos2(row.left() + 2.0, row.center().y), vec2(8.0, height));

        ui.painter()
            .rect_filled(bar, CornerRadius::same(4), Color32::WHITE);
    }

    response.clicked()
}

/// O quadrado arredondado do Discord de hoje, aceso ou não: quem diz qual está aberto é a pílula.
fn shape(_lit: bool) -> CornerRadius {
    CornerRadius::same(12)
}

fn separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(theme::RAIL_WIDTH, 2.0), Sense::hover());

    ui.painter().rect_filled(
        Rect::from_center_size(rect.center(), vec2(32.0, 2.0)),
        CornerRadius::same(1),
        theme::DIVIDER,
    );
}

/// As iniciais do servidor sem ícone, como o Discord: a primeira letra de cada palavra.
fn acronym(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(3)
        .collect::<String>()
        .to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_without_an_icon_shows_its_initials() {
        assert_eq!(acronym("Projetos do Edsu"), "PDE");
        assert_eq!(acronym("insanity"), "I");
        assert_eq!(acronym("um dois três quatro"), "UDT");
    }
}
