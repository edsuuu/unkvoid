//! Os ícones da interface num lugar só. No beta, os SVGs do Discord, para comparar lado a lado; no
//! estável, os glifos do Phosphor de sempre.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Ui, Vec2};
use egui_phosphor::fill as filled;
use egui_phosphor::regular as phosphor;

use super::theme;

// ponytail: os SVGs do Discord entram somente no beta para comparação visual; antes de publicar
// qualquer build com eles, substitua-os por ícones próprios. A versão estável fica em Phosphor.
macro_rules! icons {
    ($($variant:ident => $file:literal [$first:ident $(, $glyph:ident)*]),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Icon {
            $($variant),+
        }

        impl Icon {
            #[cfg(feature = "beta")]
            fn file(self) -> &'static str {
                match self {
                    $(Self::$variant => $file),+
                }
            }

            #[cfg(feature = "beta")]
            fn bytes(self) -> &'static [u8] {
                match self {
                    $(Self::$variant => include_bytes!(concat!("../../assets/discord-icons/", $file, ".svg"))),+
                }
            }

            #[cfg(not(feature = "beta"))]
            fn phosphor(self) -> &'static str {
                match self {
                    $(Self::$variant => phosphor::$first),+
                }
            }
        }

        /// O ícone do Discord de um glifo do Phosphor, regular ou cheio. O primeiro da tabela vence.
        #[cfg_attr(not(feature = "beta"), allow(dead_code))]
        fn from_phosphor(glyph: &str) -> Option<Icon> {
            $(
                if [phosphor::$first, filled::$first $(, phosphor::$glyph, filled::$glyph)*].contains(&glyph) {
                    return Some(Icon::$variant);
                }
            )+

            None
        }
    };
}

icons! {
    Microphone => "Microphone" [MICROPHONE],
    MicrophoneSlash => "MicrophoneSlash" [MICROPHONE_SLASH],
    Headphones => "Headphones" [HEADPHONES],
    HeadphonesSlash => "HeadphonesSlash" [HEADPHONES],
    Bell => "Bell" [BELL],
    Thread => "Thread" [CHATS_TEARDROP],
    Chat => "Chat" [CHAT_CIRCLE, CHAT_CIRCLE_TEXT],
    ChatDots => "ChatDots" [CHAT_CIRCLE_DOTS],
    Pin => "Pin" [PUSH_PIN],
    UserPlus => "UserPlus" [USER_PLUS],
    VoiceNormal => "VoiceNormal" [SPEAKER_HIGH],
    VoiceX => "VoiceX" [SPEAKER_SLASH],
    ScreenArrow => "ScreenArrow" [MONITOR_ARROW_UP],
    ScreenStream => "ScreenStream" [MONITOR_PLAY],
    Screen => "Screen" [MONITOR],
    Gift => "Gift" [GIFT],
    Gif => "Gif" [GIF],
    Sticker => "Sticker" [STICKER],
    Reaction => "Reaction" [SMILEY],
    Apps => "Apps" [SQUARES_FOUR],
    ChevronSmallDown => "ChevronSmallDown" [CARET_DOWN],
    ChevronUp => "ChevronSmallUp" [CARET_UP],
    ChevronRight => "ChevronSmallRight" [CARET_RIGHT],
    ArrowUp => "ArrowSmallUp" [ARROW_UP],
    ArrowDown => "ArrowSmallDown" [ARROW_DOWN],
    ArrowRight => "ArrowSmallRight" [ARROW_RIGHT],
    ArrowLeft => "ArrowLargeLeft" [ARROW_LEFT],
    Video => "Video" [VIDEO_CAMERA],
    VideoSlash => "VideoSlash" [VIDEO_CAMERA_SLASH],
    VideoDeny => "VideoDeny" [VIDEO_CAMERA_SLASH],
    PhoneHangUp => "PhoneHangUp" [PHONE_DISCONNECT],
    PlusSmall => "PlusSmall" [PLUS],
    CirclePlus => "CirclePlus" [PLUS_CIRCLE],
    Minus => "Minus" [MINUS],
    Pencil => "Pencil" [PENCIL_SIMPLE],
    Trash => "Trash" [TRASH],
    Link => "Link" [LINK],
    Copy => "Copy" [COPY],
    User => "User" [USER],
    Group => "Group" [USERS, USERS_THREE, USERS_FOUR],
    Crown => "Crown" [CROWN_SIMPLE],
    Id => "Id" [IDENTIFICATION_CARD],
    Waveform => "Waveform" [WAVEFORM],
    Soundboard => "Soundboard" [MEGAPHONE],
    Music => "Music" [MUSIC_NOTES],
    Clock => "Clock" [CLOCK],
    Upload => "Upload" [UPLOAD_SIMPLE],
    Download => "Download" [DOWNLOAD_SIMPLE],
    Bug => "Bug" [BUG],
    MagnifyingGlass => "MagnifyingGlass" [MAGNIFYING_GLASS],
    AuditLog => "ListView" [LIST_MAGNIFYING_GLASS],
    Settings => "Settings" [GEAR_SIX],
    Folder => "Folder" [FOLDER],
    FolderPlus => "FolderPlus" [FOLDER_PLUS],
    MoreHorizontal => "MoreHorizontal" [DOTS_THREE],
    MoreVertical => "MoreVertical" [DOTS_THREE_VERTICAL],
    Check => "CheckmarkSmall" [CHECK],
    Close => "Close" [X],
    CircleX => "CircleX" [X_CIRCLE],
    Hashtag => "Text" [HASH],
    FullscreenEnter => "FullscreenEnter" [ARROWS_OUT],
    FullscreenExit => "FullscreenExit" [ARROWS_IN, CORNERS_IN],
    Image => "Image" [IMAGE],
    Lock => "Lock" [LOCK_SIMPLE],
    Shield => "Shield" [SHIELD],
    Roles => "ShieldUser" [SHIELD_CHEVRON],
    Deny => "Deny" [PROHIBIT],
    Leave => "DoorExit" [SIGN_OUT],
    Information => "CircleInformation" [INFO],
    Play => "Play" [PLAY],
    Send => "SendMessage" [PAPER_PLANE_TILT],
    ConnectionFine => "ConnectionFine" [CELL_SIGNAL_FULL, CELL_SIGNAL_HIGH],
    ConnectionAverage => "ConnectionAverage" [CELL_SIGNAL_MEDIUM],
    ConnectionBad => "ConnectionBad" [CELL_SIGNAL_LOW],
}

/// O `painter.text(pos, align, glifo, fonte, cor)` de cada tela: no beta o glifo com equivalente
/// do Discord sai como o SVG dele, no mesmo lugar e no tamanho da fonte.
pub fn text(painter: &Painter, pos: Pos2, align: Align2, glyph: &str, font: FontId, color: Color32) -> Rect {
    #[cfg(feature = "beta")]
    if let Some(icon) = from_phosphor(glyph) {
        let rect = align.anchor_size(pos, Vec2::splat(font.size));

        paint_with(painter, rect, icon, color);

        return rect;
    }

    painter.text(pos, align, glyph, font, color)
}

pub fn paint_glyph(ui: &Ui, rect: Rect, glyph: &str, color: Color32, size: f32) {
    text(ui.painter(), rect.center(), Align2::CENTER_CENTER, glyph, theme::icon(size), color);
}

pub fn paint(ui: &Ui, rect: Rect, icon: Icon, color: Color32) {
    paint_with(ui.painter(), rect, icon, color);
}

fn paint_with(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    // O `rect` às vezes é o do botão: o ícone fica no quadrado do meio, sem esticar.
    let rect = Rect::from_center_size(rect.center(), Vec2::splat(rect.width().min(rect.height())));

    #[cfg(feature = "beta")]
    {
        let context = painter.ctx();
        let uri = format!("bytes://discord-icons/{}.svg", icon.file());
        // O SVG é rasterizado no tamanho em pixels da tela, para não sair borrado no zoom.
        let pixels = (rect.width() * context.pixels_per_point()).round().max(1.0) as u32;

        context.include_bytes(uri.clone(), icon.bytes());

        if let Ok(egui::load::TexturePoll::Ready { texture }) = context.try_load_texture(
            &uri,
            egui::TextureOptions::LINEAR,
            egui::SizeHint::Size { width: pixels, height: pixels, maintain_aspect_ratio: true },
        ) {
            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

            painter.image(texture.id, rect, uv, color);
        }
    }

    #[cfg(not(feature = "beta"))]
    painter.text(rect.center(), Align2::CENTER_CENTER, icon.phosphor(), theme::icon(rect.width()), color);
}

/// O `ui.label(RichText::new(glifo).font(fonte).color(cor))` de um ícone solto no layout.
pub fn label(ui: &mut Ui, glyph: &str, font: FontId, color: Color32) -> egui::Response {
    #[cfg(feature = "beta")]
    if from_phosphor(glyph).is_some() {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(font.size), egui::Sense::hover());

        text(ui.painter(), rect.center(), Align2::CENTER_CENTER, glyph, font, color);

        return response;
    }

    ui.label(egui::RichText::new(glyph).font(font).color(color))
}

/// O ícone de um `Button` ou `menu_button`: no beta o SVG do Discord, fora dele o glifo.
pub fn atom(glyph: &str, font: FontId, color: Color32) -> egui::Atom<'static> {
    #[cfg(feature = "beta")]
    if let Some(icon) = from_phosphor(glyph) {
        return egui::Image::from_bytes(format!("bytes://discord-icons/{}.svg", icon.file()), icon.bytes())
            .fit_to_exact_size(Vec2::splat(font.size))
            .tint(color)
            .into();
    }

    egui::RichText::new(glyph.to_owned()).font(font).color(color).into()
}

/// O fone cortado de quem está surdo. O Phosphor não tem o desenho: no estável, o fone com o traço.
pub fn headphones_slash(painter: &Painter, rect: Rect, color: Color32) {
    #[cfg(feature = "beta")]
    paint_with(painter, rect, Icon::HeadphonesSlash, color);

    #[cfg(not(feature = "beta"))]
    {
        let cut = Rect::from_center_size(rect.center(), rect.size() * 0.9);

        paint_with(painter, rect, Icon::Headphones, color);
        painter.line_segment([cut.left_top(), cut.right_bottom()], egui::Stroke::new(2.0, color));
    }
}
