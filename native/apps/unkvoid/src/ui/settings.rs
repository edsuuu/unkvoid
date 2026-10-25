//! As configurações do usuário, no molde das do Discord de hoje: uma janela grande no meio, o
//! menu das seções à esquerda com a conta no topo, e a seção escolhida à direita, embaixo da
//! barra com o nome dela e o X. Esc ou o X fecham. A janela ([`window`]) é a mesma das
//! configurações do servidor e do canal, que no Discord ainda têm o X redondo com o "ESC".

use egui::{Align2, Color32, CornerRadius, Frame, Rect, Sense, Ui, pos2, vec2};
use egui_phosphor::regular as icons;

use super::{theme, voice_settings, widgets};
use crate::app::{Action, Modal, ProfileDraft, SettingsPage, Unkvoid, VoiceSection};
use crate::backend::Refresh;

const NAV_WIDTH: f32 = 252.0;
/// A coluna do conteúdo das configurações do usuário, centrada como a do Discord.
const COLUMN: f32 = 696.0;

pub fn show(app: &mut Unkvoid, ui: &mut Ui, page: SettingsPage) {
    let title = match page {
        SettingsPage::Account => "Conta",
        SettingsPage::Clips => "Clips",
        SettingsPage::Voice(_) => "Voz e vídeo",
    };

    window(
        app,
        ui,
        "settings",
        Some(title),
        |app, ui| navigation(app, ui, page),
        |app, ui| match page {
            SettingsPage::Account => account(app, ui),
            SettingsPage::Clips => clips_page(app, ui),
            SettingsPage::Voice(section) => voice_settings::page(app, ui, section),
        },
    );
}

/// A janela grande das configurações: o menu à esquerda e a página à direita. Com `title`, é a
/// das configurações do usuário (a barra com o nome e o X); sem, a do servidor e do canal (o X
/// redondo com o "ESC" no canto). Esc ou clique fora fecham.
pub(super) fn window(
    app: &mut Unkvoid,
    ui: &mut Ui,
    id: &str,
    title: Option<&str>,
    navigation: impl FnOnce(&mut Unkvoid, &mut Ui),
    page: impl FnOnce(&mut Unkvoid, &mut Ui),
) {
    let screen = ui.ctx().content_rect();
    let size = if title.is_some() {
        vec2(
            (screen.width() - 120.0).clamp(860.0, 1400.0),
            (screen.height() - 120.0).clamp(560.0, 900.0),
        )
    } else {
        vec2(
            (screen.width() - 120.0).clamp(860.0, 1160.0),
            (screen.height() - 100.0).clamp(560.0, 820.0),
        )
    };
    let (navigation_fill, page_fill) = if title.is_some() {
        (theme::MAIN, theme::ELEVATED)
    } else {
        (theme::SIDEBAR, theme::MAIN)
    };
    let response = egui::Modal::new(egui::Id::new(id))
        .frame(
            Frame::new()
                .fill(page_fill)
                .corner_radius(CornerRadius::same(8))
                .stroke(egui::Stroke::new(1.0, theme::SELECTED)),
        )
        .backdrop_color(Color32::from_black_alpha(170))
        .show(ui.ctx(), |ui| {
            ui.set_min_size(size);
            ui.set_max_size(size);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;

                Frame::new()
                    .fill(navigation_fill)
                    .corner_radius(CornerRadius {
                        nw: 8,
                        sw: 8,
                        ne: 0,
                        se: 0,
                    })
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_width(NAV_WIDTH - 32.0);
                        ui.set_height(size.y - 32.0);
                        // A área de rolagem herda o layout de fora, que aqui é horizontal.
                        egui::ScrollArea::vertical()
                            .id_salt("navigation")
                            .auto_shrink([false, false])
                            .show(ui, |ui| ui.vertical(|ui| navigation(app, ui)));
                    });

                match title {
                    Some(title) => titled_content(app, ui, id, title, size, page),
                    None => content(app, ui, id, size, page),
                }
            });
        });

    if response.should_close() {
        app.actions.push(Action::CloseModal);
    }
}

fn navigation(app: &mut Unkvoid, ui: &mut Ui, page: SettingsPage) {
    if let Some(me) = app.me.clone() {
        ui.horizontal(|ui| {
            widgets::avatar(ui, &me.name, me.avatar_url.as_deref(), 48.0);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(&me.name)
                        .font(theme::bold(16.0))
                        .color(theme::BRIGHT),
                );

                let edit = ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("Editar perfis {}", icons::PENCIL_SIMPLE))
                            .font(theme::regular(14.0))
                            .color(theme::SECONDARY),
                    )
                    .sense(Sense::click()),
                );

                if edit
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    app.actions
                        .push(Action::Show(Modal::ProfileEditor(ProfileDraft::from_user(
                            &me,
                        ))));
                }
            });
        });
    }

    ui.add_space(16.0);
    search(ui, &mut app.settings_search);
    ui.add_space(16.0);

    let query = app.settings_search.to_lowercase();

    if "conta".contains(&query)
        && nav_item(
            ui,
            icons::USER,
            "Conta",
            page == SettingsPage::Account,
            false,
        )
    {
        app.actions
            .push(Action::Show(Modal::Settings(SettingsPage::Account)));
    }

    if "clips replay instantâneo gravação".contains(&query)
        && nav_item(
            ui,
            icons::VIDEO_CAMERA,
            "Clips",
            page == SettingsPage::Clips,
            false,
        )
    {
        app.actions
            .push(Action::Show(Modal::Settings(SettingsPage::Clips)));
    }

    let sections: Vec<VoiceSection> = VoiceSection::ALL
        .into_iter()
        .filter(|section| {
            query.is_empty()
                || "voz e vídeo".contains(&query)
                || section.label().to_lowercase().contains(&query)
        })
        .collect();

    if !sections.is_empty() {
        divider(ui);
        nav_header(ui, "Experiência");

        let on_voice = matches!(page, SettingsPage::Voice(_));

        if nav_item(ui, icons::MICROPHONE, "Voz e vídeo", on_voice, false) {
            open_voice_section(app, ui, on_voice, VoiceSection::Voice);
        }

        if on_voice {
            let shown = voice_settings::shown(ui.ctx());

            for section in sections {
                if sub_nav_item(ui, section.label(), shown == section) {
                    open_voice_section(app, ui, on_voice, section);
                }
            }
        }
    }

    divider(ui);

    if "sair".contains(&query) && nav_item(ui, icons::SIGN_OUT, "Sair", false, true) {
        app.actions.push(Action::SignOut);
    }

    ui.add_space(16.0);
    ui.label(
        egui::RichText::new(format!("{} {}", crate::NAME, env!("CARGO_PKG_VERSION")))
            .font(theme::regular(12.0))
            .color(theme::DIM),
    );
}

/// Os Clips: as configurações do replay. A galeria e o player são uma parte do app à parte
/// (`ui/clips.rs`), aberta pelo botão de filme da barra de cima.
#[cfg(target_os = "windows")]
fn clips_page(app: &mut Unkvoid, ui: &mut Ui) {
    heading(ui, "Clips");
    clips_settings(app, ui);
}

/// A linha do estado do replay: cinza desligado, verde gravando, amarelo esperando a tela e
/// vermelho com problema.
#[cfg(target_os = "windows")]
fn replay_status(app: &Unkvoid, ui: &mut Ui) {
    let health = app.clips_runtime.health();
    let color = match (app.clips_runtime.settings.enabled, &health.problem, health.recording) {
        (false, ..) => theme::MUTED,
        (true, Some(_), _) => theme::DANGER,
        (true, None, true) => theme::SUCCESS,
        (true, None, false) => theme::WARNING,
    };

    ui.label(
        egui::RichText::new(crate::clips::status_line(&app.clips_runtime.settings, &health))
            .font(theme::regular(14.0))
            .color(color),
    );
}

/// O replay: ligar, quanto tempo e com que qualidade, o monitor, o som, os atalhos e a pasta.
#[cfg(target_os = "windows")]
fn clips_settings(app: &mut Unkvoid, ui: &mut Ui) {
    use crate::clips::{FRAME_RATES, REPLAY_MINUTES};
    use clips_engine::hotkeys::Slot;

    const QUALITIES: [&str; 3] = ["Eficiente", "Equilibrada", "Alta"];

    // Os monitores e os microfones são lidos na primeira vez que a página abre, e não a cada
    // quadro: a miniatura de cada monitor é uma cópia da tela.
    if app.clips_runtime.screens.is_empty() {
        app.clips_runtime.load_devices();
    }

    let mut settings = app.clips_runtime.settings.clone();

    if voice_settings::switch_row(
        ui,
        "Replay instantâneo",
        "Guarda os últimos minutos da tela para salvar o que acabou de acontecer.",
        settings.enabled,
    ) {
        settings.enabled = !settings.enabled;
    }

    ui.add_space(8.0);
    replay_status(app, ui);

    voice_settings::divider(ui);
    voice_settings::row(
        ui,
        "Duração do replay",
        "Quanto tempo para trás o clipe alcança.",
        200.0,
        |ui| {
            let choices = REPLAY_MINUTES.map(|minutes| widgets::Choice {
                name: format!("{minutes} minutos"),
                detail: String::new(),
                selected: settings.replay_minutes == minutes,
            });

            if let Some(index) = widgets::select(ui, "clips-minutes", "", &choices, "") {
                settings.replay_minutes = REPLAY_MINUTES[index];
            }
        },
    );
    ui.add_space(24.0);
    voice_settings::row(
        ui,
        "Qualidade",
        "Mais qualidade, arquivo maior.",
        200.0,
        |ui| {
            let choices = QUALITIES.map(|name| widgets::Choice {
                name: name.to_owned(),
                detail: String::new(),
                selected: QUALITIES[settings.quality.min(QUALITIES.len() - 1)] == name,
            });

            if let Some(index) = widgets::select(ui, "clips-quality", "", &choices, "") {
                settings.quality = index;
            }
        },
    );
    ui.add_space(24.0);
    voice_settings::row(
        ui,
        "Quadros por segundo",
        "60 deixa o movimento liso; 30 ocupa metade do disco.",
        200.0,
        |ui| {
            let choices = FRAME_RATES.map(|rate| widgets::Choice {
                name: format!("{rate} fps"),
                detail: String::new(),
                selected: settings.frame_rate == rate,
            });

            if let Some(index) = widgets::select(ui, "clips-fps", "", &choices, "") {
                settings.frame_rate = FRAME_RATES[index];
            }
        },
    );
    ui.add_space(16.0);
    ui.label(
        egui::RichText::new(settings.buffer_estimate())
            .font(theme::regular(14.0))
            .color(theme::DESCRIPTION),
    );

    voice_settings::divider(ui);
    voice_settings::label(ui, "Monitor gravado");
    ui.add_space(12.0);

    let selected = app.clips_runtime.selected_screen();

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(16.0, 16.0);

        for (position, screen) in app.clips_runtime.screens.iter().enumerate() {
            if screen_card(ui, screen, selected == Some(position)).clicked() {
                // O principal fica como "seguir o principal": quem troca o principal no Windows
                // leva o replay junto.
                settings.monitor = (!screen.info.primary).then_some(screen.info.index);
            }
        }
    });

    voice_settings::divider(ui);
    if voice_settings::switch_row(
        ui,
        "Som do computador",
        "O som do jogo e de tudo que toca no Windows.",
        settings.system_audio,
    ) {
        settings.system_audio = !settings.system_audio;
    }

    ui.add_space(24.0);
    if voice_settings::switch_row(ui, "Microfone", "A sua voz entra no clipe.", settings.microphone) {
        settings.microphone = !settings.microphone;
    }

    if settings.microphone {
        ui.add_space(24.0);
        voice_settings::row(ui, "Dispositivo de entrada", "", 300.0, |ui| {
            let microphones = &app.clips_runtime.microphones;
            let mut choices = vec![widgets::Choice {
                name: "Padrão do Windows".to_owned(),
                detail: String::new(),
                selected: settings.microphone_device.is_none(),
            }];

            choices.extend(microphones.iter().map(|microphone| widgets::Choice {
                name: microphone.label.clone(),
                detail: String::new(),
                selected: settings.microphone_device.as_deref() == Some(microphone.id.as_str()),
            }));

            if let Some(index) = widgets::select(ui, "clips-microphone", "", &choices, "") {
                settings.microphone_device = index.checked_sub(1).map(|index| microphones[index].id.clone());
            }
        });
        ui.add_space(24.0);

        if voice_settings::switch_row(
            ui,
            "Supressão de ruído",
            "Tira o teclado, o ventilador e o barulho de fundo da sua voz.",
            settings.noise_suppression,
        ) {
            settings.noise_suppression = !settings.noise_suppression;
        }
    }

    voice_settings::divider(ui);
    hotkey_row(
        app,
        ui,
        Slot::Save,
        "Atalho para salvar",
        "Aperte no jogo para salvar o clipe.",
        settings.save_hotkey.label(),
    );
    ui.add_space(24.0);
    hotkey_row(
        app,
        ui,
        Slot::Overlay,
        "Atalho para ligar o replay",
        "Com o replay desligado, liga; ligado, salva o clipe.",
        settings.overlay_hotkey.label(),
    );

    if let Some(message) = app.clips_runtime.hotkey_error() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(message).color(theme::DANGER));
    }

    ui.add_space(24.0);
    voice_settings::label(ui, "Pasta dos clipes");
    ui.add_space(8.0);
    let mut folder = settings.folder.to_string_lossy().into_owned();
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 40.0),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;

            if widgets::secondary_button(ui, "Abrir pasta") {
                crate::shell::open_folder(&settings.folder);
            }

            if widgets::secondary_button(ui, "Escolher pasta") {
                app.clips_runtime.request_folder();
            }

            if widgets::text_field(ui, &mut folder, "", false).changed() {
                settings.folder = folder.into();
            }
        },
    );

    ui.add_space(24.0);
    if voice_settings::switch_row(
        ui,
        "Tocar som ao salvar",
        "Um toque curto confirma que o clipe foi salvo.",
        settings.save_chime,
    ) {
        settings.save_chime = !settings.save_chime;
    }

    ui.add_space(24.0);
    if voice_settings::switch_row(
        ui,
        "Iniciar com o Windows",
        "O Unkvoid abre escondido na bandeja quando você entra no Windows.",
        settings.start_with_windows,
    ) {
        settings.start_with_windows = !settings.start_with_windows;
    }

    if settings != app.clips_runtime.settings {
        app.clips_runtime.update(settings);
    }
}

#[cfg(not(target_os = "windows"))]
fn clips_page(_app: &mut Unkvoid, ui: &mut Ui) {
    heading(ui, "Clips");
    ui.label("O replay instantâneo está disponível no Windows.");
}

/// A busca do menu, como a do Discord: a lupa à esquerda e o "Buscar" no fundo escuro.
fn search(ui: &mut Ui, query: &mut String) {
    Frame::new()
        .fill(theme::SEARCH)
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::symmetric(10, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (icon, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                super::icons::paint_glyph(ui, icon, icons::MAGNIFYING_GLASS, theme::SECONDARY, 18.0);
                ui.add(
                    egui::TextEdit::singleline(query)
                        .hint_text(
                            egui::RichText::new("Buscar")
                                .font(theme::regular(16.0))
                                .color(theme::PLACEHOLDER),
                        )
                        .frame(egui::Frame::NONE)
                        .font(theme::regular(16.0))
                        .desired_width(f32::INFINITY),
                );
            });
        });
}

/// Na página de Voz e vídeo, o menu rola até a seção; fora dela, abre a página já na seção.
fn open_voice_section(app: &mut Unkvoid, ui: &Ui, on_voice: bool, section: VoiceSection) {
    if on_voice {
        voice_settings::go_to(ui.ctx(), section);
    } else {
        app.actions
            .push(Action::Show(Modal::Settings(SettingsPage::Voice(section))));
    }
}

/// Um dos dois atalhos dos Clips: a combinação de agora e o botão que espera a próxima.
#[cfg(target_os = "windows")]
fn hotkey_row(
    app: &mut Unkvoid,
    ui: &mut Ui,
    slot: clips_engine::hotkeys::Slot,
    heading: &str,
    about: &str,
    hotkey: String,
) {
    let waiting = app.clips_runtime.capturing == Some(slot);

    voice_settings::row(ui, heading, about, 300.0, |ui| {
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), 38.0),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 12.0;

                let label = if waiting { "Aperte as teclas…" } else { "Alterar atalho" };

                if widgets::secondary_button(ui, label) && !waiting {
                    app.clips_runtime.capture_hotkey(slot);
                }

                ui.label(
                    egui::RichText::new(hotkey)
                        .font(theme::bold(15.0))
                        .color(theme::BRIGHT),
                );
            },
        );
    });
}

/// Um monitor para gravar: a miniatura do que ele mostra, o aro roxo no escolhido, e o nome com
/// a resolução embaixo.
#[cfg(target_os = "windows")]
fn screen_card(ui: &mut Ui, screen: &crate::clips::Screen, selected: bool) -> egui::Response {
    const WIDTH: f32 = 208.0;

    let preview_height = WIDTH * 9.0 / 16.0;
    let (rect, response) = ui.allocate_exact_size(vec2(WIDTH, preview_height + 46.0), Sense::click());
    let preview = Rect::from_min_size(rect.min, vec2(WIDTH, preview_height));

    ui.painter().rect_filled(preview, CornerRadius::same(6), Color32::BLACK);

    if let Some(texture) = &screen.preview {
        let fitted = super::call::fit(texture.size_vec2(), preview);

        egui::Image::new((texture.id(), fitted.size()))
            .corner_radius(CornerRadius::same(6))
            .paint_at(ui, fitted);
    }

    let painter = ui.painter();

    if selected || response.hovered() {
        painter.rect_stroke(
            preview.expand(3.0),
            CornerRadius::same(8),
            egui::Stroke::new(2.0, if selected { theme::ACCENT } else { theme::TRACK }),
            egui::StrokeKind::Inside,
        );
    }

    for (text, font, color, middle) in [
        (&screen.label, theme::bold(14.0), theme::BRIGHT, 14.0),
        (&screen.detail, theme::regular(12.0), theme::MUTED, 34.0),
    ] {
        let mut job = egui::text::LayoutJob::simple_singleline(text.clone(), font, color);

        job.wrap = egui::text::TextWrapping::truncate_at_width(WIDTH);

        let galley = painter.layout_job(job);

        painter.galley(pos2(rect.left(), preview.bottom() + middle - galley.size().y / 2.0), galley, color);
    }

    response
}

fn divider(ui: &mut Ui) {
    ui.add_space(8.0);

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());

    ui.painter().rect_filled(rect, 0.0, theme::MENU_BORDER);
    ui.add_space(8.0);
}

/// A seção de dentro de "Voz e vídeo", como no Discord: recuada, com o trilho fino à esquerda e
/// o traço branco na que está na tela.
fn sub_nav_item(ui: &mut Ui, label: &str, selected: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let rail = rect.left() + 18.0;

    ui.painter().vline(
        rail,
        rect.y_range(),
        egui::Stroke::new(1.0, theme::SELECTED),
    );

    if selected {
        ui.painter().vline(
            rail,
            rect.y_range().shrink(8.0),
            egui::Stroke::new(2.0, theme::BRIGHT),
        );
    }

    ui.painter().text(
        pos2(rect.left() + 36.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::regular(16.0),
        if selected || response.hovered() {
            theme::BRIGHT
        } else {
            theme::DESCRIPTION
        },
    );

    response.clicked()
}

/// O título dos grupos do menu ("Experiência").
pub(super) fn nav_header(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(text)
            .font(theme::bold(13.0))
            .color(theme::SECONDARY),
    );
    ui.add_space(6.0);
}

pub(super) fn nav_item(ui: &mut Ui, icon: &str, label: &str, selected: bool, danger: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    if selected {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::MENU_BORDER);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, theme::RADIUS, theme::HOVER);
    }

    let color = match (danger, selected || response.hovered()) {
        (true, _) => theme::DANGER,
        (false, true) => theme::BRIGHT,
        (false, false) => theme::DESCRIPTION,
    };

    super::icons::paint_glyph(
        ui,
        Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(20.0, 20.0)),
        icon,
        color,
        20.0,
    );
    ui.painter().text(
        pos2(rect.left() + 38.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::regular(16.0),
        color,
    );

    response.clicked()
}

/// O conteúdo das configurações do usuário: a barra de 48 px com o nome da página e o X, a
/// linha embaixo, e a página numa coluna centrada.
fn titled_content(
    app: &mut Unkvoid,
    ui: &mut Ui,
    id: &str,
    title: &str,
    size: egui::Vec2,
    page: impl FnOnce(&mut Unkvoid, &mut Ui),
) {
    let width = size.x - NAV_WIDTH;

    ui.vertical(|ui| {
        ui.set_width(width);

        let (bar, _) = ui.allocate_exact_size(vec2(width, 48.0), Sense::hover());

        ui.painter().text(
            pos2(bar.left() + 16.0, bar.center().y),
            Align2::LEFT_CENTER,
            title,
            theme::bold(16.0),
            theme::BRIGHT,
        );

        let close =
            Rect::from_center_size(pos2(bar.right() - 24.0, bar.center().y), vec2(32.0, 32.0));
        let closing = ui
            .interact(close, ui.id().with("close-settings"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);

        super::icons::paint_glyph(ui, close, icons::X, if closing.hovered() { theme::BRIGHT } else { theme::SECONDARY }, 20.0);

        if closing.clicked() {
            app.actions.push(Action::CloseModal);
        }

        ui.painter().hline(
            bar.x_range(),
            bar.bottom() - 0.5,
            egui::Stroke::new(1.0, theme::DIVIDER),
        );

        egui::ScrollArea::vertical()
            .id_salt(id)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let column = COLUMN.min(width - 48.0);
                let margin = ((ui.available_width() - column) / 2.0).max(0.0);

                ui.horizontal_top(|ui| {
                    ui.add_space(margin);
                    ui.vertical(|ui| {
                        ui.set_width(column);
                        page(app, ui);
                    });
                });
            });
    });
}

fn content(
    app: &mut Unkvoid,
    ui: &mut Ui,
    id: &str,
    size: egui::Vec2,
    page: impl FnOnce(&mut Unkvoid, &mut Ui),
) {
    Frame::new()
        .inner_margin(egui::Margin {
            left: 40,
            right: 24,
            top: 24,
            bottom: 24,
        })
        .show(ui, |ui| {
            let width = size.x - NAV_WIDTH - 64.0;

            ui.set_width(width);
            ui.set_height(size.y - 48.0);

            // O X do canto, com o "ESC" embaixo, como o Discord.
            let close = Rect::from_center_size(
                pos2(ui.max_rect().right() - 18.0, ui.max_rect().top() + 18.0),
                vec2(36.0, 36.0),
            );
            let closing = ui.interact(close, ui.id().with("close-settings"), Sense::click());

            ui.painter().circle_stroke(
                close.center(),
                17.0,
                egui::Stroke::new(
                    1.5,
                    if closing.hovered() {
                        theme::TEXT
                    } else {
                        theme::MUTED
                    },
                ),
            );
            super::icons::paint_glyph(ui, close, icons::X, if closing.hovered() { theme::TEXT } else { theme::MUTED }, 16.0);
            ui.painter().text(
                close.center_bottom() + vec2(0.0, 12.0),
                Align2::CENTER_CENTER,
                "ESC",
                theme::bold(11.0),
                theme::MUTED,
            );

            if closing.clicked() {
                app.actions.push(Action::CloseModal);
            }

            egui::ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_max_width((width - 60.0).min(740.0));
                    ui.vertical(|ui| page(app, ui));
                });
        });
}

/// A faixa do Discord quando há alteração não salva. `Some(true)` é salvar, `Some(false)` é
/// voltar ao que está no servidor.
pub(super) fn unsaved(ui: &mut Ui) -> Option<bool> {
    let mut choice = None;

    Frame::new()
        .fill(theme::RAIL)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Cuidado — você tem alterações que não foram salvas!")
                        .font(theme::bold(14.0)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::primary_button(ui, "Salvar alterações", true, 150.0) {
                        choice = Some(true);
                    }

                    let reset = ui.add(
                        egui::Label::new(
                            egui::RichText::new("Redefinir")
                                .font(theme::bold(14.0))
                                .color(theme::TEXT),
                        )
                        .sense(Sense::click()),
                    );

                    if reset.clicked() {
                        choice = Some(false);
                    }
                });
            });
        });

    choice
}

pub(super) fn heading(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).font(theme::bold(22.0)));
    ui.add_space(18.0);
}

fn account(app: &mut Unkvoid, ui: &mut Ui) {
    let Some(me) = app.me.clone() else {
        return;
    };

    heading(ui, "Conta");
    ui.label(egui::RichText::new("Informações da conta").font(theme::bold(18.0)));
    ui.add_space(8.0);
    Frame::new()
        .fill(theme::SIDEBAR)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            account_row(ui, "Nome de usuário", &me.name, |ui| {
                if !app.editing_account_name && widgets::secondary_button(ui, "Editar") {
                    app.editing_account_name = true;
                    app.nickname.value.clone_from(&me.name);
                }
            });
            // O e-mail de quem nasceu com o nome é o de mentira do servidor: não se mostra.
            if me.name_only {
                account_row(ui, "E-mail", "Nenhum ainda", |_| {});
            } else {
                let displayed_email = if app.email_visible {
                    me.email.clone()
                } else {
                    mask_email(&me.email)
                };
                account_row(ui, "E-mail", &displayed_email, |ui| {
                    if widgets::secondary_button(
                        ui,
                        if app.email_visible {
                            "Ocultar"
                        } else {
                            "Mostrar"
                        },
                    ) {
                        app.email_visible = !app.email_visible;
                    }
                });
            }
            if app.editing_account_name {
                ui.add_space(8.0);
                super::modals::nickname_field(app, ui);
            }
        });

    if me.name_only {
        ui.add_space(20.0);
        add_email(app, ui);
    }

    ui.add_space(20.0);
    ui.label(egui::RichText::new("Perfil").font(theme::bold(18.0)));
    if widgets::secondary_button(ui, "Editar perfis") {
        app.actions
            .push(Action::Show(Modal::ProfileEditor(ProfileDraft::from_user(
                &me,
            ))));
    }
    ui.add_space(20.0);
    ui.label(egui::RichText::new("Privacidade de atividade").font(theme::bold(18.0)));
    ui.add_space(8.0);
    if voice_settings::switch_row(
        ui,
        "Compartilhar minha atividade",
        "Mostra aos seus amigos o jogo em tela cheia que você está usando. Essa opção fica salva neste dispositivo.",
        app.activity_sharing,
    ) {
        app.actions
            .push(Action::SetActivitySharing(!app.activity_sharing));
    }
}

/// A conta que nasceu com o nome entra pelo apelido e a senha. O e-mail é opcional: com ele a
/// conta também entra pelo e-mail e consegue trocar a senha se esquecer.
fn add_email(app: &mut Unkvoid, ui: &mut Ui) {
    ui.label(egui::RichText::new("E-mail").font(theme::bold(18.0)));
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("Sua conta entra pelo apelido e a senha. Com um e-mail ela também entra por ele, e dá para trocar a senha se você esquecer.")
            .color(theme::MUTED),
    );
    ui.add_space(10.0);

    widgets::field_label(ui, "E-mail", None);
    ui.add_space(6.0);
    let email = widgets::text_field(ui, &mut app.add_email.email, "", false);

    if email.changed() {
        app.add_email.error = None;
    }

    let submitted = email.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));

    if let Some(error) = &app.add_email.error {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(error).color(theme::DANGER));
    }

    ui.add_space(12.0);

    let ready = !app.add_email.busy && !app.add_email.email.trim().is_empty();
    let label = if app.add_email.busy { "Um instante…" } else { "Salvar e-mail" };

    if widgets::primary_button(ui, label, ready, 220.0) || (submitted && ready) {
        app.actions.push(Action::AddEmail);
    }
}

fn account_row(ui: &mut Ui, label: &str, value: &str, button: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(label)
                    .font(theme::bold(13.0))
                    .color(theme::MUTED),
            );
            ui.label(egui::RichText::new(value).font(theme::regular(15.0)));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), button);
    });
    ui.add_space(12.0);
}

fn mask_email(email: &str) -> String {
    email.split_once('@').map_or_else(
        || "•••••".to_owned(),
        |(_, domain)| format!("•••••@{domain}"),
    )
}

pub(super) fn profile_editor(app: &mut Unkvoid, ui: &mut Ui, mut draft: ProfileDraft) {
    let Some(me) = app.me.clone() else {
        return;
    };
    let saved = ProfileDraft::from_user(&me);
    window(
        app,
        ui,
        "profile-editor",
        Some("Perfis"),
        |app, ui| {
            nav_header(ui, "Perfil");
            if nav_item(
                ui,
                icons::ARROW_LEFT,
                "Voltar às configurações",
                false,
                false,
            ) {
                app.actions
                    .push(Action::Show(Modal::Settings(SettingsPage::Account)));
            }
        },
        |app, ui| {
            heading(ui, "Editar perfis");
            widgets::columns(ui, 2, |columns| {
                let left = &mut columns[0];
                left.label(egui::RichText::new("Avatar").font(theme::bold(15.0)));
                left.horizontal(|ui| {
                    widgets::avatar(ui, &me.name, me.avatar_url.as_deref(), 56.0);
                    if widgets::secondary_button(ui, "Mudar") {
                        app.actions.push(Action::UploadImage {
                            route: "uploadAvatar",
                            params: serde_json::Value::Null,
                            field: "avatar",
                            refresh: Refresh::Me,
                        });
                    }
                    if me.avatar_uploaded && widgets::secondary_button(ui, "Remover") {
                        app.actions.push(Action::request(
                            "deleteAvatar",
                            serde_json::Value::Null,
                            serde_json::Value::Null,
                            Refresh::Me,
                        ));
                    }
                });
                left.add_space(12.0);
                left.label(egui::RichText::new("Cor da faixa").font(theme::bold(15.0)));
                left.horizontal_wrapped(|ui| {
                    for color in [
                        "#5865f2", "#e67e22", "#e84393", "#20a67a", "#9b59b6", "#ed4245",
                    ] {
                        let tint = theme::color_of(color).unwrap_or(theme::ACCENT);
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                        ui.painter().circle_filled(rect.center(), 10.0, tint);
                        if draft.banner_color == color {
                            ui.painter().circle_stroke(
                                rect.center(),
                                11.0,
                                egui::Stroke::new(2.0, theme::BRIGHT),
                            );
                        }
                        if response.clicked() {
                            draft.banner_color = color.to_owned();
                        }
                    }
                    if ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new("padrão").color(theme::ACCENT_TEXT),
                            )
                            .sense(Sense::click()),
                        )
                        .clicked()
                    {
                        draft.banner_color.clear();
                    }
                });
                left.add_space(12.0);
                widgets::field_label(left, "Pronomes", None);
                widgets::text_field(left, &mut draft.pronouns, "ela/dela", false);
                draft.pronouns = draft.pronouns.chars().take(40).collect();
                left.add_space(10.0);
                widgets::field_label(left, "Bio", None);
                left.add(
                    egui::TextEdit::multiline(&mut draft.bio)
                        .desired_rows(5)
                        .hint_text("Conte um pouco sobre você"),
                );
                draft.bio = draft.bio.chars().take(190).collect();
                left.label(
                    egui::RichText::new(format!("{}/190", draft.bio.chars().count()))
                        .font(theme::regular(12.0))
                        .color(theme::DIM),
                );
                left.add_space(10.0);
                widgets::field_label(left, "Status personalizado", None);
                widgets::text_field(left, &mut draft.custom_status, "O que está fazendo?", false);
                draft.custom_status = draft.custom_status.chars().take(128).collect();

                let right = &mut columns[1];
                right.label(egui::RichText::new("Prévia ao vivo").font(theme::bold(15.0)));
                right.add_space(8.0);
                let banner = theme::color_of(&draft.banner_color)
                    .unwrap_or_else(|| widgets::color_for(&me.name));
                Frame::new()
                    .fill(theme::MENU)
                    .stroke(egui::Stroke::new(1.0, theme::MENU_BORDER))
                    .corner_radius(theme::RADIUS)
                    .show(right, |ui| {
                        ui.set_width(ui.available_width());
                        let (rect, _) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 76.0), Sense::hover());
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius {
                                nw: 8,
                                ne: 8,
                                sw: 0,
                                se: 0,
                            },
                            banner,
                        );
                        ui.add_space(22.0);
                        ui.horizontal(|ui| {
                            widgets::avatar(ui, &me.name, me.avatar_url.as_deref(), 64.0);
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(&me.name).font(theme::bold(18.0)));
                                if !draft.pronouns.is_empty() {
                                    ui.label(
                                        egui::RichText::new(&draft.pronouns).color(theme::MUTED),
                                    );
                                }
                            });
                        });
                        if !draft.custom_status.is_empty() {
                            ui.label(egui::RichText::new(&draft.custom_status).color(theme::TEXT));
                        }
                        if !draft.bio.is_empty() {
                            ui.add_space(8.0);
                            ui.label(&draft.bio);
                        }
                        ui.add_space(10.0);
                    });
            });
            ui.add_space(16.0);
            let changed = draft != saved;
            if changed && let Some(save) = unsaved(ui) {
                if save {
                    app.actions.push(Action::request(
                        "updateMe",
                        serde_json::Value::Null,
                        serde_json::json!({
                            "bio": optional_text(&draft.bio),
                            "pronouns": optional_text(&draft.pronouns),
                            "banner_color": optional_text(&draft.banner_color),
                            "custom_status": optional_text(&draft.custom_status),
                        }),
                        Refresh::Me,
                    ));
                    app.actions
                        .push(Action::Show(Modal::Settings(SettingsPage::Account)));
                } else {
                    draft = saved.clone();
                }
            }
        },
    );
    if matches!(app.modal, Some(Modal::ProfileEditor(_))) {
        app.modal = Some(Modal::ProfileEditor(draft));
    }
}

fn optional_text(value: &str) -> serde_json::Value {
    if value.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(value.to_owned())
    }
}

/// O medidor do microfone: a barra acompanha o nível, e na detecção de voz a marca da
/// sensibilidade divide o que abre (verde) do que não abre (amarelo), como no Discord.
pub(super) fn meter(ui: &mut Ui, level: f32, threshold: Option<u64>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 10.0), Sense::hover());
    let open = threshold.is_none_or(|threshold| level * 100.0 >= threshold as f32);
    let segment_count = 24_usize;
    let gap = 2.0;
    let segment_width = (rect.width() - gap * (segment_count - 1) as f32) / segment_count as f32;
    let filled_segments = (level.clamp(0.0, 1.0) * segment_count as f32).ceil() as usize;
    for segment in 0..segment_count {
        let left = rect.left() + segment as f32 * (segment_width + gap);
        let segment_rect =
            Rect::from_min_size(pos2(left, rect.top()), vec2(segment_width, rect.height()));
        ui.painter().rect_filled(
            segment_rect,
            CornerRadius::same(2),
            if segment < filled_segments {
                if open { theme::SUCCESS } else { theme::WARNING }
            } else {
                theme::RAIL
            },
        );
    }

    if let Some(threshold) = threshold {
        let x = rect.left() + rect.width() * threshold as f32 / 100.0;

        ui.painter().vline(
            x,
            rect.y_range().expand(4.0),
            egui::Stroke::new(2.0, theme::TEXT),
        );
    }
}
