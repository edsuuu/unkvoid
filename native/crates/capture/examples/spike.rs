//! Native capture spike.
//!
//! A mesma ideia da sondagem web: provar a parte mais arriscada antes de construir em
//! cima dela. A pergunta aqui é se a captura nativa entrega quadros e som do sistema de
//! forma confiável, sem barra de navegador e sem o limite do WKWebView.
//!
//! Rode com: cargo run -p capture --example spike -- [720|1080|1440] [segundos]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use capture::{CaptureConfig, CaptureEvent, PlatformCapturer, Quality};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);

    let quality = match args.next().as_deref() {
        Some("720") => Quality::Hd720,
        Some("1440") => Quality::Qhd1440,
        _ => Quality::Hd1080,
    };

    let seconds: u64 = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10);

    println!("available screens:");
    for display in PlatformCapturer::displays()? {
        println!("  #{} — {}x{}", display.id, display.width, display.height);
    }

    let (width, height) = quality.fit(PlatformCapturer::source_size(capture::CaptureSource::PrimaryDisplay)?);
    println!("\ncapturing {width}x{height} for {seconds}s (system audio enabled)\n");

    let video_frames = Arc::new(AtomicU64::new(0));
    let audio_chunks = Arc::new(AtomicU64::new(0));
    let last_width = Arc::new(AtomicU32::new(0));
    let last_height = Arc::new(AtomicU32::new(0));

    let counters = (
        video_frames.clone(),
        audio_chunks.clone(),
        last_width.clone(),
        last_height.clone(),
    );

    let config = CaptureConfig {
        source: capture::CaptureSource::PrimaryDisplay,
        quality,
        frame_rate: 60,
        capture_audio: true,
        mute_listed_apps: true,
        show_cursor: true,
    };

    let started = Instant::now();

    let mut capturer = PlatformCapturer::start(&config, move |event| match event {
        CaptureEvent::Video(frame) => {
            counters.0.fetch_add(1, Ordering::Relaxed);
            counters.2.store(frame.width, Ordering::Relaxed);
            counters.3.store(frame.height, Ordering::Relaxed);
        }
        CaptureEvent::Audio(_) => {
            counters.1.fetch_add(1, Ordering::Relaxed);
        }
    })?;

    let mut previous = 0;

    for segundo in 1..=seconds {
        std::thread::sleep(Duration::from_secs(1));

        let total = video_frames.load(Ordering::Relaxed);

        println!(
            "{segundo:>3}s  {:>3} fps  {}x{}  audio: {} chunks",
            total - previous,
            last_width.load(Ordering::Relaxed),
            last_height.load(Ordering::Relaxed),
            audio_chunks.load(Ordering::Relaxed),
        );

        previous = total;
    }

    capturer.stop()?;

    let total = video_frames.load(Ordering::Relaxed);
    let media = total as f64 / started.elapsed().as_secs_f64();
    let audio = audio_chunks.load(Ordering::Relaxed);

    println!("\n--- resultado ---");
    println!("frames: {total} · average {media:.1} fps");
    println!(
        "system audio: {}",
        if audio > 0 {
            format!("{audio} chunks — WORKING")
        } else {
            "NO CHUNKS — none received".into()
        }
    );
    println!(
        "delivered resolution: {}x{}",
        last_width.load(Ordering::Relaxed),
        last_height.load(Ordering::Relaxed)
    );

    Ok(())
}
