//! As cores, as fontes e os tamanhos da janela, medidos no Discord a 100% de escala: o mesmo
//! cinza neutro, os mesmos tamanhos de linha e de ícone. Só o destaque é o roxo do Unkvoid.

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke};

/// O trilho, a barra de título e a coluna de canais são da mesma cor; a coluna se separa pela
/// borda e pelo canto arredondado.
pub const RAIL: Color32 = Color32::from_rgb(18, 18, 20);
pub const SIDEBAR: Color32 = Color32::from_rgb(18, 18, 20);
/// O chat, a barra do canal e a lista de membros.
pub const MAIN: Color32 = Color32::from_rgb(26, 26, 30);
/// O cartão da conta, os menus e as janelas por cima.
pub const ELEVATED: Color32 = Color32::from_rgb(32, 32, 36);
pub const COMPOSER: Color32 = Color32::from_rgb(34, 35, 39);
pub const COMPOSER_BORDER: Color32 = Color32::from_rgb(39, 40, 43);
/// O texto de exemplo da caixa de mensagem ("Conversar em #geral").
pub const PLACEHOLDER: Color32 = Color32::from_rgb(109, 111, 120);
pub const HOVER: Color32 = Color32::from_rgb(34, 34, 37);
pub const SELECTED: Color32 = Color32::from_rgb(44, 44, 48);
/// A linha embaixo da barra do canal e a que separa a lista de membros do chat.
pub const BORDER: Color32 = Color32::from_rgb(41, 41, 45);
/// O fundo da busca da barra do canal.
pub const SEARCH: Color32 = Color32::from_rgb(23, 23, 26);
pub const DIVIDER: Color32 = Color32::from_rgb(37, 37, 41);
pub const MENU: Color32 = Color32::from_rgb(36, 36, 41);
pub const MENU_BORDER: Color32 = Color32::from_rgb(46, 46, 51);
/// Os botões cheios do cartão da voz (câmera, tela) e o hover deles.
pub const BUTTON: Color32 = Color32::from_rgb(43, 43, 48);
pub const BUTTON_HOVER: Color32 = Color32::from_rgb(51, 51, 56);
/// Os campos das configurações do usuário (a caixa de escolher aparelho) e a borda deles.
pub const FIELD: Color32 = Color32::from_rgb(28, 28, 32);
pub const FIELD_BORDER: Color32 = Color32::from_rgb(52, 52, 57);
/// A trilha vazia do controle deslizante e os traços apagados do medidor.
pub const TRACK: Color32 = Color32::from_rgb(71, 72, 81);
/// O texto que explica uma opção nas configurações.
pub const DESCRIPTION: Color32 = Color32::from_rgb(151, 152, 159);
/// A régua da sensibilidade: o que não abre o microfone e o que abre.
pub const SENSITIVITY_LOW: Color32 = Color32::from_rgb(253, 184, 51);
pub const SENSITIVITY_HIGH: Color32 = Color32::from_rgb(61, 158, 96);
pub const CALL_CONTROL: Color32 = Color32::from_rgb(10, 10, 12);
pub const CALL_CONTROL_BORDER: Color32 = Color32::from_rgb(38, 38, 43);
/// O microfone e o fone desligados: o fundo avermelhado e o do hover.
pub const OFF: Color32 = Color32::from_rgb(54, 33, 39);
pub const OFF_HOVER: Color32 = Color32::from_rgb(68, 32, 38);

/// Nomes, títulos e o que está aceso.
pub const BRIGHT: Color32 = Color32::from_rgb(251, 251, 251);
/// O texto das mensagens.
pub const TEXT: Color32 = Color32::from_rgb(220, 220, 223);
/// O subtítulo: o status, o canal da voz.
pub const SECONDARY: Color32 = Color32::from_rgb(171, 172, 178);
/// Os canais apagados, as categorias e os ícones em repouso.
pub const MUTED: Color32 = Color32::from_rgb(129, 130, 138);
pub const DIM: Color32 = Color32::from_rgb(98, 99, 106);

pub const ACCENT: Color32 = Color32::from_rgb(138, 124, 245);
pub const ACCENT_TEXT: Color32 = Color32::from_rgb(174, 176, 255);
pub const DANGER: Color32 = Color32::from_rgb(218, 62, 68);
pub const ORANGE: Color32 = Color32::from_rgb(230, 126, 34);
pub const HANG_UP: Color32 = Color32::from_rgb(210, 45, 57);
pub const SUCCESS: Color32 = Color32::from_rgb(94, 180, 121);
pub const WARNING: Color32 = Color32::from_rgb(240, 178, 50);

pub const TITLEBAR_HEIGHT: f32 = 32.0;
pub const RAIL_WIDTH: f32 = 72.0;
/// A coluna de canais se redimensiona arrastando a borda, como no Discord.
pub const SIDEBAR_WIDTH: f32 = 300.0;
pub const SIDEBAR_RANGE: std::ops::RangeInclusive<f32> = 240.0..=440.0;
pub const MEMBERS_WIDTH: f32 = 264.0;
pub const HEADER_HEIGHT: f32 = 48.0;
pub const USER_PANEL_HEIGHT: f32 = 56.0;

/// Arredondado, nunca pílula: os 8 px do Discord em botão, campo, linha e cartão.
pub const RADIUS: CornerRadius = CornerRadius::same(8);

/// Os tamanhos do código são os do Discord (16 no nome do canal, 14 no subtítulo). A Archivo é
/// mais larga que a gg sans dele: no mesmo número ela saía uns 6% maior. Este é o ajuste fino,
/// medido lado a lado; os ícones não passam por ele.
const TEXT_SCALE: f32 = 0.94;

pub fn regular(size: f32) -> FontId {
    FontId::new(size * TEXT_SCALE, FontFamily::Proportional)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size * TEXT_SCALE, FontFamily::Name("bold".into()))
}

pub fn icon(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn filled(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("fill".into()))
}

/// A cor de um cargo, que o Laravel manda como `#rrggbb`.
pub fn color_of(hex: &str) -> Option<Color32> {
    let digits = u32::from_str_radix(hex.strip_prefix('#')?, 16).ok()?;
    let [_, red, green, blue] = digits.to_be_bytes();

    (hex.len() == 7).then_some(Color32::from_rgb(red, green, blue))
}

/// A Archivo, a fonte do site, com os ícones do Phosphor e os emojis do egui atrás dela.
pub fn install(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let mut normal = egui::FontData::from_static(include_bytes!("../../assets/fonts/Archivo.ttf"));

    // O peso padrão deste arquivo é 600: sem dizer o 400, o texto normal saía tão pesado quanto
    // o negrito, e tudo parecia maior que no Discord.
    normal.tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", 400.0)]);
    fonts
        .font_data
        .insert("archivo".into(), std::sync::Arc::new(normal));

    let mut heavy = egui::FontData::from_static(include_bytes!("../../assets/fonts/Archivo.ttf"));

    // A Archivo é variável: o negrito é a mesma fonte com outro peso, sem um segundo arquivo.
    heavy.tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", 600.0)]);
    fonts
        .font_data
        .insert("archivo-bold".into(), std::sync::Arc::new(heavy));

    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    egui_phosphor::add_font_bytes_as_family(
        &mut fonts,
        "fill",
        egui_phosphor::Variant::Fill.font_bytes(),
    );

    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();

    proportional.insert(0, "archivo".into());

    let mut bold_family = vec!["archivo-bold".to_owned()];

    bold_family.extend(
        fonts.families[&FontFamily::Proportional]
            .iter()
            .skip(1)
            .cloned(),
    );
    fonts
        .families
        .insert(FontFamily::Name("bold".into()), bold_family);
    context.set_fonts(fonts);

    let mut style = (*context.global_style()).clone();

    {
        let style = &mut style;

        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = MAIN;
        style.visuals.extreme_bg_color = RAIL;
        style.visuals.faint_bg_color = SIDEBAR;
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.hyperlink_color = ACCENT_TEXT;
        style.visuals.selection.bg_fill = ACCENT.gamma_multiply(0.45);
        style.visuals.selection.stroke = Stroke::new(1.0, ACCENT_TEXT);
        style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, DIVIDER);
        style.visuals.widgets.inactive.bg_fill = ELEVATED;
        style.visuals.widgets.inactive.weak_bg_fill = ELEVATED;
        style.visuals.widgets.inactive.corner_radius = RADIUS;
        style.visuals.widgets.noninteractive.corner_radius = RADIUS;
        style.visuals.widgets.hovered.bg_fill = HOVER;
        style.visuals.widgets.hovered.weak_bg_fill = HOVER;
        style.visuals.widgets.hovered.corner_radius = RADIUS;
        style.visuals.widgets.active.bg_fill = SELECTED;
        style.visuals.widgets.active.weak_bg_fill = SELECTED;
        style.visuals.widgets.active.corner_radius = RADIUS;
        style.visuals.text_edit_bg_color = Some(RAIL);
        // As dicas e os menus do egui no molde dos do Discord: escuros, com borda, arredondados.
        style.visuals.window_fill = MENU;
        style.visuals.window_stroke = Stroke::new(1.0, MENU_BORDER);
        style.visuals.window_corner_radius = RADIUS;
        style.visuals.menu_corner_radius = RADIUS;
        style.spacing.menu_margin = egui::Margin::same(8);
        style.visuals.popup_shadow = egui::epaint::Shadow {
            offset: [0, 4],
            blur: 16,
            spread: 0,
            color: Color32::from_black_alpha(120),
        };
        style.interaction.tooltip_delay = 0.1;
        style.spacing.item_spacing = egui::vec2(8.0, 4.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.interaction.selectable_labels = false;
        // A mãozinha em tudo que se clica, como no Discord: botão, canal, servidor, item de menu.
        style.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
        style.url_in_tooltip = false;
        style
            .text_styles
            .insert(egui::TextStyle::Body, regular(15.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, regular(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, regular(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, bold(20.0));
    }

    context.set_global_style(style);
    egui_extras::install_image_loaders(context);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_role_color_reads_only_the_full_hex() {
        assert_eq!(color_of("#8a7cf5"), Some(Color32::from_rgb(138, 124, 245)));
        assert_eq!(color_of("#fff"), None);
        assert_eq!(color_of("8a7cf5"), None);
    }
}
