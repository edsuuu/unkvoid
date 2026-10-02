//! O aviso de "replay salvo" no canto da tela.
//!
//! Uma janela do Windows desenhada à mão, e não do Slint, porque ela precisa aparecer por
//! cima do jogo sem nunca pegar o foco: janela que ativa tira o jogo de tela cheia e o
//! minimiza. `WS_EX_NOACTIVATE` + `SW_SHOWNOACTIVATE` é o que garante isso, e a janela do
//! Slint não expõe nenhum dos dois. `WS_EX_TRANSPARENT` deixa o clique passar para o jogo.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DT_END_ELLIPSIS, DT_LEFT,
    DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FillRect, FW_NORMAL,
    FW_SEMIBOLD, HFONT, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, SetWindowRgn,
    TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, KillTimer,
    RegisterClassW, SPI_GETWORKAREA, SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SetLayeredWindowAttributes, SetTimer, ShowWindow, SystemParametersInfoW, WM_DESTROY, WM_PAINT,
    WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP, LWA_ALPHA,
};
use windows::core::{PCWSTR, w};

const CLASS: PCWSTR = w!("unkvoid-toast");
const VISIBLE_MS: u32 = 3_000;

thread_local! {
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
    static CURRENT: Cell<Option<isize>> = const { Cell::new(None) };
    static TEXT: RefCell<(Vec<u16>, Vec<u16>)> = const { RefCell::new((Vec::new(), Vec::new())) };
}

/// Mostra o aviso por três segundos. Só na thread da interface: é ela que roda o laço de
/// mensagens que pinta a janela.
pub fn show(title: &str, detail: &str) {
    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else { return };

        if !REGISTERED.get() {
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: instance.into(),
                lpszClassName: CLASS,
                ..Default::default()
            });
            REGISTERED.set(true);
        }

        if let Some(previous) = CURRENT.take() {
            let _ = DestroyWindow(HWND(previous as *mut _));
        }

        TEXT.set((title.encode_utf16().collect(), detail.encode_utf16().collect()));

        let mut work = RECT::default();
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(std::ptr::from_mut(&mut work).cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));

        let Ok(window) = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
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
        ) else {
            return;
        };

        let scale = GetDpiForWindow(window).max(96) as i32;
        let (width, height, margin) = (360 * scale / 96, 68 * scale / 96, 20 * scale / 96);

        let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
            window,
            None,
            work.right - width - margin,
            work.top + margin,
            width,
            height,
            windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER,
        );

        let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, 14 * scale / 96, 14 * scale / 96);

        SetWindowRgn(window, Some(region), false);
        let _ = SetLayeredWindowAttributes(window, COLORREF(0), 242, LWA_ALPHA);
        let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
        SetTimer(Some(window), 1, VISIBLE_MS, None);
        CURRENT.set(Some(window.0 as isize));
    }
}

/// `COLORREF` é 0x00BBGGRR.
const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF(red as u32 | (green as u32) << 8 | (blue as u32) << 16)
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match message {
            WM_PAINT => {
                paint(window);

                LRESULT(0)
            }
            WM_TIMER => {
                let _ = KillTimer(Some(window), 1);
                let _ = DestroyWindow(window);

                LRESULT(0)
            }
            WM_DESTROY => {
                if CURRENT.get() == Some(window.0 as isize) {
                    CURRENT.set(None);
                }

                LRESULT(0)
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}

unsafe fn paint(window: HWND) {
    unsafe {
        let mut paint = PAINTSTRUCT::default();
        let context = BeginPaint(window, &mut paint);
        let scale = GetDpiForWindow(window).max(96) as i32;
        let mut client = RECT::default();

        let _ = GetClientRect(window, &mut client);

        let background = CreateSolidBrush(rgb(0x14, 0x11, 0x1f));
        let accent = CreateSolidBrush(rgb(0x8a, 0x7c, 0xf5));

        FillRect(context, &client, background);
        FillRect(context, &RECT { right: 5 * scale / 96, ..client }, accent);
        SetBkMode(context, TRANSPARENT);

        let font = |size: i32, weight: u32| -> HFONT {
            CreateFontW(-size * scale / 96, 0, 0, 0, weight as i32, 0, 0, 0, Default::default(), Default::default(), Default::default(), Default::default(), 0, w!("Segoe UI"))
        };
        let title_font = font(15, FW_SEMIBOLD.0);
        let detail_font = font(12, FW_NORMAL.0);
        let left = 20 * scale / 96;
        let right = client.right - 14 * scale / 96;
        let middle = client.bottom / 2;

        TEXT.with_borrow_mut(|(title, detail)| {
            let format = DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS;

            SelectObject(context, title_font.into());
            SetTextColor(context, rgb(0xff, 0xff, 0xff));
            DrawTextW(context, title, &mut RECT { left, top: 0, right, bottom: middle + 2 * scale / 96 }, format);
            SelectObject(context, detail_font.into());
            SetTextColor(context, rgb(0xa8, 0x9f, 0xc0));
            DrawTextW(context, detail, &mut RECT { left, top: middle - 2 * scale / 96, right, bottom: client.bottom }, format);
        });

        let _ = DeleteObject(title_font.into());
        let _ = DeleteObject(detail_font.into());
        let _ = DeleteObject(background.into());
        let _ = DeleteObject(accent.into());
        let _ = EndPaint(window, &paint);
    }
}
