//! O escuro translúcido atrás do painel do Alt+Z: uma janela preta do Windows cobrindo o
//! monitor, com a transparência feita pelo próprio Windows (janela em camadas).
//!
//! Separada do painel de propósito. A transparência por janela vale para a janela inteira, e o
//! quadro de configurações não pode ser translúcido; a por pixel, pelo Slint, falhava no driver
//! da NVIDIA e cobria o jogo de preto sólido. Assim o fundo é translúcido e o painel, que fica
//! por cima desta, é sólido.
//!
//! Não pega o foco (o painel é que pega, para o Esc) e fecha o painel quando alguém clica fora
//! dele.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GW_HWNDPREV, GetWindow, HWND_TOPMOST, IDC_ARROW, LWA_ALPHA, LoadCursorW,
    MA_NOACTIVATE, RegisterClassW, SW_HIDE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    SetLayeredWindowAttributes, SetWindowPos,
    ShowWindow, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_RBUTTONDOWN, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

const CLASS: PCWSTR = w!("Unkvoid-backdrop");

thread_local! {
    static WINDOW: Cell<Option<isize>> = const { Cell::new(None) };
    static ON_CLICK: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
}

/// O que fazer quando clicam no escuro, fora do painel. Só na thread da interface.
pub fn on_click(action: impl Fn() + 'static) {
    ON_CLICK.set(Some(Box::new(action)));
}

/// Cobre o retângulo (pixels físicos) com o preto, na opacidade pedida (0 a 255).
pub fn show(x: i32, y: i32, width: i32, height: i32, opacity: u8) {
    let Some(window) = window() else { return };

    unsafe {
        let _ = SetLayeredWindowAttributes(window, COLORREF(0), opacity, LWA_ALPHA);
        let _ = SetWindowPos(window, Some(HWND_TOPMOST), x, y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

pub fn set_opacity(opacity: u8) {
    if let Some(window) = WINDOW.get() {
        unsafe {
            let _ = SetLayeredWindowAttributes(HWND(window as *mut _), COLORREF(0), opacity, LWA_ALPHA);
        }
    }
}

/// O painel no topo e o escuro logo abaixo dele. As duas são "sempre no topo", e entre elas
/// vale a última que subiu: o escuro sobe a cada abertura, e sem isto podia cobrir o painel.
pub fn keep_below(panel: HWND) {
    let Some(window) = WINDOW.get() else { return };

    unsafe {
        let _ = SetWindowPos(panel, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        let _ = SetWindowPos(HWND(window as *mut _), Some(panel), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Se o painel está acima do escuro na pilha de janelas: o que o log conta quando o painel
/// não aparece.
pub fn is_below(panel: HWND) -> bool {
    let Some(window) = WINDOW.get() else { return false };
    let mut above = HWND(window as *mut _);

    while let Ok(next) = unsafe { GetWindow(above, GW_HWNDPREV) } {
        if next == panel {
            return true;
        }

        above = next;
    }

    false
}

/// Sem clique no escuro enquanto o seletor de pasta, aberto pelo painel, está na tela: ele fecharia
/// o painel por baixo do diálogo.
pub fn set_enabled(enabled: bool) {
    if let Some(window) = WINDOW.get() {
        unsafe {
            let _ = EnableWindow(HWND(window as *mut _), enabled);
        }
    }
}

pub fn hide() {
    if let Some(window) = WINDOW.get() {
        unsafe {
            let _ = ShowWindow(HWND(window as *mut _), SW_HIDE);
        }
    }
}

/// A janela é criada uma vez e reaproveitada: o Alt+Z abre e fecha o tempo todo.
fn window() -> Option<HWND> {
    if let Some(window) = WINDOW.get() {
        return Some(HWND(window as *mut _));
    }

    unsafe {
        let instance = GetModuleHandleW(None).ok()?;

        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            lpszClassName: CLASS,
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            // Sem cursor na classe, o do jogo (às vezes escondido) fica por cima do escuro todo.
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        });

        let window = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
            CLASS,
            w!("Unkvoid"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;

        WINDOW.set(Some(window.0 as isize));

        Some(window)
    }
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        // Clicar no escuro não tira o foco do painel (nem o dá a esta janela).
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
            ON_CLICK.with_borrow(|action| {
                if let Some(action) = action {
                    action();
                }
            });

            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}
