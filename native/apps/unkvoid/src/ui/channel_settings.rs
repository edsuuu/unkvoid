//! Configurações de um canal ou de uma categoria, no molde do Discord.

use egui::{Align2, Color32, CornerRadius, RichText, Sense, Ui, pos2, vec2};
use egui_phosphor::regular as icons;
use serde_json::{Value, json};

use core_app::models::{Channel, ChannelKind, Member, Overwrite, OverwriteTarget, ServerTree};
use core_app::permissions::{
    CONNECT, CREATE_INVITE, DEAFEN_MEMBERS, MANAGE_CHANNELS, MANAGE_MESSAGES, MANAGE_ROLES,
    MOVE_MEMBERS, MUTE_MEMBERS, SEND_MESSAGES, SPEAK, STREAM, VIDEO, VIEW_CHANNEL,
};

use super::server_settings::{PERMISSIONS, link};
use super::{settings, theme, widgets};
use crate::app::{Action, ChannelPage, ChannelSettings, Confirm, Modal, OverwriteDraft, Unkvoid};
use crate::backend::Refresh;

pub fn show(app: &mut Unkvoid, ui: &mut Ui, mut state: ChannelSettings) {
    let Some(tree) = app.tree.clone().filter(|tree| tree.id == state.server) else {
        app.actions.push(Action::CloseModal);
        return;
    };
    let Some(channel) = tree
        .channels
        .iter()
        .find(|channel| channel.id == state.channel)
        .cloned()
    else {
        app.actions.push(Action::CloseModal);
        return;
    };
    let shown = state.clone();

    settings::window(
        app,
        ui,
        "channel-settings",
        None,
        |app, ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            navigation(app, ui, &tree, &channel, &shown)
        },
        |app, ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            match state.page {
            ChannelPage::Overview => overview(app, ui, &tree, &channel, &mut state),
            ChannelPage::Permissions => permissions(app, ui, &tree, &channel, &mut state),
            }
        },
    );

    if matches!(app.modal, Some(Modal::ChannelSettings(_))) {
        app.modal = Some(Modal::ChannelSettings(state));
    }
}

fn navigation(
    app: &mut Unkvoid,
    ui: &mut Ui,
    tree: &ServerTree,
    channel: &Channel,
    state: &ChannelSettings,
) {
    ui.add_space(6.0);
    ui.label(RichText::new(channel.name.to_uppercase()).font(theme::bold(12.0)).color(theme::MUTED));
    ui.label(
        RichText::new(format!(
            "{} {}",
            channel_icon(channel.kind),
            if channel.kind == ChannelKind::Category {
                "Categoria"
            } else {
                "Canal"
            }
        ))
        .font(theme::regular(13.0))
        .color(theme::MUTED),
    );
    ui.add_space(12.0);

    let go = |app: &mut Unkvoid, ui: &mut Ui, page: ChannelPage, _icon: &str, label: &str| {
        if channel_nav_item(ui, label, state.page == page, false) && state.page != page {
            app.actions
                .push(Action::Show(Modal::ChannelSettings(ChannelSettings {
                    page,
                    overwrite: None,
                    ..state.clone()
                })));
        }
    };
    go(app, ui, ChannelPage::Overview, icons::INFO, "Visão geral");

    if tree.can(MANAGE_ROLES) && core_app::permissions::has(channel.permissions, MANAGE_ROLES) {
        go(
            app,
            ui,
            ChannelPage::Permissions,
            icons::SHIELD,
            "Permissões",
        );
    }

    if tree.can(MANAGE_CHANNELS) {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);
        let label = if channel.kind == ChannelKind::Category {
            "Excluir categoria"
        } else {
            "Excluir canal"
        };
        if channel_nav_item(ui, label, false, true) {
            app.actions.push(Action::Ask(Confirm::DeleteChannel {
                server: state.server,
                channel: state.channel.clone(),
                name: channel.name.clone(),
            }));
        }
    }
}

fn channel_nav_item(ui: &mut Ui, label: &str, selected: bool, danger: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width().max(0.0), 34.0),
        Sense::click(),
    );
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(5),
            if selected { theme::SELECTED } else { theme::HOVER },
        );
    }
    ui.painter().text(
        rect.left_center() + vec2(10.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        theme::regular(14.0),
        if danger { theme::DANGER } else { theme::TEXT },
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn overview(
    app: &mut Unkvoid,
    ui: &mut Ui,
    tree: &ServerTree,
    channel: &Channel,
    state: &mut ChannelSettings,
) {
    ui.set_max_width(640.0);
    let editable = tree.can(MANAGE_CHANNELS)
        && core_app::permissions::has(channel.permissions, MANAGE_CHANNELS);
    settings::heading(
        ui,
        if channel.kind == ChannelKind::Category {
            "Visão geral da categoria"
        } else {
            "Visão geral do canal"
        },
    );
    ui.add_space(28.0);
    widgets::field_label(
        ui,
        "Nome do canal",
        state
            .name
            .trim()
            .is_empty()
            .then_some("O nome não pode ficar vazio."),
    );
    ui.add_space(6.0);
    ui.add_enabled_ui(editable, |ui| {
        widgets::text_field(ui, &mut state.name, "novo-canal", false)
    });

    match channel.kind {
        ChannelKind::Text => {
            ui.add_space(24.0);
            widgets::field_label(ui, "Tópico do canal", None);
            ui.add_space(6.0);
            egui::Frame::new()
                .fill(theme::RAIL)
                .corner_radius(CornerRadius::same(4))
                .inner_margin(egui::Margin::symmetric(10, 10))
                .show(ui, |ui| {
                    ui.add_enabled(
                        editable,
                        egui::TextEdit::multiline(&mut state.topic)
                            .hint_text("Sobre o que é este canal?")
                            .frame(egui::Frame::NONE)
                            .font(theme::regular(15.0))
                            .desired_width(f32::INFINITY)
                            .desired_rows(4),
                    );
                });
        }
        ChannelKind::Voice => {
            ui.add_space(24.0);
            widgets::field_label(ui, "Limite de usuários", None);
            ui.add_space(6.0);
            ui.add_enabled_ui(editable, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Slider::new(&mut state.user_limit, 0..=99).show_value(false));
                    ui.label(if state.user_limit == 0 {
                        "Sem limite".to_owned()
                    } else {
                        state.user_limit.to_string()
                    });
                });
            });
        }
        ChannelKind::Category => {}
    }

    let changed = state.name.trim() != channel.name
        || match channel.kind {
            ChannelKind::Text => state.topic != channel.topic.clone().unwrap_or_default(),
            ChannelKind::Voice => {
                state.user_limit
                    != channel
                        .user_limit
                        .and_then(|limit| u32::try_from(limit).ok())
                        .unwrap_or(0)
            }
            ChannelKind::Category => false,
        };

    if editable && changed && !state.name.trim().is_empty() {
        ui.add_space(20.0);
        match settings::unsaved(ui) {
            Some(true) => {
                let mut body = serde_json::Map::new();
                if state.name.trim() != channel.name {
                    body.insert(
                        "name".to_owned(),
                        Value::String(state.name.trim().to_owned()),
                    );
                }
                match channel.kind {
                    ChannelKind::Text
                        if state.topic != channel.topic.clone().unwrap_or_default() =>
                    {
                        body.insert(
                            "topic".to_owned(),
                            if state.topic.is_empty() {
                                Value::Null
                            } else {
                                Value::String(state.topic.clone())
                            },
                        );
                    }
                    ChannelKind::Voice
                        if state.user_limit
                            != channel
                                .user_limit
                                .and_then(|limit| u32::try_from(limit).ok())
                                .unwrap_or(0) =>
                    {
                        body.insert(
                            "user_limit".to_owned(),
                            if state.user_limit == 0 {
                                Value::Null
                            } else {
                                json!(state.user_limit)
                            },
                        );
                    }
                    _ => {}
                }
                app.actions.push(Action::request(
                    "updateChannel",
                    json!({ "channel": channel.id }),
                    Value::Object(body),
                    Refresh::Tree(state.server),
                ));
            }
            Some(false) => {
                state.name.clone_from(&channel.name);
                state
                    .topic
                    .clone_from(&channel.topic.clone().unwrap_or_default());
                state.user_limit = channel
                    .user_limit
                    .and_then(|limit| u32::try_from(limit).ok())
                    .unwrap_or(0);
            }
            None => {}
        }
    }
}

fn permissions(
    app: &mut Unkvoid,
    ui: &mut Ui,
    tree: &ServerTree,
    channel: &Channel,
    state: &mut ChannelSettings,
) {
    ui.set_max_width(760.0);
    settings::heading(ui, "Permissões");
    ui.add_space(8.0);
    ui.label(
        RichText::new("Escolha quem pode fazer o quê neste canal.")
            .font(theme::regular(13.0))
            .color(theme::MUTED),
    );
    ui.add_space(16.0);

    overwrite_picker(ui, tree, channel, state);
    ui.add_space(20.0);
    let Some(draft) = state.overwrite.as_mut() else {
        ui.label(
            RichText::new("Escolha um cargo ou membro para editar as permissões.")
                .color(theme::MUTED),
        );
        return;
    };

    let existing = channel
        .overwrites
        .iter()
        .any(|overwrite| matches_target(overwrite, draft.target, draft.id));
    let permissions = visible_permissions(channel.kind);
    for (group, entries) in PERMISSIONS {
        let rows: Vec<_> = entries
            .iter()
            .filter(|(flag, _, _)| permissions.contains(flag))
            .collect();
        if rows.is_empty() {
            continue;
        }
        ui.label(
            RichText::new(group.to_uppercase())
                .font(theme::bold(12.0))
                .color(theme::MUTED),
        );
        ui.add_space(6.0);
        for (flag, name, explanation) in rows {
            let choice = permission_choice(draft, *flag);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(*name).font(theme::bold(14.0)));
                    ui.label(
                        RichText::new(*explanation)
                            .font(theme::regular(12.0))
                            .color(theme::MUTED),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (selected, icon, color, value, tip) in [
                        (
                            choice == PermissionChoice::Allow,
                            icons::CHECK,
                            theme::SUCCESS,
                            PermissionChoice::Allow,
                            "Permitir",
                        ),
                        (
                            choice == PermissionChoice::Inherit,
                            icons::MINUS,
                            theme::MUTED,
                            PermissionChoice::Inherit,
                            "Herdar",
                        ),
                        (
                            choice == PermissionChoice::Deny,
                            icons::X,
                            theme::DANGER,
                            PermissionChoice::Deny,
                            "Negar",
                        ),
                    ] {
                        let response = ui.add_enabled(
                            tree.can(*flag),
                            egui::Button::new(crate::ui::icons::atom(
                                icon,
                                theme::icon(16.0),
                                if selected { Color32::WHITE } else { color },
                            ))
                            .fill(if selected { color } else { theme::RAIL })
                            .corner_radius(CornerRadius::same(4))
                            .min_size(vec2(30.0, 30.0)),
                        );
                        if response.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            apply_permission_choice(draft, *flag, value);
                        }
                    }
                });
            });
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);
        }
        ui.add_space(12.0);
    }

    if !tree.can(MANAGE_ROLES) || !core_app::permissions::has(channel.permissions, MANAGE_ROLES) {
        return;
    }
    let changed = channel
        .overwrites
        .iter()
        .find(|overwrite| matches_target(overwrite, draft.target, draft.id))
        .is_none_or(|overwrite| overwrite.allow != draft.allow || overwrite.deny != draft.deny);
    if changed {
        ui.add_space(12.0);
        match settings::unsaved(ui) {
            Some(true) => {
                let (overwrite_type, id) = target_params(draft.target, draft.id);
                app.actions.push(Action::request(
                    "putOverwrite",
                    json!({ "channel": channel.id, "type": overwrite_type, "id": id }),
                    json!({ "allow": draft.allow, "deny": draft.deny }),
                    Refresh::Tree(state.server),
                ));
            }
            Some(false) => {
                if let Some(overwrite) = channel
                    .overwrites
                    .iter()
                    .find(|overwrite| matches_target(overwrite, draft.target, draft.id))
                {
                    draft.allow = overwrite.allow;
                    draft.deny = overwrite.deny;
                } else {
                    draft.allow = 0;
                    draft.deny = 0;
                }
            }
            None => {}
        }
    }
    if existing {
        ui.add_space(8.0);
        if link(ui, "Remover sobrescrita", theme::DANGER) {
            let (overwrite_type, id) = target_params(draft.target, draft.id);
            app.actions.push(Action::request(
                "deleteOverwrite",
                json!({ "channel": channel.id, "type": overwrite_type, "id": id }),
                Value::Null,
                Refresh::Tree(state.server),
            ));
        }
    }
}

/// Como o Discord: o `@everyone` sempre, e depois só quem já tem sobrescrita no canal (ou acabou
/// de ser escolhido no "+"). O resto do servidor fica no menu do "+".
fn overwrite_picker(
    ui: &mut Ui,
    tree: &ServerTree,
    channel: &Channel,
    state: &mut ChannelSettings,
) {
    let mut selected = None;
    let chosen = |target: OverwriteTarget, id: i64| {
        state
            .overwrite
            .is_some_and(|draft| draft.target == target && draft.id == id)
    };
    let listed =
        |target: OverwriteTarget, id: i64| has_overwrite(channel, target, id) || chosen(target, id);

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        if let Some(everyone) = tree.roles.iter().find(|role| role.is_everyone)
            && target_button(
                ui,
                "@everyone",
                None,
                chosen(OverwriteTarget::Role, everyone.id),
            )
        {
            selected = Some((OverwriteTarget::Role, everyone.id));
        }
        let mut roles: Vec<_> = tree
            .roles
            .iter()
            .filter(|role| !role.is_everyone && listed(OverwriteTarget::Role, role.id))
            .collect();
        roles.sort_by_key(|role| std::cmp::Reverse(role.position));
        for role in roles {
            if target_button(
                ui,
                &role.name,
                role.color.as_deref(),
                chosen(OverwriteTarget::Role, role.id),
            ) {
                selected = Some((OverwriteTarget::Role, role.id));
            }
        }
        for member in tree
            .members
            .iter()
            .filter(|member| listed(OverwriteTarget::Member, member.user_id))
        {
            if target_member_button(ui, member, chosen(OverwriteTarget::Member, member.user_id)) {
                selected = Some((OverwriteTarget::Member, member.user_id));
            }
        }
        let menu = ui.menu_button(crate::ui::icons::atom(icons::PLUS, theme::icon(18.0), theme::TEXT), |ui| {
            ui.set_min_width(220.0);
            for role in tree
                .roles
                .iter()
                .filter(|role| !role.is_everyone && !listed(OverwriteTarget::Role, role.id))
            {
                if channel_menu_item(ui, &role.name, false) {
                    selected = Some((OverwriteTarget::Role, role.id));
                    ui.close();
                }
            }
            for member in tree
                .members
                .iter()
                .filter(|member| !listed(OverwriteTarget::Member, member.user_id))
            {
                if channel_menu_item(ui, member_name(member), false) {
                    selected = Some((OverwriteTarget::Member, member.user_id));
                    ui.close();
                }
            }
        });
        menu.response.on_hover_cursor(egui::CursorIcon::PointingHand);
    });
    if let Some((target, id)) = selected {
        let existing = channel
            .overwrites
            .iter()
            .find(|overwrite| matches_target(overwrite, target, id));
        state.overwrite = Some(OverwriteDraft {
            target,
            id,
            allow: existing.map_or(0, |overwrite| overwrite.allow),
            deny: existing.map_or(0, |overwrite| overwrite.deny),
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermissionChoice {
    Deny,
    Inherit,
    Allow,
}

fn permission_choice(draft: &OverwriteDraft, flag: i64) -> PermissionChoice {
    if draft.allow & flag != 0 {
        PermissionChoice::Allow
    } else if draft.deny & flag != 0 {
        PermissionChoice::Deny
    } else {
        PermissionChoice::Inherit
    }
}

fn apply_permission_choice(draft: &mut OverwriteDraft, flag: i64, choice: PermissionChoice) {
    draft.allow &= !flag;
    draft.deny &= !flag;
    match choice {
        PermissionChoice::Deny => draft.deny |= flag,
        PermissionChoice::Inherit => {}
        PermissionChoice::Allow => draft.allow |= flag,
    }
}

fn visible_permissions(kind: ChannelKind) -> Vec<i64> {
    let mut flags = vec![VIEW_CHANNEL, MANAGE_CHANNELS, MANAGE_ROLES, CREATE_INVITE];
    if matches!(kind, ChannelKind::Text | ChannelKind::Category) {
        flags.extend([SEND_MESSAGES, MANAGE_MESSAGES]);
    }
    if matches!(kind, ChannelKind::Voice | ChannelKind::Category) {
        flags.extend([
            CONNECT,
            SPEAK,
            VIDEO,
            STREAM,
            MUTE_MEMBERS,
            DEAFEN_MEMBERS,
            MOVE_MEMBERS,
        ]);
        if kind == ChannelKind::Voice {
            flags.extend([SEND_MESSAGES, MANAGE_MESSAGES]);
        }
    }
    flags
}

fn has_overwrite(channel: &Channel, target: OverwriteTarget, id: i64) -> bool {
    channel
        .overwrites
        .iter()
        .any(|overwrite| matches_target(overwrite, target, id))
}

fn matches_target(overwrite: &Overwrite, target: OverwriteTarget, id: i64) -> bool {
    overwrite.target_type == target && overwrite.target_id == id
}

fn target_params(target: OverwriteTarget, id: i64) -> (&'static str, i64) {
    (
        if target == OverwriteTarget::Role {
            "role"
        } else {
            "member"
        },
        id,
    )
}

fn target_button(ui: &mut Ui, name: &str, color: Option<&str>, selected: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(120.0, 36.0), Sense::click());
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(6),
            if selected {
                theme::SELECTED
            } else {
                theme::HOVER
            },
        );
    }
    let tint = color.and_then(theme::color_of).unwrap_or(theme::MUTED);
    ui.painter()
        .circle_filled(pos2(rect.left() + 14.0, rect.center().y), 5.0, tint);
    ui.painter().text(
        pos2(rect.left() + 26.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        theme::regular(13.0),
        theme::TEXT,
    );
    response.on_hover_text(name).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

fn target_member_button(ui: &mut Ui, member: &Member, selected: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(144.0, 36.0), Sense::click());
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(6),
            if selected {
                theme::SELECTED
            } else {
                theme::HOVER
            },
        );
    }
    widgets::paint_avatar(
        ui,
        egui::Rect::from_center_size(pos2(rect.left() + 17.0, rect.center().y), vec2(24.0, 24.0)),
        member_name(member),
        member.avatar_url.as_deref(),
    );
    ui.painter().text(
        pos2(rect.left() + 34.0, rect.center().y),
        Align2::LEFT_CENTER,
        member_name(member),
        theme::regular(13.0),
        theme::TEXT,
    );
    response.on_hover_text(member_name(member)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

fn channel_menu_item(ui: &mut Ui, label: &str, danger: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width().max(0.0), 34.0),
        Sense::click(),
    );
    if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(4), theme::HOVER);
    }
    ui.painter().text(
        rect.left_center() + vec2(10.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        theme::regular(14.0),
        if danger { theme::DANGER } else { theme::TEXT },
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn member_name(member: &Member) -> &str {
    member.nickname.as_deref().unwrap_or(&member.name)
}

fn channel_icon(kind: ChannelKind) -> &'static str {
    match kind {
        ChannelKind::Text => icons::HASH,
        ChannelKind::Voice => icons::SPEAKER_HIGH,
        ChannelKind::Category => icons::FOLDER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_choices_replace_only_the_selected_bit() {
        let mut draft = OverwriteDraft {
            target: OverwriteTarget::Role,
            id: 4,
            allow: SPEAK,
            deny: CONNECT,
        };

        apply_permission_choice(&mut draft, CONNECT, PermissionChoice::Inherit);
        assert_eq!(draft.allow, SPEAK);
        assert_eq!(draft.deny, 0);

        apply_permission_choice(&mut draft, CONNECT, PermissionChoice::Allow);
        assert_eq!(draft.allow, SPEAK | CONNECT);
        assert_eq!(draft.deny, 0);

        apply_permission_choice(&mut draft, CONNECT, PermissionChoice::Deny);
        assert_eq!(draft.allow, SPEAK);
        assert_eq!(draft.deny, CONNECT);
    }
}
