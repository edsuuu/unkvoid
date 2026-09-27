//! A moldura da janela: o Windows não deixa pôr botão ao lado do minimizar na barra dele, e
//! a atualização pronta mora ali, como no Discord. Então a barra é nossa (`ui/frame.slint`).
//!
//! Arrastar e redimensionar pedem ao Windows o mesmo laço da barra nativa (`drag_window`,
//! `drag_resize_window`): encostar na borda da tela ainda encaixa a janela, e arrastar a
//! maximizada ainda a devolve ao tamanho de antes.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::ComponentHandle;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use winit::window::ResizeDirection;

use crate::{AppWindow, Ui};

/// Dois cliques na barra dentro disto maximizam, como na barra do Windows. O laço de arrastar
/// do sistema engole o segundo clique, então quem conta é aqui.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

pub fn wire(window: &AppWindow) {
    let ui = window.global::<Ui>();
    let last_press: Rc<Cell<Option<Instant>>> = Rc::default();

    ui.on_drag_window({
        let window = window.as_weak();

        move || {
            let Some(app) = window.upgrade() else {
                return;
            };
            let now = Instant::now();

            if last_press.replace(Some(now)).is_some_and(|before| now.duration_since(before) < DOUBLE_CLICK) {
                last_press.set(None);
                toggle_maximize(&app);

                return;
            }

            app.window().with_winit_window(|winit| {
                if let Err(failure) = winit.drag_window() {
                    tracing::warn!(%failure, "moldura: o Windows não arrastou a janela");
                }
            });
        }
    });

    ui.on_resize_window({
        let window = window.as_weak();

        move |side| {
            let (Some(app), Some(direction)) = (window.upgrade(), direction_of(&side)) else {
                return;
            };

            app.window().with_winit_window(|winit| {
                if let Err(failure) = winit.drag_resize_window(direction) {
                    tracing::warn!(%failure, "moldura: o Windows não redimensionou a janela");
                }
            });
        }
    });

    ui.on_minimize_window({
        let window = window.as_weak();

        move || {
            if let Some(app) = window.upgrade() {
                app.window().set_minimized(true);
            }
        }
    });

    ui.on_toggle_maximize({
        let window = window.as_weak();

        move || {
            if let Some(app) = window.upgrade() {
                toggle_maximize(&app);
            }
        }
    });

    ui.on_close_window(|| {
        if let Err(failure) = slint::quit_event_loop() {
            tracing::warn!(%failure, "moldura: a janela não fechou");
        }
    });

    // A janela do winit só existe com o laço de eventos rodando: é no primeiro evento dela
    // que a sombra e o canto voltam. E maximizar por fora — o encaixe no topo da tela, o
    // Win+↑ — só chega aqui pelo tamanho que mudou.
    window.window().on_winit_window_event({
        let (window, dressed) = (window.as_weak(), Cell::new(false));

        move |slint_window, event| {
            if !dressed.get() {
                dressed.set(slint_window.with_winit_window(dress).is_some());
            }

            if matches!(event, winit::event::WindowEvent::Resized(_))
                && let Some(app) = window.upgrade()
            {
                app.global::<Ui>().set_maximized(slint_window.is_maximized());
            }

            EventResult::Propagate
        }
    });
}

fn toggle_maximize(app: &AppWindow) {
    let window = app.window();
    let maximized = !window.is_maximized();

    window.set_maximized(maximized);
    app.global::<Ui>().set_maximized(maximized);
}

fn direction_of(side: &str) -> Option<ResizeDirection> {
    Some(match side {
        "n" => ResizeDirection::North,
        "s" => ResizeDirection::South,
        "e" => ResizeDirection::East,
        "w" => ResizeDirection::West,
        "ne" => ResizeDirection::NorthEast,
        "nw" => ResizeDirection::NorthWest,
        "se" => ResizeDirection::SouthEast,
        "sw" => ResizeDirection::SouthWest,
        _ => return None,
    })
}

/// Sem a moldura, o Windows tira junto a sombra e o canto redondo do 11. A sombra volta pelo
/// winit; o canto, pelo DWM — no Windows 10 o pedido é recusado calado, e o canto fica reto.
#[cfg(target_os = "windows")]
fn dress(window: &winit::window::Window) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DWM_WINDOW_CORNER_PREFERENCE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute};
    use winit::platform::windows::WindowExtWindows;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    window.set_undecorated_shadow(true);

    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let rounded = DWMWCP_ROUND;

    // SAFETY: o HWND é o da janela viva que o winit acabou de entregar, e o valor aponta para
    // uma variável local do tamanho que o próprio atributo pede.
    let asked = unsafe {
        DwmSetWindowAttribute(
            HWND(handle.hwnd.get() as *mut std::ffi::c_void),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const rounded).cast(),
            size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        )
    };

    if let Err(failure) = asked {
        tracing::info!(%failure, "moldura: sem canto redondo neste Windows");
    }
}

#[cfg(not(target_os = "windows"))]
fn dress(_window: &winit::window::Window) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_edge_the_frame_draws_pulls_the_window() {
        for side in ["n", "s", "e", "w", "ne", "nw", "se", "sw"] {
            assert!(direction_of(side).is_some(), "{side}");
        }

        assert!(direction_of("meio").is_none());
    }
}
