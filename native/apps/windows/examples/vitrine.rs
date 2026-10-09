//! Fotografa as telas do app do Windows sem abrir janela: o renderizador por software do
//! Slint desenha num buffer, e o buffer vira um BMP. Serve para comparar o desenho com o
//! React e o Mac com alguém jogando na frente do computador — nada aparece na tela dele.
//!
//! `cargo run -p unkvoid-windows --example vitrine -- <pasta> [largura altura]`
//!
//! As capturas da página da Microsoft Store saem daqui, em 1920 1080: a Store pede no mínimo
//! 1366×768, e com dados inventados nenhum nome de quem usa vai parar numa página pública.

use std::rc::Rc;
use std::time::Duration;

use anyhow::anyhow;
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, Image, ModelRc, Rgb8Pixel, SharedPixelBuffer, SharedString, VecModel};

slint::include_modules!();

/// O tamanho de quando não se diz outro: o da comparação com o React.
const SIZE: (u32, u32) = (1280, 800);

struct Offscreen(Rc<MinimalSoftwareWindow>);

impl Platform for Offscreen {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn main() -> anyhow::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let folder = arguments.first().cloned().unwrap_or_else(|| ".".into());
    let (width, height) = match (arguments.get(1).and_then(|text| text.parse().ok()), arguments.get(2).and_then(|text| text.parse().ok())) {
        (Some(width), Some(height)) => (width, height),
        _ => SIZE,
    };
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);

    window.set_size(slint::PhysicalSize::new(width, height));
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

    // Os Clips: a aba com a galeria por jogo, e a seção nas Configurações, com conta e sem.
    let clips = app.global::<ClipsUi>();

    clips.set_available(true);
    clips.set_open(true);
    clips.set_recording(true);
    clips.set_replay_enabled(true);
    clips.set_status("Gravando · replay de 5 min".into());
    clips.set_save_label("Salvar últimos 5 min".into());
    clips.set_games(model(vec!["Todos · 3".into(), "Corrida Neon · 2".into(), "Desktop · 1".into()]));
    clips.set_clips(model(vec![
        clip("Corrida Neon", "26/09/2026 21:40", "5:00 · 1312 MB", 180, 60),
        clip("Corrida Neon", "26/09/2026 21:12", "5:00 · 1298 MB", 60, 180),
        clip("Desktop", "26/09/2026 18:03", "1:12 · 301 MB", 120, 120),
    ]));
    shoot("clips")?;

    clips.set_open(false);
    clips.set_monitors(model(vec![
        MonitorItem { label: "Monitor 1".into(), detail: "AW2525HM · 1920×1080 · 240 Hz".into(), primary: true, preview: thumbnail(180, 60) },
        MonitorItem { label: "Monitor 2".into(), detail: "25G3ZM · 1920×1080 · 240 Hz".into(), primary: false, preview: thumbnail(60, 180) },
    ]));
    clips.set_microphones(model(vec!["Padrão do Windows".into(), "Microfone (NVIDIA Broadcast)".into()]));
    clips.set_quality_index(1);
    clips.set_fps_index(1);
    clips.set_system_audio(true);
    clips.set_microphone(true);
    clips.set_noise_suppression(true);
    clips.set_start_with_windows(true);
    clips.set_save_hotkey("End".into());
    clips.set_overlay_hotkey("Alt + Z".into());
    clips.set_clips_folder(r"C:\Users\Ada\Videos\UnkvoidClips".into());
    clips.set_buffer_estimate("O replay de 5 min ocupa até 2,0 GB no disco, e o que passa do tempo é apagado sozinho.".into());
    ui.set_settings_tab("clips".into());
    ui.set_settings_open(true);
    shoot("configuracoes-clips")?;

    ui.set_settings_tab("account".into());
    shoot("configuracoes-conta")?;

    // A conta nova, com o apelido automático: o modal que não fecha pede a escolha.
    ui.set_settings_open(false);
    ui.set_nickname_pending(true);
    shoot("apelido-escolha")?;

    ui.set_nickname_error("Esse apelido já é de outra pessoa.".into());
    shoot("apelido-escolha-erro")?;

    ui.set_nickname_pending(false);
    ui.set_nickname_error(SharedString::new());
    ui.set_settings_tab("clips".into());
    ui.set_settings_open(true);
    ui.set_signed_in(false);
    ui.set_screen("entry".into());
    shoot("configuracoes-clips-sem-conta")?;

    // Na sala as abas vão para a barra dela, à direita da casinha, com conta ou sem.
    ui.set_settings_open(false);
    ui.set_screen("room".into());
    shoot("sala-abas-sem-conta")?;

    ui.set_signed_in(true);
    shoot("sala-abas-com-conta")?;

    clips.set_open(true);
    shoot("clips-com-faixa")?;

    clips.set_page(1);
    clips.set_player_title("Corrida Neon · 26/09/2026 21:40".into());
    clips.set_player_time("1:12 / 5:00".into());
    clips.set_player_progress(0.24);
    shoot("clips-player")?;

    // A janela no mínimo do app: os cartões da Home descem um embaixo do outro, e a entrada
    // sem conta tem de caber inteira.
    clips.set_open(false);
    window.set_size(slint::PhysicalSize::new(940, 600));
    ui.set_screen("hub".into());
    ui.set_home_tab("servers".into());
    ui.set_recent_rooms(model(vec![model(vec!["np9cabl01opi".into(), "mg6gag7qik00".into()])]));
    shoot("home-estreita")?;

    window.set_size(slint::PhysicalSize::new(width, height));
    shoot("home-larga")?;

    window.set_size(slint::PhysicalSize::new(940, 600));
    ui.set_signed_in(false);
    ui.set_screen("entry".into());
    shoot("entrada-estreita")?;

    Ok(())
}

fn clip(title: &str, date: &str, details: &str, red: u8, blue: u8) -> ClipItem {
    ClipItem { title: title.into(), date: date.into(), details: details.into(), thumbnail: thumbnail(red, blue) }
}

/// A miniatura de um clipe ou de um monitor: uma das cenas, pequena.
fn thumbnail(red: u8, _blue: u8) -> Image {
    Image::from_rgb8(scene(160, 90, u32::from(red) / 60))
}

/// Uma cena desenhada aqui mesmo — céu, sol, montanhas e uma grade neon em perspectiva — no
/// lugar da tela de um jogo: a foto da loja precisa parecer uma transmissão, e imagem de jogo é
/// marca de outro. `variant` troca a paleta.
fn scene(width: u32, height: u32, variant: u32) -> SharedPixelBuffer<Rgb8Pixel> {
    type Color = [f32; 3];

    // Topo do céu, céu no horizonte, sol em cima, sol embaixo, grade.
    let palettes: [[Color; 5]; 3] = [
        [[26.0, 11.0, 61.0], [255.0, 106.0, 90.0], [255.0, 214.0, 107.0], [255.0, 79.0, 154.0], [255.0, 47.0, 214.0]],
        [[6.0, 18.0, 48.0], [64.0, 160.0, 220.0], [200.0, 255.0, 240.0], [60.0, 200.0, 220.0], [60.0, 220.0, 255.0]],
        [[40.0, 10.0, 20.0], [255.0, 140.0, 60.0], [255.0, 240.0, 150.0], [255.0, 120.0, 60.0], [255.0, 160.0, 60.0]],
    ];
    let [sky_top, sky_horizon, sun_top, sun_bottom, grid] = palettes[variant as usize % palettes.len()];
    let mix = |from: Color, to: Color, amount: f32| -> Color {
        let amount = amount.clamp(0.0, 1.0);

        [0, 1, 2].map(|channel| from[channel] + (to[channel] - from[channel]) * amount)
    };
    let (w, h) = (width as f32, height as f32);
    let horizon = h * 0.58;
    let center = w * 0.5;
    let sun_y = horizon - h * 0.08;
    let sun_radius = h * 0.2;
    let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(width, height);

    for (index, pixel) in buffer.make_mut_slice().iter_mut().enumerate() {
        let (x, y) = ((index as u32 % width) as f32, (index as u32 / width) as f32);

        let color = if y < horizon {
            let mut color = mix(sky_top, sky_horizon, (y / horizon).powf(1.8));

            // Estrelas espalhadas no alto do céu, por um hash da posição.
            let mut hash = (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663);

            hash ^= hash >> 13;
            hash = hash.wrapping_mul(0x5bd1_e995);
            hash ^= hash >> 15;

            if y < horizon * 0.55 && hash.is_multiple_of(900) {
                color = mix(color, [255.0, 255.0, 255.0], 0.8);
            }

            let distance = ((x - center).powi(2) + (y - sun_y).powi(2)).sqrt();

            if distance < sun_radius {
                let down = (y - (sun_y - sun_radius)) / (2.0 * sun_radius);
                let stripe = ((y - sun_y) / (h * 0.022)).fract();

                if down < 0.5 || stripe > (down - 0.5) * 1.4 {
                    color = mix(sun_top, sun_bottom, down);
                }
            }

            let ridge = horizon
                - h * (0.07 + 0.035 * (x / w * 9.0).sin() + 0.02 * (x / w * 23.0 + 1.3).sin() + 0.01 * (x / w * 57.0).sin());

            if y > ridge {
                color = mix([20.0, 8.0, 40.0], [40.0, 14.0, 70.0], (y - ridge) / (horizon - ridge + 1.0));
            }

            color
        } else {
            let depth = y - horizon + 1.0;
            let near = depth / (h - horizon);

            // Linhas para o fundo: saem todas do ponto de fuga no centro do horizonte. Perto dele
            // ficam mais juntas que um pixel e somem, em vez de virar uma faixa sólida.
            let spread = (x - center) / depth / 0.45;
            let across = spread.fract().abs().min(1.0 - spread.fract().abs()) * 0.45 * depth;
            let across_line = (1.4 - across).clamp(0.0, 1.0) * ((0.45 * depth - 1.5) / 3.0).clamp(0.0, 1.0);

            // Linhas de lado a lado, cada vez mais juntas perto do horizonte, e somem do mesmo jeito.
            let distance_z = 6.0 / (near + 0.04);
            let step = (distance_z.fract()).min(1.0 - distance_z.fract());
            let units_per_pixel = 6.0 / (near + 0.04).powi(2) / (h - horizon);
            let along = step / units_per_pixel.max(0.0001);
            let along_line = (1.4 - along).clamp(0.0, 1.0) * (1.0 - (units_per_pixel - 0.3) / 0.4).clamp(0.0, 1.0);

            let line = across_line.max(along_line);
            let base = mix([10.0, 2.0, 30.0], [24.0, 6.0, 52.0], near);

            mix(base, grid, line * (0.35 + 0.65 * near))
        };

        *pixel = Rgb8Pixel::new(color[0] as u8, color[1] as u8, color[2] as u8);
    }

    buffer
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
        sharing: false,
        reconnecting: false,
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
        people: slint::ModelRc::default(),
    }
}

/// Um cartão com uma imagem de mentira: uma das cenas, no tamanho que a tela teria.
fn tile(label: &str, column: i32, line: i32, width: u32, height: u32, watchers: i32) -> TileRow {
    let buffer = scene(width, height, (column + line * 2) as u32);

    TileRow {
        producer: label.into(),
        label: label.into(),
        initial: label.chars().next().unwrap_or('?').to_string().into(),
        mine: false,
        camera: false,
        paused: false,
        audio: true,
        heard: false,
        volume: 100,
        watchers,
        watcher_names: SharedString::new(),
        stats: format!("{height}p · 60 fps · 0,0%").into(),
        loss_high: false,
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
    let (width, height) = (window.size().width, window.size().height);

    std::thread::sleep(Duration::from_millis(400));
    slint::platform::update_timers_and_animations();
    window.request_redraw();

    let mut pixels = vec![PremultipliedRgbaColor::default(); (width * height) as usize];

    window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, width as usize);
    });

    let row = width as usize * 4;
    let mut bmp = Vec::with_capacity(54 + row * height as usize);

    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(54 + row as u32 * height).to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&54_u32.to_le_bytes());
    bmp.extend_from_slice(&40_u32.to_le_bytes());
    bmp.extend_from_slice(&width.to_le_bytes());
    bmp.extend_from_slice(&height.to_le_bytes());
    bmp.extend_from_slice(&1_u16.to_le_bytes());
    bmp.extend_from_slice(&32_u16.to_le_bytes());
    bmp.extend_from_slice(&[0; 24]);

    for line in (0..height as usize).rev() {
        for pixel in &pixels[line * width as usize..(line + 1) * width as usize] {
            bmp.extend_from_slice(&[pixel.blue, pixel.green, pixel.red, 255]);
        }
    }

    std::fs::write(path, bmp)?;
    println!("{path}");

    Ok(())
}
