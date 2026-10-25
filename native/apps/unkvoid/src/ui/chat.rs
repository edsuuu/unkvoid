//! O chat: o do canal aberto e o da conversa direta, com o mesmo desenho de linha.
//!
//! As mensagens se agrupam como no Discord: a de quem acabou de falar (mesma pessoa, mesmo
//! dia, até 7 minutos depois) vem sem avatar nem nome, e o nome leva a cor do cargo mais alto.

use egui::{Align2, Color32, CornerRadius, Frame, Key, Modifiers, Rect, Sense, Ui, pos2, vec2};
use egui_phosphor::{fill as filled_icons, regular as icons};

use core_app::models::{ChannelKind, Message, ServerTree};
use core_app::permissions;

use super::{theme, widgets};
use crate::app::{Action, Unkvoid};
use crate::words::{clock, day};

/// Até quantos minutos depois a mensagem da mesma pessoa continua a anterior.
const GROUP_MINUTES: i64 = 7;

/// Uma linha do chat, venha ela de um canal ou de uma conversa direta.
pub struct Line<'a> {
    pub id: i64,
    pub author: &'a str,
    pub author_id: i64,
    pub avatar: Option<&'a str>,
    pub color: Option<Color32>,
    pub body: &'a str,
    pub at: &'a str,
    pub edited: bool,
    pub join: bool,
    pub reply: Option<(&'a str, &'a str)>,
    pub images: Vec<&'a str>,
}

/// O que se pode fazer com uma linha, decidido por quem desenha.
#[derive(Clone, Copy, Default)]
pub struct Abilities {
    pub edit: bool,
    pub delete: bool,
}

/// O chat do canal aberto. `with_header` é falso dentro da chamada, que tem o cabeçalho dela.
pub fn channel(app: &mut Unkvoid, ui: &mut Ui, with_header: bool) {
    let Some(reading) = app.reading.clone() else {
        nothing_open(ui);

        return;
    };
    let tree = app.tree.clone();

    if with_header {
        header(app, ui, &reading);
    }

    let can_send = permissions::has(reading.permissions, permissions::SEND_MESSAGES);

    egui::Panel::bottom("composer")
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(theme::MAIN).inner_margin(egui::Margin {
            left: 8,
            right: 8,
            top: 0,
            bottom: 8,
        }))
        .show(ui, |ui| {
            let hint = if can_send {
                format!(
                    "Conversar em {}{}",
                    if reading.kind == ChannelKind::Voice {
                        ""
                    } else {
                        "#"
                    },
                    reading.name
                )
            } else {
                "Você não tem permissão para enviar mensagens neste canal.".to_owned()
            };
            let mut text = std::mem::take(&mut app.composer);

            if can_send && !app.attachments.is_empty() {
                attachments(app, ui);
            }

            let composed = composer(ui, "channel-composer", &mut text, &hint, can_send, can_send);
            let has_content = !text.trim().is_empty() || !app.attachments.is_empty();

            app.composer = text;

            if composed.attach {
                app.actions.push(Action::AttachImage);
            }

            if composed.enter && has_content {
                app.actions.push(Action::SendMessage);
            }

            // O Ctrl+V de um print: sem texto na área de transferência o egui não vê colagem
            // nenhuma, só o V solto com o Ctrl. Vale com a caixa em foco ou sem campo nenhum em
            // foco, para não anexar um print colado em outro lugar.
            let nothing_focused = ui.memory(|memory| memory.focused().is_none());
            let pasted = ui.input(|input| {
                input.events.iter().any(|event| {
                    matches!(event, egui::Event::Key { key: Key::V, pressed: false, modifiers, .. } if modifiers.command)
                })
            });

            if can_send && pasted && (composed.focused || nothing_focused) {
                app.actions.push(Action::PasteImage);
            }
        });

    let me = app.my_id();
    let manage = permissions::has(reading.permissions, permissions::MANAGE_MESSAGES);
    let messages = app.messages.clone();
    let lines: Vec<Line<'_>> = messages
        .iter()
        .map(|message| line_of(message, tree.as_ref()))
        .collect();
    let beginning = app.beginning;
    let mut scroll = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true);

    // A página anterior entrou em cima: a tela volta para a mensagem que estava à vista, em vez
    // de pular para o alto. A altura nova só se sabe depois de um quadro desenhado com ela.
    if let (Some((height, offset)), Some(now)) = (app.scroll_anchor, app.chat_height)
        && now > height + 1.0
    {
        scroll = scroll.vertical_scroll_offset(now - height + offset);
        app.scroll_anchor = None;
    }

    let output = scroll.show(ui, |ui| {
        ui.add_space(16.0);

        if beginning {
            // O aviso de quem chegou não conta como a primeira mensagem do servidor.
            welcome(app, ui, &reading, tree.as_ref(), !messages.iter().any(|message| message.kind == "user"));
        } else if app.loading_older {
            ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(theme::DIM)));
        } else if messages.is_empty() {
            for row in 0..5 {
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
                ui.painter().circle_filled(rect.left_top() + vec2(32.0, 28.0), 20.0, theme::ELEVATED);
                let width = 90.0 + (row as f32 * 23.0) % 110.0;
                ui.painter().rect_filled(Rect::from_min_size(rect.left_top() + vec2(64.0, 16.0), vec2(width, 10.0)), CornerRadius::same(5), theme::ELEVATED);
                ui.painter().rect_filled(Rect::from_min_size(rect.left_top() + vec2(64.0, 36.0), vec2((rect.width() * 0.58).max(0.0), 10.0)), CornerRadius::same(5), theme::ELEVATED);
            }
        }

        let editing = app.editing.clone();

        for (index, line) in lines.iter().enumerate() {
            let previous = index.checked_sub(1).and_then(|index| lines.get(index));
            let abilities = Abilities {
                edit: Some(line.author_id) == me && !line.join,
                delete: Some(line.author_id) == me || manage,
            };

            if previous.is_none_or(|previous| previous.at.get(..10) != line.at.get(..10)) {
                day_divider(ui, line.at);
            }

            let continued = previous.is_some_and(|previous| continues(previous, line));
            let edit = editing
                .as_ref()
                .filter(|(id, _)| *id == line.id)
                .map(|(_, body)| body.clone());

            row(app, ui, line, continued, abilities, edit);
        }

        ui.add_space(12.0);
    });

    // Rolou até o alto e há mais para trás: busca a página anterior e guarda onde se estava,
    // para a tela não pular quando ela chegar.
    let at_top = output.state.offset.y < 40.0;

    app.chat_height = Some(output.content_size.y);

    if app.scroll_anchor.is_some() {
        ui.ctx().request_repaint();
    }

    if at_top && !beginning && !app.loading_older && messages.len() >= 50 {
        app.scroll_anchor_pending = Some((output.content_size.y, output.state.offset.y));
        app.actions.push(Action::LoadOlder);
    }
}

pub fn header(app: &mut Unkvoid, ui: &mut Ui, reading: &core_app::models::Channel) {
    egui::Panel::top("chat-header")
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

            // Dentro do painel, meio pixel acima da borda: na borda ela era cortada e sumia.
            ui.painter().hline(
                rect.x_range().expand(16.0),
                rect.bottom() - 0.5,
                egui::Stroke::new(1.0, theme::BORDER),
            );
            ui.horizontal_centered(|ui| {
                let icon = if reading.kind == ChannelKind::Voice {
                    filled_icons::SPEAKER_HIGH
                } else {
                    icons::HASH
                };
                let icon_font = if reading.kind == ChannelKind::Voice {
                    theme::filled(24.0)
                } else {
                    theme::icon(24.0)
                };

                ui.label(
                    egui::RichText::new(icon)
                        .font(icon_font)
                        .color(theme::MUTED),
                );
                ui.label(
                    egui::RichText::new(&reading.name)
                        .font(theme::bold(16.0))
                        .color(theme::BRIGHT),
                );

                if let Some(topic) = reading
                    .topic
                    .as_deref()
                    .filter(|topic| !topic.trim().is_empty())
                {
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(topic)
                                .font(theme::regular(14.0))
                                .color(theme::MUTED),
                        )
                        .truncate(),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let server = app
                        .tree
                        .as_ref()
                        .map(|tree| tree.name.clone())
                        .unwrap_or_default();

                    search_box(ui, &server);
                    ui.add_space(2.0);

                    let shown = if app.members_room { app.show_members } else { app.members_narrow };
                    let tip = if shown {
                        "Ocultar lista de membros"
                    } else {
                        "Mostrar lista de membros"
                    };

                    if header_icon(ui, filled_icons::USERS, tip, shown).clicked() {
                        app.actions.push(Action::ToggleMembers);
                    }

                    // ponytail: fixadas, notificações por canal e tópicos não existem ainda; os
                    // ícones ficam no lugar do Discord com o "em breve", e ganham a ação quando o
                    // servidor tiver a rota.
                    for (icon, tip) in [
                        (filled_icons::PUSH_PIN, "Mensagens fixadas: em breve"),
                        (filled_icons::BELL, "Notificações do canal: em breve"),
                        (filled_icons::CHATS_TEARDROP, "Tópicos: em breve"),
                    ] {
                        header_icon(ui, icon, tip, false);
                    }
                });
            });
        });
}

/// Os ícones da barra do canal: 20 px, sem fundo, cinza em repouso e acesos no hover ou quando
/// ligados, a 40 px um do outro, como no Discord.
fn header_icon(ui: &mut Ui, icon: &str, tip: &str, on: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
    let color = if on || response.hovered() {
        theme::BRIGHT
    } else {
        theme::DESCRIPTION
    };

    crate::ui::icons::text(ui.painter(), rect.center(), Align2::CENTER_CENTER, icon, theme::filled(20.0), color);

    response.on_hover_text(tip)
}

/// A busca da barra do canal, no tamanho e no lugar da do Discord.
// ponytail: a busca de mensagens ainda não existe no servidor; a caixa fica com o "em breve".
fn search_box(ui: &mut Ui, server: &str) {
    let (rect, response) = ui.allocate_exact_size(vec2(244.0, 32.0), Sense::hover());

    ui.painter().rect_filled(rect, theme::RADIUS, theme::SEARCH);
    ui.painter().rect_stroke(
        rect,
        theme::RADIUS,
        egui::Stroke::new(1.0, theme::BORDER),
        egui::StrokeKind::Inside,
    );

    let mut placeholder = egui::text::LayoutJob::simple_singleline(
        format!("Buscar {server}"),
        theme::regular(14.0),
        theme::DESCRIPTION,
    );

    placeholder.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 44.0);

    let galley = ui.painter().layout_job(placeholder);

    ui.painter().galley(
        pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0),
        galley,
        theme::DESCRIPTION,
    );
    crate::ui::icons::text(ui.painter(), pos2(rect.right() - 12.0, rect.center().y), Align2::RIGHT_CENTER, icons::MAGNIFYING_GLASS, theme::icon(18.0), theme::SECONDARY);
    response.on_hover_text("Busca: em breve");
}

/// Uma mensagem. A primeira de um grupo leva avatar, nome e hora; as seguintes, só o texto, com
/// a hora aparecendo à esquerda no hover. Os botões de editar e apagar flutuam no canto.
pub fn row(
    app: &mut Unkvoid,
    ui: &mut Ui,
    line: &Line<'_>,
    continued: bool,
    abilities: Abilities,
    edit: Option<String>,
) {
    let top = ui.cursor().top();
    let editing = edit.is_some();
    // O fundo do hover vai atrás do texto: a forma é reservada antes e preenchida depois, quando
    // a altura da mensagem já é conhecida.
    let background = ui.painter().add(egui::Shape::Noop);

    if !continued {
        ui.add_space(14.0);
    }

    let response = ui
        .horizontal_top(|ui| {
            ui.add_space(16.0);

            if line.join {
                arrival(app, ui, line);

                return;
            }

            if continued {
                ui.allocate_exact_size(vec2(40.0, 0.0), Sense::hover());
            } else {
                let avatar = widgets::avatar(ui, line.author, line.avatar, 40.0)
                    .on_hover_cursor(egui::CursorIcon::PointingHand);

                author_card(app, &avatar, line.author_id);
            }

            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 16.0).max(0.0));
                ui.spacing_mut().item_spacing.y = 2.0;

                if !continued {
                    if let Some((name, body)) = line.reply {
                        ui.label(
                            egui::RichText::new(format!("╭ {name}: {body}"))
                                .font(theme::regular(12.0))
                                .color(theme::DIM),
                        );
                    }

                    ui.horizontal(|ui| {
                        let author = ui
                            .add(
                                egui::Label::new(
                                    egui::RichText::new(line.author)
                                        .font(theme::bold(15.0))
                                        .color(line.color.unwrap_or(theme::TEXT)),
                                )
                                .sense(Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);

                        author_card(app, &author, line.author_id);
                        ui.label(
                            egui::RichText::new(stamp(line.at))
                                .font(theme::regular(12.0))
                                .color(theme::DIM),
                        );
                    });
                }

                match edit {
                    Some(mut body) => {
                        let field = editor(app, ui, &mut body);

                        app.editing = Some((line.id, body));

                        if field {
                            app.actions.push(Action::SaveEdit);
                        }
                    }
                    None => {
                        let mut text = egui::text::LayoutJob::default();

                        text.append(
                            line.body,
                            0.0,
                            egui::TextFormat {
                                font_id: theme::regular(15.0),
                                color: theme::TEXT,
                                ..Default::default()
                            },
                        );

                        if line.edited {
                            text.append(
                                " (editado)",
                                0.0,
                                egui::TextFormat {
                                    font_id: theme::regular(11.0),
                                    color: theme::DIM,
                                    ..Default::default()
                                },
                            );
                        }

                        if !line.body.is_empty() || line.edited {
                            if line.body.contains("http://") || line.body.contains("https://") {
                                message_body(app, ui, line.body);
                                if line.edited {
                                    ui.label(egui::RichText::new("(editado)").font(theme::regular(11.0)).color(theme::DIM));
                                }
                            } else {
                                ui.add(egui::Label::new(text).wrap().selectable(true));
                            }
                        }
                    }
                }

                if let Some(code) = invite_code_in_message(line.body, &crate::system::server()) {
                    invite_card(app, ui, &code);
                }

                for image in &line.images {
                    ui.add(
                        egui::Image::new(image.to_owned())
                            .max_size(vec2(400.0, 300.0))
                            .corner_radius(CornerRadius::same(6)),
                    );
                }
            });
        })
        .response;

    let rect = Rect::from_min_max(
        pos2(
            ui.max_rect().left(),
            top + if continued { 0.0 } else { 10.0 },
        ),
        pos2(ui.max_rect().right(), response.rect.bottom() + 2.0),
    );
    let action_rect = (!line.join && !editing)
        .then(|| actions_bar_rect(rect, abilities, app.editing.is_none()));
    // A barra é uma camada própria por cima da lista: o `rect_contains_pointer` da lista dá falso
    // sobre ela, e a barra sumia quando o mouse chegava nos botões.
    let bar_layer = egui::LayerId::new(egui::Order::Foreground, actions_id(ui, line));
    let on_bar = ui.ctx().pointer_hover_pos().is_some_and(|pointer| {
        action_rect.is_some_and(|action_rect| action_rect.contains(pointer))
            && ui.ctx().layer_id_at(pointer) == Some(bar_layer)
    });
    let hovered = ui.rect_contains_pointer(rect) || on_bar;
    // O texto selecionável por cima fica com o clique direito, e o `context_menu` da linha nunca
    // o veria: o clique direito dentro da mensagem abre o menu dela no ponteiro.
    let right_clicked =
        !line.join && hovered && ui.input(|input| input.pointer.secondary_clicked());

    egui::Popup::menu(&response)
        .open_memory(right_clicked.then_some(egui::SetOpenCommand::Bool(true)))
        .at_pointer_fixed()
        .frame(widgets::menu_frame())
        .show(|ui| message_menu(app, ui, line, abilities));

    if hovered && !line.join {
        ui.painter().set(
            background,
            egui::epaint::RectShape::filled(
                rect,
                CornerRadius::ZERO,
                theme::ELEVATED.gamma_multiply(0.6),
            ),
        );

        if continued {
            ui.painter().text(
                pos2(rect.left() + 36.0, rect.top() + 11.0),
                Align2::CENTER_CENTER,
                clock(line.at),
                theme::regular(11.0),
                theme::DIM,
            );
        }

        actions_bar(app, ui, rect, line, abilities);
    }
}

fn message_body(app: &mut Unkvoid, ui: &mut Ui, body: &str) {
    ui.horizontal_wrapped(|ui| {
        for (text, link) in message_link_parts(body) {
            if let Some(url) = link {
                let response = ui
                    .add(egui::Label::new(
                        egui::RichText::new(text).font(theme::regular(15.0)).color(theme::ACCENT_TEXT),
                    ).sense(Sense::click()))
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if response.hovered() {
                    ui.painter().hline(
                        response.rect.x_range(),
                        response.rect.bottom() - 1.0,
                        egui::Stroke::new(1.0, theme::ACCENT_TEXT),
                    );
                }
                let response = if response.hovered() {
                    response.on_hover_text(&url)
                } else {
                    response
                };
                if response.clicked() {
                    app.actions.push(Action::OpenUrl(url));
                }
            } else {
                ui.label(egui::RichText::new(text).font(theme::regular(15.0)).color(theme::TEXT));
            }
        }
    });
}

fn message_link_parts(body: &str) -> Vec<(String, Option<String>)> {
    let mut parts = Vec::new();
    let mut cursor = 0;

    for (start, word) in body.match_indices(|character: char| character.is_whitespace()) {
        if start > cursor {
            append_link_part(&mut parts, &body[cursor..start]);
        }
        parts.push((word.to_owned(), None));
        cursor = start + word.len();
    }
    if cursor < body.len() {
        append_link_part(&mut parts, &body[cursor..]);
    }

    parts
}

fn append_link_part(parts: &mut Vec<(String, Option<String>)>, word: &str) {
    let start = [word.find("https://"), word.find("http://")].into_iter().flatten().min();
    if let Some(start) = start {
        if start > 0 {
            parts.push((word[..start].to_owned(), None));
        }
        let token = &word[start..];
        let trailing_length = token.len() - token.trim_end_matches(['.', ',', '!', '?', ';', ':', ')', ']', '}', '\"', '\'']).len();
        let (url, trailing) = token.split_at(token.len() - trailing_length);
        if !url.is_empty() {
            parts.push((url.to_owned(), Some(url.to_owned())));
        }
        if !trailing.is_empty() {
            parts.push((trailing.to_owned(), None));
        }
    } else {
        parts.push((word.to_owned(), None));
    }
}

/// O código do primeiro convite deste site (`<servidor>/convite/<código>`) na mensagem: o
/// convite de outro endereço é só um link.
fn invite_code_in_message(body: &str, server: &str) -> Option<String> {
    message_link_parts(body).into_iter().find_map(|(_, link)| link.and_then(|url| invite_code_from_url(&url, server)))
}

fn invite_code_from_url(url: &str, server: &str) -> Option<String> {
    let path = url.strip_prefix(server.trim_end_matches('/'))?;
    let code = path.strip_prefix("/convite/")?.split(['?', '#', '/']).next()?.trim_end_matches(['.', ',', '!', '?', ';', ':', ')', ']', '}']);

    (!code.is_empty() && code.chars().all(|character| character.is_ascii_alphanumeric() || character == '-'))
        .then(|| code.to_owned())
}

fn invite_card(app: &mut Unkvoid, ui: &mut Ui, code: &str) {
    let fresh = app.invite_previews.get(code).is_some_and(|(at, _)| at.elapsed() < std::time::Duration::from_secs(60));
    if !fresh && !app.invite_preview_pending.contains(code) {
        app.actions.push(Action::LoadInvitePreview(code.to_owned()));
    }
    let Some((_, result)) = app.invite_previews.get(code) else {
        return;
    };
    let preview = match result {
        Ok(preview) => preview.clone(),
        Err(core_app::api::HttpError::Failed(core_app::failure::Failure::Gone)) => {
        Frame::new()
            .fill(theme::MAIN)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Você enviou um convite, mas").font(theme::regular(14.0)).color(theme::DESCRIPTION));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    crate::ui::icons::label(ui, icons::ENVELOPE_SIMPLE, theme::icon(28.0), theme::DANGER);
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Convite inválido").font(theme::bold(16.0)).color(theme::DANGER));
                        ui.label(egui::RichText::new("Tente enviar um novo convite").font(theme::regular(13.0)).color(theme::DESCRIPTION));
                    });
                });
            });
        return;
        }
        Err(_) => return,
    };

    Frame::new()
        .fill(theme::ELEVATED)
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(egui::RichText::new("VOCÊ FOI CONVIDADO(A) PARA ENTRAR EM UM SERVIDOR")
                .font(theme::bold(11.0)).color(theme::DESCRIPTION));
            ui.horizontal(|ui| {
                let (_, avatar) = ui.allocate_space(vec2(48.0, 48.0));
                widgets::paint_avatar(ui, avatar, &preview.server.name, preview.server.icon_url.as_deref());
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(&preview.server.name).font(theme::bold(16.0)).color(theme::BRIGHT));
                    let members = preview.members;
                    let suffix = if members == 1 { "membro" } else { "membros" };
                    ui.label(egui::RichText::new(format!("{members} {suffix}")).font(theme::regular(13.0)).color(theme::DESCRIPTION));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let joined = app.servers.iter().any(|server| server.id == preview.server.id);
                    let label = if joined { "Entrou" } else { "Entrar" };
                    let button = egui::Button::new(egui::RichText::new(label).font(theme::bold(14.0)).color(theme::BRIGHT))
                        .fill(if joined { theme::BUTTON } else { theme::SUCCESS })
                        .corner_radius(CornerRadius::same(6));
                    if ui.add_enabled(!joined, button).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        app.actions.push(Action::JoinInviteCode(code.to_owned()));
                    }
                });
            });
        });
}

/// O cartão de perfil de quem escreveu, ao lado do nome ou do avatar clicado, como no Discord.
/// Fora de um servidor (nas conversas diretas) não há cartão de membro.
fn author_card(app: &mut Unkvoid, response: &egui::Response, author: i64) {
    let Some(tree) = app.tree.clone() else {
        return;
    };
    let Some(member) = tree.members.iter().find(|member| member.user_id == author).cloned() else {
        return;
    };

    egui::Popup::menu(response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::RIGHT_START)
        .gap(8.0)
        .frame(super::members::card_frame())
        .show(|ui| super::members::profile_card(app, ui, &tree, &member, false));
}

/// O menu do clique direito na mensagem: o que dá para fazer com ela aqui.
fn message_menu(app: &mut Unkvoid, ui: &mut Ui, line: &Line<'_>, abilities: Abilities) {
    if widgets::menu_item(ui, icons::COPY, "Copiar texto", false) {
        app.actions.push(Action::CopyText(line.body.to_owned()));
        ui.close();
    }

    if abilities.edit && app.editing.is_none() && widgets::menu_item(ui, icons::PENCIL_SIMPLE, "Editar mensagem", false) {
        app.actions
            .push(Action::StartEdit(line.id, line.body.to_owned()));
        ui.close();
    }

    if abilities.delete && widgets::menu_item(ui, icons::TRASH, "Apagar mensagem", true) {
        app.actions.push(Action::DeleteMessage(line.id));
        ui.close();
    }

    widgets::menu_separator(ui);

    if widgets::menu_item(ui, icons::IDENTIFICATION_CARD, "Copiar ID da mensagem", false) {
        app.actions.push(Action::CopyText(line.id.to_string()));
        ui.close();
    }
}

/// Os botões que aparecem no canto da mensagem sob o cursor.
fn actions_bar(app: &mut Unkvoid, ui: &mut Ui, rect: Rect, line: &Line<'_>, abilities: Abilities) {
    let mut buttons: Vec<(&str, &str, Action)> = vec![(
        icons::COPY,
        "Copiar texto",
        Action::CopyText(line.body.to_owned()),
    )];

    if abilities.edit && app.editing.is_none() {
        buttons.push((
            icons::PENCIL_SIMPLE,
            "Editar",
            Action::StartEdit(line.id, line.body.to_owned()),
        ));
    }

    if abilities.delete {
        buttons.push((
            icons::TRASH,
            "Apagar mensagem",
            Action::DeleteMessage(line.id),
        ));
    }

    let bar = actions_bar_rect(rect, abilities, app.editing.is_none());

    egui::Area::new(actions_id(ui, line))
        .order(egui::Order::Foreground)
        .fixed_pos(bar.min)
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(theme::MAIN)
                .stroke(egui::Stroke::new(1.0, theme::DIVIDER))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin::same(2))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;

                        for (icon, tip, action) in buttons {
                            let color = if matches!(action, Action::DeleteMessage(_)) {
                                theme::DANGER
                            } else {
                                theme::MUTED
                            };

                            if widgets::icon_button(ui, icon, tip, color).clicked() {
                                app.actions.push(action);
                            }
                        }
                    });
                });
        });
}

fn actions_id(ui: &Ui, line: &Line<'_>) -> egui::Id {
    ui.id().with(("actions", line.id))
}

fn actions_bar_rect(rect: Rect, abilities: Abilities, editable: bool) -> Rect {
    let button_count = 1 + usize::from(abilities.edit && editable) + usize::from(abilities.delete);
    let width = 32.0 * button_count as f32 + 8.0;

    Rect::from_min_size(
        pos2(rect.right() - width - 16.0, rect.top() - 14.0),
        vec2(width, 32.0),
    )
}

/// A caixa de escrever: Enter envia, Shift+Enter quebra a linha. Devolve se enviou.
/// O que aconteceu na caixa de mensagem neste quadro.
pub struct Composed {
    /// Enter com a caixa em foco: quem chama decide se há o que mandar.
    pub enter: bool,
    /// O "+" de anexar imagem.
    pub attach: bool,
    pub focused: bool,
}

/// A caixa de escrever: Enter envia, Shift+Enter quebra a linha. `attachable` acende o "+" de
/// anexar imagem; nas conversas diretas ele fica apagado, porque o servidor não leva imagem lá.
pub fn composer(
    ui: &mut Ui,
    id: &str,
    text: &mut String,
    hint: &str,
    enabled: bool,
    attachable: bool,
) -> Composed {
    let id = ui.id().with(id);

    if ui.ctx().data_mut(|data| data.remove_temp::<bool>(egui::Id::new(FOCUS_COMPOSER))).unwrap_or(false) {
        ui.memory_mut(|memory| memory.request_focus(id));
    }

    let focused = ui.memory(|memory| memory.has_focus(id));
    let enter =
        focused && enabled && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));
    let mut attach = false;

    // A caixa do Discord: 56 de altura, cantos de 8 com a borda fina, o "+" na esquerda, o texto
    // de 16 e os ícones na direita.
    Frame::new()
        .fill(theme::COMPOSER)
        .stroke(egui::Stroke::new(1.0, theme::COMPOSER_BORDER))
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin {
            left: 16,
            right: 16,
            top: 16,
            bottom: 16,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                // O "+" do Discord é de traço grosso, e o do Phosphor é fino: desenhado à mão.
                let (plus, plus_response) = ui.allocate_exact_size(
                    vec2(24.0, 24.0),
                    if attachable { Sense::click() } else { Sense::hover() },
                );
                let stroke = egui::Stroke::new(
                    2.0,
                    if !attachable {
                        theme::DIM
                    } else if plus_response.hovered() {
                        theme::BRIGHT
                    } else {
                        theme::SECONDARY
                    },
                );

                attach = plus_response
                    .on_hover_text(if attachable {
                        "Enviar uma imagem"
                    } else {
                        "A conversa direta ainda não leva imagem"
                    })
                    .clicked();

                ui.painter().line_segment(
                    [plus.center() - vec2(8.0, 0.0), plus.center() + vec2(8.0, 0.0)],
                    stroke,
                );
                ui.painter().line_segment(
                    [plus.center() - vec2(0.0, 8.0), plus.center() + vec2(0.0, 8.0)],
                    stroke,
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;

                    // ponytail: GIF, figurinha, emoji e apps ainda não existem; os ícones ficam no
                    // lugar do Discord com o "em breve" e ganham ação quando existirem.
                    for (icon, tip) in [
                        (filled_icons::SQUARES_FOUR, "Apps: em breve"),
                        (filled_icons::SMILEY, "Emoji: em breve"),
                        (filled_icons::STICKER, "Figurinhas: em breve"),
                        (filled_icons::GIF, "GIFs: em breve"),
                    ] {
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(32.0, 24.0), Sense::hover());

                        crate::ui::icons::text(ui.painter(), rect.center(), Align2::CENTER_CENTER, icon, theme::filled(22.0), if response.hovered() {
                                theme::BRIGHT
                            } else {
                                theme::SECONDARY
                            });
                        response.on_hover_text(tip);
                    }

                    ui.add_space(8.0);
                    ui.add_enabled(
                        enabled,
                        egui::TextEdit::multiline(text)
                            .id(id)
                            .hint_text(
                                egui::RichText::new(hint)
                                    .font(theme::regular(16.0))
                                    .color(theme::PLACEHOLDER),
                            )
                            .frame(egui::Frame::NONE)
                            .font(theme::regular(16.0))
                            .desired_rows(1)
                            .desired_width(ui.available_width()),
                    );
                });
            });
        });

    Composed {
        enter,
        attach,
        focused,
    }
}

/// As imagens anexadas, em cima da caixa de mensagem: a miniatura, o nome e o X para tirar.
fn attachments(app: &mut Unkvoid, ui: &mut Ui) {
    let mut removed = None;

    Frame::new()
        .fill(theme::COMPOSER)
        .stroke(egui::Stroke::new(1.0, theme::COMPOSER_BORDER))
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;

                for (index, (path, preview)) in app.attachments.iter().enumerate() {
                    let (card, _) = ui.allocate_exact_size(vec2(160.0, 150.0), Sense::hover());
                    let picture = Rect::from_min_size(card.min + vec2(8.0, 8.0), vec2(144.0, 108.0));

                    ui.painter().rect_filled(card, theme::RADIUS, theme::ELEVATED);

                    let shown = super::call::fit(preview.size_vec2(), picture);

                    egui::Image::new((preview.id(), shown.size()))
                        .corner_radius(CornerRadius::same(4))
                        .paint_at(ui, shown);

                    let mut name = egui::text::LayoutJob::simple_singleline(
                        path.file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        theme::regular(13.0),
                        theme::TEXT,
                    );

                    name.wrap = egui::text::TextWrapping::truncate_at_width(144.0);

                    let galley = ui.painter().layout_job(name);

                    ui.painter().galley(
                        pos2(card.left() + 8.0, picture.bottom() + 6.0),
                        galley,
                        theme::TEXT,
                    );

                    let close = Rect::from_center_size(card.right_top() + vec2(-14.0, 14.0), vec2(24.0, 24.0));
                    let closing = ui
                        .interact(close, ui.id().with(("remove-attachment", index)), Sense::click())
                        .on_hover_text("Remover anexo");

                    ui.painter().circle_filled(
                        close.center(),
                        12.0,
                        if closing.hovered() { theme::DANGER } else { theme::RAIL },
                    );
                    crate::ui::icons::text(ui.painter(), close.center(), Align2::CENTER_CENTER, icons::TRASH, theme::icon(14.0), theme::BRIGHT);

                    if closing.clicked() {
                        removed = Some(index);
                    }
                }
            });
        });
    ui.add_space(8.0);

    if let Some(index) = removed {
        app.actions.push(Action::RemoveAttachment(index));
    }
}

/// O campo da edição: Enter salva, Esc cancela. Devolve se salvou.
fn editor(app: &mut Unkvoid, ui: &mut Ui, body: &mut String) -> bool {
    let id = ui.id().with("editing");
    let saved = ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));

    if ui.input(|input| input.key_pressed(Key::Escape)) {
        app.actions.push(Action::CancelEdit);
    }

    let field = Frame::new()
        .fill(theme::ELEVATED)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(body)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .font(theme::regular(15.0))
                    .desired_rows(1)
                    .desired_width(f32::INFINITY),
            )
        });

    field.inner.request_focus();
    ui.label(
        egui::RichText::new("Esc para cancelar • Enter para salvar")
            .font(theme::regular(12.0))
            .color(theme::DIM),
    );

    saved
}

fn day_divider(ui: &mut Ui, at: &str) {
    let label = day(at);

    if label.is_empty() {
        return;
    }

    ui.add_space(12.0);

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
    let galley = ui
        .painter()
        .layout_no_wrap(label, theme::bold(12.0), theme::DIM);
    let middle = rect.center();
    let half = galley.size().x / 2.0 + 8.0;

    ui.painter().hline(
        (rect.left() + 16.0)..=(middle.x - half),
        middle.y,
        egui::Stroke::new(1.0, theme::DIVIDER),
    );
    ui.painter().hline(
        (middle.x + half)..=(rect.right() - 16.0),
        middle.y,
        egui::Stroke::new(1.0, theme::DIVIDER),
    );
    ui.painter()
        .galley(middle - galley.size() / 2.0, galley, theme::DIM);
}

/// O começo do canal, quando não há mais nada para trás.
/// Quem chegou ao servidor, como o Discord escreve: a seta verde na coluna das fotos, uma frase
/// com o nome em destaque e o botão de acenar. As frases são nossas, escolhidas pela mensagem
/// para não mudarem a cada quadro.
fn arrival(app: &mut Unkvoid, ui: &mut Ui, line: &Line<'_>) {
    const PHRASES: [(&str, &str); 5] = [
        ("", " chegou. Sinta-se em casa!"),
        ("Olha quem apareceu: ", "!"),
        ("", " acabou de entrar no servidor."),
        ("Puxe uma cadeira, ", "!"),
        ("", " pousou por aqui."),
    ];
    let (before, after) = PHRASES[usize::try_from(line.id.rem_euclid(PHRASES.len() as i64)).unwrap_or(0)];
    let mut sentence = egui::text::LayoutJob::default();
    let plain = egui::TextFormat { font_id: theme::regular(16.0), color: theme::MUTED, ..Default::default() };

    sentence.append(before, 0.0, plain.clone());
    sentence.append(line.author, 0.0, egui::TextFormat { font_id: theme::bold(16.0), color: theme::BRIGHT, ..Default::default() });
    sentence.append(after, 0.0, plain);
    sentence.append(&format!("  {}", clock(line.at)), 0.0, egui::TextFormat { font_id: theme::regular(12.0), color: theme::DIM, ..Default::default() });

    let (arrow, _) = ui.allocate_exact_size(vec2(40.0, 22.0), Sense::hover());

    crate::ui::icons::text(ui.painter(), arrow.center(), Align2::CENTER_CENTER, icons::ARROW_RIGHT, theme::icon(20.0), theme::SUCCESS);
    ui.add_space(16.0);
    ui.vertical(|ui| {
        ui.label(sentence);
        ui.add_space(6.0);

        let wave = egui::Button::new(egui::RichText::new("👋  Acene para dizer olá!").font(theme::regular(15.0)).color(theme::BRIGHT))
            .fill(theme::BUTTON)
            .corner_radius(theme::RADIUS)
            .min_size(vec2(0.0, 36.0));

        if ui.add(wave).clicked() {
            app.actions.push(Action::Wave);
        }
    });
}

/// O começo do canal. No primeiro canal de texto do servidor o Discord mostra as boas-vindas do
/// servidor, centralizadas, com os primeiros passos; nos outros, o começo do canal, contido.
fn welcome(app: &mut Unkvoid, ui: &mut Ui, reading: &core_app::models::Channel, tree: Option<&ServerTree>, empty: bool) {
    let first = tree.and_then(|tree| tree.sections().into_iter().flat_map(|section| section.channels).find(|channel| channel.kind == ChannelKind::Text));

    match tree {
        Some(tree) if first.is_some_and(|first| first.id == reading.id) => server_welcome(app, ui, tree, empty),
        _ => channel_welcome(ui, &reading.name, reading.kind == ChannelKind::Voice),
    }
}

/// "Bem-vindo(a) a <servidor>" no meio do chat e os cartões dos primeiros passos de quem pode
/// dá-los: convidar, pôr o ícone, mandar a primeira mensagem. O feito fica com o visto verde.
fn server_welcome(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree, empty: bool) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.18).clamp(24.0, 160.0));
        ui.label(egui::RichText::new("Bem-vindo(a) a").font(theme::bold(32.0)).color(theme::BRIGHT));
        ui.label(egui::RichText::new(&tree.name).font(theme::bold(32.0)).color(theme::BRIGHT));
        ui.add_space(8.0);
        ui.scope(|ui| {
            ui.set_max_width(400.0_f32.min(ui.available_width() - 32.0));
            ui.label(egui::RichText::new("Este é o começo do servidor. Aqui vão alguns passos para deixar ele com a cara de vocês.").font(theme::regular(15.0)).color(theme::TEXT));
        });
        ui.add_space(18.0);

        let steps = [
            (tree.can(permissions::CREATE_INVITE), icons::USER_PLUS, "Convide seus amigos", false, Some(Action::Show(crate::app::Modal::Invite { server: tree.id, code: None }))),
            (
                tree.can(permissions::MANAGE_SERVER),
                icons::IMAGE,
                "Personalize seu servidor com um ícone",
                tree.icon_url.is_some(),
                Some(Action::Show(crate::app::Modal::ServerSettings(crate::app::ServerSettings::new(tree, crate::app::ServerPage::Overview)))),
            ),
            (true, icons::PAPER_PLANE_TILT, "Envie sua primeira mensagem", !empty, None),
        ];

        for (allowed, icon, label, done, action) in steps {
            if allowed && step_card(ui, icon, label, done) {
                match action {
                    Some(action) => app.actions.push(action),
                    // O cartão de mandar mensagem leva o cursor para a caixa de texto.
                    None => {
                        ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new(FOCUS_COMPOSER), true));
                    }
                }
            }
        }

        ui.add_space(24.0);
    });
}

/// O sinal que o cartão "Envie sua primeira mensagem" deixa para a caixa de texto pegar o foco.
const FOCUS_COMPOSER: &str = "focus-composer";

fn step_card(ui: &mut Ui, icon: &str, label: &str, done: bool) -> bool {
    // Até 400 px como no Discord, mas cabe no chat estreito: maior, o meio do layout sai da tela.
    let width = 400.0_f32.min(ui.available_width() - 32.0).max(220.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 56.0), Sense::click());
    let fill = if response.hovered() && !done { theme::SELECTED } else { theme::ELEVATED };

    ui.painter().rect_filled(rect, theme::RADIUS, fill);
    ui.painter().rect_stroke(rect, theme::RADIUS, egui::Stroke::new(1.0, theme::DIVIDER), egui::StrokeKind::Inside);
    crate::ui::icons::text(ui.painter(), pos2(rect.left() + 26.0, rect.center().y), Align2::CENTER_CENTER, icon, theme::icon(22.0), if done { theme::MUTED } else { theme::ACCENT_TEXT });
    let label = super::sidebar::elide(ui, label, theme::bold(15.0), rect.width() - 100.0);

    ui.painter().text(pos2(rect.left() + 52.0, rect.center().y), Align2::LEFT_CENTER, label, theme::bold(15.0), if done { theme::MUTED } else { theme::BRIGHT });

    if done {
        ui.painter().circle_filled(pos2(rect.right() - 28.0, rect.center().y), 11.0, theme::SUCCESS);
        crate::ui::icons::text(ui.painter(), pos2(rect.right() - 28.0, rect.center().y), Align2::CENTER_CENTER, icons::CHECK, theme::icon(14.0), Color32::WHITE);
    } else {
        crate::ui::icons::text(ui.painter(), pos2(rect.right() - 28.0, rect.center().y), Align2::CENTER_CENTER, icons::CARET_RIGHT, theme::icon(18.0), theme::TEXT);
    }

    ui.add_space(8.0);

    response.clicked() && !done
}

fn channel_welcome(ui: &mut Ui, name: &str, voice: bool) {
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "Boas-vindas a {}{name}!",
                    if voice { "" } else { "#" }
                ))
                .font(theme::bold(26.0))
                .color(theme::BRIGHT),
            );
            ui.label(
                egui::RichText::new(format!(
                    "Este é o começo do canal {}{name}.",
                    if voice { "" } else { "#" }
                ))
                .color(theme::MUTED),
            );
        });
    });
    ui.add_space(8.0);
}

fn nothing_open(ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 2.0 - 40.0).max(0.0));
        crate::ui::icons::label(ui, icons::CHAT_CIRCLE_DOTS, theme::icon(48.0), theme::DIM);
        ui.label(
            egui::RichText::new("Nenhum canal de texto que você possa ver por aqui.")
                .color(theme::MUTED),
        );
    });
}

/// A hora do cabeçalho do grupo: "Hoje às", "Ontem às" ou a data, como o Discord.
fn stamp(at: &str) -> String {
    let today = chrono::Local::now().date_naive();
    let date = chrono::DateTime::parse_from_rfc3339(at)
        .map(|stamp| stamp.with_timezone(&chrono::Local).date_naive());

    match date {
        Ok(date) if date == today => format!("Hoje às {}", clock(at)),
        Ok(date) if today.pred_opt() == Some(date) => format!("Ontem às {}", clock(at)),
        Ok(date) => format!("{} {}", date.format("%d/%m/%Y"), clock(at)),
        Err(_) => clock(at).to_owned(),
    }
}

fn line_of<'a>(message: &'a Message, tree: Option<&ServerTree>) -> Line<'a> {
    let color = tree.and_then(|tree| {
        let member = tree
            .members
            .iter()
            .find(|member| member.user_id == message.user.id)?;

        core_app::members::top_role(tree, member)?
            .color
            .as_deref()
            .and_then(theme::color_of)
    });

    Line {
        id: message.id,
        author: &message.user.name,
        author_id: message.user.id,
        avatar: message.user.avatar_url.as_deref(),
        color,
        body: &message.body,
        at: &message.created_at,
        edited: message.edited_at.is_some(),
        join: message.kind == "join",
        reply: message
            .reply_to
            .as_ref()
            .map(|reply| (reply.name.as_str(), reply.body.as_str())),
        images: message
            .files
            .iter()
            .filter(|file| file.mime_type.starts_with("image/"))
            .map(|file| file.url.as_str())
            .collect(),
    }
}

/// A mensagem continua a anterior: mesma pessoa, mesmo tipo, mesmo dia, menos de 7 minutos
/// depois.
pub fn continues(previous: &Line<'_>, line: &Line<'_>) -> bool {
    let minute_of = |stamp: &str| -> Option<i64> {
        let hours: i64 = stamp.get(11..13)?.parse().ok()?;
        let minutes: i64 = stamp.get(14..16)?.parse().ok()?;

        Some(hours * 60 + minutes)
    };

    previous.author_id == line.author_id
        && previous.join == line.join
        && !line.join
        && line.reply.is_none()
        && previous.at.get(..10) == line.at.get(..10)
        && matches!((minute_of(previous.at), minute_of(line.at)), (Some(before), Some(now)) if (0..GROUP_MINUTES).contains(&(now - before)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line<'a>(author_id: i64, at: &'a str) -> Line<'a> {
        Line {
            id: 1,
            author: "Ana",
            author_id,
            avatar: None,
            color: None,
            body: "oi",
            at,
            edited: false,
            join: false,
            reply: None,
            images: Vec::new(),
        }
    }

    #[test]
    fn the_same_person_within_seven_minutes_continues_the_group() {
        let first = line(1, "2026-10-12T17:02:00-03:00");

        assert!(continues(&first, &line(1, "2026-10-12T17:08:59-03:00")));
        assert!(
            !continues(&first, &line(1, "2026-10-12T17:09:00-03:00")),
            "sete minutos já é outro grupo"
        );
        assert!(
            !continues(&first, &line(2, "2026-10-12T17:03:00-03:00")),
            "outra pessoa abre outro grupo"
        );
        assert!(
            !continues(&first, &line(1, "2026-10-13T17:03:00-03:00")),
            "outro dia abre outro grupo"
        );
    }

    #[test]
    fn a_reply_always_opens_a_group_to_show_what_it_answers() {
        let first = line(1, "2026-10-12T17:02:00-03:00");
        let mut reply = line(1, "2026-10-12T17:03:00-03:00");

        reply.reply = Some(("Bia", "e aí"));

        assert!(!continues(&first, &reply));
    }

    #[test]
    fn the_message_hover_region_includes_the_floating_actions_bar() {
        let message = Rect::from_min_size(pos2(0.0, 40.0), vec2(600.0, 72.0));
        let actions = actions_bar_rect(message, Abilities { edit: true, delete: true }, true);
        let pointer = pos2(actions.center().x, actions.top() + 2.0);

        assert!(!message.contains(pointer), "o botão fica fora do retângulo da mensagem");
        assert!(actions.contains(pointer), "o hover continua ativo sobre a barra flutuante");
    }

    #[test]
    fn finds_urls_and_trims_sentence_punctuation_from_invite_codes() {
        let text = "Veja https://unkvoid.com/convite/abc123). e https://example.org/pagina!";
        let links: Vec<String> = message_link_parts(text).into_iter().filter_map(|(_, link)| link).collect();

        assert_eq!(links, vec![
            "https://unkvoid.com/convite/abc123".to_owned(),
            "https://example.org/pagina".to_owned(),
        ]);
        assert_eq!(invite_code_in_message(text, "https://unkvoid.com").as_deref(), Some("abc123"));
        assert_eq!(invite_code_from_url("https://example.org/convite/nope", "https://unkvoid.com"), None);
        assert_eq!(invite_code_from_url("http://127.0.0.1:8000/convite/YyOwPta9", "http://127.0.0.1:8000/").as_deref(), Some("YyOwPta9"));
    }
}
