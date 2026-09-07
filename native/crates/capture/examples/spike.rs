//! Spike da captura nativa.
//!
//! Mesma lógica do spike da web: provar o pedaço mais arriscado antes de construir
//! em cima. Aqui a pergunta é se a captura nativa entrega quadros e áudio de sistema
//! de forma estável, sem barra do navegador e sem o limite do WKWebView.
//!
//! Roda com: cargo run -p capture --example spike -- [720|1080|1440] [segundos]

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

    println!("telas disponíveis:");
    for display in PlatformCapturer::displays()? {
        println!("  #{} — {}x{}", display.id, display.width, display.height);
    }

    let (width, height) = quality.dimensions();
    println!("\ncapturando {width}x{height} por {seconds}s (áudio de sistema ligado)\n");

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
        quality,
        frame_rate: 60,
        capture_audio: true,
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

    let mut anterior = 0;

    for segundo in 1..=seconds {
        std::thread::sleep(Duration::from_secs(1));

        let total = video_frames.load(Ordering::Relaxed);

        println!(
            "{segundo:>3}s  {:>3} fps  {}x{}  áudio: {} blocos",
            total - anterior,
            last_width.load(Ordering::Relaxed),
            last_height.load(Ordering::Relaxed),
            audio_chunks.load(Ordering::Relaxed),
        );

        anterior = total;
    }

    capturer.stop()?;

    let total = video_frames.load(Ordering::Relaxed);
    let media = total as f64 / started.elapsed().as_secs_f64();
    let audio = audio_chunks.load(Ordering::Relaxed);

    println!("\n--- resultado ---");
    println!("quadros: {total} · média {media:.1} fps");
    println!(
        "áudio de sistema: {}",
        if audio > 0 {
            format!("{audio} blocos — FUNCIONA")
        } else {
            "NENHUM BLOCO — não veio".into()
        }
    );
    println!(
        "resolução entregue: {}x{}",
        last_width.load(Ordering::Relaxed),
        last_height.load(Ordering::Relaxed)
    );

    Ok(())
}
