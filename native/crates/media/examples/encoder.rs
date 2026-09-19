//! Prova que o encoder de hardware funciona sem depender da captura.
//!
//! Cria um IOSurface, codifica N quadros e informa bytes, keyframes e tempo por quadro.
//! Se o tempo por quadro fica bem abaixo do intervalo do fps alvo, há folga — sinal de
//! que ele roda no chip de mídia, e não no processador.
//!
//! cargo run -p media --example encoder -- [720|1080|1440] [frames]
//!
//! No Windows a pergunta é outra: o MFT aceita trocar a taxa com o encoder no ar? Uma
//! textura de ruído entra no lugar da captura, a taxa vai do teto ao piso e volta, e sai a
//! taxa medida em cada trecho. É a prova de que o `set_bitrate` do governador faz efeito.
//!
//! No Linux não há o que medir: o encoder é o `gst-launch` da captura. O esqueleto abaixo
//! mantém o workspace compilando lá — o `cargo test` constrói todo exemplo.

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() {
    eprintln!("this example only runs on macOS and Windows: on Linux the capture encodes");
}

#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    use rand::RngCore;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
        D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

    /// Dez segundos por trecho, medidos segundo a segundo: a média do trecho inteiro
    /// esconde quanto tempo o controle de taxa leva para chegar ao alvo novo, e é esse
    /// tempo que diz se a janela de 1 s do governador enxerga o efeito da própria troca.
    const SECONDS: u64 = 10;
    const FRAMES_PER_SECOND: u64 = 60;
    const FRAME_NS: u64 = 16_666_667;

    tracing_subscriber::fmt().with_target(false).without_time().init();

    let quality = match std::env::args().nth(1).as_deref() {
        Some("720") => capture::Quality::Hd720,
        Some("1440") => capture::Quality::Qhd1440,
        _ => capture::Quality::Hd1080,
    };
    let config = media::EncoderConfig::new(quality, 60, (1920, 1080));
    let (width, height) = (1920_u32, 1080_u32);

    // O device que faria o papel da captura: o encoder cria o dele e monta a ponte.
    let (mut device, mut context) = (None, None);

    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            Default::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }

    let device = device.ok_or_else(|| anyhow::anyhow!("no Direct3D device"))?;
    let context = context.ok_or_else(|| anyhow::anyhow!("no Direct3D context"))?;
    let mut texture = None;

    unsafe {
        device.CreateTexture2D(
            &D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            },
            None,
            Some(&mut texture),
        )?;
    }

    let surface = capture::GpuSurface {
        texture: texture.ok_or_else(|| anyhow::anyhow!("no texture"))?,
        device,
        context,
    };

    // Ruído de amplitude média sobre cinza, um quadro diferente do outro: o pior caso de
    // um jogo, mas que ainda some com quantização grossa. Imagem parada sairia em poucos
    // kb/s com qualquer alvo, e ruído cheio estouraria qualquer alvo: nos dois casos a
    // taxa medida não diria nada sobre o que foi pedido.
    let noise: Vec<Vec<u8>> = (0..8)
        .map(|_| {
            let mut pixels = vec![0_u8; (width * height * 4) as usize];

            rand::thread_rng().fill_bytes(&mut pixels);
            pixels.iter_mut().for_each(|byte| *byte = 96 + (*byte & 63));

            pixels
        })
        .collect();

    let mut encoder = media::PlatformEncoder::new(&config)?;
    let ceiling = encoder.bitrate();

    println!(
        "encoder H.264 · {}x{} · {} · teto {} kb/s\n",
        config.width,
        config.height,
        if encoder.hardware() { "placa" } else { "processador" },
        ceiling / 1000
    );

    let mut index = 0_u64;

    for bitrate in [ceiling, ceiling * 35 / 100, ceiling] {
        let accepted = bitrate == encoder.bitrate() || encoder.set_bitrate(bitrate);
        let mut per_second = Vec::with_capacity(SECONDS as usize);

        for _ in 0..SECONDS {
            let mut bytes = 0_u64;

            for _ in 0..FRAMES_PER_SECOND {
                unsafe {
                    surface.context.UpdateSubresource(
                        &surface.texture,
                        0,
                        None,
                        noise[(index % 8) as usize].as_ptr().cast(),
                        width * 4,
                        0,
                    );
                }

                let outcome = encoder.encode(&surface, index * FRAME_NS);

                index += 1;

                match outcome {
                    Ok(frame) => bytes += frame.data.len() as u64,
                    Err(media::EncoderError::NeedsMoreInput) => {}
                    Err(error) => return Err(error.into()),
                }
            }

            per_second.push(bytes * 8 / 1000);
        }

        println!("pedido {:>6} kb/s · aceito: {accepted} · kb/s por segundo: {per_second:?}", bitrate / 1000);
    }

    Ok(())
}

#[cfg(target_os = "macos")]
use std::time::Instant;

#[cfg(target_os = "macos")]
use apple_cf::iosurface::IOSurface;
#[cfg(target_os = "macos")]
use capture::Quality;
#[cfg(target_os = "macos")]
use media::{EncoderConfig, PlatformEncoder};

/// Os tipos de NAL de um bitstream Annex-B, na ordem em que aparecem.
#[cfg(target_os = "macos")]
fn nal_types(data: &[u8]) -> Vec<u8> {
    data.windows(4)
        .enumerate()
        .filter(|(_, janela)| *janela == [0, 0, 0, 1])
        .filter_map(|(inicio, _)| data.get(inicio + 4))
        .map(|byte| byte & 0x1F)
        .collect()
}

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);

    let quality = match args.next().as_deref() {
        Some("720") => Quality::Hd720,
        Some("1440") => Quality::Qhd1440,
        _ => Quality::Hd1080,
    };

    let total: u64 = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(120);
    // Um monitor 16:9 de mentira: o encoder não precisa de tela para ser medido.
    let config = EncoderConfig::new(quality, 60, (3840, 2160));
    let (width, height) = (config.width, config.height);

    println!(
        "encoder H.264 · {width}x{height} · {} Mbps · alvo {} fps\n",
        config.bitrate / 1_000_000,
        config.frame_rate
    );

    let surface = IOSurface::create(
        width as usize,
        height as usize,
        u32::from_be_bytes(*b"BGRA"),
        4,
    )
    .ok_or_else(|| anyhow::anyhow!("could not allocate the IOSurface"))?;

    let mut encoder = PlatformEncoder::new(&config)?;

    let start = Instant::now();
    let mut bytes = 0usize;
    let mut keyframes = 0u64;
    let mut worst_ms = 0f64;

    for index in 0..total {
        let started = Instant::now();
        let frame = encoder.encode(&surface, index * 16_666_667)?;
        let took = started.elapsed().as_secs_f64() * 1000.0;

        bytes += frame.data.len();
        keyframes += u64::from(frame.keyframe);
        worst_ms = worst_ms.max(took);

        // O empacotador RTP só quebra Annex-B, e só aprende SPS/PPS se eles passarem
        // por ele. Um keyframe sem os dois vira uma transmissão que nenhuma tela abre,
        // sem erro em contador nenhum — foi exatamente o que aconteceu por meses.
        if frame.keyframe {
            let types = nal_types(&frame.data);

            anyhow::ensure!(
                frame.data.starts_with(&[0, 0, 0, 1]),
                "keyframe não começa com start code: o bitstream saiu em AVCC",
            );
            anyhow::ensure!(
                types.contains(&7) && types.contains(&8) && types.contains(&5),
                "keyframe sem SPS(7), PPS(8) ou IDR(5): veio {types:?}",
            );
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    let average_ms = elapsed * 1000.0 / total as f64;
    let budget = 1000.0 / config.frame_rate;

    println!("encoded frames: {total}");
    println!("keyframes: {keyframes}");
    println!("output: {:.1} KB", bytes as f64 / 1024.0);
    println!("average: {average_ms:.2} ms/frame · worst case: {worst_ms:.2} ms");
    println!(
        "budget at {} fps: {budget:.2} ms/frame",
        config.frame_rate
    );
    println!(
        "\n{}",
        if average_ms < budget {
            format!(
                "HEADROOM: uses {:.0}% of the per-frame budget",
                average_ms / budget * 100.0
            )
        } else {
            "TIGHT: the encoder cannot keep up with the target FPS".into()
        }
    );

    Ok(())
}
