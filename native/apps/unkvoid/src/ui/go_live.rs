//! Os dois jeitos de mandar vídeo, como no Discord: o "Compartilhe sua tela" (as janelas e as
//! telas em miniatura, e um clique já transmite) e o "Vamos ligar a câmera?" (a própria câmera
//! na prévia antes de entrar na chamada).

use egui::{Align2, Color32, CornerRadius, Frame, Rect, Sense, Stroke, Ui, pos2, vec2};
use egui_phosphor::regular as icons;

use super::{theme, widgets};
use crate::app::{Action, GoLiveTab, Modal, ShareSource, Unkvoid};

/// Os modos da transmissão: o nome, a explicação e a resolução e os quadros de cada um. O
/// "Personalizada" usa os escolhidos.
const PRESETS: [(&str, &str, &str); 3] = [
    ("games", "Jogos", "Vídeo mais suave"),
    ("screen", "Compartilhamento de tela", "Texto mais claro"),
    ("custom", "Personalizada", ""),
];
const RESOLUTIONS: [(&str, &str); 4] = [
    ("720", "720p"),
    ("1080", "1080p"),
    ("1440", "1440p"),
    ("2160", "Fonte"),
];
const FRAME_RATES: [u64; 3] = [15, 30, 60];

/// A resolução (como o `capture_config` lê) e os quadros por segundo do modo guardado.
pub fn share_mode(saved: &serde_json::Value) -> (String, u64) {
    match saved["sharePreset"].as_str().unwrap_or("games") {
        "screen" => ("2160".into(), 15),
        "custom" => (
            saved["shareResolution"]
                .as_str()
                .filter(|chosen| RESOLUTIONS.iter().any(|(value, _)| value == chosen))
                .unwrap_or("1080")
                .to_owned(),
            saved["shareFps"]
                .as_u64()
                .filter(|chosen| FRAME_RATES.contains(chosen))
                .unwrap_or(60),
        ),
        _ => ("1080".into(), 60),
    }
}

fn resolution_name(value: &str) -> &'static str {
    RESOLUTIONS
        .iter()
        .find(|(known, _)| *known == value)
        .map_or("1080p", |(_, name)| name)
}

pub fn show(app: &mut Unkvoid, ui: &mut Ui, tab: GoLiveTab) {
    let screen = ui.ctx().content_rect();
    let width = (screen.width() - 80.0).clamp(560.0, 960.0);
    let height = (screen.height() - 80.0).clamp(420.0, 720.0);
    let response = egui::Modal::new(egui::Id::new("go-live"))
        .frame(
            Frame::new()
                .fill(theme::ELEVATED)
                .corner_radius(theme::RADIUS)
                .stroke(Stroke::new(1.0, theme::MENU_BORDER)),
        )
        .backdrop_color(Color32::from_black_alpha(170))
        .show(ui.ctx(), |ui| {
            ui.set_width(width);
            ui.set_height(height);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

            Frame::new()
                .inner_margin(egui::Margin::symmetric(24, 16))
                .show(ui, |ui| {
                    ui.set_width(width - 48.0);

                    if let Some(chosen) = tabs(ui, tab) {
                        app.actions.push(Action::Show(Modal::GoLive(chosen)));
                    }
                });

            let footer_height = 72.0;

            Frame::new()
                .inner_margin(egui::Margin {
                    left: 24,
                    right: 24,
                    top: 8,
                    bottom: 0,
                })
                .show(ui, |ui| {
                    ui.set_width(width - 48.0);
                    ui.set_height((ui.available_height() - footer_height).max(0.0));
                    sources(app, ui, tab);
                });

            footer(app, ui, width, footer_height);
        });

    if response.should_close() {
        app.actions.push(Action::CloseModal);
    }
}

fn tabs(ui: &mut Ui, tab: GoLiveTab) -> Option<GoLiveTab> {
    const TABS: [GoLiveTab; 2] = [GoLiveTab::Screens, GoLiveTab::Applications];

    widgets::pill_tabs(
        ui,
        "go-live-tab",
        &[(icons::MONITOR, "Tela Inteira"), (icons::APP_WINDOW, "Aplicativos")],
        usize::from(tab == GoLiveTab::Applications),
    )
    .map(|index| TABS[index])
}

fn sources(app: &mut Unkvoid, ui: &mut Ui, tab: GoLiveTab) {
    let Some(found) = app.share_sources.clone() else {
        ui.centered_and_justified(|ui| ui.spinner());

        return;
    };
    let shown: Vec<ShareSource> = found
        .into_iter()
        .filter(|source| source.screen == (tab == GoLiveTab::Screens))
        .collect();

    if shown.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new(if tab == GoLiveTab::Screens {
                    "Nenhuma tela encontrada."
                } else {
                    "Nenhuma janela aberta para transmitir."
                })
                .font(theme::regular(15.0))
                .color(theme::MUTED),
            );
        });

        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let gap = 16.0;
            let card_width = ((ui.available_width() - gap) / 2.0).min(447.0);

            ui.add_space(6.0);

            for pair in shown.chunks(2) {
                ui.horizontal_top(|ui| {
                    ui.add_space(4.0);

                    for source in pair {
                        let selected = app.share_selected.as_deref() == Some(source.key.as_str());
                        let response = card(app, ui, source, card_width, selected);

                        // Um clique escolhe; o "Transmitir" do rodapé, ou o clique duplo, começa.
                        if response.double_clicked() {
                            app.actions.push(Action::StartShare(source.key.clone()));
                        } else if response.clicked() {
                            app.share_selected = Some(source.key.clone());
                        }

                        ui.add_space(gap);
                    }
                });
                ui.add_space(24.0);
            }
        });
}

/// Uma janela ou tela: a miniatura 16:9 no fundo preto, o "Selecionar" por cima no hover, o aro
/// cinza no hover e roxo na escolhida, e o ícone e o nome embaixo.
fn card(app: &Unkvoid, ui: &mut Ui, source: &ShareSource, width: f32, selected: bool) -> egui::Response {
    let preview_height = width * 9.0 / 16.0;
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, preview_height + 30.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let preview = Rect::from_min_size(rect.min, vec2(width, preview_height));
    let painter = ui.painter();

    if selected || response.hovered() {
        painter.rect_stroke(
            preview.expand(4.0),
            CornerRadius::same(10),
            Stroke::new(2.0, if selected { theme::ACCENT } else { theme::TRACK }),
            egui::StrokeKind::Inside,
        );
    }

    painter.rect_filled(preview, CornerRadius::same(4), Color32::BLACK);

    if let Some(texture) = app.share_previews.get(&source.key) {
        let fitted = super::call::fit(texture.size_vec2(), preview);

        egui::Image::new((texture.id(), fitted.size()))
            .corner_radius(CornerRadius::same(4))
            .paint_at(ui, fitted);
    } else {
        super::icons::paint_glyph(ui, preview, if source.screen { icons::MONITOR } else { icons::APP_WINDOW }, theme::DIM, 40.0);
    }

    let painter = ui.painter();

    if response.hovered() && !selected {
        painter.rect_filled(
            preview,
            CornerRadius::same(4),
            Color32::from_black_alpha(102),
        );

        let call_to_action = painter.layout_no_wrap(
            "Selecionar".to_owned(),
            theme::bold(14.0),
            Color32::BLACK,
        );
        let pill = Rect::from_center_size(
            preview.center(),
            call_to_action.size() + vec2(48.0, 24.0),
        );

        painter.rect_filled(pill, CornerRadius::same(4), Color32::WHITE);
        painter.galley(
            pill.center() - call_to_action.size() / 2.0,
            call_to_action,
            Color32::BLACK,
        );
    }

    let name_top = preview.bottom() + 8.0;

    crate::ui::icons::text(painter, pos2(preview.left(), name_top + 10.0), Align2::LEFT_CENTER, if source.screen {
            icons::MONITOR
        } else {
            icons::APP_WINDOW
        }, theme::icon(16.0), theme::SECONDARY);

    let mut name = egui::text::LayoutJob::simple_singleline(
        source.name.clone(),
        theme::regular(14.0),
        theme::TEXT,
    );

    name.wrap = egui::text::TextWrapping::truncate_at_width(width - 26.0);

    let galley = painter.layout_job(name);

    painter.galley(
        pos2(preview.left() + 24.0, name_top + 10.0 - galley.size().y / 2.0),
        galley,
        theme::TEXT,
    );

    response
}

/// O rodapé: o modo escolhido com a resolução e os quadros, e o botão das opções.
fn footer(app: &mut Unkvoid, ui: &mut Ui, width: f32, height: f32) {
    let saved = app.voice_settings.clone();
    let preset = saved["sharePreset"].as_str().unwrap_or("games").to_owned();
    let muted = saved["shareMuted"].as_bool().unwrap_or(false);
    let (resolution, frame_rate) = share_mode(&saved);
    let (_, name, about) = PRESETS
        .iter()
        .find(|(value, _, _)| *value == preset)
        .copied()
        .unwrap_or(PRESETS[0]);
    let chosen = app.share_selected.clone().and_then(|key| {
        app.share_sources
            .as_ref()?
            .iter()
            .find(|source| source.key == key)
            .cloned()
    });

    Frame::new()
        .fill(theme::MENU)
        .corner_radius(CornerRadius {
            nw: 0,
            ne: 0,
            sw: 8,
            se: 8,
        })
        .inner_margin(egui::Margin::symmetric(24, 16))
        .show(ui, |ui| {
            ui.set_width(width - 48.0);
            ui.set_height(height - 32.0);
            ui.horizontal_centered(|ui| {
                ui.vertical(|ui| {
                    // Com uma escolhida, o título é ela e o modo vai para a linha de baixo, como no
                    // Discord.
                    ui.label(
                        egui::RichText::new(chosen.as_ref().map_or(name, |source| source.name.as_str()))
                            .font(theme::bold(16.0))
                            .color(theme::BRIGHT),
                    );
                    ui.add_space(2.0);

                    let mut details = Vec::new();

                    if chosen.is_some() {
                        details.push(format!("{} {name}", icons::GEAR_SIX));
                    }

                    if !about.is_empty() {
                        details.push(about.to_owned());
                    }

                    details.push(resolution_name(&resolution).to_owned());
                    details.push(format!("{frame_rate}fps"));

                    if muted {
                        details.push(format!("{} Transmissão silenciada", icons::SPEAKER_SLASH));
                    }

                    ui.label(
                        egui::RichText::new(details.join("  •  "))
                            .font(theme::regular(12.0))
                            .color(theme::MUTED),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (button, response) =
                        ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

                    ui.painter().rect_filled(
                        button,
                        theme::RADIUS,
                        if response.hovered() {
                            theme::BUTTON_HOVER
                        } else {
                            theme::BUTTON
                        },
                    );
                    super::icons::paint_glyph(ui, button, icons::GEAR_SIX, theme::TEXT, 20.0);

                    egui::Popup::menu(&response)
                        .align(egui::RectAlign::TOP_END)
                        .gap(8.0)
                        .frame(widgets::menu_frame())
                        .width(280.0)
                        .show(|ui| options(app, ui, &preset, &saved));

                    ui.add_space(8.0);

                    let start = widgets::primary_button(ui, "Transmitir", chosen.is_some(), 120.0);

                    if start && let Some(source) = &chosen {
                        app.actions.push(Action::StartShare(source.key.clone()));
                    }

                    ui.add_space(16.0);
                    quality(app, ui, &saved);
                });
            });
        });
}

/// A resolução e os quadros à mão, como no seletor do Unkvoid de antes: escolher um deles passa
/// o modo para "Personalizada" e guarda os dois, para o outro não voltar ao padrão.
fn quality(app: &mut Unkvoid, ui: &mut Ui, saved: &serde_json::Value) {
    let (resolution, frame_rate) = share_mode(saved);
    let custom = |app: &mut Unkvoid, resolution: &str, frame_rate: u64| {
        app.actions
            .push(Action::SetVoice("sharePreset", serde_json::json!("custom")));
        app.actions
            .push(Action::SetVoice("shareResolution", serde_json::json!(resolution)));
        app.actions
            .push(Action::SetVoice("shareFps", serde_json::json!(frame_rate)));
    };

    ui.allocate_ui(vec2(104.0, 40.0), |ui| {
        let choices = FRAME_RATES.map(|value| widgets::Choice {
            name: format!("{value} fps"),
            detail: String::new(),
            selected: value == frame_rate,
        });

        if let Some(index) = widgets::select(ui, "share-fps", "", &choices, "") {
            custom(app, &resolution, FRAME_RATES[index]);
        }
    })
    .response
    .on_hover_text("Quadros por segundo");

    ui.add_space(8.0);
    ui.allocate_ui(vec2(116.0, 40.0), |ui| {
        let choices = RESOLUTIONS.map(|(value, name)| widgets::Choice {
            name: name.to_owned(),
            detail: String::new(),
            selected: value == resolution,
        });

        if let Some(index) = widgets::select(ui, "share-resolution", "", &choices, "") {
            custom(app, RESOLUTIONS[index].0, frame_rate);
        }
    })
    .response
    .on_hover_text("Resolução da transmissão");
}

/// O menu "Opções": o modo da transmissão, a resolução e os quadros do personalizado, e o som.
fn options(app: &mut Unkvoid, ui: &mut Ui, preset: &str, saved: &serde_json::Value) {
    ui.set_width(264.0);
    ui.label(
        egui::RichText::new("MODO DE TRANSMISSÃO")
            .font(theme::bold(12.0))
            .color(theme::MUTED),
    );
    ui.add_space(6.0);

    for (value, name, about) in PRESETS {
        let subtext = match value {
            "games" => format!("{about} (1080p, 60fps)"),
            "screen" => format!("{about} (Fonte, 15fps)"),
            _ => String::new(),
        };

        if menu_radio(ui, name, &subtext, preset == value) {
            app.actions
                .push(Action::SetVoice("sharePreset", serde_json::json!(value)));
        }
    }

    if preset == "custom" {
        let (resolution, frame_rate) = share_mode(saved);

        separator(ui);
        ui.menu_button("Resolução da tela", |ui| {
            for (value, name) in RESOLUTIONS {
                if menu_radio(ui, name, "", resolution == value) {
                    app.actions
                        .push(Action::SetVoice("shareResolution", serde_json::json!(value)));
                }
            }
        });
        ui.menu_button("Taxa de quadros", |ui| {
            for value in FRAME_RATES {
                if menu_radio(ui, &format!("{value}fps"), "", frame_rate == value) {
                    app.actions
                        .push(Action::SetVoice("shareFps", serde_json::json!(value)));
                }
            }
        });
    }

    separator(ui);

    let muted = saved["shareMuted"].as_bool().unwrap_or(false);

    if widgets::menu_check(ui, "Silenciar áudio da transmissão", muted, false) {
        app.actions
            .push(Action::SetVoice("shareMuted", serde_json::json!(!muted)));
    }

    // Ligado de fábrica: quem transmite quase sempre conversa pelo Discord ao mesmo tempo, e a
    // chamada de lá entraria na transmissão.
    let without_discord = saved["shareMuteDiscord"].as_bool().unwrap_or(true);

    if widgets::menu_check(ui, "Tirar o som do Discord", without_discord, false) {
        app.actions
            .push(Action::SetVoice("shareMuteDiscord", serde_json::json!(!without_discord)));
    }
}

fn separator(ui: &mut Ui) {
    ui.add_space(4.0);

    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());

    ui.painter().rect_filled(line, 0.0, theme::MENU_BORDER);
    ui.add_space(4.0);
}

/// Um item de menu de escolha única: o nome, a explicação embaixo e o círculo à direita.
fn menu_radio(ui: &mut Ui, name: &str, subtext: &str, checked: bool) -> bool {
    let height = if subtext.is_empty() { 34.0 } else { 48.0 };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), theme::SELECTED);
    }

    let tint = if response.hovered() {
        theme::BRIGHT
    } else {
        theme::TEXT
    };

    if subtext.is_empty() {
        ui.painter().text(
            pos2(rect.left() + 10.0, rect.center().y),
            Align2::LEFT_CENTER,
            name,
            theme::bold(14.0),
            tint,
        );
    } else {
        ui.painter().text(
            pos2(rect.left() + 10.0, rect.top() + 15.0),
            Align2::LEFT_CENTER,
            name,
            theme::bold(14.0),
            tint,
        );
        ui.painter().text(
            pos2(rect.left() + 10.0, rect.top() + 33.0),
            Align2::LEFT_CENTER,
            subtext,
            theme::regular(12.0),
            theme::MUTED,
        );
    }

    let circle = pos2(rect.right() - 19.0, rect.center().y);

    if checked {
        ui.painter().circle_filled(circle, 9.0, theme::ACCENT);
        ui.painter().circle_filled(circle, 3.5, Color32::WHITE);
    } else {
        ui.painter()
            .circle_stroke(circle, 8.0, Stroke::new(2.0, theme::MUTED));
    }

    response.clicked()
}

/// O "Vamos ligar a câmera?": a própria câmera na prévia, a escolha da câmera, o "Sempre ver
/// prévia do vídeo" e o "Ativar câmera".
pub fn camera_preview(app: &mut Unkvoid, ui: &mut Ui) {
    let width = 520.0;
    let response = egui::Modal::new(egui::Id::new("camera-preview"))
        .frame(
            Frame::new()
                .fill(theme::ELEVATED)
                .corner_radius(theme::RADIUS)
                .stroke(Stroke::new(1.0, theme::MENU_BORDER)),
        )
        .backdrop_color(Color32::from_black_alpha(170))
        .show(ui.ctx(), |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

            Frame::new()
                .inner_margin(egui::Margin {
                    left: 24,
                    right: 24,
                    top: 24,
                    bottom: 16,
                })
                .show(ui, |ui| {
                    ui.set_width(width - 48.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Vamos ligar a câmera?")
                                .font(theme::bold(20.0))
                                .color(theme::BRIGHT),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::icon_button(ui, icons::X, "Fechar", theme::MUTED)
                                .clicked()
                            {
                                app.actions.push(Action::CloseModal);
                            }
                        });
                    });
                    ui.add_space(16.0);

                    let (rect, _) = ui.allocate_exact_size(
                        vec2(width - 48.0, (width - 48.0) * 9.0 / 16.0),
                        Sense::hover(),
                    );

                    ui.painter().rect_filled(rect, theme::RADIUS, Color32::BLACK);

                    let none = app.cameras_known && app.cameras.is_empty();

                    match &app.camera_picture {
                        _ if none => {
                            ui.painter().text(
                                rect.center(),
                                Align2::CENTER_CENTER,
                                "Nenhuma câmera encontrada.",
                                theme::regular(15.0),
                                theme::DESCRIPTION,
                            );
                        }
                        Some(picture) => {
                            let fitted = super::call::fit(picture.size_vec2(), rect);

                            egui::Image::new((picture.id(), fitted.size()))
                                .corner_radius(theme::RADIUS)
                                .paint_at(ui, fitted);
                        }
                        None => {
                            ui.place(rect, egui::Spinner::new().size(32.0));
                        }
                    }

                    if app.cameras.len() > 1 {
                        ui.add_space(16.0);

                        let chosen = app.voice_settings["camera"]
                            .as_u64()
                            .and_then(|index| u32::try_from(index).ok())
                            .or_else(|| app.cameras.first().map(|camera| camera.index));
                        let choices: Vec<widgets::Choice> = app
                            .cameras
                            .iter()
                            .map(|camera| widgets::Choice {
                                name: camera.name.clone(),
                                detail: String::new(),
                                selected: Some(camera.index) == chosen,
                            })
                            .collect();

                        if let Some(index) =
                            widgets::select(ui, "preview-camera", icons::VIDEO_CAMERA, &choices, "")
                            && let Some(camera) = app.cameras.get(index)
                        {
                            app.actions
                                .push(Action::SetVoice("camera", serde_json::json!(camera.index)));
                            app.actions.push(Action::Show(Modal::CameraPreview));
                        }
                    }
                });

            Frame::new()
                .fill(theme::MENU)
                .corner_radius(CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 8,
                    se: 8,
                })
                .inner_margin(egui::Margin::symmetric(24, 16))
                .show(ui, |ui| {
                    ui.set_width(width - 48.0);
                    ui.horizontal_centered(|ui| {
                        let always = app.voice_settings["alwaysPreviewVideo"]
                            .as_bool()
                            .unwrap_or(true);

                        if check(ui, "Sempre ver prévia do vídeo", always) {
                            app.actions.push(Action::SetVoice(
                                "alwaysPreviewVideo",
                                serde_json::json!(!always),
                            ));
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let ready = !app.cameras_known || !app.cameras.is_empty();

                            if widgets::primary_button(ui, "Ativar câmera", ready, 0.0) {
                                app.actions.push(Action::StartCamera);
                            }
                        });
                    });
                });
        });

    if response.should_close() {
        app.actions.push(Action::CloseModal);
    }
}

/// A caixa de marcar do Discord com o texto ao lado.
fn check(ui: &mut Ui, label: &str, checked: bool) -> bool {
    let response = ui
        .horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());

            if checked {
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(4), theme::ACCENT);
                super::icons::paint_glyph(ui, rect, icons::CHECK, Color32::WHITE, 14.0);
            } else {
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::same(4),
                    Stroke::new(1.5, theme::MUTED),
                    egui::StrokeKind::Inside,
                );
            }

            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(label)
                    .font(theme::regular(14.0))
                    .color(theme::SECONDARY),
            );
        })
        .response
        .interact(Sense::click());

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mode_streams_at_its_own_size_and_rate() {
        assert_eq!(share_mode(&serde_json::json!({})), ("1080".to_owned(), 60));
        assert_eq!(
            share_mode(&serde_json::json!({ "sharePreset": "screen" })),
            ("2160".to_owned(), 15)
        );
        assert_eq!(
            share_mode(&serde_json::json!({ "sharePreset": "custom", "shareResolution": "1440", "shareFps": 30 })),
            ("1440".to_owned(), 30)
        );
        assert_eq!(
            share_mode(&serde_json::json!({ "sharePreset": "custom", "shareResolution": "999", "shareFps": 7 })),
            ("1080".to_owned(), 60),
            "um valor que o seletor não oferece volta ao padrão"
        );
    }
}
