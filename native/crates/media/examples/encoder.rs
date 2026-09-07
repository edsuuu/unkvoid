//! Proves that the hardware encoder works without depending on capture.
//!
//! Creates an IOSurface, encodes N frames, and reports bytes, keyframes, and time
//! per frame. If `ms/frame` is well below the target FPS interval, there is headroom —
//! a sign that it runs on the media chip rather than the CPU.
//!
//! cargo run -p media --example encoder -- [720|1080|1440] [frames]
//!
//! macOS only: it builds an IOSurface by hand. The stub below keeps the workspace
//! compiling elsewhere — `cargo test` builds every example, so without it the whole
//! suite fails on Linux and Windows for a file that could never run there anyway.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("this example only runs on macOS: it encodes from an IOSurface");
}

#[cfg(target_os = "macos")]
use std::time::Instant;

#[cfg(target_os = "macos")]
use apple_cf::iosurface::IOSurface;
#[cfg(target_os = "macos")]
use capture::Quality;
#[cfg(target_os = "macos")]
use media::{EncoderConfig, PlatformEncoder};

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
        .and_then(|valor| valor.parse().ok())
        .unwrap_or(120);
    let (width, height) = quality.dimensions();
    let config = EncoderConfig::for_quality(quality);

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

    let inicio = Instant::now();
    let mut bytes = 0usize;
    let mut keyframes = 0u64;
    let mut pior_ms = 0f64;

    for indice in 0..total {
        let antes = Instant::now();
        let quadro = encoder.encode(&surface, indice * 16_666_667)?;
        let levou = antes.elapsed().as_secs_f64() * 1000.0;

        bytes += quadro.data.len();
        keyframes += u64::from(quadro.keyframe);
        pior_ms = pior_ms.max(levou);
    }

    let decorrido = inicio.elapsed().as_secs_f64();
    let media_ms = decorrido * 1000.0 / total as f64;
    let orcamento = 1000.0 / config.frame_rate;

    println!("encoded frames: {total}");
    println!("keyframes: {keyframes}");
    println!("output: {:.1} KB", bytes as f64 / 1024.0);
    println!("average: {media_ms:.2} ms/frame · worst case: {pior_ms:.2} ms");
    println!(
        "budget at {} fps: {orcamento:.2} ms/frame",
        config.frame_rate
    );
    println!(
        "\n{}",
        if media_ms < orcamento {
            format!(
                "HEADROOM: uses {:.0}% of the per-frame budget",
                media_ms / orcamento * 100.0
            )
        } else {
            "TIGHT: the encoder cannot keep up with the target FPS".into()
        }
    );

    Ok(())
}
