//! O ícone na bandeja. O X esconde a janela em vez de fechar o app (fechado, o replay pararia de
//! gravar), e é por aqui que a janela volta, que se salva o replay sem abrir nada e que se sai
//! de verdade.
//!
//! Win32 puro numa thread só dela: o eframe não tem bandeja, e uma dependência a mais só para um
//! ícone e três itens de menu não se paga.

use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, HICON, IDI_APPLICATION, LoadIconW, MF_SEPARATOR,
    MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SendMessageW, SetForegroundWindow, TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_DESTROY,
    WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

const CLASS: PCWSTR = w!("unkvoid-tray");
/// O aviso do ícone (clique, menu) e o pedido de trocar a dica, vindo da thread da janela.
const ICON_MESSAGE: u32 = WM_APP + 1;
const TIP_MESSAGE: u32 = WM_APP + 2;
/// O ícone do `.exe`: o `tauri-winres` grava o `set_icon` com este número.
const ICON_RESOURCE: usize = 32512;

const OPEN: usize = 1;
const SAVE: usize = 2;
const QUIT: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    Open,
    Save,
    Quit,
}

type Handler = Box<dyn Fn(TrayEvent)>;

thread_local! {
    static HANDLER: RefCell<Option<Handler>> = const { RefCell::new(None) };
    static TIP: RefCell<Vec<u16>> = const { RefCell::new(Vec::new()) };
    static TASKBAR_CREATED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

pub struct Tray {
    window: isize,
    tip: String,
}

impl Tray {
    pub fn start(tip: &str, on_event: impl Fn(TrayEvent) + Send + 'static) -> anyhow::Result<Self> {
        let (ready, created) = std::sync::mpsc::sync_channel(1);
        let first = tip.to_owned();

        std::thread::Builder::new().name("tray".into()).spawn(move || {
            HANDLER.set(Some(Box::new(on_event)));
            TIP.set(first.encode_utf16().collect());

            // SAFETY: a janela e o ícone vivem nesta thread, até o `WM_DESTROY`.
            let window = unsafe { create() };
            let failed = window.is_err();
            let _ = ready.send(window.map(|window| window.0 as isize));

            if failed {
                return;
            }

            let mut message = MSG::default();

            // SAFETY: o laço de mensagens da thread que criou a janela.
            unsafe {
                while GetMessageW(&mut message, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        })?;

        let window = created.recv()??;

        Ok(Self { window, tip: tip.to_owned() })
    }

    /// A dica do ícone: o que o replay está fazendo agora. Chamada a cada quadro, só vai para a
    /// bandeja quando muda.
    pub fn set_tip(&mut self, tip: &str) {
        if self.tip == tip {
            return;
        }

        tip.clone_into(&mut self.tip);

        let text: Box<Vec<u16>> = Box::new(tip.encode_utf16().collect());

        // SAFETY: o texto vai para a thread da bandeja, que o devolve ao `Box` e o solta.
        unsafe {
            if PostMessageW(Some(self.hwnd()), TIP_MESSAGE, WPARAM(0), LPARAM(Box::into_raw(text) as isize)).is_err() {
                tracing::warn!("bandeja: a dica nova não chegou");
            }
        }
    }

    fn hwnd(&self) -> HWND {
        HWND(self.window as *mut std::ffi::c_void)
    }
}

impl Drop for Tray {
    /// O ícone sai da bandeja antes do processo acabar: sem o `NIM_DELETE` ele fica lá até
    /// alguém passar o mouse por cima. `SendMessage` espera a thread da bandeja tirá-lo.
    fn drop(&mut self) {
        // SAFETY: a janela é da thread da bandeja, que ainda roda o laço.
        unsafe {
            SendMessageW(self.hwnd(), WM_CLOSE, None, None);
        }
    }
}

unsafe fn create() -> anyhow::Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;

        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            lpszClassName: CLASS,
            ..Default::default()
        });
        // O Explorer que reinicia (ou cai) esquece os ícones: ele avisa por esta mensagem, e o
        // ícone volta.
        TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));

        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
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
        )?;

        add_icon(window);

        Ok(window)
    }
}

unsafe fn icon_data(window: HWND) -> NOTIFYICONDATAW {
    unsafe {
        let instance = GetModuleHandleW(None).ok();
        let icon: HICON = LoadIconW(instance.map(Into::into), PCWSTR(ICON_RESOURCE as *const u16))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default();
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: ICON_MESSAGE,
            hIcon: icon,
            ..Default::default()
        };

        TIP.with_borrow(|tip| {
            let length = tip.len().min(data.szTip.len() - 1);

            data.szTip[..length].copy_from_slice(&tip[..length]);
        });

        data
    }
}

unsafe fn add_icon(window: HWND) {
    unsafe {
        if !Shell_NotifyIconW(NIM_ADD, &icon_data(window)).as_bool() {
            tracing::warn!("bandeja: o ícone não entrou");
        }
    }
}

fn tell(event: TrayEvent) {
    HANDLER.with_borrow(|handler| {
        if let Some(handler) = handler {
            handler(event);
        }
    });
}

unsafe fn menu(window: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };

        let _ = AppendMenuW(menu, MF_STRING, OPEN, w!("Abrir o Unkvoid"));
        let _ = AppendMenuW(menu, MF_STRING, SAVE, w!("Salvar replay"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, QUIT, w!("Sair do Unkvoid"));

        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);

        // Sem a janela na frente o menu não fecha ao clicar fora dele (o aviso da documentação do
        // `TrackPopupMenu`), e o `WM_NULL` depois é o outro lado do mesmo remendo.
        let _ = SetForegroundWindow(window);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            cursor.x,
            cursor.y,
            None,
            window,
            None,
        );
        let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);

        match usize::try_from(chosen.0).unwrap_or_default() {
            OPEN => tell(TrayEvent::Open),
            SAVE => tell(TrayEvent::Save),
            QUIT => tell(TrayEvent::Quit),
            _ => {}
        }
    }
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match message {
            ICON_MESSAGE => {
                match u32::try_from(lparam.0 & 0xFFFF).unwrap_or_default() {
                    WM_LBUTTONUP | WM_LBUTTONDBLCLK => tell(TrayEvent::Open),
                    WM_RBUTTONUP => menu(window),
                    _ => {}
                }

                LRESULT(0)
            }
            TIP_MESSAGE => {
                let text = Box::from_raw(lparam.0 as *mut Vec<u16>);

                TIP.set(*text);
                let _ = Shell_NotifyIconW(NIM_MODIFY, &icon_data(window));

                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(window);

                LRESULT(0)
            }
            WM_DESTROY => {
                let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(window));
                PostQuitMessage(0);

                LRESULT(0)
            }
            _ if message == TASKBAR_CREATED.get() && message != 0 => {
                add_icon(window);

                LRESULT(0)
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}
