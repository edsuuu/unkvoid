//! Prova que o encoder por hardware funciona, sem depender da captura.
//!
//! Cria uma IOSurface, codifica N quadros e reporta bytes, keyframes e tempo por
//! quadro. Se `ms/quadro` ficar bem abaixo do intervalo do FPS alvo, sobra folga —
//! sinal de que está no chip de mídia e não na CPU.
//!
//! cargo run -p media --example encoder -- [720|1080|1440] [quadros]

use std::time::Instant;

use apple_cf::iosurface::IOSurface;
use capture::Quality;
use media::{EncoderConfig, PlatformEncoder};

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
    .ok_or_else(|| anyhow::anyhow!("não consegui alocar a IOSurface"))?;

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

    println!("quadros codificados: {total}");
    println!("keyframes: {keyframes}");
    println!("saída: {:.1} KB", bytes as f64 / 1024.0);
    println!("média: {media_ms:.2} ms/quadro · pior caso: {pior_ms:.2} ms");
    println!(
        "orçamento a {} fps: {orcamento:.2} ms/quadro",
        config.frame_rate
    );
    println!(
        "\n{}",
        if media_ms < orcamento {
            format!(
                "FOLGA: usa {:.0}% do orçamento por quadro",
                media_ms / orcamento * 100.0
            )
        } else {
            "APERTADO: o encoder não acompanha o FPS alvo".into()
        }
    );

    Ok(())
}
