//! Fotografa as telas do app do Windows sem abrir janela: o renderizador por software do
//! Slint desenha num buffer, e o buffer vira um BMP. Serve para comparar o desenho com o
//! React e o Mac com alguém jogando na frente do computador — nada aparece na tela dele.
//!
//! `cargo run -p unkvoid-windows --example vitrine -- <pasta>`

use std::rc::Rc;
use std::time::Duration;

use anyhow::anyhow;
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, Image, ModelRc, Rgb8Pixel, SharedPixelBuffer, SharedString, VecModel};

slint::include_modules!();

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 800;

struct Offscreen(Rc<MinimalSoftwareWindow>);

impl Platform for Offscreen {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn main() -> anyhow::Result<()> {
    let folder = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);

    window.set_size(slint::PhysicalSize::new(WIDTH, HEIGHT));
    slint::platform::set_platform(Box::new(Offscreen(window.clone()))).map_err(|failure| anyhow!("{failure}"))?;

    let app = AppWindow::new()?;

    app.show()?;

    let ui = app.global::<Ui>();
    let shoot = |name: &str| shoot(&window, &format!("{folder}/{name}.bmp"));

    ui.set_screen("entry".into());
    ui.set_saved_name("Ada".into());
    shoot("entrada")?;

    ui.set_screen("room".into());
    ui.set_room_code("np9cabl01opi".into());
    ui.set_peers(model(vec![peer("Ada Windows", true, false), peer("Bia Linux", false, false)]));
    ui.set_elapsed("0:04:13".into());
    ui.set_ping("12 ms".into());
    ui.set_ping_ms(12);
    shoot("sala-vazia")?;

    ui.set_tiles(model(vec![
        tile("Bia Linux", 0, 0, 1_400, 900, 3),
        tile("Caio Mac", 1, 0, 1_920, 1_080, 0),
    ]));
    ui.set_grid_columns(2);
    ui.set_grid_lines(1);
    shoot("sala-duas-telas")?;

    ui.set_screen("hub".into());
    ui.set_in_server(true);
    ui.set_server_name("Os de sempre".into());
    ui.set_user_name("Ada".into());
    ui.set_user_initial("A".into());
    ui.set_signed_in(true);
    ui.set_text_channels(model(vec![channel(0, "geral", false), channel(1, "clipes", false)]));
    ui.set_voice_channels(model(vec![channel(2, "Voz", true)]));
    ui.set_voice_channel("2".into());
    ui.set_voice_name("Voz".into());
    ui.set_mic_on(true);
    ui.set_speaking(true);
    ui.set_peers(model(vec![peer("Ada", true, true), peer("Bia", false, true), muted(peer("Caio", false, false))]));
    shoot("voz-falando")?;

    ui.set_stage_open(true);
    ui.set_tiles(model(vec![tile("Bia", 0, 0, 1_920, 1_080, 2)]));
    ui.set_grid_columns(1);
    ui.set_grid_lines(1);
    shoot("voz-palco")?;

    ui.set_focused_room(true);
    ui.set_voice_chat_open(true);
    shoot("sala-focada")?;

    ui.set_focused_room(false);
    ui.set_voice_chat_open(false);
    ui.set_stage_open(false);
    ui.set_tiles(model(Vec::new()));
    ui.set_voice_channel(SharedString::new());
    ui.set_in_server(false);
    shoot("home")?;

    Ok(())
}

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows))
}

fn peer(name: &str, mine: bool, speaking: bool) -> PeerRow {
    PeerRow {
        name: name.into(),
        initial: name.chars().next().unwrap_or('?').to_string().into(),
        note: SharedString::new(),
        mine,
        speaking,
        muted: false,
    }
}

fn muted(mut row: PeerRow) -> PeerRow {
    row.muted = true;

    row
}

fn channel(index: i32, name: &str, voice: bool) -> ChannelRow {
    ChannelRow {
        index,
        id: index.to_string().into(),
        name: name.into(),
        voice,
        current: index == 0,
    }
}

/// Um cartão com uma imagem de mentira: um degradê no tamanho que a tela teria.
fn tile(label: &str, column: i32, line: i32, width: u32, height: u32, watchers: i32) -> TileRow {
    let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(width, height);

    for (index, pixel) in buffer.make_mut_slice().iter_mut().enumerate() {
        let (x, y) = (index as u32 % width, index as u32 / width);

        *pixel = Rgb8Pixel::new((x * 255 / width) as u8, (y * 180 / height) as u8, 140);
    }

    TileRow {
        producer: label.into(),
        label: label.into(),
        initial: label.chars().next().unwrap_or('?').to_string().into(),
        mine: false,
        camera: false,
        paused: false,
        audio: true,
        heard: false,
        watchers,
        watcher_names: SharedString::new(),
        frame: Image::from_rgb8(buffer),
        has_frame: true,
        column,
        line,
        rank: column,
        focused: false,
        full: false,
    }
}

/// Deixa as animações de entrada terminarem e desenha a janela inteira num BMP.
fn shoot(window: &MinimalSoftwareWindow, path: &str) -> anyhow::Result<()> {
    std::thread::sleep(Duration::from_millis(400));
    slint::platform::update_timers_and_animations();
    window.request_redraw();

    let mut pixels = vec![PremultipliedRgbaColor::default(); (WIDTH * HEIGHT) as usize];

    window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, WIDTH as usize);
    });

    let row = WIDTH as usize * 4;
    let mut bmp = Vec::with_capacity(54 + row * HEIGHT as usize);

    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(54 + row as u32 * HEIGHT).to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&54_u32.to_le_bytes());
    bmp.extend_from_slice(&40_u32.to_le_bytes());
    bmp.extend_from_slice(&WIDTH.to_le_bytes());
    bmp.extend_from_slice(&HEIGHT.to_le_bytes());
    bmp.extend_from_slice(&1_u16.to_le_bytes());
    bmp.extend_from_slice(&32_u16.to_le_bytes());
    bmp.extend_from_slice(&[0; 24]);

    for line in (0..HEIGHT as usize).rev() {
        for pixel in &pixels[line * WIDTH as usize..(line + 1) * WIDTH as usize] {
            bmp.extend_from_slice(&[pixel.blue, pixel.green, pixel.red, 255]);
        }
    }

    std::fs::write(path, bmp)?;
    println!("{path}");

    Ok(())
}
