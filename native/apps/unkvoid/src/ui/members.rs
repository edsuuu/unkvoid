//! A lista de membros à direita: online agrupados pelo cargo mais alto (com a cor dele), e os
//! offline no fim, apagados — os grupos do Discord, decididos pelo núcleo.

use egui::{Align2, Color32, CornerRadius, Frame, Rect, RichText, Sense, Ui, pos2, vec2};
use egui_phosphor::regular as icons;
use serde_json::{Value, json};

use chrono::Datelike;
use core_app::members::{self, OFFLINE_KEY};
use core_app::models::{ChannelKind, Member, Person, ServerTree};
use core_app::permissions;
use core_app::permissions::{CONNECT, MOVE_MEMBERS, SEND_MESSAGES};

use super::{server_settings, theme, widgets};
use crate::app::{Action, Confirm, Unkvoid};
use crate::backend::Refresh;

pub(super) fn contents(app: &mut Unkvoid, ui: &mut Ui) {
    let Some(tree) = app.tree.clone() else {
        loading(ui);
        return;
    };
    ui.set_width(theme::MEMBERS_WIDTH);
    ui.scope(|ui| {
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for group in members::group(&tree, &app.online) {
                    let offline = group.key == OFFLINE_KEY;

                    // O grupo como o Discord de hoje escreve: "Online — 3", sem caixa alta.
                    ui.add_space(20.0);
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "{} — {}",
                                group.label,
                                group.members.len()
                            ))
                            .font(theme::regular(14.0))
                            .color(theme::MUTED),
                        );
                    });
                    ui.add_space(6.0);

                    for member in &group.members {
                        let name = members::display_name(member);
                        let color = if offline {
                            None
                        } else {
                            members::top_role(&tree, member)
                                .and_then(|role| role.color.as_deref())
                                .and_then(theme::color_of)
                        };
                        let (outer, _) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::hover());
                        let rect = outer.shrink2(vec2(8.0, 1.0));
                        let response = ui.interact(
                            rect,
                            ui.id().with(("member", member.user_id)),
                            Sense::click(),
                        );

                        // Id próprio: o do clique direito é o padrão da resposta, e o
                        // `context_menu` fecha o seu id a cada clique esquerdo.
                        let card = response.id.with("profile-card");
                        let card_open = egui::Popup::is_id_open(ui.ctx(), card);

                        if card_open {
                            ui.painter().rect_filled(rect, theme::RADIUS, theme::SELECTED);
                        } else if response.hovered() {
                            ui.painter().rect_filled(rect, theme::RADIUS, theme::HOVER);
                        }

                        let avatar = Rect::from_center_size(
                            pos2(rect.left() + 25.0, rect.center().y),
                            vec2(32.0, 32.0),
                        );
                        let alpha = if offline && !response.hovered() {
                            0.3
                        } else {
                            1.0
                        };

                        ui.scope(|ui| {
                            ui.set_opacity(alpha);
                            widgets::paint_avatar(ui, avatar, name, member.avatar_url.as_deref());
                        });

                        widgets::paint_status(
                            ui,
                            avatar,
                            member.status,
                            !offline,
                            if response.hovered() { theme::HOVER } else { theme::MAIN },
                        );

                        let text = color
                            .unwrap_or(if response.hovered() {
                                theme::BRIGHT
                            } else {
                                theme::TEXT
                            })
                            .gamma_multiply(alpha);

                        ui.painter().text(
                            pos2(avatar.right() + 12.0, rect.center().y - 8.0),
                            Align2::LEFT_CENTER,
                            name,
                            theme::bold(16.0),
                            text,
                        );

                        if let Some(activity) = &member.activity {
                            ui.painter().text(
                                pos2(avatar.right() + 12.0, rect.center().y + 10.0),
                                Align2::LEFT_CENTER,
                                format!("Jogando {}", activity.name),
                                theme::regular(12.0),
                                theme::MUTED.gamma_multiply(alpha),
                            );
                        }

                        if member.is_owner || member.user_id == tree.owner_id {
                            let galley = ui.painter().layout_no_wrap(
                                name.to_owned(),
                                theme::bold(16.0),
                                text,
                            );
                            let crown =
                                pos2(avatar.right() + 18.0 + galley.size().x, rect.center().y);

                            super::icons::paint_glyph(ui, Rect::from_center_size(pos2(crown.x + 7.0, crown.y), vec2(14.0, 14.0)), icons::CROWN_SIMPLE, theme::WARNING, 14.0);
                        }

                        // O cartão fica aberto enquanto se mexe nele (escrever a mensagem, o
                        // "…"): fecha só com clique fora ou Esc, como no Discord.
                        egui::Popup::menu(&response)
                            .id(card)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .align(egui::RectAlign::LEFT_START)
                            .gap(12.0)
                            .frame(card_frame())
                            .show(|ui| profile_card(app, ui, &tree, member, false));
                        response.context_menu(|ui| member_menu(app, ui, &tree, member));
                    }
                }

                ui.add_space(16.0);
            });
    });
}

fn loading(ui: &mut Ui) {
    ui.set_width(theme::MEMBERS_WIDTH);
    let time = ui.input(|input| input.time as f32);
    let offset = (time * 0.55).rem_euclid(1.0);
    for row_index in 0..12 {
        ui.add_space(if row_index % 4 == 0 { 20.0 } else { 6.0 });
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::hover());
        let avatar = Rect::from_center_size(pos2(row.left() + 24.0, row.center().y), vec2(30.0, 30.0));
        let name = Rect::from_min_size(pos2(row.left() + 48.0, row.center().y - 6.0), vec2(row.width() * 0.45, 12.0));
        let color = Color32::from_gray(53);
        ui.painter().circle_filled(avatar.center(), 15.0, color);
        ui.painter().rect_filled(name, CornerRadius::same(6), color);
        let shine_x = ui.min_rect().left() + (ui.available_width() + 160.0) * offset - 80.0;
        let shine = egui::Rect::from_min_max(pos2(shine_x, row.top()), pos2(shine_x + 80.0, row.bottom()));
        let clipped = ui.painter().with_clip_rect(row);
        clipped.rect_filled(avatar, CornerRadius::same(15), Color32::from_white_alpha(12));
        clipped.rect_filled(name, CornerRadius::same(6), Color32::from_white_alpha(12));
        clipped.rect_filled(shine, CornerRadius::same(8), Color32::from_white_alpha(10));
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(80));
}

/// O menu de um membro: o mesmo do clique direito na lista e do "⋮" das configurações. Cada item
/// só aparece se a árvore (`member_actions`) deixa; o Laravel confere de novo.
pub(super) fn member_menu(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree, member: &Member) {
    ui.set_min_width(220.0);

    let me = member.user_id == tree.me.user_id;
    let actions = tree.member_actions(member);
    let name = members::display_name(member).to_owned();
    let user = json!({ "server": tree.id, "user": member.user_id });

    let speaking_in = tree
        .voice
        .iter()
        .find(|(_, people)| people.iter().any(|person| person.user_id == member.user_id))
        .map(|(channel, _)| channel.clone());

    if widgets::menu_item(ui, "", "Perfil", false) {
        app.actions
            .push(Action::Show(crate::app::Modal::MemberProfile {
                server: tree.id,
                user: member.user_id,
            }));
        ui.close();
    }

    let can_mention = app.opened == Some(tree.id)
        && app.reading.as_ref().is_some_and(|channel| {
            tree.can(SEND_MESSAGES) && permissions::has(channel.permissions, SEND_MESSAGES)
        });
    if can_mention && widgets::menu_item(ui, "", "Mencionar", false) {
        app.actions.push(Action::Mention(name.clone()));
        ui.close();
    }

    if !me && widgets::menu_item(ui, "", "Mensagem", false) {
        app.actions.push(Action::OpenDirect(person(member)));
        ui.close();
    }

    if me {
        widgets::menu_separator(ui);
        if widgets::menu_check(ui, "Silenciar", app.call.muted, false) {
            app.actions.push(Action::ToggleMute);
        }
        if widgets::menu_check(ui, "Desativar áudio", app.call.deafened, false) {
            app.actions.push(Action::ToggleDeafen);
        }
    } else if speaking_in.is_some() {
        widgets::menu_separator(ui);
        let volume = crate::voice::peer_volume(&app.voice_settings, member.user_id);
        ui.menu_button(
            RichText::new(format!("Volume do usuário  {volume}%")).font(theme::bold(14.0)),
            |ui| {
                ui.set_min_width(220.0);
                if let Some(value) = widgets::held_slider(
                    ui,
                    &format!("user-volume-{}", member.user_id),
                    f32::from(volume),
                    0.0..=200.0,
                    false,
                ) {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    app.actions
                        .push(Action::SetPeerVolume(member.user_id, value.round() as u16));
                }
            },
        );
        if widgets::menu_check(ui, "Silenciar", volume == 0, false) {
            app.actions.push(Action::TogglePeerMute(member.user_id));
        }
    }

    if actions.nickname
        && widgets::menu_item(
            ui,
            "",
            if me {
                "Editar perfil por servidor"
            } else {
                "Alterar apelido"
            },
            false,
        )
    {
        app.actions.push(Action::Ask(Confirm::Nickname {
            server: tree.id,
            user: member.user_id,
            name: member.name.clone(),
            value: member.nickname.clone().unwrap_or_default(),
        }));
        ui.close();
    }

    let rows = tree.role_rows();
    let assignable: Vec<_> = tree
        .roles
        .iter()
        .filter(|role| rows.iter().any(|row| row.id == role.id && row.assignable))
        .collect();

    if actions.roles && !assignable.is_empty() {
        ui.menu_button(RichText::new("Cargos").font(theme::bold(14.0)), |ui| {
            ui.set_min_width(200.0);

            for role in assignable {
                let has = member.role_ids.contains(&role.id);

                if widgets::menu_check(ui, &role.name, has, false) {
                    let role_ids: Vec<i64> = if has {
                        member
                            .role_ids
                            .iter()
                            .copied()
                            .filter(|id| *id != role.id)
                            .collect()
                    } else {
                        member.role_ids.iter().copied().chain([role.id]).collect()
                    };

                    app.actions.push(server_settings::set_roles(
                        tree.id,
                        member.user_id,
                        &role_ids,
                    ));
                }
            }
        });
    }

    if !me
        && let Some(friendship) = app.friends.iter().find(|friendship| {
            friendship.status == core_app::models::FriendshipStatus::Accepted
                && ((friendship.requester.id == member.user_id
                    && friendship.addressee.id == tree.me.user_id)
                    || (friendship.addressee.id == member.user_id
                        && friendship.requester.id == tree.me.user_id))
        })
        && widgets::menu_item(ui, "", "Desfazer amizade", true) {
            app.actions.push(Action::request(
                "removeFriend",
                json!({ "friendship": friendship.id }),
                Value::Null,
                crate::backend::Refresh::Friends,
            ));
            ui.close();
        }

    if let Some(channel) = speaking_in {
        widgets::menu_separator(ui);
        let targets: Vec<_> = tree
            .channels
            .iter()
            .filter(|target| target.kind == ChannelKind::Voice && target.id != channel)
            .collect();
        if !targets.is_empty()
            && ((me && tree.can(CONNECT)) || (!me && actions.disconnect && tree.can(MOVE_MEMBERS)))
        {
            ui.menu_button(RichText::new("Mover para").font(theme::bold(14.0)), |ui| {
                ui.set_min_width(200.0);
                for target in &targets {
                    if me && !permissions::has(target.permissions, CONNECT) {
                        continue;
                    }
                    if widgets::menu_item(ui, icons::CARET_RIGHT, &target.name, false) {
                        if me {
                            if let Some(channel) =
                                tree.channels.iter().find(|channel| channel.id == target.id)
                            {
                                app.actions.push(Action::JoinVoice(channel.clone()));
                            }
                        } else {
                            app.actions.push(Action::request(
                                "moveVoiceMember",
                                json!({ "channel": channel, "user": member.user_id }),
                                json!({ "channel_id": target.id }),
                                Refresh::Tree(tree.id),
                            ));
                        }
                        ui.close();
                    }
                }
            });
        }
        if !me
            && actions.mute
            && widgets::menu_check(ui, "Silenciar voz no servidor", member.server_mute, true)
        {
            app.actions.push(Action::request(
                "updateMember",
                user.clone(),
                json!({ "server_mute": !member.server_mute }),
                Refresh::Tree(tree.id),
            ));
        }

        if !me && actions.disconnect && widgets::menu_item(ui, "", "Desconectar", true) {
            app.actions.push(Action::request(
                "disconnectFromVoice",
                json!({ "channel": channel, "user": member.user_id }),
                Value::Null,
                Refresh::Tree(tree.id),
            ));
            ui.close();
        }
    }

    if !me && (actions.kick || actions.ban) {
        widgets::menu_separator(ui);

        if actions.kick && widgets::menu_item(ui, "", &format!("Expulsar {name}"), true) {
            app.actions.push(Action::Ask(Confirm::Kick {
                server: tree.id,
                user: member.user_id,
                name: name.clone(),
            }));
            ui.close();
        }

        if actions.ban && widgets::menu_item(ui, "", &format!("Banir {name}"), true) {
            app.actions.push(Action::Ask(Confirm::Ban {
                server: tree.id,
                user: member.user_id,
                name: name.clone(),
                reason: String::new(),
            }));
            ui.close();
        }
    }

    widgets::menu_separator(ui);

    if widgets::menu_item(
        ui,
        icons::IDENTIFICATION_CARD,
        "Copiar ID do usuário",
        false,
    ) {
        app.actions
            .push(Action::CopyText(member.user_id.to_string()));
        ui.close();
    }
}

fn person(member: &Member) -> Person {
    Person {
        id: member.user_id,
        name: member.name.clone(),
        avatar_url: member.avatar_url.clone(),
        status: member.status,
        activity: member.activity.clone(),
        created_at: member.member_since.clone(),
        banner_color: member.profile.banner_color.clone(),
    }
}

/// O cartão sem margem: a faixa de cor encosta nas bordas, como no Discord.
pub(super) fn card_frame() -> Frame {
    widgets::menu_frame().inner_margin(egui::Margin::ZERO)
}

/// O cartão de perfil, como o do Discord de hoje: a faixa de 104 px na cor do perfil com os
/// botões redondos por cima, o avatar de 80 com o aro da cor do cartão e a bolinha do status, o
/// nome, o usuário, os cargos com o "+ Adicionar cargo" e a caixa de mensagem direta. `full` é o
/// perfil completo (o "Ver perfil"), que mostra também a bio e as datas.
pub(super) fn profile_card(
    app: &mut Unkvoid,
    ui: &mut Ui,
    tree: &ServerTree,
    member: &Member,
    full: bool,
) {
    const WIDTH: f32 = 300.0;
    const BANNER: f32 = 104.0;

    let name = members::display_name(member);
    let mine = member.user_id == tree.me.user_id;
    let profile = if mine {
        app.me
            .as_ref()
            .map(|user| user.profile.clone())
            .unwrap_or_else(|| member.profile.clone())
    } else {
        member.profile.clone()
    };
    let (banner, _) = ui.allocate_exact_size(vec2(WIDTH, BANNER), Sense::hover());

    ui.painter().rect_filled(
        banner,
        CornerRadius {
            nw: 8,
            ne: 8,
            sw: 0,
            se: 0,
        },
        profile
            .banner_color
            .as_deref()
            .and_then(theme::color_of)
            .unwrap_or_else(|| widgets::color_for(name)),
    );

    if !mine {
        let can_mute = tree.member_actions(member).mute
            && tree
                .voice
                .values()
                .any(|people| people.iter().any(|person| person.user_id == member.user_id));
        let mut center = pos2(banner.right() - 28.0, banner.top() + 24.0);
        let more = profile_action(ui, center, icons::DOTS_THREE, "Mais opções");

        egui::containers::menu::SubMenu::new()
            .show(ui, &more, |ui| member_menu(app, ui, tree, member));

        if can_mute {
            center.x -= 40.0;

            let mute = profile_action(
                ui,
                center,
                if member.server_mute {
                    icons::MICROPHONE
                } else {
                    icons::MICROPHONE_SLASH
                },
                if member.server_mute {
                    "Desativar silêncio no servidor"
                } else {
                    "Silenciar no servidor"
                },
            );

            if mute.clicked() {
                app.actions.push(Action::request(
                    "updateMember",
                    json!({ "server": tree.id, "user": member.user_id }),
                    json!({ "server_mute": !member.server_mute }),
                    Refresh::Tree(tree.id),
                ));
            }
        }
    }

    let avatar = Rect::from_center_size(
        pos2(banner.left() + 56.0, banner.bottom() - 6.0),
        vec2(80.0, 80.0),
    );
    let online = mine || app.online.contains(&member.user_id);

    ui.painter()
        .circle_filled(avatar.center(), 46.0, theme::MENU);
    widgets::paint_avatar(ui, avatar, name, member.avatar_url.as_deref());

    let dot = avatar.right_bottom() - vec2(12.0, 12.0);

    ui.painter().circle_filled(dot, 12.0, theme::MENU);
    ui.painter()
        .circle_filled(dot, 8.0, if online { theme::SUCCESS } else { theme::DIM });

    if !online {
        ui.painter().circle_filled(dot, 3.5, theme::MENU);
    }

    if let Some(status) = profile
        .custom_status
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        let galley =
            ui.painter()
                .layout_no_wrap(status.to_owned(), theme::regular(12.0), theme::TEXT);
        let width = (galley.size().x + 18.0).min(WIDTH - avatar.right() + banner.left() - 16.0);
        let bubble = Rect::from_min_size(
            pos2(avatar.right() + 4.0, avatar.center().y + 4.0),
            vec2(width.max(0.0), 25.0),
        );

        ui.painter()
            .rect_filled(bubble, CornerRadius::same(12), theme::ELEVATED);
        ui.painter().rect_stroke(
            bubble,
            CornerRadius::same(12),
            egui::Stroke::new(1.0, theme::MENU_BORDER),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            bubble.center(),
            Align2::CENTER_CENTER,
            status,
            theme::regular(12.0),
            theme::TEXT,
        );
    }

    Frame::new()
        .inner_margin(egui::Margin {
            left: 16,
            right: 16,
            top: 46,
            bottom: 16,
        })
        .show(ui, |ui| {
            ui.set_width(WIDTH - 32.0);
            ui.label(
                RichText::new(name)
                    .font(theme::bold(20.0))
                    .color(theme::BRIGHT),
            );
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&member.name)
                        .font(theme::regular(14.0))
                        .color(theme::TEXT),
                );

                if let Some(pronouns) = profile
                    .pronouns
                    .as_deref()
                    .filter(|value| !value.is_empty())
                {
                    ui.label(
                        RichText::new(format!("• {pronouns}"))
                            .font(theme::regular(14.0))
                            .color(theme::SECONDARY),
                    );
                }

                if member.is_owner || member.user_id == tree.owner_id {
                    crate::ui::icons::label(ui, icons::CROWN_SIMPLE, theme::icon(16.0), theme::WARNING)
                    .on_hover_text("Dono do servidor");
                }
            });

            if let Some(activity) = &member.activity {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("Jogando {}", activity.name))
                        .font(theme::regular(13.0))
                        .color(theme::ACCENT_TEXT),
                );
            }

            if let Some(bio) = profile.bio.as_deref().filter(|value| !value.is_empty()) {
                ui.add_space(12.0);
                ui.label(
                    RichText::new(bio)
                        .font(theme::regular(14.0))
                        .color(theme::TEXT),
                );
            }

            if full {
                for (label, value) in [
                    ("Membro desde", member.member_since.as_deref()),
                    ("Entrou no servidor", member.joined_at.as_deref()),
                ] {
                    if let Some(date) = value.and_then(profile_date) {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(label)
                                .font(theme::bold(12.0))
                                .color(theme::SECONDARY),
                        );
                        ui.label(
                            RichText::new(date)
                                .font(theme::regular(14.0))
                                .color(theme::TEXT),
                        );
                    }
                }
            }

            ui.add_space(16.0);

            if !member.role_ids.is_empty() {
                server_settings::role_chips(app, ui, tree, member);
                ui.add_space(8.0);
            }

            add_role(app, ui, tree, member);

            if mine {
                ui.add_space(8.0);

                let button = egui::Button::new(
                    RichText::new(format!("{}  Editar perfil", icons::PENCIL_SIMPLE))
                        .font(theme::bold(14.0))
                        .color(Color32::WHITE),
                )
                .fill(theme::ACCENT)
                .corner_radius(theme::RADIUS)
                .min_size(vec2(ui.available_width(), 40.0));

                if ui.add(button).clicked() {
                    app.actions.push(Action::Show(crate::app::Modal::Settings(
                        crate::app::SettingsPage::Account,
                    )));
                    ui.close();
                }

                return;
            }

            ui.add_space(8.0);
            direct_message(app, ui, member);
        });
}

/// O "+ Adicionar cargo" do cartão, para quem pode dar cargos e ainda tem um para dar.
fn add_role(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree, member: &Member) {
    let assignable: Vec<_> = tree
        .roles
        .iter()
        .filter(|role| {
            !role.is_everyone
                && !member.role_ids.contains(&role.id)
                && tree
                    .role_rows()
                    .iter()
                    .any(|row| row.id == role.id && row.assignable)
        })
        .collect();

    if member.user_id == tree.me.user_id
        || !tree.member_actions(member).roles
        || assignable.is_empty()
    {
        return;
    }

    let (rect, response) = ui.allocate_exact_size(vec2(130.0, 24.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let tint = if response.hovered() {
        theme::BRIGHT
    } else {
        theme::SECONDARY
    };

    super::icons::paint_glyph(ui, Rect::from_center_size(pos2(rect.left() + 8.0, rect.center().y), vec2(16.0, 16.0)), icons::PLUS, tint, 16.0);
    ui.painter().text(
        pos2(rect.left() + 20.0, rect.center().y),
        Align2::LEFT_CENTER,
        "Adicionar cargo",
        theme::regular(13.0),
        tint,
    );

    let roles = |ui: &mut Ui| {
        for role in &assignable {
            if widgets::menu_item(ui, icons::SHIELD_CHEVRON, &role.name, false) {
                let mut role_ids = member.role_ids.clone();

                role_ids.push(role.id);
                app.actions.push(server_settings::set_roles(
                    tree.id,
                    member.user_id,
                    &role_ids,
                ));
                ui.close();
            }
        }
    };

    // No cartão, que já é um popup, um `Popup::menu` fecharia o cartão: o egui guarda um aberto
    // só. O submenu dele convive com o pai; no perfil completo, que é um modal, o popup serve.
    if egui::containers::menu::is_in_menu(ui) {
        egui::containers::menu::SubMenu::new().show(ui, &response, roles);
    } else {
        egui::Popup::menu(&response).frame(widgets::menu_frame()).show(roles);
    }
}

/// A mensagem direta escrita no próprio cartão, como no Discord: a caixa de 44 px com a borda e
/// o emoji, e Enter manda.
fn direct_message(app: &mut Unkvoid, ui: &mut Ui, member: &Member) {
    let draft_id = ui.id().with(("direct-draft", member.user_id));
    let mut draft = ui
        .data_mut(|data| data.get_temp::<String>(draft_id))
        .unwrap_or_default();
    let field = Frame::new()
        .fill(theme::ELEVATED)
        .stroke(egui::Stroke::new(1.0, theme::MENU_BORDER))
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let width = ui.available_width();
                let field = ui.add_sized(
                    vec2((width - 28.0).max(0.0), 22.0),
                    egui::TextEdit::singleline(&mut draft)
                        .hint_text(
                            RichText::new(format!("Conversar com @{}", member.name))
                                .font(theme::regular(14.0))
                                .color(theme::PLACEHOLDER),
                        )
                        .frame(egui::Frame::NONE)
                        .font(theme::regular(14.0)),
                );

                crate::ui::icons::label(ui, icons::SMILEY, theme::icon(20.0), theme::SECONDARY);

                field
            })
            .inner
        })
        .inner;

    if field.lost_focus()
        && ui.input(|input| input.key_pressed(egui::Key::Enter))
        && !draft.trim().is_empty()
    {
        app.actions.push(Action::SendDirectTo(
            person(member),
            draft.trim().to_owned(),
        ));
        draft.clear();
        ui.close();
    }

    ui.data_mut(|data| data.insert_temp(draft_id, draft));
}

pub(super) fn profile_modal(app: &mut Unkvoid, ui: &mut Ui, server: i64, user: i64) {
    let Some(tree) = app.tree.clone().filter(|tree| tree.id == server) else {
        app.actions.push(Action::CloseModal);
        return;
    };
    let Some(member) = tree
        .members
        .iter()
        .find(|member| member.user_id == user)
        .cloned()
    else {
        app.actions.push(Action::CloseModal);
        return;
    };
    let response = egui::Modal::new(egui::Id::new("member-profile"))
        .frame(card_frame())
        .show(ui.ctx(), |ui| profile_card(app, ui, &tree, &member, true));
    if response.should_close() {
        app.actions.push(Action::CloseModal);
    }
}

fn profile_action(ui: &mut Ui, center: egui::Pos2, icon: &str, tip: &str) -> egui::Response {
    let rect = Rect::from_center_size(center, vec2(32.0, 32.0));
    let response = ui
        .interact(rect, ui.id().with(("profile-action", tip)), Sense::click())
        .on_hover_text(tip);
    // O botão do Discord sobre a faixa: o escuro translúcido, que vale para qualquer cor de faixa.
    ui.painter().circle_filled(
        rect.center(),
        16.0,
        Color32::from_black_alpha(if response.hovered() { 210 } else { 150 }),
    );
    crate::ui::icons::text(ui.painter(), rect.center(), Align2::CENTER_CENTER, icon, theme::filled(16.0), theme::BRIGHT);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn profile_date(value: &str) -> Option<String> {
    let date = chrono::DateTime::parse_from_rfc3339(value)
        .ok()?
        .date_naive();
    let month = [
        "jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.",
        "dez.",
    ]
    .get(usize::try_from(date.month0()).ok()?)?;
    Some(format!("{} de {month} de {}", date.day(), date.year()))
}

#[cfg(test)]
mod profile_tests {
    use super::profile_date;

    #[test]
    fn profile_dates_are_written_in_portuguese() {
        assert_eq!(
            profile_date("2018-09-14T10:00:00Z").as_deref(),
            Some("14 de set. de 2018")
        );
        assert_eq!(profile_date("invalid"), None);
    }
}
