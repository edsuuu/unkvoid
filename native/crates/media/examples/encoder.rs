//! Prova que o encoder de hardware funciona sem depender da captura.
//!
//! Cria um IOSurface, codifica N quadros e informa bytes, keyframes e tempo por quadro.
//! Se o tempo por quadro fica bem abaixo do intervalo do fps alvo, há folga — sinal de
//! que ele roda no chip de mídia, e não no processador.
//!
//! cargo run -p media --example encoder -- [720|1080|1440] [frames]
//!
//! Só macOS: ele monta um IOSurface à mão. O esqueleto abaixo mantém o workspace
//! compilando nos outros sistemas — o `cargo test` constrói todo exemplo, e sem ele a
//! suíte inteira falharia no Linux e no Windows por um arquivo que nunca rodaria lá.

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
