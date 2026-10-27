//! O palco da chamada: cartões de quem está na voz e controles que aparecem sob o ponteiro.
//! Quem liga a câmera aparece em vídeo no próprio cartão; quem transmite vira o cartão da tela
//! dele, e a que ninguém abriu ainda mostra o "Assistir transmissão". Clicar num vídeo o põe no
//! foco.

use super::widgets::TipAbove;
use egui::{Align2, Color32, CornerRadius, Frame, Rect, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use egui_phosphor::fill as icons;

use core_app::models::{Channel, VoicePerson};

use super::{chat, icons as discord_icons, sidebar, theme, widgets};
use crate::app::{Action, GoLiveTab, Modal, Unkvoid};

const TILE_GAP: f32 = 8.0;

/// Um cartão do palco: uma pessoa (em vídeo, se a câmera dela está ligada) ou uma transmissão.
#[derive(Clone)]
enum Card {
    Person {
        person: VoicePerson,
        camera: Option<String>,
    },
    Stream {
        producer: String,
        name: String,
        /// Quem transmite, com o mudo já resolvido: o cartão da tela é o cartão da pessoa.
        person: Option<VoicePerson>,
        watched: bool,
        mine: bool,
    },
}

impl Card {
    fn producer(&self) -> Option<&str> {
        match self {
            Self::Person { camera, .. } => camera.as_deref(),
            Self::Stream { producer, .. } => Some(producer),
        }
    }
}

/// Onde o retângulo `size` cabe inteiro dentro de `into`, centrado: a tela de quem transmite
/// aparece toda, com faixa preta onde sobra.
pub(super) fn fit(size: Vec2, into: Rect) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return into;
    }

    let scale = (into.width() / size.x).min(into.height() / size.y);

    Rect::from_center_size(into.center(), size * scale)
}

/// O pedaço da imagem `size` que cobre `into` sem faixa: a câmera preenche o cartão, cortada nas
/// bordas, como no Discord.
fn cover(size: Vec2, into: Rect) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 || into.width() <= 0.0 || into.height() <= 0.0 {
        return Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    }

    let wanted = into.width() / into.height();
    let have = size.x / size.y;

    if have > wanted {
        let kept = wanted / have;

        Rect::from_min_max(pos2((1.0 - kept) / 2.0, 0.0), pos2((1.0 + kept) / 2.0, 1.0))
    } else {
        let kept = have / wanted;

        Rect::from_min_max(pos2(0.0, (1.0 - kept) / 2.0), pos2(1.0, (1.0 + kept) / 2.0))
    }
}

pub fn show(app: &mut Unkvoid, ui: &mut Ui, channel: &Channel) {
    if app.voice_chat_open {
        let area = ui.max_rect();
        header(app, ui, &channel.name, area);
        return chat::channel(app, ui, false);
    }

    let people = people_of(app, channel);

    stage(app, ui, &channel.name, people);
}

/// A sala por código: o mesmo palco, com quem o SFU conta na sala. Não há servidor por trás,
/// então não há chat, soundboard nem convite — o convite é o próprio código.
pub fn room(app: &mut Unkvoid, ui: &mut Ui, code: &str) {
    let people = app.call.people.clone();

    stage(app, ui, code, people);
}

fn stage(app: &mut Unkvoid, ui: &mut Ui, title: &str, people: Vec<VoicePerson>) {
    let cards = cards_of(app, people);
    egui::CentralPanel::no_frame()
        .frame(Frame::new().fill(Color32::BLACK))
        .show(ui, |ui| {
            let stage = ui.max_rect();
            draw_tiles(app, ui, stage, &cards);

            // Pela posição do ponteiro, e não pelo hover nem pela camada: a dica de um botão da barra
            // é outra camada, e a barra sumia debaixo do clique.
            if pointer_is_inside(stage, ui.input(|input| input.pointer.interact_pos())) {
                header(app, ui, title, stage);
                controls(app, ui, stage);
            }
        });
}

/// Na sala por código não há servidor: o que se manda para chamar alguém é o código.
fn in_code_room(app: &Unkvoid) -> bool {
    app.call.seat.as_ref().is_some_and(|seat| seat.guest.is_some())
}

fn draw_tiles(app: &mut Unkvoid, ui: &mut Ui, stage: Rect, people: &[Card]) {
    if people.is_empty() {
        ui.painter().text(
            stage.center(),
            Align2::CENTER_CENTER,
            "Conectando…",
            theme::regular(15.0),
            theme::MUTED,
        );
        return;
    }

    let horizontal_margin = 24.0;
    let top_margin = 56.0;
    // Sozinho, o "Convidar para voz" fica entre os cartões e a barra, 150 px acima do fundo como
    // no Discord: a barra aparece no hover e ocupa os últimos 60.
    let invite_center = stage.bottom() - 150.0;
    let bottom_margin = if people.len() == 1 { 150.0 + 20.0 + 24.0 } else { 88.0 };
    let area = Rect::from_min_max(
        pos2(stage.left() + horizontal_margin, stage.top() + top_margin),
        pos2(
            stage.right() - horizontal_margin,
            stage.bottom() - bottom_margin,
        ),
    );
    // A transmissão tem a prioridade: sem um vídeo escolhido com o clique, vai para o foco a tela
    // de outra pessoa aberta, senão a própria aberta, senão a de outra pessoa esperando o "Assistir".
    let focused = app
        .call
        .tiles
        .iter()
        .find(|placed| placed.focused)
        .map(|placed| placed.tile.producer_id.clone())
        .or_else(|| {
            if people.len() < 2 {
                return None;
            }

            people
                .iter()
                .filter_map(|card| match card {
                    Card::Stream { producer, watched, mine, .. } if !producer.is_empty() && (*watched || !*mine) => {
                        Some((producer, (!*watched, *mine)))
                    }
                    _ => None,
                })
                .min_by_key(|(_, rank)| *rank)
                .map(|(producer, _)| producer.clone())
        });

    // Com um vídeo no foco, ele fica grande em cima e os outros numa fileira embaixo.
    if let Some(focused) = focused
        && let Some(big) = people.iter().find(|card| card.producer() == Some(focused.as_str()))
    {
        let others: Vec<Card> = people
            .iter()
            .filter(|card| card.producer() != Some(focused.as_str()))
            .cloned()
            .collect();
        let strip = if others.is_empty() { 0.0 } else { 120.0 };
        let spotlight = Rect::from_min_max(area.min, pos2(area.max.x, area.max.y - strip));

        card(app, ui, spotlight, big);

        if !others.is_empty() {
            let height = strip - TILE_GAP;
            let width = height * 16.0 / 9.0;
            #[allow(clippy::cast_precision_loss)]
            let row_width = others.len() as f32 * (width + TILE_GAP) - TILE_GAP;
            let mut left = area.center().x - row_width / 2.0;

            for other in &others {
                card(
                    app,
                    ui,
                    Rect::from_min_size(pos2(left, spotlight.bottom() + TILE_GAP), vec2(width, height)),
                    other,
                );
                left += width + TILE_GAP;
            }
        }

        return;
    }

    let (columns, width) = grid(area.width(), area.height(), people.len());
    // Sozinho, o Discord não estica o cartão no palco inteiro: ele fica com meia altura.
    let width = if people.len() == 1 {
        width.min((area.height() - TILE_GAP) / 2.0 * 16.0 / 9.0)
    } else {
        width
    };
    let height = width * 9.0 / 16.0;
    let rows = people.len().div_ceil(columns);
    let total = vec2(
        columns as f32 * width + (columns - 1) as f32 * TILE_GAP,
        rows as f32 * height + (rows - 1) as f32 * TILE_GAP,
    );
    let origin = area.center() - total / 2.0;

    for (index, person) in people.iter().enumerate() {
        let row = index / columns;
        let column = index % columns;
        let items_in_row = (people.len() - row * columns).min(columns);
        let row_width = items_in_row as f32 * width + (items_in_row - 1) as f32 * TILE_GAP;
        let left = area.center().x - row_width / 2.0;
        let top = origin.y + row as f32 * (height + TILE_GAP);
        let rect = Rect::from_min_size(
            pos2(left + column as f32 * (width + TILE_GAP), top),
            vec2(width, height),
        );

        card(app, ui, rect, person);
    }

    if people.len() == 1 {
        let invite_width = 188.0;
        let invite_rect = Rect::from_center_size(
            pos2(stage.center().x, invite_center),
            vec2(invite_width, 40.0),
        );
        let (label, icon) = if in_code_room(app) {
            ("Copiar código da sala", icons::COPY)
        } else {
            ("Convidar para voz", icons::USER_PLUS)
        };

        if invite_button(ui, invite_rect, label, icon, "call-center-invite") {
            open_invite(app);
        }
    }
}

fn header(app: &mut Unkvoid, ui: &mut Ui, title: &str, stage: Rect) {
    let header = Rect::from_min_size(stage.min, vec2(stage.width(), 48.0));
    let inner = Rect::from_min_max(header.min + vec2(16.0, 0.0), header.max - vec2(16.0, 0.0));
    let code_room = in_code_room(app);
    ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
        ui.set_min_size(inner.size());
        ui.horizontal_centered(|ui| {
            let (channel_icon, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
            discord_icons::paint_glyph(ui, channel_icon, if code_room { icons::HASH } else { icons::SPEAKER_HIGH }, theme::TEXT, 20.0);
            ui.label(
                egui::RichText::new(title)
                    .font(theme::bold(16.0))
                    .color(Color32::WHITE),
            );

            if code_room {
                return;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let icon = if app.voice_chat_open {
                    icons::USERS_FOUR
                } else {
                    icons::CHAT_CIRCLE_TEXT
                };
                let button = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
                let color = if button.1.hovered() {
                    theme::BRIGHT
                } else {
                    theme::TEXT
                };
                discord_icons::paint_glyph(ui, button.0.shrink(8.0), icon, color, 24.0);
                let response = button.1.tip("Abrir o chat da voz");
                if response.clicked() {
                    app.actions.push(Action::ToggleVoiceChat);
                }
            });
        });
    });
}

fn controls(app: &mut Unkvoid, ui: &mut Ui, stage: Rect) {
    let bottom = stage.bottom() - 16.0;
    // A sala por código é só a tela: a barra fica com o compartilhar e o desligar.
    let code_room = in_code_room(app);
    let (microphone_width, gap) = if code_room { (0.0, 0.0) } else { (132.0, 8.0) };
    let media_width = if code_room { 56.0 } else { 166.0 };
    let controls_left = stage.center().x - (microphone_width + gap + media_width + 8.0 + 66.0) / 2.0;
    let microphone_group = Rect::from_min_size(
        pos2(controls_left, bottom - 49.0),
        vec2(microphone_width, 49.0),
    );
    let media_group = Rect::from_min_size(
        pos2(microphone_group.right() + gap, bottom - 49.0),
        vec2(media_width, 49.0),
    );
    let disconnect = Rect::from_min_size(
        pos2(media_group.right() + 8.0, bottom - 49.0),
        vec2(66.0, 49.0),
    );
    draw_group(ui, media_group);

    if !code_room {
        draw_group(ui, microphone_group);
        ui.scope_builder(
            UiBuilder::new().max_rect(microphone_group.shrink2(vec2(6.0, 8.0))),
            |ui| {
                ui.horizontal(|ui| {
                    let (microphone, caret) = sidebar::split_button(
                        ui,
                        "call-microphone",
                        if app.call.muted || !app.call.can_speak {
                            icons::MICROPHONE_SLASH
                        } else {
                            icons::MICROPHONE
                        },
                        if !app.call.can_speak {
                            "Você não pode falar neste canal"
                        } else if app.call.muted {
                            "Reativar microfone"
                        } else {
                            "Silenciar microfone"
                        },
                        if app.call.muted || !app.call.can_speak {
                            theme::DANGER
                        } else {
                            theme::TEXT
                        },
                    );
                    if microphone.clicked() && app.call.can_speak {
                        app.actions.push(Action::ToggleMute);
                    }
                    sidebar::device_menu(app, &caret, true);

                    camera_button(app, ui);
                });
            },
        );
    }

    ui.scope_builder(
        UiBuilder::new().max_rect(media_group.shrink2(vec2(6.0, 8.0))),
        |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.horizontal(|ui| {
                share_button(app, ui);

                if code_room {
                    return;
                }

                // A soundboard é a do servidor: na sala por código não há de onde tirar os sons, e o
                // botão nem aparece.
                if let Some(seat) = app.call.seat.clone().filter(|seat| seat.guest.is_none()) {
                    let allowed = !app.call.muted && !app.call.deafened && app.call.can_speak;
                    let (soundboard, soundboard_response) = ui.allocate_exact_size(vec2(44.0, 32.0), Sense::click());
                    draw_icon_button(ui, soundboard, icons::MEGAPHONE, !allowed, soundboard_response.hovered());
                    let soundboard_response = soundboard_response.tip(if allowed {
                        "Abrir soundboard"
                    } else {
                        "Você está silenciado"
                    });
                    if soundboard_response.clicked() && allowed {
                        app.soundboard_search.clear();
                        app.actions.push(Action::LoadServerSounds(seat.server));
                    }
                    // Silenciado, o botão só explica: o painel não abre, como no Discord.
                    if allowed {
                        let server_name = app.server_summary(seat.server).map_or_else(String::new, |server| server.name.clone());
                        let server_tree = app.tree.clone();
                        egui::Popup::menu(&soundboard_response)
                            .id(soundboard_response.id.with("call-soundboard"))
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .align(egui::RectAlign::TOP)
                            .gap(8.0)
                            .frame(widgets::menu_frame())
                            .show(|ui| sidebar::soundboard(app, ui, &server_name, server_tree.as_ref()));
                    }
                }

                let (more, more_response) = ui.allocate_exact_size(vec2(44.0, 32.0), Sense::click());
                discord_icons::paint(ui, more.shrink(8.0), discord_icons::Icon::MoreHorizontal, if more_response.hovered() { theme::BRIGHT } else { theme::TEXT });
                let more_response = more_response.tip("Mais opções");
                egui::Popup::menu(&more_response)
                    .id(more_response.id.with("call-more-options"))
                    .align(egui::RectAlign::TOP)
                    .gap(8.0)
                    .frame(widgets::menu_frame())
                    .show(|ui| {
                        if widgets::menu_check(ui, "Desativar áudio", app.call.deafened, false) {
                            app.actions.push(Action::ToggleDeafen);
                        }
                        if widgets::menu_item(ui, "", "Configurações de voz", false) {
                            app.actions.push(Action::Show(Modal::Settings(crate::app::SettingsPage::Voice(crate::app::VoiceSection::Voice))));
                        }
                        let is_fullscreen = ui.ctx().input(|input| input.viewport().fullscreen.unwrap_or(false));
                        if widgets::menu_item(ui, "", if is_fullscreen { "Sair da tela cheia" } else { "Tela cheia" }, false) {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(!is_fullscreen));
                        }
                    });
            });
        },
    );

    let disconnect_response =
        ui.interact(disconnect, ui.id().with("call-disconnect"), Sense::click());
    ui.painter().rect_filled(
        disconnect,
        CornerRadius::same(12),
        if disconnect_response.hovered() {
            theme::HANG_UP.gamma_multiply(0.82)
        } else {
            theme::HANG_UP
        },
    );
    discord_icons::paint_glyph(ui, disconnect.shrink(14.0), icons::PHONE_DISCONNECT, theme::BRIGHT, 22.0);
    if disconnect_response.tip("Desligar").clicked() {
        app.actions.push(Action::LeaveVoice);
    }

    let invite_rect =
        Rect::from_min_size(pos2(stage.left() + 16.0, bottom - 40.0), vec2(40.0, 40.0));
    let invite_response = ui.interact(invite_rect, ui.id().with("call-invite"), Sense::click());
    draw_icon_button(
        ui,
        invite_rect,
        icons::USER_PLUS,
        false,
        invite_response.hovered(),
    );
    if invite_response
        .tip(if in_code_room(app) { "Copiar código da sala" } else { "Convidar para voz" })
        .clicked()
    {
        open_invite(app);
    }

    let fullscreen =
        Rect::from_min_size(pos2(stage.right() - 56.0, bottom - 40.0), vec2(40.0, 40.0));
    let fullscreen_response =
        ui.interact(fullscreen, ui.id().with("call-fullscreen"), Sense::click());
    let is_fullscreen = ui
        .ctx()
        .input(|input| input.viewport().fullscreen.unwrap_or(false));
    draw_icon_button(
        ui,
        fullscreen,
        if is_fullscreen {
            icons::ARROWS_IN
        } else {
            icons::ARROWS_OUT
        },
        false,
        fullscreen_response.hovered(),
    );
    if fullscreen_response
        .tip(if is_fullscreen {
            "Sair da tela cheia"
        } else {
            "Tela cheia"
        })
        .clicked()
    {
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!is_fullscreen));
    }
}

/// A câmera da barra: liga pela prévia (ou direto, se a pessoa desligou a prévia), desliga. Sem
/// a permissão de vídeo do canal ela fica apagada.
pub(super) fn camera_button(app: &mut Unkvoid, ui: &mut Ui) {
    let (rect, response) = ui.allocate_exact_size(vec2(36.0, 32.0), Sense::click());
    let allowed = crate::camera::FILMS && app.call.mine.can_video;
    let on = app.call.mine.camera;

    if on {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), theme::BUTTON_HOVER);
    }

    if allowed && response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(8), theme::BUTTON_HOVER);
    }
    discord_icons::paint(ui, rect.shrink(9.0), discord_icons::Icon::Video, if !allowed { theme::DIM } else if response.hovered() { theme::BRIGHT } else { theme::TEXT });
    if !on {
        let badge = Rect::from_center_size(rect.right_bottom() - vec2(8.0, 8.0), vec2(12.0, 12.0));
        ui.painter().circle_filled(badge.center(), 6.0, theme::CALL_CONTROL);
        ui.painter().circle_stroke(badge.center(), 5.0, Stroke::new(1.2, theme::TEXT));
        ui.painter().line_segment([badge.left_bottom(), badge.right_top()], Stroke::new(1.4, theme::TEXT));
    }

    let tip = if !crate::camera::FILMS {
        "A câmera ainda não chegou a este sistema"
    } else if !app.call.mine.can_video {
        "Você não tem permissão de vídeo neste canal"
    } else if on {
        "Desligar câmera"
    } else {
        "Ligar câmera"
    };

    if response.tip(tip).clicked() && allowed {
        app.actions.push(Action::ToggleCamera);
    }

    let (caret_rect, caret) = ui.allocate_exact_size(vec2(18.0, 32.0), Sense::click());
    let caret = caret.on_hover_cursor(egui::CursorIcon::PointingHand).tip("Opções de câmera");
    discord_icons::paint_glyph(ui, caret_rect, egui_phosphor::regular::CARET_DOWN, theme::TEXT, 14.0);
    egui::containers::menu::SubMenu::new().show(ui, &caret, |ui| {
        if widgets::menu_item(ui, "", "Configurações de vídeo", false) {
            app.actions.push(Action::Show(Modal::Settings(crate::app::SettingsPage::Voice(crate::app::VoiceSection::Video))));
        }
    });
}

/// A tela da barra: abre o seletor; transmitindo, o menu de trocar ou parar.
pub(super) fn share_button(app: &mut Unkvoid, ui: &mut Ui) {
    let (rect, response) = ui.allocate_exact_size(vec2(44.0, 32.0), Sense::click());
    let allowed = app.call.mine.can_share;
    let live = app.call.mine.sharing;

    // Transmitindo, o botão fica vermelho como o microfone mudo: é o que se clica para parar.
    if live {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(8),
            if response.hovered() {
                theme::OFF_HOVER
            } else {
                theme::OFF
            },
        );
        discord_icons::paint_glyph(ui, rect.shrink(7.0), icons::MONITOR_ARROW_UP, theme::DANGER, 20.0);
    } else {
        draw_icon_button(
            ui,
            rect,
            icons::MONITOR_ARROW_UP,
            !allowed,
            allowed && response.hovered(),
        );
    }

    let tip = if !allowed {
        "Você não tem permissão para transmitir neste canal"
    } else if live {
        "Gerenciar transmissão"
    } else {
        "Compartilhar sua tela"
    };
    let response = response.tip(tip);

    if live {
        egui::Popup::menu(&response)
            .align(egui::RectAlign::TOP)
            .gap(8.0)
            .frame(widgets::menu_frame())
            .show(|ui| {
                if widgets::menu_item(ui, egui_phosphor::regular::MONITOR, "Trocar transmissão", false) {
                    app.actions
                        .push(Action::Show(Modal::GoLive(GoLiveTab::Screens)));
                }

                if widgets::menu_item(ui, egui_phosphor::regular::X_CIRCLE, "Parar transmissão", true) {
                    app.actions.push(Action::StopShare);
                }
            });
    } else if response.clicked() && allowed {
        app.actions
            .push(Action::Show(Modal::GoLive(GoLiveTab::Screens)));
    }
}

fn draw_group(ui: &Ui, rect: Rect) {
    ui.painter()
        .rect_filled(rect, CornerRadius::same(12), theme::CALL_CONTROL);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(12),
        Stroke::new(1.0, theme::CALL_CONTROL_BORDER),
        egui::StrokeKind::Inside,
    );
}

fn pointer_is_inside(rect: Rect, pointer: Option<egui::Pos2>) -> bool {
    pointer.is_some_and(|pointer| rect.contains(pointer))
}

fn draw_icon_button(ui: &Ui, rect: Rect, icon: &str, disabled: bool, hovered: bool) {
    if hovered {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), theme::BUTTON_HOVER);
    }
    let color = if disabled {
        theme::DIM
    } else if hovered {
        theme::BRIGHT
    } else {
        theme::TEXT
    };
    discord_icons::paint_glyph(ui, rect.shrink(10.0), icon, color, 20.0);
}

fn invite_button(ui: &Ui, rect: Rect, label: &str, icon: &str, id: &str) -> bool {
    let response = ui.interact(rect, ui.id().with(id), Sense::click());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), Color32::from_rgb(18, 18, 19));
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(1.0, Color32::from_rgb(46, 46, 51)),
        egui::StrokeKind::Inside,
    );
    discord_icons::paint_glyph(
        ui,
        Rect::from_center_size(pos2(rect.left() + 25.0, rect.center().y), vec2(18.0, 18.0)),
        icon,
        if response.hovered() {
            theme::BRIGHT
        } else {
            theme::TEXT
        },
        18.0,
    );
    ui.painter().text(
        pos2(rect.left() + 42.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::regular(14.0),
        if response.hovered() {
            theme::BRIGHT
        } else {
            theme::TEXT
        },
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn open_invite(app: &mut Unkvoid) {
    let Some(seat) = &app.call.seat else {
        return;
    };

    let action = if seat.guest.is_some() {
        Action::CopyText(seat.channel.clone())
    } else {
        Action::Show(Modal::Invite {
            server: seat.server,
            code: None,
        })
    };

    app.actions.push(action);
}

fn card(app: &mut Unkvoid, ui: &mut Ui, rect: Rect, card: &Card) {
    match card {
        Card::Person { person, camera } => {
            let clicked = tile(app, ui, rect, person, camera.as_deref());

            if clicked && let Some(camera) = camera {
                app.actions.push(Action::FocusTile(camera.clone()));
            }
        }
        Card::Stream { .. } => stream(app, ui, rect, card),
    }
}

/// Um vídeo pintado no cartão: a tela inteira, com faixa (`fill` falso), ou a câmera cobrindo
/// o cartão.
fn paint_video(ui: &Ui, rect: Rect, texture: &egui::TextureHandle, fill: bool) {
    if fill {
        egui::Image::new((texture.id(), rect.size()))
            .uv(cover(texture.size_vec2(), rect))
            .corner_radius(theme::RADIUS)
            .paint_at(ui, rect);
    } else {
        let shown = fit(texture.size_vec2(), rect);

        egui::Image::new((texture.id(), shown.size())).paint_at(ui, shown);
    }
}

/// A transmissão em tela cheia, sozinha na janela inteira — sem barra de título, trilho nem o
/// resto do palco. A janela entra em tela cheia junto e, ao sair (ou se a transmissão acabar),
/// volta como estava; o Esc sai. Devolve se há uma.
pub fn full_stream(app: &mut Unkvoid, ui: &mut Ui) -> bool {
    let full = app
        .call
        .tiles
        .iter()
        .find(|placed| placed.full)
        .map(|placed| (placed.tile.producer_id.clone(), placed.tile.label.clone(), placed.tile.mine));

    match (&full, app.window_was_full) {
        (Some(_), None) => {
            app.window_was_full = Some(ui.input(|input| input.viewport().fullscreen.unwrap_or(false)));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }
        (None, Some(was_full)) => {
            app.window_was_full = None;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(was_full));
        }
        _ => {}
    }

    let Some((producer, name, mine)) = full else {
        return false;
    };

    egui::CentralPanel::no_frame()
        .frame(Frame::new().fill(Color32::BLACK))
        .show(ui, |ui| {
            let rect = ui.max_rect();

            let card = Card::Stream { producer: producer.clone(), name, person: None, watched: true, mine };

            stream(app, ui, rect, &card);
        });

    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        app.actions.push(Action::FullTile(producer));
    }

    true
}

/// O cartão de uma transmissão: a tela de quem transmite, o nome com o "AO VIVO", e o
/// "Assistir transmissão" quando ainda não foi aberta. A própria só mostra a imagem quando a
/// pessoa pede: ver a própria tela custa decodificar o que já se está codificando, e o jogo paga.
fn stream(app: &mut Unkvoid, ui: &mut Ui, rect: Rect, card: &Card) {
    let Card::Stream { producer, name, person, watched, mine } = card else {
        return;
    };
    let (producer, watched, mine) = (producer.as_str(), *watched, *mine);
    let response = ui
        .interact(rect, ui.id().with(("stream", producer, mine)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    // Pela posição do ponteiro, como no cartão da pessoa: em cima do volume o cartão não está
    // mais sob o hover, e os controles sumiriam debaixo do clique.
    let over = pointer_is_inside(rect, ui.input(|input| input.pointer.interact_pos()));
    let full = app.call.tiles.iter().any(|placed| placed.full && placed.tile.producer_id == producer);
    let painter = ui.painter();

    painter.rect_filled(rect, theme::RADIUS, if full { Color32::BLACK } else { theme::RAIL });

    match app.pictures.get(producer).filter(|_| watched) {
        Some(texture) => paint_video(ui, rect, texture, false),
        None => {
            crate::ui::icons::text(painter, rect.center() - vec2(0.0, 26.0), Align2::CENTER_CENTER, icons::MONITOR_PLAY, theme::filled(40.0), theme::MUTED);

            let label = if watched {
                "Carregando a transmissão…"
            } else if mine {
                "Assistir à minha tela"
            } else {
                "Assistir transmissão"
            };
            let galley = painter.layout_no_wrap(label.to_owned(), theme::bold(15.0), theme::BRIGHT);
            let pill = Rect::from_center_size(rect.center() + vec2(0.0, 24.0), galley.size() + vec2(32.0, 20.0));

            if !watched {
                painter.rect_filled(
                    pill,
                    theme::RADIUS,
                    if response.hovered() {
                        theme::BUTTON_HOVER
                    } else {
                        theme::BUTTON
                    },
                );
            }

            painter.galley(pill.center() - galley.size() / 2.0, galley, theme::BRIGHT);
        }
    }

    // Os números da tela no canto de cima, livre: os botões ficam à direita e o nome embaixo.
    if watched && app.pictures.contains_key(producer) && let Some((line, danger)) = app.stream_stats.get(producer) {
        let color = if *danger { theme::DANGER } else { Color32::WHITE };
        let label = painter.layout_no_wrap(line.clone(), theme::bold(13.0), color);
        let pill = Rect::from_min_size(pos2(rect.left() + 8.0, rect.top() + 8.0), vec2(label.size().x + 16.0, 28.0));

        painter.rect_filled(pill, CornerRadius::same(6), Color32::from_black_alpha(153));
        painter.galley(pos2(pill.left() + 8.0, pill.center().y - label.size().y / 2.0), label, color);
    }

    // O cartão da tela é o da pessoa: a borda verde de quem fala e o mudo ao lado do nome.
    if person.as_ref().is_some_and(|person| app.call.speaking.contains(&person.user_id)) {
        ui.painter().rect_stroke(rect, theme::RADIUS, Stroke::new(2.0, theme::SUCCESS), egui::StrokeKind::Inside);
    }

    let tag_text = match person.as_ref().filter(|person| person.muted || person.deafened) {
        Some(person) => ui.painter().layout_no_wrap(
            format!("{}  {name}", if person.deafened { icons::HEADPHONES } else { icons::MICROPHONE_SLASH }),
            theme::filled(14.0),
            Color32::WHITE,
        ),
        None => ui.painter().layout_no_wrap(name.to_owned(), theme::bold(14.0), Color32::WHITE),
    };
    let tag = Rect::from_min_size(
        pos2(rect.left() + 8.0, rect.bottom() - 36.0),
        vec2(tag_text.size().x + 16.0, 28.0),
    );
    let live = Rect::from_min_size(pos2(tag.right() + 6.0, tag.top() + 4.0), vec2(56.0, 20.0));

    ui.painter()
        .rect_filled(tag, CornerRadius::same(6), Color32::from_black_alpha(153));
    ui.painter().galley(
        pos2(tag.left() + 8.0, tag.center().y - tag_text.size().y / 2.0),
        tag_text,
        Color32::WHITE,
    );
    live_badge(ui, live.left_top(), Align2::LEFT_TOP);

    if watched && over {
        let close = Rect::from_min_size(pos2(rect.right() - 44.0, rect.top() + 8.0), vec2(36.0, 36.0));
        let closing = ui
            .interact(close, ui.id().with(("stop-watching", producer, mine)), Sense::click())
            .tip("Parar de assistir");

        ui.painter().rect_filled(
            close,
            theme::RADIUS,
            Color32::from_black_alpha(if closing.hovered() { 200 } else { 140 }),
        );
        discord_icons::paint_glyph(ui, close, icons::X, Color32::WHITE, 18.0);

        if closing.clicked() {
            app.actions.push(if mine {
                Action::SelfView(false)
            } else {
                Action::WatchStream(producer.to_owned(), false)
            });

            return;
        }

        let expand = Rect::from_min_size(close.left_top() - vec2(44.0, 0.0), close.size());
        let expanding = ui
            .interact(expand, ui.id().with(("full-stream", producer, mine)), Sense::click())
            .tip(if full { "Sair da tela cheia" } else { "Tela cheia" });

        ui.painter().rect_filled(
            expand,
            theme::RADIUS,
            Color32::from_black_alpha(if expanding.hovered() { 200 } else { 140 }),
        );
        discord_icons::paint_glyph(ui, expand, if full { icons::ARROWS_IN } else { icons::ARROWS_OUT }, Color32::WHITE, 18.0);

        if expanding.clicked() {
            app.actions.push(Action::FullTile(producer.to_owned()));

            return;
        }
    }

    // O som da própria tela não volta para cá: só a dos outros tem volume.
    if watched && over && !mine {
        stream_sound(app, ui, rect, producer);
    }

    // A própria transmissão não para com um clique no cartão: parar é o botão vermelho da barra.
    if response.clicked() {
        match (watched, mine) {
            (true, _) if !producer.is_empty() => app.actions.push(Action::FocusTile(producer.to_owned())),
            (true, _) => {}
            (false, true) => app.actions.push(Action::SelfView(true)),
            (false, false) => app.actions.push(Action::WatchStream(producer.to_owned(), true)),
        }
    }
}

/// O alto-falante e o volume no pé direito do cartão de uma tela assistida. O som chega mudo:
/// clicar no alto-falante ou mexer no volume é o que o liga.
fn stream_sound(app: &mut Unkvoid, ui: &mut Ui, rect: Rect, producer: &str) {
    let Some(placed) = app
        .call
        .tiles
        .iter()
        .find(|placed| placed.tile.producer_id == producer && placed.tile.audio.is_some())
    else {
        return;
    };
    let (heard, volume) = (placed.heard, placed.volume);
    let bar = Rect::from_min_size(pos2(rect.right() - 8.0 - 148.0, rect.bottom() - 36.0), vec2(148.0, 28.0));
    let speaker = Rect::from_min_size(bar.min, vec2(28.0, 28.0));
    let track = Rect::from_min_max(pos2(speaker.right() + 6.0, bar.center().y - 10.0), pos2(bar.right() - 14.0, bar.center().y + 10.0));

    ui.painter()
        .rect_filled(bar, CornerRadius::same(6), Color32::from_black_alpha(153));

    let sound = ui
        .interact(speaker, ui.id().with(("stream-sound", producer)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .tip(if heard { "Silenciar a transmissão" } else { "Ouvir a transmissão" });

    discord_icons::paint_glyph(ui, speaker.shrink(5.0), if heard { icons::SPEAKER_HIGH } else { icons::SPEAKER_SLASH }, Color32::WHITE, 18.0);

    if sound.clicked() {
        app.actions.push(Action::StreamSound(producer.to_owned()));
    }

    let chosen = ui
        .scope_builder(UiBuilder::new().max_rect(track), |ui| {
            widgets::held_slider(ui, &format!("stream-volume-{producer}"), if heard { f32::from(volume) } else { 0.0 }, 0.0..=100.0, true)
        })
        .inner;

    if let Some(chosen) = chosen {
        // O slider vai de 0 a 100: a conversão não perde nada.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        app.actions.push(Action::StreamVolume(producer.to_owned(), chosen.round() as u8));
    }
}

/// O selo vermelho do "AO VIVO", preso ao canto pedido.
pub(super) fn live_badge(ui: &Ui, corner: egui::Pos2, align: Align2) -> Rect {
    let rect = align.anchor_size(corner, vec2(56.0, 20.0));

    ui.painter()
        .rect_filled(rect, CornerRadius::same(4), theme::DANGER);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        "AO VIVO",
        theme::bold(11.0),
        Color32::WHITE,
    );

    rect
}

/// O cartão de uma pessoa: a câmera dela, se ligada, ou a cor dela com a foto no meio. Devolve
/// o clique.
fn tile(app: &Unkvoid, ui: &Ui, rect: Rect, person: &VoicePerson, camera: Option<&str>) -> bool {
    let response = ui.interact(
        rect,
        ui.id().with(("voice-person", person.user_id)),
        if camera.is_some() {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let speaking = app.call.speaking.contains(&person.user_id);
    let is_self = app.call.me.or(app.my_id()) == Some(person.user_id);
    // Na sala por código ninguém tem microfone: o "mudo" estaria em todo cartão sem dizer nada.
    let muted = !in_code_room(app)
        && if is_self {
            app.call.muted
        } else {
            person.muted
        };
    let deafened = if is_self { app.call.deafened } else { person.deafened };
    let member = app.tree.as_ref().and_then(|tree| {
        tree.members
            .iter()
            .find(|member| member.user_id == person.user_id)
    });
    let avatar_url = member.and_then(|member| member.avatar_url.as_deref());

    if speaking {
        ui.painter().add(
            egui::epaint::Shadow {
                offset: [0, 0],
                blur: 10,
                spread: 0,
                color: theme::SUCCESS.gamma_multiply(0.3),
            }
            .as_shape(rect, theme::RADIUS),
        );
    }
    let video = if is_self && app.call.mine.camera {
        app.camera_picture.as_ref()
    } else {
        camera.and_then(|camera| app.pictures.get(camera))
    };

    match video {
        Some(texture) => {
            ui.painter().rect_filled(rect, theme::RADIUS, Color32::BLACK);
            paint_video(ui, rect, texture, true);
        }
        None => {
            ui.painter()
                .rect_filled(rect, theme::RADIUS, widgets::color_for(&person.name));

            let avatar = Rect::from_center_size(rect.center(), vec2(80.0, 80.0));

            widgets::paint_avatar(ui, avatar, &person.name, avatar_url);
        }
    }

    if speaking {
        ui.painter().rect_stroke(
            rect,
            theme::RADIUS,
            Stroke::new(2.0, theme::SUCCESS),
            egui::StrokeKind::Inside,
        );
    }

    let live = if is_self {
        app.call.mine.sharing
    } else {
        person.sources.iter().any(|source| source == "screen")
    };

    if live {
        live_badge(ui, pos2(rect.right() - 8.0, rect.top() + 8.0), Align2::RIGHT_TOP);
    }

    let pointer_over_tile = pointer_is_inside(rect, ui.input(|input| input.pointer.interact_pos()));

    if muted || deafened || pointer_over_tile {
        let status_icon = if deafened {
            icons::HEADPHONES
        } else {
            icons::MICROPHONE_SLASH
        };
        let label = match (muted || deafened, response.hovered()) {
            (true, true) => format!("{status_icon}  {}", person.name),
            (true, false) => status_icon.to_owned(),
            (false, true) => person.name.clone(),
            (false, false) => String::new(),
        };
        let galley = ui
            .painter()
            .layout_no_wrap(label, theme::filled(14.0), Color32::WHITE);
        let tag = Rect::from_min_size(
            pos2(rect.left() + 8.0, rect.bottom() - 8.0 - 28.0),
            vec2(galley.size().x + 16.0, 28.0),
        );
        ui.painter()
            .rect_filled(tag, CornerRadius::same(6), Color32::from_black_alpha(153));
        ui.painter().galley(
            pos2(tag.left() + 8.0, tag.center().y - galley.size().y / 2.0),
            galley,
            Color32::WHITE,
        );
    }

    response.clicked()
}

/// Os cartões do palco: as transmissões primeiro, assistidas ou não, e depois as pessoas, cada
/// uma com o vídeo da câmera dela quando ligada.
fn cards_of(app: &Unkvoid, people: Vec<VoicePerson>) -> Vec<Card> {
    let owner = |peer: &str| app.call.accounts.get(peer).copied();
    let me = app.call.me.or(app.my_id());
    let code_room = in_code_room(app);
    // Quem transmite é desenhado no cartão da tela: o mudo e a fala dele vão junto. O meu mudo
    // é o da chamada, que muda antes da presença do servidor; na sala por código ninguém tem
    // microfone.
    let person_of = |user: Option<i64>| {
        let mut person = people.iter().find(|person| Some(person.user_id) == user)?.clone();

        if code_room {
            (person.muted, person.deafened) = (false, false);
        } else if user == me {
            (person.muted, person.deafened) = (app.call.muted, app.call.deafened);
        }

        Some(person)
    };
    let stream = |producer: &str, label: &str, user: Option<i64>, watched: bool, mine: bool| {
        let person = person_of(user);

        Card::Stream {
            producer: producer.to_owned(),
            name: person.as_ref().map_or_else(|| label.to_owned(), |person| person.name.clone()),
            person,
            watched,
            mine,
        }
    };
    // A própria transmissão ganha o cartão dela, com o "Assistir à minha tela". Antes de abrir,
    // ela ainda não está nos vídeos assistidos e não tem producer aqui.
    let own = app.call.mine.sharing.then(|| {
        let producer = app
            .call
            .tiles
            .iter()
            .find(|placed| placed.tile.mine && !placed.tile.camera)
            .map(|placed| placed.tile.producer_id.clone())
            .unwrap_or_default();
        let name = app.me.as_ref().map(|me| me.name.clone()).unwrap_or_default();

        stream(&producer, &name, me, app.call.mine.self_view, true)
    });
    let mut cards: Vec<Card> = own
        .into_iter()
        .chain(
            app.call
                .tiles
                .iter()
                .map(|placed| &placed.tile)
                .filter(|tile| !tile.camera && !tile.mine)
                .map(|tile| stream(&tile.producer_id, &tile.label, owner(&tile.peer_id), true, false)),
        )
        .chain(
            app.call
                .pending
                .iter()
                .filter(|tile| !tile.camera)
                .map(|tile| stream(&tile.producer_id, &tile.label, owner(&tile.peer_id), false, false)),
        )
        .collect();

    // Quem transmite já está no cartão da tela dele: o cartão com a foto ao lado repetiria a
    // pessoa. Com a câmera ligada o cartão dela fica, porque aí mostra outra coisa.
    let sharing: Vec<i64> = app
        .call
        .tiles
        .iter()
        .map(|placed| &placed.tile)
        .chain(&app.call.pending)
        .filter(|tile| !tile.camera)
        .filter_map(|tile| if tile.mine { me } else { owner(&tile.peer_id) })
        .chain(me.filter(|_| app.call.mine.sharing))
        .collect();

    cards.extend(people.iter().filter_map(|person| {
        let camera = app
            .call
            .tiles
            .iter()
            .map(|placed| &placed.tile)
            .find(|tile| tile.camera && owner(&tile.peer_id) == Some(person.user_id))
            .map(|tile| tile.producer_id.clone());
        let own_camera = Some(person.user_id) == me && app.call.mine.camera;

        (camera.is_some() || own_camera || !sharing.contains(&person.user_id)).then(|| Card::Person { person: person.clone(), camera })
    }));

    cards
}

fn people_of(app: &Unkvoid, channel: &Channel) -> Vec<VoicePerson> {
    app.tree.as_ref().map(|tree| app.voice_people(tree, &channel.id)).unwrap_or_default()
}

fn grid(width: f32, height: f32, count: usize) -> (usize, f32) {
    let mut best = (1, 0.0_f32);

    for columns in 1..=count.max(1) {
        let rows = count.max(1).div_ceil(columns);
        let tile_width = ((width - (columns - 1) as f32 * TILE_GAP) / columns as f32)
            .min(((height - (rows - 1) as f32 * TILE_GAP) / rows as f32) * 16.0 / 9.0)
            .max(0.0);
        if tile_width > best.1 {
            best = (columns, tile_width);
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_video_fits_whole_or_covers_the_card() {
        let card = Rect::from_min_size(pos2(0.0, 0.0), vec2(160.0, 90.0));
        let tall = fit(vec2(90.0, 160.0), card);

        assert!((tall.height() - 90.0).abs() < 0.01 && tall.width() < 90.0, "a tela em pé cabe inteira");

        let wide = cover(vec2(320.0, 90.0), card);

        assert!((wide.height() - 1.0).abs() < 0.01 && wide.width() < 1.0, "a câmera larga perde as laterais");
        assert_eq!(cover(vec2(1600.0, 900.0), card), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)));
    }

    #[test]
    fn the_grid_fits_everyone_and_uses_the_largest_cards_that_fit() {
        let (columns, width) = grid(1200.0, 700.0, 1);

        assert_eq!(columns, 1);
        assert!((width - 1200.0).abs() < 0.5);

        let (columns, width) = grid(1200.0, 700.0, 4);
        let rows = 4_usize.div_ceil(columns);

        assert!(
            columns as f32 * width + (columns - 1) as f32 * TILE_GAP <= 1200.0 + 0.5,
            "estoura a largura"
        );
        assert!(
            rows as f32 * width * 9.0 / 16.0 + (rows - 1) as f32 * TILE_GAP <= 700.0 + 0.5,
            "estoura a altura"
        );
    }

    #[test]
    fn a_held_pointer_keeps_the_tile_hover_overlay_visible() {
        let tile = Rect::from_min_size(pos2(20.0, 30.0), vec2(320.0, 180.0));

        assert!(pointer_is_inside(tile, Some(pos2(100.0, 200.0))));
        assert!(!pointer_is_inside(tile, Some(pos2(100.0, 220.0))));
        assert!(!pointer_is_inside(tile, None));
    }
}
