//! A janela de diagnóstico mostra somente medidas que o transporte já publica.

use std::time::{Duration, Instant};

use egui::{CornerRadius, Frame, Sense, Ui, pos2, vec2};
use egui_phosphor::regular as icons;

use super::{settings, theme, widgets};
use crate::app::{Action, Modal, Unkvoid};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugPage {
    Transport,
    Output,
}

pub fn show(app: &mut Unkvoid, ui: &mut Ui, page: DebugPage) {
    settings::window(
        app,
        ui,
        "voice-debug",
        None,
        |app, ui| navigation(app, ui, page),
        |app, ui| content(app, ui, page),
    );
}

fn navigation(app: &mut Unkvoid, ui: &mut Ui, page: DebugPage) {
    ui.add_space(10.0);
    ui.label(
        egui::RichText::new(
            app.call
                .seat
                .as_ref()
                .map_or("Voz".to_owned(), |seat| seat.name.clone()),
        )
        .font(theme::bold(17.0)),
    );
    ui.add_space(5.0);
    ui.label(
        egui::RichText::new(connection_status(app))
            .font(theme::regular(13.0))
            .color(status_color(app)),
    );
    ui.add_space(18.0);

    for (target, label, icon) in [
        (DebugPage::Transport, "Transporte", icons::ARROWS_LEFT_RIGHT),
        (DebugPage::Output, "Saída", icons::SPEAKER_HIGH),
    ] {
        let selected = page == target;
        let response = ui.add(
            egui::Button::new(
                egui::RichText::new(format!("{icon}  {label}"))
                    .font(theme::regular(14.0))
                    .color(if selected {
                        theme::TEXT
                    } else {
                        theme::SECONDARY
                    }),
            )
            .fill(if selected {
                theme::SELECTED
            } else {
                theme::SIDEBAR
            })
            .corner_radius(theme::RADIUS)
            .min_size(vec2(ui.available_width(), 36.0)),
        );

        if response.clicked() {
            app.actions.push(Action::Show(Modal::VoiceDebug(target)));
        }
    }
}

fn content(app: &mut Unkvoid, ui: &mut Ui, page: DebugPage) {
    ui.add_space(24.0);
    ui.label(egui::RichText::new("Depuração da voz").font(theme::bold(22.0)));
    ui.add_space(18.0);

    if page == DebugPage::Transport {
        widgets::columns(ui, 2, |columns| {
            metric_card(
                &mut columns[0],
                "Ping",
                app.call
                    .ping
                    .map_or_else(|| "—".to_owned(), |value| format!("{value} ms")),
                &app.call.ping_history.samples,
            );
        });
        ui.add_space(14.0);
        ui.label(egui::RichText::new("Contadores de pacotes, bytes, taxa de saída e perda não são publicados para o áudio da chamada.").font(theme::regular(13.0)).color(theme::SECONDARY));
    } else {
        ui.label(
            egui::RichText::new(
                "O núcleo ainda não publica métricas de saída de áudio para esta tela.",
            )
            .font(theme::regular(13.0))
            .color(theme::SECONDARY),
        );
    }
}

fn metric_card(
    ui: &mut Ui,
    title: &str,
    value: String,
    samples: &std::collections::VecDeque<crate::voice::PingSample>,
) {
    Frame::new()
        .fill(theme::ELEVATED)
        .stroke(egui::Stroke::new(1.0, theme::MENU_BORDER))
        .corner_radius(theme::RADIUS)
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(title).font(theme::bold(14.0)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(value)
                            .font(theme::bold(14.0))
                            .color(theme::TEXT),
                    );
                });
            });
            ui.add_space(10.0);
            minute_graph(ui, samples);
        });
}

fn minute_graph(ui: &mut Ui, samples: &std::collections::VecDeque<crate::voice::PingSample>) {
    let now = Instant::now();
    let values = minute_values(samples, now);
    let scale = graph_scale(&values);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 76.0), Sense::hover());
    let plot = rect.shrink2(vec2(4.0, 4.0));
    ui.painter()
        .rect_filled(plot, CornerRadius::same(4), theme::RAIL);
    for minute in 0..=4 {
        let x = plot.left() + plot.width() * minute as f32 / 4.0;
        ui.painter()
            .vline(x, plot.y_range(), egui::Stroke::new(1.0, theme::DIVIDER));
    }
    let mut points = Vec::new();
    for (minute, value) in values {
        let x = plot.right() - plot.width() * minute as f32 / 4.0;
        let y = plot.bottom() - plot.height() * value as f32 / scale as f32;
        points.push(pos2(x, y));
    }
    if points.len() > 1 {
        ui.painter().add(egui::Shape::line(
            points,
            egui::Stroke::new(2.0, theme::ACCENT),
        ));
    } else if let Some(point) = points.first() {
        ui.painter().circle_filled(*point, 2.0, theme::ACCENT);
    }
}

fn minute_values(
    samples: &std::collections::VecDeque<crate::voice::PingSample>,
    now: Instant,
) -> Vec<(u8, u64)> {
    let mut values = Vec::new();
    for sample in samples
        .iter()
        .filter(|sample| now.saturating_duration_since(sample.at) <= Duration::from_secs(5 * 60))
    {
        let minute = u8::try_from((now.saturating_duration_since(sample.at).as_secs() / 60).min(4))
            .unwrap_or(4);
        if let Some((_, current)) = values.iter_mut().find(|(bucket, _)| *bucket == minute) {
            *current = sample.milliseconds;
        } else {
            values.push((minute, sample.milliseconds));
        }
    }
    values
}

fn graph_scale(values: &[(u8, u64)]) -> u64 {
    values
        .iter()
        .map(|(_, value)| *value)
        .max()
        .unwrap_or(1)
        .max(1)
}

fn connection_status(app: &Unkvoid) -> &'static str {
    match app.call.status {
        crate::voice::Status::Connected => "Conectado",
        crate::voice::Status::Connecting => "Conectando…",
        crate::voice::Status::Reconnecting => "Sem rota",
    }
}

fn status_color(app: &Unkvoid) -> egui::Color32 {
    match app.call.status {
        crate::voice::Status::Connected => theme::SUCCESS,
        crate::voice::Status::Connecting => theme::WARNING,
        crate::voice::Status::Reconnecting => theme::DANGER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::PingSample;
    use std::collections::VecDeque;

    #[test]
    fn samples_are_bucketed_by_minute_and_old_samples_are_ignored() {
        let now = Instant::now();
        let samples = VecDeque::from([
            PingSample {
                at: now - Duration::from_secs(30),
                milliseconds: 20,
            },
            PingSample {
                at: now - Duration::from_secs(90),
                milliseconds: 40,
            },
            PingSample {
                at: now - Duration::from_secs(6 * 60),
                milliseconds: 900,
            },
        ]);
        assert_eq!(minute_values(&samples, now), vec![(0, 20), (1, 40)]);
    }

    #[test]
    fn chart_scale_tracks_the_largest_sample_and_never_divides_by_zero() {
        assert_eq!(graph_scale(&[(0, 12), (1, 80), (2, 24)]), 80);
        assert_eq!(graph_scale(&[]), 1);
    }
}
