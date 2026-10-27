//! As peças que todas as telas usam: o avatar, o botão de ícone, o menu e os campos.

use egui::{Align2, Color32, CornerRadius, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};

use super::theme;
use super::icons as discord_icons;
use core_app::models::UserStatus;
use crate::words::initial;

/// O avatar redondo: a foto quando há uma, senão a inicial sobre uma cor tirada do nome — a
/// mesma pessoa tem sempre a mesma cor.
pub fn avatar(ui: &mut Ui, name: &str, url: Option<&str>, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());

    paint_avatar(ui, rect, name, url);

    response
}

pub fn paint_avatar(ui: &Ui, rect: Rect, name: &str, url: Option<&str>) {
    if !ui.is_rect_visible(rect) {
        return;
    }

    match url {
        Some(url) => {
            egui::Image::new(url.to_owned())
                .corner_radius(CornerRadius::same(u8::MAX))
                .paint_at(ui, rect);
        }
        None => {
            ui.painter()
                .circle_filled(rect.center(), rect.width() / 2.0, color_for(name));
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                initial(name),
                theme::bold(rect.width() * 0.42),
                Color32::WHITE,
            );
        }
    }
}

/// A bolinha do status no canto do avatar, com o anel da cor do fundo como no Discord.
pub fn paint_status(ui: &Ui, avatar: Rect, status: UserStatus, online: bool, ground: Color32) {
    let radius = (avatar.width() * 0.16).max(4.0);
    let center = avatar.right_bottom() - vec2(radius * 0.8, radius * 0.8);

    ui.painter().circle_filled(center, radius + 2.5, ground);

    if !online || matches!(status, UserStatus::Offline | UserStatus::Invisible) {
        ui.painter().circle_stroke(center, radius - 1.0, Stroke::new(2.5, theme::DIM));
        return;
    }

    let color = match status {
        UserStatus::Online => theme::SUCCESS,
        UserStatus::Idle => theme::WARNING,
        UserStatus::Dnd => theme::DANGER,
        UserStatus::Invisible | UserStatus::Offline => theme::DIM,
    };
    ui.painter().circle_filled(center, radius, color);

    if status == UserStatus::Dnd {
        ui.painter().line_segment(
            [center - vec2(radius * 0.55, 0.0), center + vec2(radius * 0.55, 0.0)],
            Stroke::new(1.5, ground),
        );
    } else if status == UserStatus::Idle {
        ui.painter().circle_filled(center + vec2(radius * 0.35, -radius * 0.35), radius * 0.58, ground);
    }
}

/// Uma cor estável por nome, das oito do Unkvoid.
pub fn color_for(name: &str) -> Color32 {
    const COLORS: [Color32; 8] = [
        Color32::from_rgb(138, 124, 245),
        Color32::from_rgb(90, 63, 214),
        Color32::from_rgb(61, 168, 214),
        Color32::from_rgb(214, 90, 140),
        Color32::from_rgb(214, 140, 61),
        Color32::from_rgb(61, 190, 140),
        Color32::from_rgb(180, 90, 214),
        Color32::from_rgb(110, 120, 140),
    ];
    let sum = name.bytes().fold(0_usize, |sum, byte| {
        sum.wrapping_mul(31).wrapping_add(usize::from(byte))
    });

    COLORS[sum % COLORS.len()]
}

/// Um botão só de ícone, com o fundo no hover e a dica do que ele faz.
pub fn icon_button(ui: &mut Ui, icon: &str, tip: &str, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());

    if response.hovered() {
        ui.painter().rect_filled(rect, theme::RADIUS, theme::HOVER);
    }

    let tint = if response.hovered() && color == theme::MUTED {
        theme::TEXT
    } else {
        color
    };

    discord_icons::paint_glyph(ui, rect.shrink(6.0), icon, tint, 20.0);

    response.on_hover_text(tip)
}

/// O fundo e a sombra de todo menu suspenso.
pub fn menu_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::MENU)
        .stroke(Stroke::new(1.0, theme::MENU_BORDER))
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::same(8))
        .shadow(egui::epaint::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(140),
        })
}

/// A largura de um item de menu: a do menu, entre 200 e 260 px como no Discord. Sem o teto, o
/// popup do egui deixa o item ocupar a largura máxima dele, e o menu saía com 600 px.
pub fn menu_width(ui: &Ui) -> f32 {
    ui.available_width().clamp(200.0, 260.0)
}

/// O divisor entre os grupos de um menu. O `ui.separator()` do egui sai na cor dos divisores do
/// painel, que no fundo do menu some; o do Discord é a cor da borda do menu.
pub fn menu_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(menu_width(ui), 9.0), Sense::hover());

    ui.painter().hline(
        rect.x_range().shrink(8.0),
        rect.center().y,
        Stroke::new(1.0, theme::MENU_BORDER),
    );
}

/// O item que abre um submenu ("Mover para…"): a mesma linha dos outros, com a seta na direita,
/// e aceso enquanto o submenu está aberto.
pub fn menu_parent(ui: &mut Ui, label: &str) -> egui::Response {
    let width = menu_width(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click());
    let submenu = egui::containers::menu::SubMenu::id_from_widget_id(response.id);
    let lit = response.hovered()
        || egui::containers::menu::MenuState::from_ui(ui, |state, _| {
            state.open_item == Some(submenu)
        });

    if lit {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), theme::SELECTED);
    }

    let tint = if lit { theme::BRIGHT } else { theme::TEXT };

    ui.painter().text(
        pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::bold(14.0),
        tint,
    );
    discord_icons::paint_glyph(
        ui,
        Rect::from_center_size(pos2(rect.right() - 19.0, rect.center().y), vec2(16.0, 16.0)),
        egui_phosphor::regular::CARET_RIGHT,
        tint,
        16.0,
    );

    response
}

/// Uma linha de menu com ícone, como as do Discord de hoje: o hover é um cinza, não o roxo, e
/// `danger` ("Sair do servidor") fica vermelho com o fundo avermelhado.
pub fn menu_item(ui: &mut Ui, icon: &str, label: &str, danger: bool) -> bool {
    let width = menu_width(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click());

    if response.hovered() {
        let fill = if danger {
            theme::DANGER.gamma_multiply(0.16)
        } else {
            theme::SELECTED
        };

        ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    }

    let tint = match (danger, response.hovered()) {
        (true, _) => theme::DANGER,
        (false, true) => theme::BRIGHT,
        (false, false) => theme::TEXT,
    };

    ui.painter().text(
        pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::bold(14.0),
        tint,
    );

    // Sem ícone, como a maior parte dos itens do Discord.
    if !icon.is_empty() {
        discord_icons::paint_glyph(
            ui,
            Rect::from_center_size(pos2(rect.right() - 19.0, rect.center().y), vec2(18.0, 18.0)),
            icon,
            tint,
            18.0,
        );
    }

    response.clicked()
}

/// Item de menu com o ícone à esquerda, usado no menu do servidor para seguir a leitura do Discord.
pub fn menu_item_left(ui: &mut Ui, icon: &str, label: &str, danger: bool) -> bool {
    let width = menu_width(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(4),
            if danger {
                theme::DANGER.gamma_multiply(0.16)
            } else {
                theme::SELECTED
            },
        );
    }
    let tint = match (danger, response.hovered()) {
        (true, _) => theme::DANGER,
        (false, true) => theme::BRIGHT,
        (false, false) => theme::TEXT,
    };
    discord_icons::paint_glyph(
        ui,
        Rect::from_center_size(pos2(rect.left() + 19.0, rect.center().y), vec2(18.0, 18.0)),
        icon,
        tint,
        18.0,
    );
    ui.painter().text(
        pos2(rect.left() + 38.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::bold(14.0),
        tint,
    );
    response.clicked()
}

/// Um item de menu com a caixa de marcar do Discord na direita: cheia de roxo com o visto quando
/// ligada, só a borda quando desligada. `danger` é o vermelho dos itens de moderação.
pub fn menu_check(ui: &mut Ui, label: &str, checked: bool, danger: bool) -> bool {
    let width = menu_width(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click());
    let check =
        Rect::from_center_size(pos2(rect.right() - 19.0, rect.center().y), vec2(18.0, 18.0));

    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(4),
            if danger {
                theme::DANGER.gamma_multiply(0.16)
            } else {
                theme::SELECTED
            },
        );
    }

    let tint = match (danger, response.hovered()) {
        (true, _) => theme::DANGER,
        (false, true) => theme::BRIGHT,
        (false, false) => theme::TEXT,
    };

    ui.painter().text(
        pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::bold(14.0),
        tint,
    );

    if checked {
        ui.painter()
            .rect_filled(check, CornerRadius::same(4), theme::ACCENT);
        discord_icons::paint_glyph(ui, check, egui_phosphor::regular::CHECK, Color32::WHITE, 14.0);
    } else {
        ui.painter().rect_stroke(
            check,
            CornerRadius::same(4),
            Stroke::new(1.5, theme::MUTED),
            egui::StrokeKind::Inside,
        );
    }

    response.clicked()
}

/// O rótulo em caixa alta que o Discord põe em cima de cada campo.
pub fn field_label(ui: &mut Ui, text: &str, error: Option<&str>) {
    ui.horizontal(|ui| {
        let color = if error.is_some() {
            theme::DANGER
        } else {
            theme::MUTED
        };

        ui.label(
            egui::RichText::new(text.to_uppercase())
                .font(theme::bold(12.0))
                .color(color),
        );

        if let Some(error) = error {
            ui.label(
                egui::RichText::new(format!("— {error}"))
                    .font(theme::regular(12.0))
                    .italics()
                    .color(theme::DANGER),
            );
        }
    });
}

/// O campo de texto alto do Discord, sem moldura, sobre o fundo mais escuro.
pub fn text_field(ui: &mut Ui, value: &mut String, hint: &str, password: bool) -> Response {
    // A caixa do Discord: um tom acima do fundo e a borda fina. Na cor do fundo, sobre o cartão
    // do login (que tem a mesma cor), o campo sumia.
    egui::Frame::new()
        .fill(theme::FIELD)
        .stroke(egui::Stroke::new(1.0, theme::FIELD_BORDER))
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::symmetric(10, 10))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(value)
                    .hint_text(egui::RichText::new(hint).color(theme::DIM))
                    .password(password)
                    .frame(egui::Frame::NONE)
                    .font(theme::regular(15.0))
                    .desired_width(f32::INFINITY),
            )
        })
        .inner
}

/// O botão cheio da cor do Unkvoid. O texto vai no meio entre dois espaços que crescem: o botão
/// do egui alinha o conteúdo pelo layout, e no layout de cima para baixo ele ficava na esquerda.
pub fn primary_button(ui: &mut Ui, label: &str, enabled: bool, width: f32) -> bool {
    let button = egui::Button::new((
        egui::Atom::grow(),
        egui::RichText::new(label)
            .font(theme::bold(14.0))
            .color(Color32::WHITE),
        egui::Atom::grow(),
    ))
    .fill(if enabled {
        theme::ACCENT
    } else {
        theme::ACCENT.gamma_multiply(0.5)
    })
    .corner_radius(theme::RADIUS)
    .min_size(vec2(width, 40.0));

    ui.add_enabled(enabled, button).clicked()
}

/// O controle deslizante do Discord: trilha de 4 px, roxa até a bolinha branca de 16 px, e o
/// valor num balão em cima enquanto se arrasta. Grava só ao soltar: enquanto se arrasta, o valor
/// mora na memória do egui, e reler o guardado a cada quadro prenderia a bolinha no lugar.
/// Devolve o valor escolhido quando a pessoa solta ou clica na trilha.
pub fn held_slider(
    ui: &mut Ui,
    id: &str,
    saved: f32,
    range: std::ops::RangeInclusive<f32>,
    show_value: bool,
) -> Option<f32> {
    let id = ui.id().with(("held-slider", id));
    let held = ui.data(|data| data.get_temp::<f32>(id));
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click_and_drag());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let (low, high) = (*range.start(), *range.end());
    // A trilha vai de borda a borda, como a do Discord: nos extremos a bolinha passa da linha.
    let track = Rect::from_min_max(
        pos2(rect.left(), rect.center().y - 2.0),
        pos2(rect.right().max(rect.left() + 1.0), rect.center().y + 2.0),
    );
    let pointed = response.interact_pointer_pos().map(|pointer| {
        low + ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0) * (high - low)
    });
    let moving = response.dragged() || response.drag_stopped() || response.clicked();
    let value = if moving {
        pointed.or(held).unwrap_or(saved)
    } else {
        held.unwrap_or(saved)
    };
    let fraction = ((value - low) / (high - low)).clamp(0.0, 1.0);
    let knob = pos2(track.left() + fraction * track.width(), track.center().y);

    ui.painter()
        .rect_filled(track, CornerRadius::same(2), theme::TRACK);
    ui.painter().rect_filled(
        Rect::from_min_max(track.min, pos2(knob.x, track.max.y)),
        CornerRadius::same(2),
        theme::ACCENT,
    );
    ui.painter().circle_filled(knob, 8.0, Color32::WHITE);

    if show_value && response.dragged() {
        let bubble = Rect::from_center_size(knob - vec2(0.0, 26.0), vec2(48.0, 24.0));

        ui.painter()
            .rect_filled(bubble, CornerRadius::same(4), theme::RAIL);
        ui.painter().text(
            bubble.center(),
            Align2::CENTER_CENTER,
            format!("{}%", value.round()),
            theme::bold(13.0),
            theme::BRIGHT,
        );
    }

    if response.dragged() {
        ui.data_mut(|data| data.insert_temp(id, value));

        return None;
    }

    ui.data_mut(|data| data.remove::<f32>(id));

    (response.drag_stopped() || response.clicked()).then_some(value)
}

/// Uma opção da caixa de escolher: o nome, um complemento apagado ("(Microfone)") e se é a
/// escolhida.
pub struct Choice {
    pub name: String,
    pub detail: String,
    pub selected: bool,
}

/// A caixa de escolher do Discord: o escolhido com o ícone e a seta, e a lista embaixo, da mesma
/// largura, ao clicar. Devolve o índice escolhido.
pub fn select(ui: &mut Ui, id: &str, icon: &str, choices: &[Choice], empty: &str) -> Option<usize> {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let popup = ui.id().with(("select", id));
    let open = egui::Popup::is_id_open(ui.ctx(), popup);

    ui.painter().rect_filled(rect, theme::RADIUS, theme::FIELD);
    ui.painter().rect_stroke(
        rect,
        theme::RADIUS,
        Stroke::new(
            1.0,
            if open || response.hovered() {
                theme::TRACK
            } else {
                theme::FIELD_BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );

    let mut left = rect.left() + 12.0;

    if !icon.is_empty() {
        crate::ui::icons::text(ui.painter(), pos2(left, rect.center().y), Align2::LEFT_CENTER, icon, theme::icon(20.0), theme::TEXT);
        left += 28.0;
    }

    let current = choices.iter().find(|choice| choice.selected);
    let galley = ui.painter().layout_job(choice_text(
        current.map_or(empty, |choice| choice.name.as_str()),
        current.map_or("", |choice| choice.detail.as_str()),
        16.0,
        true,
        rect.right() - 40.0 - left,
    ));

    ui.painter().galley(
        pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        theme::TEXT,
    );
    discord_icons::paint_glyph(ui, Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(16.0, 16.0)), if open { egui_phosphor::regular::CARET_UP } else { egui_phosphor::regular::CARET_DOWN }, theme::TEXT, 16.0);

    let mut chosen = None;

    egui::Popup::menu(&response)
        .id(popup)
        .align(egui::RectAlign::BOTTOM_START)
        .gap(4.0)
        .width(rect.width())
        .frame(menu_frame())
        .show(|ui| {
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for (index, choice) in choices.iter().enumerate() {
                    let (row, clicked) = ui
                        .allocate_exact_size(vec2(rect.width() - 16.0, 36.0), Sense::click());

                    if clicked.hovered() || choice.selected {
                        ui.painter()
                            .rect_filled(row, CornerRadius::same(4), theme::SELECTED);
                    }

                    let galley = ui.painter().layout_job(choice_text(
                        &choice.name,
                        &choice.detail,
                        15.0,
                        choice.selected,
                        row.width() - 44.0,
                    ));

                    ui.painter().galley(
                        pos2(row.left() + 10.0, row.center().y - galley.size().y / 2.0),
                        galley,
                        theme::TEXT,
                    );

                    if choice.selected {
                        let mark = pos2(row.right() - 18.0, row.center().y);

                        ui.painter().circle_filled(mark, 9.0, theme::ACCENT);
                        crate::ui::icons::text(ui.painter(), mark, Align2::CENTER_CENTER, egui_phosphor::regular::CHECK, theme::icon(12.0), Color32::WHITE);
                    }

                    if clicked
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        chosen = Some(index);
                        ui.close();
                    }
                }
            });
        });

    chosen
}

fn choice_text(
    name: &str,
    detail: &str,
    size: f32,
    strong: bool,
    width: f32,
) -> egui::text::LayoutJob {
    let mut text = egui::text::LayoutJob::default();
    let name_font = if strong {
        theme::bold(size)
    } else {
        theme::regular(size)
    };

    text.append(
        name,
        0.0,
        egui::TextFormat::simple(name_font, if strong { theme::BRIGHT } else { theme::TEXT }),
    );
    text.append(
        detail,
        6.0,
        egui::TextFormat::simple(theme::regular(size), theme::MUTED),
    );
    text.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.0));

    text
}

/// O botão vermelho do que não tem volta: sair, expulsar, banir, excluir.
pub fn danger_button(ui: &mut Ui, label: &str, enabled: bool) -> bool {
    let button = egui::Button::new(
        egui::RichText::new(label)
            .font(theme::bold(14.0))
            .color(Color32::WHITE),
    )
    .fill(if enabled {
        theme::DANGER
    } else {
        theme::DANGER.gamma_multiply(0.5)
    })
    .corner_radius(theme::RADIUS)
    // A altura do `primary_button`: lado a lado nas perguntas, o mais baixo ficava torto.
    .min_size(vec2(0.0, 40.0));

    ui.add_enabled(enabled, button).clicked()
}

/// O controle de abas em pílula do Discord: as opções de mesma largura, a escolhida acesa.
/// Devolve a que foi clicada, se não era a escolhida.
pub fn pill_tabs(ui: &mut Ui, id: &str, options: &[(&str, &str)], selected: usize) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    let inner = rect.shrink(4.0);
    #[allow(clippy::cast_precision_loss)]
    let each = inner.width() / options.len() as f32;
    let mut chosen = None;

    ui.painter().rect_filled(rect, theme::RADIUS, theme::FIELD);

    for (position, (icon, name)) in options.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let option = Rect::from_min_size(
            pos2(inner.left() + each * position as f32, inner.top()),
            vec2(each, inner.height()),
        );
        let response = ui
            .interact(option, ui.id().with((id, position)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let on = position == selected;

        if on {
            ui.painter().rect_filled(option, CornerRadius::same(6), theme::SELECTED);
        } else if response.hovered() {
            ui.painter().rect_filled(option, CornerRadius::same(6), theme::HOVER);
        }

        let tint = if on || response.hovered() { theme::BRIGHT } else { theme::SECONDARY };
        let text = ui.painter().layout_no_wrap((*name).to_owned(), theme::bold(14.0), tint);
        let start = option.center().x - (text.size().x + 26.0) / 2.0;

        discord_icons::text(ui.painter(), pos2(start, option.center().y), Align2::LEFT_CENTER, icon, theme::icon(18.0), tint);
        ui.painter().galley(pos2(start + 26.0, option.center().y - text.size().y / 2.0), text, tint);

        if response.clicked() && !on {
            chosen = Some(position);
        }
    }

    chosen
}

/// O botão neutro, de fundo cinza.
pub fn secondary_button(ui: &mut Ui, label: &str) -> bool {
    let button = egui::Button::new(egui::RichText::new(label).font(theme::bold(14.0)))
        .fill(theme::SELECTED)
        .corner_radius(theme::RADIUS)
        // A altura do `primary_button`, ao lado dele no "Cancelar" das perguntas.
        .min_size(vec2(0.0, 40.0));

    ui.add(button).clicked()
}

/// O botão do Google, da largura do cartão.
pub fn secondary_button_wide(ui: &mut Ui, label: &str) -> bool {
    let text = format!("{}  {label}", egui_phosphor::regular::GOOGLE_LOGO);
    let button = egui::Button::new((egui::Atom::grow(), egui::RichText::new(text).font(theme::bold(14.0)), egui::Atom::grow()))
        .fill(theme::ELEVATED)
        .stroke(Stroke::new(1.0, theme::DIVIDER))
        .corner_radius(theme::RADIUS)
        .min_size(vec2(ui.available_width(), 40.0));

    ui.add(button).clicked()
}

/// A chave de liga e desliga do Discord: a pílula com a bolinha, roxa ligada e cinza desligada.
pub fn switch(ui: &mut Ui, on: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(44.0, 24.0), Sense::click());
    let moving = ui.ctx().animate_bool(response.id, on);
    let knob = egui::lerp((rect.left() + 12.0)..=(rect.right() - 12.0), moving);
    let fill = if on { theme::ACCENT } else { theme::DIM };

    ui.painter().rect_filled(rect, CornerRadius::same(12), fill);
    ui.painter()
        .circle_filled(pos2(knob, rect.center().y), 9.0, Color32::WHITE);

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A bolinha vermelha com o número de não lidas.
pub fn paint_badge(ui: &Ui, center: egui::Pos2, count: i64, ground: Color32) {
    let text = if count > 99 {
        "99+".to_owned()
    } else {
        count.to_string()
    };
    let width = 16.0_f32.max(8.0 + 7.0 * text.len() as f32);
    let rect = Rect::from_center_size(center, vec2(width, 16.0));

    ui.painter()
        .rect_filled(rect.expand(3.0), CornerRadius::same(u8::MAX), ground);
    ui.painter()
        .rect_filled(rect, CornerRadius::same(u8::MAX), theme::DANGER);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        theme::bold(11.0),
        Color32::WHITE,
    );
}

/// O `ui.columns` do egui numa área sem largura (o primeiro quadro de um menu que abre) passa
/// largura negativa às colunas, e o build de depuração cai. Aí não desenha nada: o quadro
/// seguinte já tem a largura.
pub fn columns<R>(ui: &mut Ui, count: usize, add: impl FnOnce(&mut [Ui]) -> R) -> Option<R> {
    #[allow(clippy::cast_precision_loss)]
    let gaps = ui.spacing().item_spacing.x * count.saturating_sub(1) as f32;

    (ui.available_width() > gaps).then(|| ui.columns(count, add))
}

/// A dica do Discord: em cima do que se aponta, centrada, e não embaixo do ponteiro como a do
/// egui. É a dos painéis de baixo e do palco, onde embaixo não há espaço.
pub trait TipAbove {
    fn tip(self, text: impl Into<egui::WidgetText>) -> Self;
}

impl TipAbove for Response {
    fn tip(self, text: impl Into<egui::WidgetText>) -> Self {
        let mut tooltip = egui::Tooltip::for_enabled(&self);
        let text = text.into();

        tooltip.popup = tooltip.popup.align(egui::RectAlign::TOP).gap(8.0);
        tooltip.show(|ui| ui.label(text));

        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_name_always_gets_the_same_color() {
        assert_eq!(color_for("Edsu"), color_for("Edsu"));
        assert_ne!(
            color_for("Edsu"),
            color_for("Ana"),
            "duas pessoas com a mesma cor seria azar, não regra"
        );
    }
}
