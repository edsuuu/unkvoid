//! O Unkvoid no Windows: Slint por cima do `shared/core`, sem ponte nenhuma no meio.
//!
//! A janela é a pilha das cinco telas; quem diz qual está valendo é o núcleo. Este arquivo
//! só abre a janela, liga os cliques ao núcleo e entrega o laço de eventos ao Slint.

// Sem console atrás da janela no Windows. Em `debug` ele fica: é onde o `tracing` aparece.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod bridge;
mod devices;
mod sharing;

use slint::ComponentHandle;

use bridge::Bridge;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let window = AppWindow::new()?;
    let bridge = Bridge::new(window.as_weak())?;

    bridge.wire(&window);
    bridge.start();

    window.run()?;

    Ok(())
}
