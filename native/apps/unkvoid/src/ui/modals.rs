//! As janelas por cima de tudo: criar ou entrar num servidor, criar canal, convite, sair do
//! servidor, as configurações da conta e o apelido que precisa ser confirmado.

use egui::{CornerRadius, Frame, Ui};
use egui_phosphor::regular as icons;

use core_app::models::ChannelKind;
use serde_json::{Value, json};

use super::{theme, widgets};
use crate::app::{Action, AddServerStep, Confirm, Modal, Unkvoid};
use crate::backend::Refresh;

pub fn show(app: &mut Unkvoid, ui: &mut Ui) {
    if app.needs_nickname() {
        return nickname(app, ui);
    }

    if let Some(modal) = app.modal.clone() {
        dialog(app, ui, modal);
    }

    // Depois do modal: a pergunta abre por cima das configurações.
    confirm(app, ui);
}

fn dialog(app: &mut Unkvoid, ui: &mut Ui, mut modal: Modal) {
    match modal {
        Modal::Settings(page) => return super::settings::show(app, ui, page),
        Modal::ServerSettings(state) => return super::server_settings::show(app, ui, state),
        Modal::ChannelSettings(state) => return super::channel_settings::show(app, ui, state),
        Modal::VoiceDebug(page) => return super::voice_debug::show(app, ui, page),
        Modal::MemberProfile { server, user } => {
            return super::members::profile_modal(app, ui, server, user);
        }
        Modal::ProfileEditor(draft) => return super::settings::profile_editor(app, ui, draft),
        Modal::GoLive(tab) => return super::go_live::show(app, ui, tab),
        Modal::CameraPreview => return super::go_live::camera_preview(app, ui),
        _ => {}
    }

    let width = if matches!(&modal, Modal::AddServer(_)) {
        500.0
    } else if matches!(&modal, Modal::Invite { .. }) {
        560.0
    } else if matches!(&modal, Modal::ServerSound { .. } | Modal::CodeRoom) {
        380.0
    } else {
        440.0
    };
    let closed = window(ui, "modal", width, |ui| match &mut modal {
        Modal::AddServer(step) => add_server(app, ui, step),
        Modal::CreateChannel {
            kind, name, parent, ..
        } => create_channel(app, ui, kind, name, parent.is_some()),
        Modal::Invite { server, .. } => invite(app, ui, *server),
        Modal::ServerSound {
            path, name, emoji, ..
        } => server_sound(app, ui, path, name, emoji),
        Modal::LeaveServer { server, name } => leave_server(app, ui, *server, name),
        Modal::CodeRoom => super::login::code_room(app, ui),
        Modal::Settings(_)
        | Modal::ServerSettings(_)
        | Modal::ChannelSettings(_)
        | Modal::VoiceDebug(_)
        | Modal::MemberProfile { .. } => {}
        Modal::ProfileEditor(_) | Modal::GoLive(_) | Modal::CameraPreview => {}
    });

    // O que foi digitado volta para o estado antes das ações do quadro: o "Criar" lê o nome dali.
    if closed {
        app.actions.push(Action::CloseModal);
    } else if app.modal.is_some() {
        app.modal = Some(modal);
    }
}

fn server_sound(
    app: &mut Unkvoid,
    ui: &mut Ui,
    path: &std::path::Path,
    name: &mut String,
    emoji: &mut String,
) {
    title(ui, "Adicionar som", "MP3, OGG ou WAV de até 512 KB.");
    ui.label(
        egui::RichText::new(path.file_name().unwrap_or_default().to_string_lossy())
            .color(theme::MUTED),
    );
    widgets::field_label(ui, "Nome", None);
    ui.add_space(5.0);
    widgets::text_field(ui, name, "Nome do som", false);
    ui.add_space(10.0);
    widgets::field_label(ui, "Emoji", None);
    ui.add_space(5.0);
    widgets::text_field(ui, emoji, "🎵", false);
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        if widgets::secondary_button(ui, "Cancelar") {
            app.actions.push(Action::CloseModal);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ready = !name.trim().is_empty() && !emoji.trim().is_empty();
            if widgets::primary_button(ui, "Adicionar", ready, 120.0) {
                app.actions.push(Action::SaveServerSound);
            }
        });
    });
}

/// A pergunta antes de expulsar, banir, apagar ou trocar o apelido. Fecha sozinha ao confirmar.
fn confirm(app: &mut Unkvoid, ui: &mut Ui) {
    let Some(mut asked) = app.confirm.clone() else {
        return;
    };

    let closed = window(ui, "confirm", 440.0, |ui| match &mut asked {
        Confirm::Kick { server, user, name } => {
            title(ui, &format!("Expulsar {name}"), "");
            ui.label(egui::RichText::new(format!("Tem certeza de que quer expulsar {name} do servidor? A pessoa pode voltar com outro convite.")).color(theme::MUTED));
            ui.add_space(20.0);

            if answer(app, ui, "Expulsar", true, true) {
                app.actions.push(Action::request(
                    "kickMember",
                    json!({ "server": server, "user": user }),
                    Value::Null,
                    Refresh::Tree(*server),
                ));
            }
        }
        Confirm::Ban {
            server,
            user,
            name,
            reason,
        } => {
            title(ui, &format!("Banir {name}"), "");
            ui.label(egui::RichText::new(format!("{name} sai do servidor e não volta, nem com convite, até o banimento ser revogado.")).color(theme::MUTED));
            ui.add_space(16.0);
            widgets::field_label(ui, "Motivo do banimento", None);
            ui.add_space(6.0);
            widgets::text_field(ui, reason, "Opcional", false);
            ui.add_space(20.0);

            if answer(app, ui, "Banir", true, true) {
                let reason = reason.trim();
                let body = if reason.is_empty() {
                    json!({})
                } else {
                    json!({ "reason": reason })
                };

                app.actions.push(Action::request(
                    "ban",
                    json!({ "server": server, "user": user }),
                    body,
                    Refresh::Tree(*server),
                ));
            }
        }
        Confirm::Nickname {
            server,
            user,
            name,
            value,
        } => {
            title(
                ui,
                "Alterar apelido",
                &format!("O nome de {name} só neste servidor."),
            );
            widgets::field_label(ui, "Apelido", None);
            ui.add_space(6.0);
            widgets::text_field(ui, value, name, false);
            ui.add_space(8.0);

            if ui
                .add(
                    egui::Label::new(
                        egui::RichText::new("Redefinir apelido").color(theme::ACCENT_TEXT),
                    )
                    .sense(egui::Sense::click()),
                )
                .clicked()
            {
                value.clear();
            }

            ui.add_space(16.0);

            if answer(app, ui, "Salvar", false, true) {
                let nickname = value.trim();
                let nickname = if nickname.is_empty() {
                    Value::Null
                } else {
                    json!(nickname)
                };

                app.actions.push(Action::request(
                    "updateMember",
                    json!({ "server": server, "user": user }),
                    json!({ "nickname": nickname }),
                    Refresh::Tree(*server),
                ));
            }
        }
        Confirm::DeleteServer {
            server,
            name,
            typed,
        } => {
            title(ui, &format!("Excluir \"{name}\""), "");
            ui.label(egui::RichText::new(format!("Tem certeza de que quer excluir {name}? Os canais, as mensagens e os cargos somem junto, e isso não tem volta.")).color(theme::MUTED));
            ui.add_space(16.0);
            widgets::field_label(ui, "Digite o nome do servidor", None);
            ui.add_space(6.0);
            widgets::text_field(ui, typed, name, false);
            ui.add_space(20.0);

            if answer(
                app,
                ui,
                "Excluir servidor",
                true,
                typed.trim() == name.as_str(),
            ) {
                app.actions.push(Action::request(
                    "deleteServer",
                    json!({ "server": server }),
                    Value::Null,
                    Refresh::Servers,
                ));
                app.actions.push(Action::CloseModal);
            }
        }
        Confirm::DeleteChannel {
            server,
            channel,
            name,
        } => {
            title(ui, "Excluir canal", "");
            ui.label(
                egui::RichText::new(format!(
                    "Tem certeza de que quer excluir {name}? Isso não tem volta."
                ))
                .color(theme::MUTED),
            );
            ui.add_space(20.0);

            if answer(app, ui, "Excluir canal", true, true) {
                app.actions.push(Action::request(
                    "deleteChannel",
                    json!({ "channel": channel }),
                    Value::Null,
                    Refresh::Tree(*server),
                ));
                app.actions.push(Action::CloseModal);
            }
        }
        Confirm::DeleteRole { server, role, name } => {
            title(ui, &format!("Excluir o cargo {name}"), "");
            ui.label(
                egui::RichText::new("Quem tem o cargo perde o que ele dava. Isso não tem volta.")
                    .color(theme::MUTED),
            );
            ui.add_space(20.0);

            if answer(app, ui, "Excluir cargo", true, true) {
                app.actions.push(Action::request(
                    "deleteRole",
                    json!({ "role": role }),
                    Value::Null,
                    Refresh::Tree(*server),
                ));
            }
        }
        Confirm::Unfriend { friendship, name } => {
            title(ui, &format!("Remover \"{name}\""), "");
            ui.label(
                egui::RichText::new(format!(
                    "Tem certeza de que quer remover {name} da sua lista de amigos?"
                ))
                .color(theme::MUTED),
            );
            ui.add_space(20.0);

            if answer(app, ui, "Desfazer amizade", true, true) {
                app.actions.push(Action::request(
                    "removeFriend",
                    json!({ "friendship": friendship }),
                    Value::Null,
                    Refresh::Friends,
                ));
            }
        }
        Confirm::Block { friendship, name } => {
            title(ui, &format!("Bloquear {name}?"), "");
            ui.label(
                egui::RichText::new(format!(
                    "Tem certeza de que quer bloquear {name}? Bloquear também tira essa pessoa da sua lista de amigos."
                ))
                .color(theme::MUTED),
            );
            ui.add_space(20.0);

            if answer(app, ui, "Bloquear", true, true) {
                app.actions.push(Action::BlockFriend(*friendship));
            }
        }
    });

    if closed {
        app.actions.push(Action::CloseConfirm);
    } else if app.confirm.is_some() {
        app.confirm = Some(asked);
    }
}

/// O "Cancelar" à esquerda e o botão da ação à direita, vermelho quando não tem volta. Devolve se
/// a ação foi escolhida; os dois fecham a pergunta.
fn answer(app: &mut Unkvoid, ui: &mut Ui, label: &str, danger: bool, ready: bool) -> bool {
    let mut chosen = false;

    ui.horizontal(|ui| {
        if widgets::secondary_button(ui, "Cancelar") {
            app.actions.push(Action::CloseConfirm);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clicked = if danger {
                widgets::danger_button(ui, label, ready)
            } else {
                widgets::primary_button(ui, label, ready, 110.0)
            };

            if clicked {
                chosen = true;
                app.actions.push(Action::CloseConfirm);
            }
        });
    });

    chosen
}

/// O fundo escurecido e o cartão no meio. Devolve se a pessoa fechou (Esc ou clique fora).
fn window(ui: &mut Ui, id: &str, width: f32, contents: impl FnOnce(&mut Ui)) -> bool {
    let response = egui::Modal::new(egui::Id::new(id))
        .frame(
            Frame::new()
                .fill(theme::SIDEBAR)
                .corner_radius(CornerRadius::same(8))
                .inner_margin(egui::Margin::same(24)),
        )
        .backdrop_color(egui::Color32::from_black_alpha(170))
        .show(ui.ctx(), |ui| {
            ui.set_width(width);
            contents(ui);
        });

    response.should_close()
}

fn title(ui: &mut Ui, text: &str, subtitle: &str) {
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(text).font(theme::bold(22.0)));

        if !subtitle.is_empty() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(subtitle).color(theme::MUTED));
        }
    });
    ui.add_space(18.0);
}

fn add_server(app: &mut Unkvoid, ui: &mut Ui, step: &mut AddServerStep) {
    match step {
        AddServerStep::Choose => {
            title(
                ui,
                "Criar seu servidor",
                "Seu servidor é onde você e seus amigos se reúnem. Crie o seu e comece a interagir.",
            );
            let (rect, response) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 64.0), egui::Sense::click());
            ui.painter().rect_filled(
                rect,
                theme::RADIUS,
                if response.hovered() {
                    theme::HOVER
                } else {
                    theme::ELEVATED
                },
            );
            crate::ui::icons::text(ui.painter(), rect.left_center() + egui::vec2(20.0, 0.0), egui::Align2::LEFT_CENTER, icons::PLUS_CIRCLE, theme::icon(24.0), theme::ACCENT_TEXT);
            ui.painter().text(
                rect.left_center() + egui::vec2(58.0, 0.0),
                egui::Align2::LEFT_CENTER,
                "Criar o meu",
                theme::bold(16.0),
                theme::TEXT,
            );
            crate::ui::icons::text(ui.painter(), rect.right_center() - egui::vec2(18.0, 0.0), egui::Align2::RIGHT_CENTER, icons::CARET_RIGHT, theme::icon(20.0), theme::MUTED);
            if response.clicked() {
                let account_name = app
                    .me
                    .as_ref()
                    .map(|person| person.name.as_str())
                    .unwrap_or_default();
                *step = AddServerStep::Customize {
                    name: format!("Servidor de {account_name}"),
                    icon: None,
                };
            }
            ui.add_space(24.0);
            ui.separator();
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Já tem um convite?").color(theme::MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::secondary_button(ui, "Entrar em um servidor") {
                        *step = AddServerStep::Join {
                            invite: String::new(),
                        };
                    }
                });
            });
        }
        AddServerStep::Customize { name, icon } => {
            title(
                ui,
                "Personalize o seu servidor",
                "Dê uma personalidade ao seu novo servidor com um nome e um ícone. Você pode mudar isso depois.",
            );
            ui.vertical_centered(|ui| {
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(112.0, 112.0), egui::Sense::click());
                let center = rect.center();
                if let Some(icon) = icon {
                    egui::Image::from_bytes(
                        format!("server-icon://{}", icon.path.display()),
                        std::sync::Arc::clone(&icon.bytes),
                    )
                    .corner_radius(egui::CornerRadius::same(u8::MAX))
                    .fit_to_exact_size(egui::vec2(112.0, 112.0))
                    .paint_at(ui, rect);
                } else {
                    for index in 0..24 {
                        let angle = std::f32::consts::TAU * index as f32 / 24.0;
                        ui.painter().circle_filled(
                            center + egui::vec2(angle.cos(), angle.sin()) * 52.0,
                            1.6,
                            theme::MUTED,
                        );
                    }
                    crate::ui::icons::text(ui.painter(), center, egui::Align2::CENTER_CENTER, icons::IMAGE, theme::icon(28.0), theme::MUTED);
                    ui.painter().text(
                        center + egui::vec2(0.0, 24.0),
                        egui::Align2::CENTER_CENTER,
                        "ENVIAR",
                        theme::bold(10.0),
                        theme::MUTED,
                    );
                }
                if response.clicked() {
                    app.actions.push(Action::PickServerIcon);
                }
            });
            ui.add_space(18.0);
            widgets::field_label(ui, "Nome do servidor", None);
            ui.add_space(6.0);
            widgets::text_field(ui, name, "Servidor", false);
            ui.add_space(22.0);
            let ready = !name.trim().is_empty();
            let mut go_back = false;
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Label::new(egui::RichText::new("Voltar").color(theme::ACCENT_TEXT))
                            .sense(egui::Sense::click()),
                    )
                    .clicked()
                {
                    go_back = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::primary_button(ui, "Criar", ready, 120.0) {
                        app.actions.push(Action::CreateServer);
                    }
                });
            });
            if go_back {
                *step = AddServerStep::Choose;
            }
        }
        AddServerStep::Join { invite } => {
            title(
                ui,
                "Entrar em um servidor",
                "Digite um convite abaixo para entrar em um servidor existente.",
            );
            widgets::field_label(ui, "Link do convite", None);
            ui.add_space(6.0);
            let field = widgets::text_field(ui, invite, "abcdef1234", false);
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(
                    "Os convites ficam assim: abcdef1234, https://unkvoid.com/convite/abcdef1234",
                )
                .font(theme::regular(12.0))
                .color(theme::DIM),
            );
            let typed = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let ready = !invite.trim().is_empty();
            ui.add_space(22.0);
            let mut go_back = false;
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Label::new(egui::RichText::new("Voltar").color(theme::ACCENT_TEXT))
                            .sense(egui::Sense::click()),
                    )
                    .clicked()
                {
                    go_back = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::primary_button(ui, "Entrar no servidor", ready, 170.0)
                        || (typed && ready)
                    {
                        app.actions.push(Action::JoinInvite);
                    }
                });
            });
            if go_back {
                *step = AddServerStep::Choose;
            }
        }
    }
}

fn create_channel(
    app: &mut Unkvoid,
    ui: &mut Ui,
    kind: &mut ChannelKind,
    name: &mut String,
    in_category: bool,
) {
    if *kind == ChannelKind::Category {
        title(ui, "Criar categoria", "");
    } else {
        title(
            ui,
            "Criar canal",
            if in_category {
                "na categoria escolhida"
            } else {
                ""
            },
        );
        widgets::field_label(ui, "Tipo de canal", None);
        ui.add_space(6.0);

        for (option, icon, label, about) in [
            (
                ChannelKind::Text,
                icons::HASH,
                "Texto",
                "Envie mensagens, imagens e opiniões",
            ),
            (
                ChannelKind::Voice,
                icons::SPEAKER_HIGH,
                "Voz",
                "Converse por voz, vídeo e compartilhe a tela",
            ),
        ] {
            let selected = *kind == option;
            let response = Frame::new()
                .fill(if selected {
                    theme::SELECTED
                } else {
                    theme::ELEVATED
                })
                .corner_radius(theme::RADIUS)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        crate::ui::icons::label(ui, icon, theme::icon(22.0), theme::MUTED);
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(label).font(theme::bold(15.0)));
                            ui.label(
                                egui::RichText::new(about)
                                    .font(theme::regular(13.0))
                                    .color(theme::MUTED),
                            );
                        });
                    });
                })
                .response
                .interact(egui::Sense::click());

            if response.clicked() {
                *kind = option;
            }

            ui.add_space(6.0);
        }

        ui.add_space(8.0);
    }

    widgets::field_label(
        ui,
        if *kind == ChannelKind::Category {
            "Nome da categoria"
        } else {
            "Nome do canal"
        },
        None,
    );
    ui.add_space(6.0);

    let field = widgets::text_field(
        ui,
        name,
        if *kind == ChannelKind::Category {
            "Nova categoria"
        } else {
            "novo-canal"
        },
        false,
    );
    let typed = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));

    ui.add_space(16.0);
    ui.horizontal(|ui| {
        if widgets::secondary_button(ui, "Cancelar") {
            app.actions.push(Action::CloseModal);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ready = !name.trim().is_empty();

            if widgets::primary_button(ui, "Criar", ready, 120.0) || (typed && ready) {
                app.actions.push(Action::CreateChannel);
            }
        });
    });
}

fn invite(app: &mut Unkvoid, ui: &mut Ui, server: i64) {
    let server_name = app
        .server_summary(server)
        .map_or("servidor", |server| server.name.as_str());
    let destination = app
        .reading
        .as_ref()
        .filter(|channel| channel.kind == ChannelKind::Text)
        .or_else(|| {
            app.tree.as_ref().and_then(|tree| {
                tree.channels
                    .iter()
                    .find(|channel| channel.kind == ChannelKind::Text)
            })
        });
    let subtitle = destination.map_or_else(
        || "Os destinatários chegarão ao servidor.".to_owned(),
        |channel| format!("Os destinatários chegarão em #{}", channel.name),
    );
    title(
        ui,
        &format!("Convidar amigos para {server_name}"),
        &subtitle,
    );
    ui.add_space(10.0);
    widgets::text_field(ui, &mut app.invite_search, "Buscar amigos", false);
    ui.add_space(8.0);
    let friends = app.friends.clone();
    let invite_url = app.server_invite.as_ref().map(|invite| invite.url.clone());
    egui::ScrollArea::vertical()
        .max_height(230.0)
        .show(ui, |ui| {
            let query = app.invite_search.to_lowercase();
            for friendship in friends.iter().filter(|friendship| {
                friendship.status == core_app::models::FriendshipStatus::Accepted
            }) {
                let person = if app.my_id() == Some(friendship.requester.id) {
                    &friendship.addressee
                } else {
                    &friendship.requester
                };
                if !query.is_empty() && !person.name.to_lowercase().contains(&query) {
                    continue;
                }
                let (row, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 48.0),
                    egui::Sense::hover(),
                );
                if response.hovered() {
                    ui.painter().rect_filled(row, theme::RADIUS, theme::HOVER);
                }
                let avatar = egui::Rect::from_center_size(
                    egui::pos2(row.left() + 24.0, row.center().y),
                    egui::vec2(32.0, 32.0),
                );
                widgets::paint_avatar(ui, avatar, &person.name, person.avatar_url.as_deref());
                ui.painter().text(
                    egui::pos2(avatar.right() + 10.0, row.center().y),
                    egui::Align2::LEFT_CENTER,
                    &person.name,
                    theme::regular(15.0),
                    theme::TEXT,
                );
                let sent = app.invite_sent.contains(&person.id);
                let button_rect = egui::Rect::from_center_size(
                    egui::pos2(row.right() - 48.0, row.center().y),
                    egui::vec2(88.0, 32.0),
                );
                let button = ui.put(
                    button_rect,
                    egui::Button::new(if sent { "Enviado" } else { "Convidar" })
                        .fill(if sent || invite_url.is_none() { theme::BUTTON } else { theme::ACCENT })
                        .corner_radius(theme::RADIUS)
                        .sense(if sent || invite_url.is_none() {
                            egui::Sense::hover()
                        } else {
                            egui::Sense::click()
                        }),
                );
                if button.clicked()
                    && let Some(link) = &invite_url
                {
                    app.actions.push(Action::SendDirectTo(
                        person.clone(),
                        link.clone(),
                    ));
                }
            }
        });
    ui.add_space(10.0);
    ui.label(
        egui::RichText::new("Ou, envie um convite do servidor a um amigo").font(theme::bold(13.0)),
    );
    ui.add_space(6.0);
    let paused = app.tree.as_ref().is_some_and(|tree| tree.id == server && tree.invites_paused);
    if paused {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Os convites deste servidor estão pausados.").color(theme::WARNING));
    } else if let Some(invite) = app.server_invite.clone() {
        let link = invite.url;
        Frame::new()
            .fill(theme::RAIL)
            .corner_radius(theme::RADIUS)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&link).font(theme::regular(13.0)))
                            .truncate(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::primary_button(ui, "Copiar", true, 90.0) {
                            app.actions.push(Action::CopyText(link.clone()));
                        }
                    });
                });
            });
        ui.horizontal(|ui| {
            let expiry = if app.invite_max_age == 0 { "nunca" } else { match app.invite_max_age { 1800 => "30 minutos", 3600 => "1 hora", 21600 => "6 horas", 43200 => "12 horas", 86400 => "1 dia", _ => "7 dias" } };
            ui.label(egui::RichText::new(format!("Seu link de convite expira em {expiry}. ")).font(theme::regular(12.0)).color(theme::DIM));
            if ui
                .add(
                    egui::Label::new(
                        egui::RichText::new("Editar link de convite")
                            .font(theme::regular(12.0))
                            .color(theme::ACCENT_TEXT),
                    )
                    .sense(egui::Sense::click()),
                )
                .clicked()
            {
                app.invite_editing = !app.invite_editing;
            }
        });
        if app.invite_editing {
            ui.add_space(8.0);
            egui::ComboBox::from_label("Expirar depois de")
                .selected_text(expiry_label(app.invite_max_age))
                .show_ui(ui, |ui| for (age, label) in [(1800, "30 minutos"), (3600, "1 hora"), (21600, "6 horas"), (43200, "12 horas"), (86400, "1 dia"), (604800, "7 dias"), (0, "Nunca")] { ui.selectable_value(&mut app.invite_max_age, age, label); });
            egui::ComboBox::from_label("Número máximo de usos")
                .selected_text(uses_label(app.invite_max_uses))
                .show_ui(ui, |ui| for (uses, label) in [(0, "Sem limite"), (1, "1"), (5, "5"), (10, "10"), (25, "25"), (50, "50"), (100, "100")] { ui.selectable_value(&mut app.invite_max_uses, uses, label); });
            ui.horizontal(|ui| {
                if widgets::secondary_button(ui, "Cancelar") { app.invite_editing = false; }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::primary_button(ui, "Gerar um novo link", true, 180.0) {
                        app.actions.push(Action::CreateServerInvite { server, max_age: app.invite_max_age, max_uses: app.invite_max_uses });
                    }
                });
            });
        }
    } else if !paused {
        ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(theme::DIM)));
    }
}

fn expiry_label(max_age: i64) -> &'static str {
    match max_age { 1800 => "30 minutos", 3600 => "1 hora", 21600 => "6 horas", 43200 => "12 horas", 86400 => "1 dia", 0 => "Nunca", _ => "7 dias" }
}

fn uses_label(max_uses: i64) -> String {
    if max_uses == 0 { "Sem limite".to_owned() } else { max_uses.to_string() }
}

fn leave_server(app: &mut Unkvoid, ui: &mut Ui, server: i64, name: &str) {
    title(ui, &format!("Sair de \"{name}\""), "");
    ui.label(
        egui::RichText::new(format!(
            "Tem certeza de que quer sair de {name}? Você só volta com outro convite."
        ))
        .color(theme::MUTED),
    );
    ui.add_space(20.0);
    ui.horizontal(|ui| {
        if widgets::secondary_button(ui, "Cancelar") {
            app.actions.push(Action::CloseModal);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::danger_button(ui, "Sair do servidor", true) {
                app.actions.push(Action::LeaveServer(server));
            }
        });
    });
}

pub(super) fn nickname_field(app: &mut Unkvoid, ui: &mut Ui) {
    widgets::field_label(ui, "Nome de usuário", app.nickname.error.as_deref());
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let field = ui.scope(|ui| {
            ui.set_max_width((ui.available_width() - 110.0).max(0.0));
            widgets::text_field(ui, &mut app.nickname.value, "seu.apelido", false)
        });
        let typed =
            field.inner.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        let ready = !app.nickname.busy && !app.nickname.value.trim().is_empty();

        if widgets::primary_button(
            ui,
            if app.nickname.busy {
                "Salvando…"
            } else {
                "Salvar"
            },
            ready,
            100.0,
        ) || (typed && ready)
        {
            app.actions.push(Action::Rename);
        }
    });
    ui.label(
        egui::RichText::new("3 a 32 caracteres: letras, números, ponto e sublinhado.")
            .font(theme::regular(12.0))
            .color(theme::DIM),
    );
}

/// O apelido automático precisa ser confirmado: o aviso não fecha até o servidor aceitar um,
/// mas dá para sair da conta por ele.
fn nickname(app: &mut Unkvoid, ui: &mut Ui) {
    window(ui, "nickname", 420.0, |ui| {
        title(
            ui,
            "Escolha como te chamar",
            "É o nome que aparece para todo mundo nos servidores. Dá para trocar depois.",
        );
        nickname_field(app, ui);
        ui.add_space(16.0);

        if ui
            .add(
                egui::Label::new(egui::RichText::new("Sair da conta").color(theme::DIM))
                    .sense(egui::Sense::click()),
            )
            .clicked()
        {
            app.actions.push(Action::SignOut);
        }
    });
}
