//! Bateria de diagnóstico da transmissão: um passo por processo.
//!
//! Existe porque "o app fecha ao clicar em transmitir" não diz onde. Rodar os passos em
//! sequência no mesmo processo também não resolveria: o primeiro que derrubasse levaria
//! junto a chance de saber se os seguintes funcionam.
//!
//! Aqui o pai executa a si mesmo uma vez por passo. O filho morre, o pai continua e
//! anota o código de saída — inclusive `0xC0000005`, a violação de acesso do Windows,
//! que não gera erro do Rust nem entra em nenhum hook de pânico.
//!
//! ```text
//! cargo run -p media --example diagnose
//! cargo run -p media --example diagnose -- --quality 1440 --source display:1
//! ```

use std::io::Write;
use std::process::{Command, ExitCode};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use capture::{CaptureConfig, CaptureEvent, CaptureSource, PlatformCapturer, Quality};
use media::{AudioEncoder, EncoderConfig, PlatformEncoder};

/// Quanto tempo cada passo que depende de quadros fica no ar.
const WINDOW: Duration = Duration::from_secs(3);

/// A ordem importa: é a mesma de `Broadcast::start`, e cada passo só faz sentido se o
/// anterior passou.
const STEPS: &[(&str, &str)] = &[
    ("displays", "listar os monitores"),
    ("encoder", "abrir o encoder de vídeo"),
    ("audio", "abrir o encoder de áudio"),
    ("capture", "capturar a tela, sem codificar"),
    ("pipeline", "capturar E codificar (monta a ponte)"),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|item| item == flag)
            .and_then(|at| args.get(at + 1))
            .cloned()
    };

    let quality = value("--quality").unwrap_or_else(|| "1080".into());
    let source = value("--source").unwrap_or_else(|| "primary".into());

    match value("--step") {
        Some(step) => run_step(&step, &quality, &source),
        None => run_all(&quality, &source),
    }
}

/// O pai: um filho por passo, e o veredito de cada um.
fn run_all(quality: &str, source: &str) -> ExitCode {
    let Ok(myself) = std::env::current_exe() else {
        println!("não consegui descobrir o meu próprio caminho");

        return ExitCode::FAILURE;
    };

    println!(
        "unkvoid: diagnóstico em {} · qualidade {quality} · fonte {source}\n",
        std::env::consts::OS
    );

    let mut failed = 0;

    for (index, (step, what)) in STEPS.iter().enumerate() {
        print!("[{}/{}] {what:<38} ", index + 1, STEPS.len());
        let _ = std::io::stdout().flush();

        let output = Command::new(&myself)
            .args(["--step", step, "--quality", quality, "--source", source])
            .output();

        let Ok(output) = output else {
            println!("NÃO RODOU  (não consegui iniciar o processo filho)");
            failed += 1;

            continue;
        };

        let said = String::from_utf8_lossy(&output.stdout);
        let last = said.lines().filter(|line| !line.is_empty()).last();

        if output.status.success() {
            println!("ok         {}", last.unwrap_or(""));

            continue;
        }

        failed += 1;
        println!("{}", verdict(output.status.code()));

        // Tudo que o filho alcançou a dizer antes de morrer. É aqui que aparece o último
        // passo interno, e é o que interessa quando ele morreu sem falar.
        for line in said.lines().filter(|line| !line.is_empty()) {
            println!("           · {line}");
        }

        let cried = String::from_utf8_lossy(&output.stderr);

        for line in cried.lines().filter(|line| !line.is_empty()) {
            println!("           ! {line}");
        }
    }

    println!();

    if failed == 0 {
        println!("todos os passos passaram: o problema não está na abertura da transmissão");

        return ExitCode::SUCCESS;
    }

    println!("{failed} passo(s) com problema — o primeiro da lista é o que interessa");

    ExitCode::FAILURE
}

/// Traduz o código de saída do filho. Um processo que morre sujo não devolve `1`.
fn verdict(code: Option<i32>) -> String {
    let Some(code) = code else {
        return "MORREU     (encerrado por sinal)".into();
    };

    if code == 1 {
        return "FALHOU     (erro tratado, detalhe abaixo)".into();
    }

    let raw = code as u32;
    let name = match raw {
        0xC000_0005 => " — violação de acesso",
        0xC000_0006 => " — erro de página",
        0xC000_0017 => " — sem memória",
        0xC000_001D => " — instrução ilegal",
        0xC000_0094 => " — divisão por zero",
        0xC000_00FD => " — estouro de pilha",
        0xC000_0409 => " — pilha corrompida",
        _ => "",
    };

    format!("MORREU     (código {raw:#010X}{name})")
}

/// O filho: um passo só, e sai.
fn run_step(step: &str, quality: &str, source: &str) -> ExitCode {
    let quality = match quality {
        "720" => Quality::Hd720,
        "1440" => Quality::Qhd1440,
        _ => Quality::Hd1080,
    };

    let source = match source.split_once(':') {
        Some(("display", id)) => id.parse().map(CaptureSource::Display).unwrap_or_default(),
        Some(("window", id)) => id.parse().map(CaptureSource::Window).unwrap_or_default(),
        _ => CaptureSource::PrimaryDisplay,
    };

    let outcome = match step {
        "displays" => displays(),
        "encoder" => encoder(quality),
        "audio" => audio(),
        "capture" => capture(quality, source),
        "pipeline" => pipeline(quality, source),
        other => Err(format!("passo desconhecido: {other}")),
    };

    match outcome {
        Ok(detail) => {
            say(&detail);

            ExitCode::SUCCESS
        }
        Err(problem) => {
            say(&problem);

            ExitCode::FAILURE
        }
    }
}

/// Uma linha do filho, já em disco antes da próxima chamada perigosa. Sem o flush, a
/// saída morreria no buffer junto com o processo — que é exatamente o caso que importa.
fn say(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
}

fn displays() -> Result<String, String> {
    let displays = PlatformCapturer::displays().map_err(|error| error.to_string())?;

    Ok(displays
        .iter()
        .map(|display| format!("display:{} {}x{}", display.id, display.width, display.height))
        .collect::<Vec<_>>()
        .join(", "))
}

fn encoder(quality: Quality) -> Result<String, String> {
    let config = EncoderConfig::new(quality, 60);

    say(&format!(
        "abrindo o encoder em {}x{} a {} kbps",
        config.quality.dimensions().0,
        config.quality.dimensions().1,
        config.bitrate / 1000
    ));

    PlatformEncoder::new(&config).map_err(|error| error.to_string())?;

    Ok("encoder aberto e fechado sem incidente".into())
}

fn audio() -> Result<String, String> {
    AudioEncoder::new(96_000).map_err(|error| error.to_string())?;

    Ok("encoder de áudio aberto".into())
}

/// Só a captura. Separa "a tela não vem" de "o encoder derruba".
fn capture(quality: Quality, source: CaptureSource) -> Result<String, String> {
    let frames = Arc::new(AtomicU64::new(0));
    let surfaces = Arc::new(AtomicU64::new(0));
    let audio_blocks = Arc::new(AtomicU64::new(0));
    let (frames_seen, surfaces_seen, audio_seen) =
        (frames.clone(), surfaces.clone(), audio_blocks.clone());

    say("abrindo a captura");

    let mut capturer = PlatformCapturer::start(
        &CaptureConfig {
            quality,
            source,
            frame_rate: 60,
            capture_audio: true,
            ..CaptureConfig::default()
        },
        move |event| match event {
            CaptureEvent::Video(frame) => {
                frames_seen.fetch_add(1, Ordering::Relaxed);

                if frame.surface.is_some() {
                    surfaces_seen.fetch_add(1, Ordering::Relaxed);
                }
            }
            CaptureEvent::Audio(_) => {
                audio_seen.fetch_add(1, Ordering::Relaxed);
            }
        },
    )
    .map_err(|error| error.to_string())?;

    say("captura aberta, contando quadros por 3 s");
    std::thread::sleep(WINDOW);
    capturer.stop().map_err(|error| error.to_string())?;

    Ok(format!(
        "{} quadros, {} com buffer de GPU, {} blocos de áudio",
        frames.load(Ordering::Relaxed),
        surfaces.load(Ordering::Relaxed),
        audio_blocks.load(Ordering::Relaxed)
    ))
}

/// Captura e encoder juntos: é aqui que a ponte entre os dois devices do Direct3D nasce,
/// no primeiro quadro. Um crash só neste passo aponta para a ponte, não para a abertura.
fn pipeline(quality: Quality, source: CaptureSource) -> Result<String, String> {
    let config = EncoderConfig::new(quality, 60);

    say("abrindo o encoder");

    let encoder = std::sync::Mutex::new(PlatformEncoder::new(&config).map_err(|e| e.to_string())?);
    let encoded = Arc::new(AtomicU64::new(0));
    let bytes = Arc::new(AtomicU64::new(0));
    let refused = Arc::new(AtomicU64::new(0));
    let first = Arc::new(AtomicU64::new(0));
    let (encoded_seen, bytes_seen, refused_seen, first_seen) =
        (encoded.clone(), bytes.clone(), refused.clone(), first.clone());

    say("abrindo a captura");

    let mut capturer = PlatformCapturer::start(
        &CaptureConfig {
            quality,
            source,
            frame_rate: 60,
            capture_audio: false,
            ..CaptureConfig::default()
        },
        move |event| {
            let CaptureEvent::Video(frame) = event else {
                return;
            };

            let Some(surface) = frame.surface.as_ref() else {
                return;
            };

            // Só o primeiro quadro fala, e fala antes de entrar no encoder: se o
            // processo morrer aqui, esta é a última linha e ela nomeia a ponte.
            if first_seen.fetch_add(1, Ordering::Relaxed) == 0 {
                say(&format!(
                    "primeiro quadro: {}x{} — entrando no encoder (monta a ponte)",
                    frame.width, frame.height
                ));
            }

            let Ok(mut encoder) = encoder.lock() else {
                return;
            };

            match encoder.encode(surface, frame.timestamp_ns) {
                Ok(done) => {
                    if encoded_seen.fetch_add(1, Ordering::Relaxed) == 0 {
                        say(&format!("primeiro quadro codificado: {} bytes", done.data.len()));
                    }

                    bytes_seen.fetch_add(done.data.len() as u64, Ordering::Relaxed);
                }
                Err(error) => {
                    // Fila do encoder de hardware enchendo conta como recusa, não como
                    // defeito: nos primeiros quadros ela é esperada.
                    if refused_seen.fetch_add(1, Ordering::Relaxed) == 0 {
                        say(&format!("primeira recusa do encoder: {error}"));
                    }
                }
            }
        },
    )
    .map_err(|error| error.to_string())?;

    say("no ar, codificando por 3 s");

    let started = Instant::now();

    while started.elapsed() < WINDOW {
        std::thread::sleep(Duration::from_millis(100));
    }

    capturer.stop().map_err(|error| error.to_string())?;

    let done = encoded.load(Ordering::Relaxed);

    if done == 0 {
        return Err(format!(
            "nenhum quadro codificado em 3 s ({} tentativas, {} recusas)",
            first.load(Ordering::Relaxed),
            refused.load(Ordering::Relaxed)
        ));
    }

    Ok(format!(
        "{done} quadros codificados, {} KB, {} recusas",
        bytes.load(Ordering::Relaxed) / 1024,
        refused.load(Ordering::Relaxed)
    ))
}
