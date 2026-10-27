//! A Home: a página dos amigos (todos, pendentes, adicionar) e a conversa direta aberta.

use egui::{Align2, CornerRadius, Frame, Rect, Sense, Ui, pos2, vec2};
use egui_phosphor::regular as icons;

use core_app::models::{FriendshipStatus, Person};

use super::chat::{self, Abilities, Line};
use super::{theme, widgets};
use crate::app::{Action, Confirm, FriendsTab, Home, Unkvoid};

pub fn show(app: &mut Unkvoid, ui: &mut Ui) {
    if ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::K)) {
        app.quick_conversation_open = true;
    }
    match app.home.clone() {
        Home::Friends(tab) => friends(app, ui, tab),
        Home::Direct(person) => direct(app, ui, &person),
    }
    if app.quick_conversation_open {
        quick_conversation(app, ui);
    }
}

fn friends(app: &mut Unkvoid, ui: &mut Ui, tab: FriendsTab) {
    egui::Panel::top("friends-header")
        .exact_size(theme::HEADER_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(theme::MAIN)
                .inner_margin(egui::Margin::symmetric(16, 0)),
        )
        .show(ui, |ui| {
            let rect = ui.max_rect();

            ui.painter().hline(
                rect.x_range(),
                rect.bottom(),
                egui::Stroke::new(1.0, theme::RAIL),
            );
            ui.horizontal_centered(|ui| {
                crate::ui::icons::label(ui, icons::USERS, theme::icon(22.0), theme::DIM);
                ui.label(egui::RichText::new("Amigos").font(theme::bold(16.0)));
                ui.add_space(8.0);
                ui.label(egui::RichText::new("•").color(theme::DIM));

                let pending = app.pending_requests();

                for (shown, label) in [
                    (FriendsTab::Online, "Disponível".to_owned()),
                    (FriendsTab::All, "Todos".to_owned()),
                ] {
                    let selected = tab == shown;
                    let button = egui::Button::new(
                        egui::RichText::new(label)
                            .font(theme::bold(14.0))
                            .color(if selected { theme::TEXT } else { theme::MUTED }),
                    )
                    .fill(if selected {
                        theme::SELECTED
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .corner_radius(CornerRadius::same(4));

                    if ui.add(button).clicked() {
                        app.actions.push(Action::OpenFriends(shown));
                    }
                }

                // Sempre à vista, como no Discord: quem pediu também acompanha o pedido enviado, e a
                // aba não some debaixo de quem acabou de aceitar o último.
                let selected = tab == FriendsTab::Pending;
                let button = egui::Button::new(egui::RichText::new(pending_label(pending)).font(theme::bold(14.0)).color(if selected { theme::TEXT } else { theme::MUTED }))
                    .fill(if selected { theme::SELECTED } else { egui::Color32::TRANSPARENT })
                    .corner_radius(CornerRadius::same(4));
                if ui.add(button).clicked() { app.actions.push(Action::OpenFriends(FriendsTab::Pending)); }

                let add = egui::Button::new(
                    egui::RichText::new("Adicionar amigo")
                        .font(theme::bold(14.0))
                        .color(if tab == FriendsTab::Add {
                            theme::ACCENT
                        } else {
                            egui::Color32::WHITE
                        }),
                )
                .fill(if tab == FriendsTab::Add {
                    egui::Color32::TRANSPARENT
                } else {
                    theme::ACCENT
                })
                .corner_radius(CornerRadius::same(4));

                if ui.add(add).clicked() {
                    app.actions.push(Action::OpenFriends(FriendsTab::Add));
                }
            });
        });

    // O Discord tira a coluna "Ativo agora" da janela estreita: a lista de amigos fica com o espaço.
    if ui.available_width() >= 900.0 {
        active_now(app, ui);
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(16.0);

            if tab == FriendsTab::Add {
                return add_friend(app, ui);
            }

            // Na mesma margem do título e das linhas. Com `set_max_width` o campo ignorava o
            // espaço da esquerda e colava na borda da coluna.
            ui.horizontal(|ui| {
                ui.add_space(30.0);
                let width = (ui.available_width() - 30.0).max(0.0);
                ui.allocate_ui(egui::vec2(width, 40.0), |ui| {
                    widgets::text_field(ui, &mut app.friends_search, "Buscar", false);
                });
            });
            ui.add_space(12.0);

            let me = app.my_id();
            let search = app.friends_search.trim().to_lowercase();
            let shown: Vec<_> = app
                .friends
                .iter()
                .filter(|friend| match tab {
                    FriendsTab::Pending => friend.status == FriendshipStatus::Pending,
                    FriendsTab::Online => {
                        friend.status == FriendshipStatus::Accepted
                            && friend_person(friend, me).is_some_and(|person| {
                                matches!(person.status, core_app::models::UserStatus::Online | core_app::models::UserStatus::Idle)
                            })
                    }
                    FriendsTab::All | FriendsTab::Add => {
                        friend.status == FriendshipStatus::Accepted
                    }
                })
                .filter(|friend| friend_person(friend, me).is_some_and(|person| {
                    search.is_empty() || person.name.to_lowercase().contains(&search)
                }))
                .cloned()
                .collect();

            ui.horizontal(|ui| {
                ui.add_space(30.0);
                let title = match tab {
                    FriendsTab::Pending => "Pedidos pendentes",
                    FriendsTab::Online => "Online",
                    _ => "Todos os amigos",
                };

                ui.label(
                    egui::RichText::new(format!("{title} — {}", shown.len()))
                        .font(theme::bold(14.0))
                        .color(theme::MUTED),
                );
            });
            ui.add_space(8.0);

            if shown.is_empty() {
                empty(
                    ui,
                    if tab == FriendsTab::Pending {
                        "Não há pedidos de amizade pendentes."
                    } else if tab == FriendsTab::Online {
                        "Nenhum amigo disponível agora."
                    } else {
                        "Nenhum amigo ainda. Adicione alguém pelo apelido."
                    },
                );

                return;
            }

            for friend in &shown {
                let incoming = Some(friend.addressee.id) == me;
                let other = if incoming {
                    &friend.requester
                } else {
                    &friend.addressee
                };
                let note = match (friend.status, incoming) {
                    (FriendshipStatus::Pending, true) => "Pedido de amizade recebido",
                    (FriendshipStatus::Pending, false) => "Pedido de amizade enviado",
                    _ => "Amigo",
                };

                friend_row(
                    app,
                    ui,
                    friend.id,
                    other,
                    note,
                    friend.status == FriendshipStatus::Pending,
                    incoming,
                );
            }
        });
}

pub(super) fn friend_person(friendship: &core_app::models::Friendship, own_user_id: Option<i64>) -> Option<&Person> {
    if Some(friendship.requester.id) == own_user_id {
        Some(&friendship.addressee)
    } else if Some(friendship.addressee.id) == own_user_id {
        Some(&friendship.requester)
    } else {
        None
    }
}

fn pending_label(count: i64) -> String {
    if count > 0 { format!("Pendente ({count})") } else { "Pendente".to_owned() }
}

fn friend_row(
    app: &mut Unkvoid,
    ui: &mut Ui,
    friendship: i64,
    person: &Person,
    note: &str,
    pending: bool,
    incoming: bool,
) {
    let (outer, _) = ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::hover());
    let rect = outer.shrink2(vec2(20.0, 0.0));
    let response = ui.interact(rect, ui.id().with(("friend", friendship)), Sense::click());

    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(1.0, theme::DIVIDER),
    );

    if response.hovered() {
        ui.painter().rect_filled(
            rect.shrink2(vec2(0.0, 2.0)),
            CornerRadius::same(6),
            theme::HOVER,
        );
    }

    let avatar =
        Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), vec2(32.0, 32.0));

    widgets::paint_avatar(ui, avatar, &person.name, person.avatar_url.as_deref());
    widgets::paint_status(ui, avatar, person.status, person.status != core_app::models::UserStatus::Offline, theme::MAIN);
    let status_line = if pending {
        note.to_owned()
    } else if let Some(activity) = &person.activity {
        format!("Jogando {}", activity.name)
    } else {
        user_status_label(person.status).to_owned()
    };
    // Os dois botões redondos da direita ficam com o fim da linha: o texto para antes deles.
    let room = (rect.right() - 12.0 - 2.0 * 44.0) - (avatar.right() + 12.0);

    ui.painter().text(
        pos2(avatar.right() + 12.0, rect.center().y - 8.0),
        Align2::LEFT_CENTER,
        super::sidebar::elide(ui, &person.name, theme::bold(15.0), room),
        theme::bold(15.0),
        theme::TEXT,
    );
    ui.painter().text(
        pos2(avatar.right() + 12.0, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        super::sidebar::elide(ui, &status_line, theme::regular(14.0), room),
        theme::regular(14.0),
        theme::MUTED,
    );

    let mut right = rect.right() - 12.0;

    if pending {
        if friend_action_button(ui, rect, friendship, &mut right, icons::X, "Recusar", theme::DANGER) {
            app.actions.push(Action::AnswerFriend(friendship, false));
        }

        if incoming && friend_action_button(ui, rect, friendship, &mut right, icons::CHECK, "Aceitar", theme::SUCCESS) {
            app.actions.push(Action::AnswerFriend(friendship, true));
        }
    } else {
        // Como no Discord: a conversa e, na ponta, o "⋮".
        let mut message = right - 44.0;

        if friend_action_button(ui, rect, friendship, &mut message, icons::CHAT_CIRCLE, "Mensagem", theme::TEXT) || response.clicked() {
            app.actions.push(Action::OpenDirect(person.clone()));
        }

        let more = ui
            .interact(
                Rect::from_center_size(pos2(right - 18.0, rect.center().y), vec2(36.0, 36.0)),
                ui.id().with(("friend-options", friendship)),
                Sense::click(),
            )
            .on_hover_text("Mais opções");
        ui.painter().circle_filled(
            more.rect.center(),
            18.0,
            if more.hovered() { theme::RAIL } else { theme::SIDEBAR },
        );
        super::icons::paint_glyph(
            ui,
            Rect::from_center_size(more.rect.center(), vec2(18.0, 18.0)),
            icons::DOTS_THREE_VERTICAL,
            if more.hovered() { theme::TEXT } else { theme::MUTED },
            18.0,
        );
        egui::Popup::menu(&more)
            .frame(widgets::menu_frame())
            .show(|ui| {
                if widgets::menu_item(ui, "", "Desfazer amizade", true) {
                    app.actions.push(Action::Ask(Confirm::Unfriend {
                        friendship,
                        name: person.name.clone(),
                    }));
                    ui.close();
                }
            });
    }
}

fn friend_action_button(
    ui: &mut Ui,
    row: Rect,
    friendship: i64,
    right: &mut f32,
    icon: &str,
    tip: &str,
    color: egui::Color32,
) -> bool {
    let place = Rect::from_center_size(pos2(*right - 18.0, row.center().y), vec2(36.0, 36.0));
    *right -= 44.0;

    let response = ui
        .interact(place, ui.id().with((tip, friendship)), Sense::click())
        .on_hover_text(tip);
    ui.painter().circle_filled(
        place.center(),
        18.0,
        if response.hovered() { theme::RAIL } else { theme::SIDEBAR },
    );
    crate::ui::icons::text(ui.painter(), place.center(), Align2::CENTER_CENTER, icon, theme::icon(18.0), if response.hovered() { color } else { theme::MUTED });

    response.clicked()
}

fn active_now(app: &mut Unkvoid, ui: &mut Ui) {
    egui::Panel::right("friends-active-now")
        .exact_size(320.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(theme::MAIN).inner_margin(egui::Margin::same(16)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new("Ativo agora").font(theme::bold(18.0)));
            ui.add_space(12.0);

            let own_user_id = app.my_id();
            let mut active: Vec<_> = app
                .friends
                .iter()
                .filter(|friendship| friendship.status == FriendshipStatus::Accepted)
                .filter_map(|friendship| friend_person(friendship, own_user_id))
                .filter(|person| {
                    person.status != core_app::models::UserStatus::Offline
                        && person.activity.is_some()
                })
                .cloned()
                .collect();
            active.sort_by(|left, right| {
                left.activity
                    .as_ref()
                    .map(|activity| activity.name.as_str())
                    .cmp(&right.activity.as_ref().map(|activity| activity.name.as_str()))
            });

            if active.is_empty() {
                ui.label(
                    egui::RichText::new("Quando um amigo compartilhar uma atividade, ela aparecerá aqui.")
                        .font(theme::regular(14.0))
                        .color(theme::MUTED),
                );
                return;
            }

            for person in active {
                let activity_name = person
                    .activity
                    .as_ref()
                    .map(|activity| activity.name.as_str())
                    .unwrap_or_default()
                    .to_owned();
                let (rect, response) = ui.allocate_exact_size(
                    vec2(ui.available_width(), 68.0),
                    Sense::click(),
                );
                if response.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::same(8), theme::HOVER);
                }
                let avatar = Rect::from_center_size(
                    pos2(rect.left() + 24.0, rect.center().y),
                    vec2(36.0, 36.0),
                );
                widgets::paint_avatar(ui, avatar, &person.name, person.avatar_url.as_deref());
                widgets::paint_status(ui, avatar, person.status, true, theme::MAIN);
                ui.painter().text(
                    pos2(avatar.right() + 10.0, rect.center().y - 8.0),
                    Align2::LEFT_CENTER,
                    &person.name,
                    theme::bold(14.0),
                    theme::TEXT,
                );
                ui.painter().text(
                    pos2(avatar.right() + 10.0, rect.center().y + 10.0),
                    Align2::LEFT_CENTER,
                    format!("Jogando {activity_name}"),
                    theme::regular(12.0),
                    theme::MUTED,
                );
                if response.clicked() {
                    app.actions.push(Action::OpenDirect(person));
                }
            }
        });
}

fn user_status_label(status: core_app::models::UserStatus) -> &'static str {
    match status {
        core_app::models::UserStatus::Online => "Disponível",
        core_app::models::UserStatus::Idle => "Ausente",
        core_app::models::UserStatus::Dnd => "Não perturbar",
        core_app::models::UserStatus::Invisible | core_app::models::UserStatus::Offline => "Off-line",
    }
}

fn add_friend(app: &mut Unkvoid, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.add_space(30.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("ADICIONAR AMIGO").font(theme::bold(16.0)));
            ui.label(
                egui::RichText::new("Você pode adicionar amigos pelo apelido ou pelo e-mail da conta deles.")
                    .color(theme::MUTED),
            );
            ui.add_space(14.0);
            ui.set_max_width(640.0_f32.min(ui.available_width() - 30.0));
            ui.horizontal(|ui| {
                // O campo cresce até onde a linha deixa: sem a conta, ele tomava a largura inteira
                // e empurrava o botão para fora da tela.
                let width = (ui.available_width() - 140.0 - ui.spacing().item_spacing.x).max(160.0);
                let field = ui
                    .allocate_ui(egui::vec2(width, 40.0), |ui| {
                        widgets::text_field(ui, &mut app.friend_email, "Apelido ou e-mail", false)
                    })
                    .inner;
                let sent =
                    field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));

                if widgets::primary_button(
                    ui,
                    "Enviar pedido",
                    !app.friend_email.trim().is_empty(),
                    140.0,
                ) || sent
                {
                    app.actions.push(Action::AddFriend);
                }
            });
        });
    });
}

fn direct(app: &mut Unkvoid, ui: &mut Ui, person: &Person) {
    egui::Panel::top("direct-header")
        .exact_size(theme::HEADER_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(theme::MAIN)
                .inner_margin(egui::Margin::symmetric(16, 0)),
        )
        .show(ui, |ui| {
            let rect = ui.max_rect();

            ui.painter().hline(
                rect.x_range(),
                rect.bottom(),
                egui::Stroke::new(1.0, theme::RAIL),
            );
            ui.horizontal_centered(|ui| {
                let avatar = widgets::avatar(ui, &person.name, person.avatar_url.as_deref(), 24.0);
                widgets::paint_status(ui, avatar.rect, person.status, true, theme::MAIN);
                ui.label(egui::RichText::new(&person.name).font(theme::bold(16.0)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if app.direct_search_open {
                        ui.set_width(220.0);
                        widgets::text_field(ui, &mut app.direct_search_query, &format!("Buscar {}", person.name), false);
                    } else if widgets::icon_button(ui, icons::MAGNIFYING_GLASS, "Buscar na conversa", theme::MUTED).clicked() {
                        app.direct_search_open = true;
                    }
                    if widgets::icon_button(ui, icons::USER_CIRCLE, "Mostrar perfil do usuário", if app.direct_profile_open { theme::ACCENT } else { theme::MUTED }).clicked() {
                        app.direct_profile_open = !app.direct_profile_open;
                    }
                    if friendship_state(app, person.id).is_none()
                        && widgets::icon_button(ui, icons::USER_PLUS, "Adicionar amigo", theme::MUTED).clicked()
                    {
                        app.actions.push(Action::AddFriendById(person.id));
                    }
                });
            });
        });

    // As mesmas margens da caixa do canal: trocar de canal para DM não pula a caixa de lugar, e
    // ela fecha na linha do painel da conta.
    egui::Panel::bottom("direct-composer")
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(theme::MAIN).inner_margin(egui::Margin {
            left: 8,
            right: 8,
            top: 0,
            bottom: 8,
        }))
        .show(ui, |ui| {
            let mut text = std::mem::take(&mut app.direct_composer);
            let sent = chat::composer(
                ui,
                "direct-composer",
                &mut text,
                &format!("Conversar em @{}", person.name),
                true,
                false,
            )
            .enter
                && !text.trim().is_empty();

            app.direct_composer = text;

            if sent {
                app.actions.push(Action::SendDirect);
            }
        });

    let narrow = ui.available_width() < 780.0;
    if narrow { app.direct_profile_open = false; }
    let mut profile_open = app.direct_profile_open && !narrow;
    egui::Panel::right("direct-profile")
        .exact_size(300.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(theme::MAIN).stroke(egui::Stroke::new(1.0, theme::BORDER)))
        .show_collapsible(ui, &mut profile_open, |ui| {
            direct_profile(ui, app, person);
        });
    app.direct_profile_open = profile_open;

    let messages = app.direct_messages.clone();
    let search_query = app.direct_search_query.trim().to_lowercase();
    let lines: Vec<Line<'_>> = messages
        .iter()
        .filter(|message| search_query.is_empty() || message.body.to_lowercase().contains(&search_query))
        .map(|message| Line {
            id: message.id,
            author: &message.sender.name,
            author_id: message.sender.id,
            avatar: message.sender.avatar_url.as_deref(),
            color: None,
            body: &message.body,
            at: &message.created_at,
            edited: message.edited_at.is_some(),
            join: false,
            reply: None,
            images: Vec::new(),
        })
        .collect();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.add_space(16.0);
            if app.direct_loading {
                message_skeleton(ui);
                return;
            }
            if messages.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    widgets::avatar(ui, &person.name, person.avatar_url.as_deref(), 80.0);
                    ui.label(egui::RichText::new(&person.name).font(theme::bold(32.0)));
                    ui.label(egui::RichText::new(&person.name).font(theme::regular(24.0)).color(theme::MUTED));
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new("Este é o começo do seu histórico de mensagens diretas com ").color(theme::MUTED));
                        ui.label(egui::RichText::new(&person.name).font(theme::bold(14.0)).color(theme::TEXT));
                        ui.label(egui::RichText::new(".").color(theme::MUTED));
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        for server in app.direct_mutuals.mutual_servers.iter().take(5) {
                            let (rect, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                            widgets::paint_avatar(ui, rect, &server.name, server.icon_url.as_deref());
                        }
                        ui.label(egui::RichText::new(format!("{} servidores mútuos", app.direct_mutuals.mutual_servers.len())).font(theme::regular(13.0)).color(theme::MUTED));
                    });
                    if let Some(friendship) = friendship_id(app, person.id) {
                        ui.horizontal(|ui| {
                            if widgets::secondary_button(ui, "Desfazer amizade") { app.actions.push(Action::Ask(Confirm::Unfriend { friendship, name: person.name.clone() })); }
                            if widgets::secondary_button(ui, "Bloquear") { app.actions.push(Action::Ask(Confirm::Block { friendship, name: person.name.clone() })); }
                        });
                    } else if friendship_state(app, person.id).is_none()
                        && widgets::secondary_button(ui, "Adicionar amigo")
                    {
                        app.actions.push(Action::AddFriendById(person.id));
                    }
                });
            });
            }

            let editing = app.editing.clone();

            for (index, line) in lines.iter().enumerate() {
                let previous = index.checked_sub(1).and_then(|index| lines.get(index));
                let continued = previous.is_some_and(|previous| chat::continues(previous, line));
                let mine = Some(line.author_id) == app.my_id();
                let edit = editing
                    .as_ref()
                    .filter(|(id, _)| *id == line.id)
                    .map(|(_, body)| body.clone());

                chat::row(app, ui, line, continued, Abilities { edit: mine, delete: mine }, edit);
            }

            ui.add_space(12.0);
        });
}

fn direct_profile(ui: &mut Ui, app: &mut Unkvoid, person: &Person) {
    ui.vertical(|ui| {
        let (banner, _) = ui.allocate_exact_size(vec2(ui.available_width(), 100.0), Sense::hover());
        ui.painter().rect_filled(banner, CornerRadius::same(8), person.banner_color.as_deref().and_then(parse_hex_color).unwrap_or_else(|| widgets::color_for(&person.name)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if friendship_state(app, person.id).is_none() && widgets::icon_button(ui, icons::USER_PLUS, "Adicionar amigo", theme::MUTED).clicked() {
                app.actions.push(Action::AddFriendById(person.id));
            }
            let more = widgets::icon_button(ui, icons::DOTS_THREE, "Mais opções", theme::MUTED);
            egui::Popup::menu(&more).id(more.id.with("direct-profile-menu")).show(|ui| {
                if widgets::menu_item(ui, icons::IDENTIFICATION_CARD, "Copiar ID do usuário", false) {
                    app.actions.push(Action::CopyText(person.id.to_string()));
                    ui.close();
                }
                if let Some(friendship) = friendship_id(app, person.id) {
                    widgets::menu_separator(ui);
                    if widgets::menu_item(ui, "", "Desfazer amizade", true) {
                        app.actions.push(Action::Ask(Confirm::Unfriend { friendship, name: person.name.clone() }));
                        ui.close();
                    }
                }
            });
        });
        let avatar = Rect::from_center_size(pos2(banner.left() + 56.0, banner.bottom() - 4.0), vec2(80.0, 80.0));
        ui.painter().circle_filled(avatar.center(), 46.0, theme::MAIN);
        widgets::paint_avatar(ui, avatar, &person.name, person.avatar_url.as_deref());
        widgets::paint_status(ui, avatar, person.status, true, theme::MAIN);
        ui.add_space(48.0);
        ui.label(egui::RichText::new(&person.name).font(theme::bold(20.0)));
        ui.label(egui::RichText::new(format!("@{}", person.name.to_lowercase())).font(theme::regular(14.0)).color(theme::MUTED));
        ui.add_space(18.0);
        ui.label(egui::RichText::new(format!("{} amigos mútuos • {} servidores mútuos", app.direct_mutuals.mutual_friends, app.direct_mutuals.mutual_servers.len())).font(theme::regular(12.0)).color(theme::DESCRIPTION));
        ui.add_space(12.0);
        ui.label(egui::RichText::new("Membro desde").font(theme::bold(12.0)).color(theme::DESCRIPTION));
        let member_since = person.created_at.as_deref().and_then(|date| date.get(..10)).map(|date| {
            let mut parts = date.split('-');
            match (parts.next(), parts.next(), parts.next()) {
                (Some(year), Some(month), Some(day)) => format!("{day}/{month}/{year}"),
                _ => "—".to_owned(),
            }
        }).unwrap_or_else(|| "—".to_owned());
        ui.label(egui::RichText::new(member_since).font(theme::regular(14.0)).color(theme::TEXT));
    });
}

fn friendship_id(app: &Unkvoid, person: i64) -> Option<i64> {
    app.friends.iter().find(|friendship| friendship.status == FriendshipStatus::Accepted && (friendship.requester.id == person || friendship.addressee.id == person)).map(|friendship| friendship.id)
}

fn friendship_state(app: &Unkvoid, person: i64) -> Option<(i64, FriendshipStatus)> {
    app.friends.iter().find(|friendship| friendship.requester.id == person || friendship.addressee.id == person).map(|friendship| (friendship.id, friendship.status))
}

fn parse_hex_color(value: &str) -> Option<egui::Color32> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 { return None; }
    let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(egui::Color32::from_rgb(red, green, blue))
}

fn message_skeleton(ui: &mut Ui) {
    for row in 0..5 {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
        let center = rect.left_top() + vec2(32.0, 28.0);
        ui.painter().circle_filled(center, 20.0, theme::ELEVATED);
        let first_width = 80.0 + (row as f32 * 19.0) % 100.0;
        ui.painter().rect_filled(Rect::from_min_size(rect.left_top() + vec2(64.0, 16.0), vec2(first_width, 10.0)), CornerRadius::same(5), theme::ELEVATED);
        ui.painter().rect_filled(Rect::from_min_size(rect.left_top() + vec2(64.0, 36.0), vec2((rect.width() * 0.55).max(0.0), 10.0)), CornerRadius::same(5), theme::ELEVATED);
    }
}

fn quick_conversation(app: &mut Unkvoid, ui: &mut Ui) {
    let mut open = app.quick_conversation_open;
    let mut selected = None;
    egui::Window::new("Encontre ou comece uma conversa")
        .id(ui.id().with("quick-conversation"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ui.ctx(), |ui| {
            widgets::text_field(ui, &mut app.quick_conversation_query, "Buscar amigos e conversas", false);
            let query = app.quick_conversation_query.trim().to_lowercase();
            let own_user_id = app.my_id();
            let people = quick_conversation_people(
                app.conversations.iter().map(|conversation| conversation.user.clone()),
                app.friends.iter()
                    .filter(|friendship| friendship.status == FriendshipStatus::Accepted)
                    .filter_map(|friendship| friend_person(friendship, own_user_id).cloned()),
            );
            let filtered: Vec<_> = people.into_iter().filter(|person| matches_quick_query(&person.name, &query)).collect();
            for person in filtered.iter().take(8) {
                if ui.selectable_label(false, &person.name).clicked() { selected = Some(person.clone()); }
            }
            if ui.input(|input| input.key_pressed(egui::Key::Enter)) { selected = filtered.first().cloned(); }
        });
    app.quick_conversation_open = open;
    if let Some(person) = selected { app.actions.push(Action::OpenDirect(person)); }
}

fn empty(ui: &mut Ui, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(60.0);
        crate::ui::icons::label(ui, icons::USERS_THREE, theme::icon(56.0), theme::DIM);
        ui.add_space(8.0);
        ui.label(egui::RichText::new(text).color(theme::MUTED));
    });
}

fn matches_quick_query(name: &str, query: &str) -> bool {
    query.is_empty() || name.to_lowercase().contains(query)
}

fn quick_conversation_people(
    conversations: impl IntoIterator<Item = Person>,
    friends: impl IntoIterator<Item = Person>,
) -> Vec<Person> {
    let mut seen = std::collections::HashSet::new();
    conversations
        .into_iter()
        .chain(friends)
        .filter(|person| seen.insert(person.id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{matches_quick_query, quick_conversation_people};
    use core_app::models::Person;

    fn person(id: i64, name: &str) -> Person {
        Person {
            id,
            name: name.to_owned(),
            avatar_url: None,
            status: Default::default(),
            activity: None,
            created_at: None,
            banner_color: None,
        }
    }

    #[test]
    fn quick_conversation_search_is_case_insensitive_and_matches_empty_query() {
        assert!(matches_quick_query("Mr.Himura", "him"));
        assert!(matches_quick_query("developer", ""));
        assert!(!matches_quick_query("developer", "maria"));
    }

    #[test]
    fn quick_conversation_keeps_recent_order_and_deduplicates_friends() {
        let people = quick_conversation_people(
            [person(8, "Recent"), person(4, "Older")],
            [person(8, "Recent friend"), person(2, "Friend")],
        );

        assert_eq!(people.iter().map(|person| person.id).collect::<Vec<_>>(), [8, 4, 2]);
        assert_eq!(people[0].name, "Recent");
    }
}
