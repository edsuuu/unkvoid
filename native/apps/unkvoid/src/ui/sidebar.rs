//! A coluna do meio-esquerda: os canais do servidor aberto (ou as conversas, na Home) e, no
//! pé, o cartão da conta com o microfone, o fone e as configurações.

use super::widgets::TipAbove;
use egui::{Align2, Color32, CornerRadius, Frame, Rect, Sense, Ui, pos2, vec2};
use egui_phosphor::{fill as filled_icons, regular as icons};
use std::time::{Duration, Instant};

use core_app::models::{Channel, ChannelKind, ServerTree, UserStatus};
use core_app::permissions;

use super::{server_settings, theme, widgets};
use crate::app::{
    Action, ChannelSettings, Confirm, FriendsTab, Home, Modal, ServerPage, ServerSettings,
    ProfileDraft, SettingsPage, Unkvoid,
};

/// A coluna ocupa o que sobra do painel da esquerda, que se redimensiona pela borda; a borda de 1 px
/// e o canto de cima arredondado a separam do trilho, que tem a mesma cor.
pub fn show(app: &mut Unkvoid, ui: &mut Ui) {
    egui::CentralPanel::no_frame()
        .frame(
            Frame::new()
                .fill(theme::SIDEBAR)
                .stroke(egui::Stroke::new(1.0, theme::DIVIDER))
                .corner_radius(CornerRadius {
                    nw: 10,
                    ne: 0,
                    sw: 0,
                    se: 0,
                }),
        )
        .show(ui, |ui| match app.tree.clone() {
            Some(tree) => server_column(app, ui, &tree),
            None if app.opened.is_some() => loading_column(ui),
            None => home_column(app, ui),
        });
}

/// O cartão da conta no pé, por cima do trilho e da coluna de canais, como no Discord de hoje: a
/// voz conectada em cima, quando há, e a conta embaixo.
pub fn account(app: &mut Unkvoid, ui: &mut Ui) {
    let voice = if app.call.seat.is_some() {
        VOICE_PANEL_HEIGHT
    } else {
        0.0
    };

    egui::Panel::bottom("account")
        .exact_size(theme::USER_PANEL_HEIGHT + voice + 16.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(theme::RAIL)
                .inner_margin(egui::Margin::same(8)),
        )
        .show(ui, |ui| {
            Frame::new()
                .fill(theme::ELEVATED)
                .stroke(egui::Stroke::new(1.0, theme::DIVIDER))
                .corner_radius(theme::RADIUS)
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    // As alturas do cartão são as medidas; o espaço automático entre linhas sobraria.
                    ui.spacing_mut().item_spacing.y = 0.0;

                    if app.call.seat.is_some() {
                        voice_panel(app, ui);
                    }

                    user_panel(app, ui);
                });
        });
}

fn server_column(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree) {
    server_header(app, ui, tree);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(8.0);

            let manage = tree.can(permissions::MANAGE_CHANNELS);

            for section in tree.sections() {
                if let Some(category) = &section.category {
                    let collapsed = app.collapsed.contains(&category.id);

                    if category_row(app, ui, tree, category, collapsed, manage) {
                        continue;
                    }
                }

                for channel in &section.channels {
                    channel_row(app, ui, tree, channel);
                }

                ui.add_space(8.0);
            }
        });
}

/// O nome do servidor no alto da coluna, com a seta colada nele, que abre o menu do servidor; e o
/// convite na direita, como no Discord.
fn server_header(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), theme::HEADER_HEIGHT),
        Sense::hover(),
    );
    let invite = tree.can(permissions::CREATE_INVITE).then(|| {
        Rect::from_center_size(pos2(rect.right() - 24.0, rect.center().y), vec2(28.0, 28.0))
    });
    let name_width = rect.width() - if invite.is_some() { 76.0 } else { 44.0 };
    let name = elide(ui, &tree.name, theme::bold(16.0), name_width);
    let galley = ui
        .painter()
        .layout_no_wrap(name.clone(), theme::bold(16.0), theme::BRIGHT);
    let menu_area = Rect::from_min_max(
        rect.min,
        pos2(rect.left() + 16.0 + galley.size().x + 32.0, rect.max.y),
    );
    let response = ui.interact(menu_area, ui.id().with("server-header"), Sense::click());

    if response.hovered() {
        ui.painter().rect_filled(
            menu_area.shrink2(vec2(6.0, 8.0)),
            theme::RADIUS,
            theme::HOVER,
        );
    }

    ui.painter().galley(
        pos2(rect.left() + 16.0, rect.center().y - galley.size().y / 2.0),
        galley.clone(),
        theme::BRIGHT,
    );
    let menu_open = egui::Popup::is_id_open(ui.ctx(), response.id);
    super::icons::paint_glyph(ui, Rect::from_center_size(pos2(rect.left() + 28.0 + galley.size().x, rect.center().y + 1.0), vec2(14.0, 14.0)), if menu_open { icons::CARET_UP } else { icons::CARET_DOWN }, theme::BRIGHT, 14.0);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, theme::DIVIDER),
    );

    if let Some(invite) = invite {
        let button = ui
            .interact(invite, ui.id().with("server-invite"), Sense::click())
            .tip("Convidar pessoas");

        if button.hovered() {
            ui.painter()
                .rect_filled(invite, theme::RADIUS, theme::HOVER);
        }

        super::icons::paint_glyph(ui, invite, filled_icons::USER_PLUS, if button.hovered() { theme::TEXT } else { theme::MUTED }, 18.0);

        if button.clicked() {
            app.actions.push(Action::Show(Modal::Invite {
                server: tree.id,
                code: None,
            }));
        }
    }

    egui::Popup::menu(&response)
        .frame(widgets::menu_frame())
        .show(|ui| {
            ui.set_min_width(220.0);

            if tree.can(permissions::CREATE_INVITE)
                && widgets::menu_item_left(ui, icons::USER_PLUS, "Convidar para o servidor", false)
            {
                app.actions.push(Action::Show(Modal::Invite {
                    server: tree.id,
                    code: None,
                }));
            }

            if server_settings::can_open(tree)
                && widgets::menu_item_left(ui, icons::GEAR_SIX, "Config. do servidor", false)
            {
                app.actions
                    .push(Action::Show(Modal::ServerSettings(ServerSettings::new(
                        tree,
                        ServerPage::Overview,
                    ))));
            }

            if tree.can(permissions::MANAGE_CHANNELS) {
                if widgets::menu_item_left(ui, icons::PLUS_CIRCLE, "Criar canal", false) {
                    app.actions.push(Action::Show(Modal::CreateChannel {
                        server: tree.id,
                        kind: ChannelKind::Text,
                        name: String::new(),
                        parent: None,
                    }));
                }

                if widgets::menu_item_left(ui, icons::FOLDER_PLUS, "Criar categoria", false) {
                    app.actions.push(Action::Show(Modal::CreateChannel {
                        server: tree.id,
                        kind: ChannelKind::Category,
                        name: String::new(),
                        parent: None,
                    }));
                }
            }

            widgets::menu_separator(ui);

            if let Some(me) = tree
                .members
                .iter()
                .find(|member| member.user_id == tree.me.user_id)
                && widgets::menu_item_left(
                    ui,
                    icons::PENCIL_SIMPLE,
                    "Editar perfil por servidor",
                    false,
                )
            {
                app.actions.push(Action::Ask(Confirm::Nickname {
                    server: tree.id,
                    user: me.user_id,
                    name: me.name.clone(),
                    value: me.nickname.clone().unwrap_or_default(),
                }));
            }

            if widgets::menu_item_left(
                ui,
                icons::IDENTIFICATION_CARD,
                "Copiar ID do servidor",
                false,
            ) {
                app.actions.push(Action::CopyText(tree.id.to_string()));
            }

            if tree.owner_id != tree.me.user_id
                && widgets::menu_item_left(ui, icons::SIGN_OUT, "Sair do servidor", true)
            {
                app.actions.push(Action::Show(Modal::LeaveServer {
                    server: tree.id,
                    name: tree.name.clone(),
                }));
            }
        });
}

/// A categoria como o Discord de hoje a escreve: o nome sem caixa alta, a setinha depois dele e o
/// "+" sempre visível na direita. Devolve se está recolhida.
fn category_row(
    app: &mut Unkvoid,
    ui: &mut Ui,
    tree: &ServerTree,
    category: &Channel,
    collapsed: bool,
    manage: bool,
) -> bool {
    let server = tree.id;

    ui.add_space(8.0);

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    let plus = Rect::from_center_size(pos2(rect.right() - 26.0, rect.center().y), vec2(18.0, 18.0));
    let label = Rect::from_min_max(rect.min, pos2(plus.left() - 4.0, rect.max.y));
    let response = ui.interact(
        label,
        ui.id().with(("category", &category.id)),
        Sense::click(),
    );
    let color = if response.hovered() {
        theme::TEXT
    } else {
        theme::MUTED
    };
    let name = elide(
        ui,
        &category.name,
        theme::regular(14.0),
        label.width() - 40.0,
    );
    let galley = ui
        .painter()
        .layout_no_wrap(name, theme::regular(14.0), color);

    ui.painter().galley(
        pos2(rect.left() + 16.0, rect.center().y - galley.size().y / 2.0),
        galley.clone(),
        color,
    );
    super::icons::paint_glyph(ui, Rect::from_center_size(pos2(rect.left() + 26.0 + galley.size().x, rect.center().y + 1.0), vec2(10.0, 10.0)), if collapsed { icons::CARET_RIGHT } else { icons::CARET_DOWN }, color, 10.0);

    if response.clicked() {
        app.actions
            .push(Action::ToggleCategory(category.id.clone()));
    }

    if response.dnd_hover_payload::<ChannelDrag>().is_some() {
        ui.painter()
            .rect_filled(rect.shrink2(vec2(8.0, 0.0)), theme::RADIUS, theme::HOVER);
    }

    if let Some(drag) = response.dnd_release_payload::<ChannelDrag>() {
        drop_channel(app, tree, &drag.0, Some(&category.id), None);
    }

    if manage {
        let add = ui
            .interact(
                plus,
                ui.id().with(("category-add", &category.id)),
                Sense::click(),
            )
            .tip("Criar canal");

        super::icons::paint_glyph(ui, plus, icons::PLUS, if add.hovered() { theme::BRIGHT } else { theme::MUTED }, 16.0);

        if add.clicked() {
            app.actions.push(Action::Show(Modal::CreateChannel {
                server,
                kind: ChannelKind::Text,
                name: String::new(),
                parent: Some(category.id.clone()),
            }));
        }
    }

    response.context_menu(|ui| channel_menu(app, ui, tree, category));

    collapsed
}

/// Quem pode abrir as configurações de um canal: quem gerencia canais ou as permissões deles.
fn can_edit(tree: &ServerTree, channel: &Channel) -> bool {
    [permissions::MANAGE_CHANNELS, permissions::MANAGE_ROLES]
        .iter()
        .any(|flag| permissions::has(channel.permissions, *flag) && tree.can(*flag))
}

/// O clique direito num canal ou numa categoria.
fn channel_menu(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree, channel: &Channel) {
    ui.set_min_width(200.0);

    let category = channel.kind == ChannelKind::Category;
    if category {
        let readable: Vec<String> = tree
            .channels
            .iter()
            .filter(|child| {
                child.parent_id.as_deref() == Some(&channel.id) && readable_channel(tree, child)
            })
            .map(|child| child.id.clone())
            .collect();
        if !readable.is_empty() && widgets::menu_item(ui, "", "Marcar como lida", false) {
            app.actions
                .extend(readable.into_iter().map(Action::MarkChannelRead));
            ui.close();
        }
        if !app.collapsed.contains(&channel.id) {
            widgets::menu_separator(ui);
        }
        if widgets::menu_check(
            ui,
            "Recolher categoria",
            app.collapsed.contains(&channel.id),
            false,
        ) {
            app.actions.push(Action::ToggleCategory(channel.id.clone()));
            ui.close();
        }
        let categories: Vec<String> = tree
            .channels
            .iter()
            .filter(|item| item.kind == ChannelKind::Category)
            .map(|item| item.id.clone())
            .collect();
        if categories.len() > 1 {
            let all_collapsed = categories.iter().all(|id| app.collapsed.contains(id));
            if widgets::menu_item(
                ui,
                "",
                if all_collapsed {
                    "Expandir todas as categorias"
                } else {
                    "Recolher todas as categorias"
                },
                false,
            ) {
                app.actions.extend(
                    categories
                        .into_iter()
                        .filter(|id| app.collapsed.contains(id) != all_collapsed)
                        .map(Action::ToggleCategory),
                );
                ui.close();
            }
        }
        if can_edit(tree, channel) {
            widgets::menu_separator(ui);
            if widgets::menu_item(ui, "", "Editar categoria", false) {
                app.actions
                    .push(Action::Show(Modal::ChannelSettings(ChannelSettings::new(
                        tree.id, channel,
                    ))));
                ui.close();
            }
            if can_manage_channel(tree, channel)
                && widgets::menu_item(ui, "", "Excluir categoria", true)
            {
                app.actions.push(Action::Ask(Confirm::DeleteChannel {
                    server: tree.id,
                    channel: channel.id.clone(),
                    name: channel.name.clone(),
                }));
                ui.close();
            }
        }
    } else {
        if readable_channel(tree, channel) && widgets::menu_item(ui, "", "Marcar como lida", false)
        {
            app.actions
                .push(Action::MarkChannelRead(channel.id.clone()));
            ui.close();
        }
        if tree.can(permissions::CREATE_INVITE)
            && permissions::has(channel.permissions, permissions::CREATE_INVITE)
        {
            widgets::menu_separator(ui);
            if widgets::menu_item(
                ui,
                "",
                if channel.kind == ChannelKind::Voice {
                    "Convidar para voz"
                } else {
                    "Convite para o canal"
                },
                false,
            ) {
                app.actions.push(Action::Show(Modal::Invite {
                    server: tree.id,
                    code: None,
                }));
                ui.close();
            }
        }
        if channel.kind == ChannelKind::Voice && readable_channel(tree, channel) {
            widgets::menu_separator(ui);
            if widgets::menu_item(ui, "", "Abrir chat", false) {
                app.actions.push(Action::OpenChannel(channel.clone()));
                ui.close();
            }
        }
        if can_manage_channel(tree, channel) {
            widgets::menu_separator(ui);
            let parent = widgets::menu_parent(ui, "Mover para…");

            egui::containers::menu::SubMenu::new()
                .show(ui, &parent, |ui| move_channel_menu(app, ui, tree, channel));
            widgets::menu_separator(ui);
            if can_edit(tree, channel) && widgets::menu_item(ui, "", "Editar canal", false) {
                app.actions
                    .push(Action::Show(Modal::ChannelSettings(ChannelSettings::new(
                        tree.id, channel,
                    ))));
                ui.close();
            }
            if widgets::menu_item(ui, "", "Duplicar canal", false) {
                app.actions.push(Action::request("createChannel", serde_json::json!({ "server": tree.id }), serde_json::json!({ "name": channel.name, "type": channel.kind, "parent_id": channel.parent_id }), crate::backend::Refresh::Tree(tree.id)));
                ui.close();
            }
            if widgets::menu_item(
                ui,
                "",
                if channel.kind == ChannelKind::Voice {
                    "Criar canal de voz"
                } else {
                    "Criar canal de texto"
                },
                false,
            ) {
                app.actions.push(Action::Show(Modal::CreateChannel {
                    server: tree.id,
                    kind: channel.kind,
                    name: String::new(),
                    parent: channel.parent_id.clone(),
                }));
                ui.close();
            }
            if widgets::menu_item(ui, "", "Excluir canal", true) {
                app.actions.push(Action::Ask(Confirm::DeleteChannel {
                    server: tree.id,
                    channel: channel.id.clone(),
                    name: channel.name.clone(),
                }));
                ui.close();
            }
        }
    }
    widgets::menu_separator(ui);
    if widgets::menu_item(
        ui,
        icons::IDENTIFICATION_CARD,
        if category {
            "Copiar ID da categoria"
        } else {
            "Copiar ID do canal"
        },
        false,
    ) {
        app.actions.push(Action::CopyText(channel.id.clone()));
        ui.close();
    }
}

fn readable_channel(tree: &ServerTree, channel: &Channel) -> bool {
    tree.can(permissions::VIEW_CHANNEL)
        && permissions::has(channel.permissions, permissions::VIEW_CHANNEL)
}

fn can_manage_channel(tree: &ServerTree, channel: &Channel) -> bool {
    tree.can(permissions::MANAGE_CHANNELS)
        && permissions::has(channel.permissions, permissions::MANAGE_CHANNELS)
}

/// O canal sendo arrastado na lista, para trocar de categoria ou de lugar.
struct ChannelDrag(String);

/// Solta o canal arrastado em cima de `before` (ou no topo da categoria, sem `before`) e
/// renumera os irmãos do mesmo tipo: a API muda um canal por vez, e duas posições iguais
/// deixariam a ordem ao acaso. Texto e voz se ordenam à parte, como a lista os desenha.
fn drop_channel(
    app: &mut Unkvoid,
    tree: &ServerTree,
    dragged: &str,
    parent: Option<&str>,
    before: Option<&Channel>,
) {
    let Some(moving) = tree.channels.iter().find(|channel| channel.id == dragged) else {
        return;
    };
    let voice = moving.kind == ChannelKind::Voice;
    let mut siblings: Vec<&Channel> = tree
        .channels
        .iter()
        .filter(|channel| {
            channel.id != dragged
                && channel.kind != ChannelKind::Category
                && (channel.kind == ChannelKind::Voice) == voice
                && channel.parent_id.as_deref() == parent
        })
        .collect();

    siblings.sort_by_key(|channel| channel.position);

    let at = match before {
        Some(target) => siblings
            .iter()
            .position(|channel| channel.id == target.id)
            .unwrap_or(siblings.len()),
        None => 0,
    };

    siblings.insert(at, moving);

    for (index, channel) in siblings.into_iter().enumerate() {
        let position = i64::try_from(index).unwrap_or(i64::MAX);
        let moved = channel.id == dragged;
        let reparented = moved && channel.parent_id.as_deref() != parent;

        if channel.position == position && !reparented {
            continue;
        }

        let body = if moved {
            serde_json::json!({ "position": position, "parent_id": parent })
        } else {
            serde_json::json!({ "position": position })
        };

        app.actions.push(Action::request(
            "updateChannel",
            serde_json::json!({ "channel": channel.id }),
            body,
            crate::backend::Refresh::Tree(tree.id),
        ));
    }
}

fn move_channel_menu(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree, channel: &Channel) {
    ui.set_min_width(190.0);
    for category in tree.channels.iter().filter(|item| {
        item.kind == ChannelKind::Category && channel.parent_id.as_deref() != Some(&item.id)
    }) {
        if widgets::menu_item(ui, "", &category.name, false) {
            app.actions.push(Action::request(
                "updateChannel",
                serde_json::json!({ "channel": channel.id }),
                serde_json::json!({ "parent_id": category.id }),
                crate::backend::Refresh::Tree(tree.id),
            ));
            ui.close();
        }
    }
    if channel.parent_id.is_some()
        && widgets::menu_item(ui, "", "Sem categoria", false)
    {
        app.actions.push(Action::request(
            "updateChannel",
            serde_json::json!({ "channel": channel.id }),
            serde_json::json!({ "parent_id": null }),
            crate::backend::Refresh::Tree(tree.id),
        ));
        ui.close();
    }
}

/// Um canal: `#` para texto, alto-falante para voz. O aberto fica aceso; o que tem não lidas
/// fica em negrito branco com a pílula na borda, e o resto, apagado.
fn channel_row(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree, channel: &Channel) {
    let width = ui.available_width();
    let (outer, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
    let rect = Rect::from_min_max(
        pos2(outer.left() + 8.0, outer.top() + 1.0),
        pos2(outer.right() - 8.0, outer.bottom() - 1.0),
    );
    let draggable = can_manage_channel(tree, channel);
    let response = ui.interact(
        rect,
        ui.id().with(("channel", &channel.id)),
        if draggable { Sense::click_and_drag() } else { Sense::click() },
    );

    if draggable {
        response.dnd_set_drag_payload(ChannelDrag(channel.id.clone()));
    }

    let landing = response
        .dnd_hover_payload::<ChannelDrag>()
        .is_some_and(|drag| drag.0 != channel.id);

    if let Some(drag) = response
        .dnd_release_payload::<ChannelDrag>()
        .filter(|drag| drag.0 != channel.id)
    {
        drop_channel(app, tree, &drag.0, channel.parent_id.as_deref(), Some(channel));
    }

    let opened = app
        .reading
        .as_ref()
        .is_some_and(|reading| reading.id == channel.id);
    let unread = app.unread.get(&channel.id).copied().unwrap_or(0) > 0 && !opened;

    if opened {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::SELECTED);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, theme::RADIUS, theme::HOVER);
    }

    // A linha do Discord onde o canal arrastado vai cair: em cima deste.
    if landing {
        ui.painter().hline(
            rect.x_range(),
            outer.top(),
            egui::Stroke::new(2.0, theme::ACCENT),
        );
    }

    if unread {
        let pill = Rect::from_center_size(pos2(outer.left(), rect.center().y), vec2(8.0, 8.0));

        ui.painter()
            .rect_filled(pill, CornerRadius::same(4), Color32::WHITE);
    }

    let in_this_call = app
        .call
        .seat
        .as_ref()
        .is_some_and(|seat| seat.channel == channel.id);

    let color = if opened || unread || response.hovered() || in_this_call {
        theme::BRIGHT
    } else {
        theme::MUTED
    };
    let icon = match channel.kind {
        ChannelKind::Voice => filled_icons::SPEAKER_HIGH,
        _ if hidden_from_everyone(tree, channel) => icons::LOCK_SIMPLE,
        _ => icons::HASH,
    };

    // O alto-falante do canal em que se está fica verde, como no Discord.
    let icon_color = if in_this_call {
        theme::SUCCESS
    } else {
        theme::MUTED
    };

    let icon_font = if channel.kind == ChannelKind::Voice {
        theme::filled(20.0)
    } else {
        theme::icon(20.0)
    };
    ui.painter().text(
        pos2(rect.left() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        icon,
        icon_font,
        icon_color,
    );

    // O convite e a engrenagem do Discord aparecem no canal aberto e no que está sob o cursor.
    let mut tools = Vec::new();

    if opened || response.hovered() || ui.rect_contains_pointer(rect) {
        if can_edit(tree, channel) {
            tools.push((filled_icons::GEAR_SIX, "Editar canal"));
        }

        if tree.can(permissions::CREATE_INVITE) {
            tools.push((filled_icons::USER_PLUS, "Criar convite"));
        }
    }

    let mut used = false;

    for (index, (icon, tip)) in tools.iter().enumerate() {
        let spot = Rect::from_center_size(
            pos2(rect.right() - 14.0 - 22.0 * index as f32, rect.center().y),
            vec2(20.0, 20.0),
        );
        let tool = ui
            .interact(
                spot,
                ui.id().with(("channel-tool", &channel.id, index)),
                Sense::click(),
            )
            .tip(*tip);

        crate::ui::icons::text(ui.painter(), spot.center(), Align2::CENTER_CENTER, icon, theme::filled(16.0), if tool.hovered() {
                theme::BRIGHT
            } else {
                theme::TEXT
            });

        if tool.clicked() {
            used = true;
            app.actions.push(if *icon == filled_icons::GEAR_SIX {
                Action::Show(Modal::ChannelSettings(ChannelSettings::new(
                    tree.id, channel,
                )))
            } else {
                Action::Show(Modal::Invite {
                    server: tree.id,
                    code: None,
                })
            });
        }
    }

    let voice_elapsed = if in_this_call && channel.kind == ChannelKind::Voice {
        app.call.entered_at.map(|entered_at| {
            format_voice_duration(Instant::now().saturating_duration_since(entered_at))
        })
    } else {
        None
    };
    let tools_width = 22.0 * tools.len() as f32;
    let timer_width = if voice_elapsed.is_some() { 58.0 } else { 0.0 };
    let font = if unread || opened || in_this_call {
        theme::bold(16.0)
    } else {
        theme::regular(16.0)
    };
    let name = elide(
        ui,
        &channel.name,
        font.clone(),
        rect.width() - 44.0 - tools_width - timer_width,
    );

    ui.painter().text(
        pos2(rect.left() + 36.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        font,
        color,
    );

    if let Some(elapsed) = voice_elapsed {
        let right = rect.right() - 10.0 - tools_width;
        ui.painter().text(
            pos2(right, rect.center().y),
            Align2::RIGHT_CENTER,
            elapsed,
            egui::FontId::monospace(11.0),
            theme::SUCCESS,
        );
    }

    if response.clicked() && !used {
        let action = if channel.kind == ChannelKind::Voice {
            Action::JoinVoice(channel.clone())
        } else {
            Action::OpenChannel(channel.clone())
        };

        app.actions.push(action);
    }

    response.context_menu(|ui| channel_menu(app, ui, tree, channel));

    if channel.kind == ChannelKind::Voice {
        voice_people(app, ui, tree, channel);
        drop_zone(app, ui, tree, channel, rect.with_max_y(ui.cursor().top()));
    }
}

/// O canal de voz inteiro, com quem está nele, é onde se solta quem foi arrastado de outro: o
/// Discord acende a borda e o fundo em verde, e soltar move a pessoa (ou entra, se for você).
fn drop_zone(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree, channel: &Channel, block: Rect) {
    let zone = ui.interact(
        block,
        ui.id().with(("voice-drop", &channel.id)),
        Sense::hover(),
    );
    let arriving = zone
        .dnd_hover_payload::<VoiceDrag>()
        .filter(|drag| drag.from != channel.id);

    if arriving.is_some() {
        ui.painter()
            .rect_filled(block, theme::RADIUS, theme::SUCCESS.gamma_multiply(0.12));
        ui.painter().rect_stroke(
            block,
            theme::RADIUS,
            egui::Stroke::new(1.5, theme::SUCCESS),
            egui::StrokeKind::Inside,
        );
    }

    let Some(drag) = zone
        .dnd_release_payload::<VoiceDrag>()
        .filter(|drag| drag.from != channel.id)
    else {
        return;
    };

    if Some(drag.user) == app.my_id() {
        app.actions.push(Action::JoinVoice(channel.clone()));
    } else {
        app.actions.push(Action::request(
            "moveVoiceMember",
            serde_json::json!({ "channel": drag.from, "user": drag.user }),
            serde_json::json!({ "channel_id": channel.id }),
            crate::backend::Refresh::Tree(tree.id),
        ));
    }
}

fn format_voice_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let seconds = total_seconds % 60;
    let minutes = (total_seconds / 60) % 60;

    if total_seconds >= 3_600 {
        format!("{}:{minutes:02}:{seconds:02}", total_seconds / 3_600)
    } else {
        format!("{}:{seconds:02}", total_seconds / 60)
    }
}

/// Quem está na voz, embaixo do canal: o avatar pequeno com o anel verde de quem fala, o nome
/// e o mudo. A própria pessoa segue o estado local, sem esperar o servidor.
/// Quem se arrasta de um canal de voz para outro: arrastar é mover, como no Discord.
#[derive(Debug, Clone)]
struct VoiceDrag {
    user: i64,
    from: String,
}

fn voice_people(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree, channel: &Channel) {
    let mine = app
        .call
        .seat
        .as_ref()
        .is_some_and(|seat| seat.channel == channel.id);
    let me = app.my_id();
    // O clique vale na hora: quem acabou de entrar aparece antes de o servidor contar.
    let people = app.voice_people(tree, &channel.id);

    for person in &people {
        let speaking = mine && app.call.speaking.contains(&person.user_id);
        let (muted, deafened) = if Some(person.user_id) == me && mine {
            (app.call.muted, app.call.deafened)
        } else {
            (person.muted, person.deafened)
        };
        let (outer, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
        let row = Rect::from_min_max(
            pos2(outer.left() + 36.0, outer.top() + 1.0),
            pos2(outer.right() - 8.0, outer.bottom() - 1.0),
        );
        let avatar = Rect::from_center_size(
            pos2(outer.left() + 56.0, outer.center().y),
            vec2(24.0, 24.0),
        );
        let member = tree
            .members
            .iter()
            .find(|member| member.user_id == person.user_id)
            .cloned();
        let url = member.as_ref().and_then(|member| member.avatar_url.clone());
        let url = url.as_deref();
        let itself = Some(person.user_id) == me;
        let movable = itself
            || member
                .as_ref()
                .is_some_and(|member| tree.member_actions(member).disconnect);
        let response = ui.interact(
            row,
            ui.id().with(("voice-person", &channel.id, person.user_id)),
            if movable {
                Sense::click_and_drag()
            } else {
                Sense::click()
            },
        );

        if response.hovered() || response.dragged() {
            ui.painter().rect_filled(row, theme::RADIUS, theme::HOVER);
        }

        if movable {
            response.dnd_set_drag_payload(VoiceDrag {
                user: person.user_id,
                from: channel.id.clone(),
            });

            if response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                drag_ghost(ui, row, &person.name, url, muted);
            }
        }

        // O clique abre o perfil ao lado da coluna; o direito, o menu com o que dá para fazer.
        if let Some(member) = &member {
            // Id próprio: o padrão da resposta é o do `context_menu`, que o fecha a cada clique.
            egui::Popup::menu(&response)
                .id(response.id.with("profile-card"))
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .align(egui::RectAlign::RIGHT_START)
                .gap(16.0)
                .frame(super::members::card_frame())
                .show(|ui| super::members::profile_card(app, ui, tree, member, false));
            response.context_menu(|ui| super::members::member_menu(app, ui, tree, member));
        }

        widgets::paint_avatar(ui, avatar, &person.name, url);

        if speaking {
            ui.painter().circle_stroke(
                avatar.center(),
                avatar.width() / 2.0 + 1.0,
                egui::Stroke::new(2.0, theme::SUCCESS),
            );
        }

        let color = if speaking {
            theme::BRIGHT
        } else {
            theme::MUTED
        };

        ui.painter().text(
            pos2(avatar.right() + 8.0, outer.center().y),
            Align2::LEFT_CENTER,
            &person.name,
            theme::regular(15.0),
            color,
        );

        let mut right = outer.right() - 18.0;

        // Como no Discord: o surdo na ponta, o mudo, a câmera e o selo "AO VIVO".
        if deafened {
            crate::ui::icons::headphones_slash(
                ui.painter(),
                Rect::from_center_size(pos2(right - 8.0, outer.center().y), vec2(16.0, 16.0)),
                theme::MUTED,
            );
            right -= 20.0;
        }

        for (shown, icon) in [
            (muted, icons::MICROPHONE_SLASH),
            (
                person.sources.iter().any(|source| source == "camera"),
                icons::VIDEO_CAMERA,
            ),
        ] {
            if shown {
                crate::ui::icons::text(ui.painter(), pos2(right, outer.center().y), Align2::RIGHT_CENTER, icon, theme::icon(16.0), theme::MUTED);
                right -= 20.0;
            }
        }

        // Quem transmite leva o selo vermelho do Discord ao lado dos ícones.
        if person.sources.iter().any(|source| source == "screen") {
            let badge = Rect::from_min_max(
                pos2(right - 46.0, outer.center().y - 8.0),
                pos2(right, outer.center().y + 8.0),
            );

            ui.painter()
                .rect_filled(badge, CornerRadius::same(4), theme::DANGER);
            ui.painter().text(
                badge.center(),
                Align2::CENTER_CENTER,
                "AO VIVO",
                theme::bold(10.0),
                Color32::WHITE,
            );
        }
    }
}

/// A cópia da linha que segue o ponteiro enquanto se arrasta, por cima de tudo, como no Discord.
fn drag_ghost(ui: &Ui, row: Rect, name: &str, url: Option<&str>, muted: bool) {
    let Some(pointer) = ui.ctx().pointer_interact_pos() else {
        return;
    };

    egui::Area::new(egui::Id::new("voice-drag-ghost"))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .fixed_pos(pointer - vec2(row.width() * 0.35, row.height() / 2.0))
        .show(ui.ctx(), |ui| {
            let (ghost, _) = ui.allocate_exact_size(row.size(), Sense::hover());
            let avatar = Rect::from_center_size(
                pos2(ghost.left() + 20.0, ghost.center().y),
                vec2(24.0, 24.0),
            );

            ui.painter()
                .rect_filled(ghost, theme::RADIUS, theme::SELECTED.gamma_multiply(0.92));
            widgets::paint_avatar(ui, avatar, name, url);
            ui.painter().text(
                pos2(avatar.right() + 8.0, ghost.center().y),
                Align2::LEFT_CENTER,
                name,
                theme::bold(15.0),
                theme::BRIGHT,
            );

            if muted {
                crate::ui::icons::text(ui.painter(), pos2(ghost.right() - 12.0, ghost.center().y), Align2::RIGHT_CENTER, icons::MICROPHONE_SLASH, theme::icon(16.0), theme::MUTED);
            }
        });
}

/// O cadeado do canal que `@everyone` não vê: a sobrescrita do cargo de todos nega ver.
fn hidden_from_everyone(tree: &ServerTree, channel: &Channel) -> bool {
    let Some(everyone) = tree.roles.iter().find(|role| role.is_everyone) else {
        return false;
    };

    channel.overwrites.iter().any(|overwrite| {
        overwrite.target_type == core_app::models::OverwriteTarget::Role
            && overwrite.target_id == everyone.id
            && overwrite.deny & permissions::VIEW_CHANNEL != 0
    })
}

fn loading_column(ui: &mut Ui) {
    ui.add_space(theme::HEADER_HEIGHT + 16.0);
    ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(theme::DIM)));
}

/// A Home: os amigos no alto e as conversas diretas embaixo.
fn home_column(app: &mut Unkvoid, ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), theme::HEADER_HEIGHT),
        Sense::hover(),
    );

    let search_rect = Rect::from_min_size(rect.min + vec2(8.0, 5.0), vec2((rect.width() - 16.0).max(0.0), rect.height() - 10.0));
    let search = ui.interact(search_rect, ui.id().with("quick-conversation-open"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    ui.painter().rect_filled(search_rect, CornerRadius::same(6), theme::ELEVATED);
    ui.painter().text(search_rect.center(), Align2::CENTER_CENTER, "Encontre ou comece uma conversa", theme::regular(13.0), theme::MUTED);
    if search.clicked() {
        app.quick_conversation_open = true;
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, theme::RAIL),
    );

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(8.0);

            let on_friends = matches!(app.home, Home::Friends(_));
            let pending = app.pending_requests();

            if home_row(ui, icons::USERS, "Amigos", on_friends, pending) {
                app.actions.push(Action::OpenFriends(FriendsTab::All));
            }

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new("Mensagens diretas")
                        .font(theme::regular(14.0))
                        .color(theme::MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let add = widgets::icon_button(ui, icons::PLUS, "Criar MD", theme::MUTED);
                    egui::Popup::menu(&add).id(add.id.with("start-direct")).show(|ui| {
                        ui.set_min_width(220.0);
                        ui.label(egui::RichText::new("Começar uma conversa").font(theme::bold(13.0)));
                        widgets::menu_separator(ui);
                        let own_user_id = app.my_id();
                        let people: Vec<_> = app.friends.iter().filter(|friendship| friendship.status == core_app::models::FriendshipStatus::Accepted).filter_map(|friendship| super::home::friend_person(friendship, own_user_id).cloned()).collect();
                        for person in people {
                            if widgets::menu_item(ui, "", &person.name, false) {
                                app.actions.push(Action::OpenDirect(person));
                                ui.close();
                            }
                        }
                    });
                });
            });
            ui.add_space(4.0);

            let conversations: Vec<_> = app.conversations.iter().filter(|conversation| !app.hidden_conversations.contains(&conversation.user.id) || conversation.unread > 0).cloned().collect();

            for conversation in &conversations {
                let opened =
                    matches!(&app.home, Home::Direct(person) if person.id == conversation.user.id);

                if let Some((user, hide)) = direct_row(ui, conversation, opened) {
                    if !hide {
                    app.actions
                        .push(Action::OpenDirect(conversation.user.clone()));
                    } else {
                        app.actions.push(Action::HideConversation(user));
                    }
                }
            }

            if conversations.is_empty() {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("Nenhuma conversa ainda.")
                            .font(theme::regular(13.0))
                            .color(theme::DIM),
                    );
                });
            }
        });
}

fn home_row(ui: &mut Ui, icon: &str, label: &str, opened: bool, badge: i64) -> bool {
    let (outer, _) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::hover());
    let rect = outer.shrink2(vec2(8.0, 1.0));
    let response = ui.interact(rect, ui.id().with(("home", label)), Sense::click());

    if opened {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::SELECTED);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, theme::RADIUS, theme::HOVER);
    }

    let color = if opened || response.hovered() {
        theme::TEXT
    } else {
        theme::MUTED
    };

    crate::ui::icons::text(ui.painter(), pos2(rect.left() + 14.0, rect.center().y), Align2::LEFT_CENTER, icon, theme::icon(22.0), color);
    ui.painter().text(
        pos2(rect.left() + 46.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::bold(15.0),
        color,
    );

    if badge > 0 {
        widgets::paint_badge(
            ui,
            pos2(rect.right() - 18.0, rect.center().y),
            badge,
            theme::SIDEBAR,
        );
    }

    response.clicked()
}

fn direct_row(ui: &mut Ui, conversation: &core_app::models::Conversation, opened: bool) -> Option<(i64, bool)> {
    let (outer, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
    let rect = outer.shrink2(vec2(8.0, 1.0));
    let response = ui.interact(
        rect,
        ui.id().with(("direct", conversation.user.id)),
        Sense::click(),
    ).on_hover_cursor(egui::CursorIcon::PointingHand);

    if opened {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::SELECTED);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, theme::RADIUS, theme::HOVER);
    }

    let avatar =
        Rect::from_center_size(pos2(rect.left() + 24.0, rect.center().y), vec2(32.0, 32.0));
    let unread = conversation.unread > 0;
    let color = if opened || unread || response.hovered() {
        theme::TEXT
    } else {
        theme::MUTED
    };

    widgets::paint_avatar(
        ui,
        avatar,
        &conversation.user.name,
        conversation.user.avatar_url.as_deref(),
    );
    widgets::paint_status(ui, avatar, conversation.user.status, true, if response.hovered() || opened { theme::SELECTED } else { theme::SIDEBAR });
    ui.painter().text(
        pos2(avatar.right() + 12.0, rect.center().y - 8.0),
        Align2::LEFT_CENTER,
        &conversation.user.name,
        theme::bold(16.0),
        color,
    );

    let subtitle = conversation.user.activity.as_ref().map(|activity| format!("Jogando {}", activity.name)).unwrap_or_else(|| match conversation.user.status {
        core_app::models::UserStatus::Online => "Disponível".to_owned(),
        core_app::models::UserStatus::Idle => "Ausente".to_owned(),
        core_app::models::UserStatus::Dnd => "Não perturbar".to_owned(),
        core_app::models::UserStatus::Invisible | core_app::models::UserStatus::Offline => "Off-line".to_owned(),
    });
    let subtitle = elide(ui, &subtitle, theme::regular(12.0), rect.right() - avatar.right() - 44.0);
    ui.painter().text(
        pos2(avatar.right() + 12.0, rect.center().y + 11.0),
        Align2::LEFT_CENTER,
        subtitle,
        theme::regular(12.0),
        theme::DIM,
    );

    if unread {
        widgets::paint_badge(
            ui,
            pos2(rect.right() - 16.0, rect.center().y),
            conversation.unread,
            theme::SIDEBAR,
        );
    }

    if response.hovered() {
        let close_rect = Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), vec2(24.0, 24.0));
        let close = ui.interact(close_rect, ui.id().with(("hide-direct", conversation.user.id)), Sense::click()).on_hover_text("Fechar conversa");
        crate::ui::icons::text(ui.painter(), close_rect.center(), Align2::CENTER_CENTER, icons::X, theme::icon(14.0), theme::MUTED);
        if close.clicked() { return Some((conversation.user.id, true)); }
    }
    response.clicked().then_some((conversation.user.id, false))
}

#[cfg(test)]
mod direct_message_tests {
    use std::collections::HashSet;

    fn conversation_visible(hidden: &HashSet<i64>, user: i64, unread: i64) -> bool {
        !hidden.contains(&user) || unread > 0
    }

    #[test]
    fn hidden_conversations_return_when_a_new_message_is_unread() {
        let hidden = HashSet::from([12]);
        assert!(!conversation_visible(&hidden, 12, 0));
        assert!(conversation_visible(&hidden, 12, 1));
        assert!(conversation_visible(&hidden, 13, 0));
    }
}

/// A altura da parte da voz no cartão, em cima da conta, enquanto se está num canal de voz: o
/// estado, a fileira da câmera e da tela, e a linha que a separa da conta.
const VOICE_PANEL_HEIGHT: f32 = 95.0;

/// O ícone, a fonte e o botão medidos no cartão do Discord a 100%.
const PANEL_BUTTON: f32 = 32.0;
const SPLIT_MAIN: f32 = 33.0;
const SPLIT_CARET: f32 = 17.0;

/// A voz conectada no alto do cartão, como no Discord: o sinal num quadrado verde, "Voz
/// conectada" com o canal e o servidor, a supressão de ruído e o desligar; embaixo, a câmera e a
/// tela. A cor diz o estado: verde conectado, amarelo conectando ou voltando.
fn voice_panel(app: &mut Unkvoid, ui: &mut Ui) {
    let Some(seat) = app.call.seat.clone() else {
        return;
    };
    let (label, status_color) = match app.call.status {
        crate::voice::Status::Connected => ("Voz conectada", theme::SUCCESS),
        crate::voice::Status::Connecting => ("Conectando a RTC", theme::WARNING),
        crate::voice::Status::Reconnecting => ("Sem rota", theme::DANGER),
    };
    let color = if app.call.status == crate::voice::Status::Reconnecting {
        theme::DANGER
    } else {
        ping_color(app.call.ping)
    };
    let code_room = seat.guest.is_some();
    let server = if code_room {
        "Sala por código".to_owned()
    } else {
        app.server_summary(seat.server)
            .map(|server| server.name.clone())
            .unwrap_or_default()
    };
    let server_tree = app.tree.clone();
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
    let badge = Rect::from_min_size(
        pos2(row.left() + 4.0, row.center().y - 16.0),
        vec2(32.0, 32.0),
    );
    let tip = app.call.ping.map_or_else(
        || "Medindo a conexão…".to_owned(),
        |ping| format!("Ping: {ping} ms"),
    );

    ui.painter()
        .rect_filled(badge, theme::RADIUS, color.gamma_multiply(0.14));
    let bars = app.call.ping.map_or(4, core_app::room::signal_bars);
    let signal = match bars {
        1 => filled_icons::CELL_SIGNAL_LOW,
        2 => filled_icons::CELL_SIGNAL_MEDIUM,
        3 => filled_icons::CELL_SIGNAL_HIGH,
        _ => filled_icons::CELL_SIGNAL_FULL,
    };
    crate::ui::icons::text(ui.painter(), badge.center(), Align2::CENTER_CENTER, signal, theme::filled(20.0), color);
    let details_rect =
        Rect::from_min_max(badge.min, pos2(row.right() - 92.0, row.center().y + 1.0));
    let details = ui
        .interact(details_rect, ui.id().with("voice-details"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .tip(tip);
    let title_position = pos2(badge.right() + 8.0, row.center().y - 9.0);
    let title_rect = Rect::from_min_size(
        title_position,
        vec2(row.right() - title_position.x - 92.0, 19.0),
    );
    ui.painter().text(
        title_position,
        Align2::LEFT_CENTER,
        label,
        theme::bold(16.0),
        status_color,
    );
    if details.hovered() {
        let title_width = ui
            .painter()
            .layout_no_wrap(label.to_owned(), theme::bold(16.0), status_color)
            .size()
            .x;
        // Logo embaixo do título: a conta de antes partia do meio do texto, e a linha caía em
        // cima do nome do canal.
        ui.painter().hline(
            title_rect.left()..=(title_rect.left() + title_width),
            title_position.y + 9.0,
            egui::Stroke::new(1.0, status_color),
        );
    }
    ui.painter().text(
        pos2(badge.right() + 8.0, row.center().y + 10.0),
        Align2::LEFT_CENTER,
        elide(
            ui,
            &format!("{} / {server}", seat.name),
            theme::regular(13.0),
            row.width() - 130.0,
        ),
        theme::regular(13.0),
        theme::SECONDARY,
    );

    // Painéis com coisa para mexer dentro fecham só com clique fora ou Esc: o `menu` do egui
    // fecha a cada clique, e o busca, o som, o volume ou o teste sumiam no primeiro toque.
    egui::Popup::menu(&details)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::TOP_START)
        .gap(8.0)
        .frame(widgets::menu_frame())
        .show(|ui| {
            ui.set_width(300.0);
            voice_connection_details(app, ui);
        });

    let leave = Rect::from_center_size(
        pos2(row.right() - 20.0, row.center().y),
        vec2(PANEL_BUTTON, PANEL_BUTTON),
    );
    let noise = Rect::from_center_size(
        pos2(leave.center().x - 36.0, row.center().y),
        vec2(PANEL_BUTTON, PANEL_BUTTON),
    );
    let filtering = app.voice_settings["noise"].as_str() == Some("light");
    let noise_tip = if filtering {
        "Supressão de ruído: ligada"
    } else {
        "Supressão de ruído: desligada"
    };

    let noise_response = panel_icon(
        ui,
        noise,
        "noise",
        icons::WAVEFORM,
        noise_tip,
        if filtering { theme::TEXT } else { theme::MUTED },
        false,
    );
    egui::Popup::menu(&noise_response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::TOP_START)
        .gap(8.0)
        .frame(widgets::menu_frame())
        .show(|ui| {
            app.noise_test_seen = true;
            ui.set_width(320.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Supressão de ruído").font(theme::bold(15.0)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::switch(ui, filtering).clicked() {
                        app.actions.push(Action::SetVoice(
                            "noise",
                            serde_json::json!(if filtering { "off" } else { "light" }),
                        ));
                    }
                });
            });
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(
                    "Tira o barulho de fundo — teclado, ventilador, rua — enquanto você fala.",
                )
                .color(theme::MUTED),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Teste do microfone").font(theme::bold(13.0)));
                if widgets::secondary_button(
                    ui,
                    if app.noise_test_active {
                        "Parar"
                    } else {
                        "Testar"
                    },
                ) {
                    app.actions.push(Action::ToggleNoiseTest);
                }
            });
            if app.noise_test_active {
                super::settings::meter(ui, app.call_level, None);
            }
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("Fornecido por")
                    .font(theme::regular(12.0))
                    .color(theme::DIM),
            );
            ui.label(
                egui::RichText::new("RNNoise")
                    .font(theme::regular(14.0))
                    .color(theme::TEXT),
            );
        });

    if panel_icon(
        ui,
        leave,
        "leave",
        icons::PHONE_DISCONNECT,
        "Desconectar",
        theme::TEXT,
        false,
    )
    .clicked()
    {
        app.actions.push(Action::LeaveVoice);
    }

    ui.add_space(4.0);

    widgets::columns(ui, 3, |columns| {
        // A sala por código é só para a tela, como na própria sala.
        let camera_allowed = crate::camera::FILMS && app.call.mine.can_video && !code_room;
        let camera_on = app.call.mine.camera;
        let camera = panel_video_button(
            &mut columns[0],
            if camera_on {
                icons::VIDEO_CAMERA
            } else {
                icons::VIDEO_CAMERA_SLASH
            },
            camera_allowed,
            camera_on,
            false,
        )
        .tip(if !crate::camera::FILMS {
            "A câmera ainda não chegou a este sistema"
        } else if code_room {
            "A sala por código é só para compartilhar a tela"
        } else if !app.call.mine.can_video {
            "Você não tem permissão de vídeo neste canal"
        } else if camera_on {
            "Desligar câmera"
        } else {
            "Ligar câmera"
        });

        if camera.clicked() && camera_allowed {
            app.actions.push(Action::ToggleCamera);
        }

        let live = app.call.mine.sharing;
        let share = panel_video_button(
            &mut columns[1],
            icons::MONITOR_ARROW_UP,
            app.call.mine.can_share,
            live,
            live,
        )
        .tip(if !app.call.mine.can_share {
            "Você não tem permissão para transmitir neste canal"
        } else if live {
            "Gerenciar transmissão"
        } else {
            "Compartilhar sua tela"
        });

        if live {
            egui::Popup::menu(&share)
                .align(egui::RectAlign::TOP_START)
                .gap(8.0)
                .frame(widgets::menu_frame())
                .show(|ui| {
                    if widgets::menu_item(ui, icons::MONITOR, "Trocar transmissão", false) {
                        app.actions.push(Action::Show(Modal::GoLive(
                            crate::app::GoLiveTab::Screens,
                        )));
                    }

                    if widgets::menu_item(ui, icons::X_CIRCLE, "Parar transmissão", true) {
                        app.actions.push(Action::StopShare);
                    }
                });
        } else if share.clicked() && app.call.mine.can_share {
            app.actions.push(Action::Show(Modal::GoLive(
                crate::app::GoLiveTab::Screens,
            )));
        }

        let allowed = !app.call.muted && !app.call.deafened && app.call.can_speak;
        let (rect, response) = columns[2].allocate_exact_size(
            vec2(columns[2].available_width(), PANEL_BUTTON),
            if allowed {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        columns[2].painter().rect_filled(
            rect,
            theme::RADIUS,
            if response.hovered() {
                theme::BUTTON_HOVER
            } else {
                theme::BUTTON
            },
        );
        super::icons::paint_glyph(&columns[2], rect, filled_icons::MEGAPHONE, if allowed { theme::TEXT } else { theme::DIM }, 20.0);
        let response = response.tip(if allowed {
            "Abrir soundboard"
        } else {
            "Você está silenciado"
        });
        if response.clicked() && allowed {
            app.soundboard_search.clear();
            app.actions
                .push(Action::LoadServerSounds(app.opened.unwrap_or_default()));
        }
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .align(egui::RectAlign::TOP_START)
            .gap(8.0)
            .frame(widgets::menu_frame())
            .show(|ui| soundboard(app, ui, &server, server_tree.as_ref()));
    });

    ui.add_space(8.0);

    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());

    ui.painter().hline(
        line.x_range().expand(8.0),
        line.center().y,
        egui::Stroke::new(1.0, theme::DIVIDER),
    );
    ui.add_space(6.0);
}

/// O painel de efeitos sonoros, como o do Discord: a busca com a lupa e o volume em cima, o trilho
/// das seções à esquerda e, à direita, cada seção com o cabeçalho e a grade de três botões.
pub(super) fn soundboard(app: &mut Unkvoid, ui: &mut Ui, server_name: &str, tree: Option<&ServerTree>) {
    const WIDTH: f32 = 530.0;
    const HEIGHT: f32 = 440.0;

    ui.set_width(WIDTH);
    ui.set_height(HEIGHT);
    ui.spacing_mut().item_spacing = vec2(8.0, 0.0);

    ui.horizontal(|ui| {
        ui.allocate_ui(vec2(WIDTH - 48.0, 40.0), |ui| sound_search(ui, &mut app.soundboard_search));

        let (button, response) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());

        super::icons::paint_glyph(ui, button, filled_icons::SPEAKER_HIGH, if response.hovered() { theme::BRIGHT } else { theme::SECONDARY }, 22.0);

        egui::containers::menu::SubMenu::new().show(ui, &response.tip("Volume do painel"), |ui| {
                ui.set_width(200.0);
                ui.label(
                    egui::RichText::new("Volume do painel")
                        .font(theme::bold(12.0))
                        .color(theme::MUTED),
                );
                ui.add_space(8.0);

                #[allow(clippy::cast_precision_loss)]
                let volume = app.voice_settings["soundboardVolume"]
                    .as_u64()
                    .unwrap_or(100)
                    .min(100) as f32;

                if let Some(value) =
                    widgets::held_slider(ui, "soundboard-volume", volume, 0.0..=100.0, true)
                {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    app.actions.push(Action::SetVoice(
                        "soundboardVolume",
                        serde_json::json!(value.round() as u8),
                    ));
                }
            });
    });

    ui.add_space(8.0);

    let (line, _) = ui.allocate_exact_size(vec2(WIDTH, 1.0), Sense::hover());

    ui.painter().rect_filled(line, 0.0, theme::MENU_BORDER);

    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.add_space(8.0);
            ui.spacing_mut().item_spacing.y = 8.0;

            for (section, icon, tip) in [
                ("frequent", filled_icons::CLOCK, "Utilizados com frequência"),
                ("server", filled_icons::USERS_THREE, server_name),
                ("unkvoid", filled_icons::MUSIC_NOTES, "Sons do Unkvoid"),
            ] {
                let (rect, response) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());

                if response.hovered() {
                    ui.painter().rect_filled(rect, theme::RADIUS, theme::HOVER);
                }

                crate::ui::icons::text(ui.painter(), rect.center(), Align2::CENTER_CENTER, icon, theme::filled(20.0), if response.hovered() {
                        theme::BRIGHT
                    } else {
                        theme::SECONDARY
                    });

                if response.tip(tip).clicked() {
                    app.soundboard_scroll_to = Some(section.to_owned());
                }
            }
        });

        let (rail, _) = ui.allocate_exact_size(vec2(1.0, HEIGHT - 60.0), Sense::hover());

        ui.painter().rect_filled(rail, 0.0, theme::MENU_BORDER);

        // Dentro da linha horizontal a rolagem herdava a direção dela: as seções saíam uma ao
        // lado da outra, para fora do painel.
        ui.vertical(|ui| {
            egui::ScrollArea::vertical()
                .id_salt("soundboard-sections")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 0.0;

                    let query = app.soundboard_search.to_lowercase();
                    let frequent = app.voice_settings["soundboardFrequent"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    let mut recent: Vec<_> = frequent
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .filter_map(sound_from_name)
                        .take(6)
                        .collect();

                    recent.retain(|sound| sound.label().0.to_lowercase().contains(&query));

                    if section_heading(app, ui, "frequent", filled_icons::CLOCK, "Utilizados com frequência") {
                        built_in_grid(app, ui, &recent);
                    }

                    if section_heading(app, ui, "server", filled_icons::USERS_THREE, server_name) {
                        let can_manage = tree.is_some_and(|tree| tree.can(permissions::MANAGE_EXPRESSIONS));
                        let sounds: Vec<_> = app
                            .server_sounds
                            .iter()
                            .filter(|sound| sound.name.to_lowercase().contains(&query))
                            .cloned()
                            .collect();

                        server_grid(app, ui, &sounds, can_manage, tree.map(|tree| tree.id));
                    }

                    let mut all: Vec<_> = core_app::chimes::SoundboardSound::ALL.into_iter().collect();

                    all.retain(|sound| sound.label().0.to_lowercase().contains(&query));

                    if section_heading(app, ui, "unkvoid", filled_icons::MUSIC_NOTES, "Sons do Unkvoid") {
                        built_in_grid(app, ui, &all);
                    }
                });
        });
    });
}

/// A busca do painel: a lupa, o "Encontre o som perfeito" e o aro roxo com ela em foco.
fn sound_search(ui: &mut Ui, query: &mut String) {
    let id = ui.id().with("soundboard-search");
    let focused = ui.memory(|memory| memory.has_focus(id));

    Frame::new()
        .fill(theme::FIELD)
        .stroke(egui::Stroke::new(
            1.0,
            if focused {
                theme::ACCENT
            } else {
                theme::FIELD_BORDER
            },
        ))
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                crate::ui::icons::label(ui, icons::MAGNIFYING_GLASS, theme::icon(18.0), theme::SECONDARY);
                ui.add(
                    egui::TextEdit::singleline(query)
                        .id(id)
                        .hint_text(
                            egui::RichText::new("Encontre o som perfeito")
                                .font(theme::regular(15.0))
                                .color(theme::PLACEHOLDER),
                        )
                        .frame(egui::Frame::NONE)
                        .font(theme::regular(15.0))
                        .desired_width(f32::INFINITY),
                );
            });
        });
}

/// O cabeçalho de uma seção: o ícone, o nome e a seta, que abre e fecha. Devolve se está aberta.
fn section_heading(app: &mut Unkvoid, ui: &mut Ui, id: &str, icon: &str, title: &str) -> bool {
    let collapsed = app.soundboard_collapsed.contains(id);

    ui.add_space(12.0);

    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    let tint = if response.hovered() {
        theme::BRIGHT
    } else {
        theme::SECONDARY
    };

    crate::ui::icons::text(ui.painter(), pos2(rect.left() + 8.0, rect.center().y), Align2::CENTER_CENTER, icon, theme::filled(16.0), tint);

    let title = ui
        .painter()
        .layout_no_wrap(title.to_owned(), theme::bold(14.0), tint);
    let caret_x = rect.left() + 22.0 + title.size().x + 8.0;

    ui.painter().galley(
        pos2(rect.left() + 20.0, rect.center().y - title.size().y / 2.0),
        title,
        tint,
    );
    crate::ui::icons::text(ui.painter(), pos2(caret_x, rect.center().y), Align2::LEFT_CENTER, if collapsed {
            icons::CARET_RIGHT
        } else {
            icons::CARET_DOWN
        }, theme::icon(14.0), tint);

    if app.soundboard_scroll_to.as_deref() == Some(id) {
        ui.scroll_to_rect(rect, Some(egui::Align::Min));
        app.soundboard_scroll_to = None;
    }

    if response.clicked() && !app.soundboard_collapsed.remove(id) {
        app.soundboard_collapsed.insert(id.to_owned());
    }

    ui.add_space(8.0);

    !collapsed
}

/// Onde cai cada botão da grade de três colunas, como a do Discord: 40 px de altura, 8 de vão.
fn sound_slots(ui: &mut Ui, count: usize) -> Vec<Rect> {
    const GAP: f32 = 8.0;
    const HEIGHT: f32 = 40.0;

    let width = ((ui.available_width() - 4.0 - 2.0 * GAP) / 3.0).max(0.0);
    let rows = count.div_ceil(3);
    #[allow(clippy::cast_precision_loss)]
    let (area, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), rows as f32 * (HEIGHT + GAP)),
        Sense::hover(),
    );

    (0..count)
        .map(|index| {
            #[allow(clippy::cast_precision_loss)]
            let origin = area.min
                + vec2(
                    (index % 3) as f32 * (width + GAP),
                    (index / 3) as f32 * (HEIGHT + GAP),
                );

            Rect::from_min_size(origin, vec2(width, HEIGHT))
        })
        .collect()
}

/// Um botão de som: o emoji e o nome no meio, o cinza do Discord e o hover mais claro.
fn sound_button(ui: &Ui, rect: Rect, id: egui::Id, emoji: &str, name: &str) -> egui::Response {
    let response = ui.interact(rect, id, Sense::click());

    ui.painter().rect_filled(
        rect,
        theme::RADIUS,
        if response.hovered() {
            theme::BUTTON_HOVER
        } else {
            theme::BUTTON
        },
    );

    let mut label = egui::text::LayoutJob::default();

    label.append(
        &format!("{emoji}  "),
        0.0,
        egui::TextFormat::simple(theme::regular(15.0), theme::TEXT),
    );
    label.append(
        name,
        0.0,
        egui::TextFormat::simple(theme::bold(14.0), theme::BRIGHT),
    );
    label.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 16.0);

    let galley = ui.painter().layout_job(label);

    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, theme::TEXT);

    response
}

fn built_in_grid(app: &mut Unkvoid, ui: &mut Ui, sounds: &[core_app::chimes::SoundboardSound]) {
    if sounds.is_empty() {
        empty_section(ui);

        return;
    }

    for (rect, sound) in sound_slots(ui, sounds.len()).into_iter().zip(sounds) {
        let (name, emoji) = sound.label();

        if sound_button(ui, rect, ui.id().with(("built-in-sound", sound_name(*sound))), emoji, name).clicked() {
            let mut recent = app.voice_settings["soundboardFrequent"]
                .as_array()
                .cloned()
                .unwrap_or_default();

            recent.retain(|value| value.as_str() != Some(sound_name(*sound)));
            recent.insert(0, serde_json::json!(sound_name(*sound)));
            recent.truncate(6);
            app.actions.push(Action::SetVoice(
                "soundboardFrequent",
                serde_json::Value::Array(recent),
            ));
            app.actions.push(Action::PlaySoundboard(*sound));
        }
    }
}

/// Os sons do servidor e, para quem pode, o "+ Adicionar som" tracejado; o clique direito num som
/// do servidor oferece excluí-lo.
fn server_grid(
    app: &mut Unkvoid,
    ui: &mut Ui,
    sounds: &[core_app::models::ServerSound],
    can_manage: bool,
    server: Option<i64>,
) {
    let slots = sound_slots(ui, sounds.len() + usize::from(can_manage));

    for (rect, sound) in slots.iter().zip(sounds) {
        let response = sound_button(
            ui,
            *rect,
            ui.id().with(("server-sound", sound.id)),
            sound.emoji.as_deref().unwrap_or("🎵"),
            &sound.name,
        );

        if response.clicked() {
            app.actions.push(Action::PlayServerSound(sound.clone()));
        }

        if can_manage {
            response.context_menu(|ui| {
                if widgets::menu_item(ui, icons::TRASH, "Excluir som", true) {
                    if let Some(server) = server {
                        app.actions
                            .push(Action::DeleteServerSound(server, sound.id));
                    }

                    ui.close();
                }
            });
        }
    }

    if can_manage
        && let Some(rect) = slots.last()
        && let Some(server) = server
    {
        let response = ui.interact(*rect, ui.id().with("add-server-sound"), Sense::click());
        let tint = if response.hovered() {
            theme::BRIGHT
        } else {
            theme::SECONDARY
        };

        ui.painter().add(egui::Shape::dashed_line(
            &[
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
                rect.left_top(),
            ],
            egui::Stroke::new(1.0, theme::TRACK),
            4.0,
            3.0,
        ));
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            format!("{}  Adicionar som", icons::PLUS),
            theme::regular(13.0),
            tint,
        );

        if response.clicked() {
            app.actions.push(Action::PickServerSound(server));
        }
    }

    if sounds.is_empty() && !can_manage {
        empty_section(ui);
    }
}

fn empty_section(ui: &mut Ui) {
    ui.label(
        egui::RichText::new("Nenhum som aqui.")
            .font(theme::regular(13.0))
            .color(theme::MUTED),
    );
    ui.add_space(4.0);
}

fn sound_name(sound: core_app::chimes::SoundboardSound) -> &'static str {
    match sound {
        core_app::chimes::SoundboardSound::Pulse => "pulse",
        core_app::chimes::SoundboardSound::Spark => "spark",
        core_app::chimes::SoundboardSound::Orbit => "orbit",
        core_app::chimes::SoundboardSound::Breeze => "breeze",
        core_app::chimes::SoundboardSound::Prism => "prism",
        core_app::chimes::SoundboardSound::Echo => "echo",
        core_app::chimes::SoundboardSound::Aurora => "aurora",
        core_app::chimes::SoundboardSound::Comet => "comet",
    }
}

fn sound_from_name(name: &str) -> Option<core_app::chimes::SoundboardSound> {
    core_app::chimes::SoundboardSound::ALL
        .into_iter()
        .find(|sound| sound_name(*sound) == name)
}

fn ping_color(ping: Option<u64>) -> Color32 {
    match crate::voice::ping_quality(ping) {
        crate::voice::PingQuality::Good => theme::SUCCESS,
        crate::voice::PingQuality::Fair => theme::WARNING,
        crate::voice::PingQuality::Poor => theme::ORANGE,
        crate::voice::PingQuality::Bad => theme::DANGER,
    }
}

fn voice_connection_details(app: &mut Unkvoid, ui: &mut Ui) {
    let history = app.call.ping_history.clone();
    let host = app.call.sfu_host.clone();
    let (tab, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    ui.painter().text(
        tab.left_top(),
        Align2::LEFT_TOP,
        "Conexão",
        theme::bold(13.0),
        theme::ACCENT_TEXT,
    );
    ui.painter().hline(
        tab.x_range(),
        tab.bottom(),
        egui::Stroke::new(1.0, theme::DIVIDER),
    );
    ui.painter().hline(
        tab.left()..=(tab.left() + 54.0),
        tab.bottom(),
        egui::Stroke::new(2.0, theme::ACCENT),
    );
    ui.add_space(8.0);
    ping_graph(ui, &history);
    ui.add_space(10.0);

    if let Some(host) = host.as_deref().filter(|host| !host.is_empty()) {
        ui.label(
            egui::RichText::new(host)
                .font(theme::bold(14.0))
                .color(theme::BRIGHT),
        );
    }

    let average = history
        .average_ms()
        .map_or_else(|| "—".to_owned(), |milliseconds| milliseconds.to_string());
    let latest = history
        .latest_ms()
        .map_or_else(|| "—".to_owned(), |milliseconds| milliseconds.to_string());
    ui.label(
        egui::RichText::new(format!("Ping médio: {average} ms"))
            .font(theme::regular(12.0))
            .color(theme::TEXT),
    );
    ui.label(
        egui::RichText::new(format!("Último ping: {latest} ms"))
            .font(theme::regular(12.0))
            .color(theme::TEXT),
    );
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Acima de 250 ms, a voz pode chegar atrasada. Perda alta de pacotes pode deixar o áudio robótico.").font(theme::regular(12.0)).color(theme::SECONDARY));
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        for (label, icon) in [
            ("Depuração", icons::BUG),
            ("Enviar registros", icons::UPLOAD_SIMPLE),
        ] {
            let button = ui.add(
                egui::Button::new(
                    egui::RichText::new(format!("{icon}  {label}"))
                        .font(theme::regular(12.0))
                        .color(theme::TEXT),
                )
                .fill(theme::BUTTON)
                .corner_radius(theme::RADIUS),
            );
            if button.clicked() {
                if label == "Depuração" {
                    app.actions.push(Action::Show(Modal::VoiceDebug(
                        crate::ui::voice_debug::DebugPage::Transport,
                    )));
                } else {
                    app.actions.push(Action::SendVoiceLogs);
                }
            }
        }
    });
}

fn ping_graph(ui: &mut Ui, history: &crate::voice::PingHistory) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 90.0), Sense::hover());
    let plot = Rect::from_min_max(rect.min + vec2(5.0, 5.0), rect.max - vec2(34.0, 19.0));
    let now = Instant::now();
    let visible: Vec<_> = history
        .visible_since(now - Duration::from_secs(4 * 60))
        .map(|sample| {
            (
                now.saturating_duration_since(sample.at),
                sample.milliseconds,
            )
        })
        .collect();
    let scale = visible
        .iter()
        .map(|(_, milliseconds)| *milliseconds)
        .max()
        .unwrap_or(1)
        .max(1);
    let painter = ui.painter();

    painter.rect_filled(rect, CornerRadius::same(4), theme::ELEVATED);
    painter.rect_filled(plot, CornerRadius::same(3), theme::RAIL);

    for minutes_ago in (0..=4).rev() {
        let fraction = (4 - minutes_ago) as f32 / 4.0;
        let x = plot.left() + plot.width() * fraction;
        painter.vline(x, plot.y_range(), egui::Stroke::new(1.0, theme::DIVIDER));
        let time = chrono::Local::now() - chrono::Duration::minutes(minutes_ago);
        painter.text(
            pos2(x, plot.bottom() + 2.0),
            Align2::CENTER_TOP,
            time.format("%H:%M").to_string(),
            theme::regular(8.0),
            theme::SECONDARY,
        );
    }

    for milliseconds in [scale, 0] {
        let y = plot.bottom() - plot.height() * milliseconds as f32 / scale as f32;
        painter.text(
            pos2(plot.right() + 5.0, y),
            Align2::LEFT_CENTER,
            milliseconds.to_string(),
            theme::regular(9.0),
            theme::SECONDARY,
        );
    }

    let points: Vec<_> = visible
        .iter()
        .rev()
        .map(|(age, milliseconds)| {
            let x = plot.right() - plot.width() * (age.as_secs_f32() / 240.0).clamp(0.0, 1.0);
            let y = plot.bottom() - plot.height() * (*milliseconds as f32 / scale as f32);

            pos2(x, y)
        })
        .collect();

    let stroke = egui::Stroke::new(2.0, theme::ACCENT);
    for pair in points.windows(2) {
        let [from, to] = pair else { continue };
        let corner = pos2(to.x, from.y);
        painter.line_segment([*from, corner], stroke);
        painter.line_segment([corner, *to], stroke);
        painter.circle_filled(corner, 1.0, theme::ACCENT);
    }
    if points.len() == 1 {
        painter.circle_filled(points[0], 1.5, theme::ACCENT);
    }
}

/// Um ícone de 20 px num quadrado de 32 que acende no hover: os do cartão da voz e a engrenagem.
/// Um botão cheio do cartão da voz (câmera, tela): aceso quando ligado, apagado sem permissão.
/// `red` é a transmissão no ar: o botão fica vermelho como o microfone mudo, é o que se clica
/// para parar.
fn panel_video_button(ui: &mut Ui, icon: &str, allowed: bool, on: bool, red: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), PANEL_BUTTON),
        if allowed { Sense::click() } else { Sense::hover() },
    );
    let fill = if !allowed {
        theme::BUTTON.gamma_multiply(0.7)
    } else if red {
        if response.hovered() {
            theme::OFF_HOVER
        } else {
            theme::OFF
        }
    } else if on || response.hovered() {
        theme::BUTTON_HOVER
    } else {
        theme::BUTTON
    };

    ui.painter().rect_filled(rect, theme::RADIUS, fill);
    crate::ui::icons::text(ui.painter(), rect.center(), Align2::CENTER_CENTER, icon, theme::icon(20.0), if !allowed {
            theme::DIM
        } else if red {
            theme::DANGER
        } else if on {
            theme::BRIGHT
        } else {
            theme::TEXT
        });

    if allowed {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

fn panel_icon(
    ui: &mut Ui,
    rect: Rect,
    id: &str,
    icon: &str,
    tip: &str,
    color: Color32,
    filled: bool,
) -> egui::Response {
    let response = ui
        .interact(rect, ui.id().with(id), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .tip(tip);

    if response.hovered() {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::SELECTED);
    }

    let font = if filled {
        theme::filled(20.0)
    } else {
        theme::icon(20.0)
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon,
        font,
        if response.hovered() {
            theme::BRIGHT
        } else {
            color
        },
    );

    response
}

/// A conta no pé do cartão, como no Discord: a foto com o status, o nome e o status embaixo, o
/// microfone e o fone em dois botões cada (o ícone liga e desliga, a setinha abre as opções) e a
/// engrenagem.
fn user_panel(app: &mut Unkvoid, ui: &mut Ui) {
    let Some(me) = app.me.clone() else {
        return;
    };
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
    let avatar = Rect::from_min_size(
        pos2(row.left() + 2.0, row.center().y - 16.0),
        vec2(32.0, 32.0),
    );

    let gear = Rect::from_center_size(
        pos2(row.right() - 18.0, row.center().y),
        vec2(PANEL_BUTTON, PANEL_BUTTON),
    );
    let speaker = Rect::from_min_size(
        pos2(
            gear.left() - 8.0 - SPLIT_MAIN - SPLIT_CARET,
            row.center().y - 16.0,
        ),
        vec2(SPLIT_MAIN + SPLIT_CARET, 32.0),
    );
    let microphone = speaker.translate(vec2(-(SPLIT_MAIN + SPLIT_CARET + 8.0), 0.0));
    let room = microphone.left() - avatar.right() - 16.0;
    let account_trigger_rect = Rect::from_min_max(
        row.left_top(),
        pos2(microphone.left() - 6.0, row.bottom()),
    );
    let account_trigger = ui
        .interact(
            account_trigger_rect,
            ui.id().with("account-card"),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);

    if account_trigger.hovered() {
        ui.painter().rect_filled(
            account_trigger_rect,
            CornerRadius::same(6),
            theme::BUTTON_HOVER,
        );
    }

    // Por cima do fundo do hover, senão ele apaga o avatar.
    widgets::paint_avatar(ui, avatar, &me.name, me.avatar_url.as_deref());
    widgets::paint_status(
        ui,
        avatar,
        me.status,
        true,
        if account_trigger.hovered() { theme::BUTTON_HOVER } else { theme::ELEVATED },
    );

    ui.painter().text(
        pos2(avatar.right() + 8.0, row.center().y - 8.0),
        Align2::LEFT_CENTER,
        elide(ui, &me.name, theme::bold(15.0), room),
        theme::bold(15.0),
        theme::BRIGHT,
    );
    ui.painter().text(
        pos2(avatar.right() + 8.0, row.center().y + 10.0),
        Align2::LEFT_CENTER,
        status_label(me.status),
        theme::regular(13.0),
        theme::SECONDARY,
    );

    let account_popup_id = egui::Popup::default_response_id(&account_trigger);

    if app.account_popup_requested {
        egui::Popup::open_id(ui.ctx(), account_popup_id);
        app.account_popup_requested = false;
    }

    egui::Popup::menu(&account_trigger)
        .id(account_popup_id)
        .align(egui::RectAlign::TOP_START)
        .gap(8.0)
        .width(300.0)
        .frame(widgets::menu_frame().inner_margin(egui::Margin::same(12)))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| account_popup(app, ui, &me));

    let (icon, tip, off) = if !app.call.can_speak {
        (
            filled_icons::MICROPHONE_SLASH,
            "Você não tem permissão para falar aqui",
            true,
        )
    } else if app.call.muted {
        (filled_icons::MICROPHONE_SLASH, "Dessilenciar", true)
    } else {
        (filled_icons::MICROPHONE, "Silenciar", false)
    };
    let (main, caret) = split_at(ui, microphone, "mute", icon, tip, off, "Opções de entrada");

    if main.clicked() {
        app.actions.push(Action::ToggleMute);
    }

    device_menu(app, &caret, true);

    let (tip, off) = if app.call.deafened {
        ("Desativar ensurdecer", true)
    } else {
        ("Ensurdecer", false)
    };
    let (main, caret) = split_at(
        ui,
        speaker,
        "deafen",
        filled_icons::HEADPHONES,
        tip,
        off,
        "Opções de saída",
    );

    if off {
        // O Phosphor não tem o fone cortado: o traço do Discord vai por cima.
        let cut = Rect::from_center_size(main.rect.center(), vec2(18.0, 18.0));

        ui.painter().line_segment(
            [cut.left_top(), cut.right_bottom()],
            egui::Stroke::new(2.0, theme::DANGER),
        );
    }

    if main.clicked() {
        app.actions.push(Action::ToggleDeafen);
    }

    device_menu(app, &caret, false);

    if panel_icon(
        ui,
        gear,
        "settings",
        filled_icons::GEAR_SIX,
        "Configurações de usuário",
        theme::TEXT,
        true,
    )
    .clicked()
    {
        app.actions
            .push(Action::Show(Modal::Settings(SettingsPage::Account)));
    }
}

fn account_popup(app: &mut Unkvoid, ui: &mut Ui, user: &core_app::models::User) {
    ui.set_width(276.0);
    let banner = user.profile.banner_color.as_deref().and_then(theme::color_of).unwrap_or_else(|| widgets::color_for(&user.name));
    let (banner_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
    ui.painter().rect_filled(banner_rect, CornerRadius::same(6), banner);
    let avatar = Rect::from_min_size(pos2(banner_rect.left() + 4.0, banner_rect.bottom() - 40.0), vec2(80.0, 80.0));
    ui.allocate_space(vec2(84.0, 48.0));
    // O aro da cor do cartão separa o avatar da faixa, que muitas vezes tem a mesma cor dele.
    ui.painter().circle_filled(avatar.center(), 46.0, theme::MENU);
    widgets::paint_avatar(ui, avatar, &user.name, user.avatar_url.as_deref());
    widgets::paint_status(ui, avatar, user.status, true, theme::MENU);
    ui.add_space(4.0);
    ui.label(egui::RichText::new(&user.name).font(theme::bold(20.0)).color(theme::BRIGHT));
    ui.label(egui::RichText::new(format!("@{}", user.name)).font(theme::regular(14.0)).color(theme::SECONDARY));
    ui.add_space(8.0);

    let status_text = user.profile.custom_status.as_deref().filter(|value| !value.is_empty()).unwrap_or("Melhor emoji para descrever o seu dia?");
    let (custom_rect, custom_response) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    let custom_response = custom_response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if custom_response.hovered() { ui.painter().rect_filled(custom_rect, theme::RADIUS, theme::HOVER); }
    ui.painter().text(pos2(custom_rect.left() + 10.0, custom_rect.center().y), Align2::LEFT_CENTER, status_text, theme::regular(13.0), if user.profile.custom_status.is_some() { theme::TEXT } else { theme::MUTED });
    ui.painter().text(pos2(custom_rect.right() - 12.0, custom_rect.center().y), Align2::RIGHT_CENTER, "+", theme::bold(18.0), theme::TEXT);
    if custom_response.clicked() {
        app.actions.push(Action::Show(Modal::ProfileEditor(ProfileDraft::from_user(user))));
    }

    if let Some(activity) = &app.current_activity {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(format!("Jogando {}", activity.name))
                .font(theme::regular(13.0))
                .color(theme::ACCENT_TEXT),
        );
    }

    ui.add_space(8.0);
    Frame::new().fill(theme::FIELD).stroke(egui::Stroke::new(1.0, theme::BORDER)).corner_radius(theme::RADIUS).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
        if account_popup_row(ui, "Editar perfil", icons::PENCIL_SIMPLE, false).clicked() {
            app.actions.push(Action::Show(Modal::ProfileEditor(ProfileDraft::from_user(user))));
        }
        let (divider, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().hline(divider.x_range().shrink(8.0), divider.center().y, egui::Stroke::new(1.0, theme::BORDER));
        let status_response = account_status_trigger(ui, user.status);
        // O egui guarda um popup aberto só: um `Popup::menu` aqui dentro fecharia o cartão. O
        // submenu dele convive com o pai e abre no hover, como o do Discord.
        egui::containers::menu::SubMenu::new().show(ui, &status_response, |ui| {
            ui.set_width(320.0);
            status_menu(app, ui, user.status);
        });
    });
    ui.add_space(8.0);
    Frame::new().fill(theme::FIELD).stroke(egui::Stroke::new(1.0, theme::BORDER)).corner_radius(theme::RADIUS).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
        if account_popup_row(ui, "Copiar ID do usuário", icons::IDENTIFICATION_CARD, false).clicked() {
            app.actions.push(Action::CopyText(user.id.to_string()));
        }
    });
}

fn status_menu(app: &mut Unkvoid, ui: &mut Ui, current: UserStatus) {
    let online = status_option(ui, UserStatus::Online, current, "Disponível", None);
    if online.clicked() {
        app.actions.push(Action::SetUserStatus(UserStatus::Online, None));
        ui.close();
    }
    ui.add_space(4.0);
    let (separator, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(separator.x_range(), separator.center().y, egui::Stroke::new(1.0, theme::BORDER));
    ui.add_space(4.0);
    for (status, label, detail) in [
        (UserStatus::Idle, "Ausente", None),
        (UserStatus::Dnd, "Não perturbar", Some("Você não receberá notificação na área de trabalho")),
        (UserStatus::Invisible, "Invisível", Some("Você vai aparecer Off-line")),
    ] {
        let response = status_option(ui, status, current, label, detail);
        egui::containers::menu::SubMenu::new().show(ui, &response, |ui| {
            ui.set_width(190.0);

            for (label, minutes) in [("Por 15 minutos", Some(15)), ("Por 1 hora", Some(60)), ("Por 8 horas", Some(480)), ("Por 24 horas", Some(1440)), ("Por 3 dias", Some(4320)), ("Para sempre", None)] {
                if widgets::menu_item(ui, "", label, false) {
                    app.actions.push(Action::SetUserStatus(status, minutes));
                    ui.close();
                }
            }
        });
    }
}

fn status_option(ui: &mut Ui, status: UserStatus, current: UserStatus, label: &str, detail: Option<&str>) -> egui::Response {
    let height = if detail.is_some() { 48.0 } else { 36.0 };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() { ui.painter().rect_filled(rect, CornerRadius::same(4), theme::HOVER); }
    let color = status_color(status);
    ui.painter().circle_filled(pos2(rect.left() + 16.0, rect.center().y), 5.0, color);
    ui.painter().text(pos2(rect.left() + 30.0, rect.center().y - if detail.is_some() { 6.0 } else { 0.0 }), Align2::LEFT_CENTER, label, theme::regular(14.0), theme::TEXT);
    if let Some(detail) = detail { ui.painter().text(pos2(rect.left() + 30.0, rect.center().y + 10.0), Align2::LEFT_CENTER, detail, theme::regular(12.0), theme::DESCRIPTION); }
    if status == current { crate::ui::icons::text(ui.painter(), pos2(rect.right() - 12.0, rect.center().y), Align2::RIGHT_CENTER, icons::CHECK, theme::icon(16.0), theme::TEXT); }
    response
}

fn account_popup_row(ui: &mut Ui, label: &str, icon: &str, arrow: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() { ui.painter().rect_filled(rect, CornerRadius::same(4), theme::HOVER); }
    crate::ui::icons::text(ui.painter(), pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, icon, theme::icon(16.0), theme::TEXT);
    ui.painter().text(pos2(rect.left() + 34.0, rect.center().y), Align2::LEFT_CENTER, label, theme::regular(14.0), theme::TEXT);
    if arrow { ui.painter().text(pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, "›", theme::regular(18.0), theme::SECONDARY); }
    response
}

fn account_status_trigger(ui: &mut Ui, status: UserStatus) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() { ui.painter().rect_filled(rect, CornerRadius::same(4), theme::HOVER); }
    ui.painter().circle_filled(pos2(rect.left() + 18.0, rect.center().y), 5.0, status_color(status));
    ui.painter().text(pos2(rect.left() + 34.0, rect.center().y), Align2::LEFT_CENTER, status_label(status), theme::regular(14.0), theme::TEXT);
    ui.painter().text(pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, "›", theme::regular(18.0), theme::SECONDARY);
    response
}

fn status_label(status: UserStatus) -> &'static str {
    match status { UserStatus::Online => "Disponível", UserStatus::Idle => "Ausente", UserStatus::Dnd => "Não perturbar", UserStatus::Invisible | UserStatus::Offline => "Invisível" }
}

fn status_color(status: UserStatus) -> Color32 {
    match status { UserStatus::Online => theme::SUCCESS, UserStatus::Idle => theme::WARNING, UserStatus::Dnd => theme::DANGER, UserStatus::Invisible | UserStatus::Offline => theme::DIM }
}

/// O microfone e o fone da barra da chamada, no fluxo do layout: o mesmo par de botões do cartão.
/// `color` vermelho é desligado.
pub(super) fn split_button(
    ui: &mut Ui,
    id: &str,
    icon: &str,
    tip: &str,
    color: Color32,
) -> (egui::Response, egui::Response) {
    let (whole, _) = ui.allocate_exact_size(vec2(SPLIT_MAIN + SPLIT_CARET, 32.0), Sense::hover());

    split_at(
        ui,
        whole,
        id,
        icon,
        tip,
        color == theme::DANGER,
        if id == "deafen" {
            "Opções de saída"
        } else {
            "Opções de entrada"
        },
    )
}

/// Duas áreas de clique, como as duas `div` do Discord: o ícone, que liga e desliga, e a setinha,
/// que abre as opções. Cada uma acende sozinha no hover; desligado, as duas ficam avermelhadas com
/// um fio entre elas. A setinha vira para cima com as opções abertas.
fn split_at(
    ui: &mut Ui,
    whole: Rect,
    id: &str,
    icon: &str,
    tip: &str,
    off: bool,
    options: &str,
) -> (egui::Response, egui::Response) {
    let (main_rect, caret_rect) = whole.split_left_right_at_x(whole.left() + SPLIT_MAIN);
    let main = ui
        .interact(main_rect, ui.id().with((id, "main")), Sense::click())
        .tip(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let caret = ui
        .interact(caret_rect, ui.id().with((id, "caret")), Sense::click())
        .tip(options)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let open = ui
        .data(|data| data.get_temp::<bool>(caret.id))
        .unwrap_or(false);
    let left = CornerRadius {
        nw: 8,
        sw: 8,
        ne: 0,
        se: 0,
    };
    let right = CornerRadius {
        nw: 0,
        sw: 0,
        ne: 8,
        se: 8,
    };

    for (rect, corners, hovered) in [
        (main_rect, left, main.hovered()),
        (caret_rect, right, caret.hovered() || open),
    ] {
        let fill = match (off, hovered) {
            (true, true) => theme::OFF_HOVER,
            (true, false) => theme::OFF,
            (false, true) => theme::SELECTED,
            (false, false) => Color32::TRANSPARENT,
        };

        ui.painter().rect_filled(rect, corners, fill);
    }

    if off {
        ui.painter().vline(
            main_rect.right(),
            main_rect.y_range().shrink(6.0),
            egui::Stroke::new(1.0, Color32::from_rgb(40, 24, 29)),
        );
    }

    let color = match (off, main.hovered()) {
        (true, _) => theme::DANGER,
        (false, true) => theme::BRIGHT,
        (false, false) => theme::TEXT,
    };

    super::icons::paint_glyph(ui, main_rect.shrink(6.0), icon, color, 20.0);
    super::icons::paint_glyph(
        ui,
        caret_rect.shrink(5.0),
        if open {
            icons::CARET_UP
        } else {
            icons::CARET_DOWN
        },
        if off { theme::DANGER } else { theme::TEXT },
        16.0,
    );

    (main, caret)
}

/// As "Opções de entrada" e as "de saída" do Discord, abertas pela setinha: o aparelho num
/// submenu com o escolhido marcado, a supressão de ruído (no lugar do "Perfil de entrada") e o
/// atalho para as configurações de voz.
pub(super) fn device_menu(app: &mut Unkvoid, caret: &egui::Response, microphone: bool) {
    let field = if microphone { "microphone" } else { "speaker" };
    let devices = if microphone {
        app.microphones.clone()
    } else {
        app.speakers.clone()
    };
    let chosen = app.voice_settings[field]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let current = devices
        .iter()
        .find(|device| device.id == chosen)
        .map_or("Padrão do sistema", |device| device.label.as_str())
        .to_owned();
    let shown = egui::Popup::menu(caret)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::TOP_END)
        .gap(8.0)
        .frame(widgets::menu_frame())
        .show(|ui| {
            ui.set_width(250.0);
            // O menu do egui vem apertado: as linhas e os blocos com o respiro do Discord.
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);
            ui.spacing_mut().item_spacing.y = 4.0;

            ui.menu_button(
                two_lines(
                    if microphone {
                        "Dispositivo de entrada"
                    } else {
                        "Dispositivo de saída"
                    },
                    &current,
                ),
                |ui| {
                    ui.set_min_width(240.0);

                    if widgets::menu_item(
                        ui,
                        if chosen.is_empty() {
                            icons::RADIO_BUTTON
                        } else {
                            icons::CIRCLE
                        },
                        "Padrão do sistema",
                        false,
                    ) {
                        app.actions
                            .push(Action::SetVoice(field, serde_json::Value::Null));
                        ui.close();
                    }

                    for device in &devices {
                        if widgets::menu_item(
                            ui,
                            if device.id == chosen {
                                icons::RADIO_BUTTON
                            } else {
                                icons::CIRCLE
                            },
                            &device.label,
                            false,
                        ) {
                            app.actions
                                .push(Action::SetVoice(field, serde_json::json!(device.id)));
                            ui.close();
                        }
                    }
                },
            );

            if microphone {
                Frame::new()
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Nível de entrada")
                                .font(theme::bold(12.0))
                                .color(theme::MUTED),
                        );
                        ui.add_space(6.0);
                        super::settings::meter(ui, app.call_level, None);
                    });
            }

            let volume_field = if microphone {
                "inputVolume"
            } else {
                "outputVolume"
            };
            let volume = app.voice_settings[volume_field]
                .as_u64()
                .unwrap_or(100)
                .clamp(0, 200);

            Frame::new()
                .inner_margin(egui::Margin {
                    left: 10,
                    right: 10,
                    top: 6,
                    bottom: 10,
                })
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(if microphone {
                            "Volume de entrada"
                        } else {
                            "Volume de saída"
                        })
                        .font(theme::bold(12.0))
                        .color(theme::MUTED),
                    );
                    ui.add_space(8.0);

                    #[allow(clippy::cast_precision_loss)]
                    if let Some(chosen) =
                        widgets::held_slider(ui, volume_field, volume as f32, 0.0..=200.0, true)
                    {
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        app.actions.push(Action::SetVoice(
                            volume_field,
                            serde_json::json!(chosen.round() as u16),
                        ));
                    }
                });

            if microphone {
                let filtering = app.voice_settings["noise"].as_str() == Some("light");

                ui.menu_button(
                    two_lines(
                        "Supressão de ruído",
                        if filtering {
                            "Leve (RNNoise)"
                        } else {
                            "Nenhuma"
                        },
                    ),
                    |ui| {
                        ui.set_min_width(200.0);

                        for (value, label) in [("light", "Leve (RNNoise)"), ("off", "Nenhuma")] {
                            let selected = (value == "light") == filtering;

                            if widgets::menu_item(
                                ui,
                                if selected {
                                    icons::RADIO_BUTTON
                                } else {
                                    icons::CIRCLE
                                },
                                label,
                                false,
                            ) {
                                app.actions
                                    .push(Action::SetVoice("noise", serde_json::json!(value)));
                                ui.close();
                            }
                        }
                    },
                );
            }

            widgets::menu_separator(ui);

            if widgets::menu_item(ui, icons::GEAR_SIX, "Configurações de voz", false) {
                app.actions
                    .push(Action::Show(Modal::Settings(SettingsPage::Voice(crate::app::VoiceSection::Voice))));
                ui.close();
            }
        });

    if microphone && shown.is_some() {
        app.input_menu_seen = true;
    }

    caret
        .ctx
        .data_mut(|data| data.insert_temp(caret.id, shown.is_some()));
}

/// O título em negrito e, embaixo, o que está escolhido: os itens com submenu do Discord.
fn two_lines(title: &str, value: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();

    job.append(
        title,
        0.0,
        egui::TextFormat {
            font_id: theme::bold(14.0),
            color: theme::BRIGHT,
            ..Default::default()
        },
    );
    job.append(
        &format!("\n{value}"),
        0.0,
        egui::TextFormat {
            font_id: theme::regular(12.0),
            color: theme::SECONDARY,
            ..Default::default()
        },
    );

    job
}

/// Corta o texto com reticências quando ele não cabe na largura.
pub fn elide(ui: &Ui, text: &str, font: egui::FontId, width: f32) -> String {
    let fits = |candidate: &str| {
        ui.painter()
            .layout_no_wrap(candidate.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
            <= width
    };

    if fits(text) {
        return text.to_owned();
    }

    let mut letters: Vec<char> = text.chars().collect();

    while !letters.is_empty() {
        letters.pop();

        let candidate = format!("{}…", letters.iter().collect::<String>().trim_end());

        if fits(&candidate) {
            return candidate;
        }
    }

    "…".to_owned()
}

#[cfg(test)]
mod tests {
    use super::format_voice_duration;
    use std::time::Duration;

    #[test]
    fn voice_duration_uses_minutes_then_hours() {
        assert_eq!(format_voice_duration(Duration::from_secs(32)), "0:32");
        assert_eq!(format_voice_duration(Duration::from_secs(65)), "1:05");
        assert_eq!(format_voice_duration(Duration::from_secs(3_610)), "1:00:10");
    }
}
