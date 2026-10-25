//! A página Voz e vídeo das configurações, como a do Discord: uma página só, com as seções uma
//! embaixo da outra. O menu da esquerda leva a cada uma e acende a que está no alto da tela.
//!
//! Só entra o que o Unkvoid faz de verdade: cancelamento de eco, apertar para falar e o controle
//! automático do microfone do Discord não existem aqui, e uma chave que não faz nada mentiria.

use egui::{Align, Align2, Color32, CornerRadius, Rect, Sense, Ui, pos2, vec2};
use egui_phosphor::regular as icons;

use core_app::chimes::Chime;

use super::{theme, widgets};
use crate::app::{Action, Unkvoid, VoiceSection};

/// A seção até onde a página rola no próximo quadro: o clique no menu, ou a pedida ao abrir.
const GOTO: &str = "voice-settings-goto";
/// A seção no alto da página agora, que o menu acende.
const SHOWN: &str = "voice-settings-shown";
/// O quadro em que a página foi desenhada por último: se não foi no anterior, ela acabou de abrir.
const LAST_PASS: &str = "voice-settings-pass";
const MORE_SOUNDS: &str = "voice-settings-more-sounds";
const MICROPHONE_TEST: &str = "voice-settings-microphone-test";

/// Os sons que aparecem antes do "Mostrar mais", na ordem do Discord.
const FIRST_SOUNDS: [Chime; 4] = [Chime::Deafened, Chime::Undeafened, Chime::Muted, Chime::Unmuted];

/// O que o "Redefinir" apaga: volta tudo ao padrão.
const VOICE_FIELDS: [&str; 13] = [
    "microphone",
    "speaker",
    "inputVolume",
    "outputVolume",
    "mode",
    "sensitivity",
    "noise",
    "noiseSuppress",
    "muteOnJoin",
    "soundboardVolume",
    "inputProfile",
    "camera",
    "alwaysPreviewVideo",
];

/// Os perfis de entrada: atalhos para os ajustes que já existem. "Personalizado" mostra cada um.
const PROFILES: [(&str, &str, &str); 3] = [
    (
        "isolation",
        "Isolamento de voz",
        "Só a sua voz: o ruído de fundo sai e o microfone abre quando você fala.",
    ),
    ("studio", "Estúdio", "O som cru: microfone sempre aberto e sem filtro nenhum."),
    ("custom", "Personalizado", "Você escolhe cada ajuste da entrada."),
];

pub fn page(app: &mut Unkvoid, ui: &mut Ui, asked: VoiceSection) {
    let saved = app.voice_settings.clone();
    let goto = going_to(ui, asked);
    let mut tops = Vec::new();

    ui.add_space(80.0);

    for (position, section) in VoiceSection::ALL.into_iter().enumerate() {
        if position > 0 {
            divider(ui);
        }

        let top = ui.cursor().top();

        if goto == Some(section) {
            ui.scroll_to_rect(
                Rect::from_min_size(pos2(ui.cursor().left(), top - 32.0), vec2(1.0, 1.0)),
                Some(Align::TOP),
            );
        }

        tops.push((section, top));

        match section {
            VoiceSection::Voice => voice(app, ui, &saved),
            VoiceSection::Video => video(app, ui, &saved),
            VoiceSection::Sounds => sounds(app, ui),
            VoiceSection::Soundboard => soundboard(app, ui, &saved),
            VoiceSection::Advanced => advanced(app, ui),
        }
    }

    ui.add_space(80.0);

    let view = ui.clip_rect();
    let shown = if ui.cursor().top() <= view.bottom() + 1.0 {
        VoiceSection::Advanced
    } else {
        tops.iter()
            .rev()
            .find(|(_, top)| *top <= view.top() + 120.0)
            .map_or(VoiceSection::Voice, |(section, _)| *section)
    };

    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new(SHOWN), shown));
}

/// A seção acesa no menu.
pub fn shown(context: &egui::Context) -> VoiceSection {
    context
        .data(|data| data.get_temp(egui::Id::new(SHOWN)))
        .unwrap_or(VoiceSection::Voice)
}

/// O clique no menu: a página rola até a seção.
pub fn go_to(context: &egui::Context, section: VoiceSection) {
    context.data_mut(|data| data.insert_temp(egui::Id::new(GOTO), section));
}

/// A seção pedida pelo menu, ou, se a página acabou de abrir, a que veio com ela.
fn going_to(ui: &Ui, asked: VoiceSection) -> Option<VoiceSection> {
    let pass = ui.ctx().cumulative_pass_nr();
    let last = ui.ctx().data_mut(|data| {
        let last = data.get_temp::<u64>(egui::Id::new(LAST_PASS));

        data.insert_temp(egui::Id::new(LAST_PASS), pass);

        last
    });
    let clicked = ui
        .ctx()
        .data_mut(|data| data.remove_temp::<VoiceSection>(egui::Id::new(GOTO)));

    if last.is_none_or(|last| last + 1 < pass) {
        return Some(asked);
    }

    clicked
}

fn title(ui: &mut Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .font(theme::regular(24.0))
            .color(theme::BRIGHT),
    );
    ui.add_space(30.0);
}

pub(super) fn label(ui: &mut Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .font(theme::bold(16.0))
            .color(theme::BRIGHT),
    );
}

fn description(ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .font(theme::regular(14.0))
                .color(theme::DESCRIPTION),
        )
        .wrap(),
    );
}

pub(super) fn divider(ui: &mut Ui) {
    ui.add_space(40.0);

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());

    ui.painter().rect_filled(rect, 0.0, theme::MENU_BORDER);
    ui.add_space(40.0);
}

/// Uma linha com o título e a explicação à esquerda e o controle à direita, como as do Discord.
pub(super) fn row(ui: &mut Ui, heading: &str, about: &str, control_width: f32, control: impl FnOnce(&mut Ui)) {
    ui.horizontal_top(|ui| {
        let text_width = (ui.available_width() - control_width - 24.0).max(0.0);

        ui.vertical(|ui| {
            ui.set_width(text_width);
            label(ui, heading);

            if !about.is_empty() {
                ui.add_space(4.0);
                description(ui, about);
            }
        });
        ui.with_layout(egui::Layout::right_to_left(Align::Min), |ui| {
            ui.set_width(control_width);
            control(ui);
        });
    });
}

pub(super) fn switch_row(ui: &mut Ui, heading: &str, about: &str, on: bool) -> bool {
    let mut clicked = false;

    row(ui, heading, about, 44.0, |ui| {
        clicked = widgets::switch(ui, on).clicked();
    });

    clicked
}

fn voice(app: &mut Unkvoid, ui: &mut Ui, saved: &serde_json::Value) {
    title(ui, "Voz");
    ui.spacing_mut().item_spacing.x = 16.0;
    widgets::columns(ui, 2, |columns| {
        device(app, &mut columns[0], true, saved);
        device(app, &mut columns[1], false, saved);
    });
    ui.add_space(24.0);
    widgets::columns(ui, 2, |columns| {
        volume(app, &mut columns[0], "Volume do microfone", "inputVolume", saved);
        volume(app, &mut columns[1], "Volume do alto-falante", "outputVolume", saved);
    });
    ui.add_space(24.0);

    let testing = ui
        .ctx()
        .data(|data| data.get_temp::<bool>(egui::Id::new(MICROPHONE_TEST)))
        .unwrap_or(false);

    ui.horizontal(|ui| {
        let button = egui::Button::new(
            egui::RichText::new(if testing { "Parar teste" } else { "Teste do microfone" })
                .font(theme::bold(16.0))
                .color(Color32::WHITE),
        )
        .fill(theme::ACCENT)
        .corner_radius(theme::RADIUS)
        .min_size(vec2(164.0, 40.0));

        if ui.add(button).clicked() {
            ui.ctx()
                .data_mut(|data| data.insert_temp(egui::Id::new(MICROPHONE_TEST), !testing));
        }

        ui.add_space(4.0);
        bars(ui, testing.then_some(app.call_level));
    });

    divider(ui);
    label(ui, "Perfil de entrada");
    ui.add_space(16.0);

    let profile = saved["inputProfile"].as_str().unwrap_or("custom").to_owned();

    for (value, name, about) in PROFILES {
        if radio(ui, profile == value, name, about) && profile != value {
            app.actions
                .push(Action::SetVoice("inputProfile", serde_json::json!(value)));

            match value {
                "isolation" => {
                    app.actions.push(Action::SetVoice("mode", serde_json::json!("voice")));
                    app.actions.push(Action::SetVoice("noise", serde_json::json!("light")));
                }
                "studio" => {
                    app.actions.push(Action::SetVoice("mode", serde_json::json!("open")));
                    app.actions.push(Action::SetVoice("noise", serde_json::json!("off")));
                }
                _ => {}
            }
        }

        ui.add_space(20.0);
    }

    if profile == "custom" {
        custom_input(app, ui, saved);
    }

    ui.add_space(24.0);

    let mute_on_join = saved["muteOnJoin"].as_bool().unwrap_or(false);

    if switch_row(
        ui,
        "Silenciar ao entrar",
        "Entra nos canais de voz com o microfone desligado.",
        mute_on_join,
    ) {
        app.actions
            .push(Action::SetVoice("muteOnJoin", serde_json::json!(!mute_on_join)));
    }
}

/// Os ajustes que o perfil "Personalizado" mostra: a sensibilidade e a supressão de ruído.
fn custom_input(app: &mut Unkvoid, ui: &mut Ui, saved: &serde_json::Value) {
    let detecting = saved["mode"].as_str().unwrap_or("open") == "voice";
    let sensitivity = saved["sensitivity"].as_u64().unwrap_or(35).min(100);

    ui.add_space(12.0);

    if switch_row(
        ui,
        "Sensibilidade de entrada",
        "Ligada, o microfone só transmite quando o som passa da marca. Desligada, transmite sempre.",
        detecting,
    ) {
        app.actions.push(Action::SetVoice(
            "mode",
            serde_json::json!(if detecting { "open" } else { "voice" }),
        ));
    }

    if detecting {
        ui.add_space(12.0);

        #[allow(clippy::cast_precision_loss)]
        if let Some(chosen) = sensitivity_slider(ui, sensitivity as f32, app.call_level) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            app.actions.push(Action::SetVoice(
                "sensitivity",
                serde_json::json!(chosen.round() as u64),
            ));
        }
    }

    ui.add_space(24.0);

    let noise = saved["noise"].as_str().map_or_else(
        || {
            if saved["noiseSuppress"].as_bool() == Some(true) {
                "light"
            } else {
                "off"
            }
            .to_owned()
        },
        str::to_owned,
    );

    row(
        ui,
        "Supressão de ruído",
        "Tira do seu microfone o ruído de fundo: teclado, ventilador, rua.",
        200.0,
        |ui| {
            let choices = [("light", "RNNoise"), ("off", "Nenhuma")].map(|(value, name)| {
                widgets::Choice {
                    name: name.to_owned(),
                    detail: String::new(),
                    selected: noise == value,
                }
            });

            if let Some(index) = widgets::select(ui, "noise", "", &choices, "Nenhuma") {
                let value = if index == 0 { "light" } else { "off" };

                app.actions
                    .push(Action::SetVoice("noise", serde_json::json!(value)));
            }
        },
    );
}

/// A escolha do aparelho: o padrão do sistema primeiro, e cada um com o tipo à parte, como o
/// Discord escreve ("HyperX SoloCast (Microfone)").
fn device(app: &mut Unkvoid, ui: &mut Ui, microphone: bool, saved: &serde_json::Value) {
    let (heading, field, icon) = if microphone {
        ("Microfone", "microphone", icons::MICROPHONE)
    } else {
        ("Alto-falante", "speaker", icons::HEADPHONES)
    };
    let devices = if microphone {
        app.microphones.clone()
    } else {
        app.speakers.clone()
    };
    let chosen = saved[field].as_str().unwrap_or_default().to_owned();
    let mut choices = vec![widgets::Choice {
        name: "Padrão".into(),
        detail: String::new(),
        selected: chosen.is_empty(),
    }];

    choices.extend(devices.iter().map(|device| {
        let (name, detail) = split_device(&device.label);

        widgets::Choice {
            name,
            detail,
            selected: device.id == chosen,
        }
    }));

    label(ui, heading);
    ui.add_space(8.0);

    if let Some(index) = widgets::select(ui, field, icon, &choices, "Padrão") {
        let value = index
            .checked_sub(1)
            .and_then(|index| devices.get(index))
            .map_or(serde_json::Value::Null, |device| serde_json::json!(device.id));

        app.actions.push(Action::SetVoice(field, value));
    }
}

/// "Microfone (HyperX SoloCast)", como o Windows chama, vira o nome e o tipo à parte.
fn split_device(label: &str) -> (String, String) {
    match label.split_once(" (") {
        Some((kind, name)) if name.ends_with(')') && !kind.is_empty() => {
            (name.trim_end_matches(')').to_owned(), format!("({kind})"))
        }
        _ => (label.to_owned(), String::new()),
    }
}

fn volume(app: &mut Unkvoid, ui: &mut Ui, heading: &str, field: &'static str, saved: &serde_json::Value) {
    label(ui, heading);
    ui.add_space(12.0);

    let volume = saved[field].as_u64().unwrap_or(100).min(200);

    #[allow(clippy::cast_precision_loss)]
    if let Some(value) = widgets::held_slider(ui, field, volume as f32, 0.0..=200.0, true) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        app.actions
            .push(Action::SetVoice(field, serde_json::json!(value.round() as u16)));
    }
}

/// Os traços do medidor do Discord: 4 px a cada 8, acesos até o nível do microfone. `None` é o
/// medidor parado, antes do teste.
fn bars(ui: &mut Ui, level: Option<f32>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let count = ((rect.width() + 4.0) / 8.0).floor().max(0.0);
    let lit = level.map_or(0.0, |level| (level.clamp(0.0, 1.0) * count).ceil());

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    for bar in 0..count as usize {
        #[allow(clippy::cast_precision_loss)]
        let left = rect.left() + bar as f32 * 8.0;
        #[allow(clippy::cast_precision_loss)]
        let color = if (bar as f32) < lit {
            theme::SUCCESS
        } else {
            theme::TRACK
        };

        ui.painter().rect_filled(
            Rect::from_min_size(pos2(left, rect.top()), vec2(4.0, rect.height())),
            CornerRadius::same(2),
            color,
        );
    }
}

/// A régua da sensibilidade do Discord: amarela até a marca (o que não abre o microfone), verde
/// depois, acesa até o nível do microfone agora. Arrastar move a marca; grava ao soltar.
fn sensitivity_slider(ui: &mut Ui, saved: f32, level: f32) -> Option<f32> {
    let id = ui.id().with("sensitivity-held");
    let held = ui.data(|data| data.get_temp::<f32>(id));
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click_and_drag());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let track = Rect::from_min_max(
        pos2(rect.left(), rect.center().y - 2.0),
        pos2(rect.right().max(rect.left() + 1.0), rect.center().y + 2.0),
    );
    let pointed = response
        .interact_pointer_pos()
        .map(|pointer| ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0) * 100.0);
    let value = if response.dragged() || response.drag_stopped() || response.clicked() {
        pointed.or(held).unwrap_or(saved)
    } else {
        held.unwrap_or(saved)
    };
    let mark = track.left() + track.width() * value / 100.0;
    let heard = track.left() + track.width() * level.clamp(0.0, 1.0);
    let painter = ui.painter();

    painter.rect_filled(
        Rect::from_min_max(track.min, pos2(mark, track.max.y)),
        CornerRadius::same(2),
        theme::SENSITIVITY_LOW.gamma_multiply(0.45),
    );
    painter.rect_filled(
        Rect::from_min_max(pos2(mark, track.min.y), track.max),
        CornerRadius::same(2),
        theme::SENSITIVITY_HIGH.gamma_multiply(0.45),
    );
    painter.rect_filled(
        Rect::from_min_max(track.min, pos2(heard.min(mark), track.max.y)),
        CornerRadius::same(2),
        theme::SENSITIVITY_LOW,
    );

    if heard > mark {
        painter.rect_filled(
            Rect::from_min_max(pos2(mark, track.min.y), pos2(heard, track.max.y)),
            CornerRadius::same(2),
            theme::SENSITIVITY_HIGH,
        );
    }

    painter.circle_filled(pos2(mark, track.center().y), 8.0, Color32::WHITE);

    if response.dragged() {
        ui.data_mut(|data| data.insert_temp(id, value));

        return None;
    }

    ui.data_mut(|data| data.remove::<f32>(id));

    (response.drag_stopped() || response.clicked()).then_some(value)
}

/// O botão de escolha do Discord: o aro cinza, ou cheio da cor com o miolo branco quando
/// escolhido; o nome e a explicação ao lado.
fn radio(ui: &mut Ui, selected: bool, name: &str, about: &str) -> bool {
    let response = ui
        .horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;

            let (circle, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());

            if selected {
                ui.painter().circle_filled(circle.center(), 10.0, theme::ACCENT);
                ui.painter().circle_filled(circle.center(), 4.0, Color32::WHITE);
            } else {
                ui.painter().circle_stroke(
                    circle.center(),
                    9.0,
                    egui::Stroke::new(2.0, theme::TRACK),
                );
            }

            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(name)
                        .font(theme::regular(16.0))
                        .color(theme::BRIGHT),
                );
                description(ui, about);
            });
        })
        .response
        .interact(Sense::click());

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn video(app: &mut Unkvoid, ui: &mut Ui, saved: &serde_json::Value) {
    title(ui, "Vídeo");

    if !crate::camera::FILMS {
        description(ui, "A câmera ainda não chegou ao Unkvoid neste sistema.");

        return;
    }

    let chosen = saved["camera"]
        .as_u64()
        .and_then(|index| u32::try_from(index).ok());
    let selected = chosen
        .filter(|index| app.cameras.iter().any(|camera| camera.index == *index))
        .or_else(|| app.cameras.first().map(|camera| camera.index));
    let choices: Vec<widgets::Choice> = app
        .cameras
        .iter()
        .map(|camera| widgets::Choice {
            name: camera.name.clone(),
            detail: String::new(),
            selected: Some(camera.index) == selected,
        })
        .collect();

    label(ui, "Câmera");
    ui.add_space(8.0);
    ui.scope(|ui| {
        ui.set_max_width((ui.available_width() - 16.0) / 2.0);

        if let Some(index) =
            widgets::select(ui, "camera", icons::VIDEO_CAMERA, &choices, "Nenhuma câmera encontrada")
            && let Some(camera) = app.cameras.get(index)
        {
            app.actions
                .push(Action::SetVoice("camera", serde_json::json!(camera.index)));

            if app.video_test {
                app.actions.push(Action::TestVideo(true));
            }
        }
    });
    ui.add_space(16.0);
    preview(app, ui, selected.is_some());
    ui.add_space(24.0);

    let always = saved["alwaysPreviewVideo"].as_bool().unwrap_or(true);

    if switch_row(
        ui,
        "Sempre ver prévia do vídeo",
        "Mostra a câmera antes de ligá-la numa chamada.",
        always,
    ) {
        app.actions.push(Action::SetVoice(
            "alwaysPreviewVideo",
            serde_json::json!(!always),
        ));
    }
}

/// O quadro da prévia: a própria câmera durante o teste, ou o botão de testar.
fn preview(app: &mut Unkvoid, ui: &mut Ui, has_camera: bool) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, (width * 9.0 / 16.0).min(392.0)), Sense::hover());

    ui.painter().rect_filled(rect, theme::RADIUS, theme::RAIL);

    if !crate::camera::PREVIEWS {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "A prévia da câmera ainda não chegou a este sistema; ela liga direto na chamada.",
            theme::regular(14.0),
            theme::DESCRIPTION,
        );

        return;
    }

    if app.video_test
        && let Some(picture) = &app.camera_picture
    {
        let shown = super::call::fit(picture.size_vec2(), rect);

        egui::Image::new((picture.id(), shown.size()))
            .corner_radius(theme::RADIUS)
            .paint_at(ui, shown);
    } else {
        crate::ui::icons::text(ui.painter(), rect.center() - vec2(0.0, 36.0), Align2::CENTER_CENTER, icons::VIDEO_CAMERA, theme::icon(48.0), theme::DIM);
    }

    let button_rect = Rect::from_center_size(
        if app.video_test {
            pos2(rect.center().x, rect.bottom() - 40.0)
        } else {
            rect.center() + vec2(0.0, 28.0)
        },
        vec2(136.0, 40.0),
    );
    let button = egui::Button::new(
        egui::RichText::new(if app.video_test { "Parar vídeo" } else { "Testar vídeo" })
            .font(theme::bold(16.0))
            .color(Color32::WHITE),
    )
    .fill(if app.video_test {
        theme::BUTTON
    } else if has_camera {
        theme::ACCENT
    } else {
        theme::ACCENT.gamma_multiply(0.5)
    })
    .corner_radius(theme::RADIUS);

    if ui.place(button_rect, button.min_size(button_rect.size())).clicked() && has_camera {
        app.actions.push(Action::TestVideo(!app.video_test));
    }
}

fn sounds(app: &mut Unkvoid, ui: &mut Ui) {
    let saved = app.sound_settings.clone();
    let more = ui
        .ctx()
        .data(|data| data.get_temp::<bool>(egui::Id::new(MORE_SOUNDS)))
        .unwrap_or(false);
    let rest: Vec<Chime> = Chime::ALL
        .into_iter()
        .filter(|chime| !FIRST_SOUNDS.contains(chime))
        .collect();

    title(ui, "Sons");

    for chime in FIRST_SOUNDS.iter().chain(rest.iter().filter(|_| more)) {
        let on = chime.allowed(Some(&saved));

        row(ui, sound_label(*chime), "", 44.0, |ui| {
            if widgets::switch(ui, on).clicked() {
                app.actions.push(Action::SetSound(*chime, !on));
            }
        });

        let preview = ui.add(
            egui::Label::new(
                egui::RichText::new("Prévia do som")
                    .font(theme::regular(14.0))
                    .color(theme::ACCENT_TEXT),
            )
            .sense(Sense::click()),
        );

        if preview
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            app.actions.push(Action::PreviewSound(*chime));
        }

        ui.add_space(16.0);

        let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());

        ui.painter().rect_filled(line, 0.0, theme::MENU_BORDER);
        ui.add_space(16.0);
    }

    let names: Vec<&str> = rest.iter().take(3).map(|chime| sound_label(*chime)).collect();
    let heading = if more {
        "Ocultar sons".to_owned()
    } else {
        format!("Mostrar {} mais sons", rest.len())
    };
    let about = if more {
        String::new()
    } else {
        format!("{} e mais", names.join(", "))
    };

    if expander(ui, &heading, &about, more) {
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new(MORE_SOUNDS), !more));
    }
}

/// A linha que abre mais itens, com o botão da seta à direita e o fundo no hover.
fn expander(ui: &mut Ui, heading: &str, about: &str, open: bool) -> bool {
    // O texto fica na linha da coluna e o fundo do hover passa 12 px para fora, como no Discord.
    let response = egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(12, 10))
        .outer_margin(egui::Margin::symmetric(-12, 0))
        .corner_radius(theme::RADIUS)
        .show(ui, |ui| {
            row(ui, heading, about, 40.0, |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());

                ui.painter().rect_filled(rect, theme::RADIUS, theme::BUTTON);
                crate::ui::icons::text(ui.painter(), rect.center(), Align2::CENTER_CENTER, if open { icons::CARET_UP } else { icons::CARET_DOWN }, theme::icon(20.0), theme::TEXT);
            });
        })
        .response
        .interact(Sense::click());

    if response.hovered() {
        ui.painter().rect_filled(
            response.rect,
            theme::RADIUS,
            Color32::from_white_alpha(6),
        );
    }

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn soundboard(app: &mut Unkvoid, ui: &mut Ui, saved: &serde_json::Value) {
    title(ui, "Painel de efeitos sonoros");
    label(ui, "Volume dos efeitos sonoros");
    ui.add_space(4.0);
    description(ui, "O volume dos sons do painel, só para você.");
    ui.add_space(12.0);

    let volume = saved["soundboardVolume"].as_u64().unwrap_or(100).min(100);

    #[allow(clippy::cast_precision_loss)]
    if let Some(value) =
        widgets::held_slider(ui, "soundboardVolume", volume as f32, 0.0..=100.0, true)
    {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        app.actions.push(Action::SetVoice(
            "soundboardVolume",
            serde_json::json!(value.round() as u16),
        ));
    }
}

fn advanced(app: &mut Unkvoid, ui: &mut Ui) {
    title(ui, "Avançado");
    row(
        ui,
        "Redefinir todas as configurações de Voz e vídeo",
        "Volta ao padrão os aparelhos, os volumes, o perfil de entrada, a câmera e os sons.",
        110.0,
        |ui| {
            let button = egui::Button::new(
                egui::RichText::new("Redefinir")
                    .font(theme::bold(16.0))
                    .color(theme::DANGER),
            )
            .fill(theme::BUTTON)
            .corner_radius(theme::RADIUS)
            .min_size(vec2(100.0, 40.0));

            if ui.add(button).clicked() {
                for field in VOICE_FIELDS {
                    app.actions
                        .push(Action::SetVoice(field, serde_json::Value::Null));
                }

                for chime in Chime::ALL {
                    app.actions.push(Action::SetSound(chime, true));
                }
            }
        },
    );
}

/// O nome de cada som na tela, nos termos do Discord.
fn sound_label(chime: Chime) -> &'static str {
    match chime {
        Chime::Deafened => "Desativar áudio",
        Chime::Undeafened => "Reativar áudio",
        Chime::Muted => "Silenciar",
        Chime::Unmuted => "Dessilenciar",
        Chime::Disconnected => "Voz desconectada",
        Chime::TalkStarted => "Apertar para falar ativado",
        Chime::TalkStopped => "Apertar para falar desativado",
        Chime::Joined => "Usuário entrou",
        Chime::Left => "Usuário saiu",
        Chime::Moved => "Usuário movido",
        Chime::StreamStarted => "Transmissão iniciada",
        Chime::StreamStopped => "Transmissão interrompida",
        Chime::ViewerJoined => "Espectador entrou",
        Chime::ViewerLeft => "Espectador saiu",
        Chime::CameraOn => "Câmera ligada",
        Chime::CameraOff => "Câmera desligada",
        Chime::Message => "Mensagem",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_has_a_name_on_the_screen() {
        let labels: std::collections::HashSet<&str> =
            Chime::ALL.iter().map(|chime| sound_label(*chime)).collect();

        assert_eq!(
            labels.len(),
            Chime::ALL.len(),
            "dois sons com o mesmo nome na tela"
        );
    }

    #[test]
    fn a_windows_device_name_shows_the_name_first_and_the_kind_apart() {
        assert_eq!(
            split_device("Microfone (3- HyperX SoloCast)"),
            ("3- HyperX SoloCast".to_owned(), "(Microfone)".to_owned())
        );
        assert_eq!(split_device("Realtek"), ("Realtek".to_owned(), String::new()));
        assert_eq!(split_device("(estranho)"), ("(estranho)".to_owned(), String::new()));
    }
}
