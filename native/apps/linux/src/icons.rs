//! Os desenhos da interface, nas mesmas formas do app em React.
//!
//! O React desenha cada ícone como SVG e deixa o `currentColor` pegar a cor do botão. Aqui
//! não há herança de cor: o desenho vira imagem antes de entrar na janela, então a cor entra
//! na hora de montar o SVG e cada estado (ligado, desligado, apagado) pede a sua.
//!
//! Renderizar o SVG no dobro do tamanho e mandar o `gtk::Image` mostrá-lo no tamanho pedido é
//! o que mantém a borda limpa em tela de muita densidade.

use gtk::gdk;
use gtk::gdk_pixbuf::PixbufLoader;
use gtk::prelude::*;

/// A cor de um ícone parado, a mesma do `--color-ink-icon` do React.
pub const RESTING: &str = "#cfc9de";
/// A cor de um ícone sobre fundo violeta, ou de um estado ligado.
pub const STRONG: &str = "#ffffff";
/// A cor de um ícone que avisa: microfone fechado, som cortado, sair da sala.
pub const DANGER: &str = "#e2445c";
/// A cor de um ícone secundário, ao lado de um texto apagado.
pub const DIM: &str = "#8a80a6";
/// O violeta claro do código da sala e das marcas da marca.
pub const LILAC: &str = "#aeb0ff";

/// O traço que corta o ícone quando o aparelho está desligado.
const SLASH: &str = r#"<path d="M4 4l16 16"/>"#;

/// O miolo de cada SVG, copiado forma a forma do `ui/components/common/Icon.tsx`. Divergir
/// aqui faz o mesmo botão ter dois desenhos, um por sistema.
fn shape(name: &str) -> &'static str {
    match name {
        "arrowLeft" => r#"<path d="M19 12H5M11 6l-6 6 6 6"/>"#,
        "camera" => r#"<rect x="3" y="6" width="13" height="12" rx="2.5"/><path d="M16 10.5l5-3v9l-5-3z"/>"#,
        "cameraOff" => {
            r#"<rect x="3" y="6" width="13" height="12" rx="2.5"/><path d="M16 10.5l5-3v9l-5-3z"/><path d="M4 4l16 16"/>"#
        }
        "chat" => r#"<path d="M4 5h16v11H9l-4 4v-4H4z"/>"#,
        "check" => r#"<path d="M5 12l5 5L19 7"/>"#,
        "chevronDown" => r#"<path d="M7 10l5 5 5-5"/>"#,
        "close" => r#"<path d="M6 6l12 12M18 6L6 18"/>"#,
        "copy" => {
            r#"<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"/>"#
        }
        "crown" => r#"<path d="M4 18h16M4 8l4 4 4-7 4 7 4-4-2 10H6z"/>"#,
        "dots" => {
            r#"<circle cx="6" cy="12" r="1.3" fill="currentColor"/><circle cx="12" cy="12" r="1.3" fill="currentColor"/><circle cx="18" cy="12" r="1.3" fill="currentColor"/>"#
        }
        "download" => r#"<path d="M12 4v11M7 10l5 5 5-5M5 20h14"/>"#,
        "edit" => r#"<path d="M16.5 3.5l4 4L8 20H4v-4z"/>"#,
        "eye" => {
            r#"<path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>"#
        }
        "focus" => {
            r#"<rect x="3" y="5" width="18" height="14" rx="2"/><rect x="9.5" y="9.5" width="5" height="5" fill="currentColor" stroke="none"/>"#
        }
        "fullscreen" => r#"<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>"#,
        "fullscreenExit" => r#"<path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5"/>"#,
        "gear" => {
            r#"<path d="M10.3 4.3a1 1 0 0 1 1-.8h1.4a1 1 0 0 1 1 .8l.3 1.6a7 7 0 0 1 1.7 1l1.5-.6a1 1 0 0 1 1.2.4l.7 1.2a1 1 0 0 1-.2 1.3l-1.2 1a7 7 0 0 1 0 2l1.2 1a1 1 0 0 1 .2 1.3l-.7 1.2a1 1 0 0 1-1.2.4l-1.5-.6a7 7 0 0 1-1.7 1l-.3 1.6a1 1 0 0 1-1 .8h-1.4a1 1 0 0 1-1-.8l-.3-1.6a7 7 0 0 1-1.7-1l-1.5.6a1 1 0 0 1-1.2-.4l-.7-1.2a1 1 0 0 1 .2-1.3l1.2-1a7 7 0 0 1 0-2l-1.2-1a1 1 0 0 1-.2-1.3l.7-1.2a1 1 0 0 1 1.2-.4l1.5.6a7 7 0 0 1 1.7-1z"/><circle cx="12" cy="12" r="2.5"/>"#
        }
        "grid" => {
            r#"<rect x="4" y="4" width="7" height="7" rx="1.2"/><rect x="13" y="4" width="7" height="7" rx="1.2"/><rect x="4" y="13" width="7" height="7" rx="1.2"/><rect x="13" y="13" width="7" height="7" rx="1.2"/>"#
        }
        "hash" => r#"<path d="M9 4L7 20M17 4l-2 16M4 9h16M3 15h16"/>"#,
        "headphones" => {
            r#"<path d="M4 15v-3a8 8 0 0 1 16 0v3"/><rect x="3" y="14" width="4" height="6" rx="1.5"/><rect x="17" y="14" width="4" height="6" rx="1.5"/>"#
        }
        "headphonesOff" => {
            r#"<path d="M4 15v-3a8 8 0 0 1 16 0v3"/><rect x="3" y="14" width="4" height="6" rx="1.5"/><rect x="17" y="14" width="4" height="6" rx="1.5"/><path d="M4 4l16 16"/>"#
        }
        "home" => r#"<path d="M4 11l8-7 8 7M6 10v10h12V10M10 20v-5h4v5"/>"#,
        "logout" => r#"<path d="M15 4h4v16h-4M10 8l-4 4 4 4M6 12h11"/>"#,
        "logs" => r#"<path d="M6 4h9l3 3v13H6zM9 10h6M9 14h6M9 18h4"/>"#,
        "menu" => r#"<path d="M5 7h14M5 12h14M5 17h14"/>"#,
        "mic" => {
            r#"<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3M8.5 21h7"/>"#
        }
        "micOff" => {
            r#"<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3M8.5 21h7"/><path d="M4 4l16 16"/>"#
        }
        "pause" => r#"<path d="M9 6v12M15 6v12"/>"#,
        "phoneOff" => {
            r#"<path d="M6.6 10.8c1.2 2.4 3.2 4.4 5.6 5.6l2-2c.3-.3.7-.4 1-.2 1.1.4 2.3.6 3.5.6.6 0 1 .4 1 1V19c0 .6-.4 1-1 1-8.3 0-15-6.7-15-15 0-.6.4-1 1-1h3.2c.6 0 1 .4 1 1 0 1.2.2 2.4.6 3.5.1.4 0 .8-.3 1l-1.6 1.3z" fill="currentColor" stroke="none" transform="rotate(135 12 12)"/>"#
        }
        "play" => r#"<path d="M8 5v14l11-7z" fill="currentColor"/>"#,
        "plus" => r#"<path d="M12 5v14M5 12h14"/>"#,
        "refresh" => r#"<path d="M20 11a8 8 0 1 0-2.3 5.7M20 5v6h-6"/>"#,
        "screen" => {
            r#"<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>"#
        }
        "signal" => r#"<path d="M5 19v-3M10 19v-7M15 19v-11M20 19V5"/>"#,
        "sliders" => {
            r#"<path d="M4 6h16M4 12h16M4 18h16"/><circle cx="9" cy="6" r="2" fill="currentColor"/><circle cx="15" cy="12" r="2" fill="currentColor"/><circle cx="8" cy="18" r="2" fill="currentColor"/>"#
        }
        "speaker" => {
            r#"<path d="M4 9h4l5-4v14l-5-4H4z"/><path d="M17 9a4 4 0 0 1 0 6"/>"#
        }
        "speakerOff" => {
            r#"<path d="M4 9h4l5-4v14l-5-4H4z"/><path d="M17 9l5 6M22 9l-5 6"/>"#
        }
        "stop" => r#"<rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" stroke="none"/>"#,
        "trash" => r#"<path d="M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13"/>"#,
        "users" => {
            r#"<circle cx="9" cy="8" r="3.5"/><path d="M3 20a6 6 0 0 1 12 0M16 4.5a3.5 3.5 0 0 1 0 7M21 20a6 6 0 0 0-4-5.6"/>"#
        }
        _ => SLASH,
    }
}

fn document(name: &str, color: &str, pixels: i32) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{pixels}" height="{pixels}" viewBox="0 0 24 24" fill="none" stroke="{color}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
        shape(name).replace("currentColor", color),
    )
}

/// O "+" ao lado do título de uma seção de canais: 20 de lado, como no React.
pub fn small_plus(tooltip: &str) -> gtk::Button {
    let button = crate::components::icon_button("plus", 12, RESTING, tooltip);

    button.add_css_class("tiny");

    button
}

/// A marca do Google, nas quatro cores dela. Fica fora da tabela porque é a única coisa aqui
/// que não pode ser recolorida: a cor é da marca, não do desenho.
const GOOGLE: &str = r##"<path fill="#FFC107" d="M43.6 20.5H42V20H24v8h11.3C33.7 32.7 29.2 36 24 36c-6.6 0-12-5.4-12-12s5.4-12 12-12c3 0 5.8 1.1 7.9 3l5.7-5.7C34 6.1 29.3 4 24 4 12.9 4 4 12.9 4 24s8.9 20 20 20 20-8.9 20-20c0-1.3-.1-2.4-.4-3.5z"/><path fill="#FF3D00" d="M6.3 14.7l6.6 4.8C14.7 15.1 19 12 24 12c3 0 5.8 1.1 7.9 3l5.7-5.7C34 6.1 29.3 4 24 4 16.3 4 9.7 8.3 6.3 14.7z"/><path fill="#4CAF50" d="M24 44c5.2 0 9.9-2 13.4-5.2l-6.2-5.2C29.2 35.1 26.7 36 24 36c-5.2 0-9.6-3.3-11.3-8l-6.5 5C9.5 39.6 16.2 44 24 44z"/><path fill="#1976D2" d="M43.6 20.5H42V20H24v8h11.3c-.8 2.2-2.2 4.2-4.1 5.6l6.2 5.2C37 38.2 44 33 44 24c0-1.3-.1-2.4-.4-3.5z"/>"##;

/// O desenho já no tamanho pedido. Ícone que não existe na tabela vira o traço do "desligado",
/// que aparece na tela e denuncia o nome errado — melhor que um buraco silencioso.
pub fn icon(name: &str, size: i32, color: &str) -> gtk::Image {
    draw(&document(name, color, size * 2), size)
}

pub fn google_mark(size: i32) -> gtk::Image {
    let pixels = size * 2;

    draw(
        &format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{pixels}" height="{pixels}" viewBox="0 0 48 48">{GOOGLE}</svg>"#),
        size,
    )
}

fn draw(svg: &str, size: i32) -> gtk::Image {
    let image = gtk::Image::new();

    image.set_pixel_size(size);

    let loader = PixbufLoader::new();

    if loader.write(svg.as_bytes()).is_err() || loader.close().is_err() {
        tracing::warn!("o ícone não foi desenhado");

        return image;
    }

    if let Some(pixbuf) = loader.pixbuf() {
        image.set_paintable(Some(&gdk::Texture::for_pixbuf(&pixbuf)));
    }

    image
}
