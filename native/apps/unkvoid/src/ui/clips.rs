//! Os Clips, a outra parte do app, no lugar do hub ou da sala, como a aba Clips do app de antes:
//! o estado do replay e o salvar no alto, a galeria por jogo e o player. As configurações moram
//! no modal de Configurações, na seção Clips. O botão de filme da barra de cima abre e fecha.

use egui::{Frame, Ui};

use super::theme;
use crate::app::Unkvoid;
#[cfg(target_os = "windows")]
use {
    super::widgets,
    crate::app::{Action, Modal, SettingsPage},
    clips_engine::gallery::Clip,
    egui::{Color32, CornerRadius, Rect, Sense, pos2, vec2},
    egui_phosphor::regular as icons,
    std::path::PathBuf,
};

/// A largura mínima de um card da galeria: cabem quantos der na janela.
#[cfg(target_os = "windows")]
const CARD_WIDTH: f32 = 250.0;
#[cfg(target_os = "windows")]
const GAP: f32 = 16.0;

#[cfg(target_os = "windows")]
pub fn show(app: &mut Unkvoid, ui: &mut Ui) {
    bar(app, ui);

    egui::CentralPanel::no_frame()
        .frame(Frame::new().fill(theme::MAIN).inner_margin(egui::Margin::symmetric(22, 16)))
        .show(ui, |ui| {
            let clips = app.clips_runtime.clips();
            let open = clips
                .iter()
                .find(|clip| app.clips_runtime.player_path() == Some(clip.path.as_path()))
                .cloned();

            match open {
                Some(clip) => player(app, ui, &clip),
                None => {
                    // O clipe aberto sumiu da pasta (apagado no Explorer): o player não fica órfão.
                    app.clips_runtime.close_player();
                    gallery(app, ui, &clips);
                }
            }
        });

    confirm_delete(app, ui);
}

#[cfg(not(target_os = "windows"))]
pub fn show(_app: &mut Unkvoid, ui: &mut Ui) {
    egui::CentralPanel::no_frame()
        .frame(Frame::new().fill(theme::MAIN))
        .show(ui, |ui| {
            ui.centered_and_justified(|ui| {
                ui.label("O replay instantâneo está disponível no Windows.");
            });
        });
}

/// A linha de cima: o estado do replay numa pílula, e as Configurações e o salvar à direita.
#[cfg(target_os = "windows")]
fn bar(app: &mut Unkvoid, ui: &mut Ui) {
    egui::Panel::top("clips-bar")
        .exact_size(64.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(theme::MAIN))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            let health = app.clips_runtime.health();
            let settings = app.clips_runtime.settings.clone();
            let recording = settings.enabled && health.recording;

            ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0, theme::BORDER));
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.add_space(14.0);
                super::icons::label(ui, icons::FILM_STRIP, theme::icon(22.0), theme::SECONDARY);
                ui.label(egui::RichText::new("Clips").font(theme::bold(16.0)).color(theme::BRIGHT));
                ui.add_space(4.0);

                // O problema tem a faixa vermelha embaixo: a pílula não o repete.
                let status = if settings.enabled && health.problem.is_some() {
                    "Replay parado".to_owned()
                } else {
                    crate::clips::status_line(&settings, &health)
                };
                let text = ui.painter().layout_no_wrap(status, theme::regular(13.0), theme::TEXT);
                let (pill, _) = ui.allocate_exact_size(vec2(text.size().x + 40.0, 30.0), Sense::hover());

                ui.painter().rect(
                    pill,
                    CornerRadius::same(15),
                    theme::FIELD,
                    egui::Stroke::new(1.0, theme::FIELD_BORDER),
                    egui::StrokeKind::Inside,
                );
                ui.painter().circle_filled(
                    pos2(pill.left() + 16.0, pill.center().y),
                    4.0,
                    if recording { theme::DANGER } else { theme::DIM },
                );
                ui.painter().galley(pos2(pill.left() + 28.0, pill.center().y - text.size().y / 2.0), text, theme::TEXT);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(14.0);

                    if !settings.enabled {
                        if widgets::primary_button(ui, "Ligar replay", true, 150.0) {
                            app.clips_runtime.update(crate::clips::ClipsSettings { enabled: true, ..settings.clone() });
                        }
                    } else {
                        let saving = app.clips_runtime.saving();
                        let label = if saving {
                            "Salvando…".to_owned()
                        } else {
                            format!("Salvar últimos {} min", settings.replay_minutes)
                        };

                        // O mesmo caminho do atalho e da bandeja.
                        if widgets::primary_button(ui, &label, recording && !saving, 150.0) {
                            app.clips_runtime.request_save();
                        }
                    }

                    if widgets::secondary_button(ui, "Configurações") {
                        app.actions.push(Action::Show(Modal::Settings(SettingsPage::Clips)));
                    }
                });
            });
        });

    let problem = app.clips_runtime.health().problem.filter(|_| app.clips_runtime.settings.enabled);

    if let Some(problem) = problem {
        egui::Panel::top("clips-problem")
            .exact_size(38.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(theme::DANGER.gamma_multiply(0.25)))
            .show(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new(problem).font(theme::regular(13.0)).color(theme::BRIGHT));
                });
            });
    }
}

/// Os clipes salvos, do mais novo ao mais velho, com a fileira dos jogos no alto: "Todos" e um
/// por pasta de jogo. Clicar no card assiste; o X do hover apaga.
#[cfg(target_os = "windows")]
fn gallery(app: &mut Unkvoid, ui: &mut Ui, all: &[Clip]) {
    if all.is_empty() {
        let save = app.clips_runtime.settings.save_hotkey.label();
        let hint = if app.clips_runtime.settings.enabled {
            format!("O replay já está gravando. Aperte {save} no jogo para salvar os últimos minutos.")
        } else {
            format!("Ligue o replay e aperte {save} no jogo para salvar o que acabou de acontecer.")
        };

        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() / 2.0 - 40.0).max(0.0));
            ui.label(egui::RichText::new("Nenhum clipe ainda").font(theme::bold(18.0)).color(theme::BRIGHT));
            ui.add_space(6.0);
            ui.label(egui::RichText::new(hint).font(theme::regular(14.0)).color(theme::DESCRIPTION));
        });

        return;
    }

    let games = games(all);
    let game_id = egui::Id::new("clips-game");
    // O jogo escolhido some quando o último clipe dele sai: volta para "Todos".
    let mut chosen = ui
        .ctx()
        .data(|data| data.get_temp::<Option<String>>(game_id))
        .flatten()
        .filter(|chosen| games.iter().any(|(game, _)| game == chosen));

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);

        if chip(ui, &format!("Todos · {}", all.len()), chosen.is_none()) {
            chosen = None;
        }

        for (game, count) in &games {
            let label = format!("{} · {count}", clips_engine::gallery::game_label(game));

            if chip(ui, &label, chosen.as_ref() == Some(game)) {
                chosen = Some(game.clone());
            }
        }
    });
    ui.ctx().data_mut(|data| data.insert_temp(game_id, chosen.clone()));
    ui.add_space(GAP);

    let shown: Vec<&Clip> = all
        .iter()
        .filter(|clip| chosen.as_ref().is_none_or(|chosen| clip.game == *chosen))
        .collect();

    egui::ScrollArea::vertical()
        .id_salt("clips-gallery")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width();
            let columns = ((width + GAP) / (CARD_WIDTH + GAP)).floor().max(1.0);
            let card = (width - GAP * (columns - 1.0)) / columns;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let columns = columns as usize;

            for (row, line) in shown.chunks(columns).enumerate() {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = GAP;

                    for (column, clip) in line.iter().enumerate() {
                        // ponytail: miniatura só dos 60 mais novos; paginar se a pasta crescer muito.
                        if row * columns + column < 60 {
                            app.clips_runtime.request_thumbnail(&clip.path);
                        }

                        let (open, remove) = clip_card(app, ui, clip, card);

                        if remove {
                            ask_delete(ui, &clip.path);
                        } else if open {
                            open_clip(app, &clip.path);
                        }
                    }
                });
                ui.add_space(GAP);
            }
        });
}

/// As pastas de jogo da galeria, na ordem em que aparecem (a do clipe mais novo primeiro), com
/// quantos clipes cada uma tem.
#[cfg(target_os = "windows")]
fn games(clips: &[Clip]) -> Vec<(String, usize)> {
    let mut games: Vec<(String, usize)> = Vec::new();

    for clip in clips {
        match games.iter_mut().find(|(game, _)| *game == clip.game) {
            Some((_, count)) => *count += 1,
            None => games.push((clip.game.clone(), 1)),
        }
    }

    games
}

/// Um botão da fileira dos jogos: aceso o escolhido.
#[cfg(target_os = "windows")]
fn chip(ui: &mut Ui, label: &str, on: bool) -> bool {
    let button = egui::Button::new(
        egui::RichText::new(label)
            .font(theme::bold(13.0))
            .color(if on { theme::BRIGHT } else { theme::SECONDARY }),
    )
    .fill(if on { theme::SELECTED } else { theme::FIELD })
    .corner_radius(CornerRadius::same(16))
    .min_size(vec2(0.0, 32.0));

    ui.add(button).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Um clipe da grade: a miniatura 16:9 com o play e o X no hover, o nome, e embaixo a data, a
/// duração e o tamanho. Devolve se foi aberto e se o X foi clicado.
#[cfg(target_os = "windows")]
fn clip_card(app: &Unkvoid, ui: &mut Ui, clip: &Clip, width: f32) -> (bool, bool) {
    let height = width * 9.0 / 16.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height + 50.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let preview = Rect::from_min_size(rect.min, vec2(width, height));
    // Depois do card, para o X ficar por cima e levar o clique.
    let remove_rect = Rect::from_min_size(pos2(preview.right() - 38.0, preview.top() + 8.0), vec2(30.0, 30.0));
    let remove = ui
        .interact(remove_rect, egui::Id::new(("clip-remove", clip.path.clone())), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Apagar");
    let hovered = response.hovered() || remove.hovered();

    ui.painter().rect_filled(preview, CornerRadius::same(6), theme::FIELD);

    if let Some(thumbnail) = app.clips_runtime.thumbnail(&clip.path) {
        let fitted = super::call::fit(thumbnail.size_vec2(), preview);

        egui::Image::new((thumbnail.id(), fitted.size()))
            .corner_radius(CornerRadius::same(6))
            .paint_at(ui, fitted);
    }

    if hovered {
        ui.painter().rect_filled(preview, CornerRadius::same(6), Color32::from_black_alpha(110));
        super::icons::paint_glyph(ui, preview, icons::PLAY_CIRCLE, Color32::WHITE, 44.0);
        ui.painter().rect_stroke(
            preview.expand(3.0),
            CornerRadius::same(8),
            egui::Stroke::new(2.0, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
        ui.painter().rect_filled(
            remove_rect,
            CornerRadius::same(9),
            if remove.hovered() { theme::DANGER } else { Color32::from_black_alpha(200) },
        );
        super::icons::paint_glyph(ui, remove_rect, icons::X, Color32::WHITE, 14.0);
    }

    let painter = ui.painter();

    for (text, font, color, middle) in [
        (clip.title.clone(), theme::bold(14.0), theme::BRIGHT, 16.0),
        (format!("{} · {}", clip.date, clip.details), theme::regular(12.0), theme::MUTED, 36.0),
    ] {
        let mut job = egui::text::LayoutJob::simple_singleline(text, font, color);

        job.wrap = egui::text::TextWrapping::truncate_at_width(width);

        let galley = painter.layout_job(job);

        painter.galley(pos2(rect.left(), preview.bottom() + middle - galley.size().y / 2.0), galley, color);
    }

    (response.clicked(), remove.clicked())
}

#[cfg(target_os = "windows")]
fn open_clip(app: &mut Unkvoid, path: &std::path::Path) {
    if let Err(error) = app.clips_runtime.open_player(path) {
        tracing::warn!(error = %format!("{error:#}"), "clips: o player não abriu o clipe");
        app.notify("Não deu para abrir o clipe. O arquivo pode estar corrompido ou aberto em outro programa.", true);
    }
}

/// O clipe aberto: voltar, o nome (que se renomeia ali mesmo), mostrar na pasta e apagar no
/// alto; o vídeo no meio; tocar e pausar, a barra do tempo e o volume embaixo.
#[cfg(target_os = "windows")]
fn player(app: &mut Unkvoid, ui: &mut Ui, clip: &Clip) {
    let rename_id = egui::Id::new(("clips-rename", clip.path.clone()));
    let mut title = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(rename_id).unwrap_or_else(|| clip.title.clone()));
    let (mut back, mut rename, mut remove) = (false, false, false);

    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 40.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            back = widgets::icon_button(ui, icons::ARROW_LEFT, "Voltar para a galeria", theme::MUTED).clicked();

            let field = ui.add(
                egui::TextEdit::singleline(&mut title)
                    .font(theme::bold(16.0))
                    .desired_width(320.0),
            );

            if field.changed() {
                ui.ctx().data_mut(|data| data.insert_temp(rename_id, title.clone()));
            }

            let entered = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));

            rename = (widgets::secondary_button(ui, "Renomear") || entered) && title.trim() != clip.title;

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                remove = widgets::danger_button(ui, "Apagar", true);

                if widgets::secondary_button(ui, "Mostrar na pasta") {
                    crate::shell::show_in_folder(&clip.path);
                }
            });
        },
    );
    ui.add_space(12.0);

    let size = vec2(ui.available_width(), (ui.available_height() - 52.0).max(120.0));
    let (video, _) = ui.allocate_exact_size(size, Sense::hover());

    ui.painter().rect_filled(video, theme::RADIUS, Color32::BLACK);
    // O player é uma janela do Windows por cima da nossa, posta em cima deste retângulo a cada
    // quadro: segue a janela quando ela muda de tamanho.
    app.clips_runtime.place_player(video, ui.ctx().pixels_per_point());
    ui.add_space(12.0);

    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 40.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 14.0;

            let playing = app.clips_runtime.player_is_playing();
            let (icon, tip) = if playing { (icons::PAUSE, "Pausar") } else { (icons::PLAY, "Tocar") };

            if widgets::icon_button(ui, icon, tip, theme::TEXT).clicked() {
                app.clips_runtime.toggle_player();
            }

            // A barra anda sozinha enquanto o vídeo toca; o egui só redesenha quando pedem.
            if playing {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            }

            let (position, duration) = app.clips_runtime.player_position();
            #[allow(clippy::cast_possible_truncation)]
            let mut fraction = if duration.is_finite() && duration > 0.0 {
                (position / duration).clamp(0.0, 1.0) as f32
            } else {
                0.0
            };

            // O resto da linha: o tempo, o alto-falante, o volume e a porcentagem.
            ui.spacing_mut().slider_width = (ui.available_width() - 320.0).max(80.0);

            if ui
                .add(egui::Slider::new(&mut fraction, 0.0..=1.0).show_value(false).trailing_fill(true))
                .changed()
            {
                app.clips_runtime.seek_player(fraction);
            }

            ui.label(
                egui::RichText::new(format!("{} / {}", clock(position), clock(duration)))
                    .font(theme::regular(13.0))
                    .color(theme::SECONDARY),
            );

            let mut volume = app.clips_runtime.player_volume();

            super::icons::label(
                ui,
                if volume == 0 { icons::SPEAKER_SLASH } else { icons::SPEAKER_HIGH },
                theme::icon(18.0),
                theme::MUTED,
            );
            ui.spacing_mut().slider_width = 110.0;

            if ui
                .add(egui::Slider::new(&mut volume, 0..=100).show_value(false).trailing_fill(true))
                .changed()
            {
                app.clips_runtime.set_player_volume(volume);
            }

            ui.label(egui::RichText::new(format!("{volume}%")).font(theme::regular(12.0)).color(theme::SECONDARY));
        },
    );

    if back {
        app.clips_runtime.close_player();
    } else if remove {
        // A confirmação é desenhada pelo egui, e o vídeo fica por cima dela: o player fecha
        // antes de perguntar.
        app.clips_runtime.close_player();
        ask_delete(ui, &clip.path);
    } else if rename {
        // O player solta o arquivo antes: aberto, o Windows pode não deixar renomear.
        app.clips_runtime.close_player();

        let path = match crate::clips::rename_clip(&clip.path, &title) {
            Ok(renamed) => {
                ui.ctx().data_mut(|data| data.remove::<String>(rename_id));
                renamed
            }
            Err(error) => {
                app.notify(format!("Não deu para renomear o clipe. {error}"), true);
                clip.path.clone()
            }
        };

        open_clip(app, &path);
    }
}

/// O tempo do player: `1:05`.
#[cfg(target_os = "windows")]
fn clock(seconds: f64) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let seconds = if seconds.is_finite() { seconds.max(0.0) as u64 } else { 0 };

    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(target_os = "windows")]
fn ask_delete(ui: &Ui, path: &std::path::Path) {
    ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("clips-delete"), path.to_owned()));
}

/// O "Apagar este clipe?": o clipe vai para a Lixeira.
#[cfg(target_os = "windows")]
fn confirm_delete(app: &mut Unkvoid, ui: &mut Ui) {
    let id = egui::Id::new("clips-delete");
    let Some(path) = ui.ctx().data(|data| data.get_temp::<PathBuf>(id)) else {
        return;
    };
    let mut answer = None;
    let response = egui::Modal::new(egui::Id::new("clips-delete-modal"))
        .frame(
            Frame::new()
                .fill(theme::SIDEBAR)
                .corner_radius(CornerRadius::same(8))
                .inner_margin(egui::Margin::same(24)),
        )
        .backdrop_color(Color32::from_black_alpha(170))
        .show(ui.ctx(), |ui| {
            ui.set_width(380.0);
            ui.label(egui::RichText::new("Apagar este clipe?").font(theme::bold(20.0)).color(theme::BRIGHT));
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Ele vai para a Lixeira, dá para recuperar de lá.").color(theme::MUTED));
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                if widgets::secondary_button(ui, "Cancelar") {
                    answer = Some(false);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::danger_button(ui, "Apagar", true) {
                        answer = Some(true);
                    }
                });
            });
        });

    if response.should_close() && answer.is_none() {
        answer = Some(false);
    }

    let Some(delete) = answer else {
        return;
    };

    ui.ctx().data_mut(|data| data.remove::<PathBuf>(id));

    if delete && let Err(error) = crate::shell::move_to_recycle_bin(&path) {
        tracing::warn!(error = %error, "clips: o clipe não foi para a Lixeira");
        app.notify("Não deu para apagar o clipe. Ele pode estar aberto em outro programa.", true);
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn the_gallery_counts_each_game_in_the_order_the_clips_come() {
        let clip = |game: &str| Clip {
            path: PathBuf::new(),
            game: game.into(),
            title: String::new(),
            date: String::new(),
            details: String::new(),
        };
        let clips = [clip("ArenaBreakout"), clip("Desktop"), clip("ArenaBreakout"), clip("")];

        assert_eq!(
            games(&clips),
            [("ArenaBreakout".to_owned(), 2), ("Desktop".to_owned(), 1), (String::new(), 1)]
        );
        assert_eq!(clock(65.4), "1:05");
        assert_eq!(clock(f64::NAN), "0:00", "antes de o vídeo carregar, a duração não é número");
    }
}
