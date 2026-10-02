//! Atalhos globais pelo `RegisterHotKey` do Windows.
//!
//! O Windows entrega a combinação só para este app, e o jogo não a recebe: o Alt+Z abre o
//! painel sem o jogo agir junto, como no GeForce Experience. É também o que funciona com jogo
//! rodando como administrador (o anti-cheat do Arena Breakout pede isso): ler o estado do
//! teclado, como o unkvoid faz nos atalhos dele, não enxerga tecla nenhuma enquanto uma janela
//! elevada está em foco.
//!
//! Se outro programa já registrou a mesma combinação, o registro falha. Aquele atalho cai
//! então para a leitura do estado do teclado a cada 20 ms — funciona, mas sem segurar a tecla —
//! e a interface avisa.

use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::WPARAM;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyNameTextW, HOT_KEY_MODIFIERS, MAPVK_VK_TO_VSC, MOD_ALT, MOD_CONTROL,
    MOD_NOREPEAT, MOD_SHIFT, MapVirtualKeyW, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    MSG, MsgWaitForMultipleObjects, PM_NOREMOVE, PM_REMOVE, PeekMessageW, PostThreadMessageW,
    QS_ALLINPUT, WM_APP, WM_HOTKEY,
};

const CONTROL: i32 = 0x11;
const ALT: i32 = 0x12;
const SHIFT: i32 = 0x10;
const ESCAPE: u16 = 0x1B;

const OVERLAY_ID: i32 = 1;
const SAVE_ID: i32 = 2;

/// Pedidos da interface para a thread dos atalhos: o registro é por thread, e só quem
/// registrou pode desfazer. O `wParam` diz de qual atalho é o pedido.
const SET_MESSAGE: u32 = WM_APP + 1;
const CAPTURE_MESSAGE: u32 = WM_APP + 2;
/// Liga (`wParam` 1) ou solta (0) os dois atalhos de uma vez.
const ACTIVE_MESSAGE: u32 = WM_APP + 3;

/// Os dois atalhos, e os dois a pessoa escolhe: abrir o painel e salvar o replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Overlay,
    Save,
}

impl Slot {
    fn id(self) -> i32 {
        match self {
            Self::Overlay => OVERLAY_ID,
            Self::Save => SAVE_ID,
        }
    }

    fn from_id(id: i32) -> Option<Self> {
        match id {
            OVERLAY_ID => Some(Self::Overlay),
            SAVE_ID => Some(Self::Save),
            _ => None,
        }
    }

    fn event(self) -> HotkeyEvent {
        match self {
            Self::Overlay => HotkeyEvent::Overlay,
            Self::Save => HotkeyEvent::Save,
        }
    }
}

/// A combinação de cada atalho, lida pela thread que os registra.
#[derive(Clone, Copy)]
struct Bindings {
    overlay: Hotkey,
    save: Hotkey,
}

impl Bindings {
    fn get(&self, slot: Slot) -> Hotkey {
        match slot {
            Slot::Overlay => self.overlay,
            Slot::Save => self.save,
        }
    }

    fn set(&mut self, slot: Slot, hotkey: Hotkey) {
        match slot {
            Slot::Overlay => self.overlay = hotkey,
            Slot::Save => self.save = hotkey,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hotkey {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    /// O código de tecla virtual do Windows (`VK_*`).
    pub key: u16,
}

impl Hotkey {
    /// Alt+F10, o mesmo do ShadowPlay: quem vem do GeForce Experience já tem na mão.
    pub const SAVE_DEFAULT: Self = Self { control: false, alt: true, shift: false, key: 0x79 };
    /// Alt+Z, o do GeForce Experience.
    pub const OVERLAY_DEFAULT: Self = Self { control: false, alt: true, shift: false, key: 0x5A };

    pub fn label(&self) -> String {
        let mut parts = Vec::new();

        if self.control {
            parts.push("Ctrl".to_owned());
        }

        if self.alt {
            parts.push("Alt".to_owned());
        }

        if self.shift {
            parts.push("Shift".to_owned());
        }

        parts.push(key_name(self.key));
        parts.join(" + ")
    }

    fn modifiers(&self) -> HOT_KEY_MODIFIERS {
        let mut modifiers = MOD_NOREPEAT;

        if self.control {
            modifiers |= MOD_CONTROL;
        }

        if self.alt {
            modifiers |= MOD_ALT;
        }

        if self.shift {
            modifiers |= MOD_SHIFT;
        }

        modifiers
    }

    fn is_down(&self, modifiers: (bool, bool, bool)) -> bool {
        (self.control, self.alt, self.shift) == modifiers && is_down(i32::from(self.key))
    }
}

pub enum HotkeyEvent {
    Save,
    Overlay,
    Captured(Slot, Hotkey),
    CaptureCancelled,
    /// Outro programa já tem esta combinação: ela funciona, mas o jogo também a recebe.
    Taken(Hotkey),
}

pub struct Hotkeys {
    thread: u32,
    bindings: Arc<Mutex<Bindings>>,
}

impl Hotkeys {
    /// Com `active` falso a thread sobe sem registrar nada: as teclas seguem do jogo até o
    /// `set_active(true)`.
    pub fn start(overlay: Hotkey, save: Hotkey, active: bool, on_event: impl Fn(HotkeyEvent) + Send + 'static) -> anyhow::Result<Self> {
        let (ready, thread) = sync_channel(1);
        let shared = Arc::new(Mutex::new(Bindings { overlay, save }));
        let watched = shared.clone();

        std::thread::Builder::new().name("UnkvoidClips-hotkeys".into()).spawn(move || {
            let mut message = MSG::default();

            // Cria a fila de mensagens da thread antes de avisar quem espera: pedido postado
            // para uma thread ainda sem fila se perde.
            unsafe {
                let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
            }

            let _ = ready.send(unsafe { GetCurrentThreadId() });
            Watcher::new(watched, active, on_event).run();
        })?;

        Ok(Self { thread: thread.recv()?, bindings: shared })
    }

    pub fn set(&self, slot: Slot, hotkey: Hotkey) {
        self.bindings.lock().unwrap_or_else(PoisonError::into_inner).set(slot, hotkey);
        self.post(SET_MESSAGE, slot.id() as usize);
    }

    /// A próxima combinação apertada vira o atalho escolhido; Esc desiste.
    pub fn capture_next(&self, slot: Slot) {
        self.post(CAPTURE_MESSAGE, slot.id() as usize);
    }

    /// Registra os dois atalhos, ou os devolve ao Windows: soltos, as teclas voltam a ir para o
    /// jogo e para quem mais as usa (o Alt+Z da NVIDIA).
    pub fn set_active(&self, active: bool) {
        self.post(ACTIVE_MESSAGE, usize::from(active));
    }

    fn post(&self, message: u32, parameter: usize) {
        if let Err(error) = unsafe { PostThreadMessageW(self.thread, message, WPARAM(parameter), Default::default()) } {
            tracing::warn!(error = %error, "atalhos: a thread não recebeu o pedido");
        }
    }
}

struct Watcher<F: Fn(HotkeyEvent)> {
    bindings: Arc<Mutex<Bindings>>,
    on_event: F,
    /// Os atalhos cujo registro falhou e são lidos pelo estado do teclado, com o estado do
    /// último giro para disparar só na descida da tecla.
    fallbacks: Vec<(i32, Hotkey, bool)>,
    capturing: Option<Slot>,
    pressed: [bool; 256],
    /// Soltos, os atalhos não são registrados: a escolha de uma combinação nova fica guardada
    /// para quando ligarem.
    active: bool,
}

impl<F: Fn(HotkeyEvent)> Watcher<F> {
    fn new(bindings: Arc<Mutex<Bindings>>, active: bool, on_event: F) -> Self {
        Self { bindings, on_event, fallbacks: Vec::new(), capturing: None, pressed: [false; 256], active }
    }

    fn run(mut self) {
        self.register(Slot::Overlay);
        self.register(Slot::Save);

        loop {
            // Acorda com mensagem ou a cada 20 ms: o giro serve aos atalhos que caíram para a
            // leitura do teclado e à captura de um atalho novo.
            unsafe {
                MsgWaitForMultipleObjects(None, false, 20, QS_ALLINPUT);
            }

            let mut message = MSG::default();

            while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                if message.message == ACTIVE_MESSAGE {
                    self.set_active(message.wParam.0 != 0);

                    continue;
                }

                let slot = Slot::from_id(message.wParam.0 as i32);

                match (message.message, slot) {
                    (WM_HOTKEY, Some(slot)) => (self.on_event)(slot.event()),
                    (SET_MESSAGE, Some(slot)) => {
                        self.unregister(slot);
                        self.register(slot);
                    }
                    (CAPTURE_MESSAGE, Some(slot)) => {
                        // Solto enquanto a pessoa escolhe: senão apertar a combinação atual
                        // dispararia o atalho em vez de ser lida como a escolha.
                        self.unregister(slot);
                        self.capturing = Some(slot);
                        self.pressed = std::array::from_fn(|key| is_down(key as i32));
                    }
                    _ => {}
                }
            }

            if let Some(slot) = self.capturing {
                self.capture(slot);
            } else {
                self.poll_fallbacks();
            }
        }
    }

    fn set_active(&mut self, active: bool) {
        if active == self.active {
            return;
        }

        if active {
            self.active = true;
            self.register(Slot::Overlay);
            self.register(Slot::Save);
        } else {
            self.unregister(Slot::Overlay);
            self.unregister(Slot::Save);
            self.active = false;
        }
    }

    fn register(&mut self, slot: Slot) {
        if !self.active {
            return;
        }

        let id = slot.id();
        let hotkey = self.bindings.lock().unwrap_or_else(PoisonError::into_inner).get(slot);

        self.fallbacks.retain(|(fallback, _, _)| *fallback != id);

        if let Err(error) = unsafe { RegisterHotKey(None, id, hotkey.modifiers(), u32::from(hotkey.key)) } {
            tracing::warn!(error = %error, hotkey = hotkey.label(), "atalhos: outro programa já usa a combinação");
            self.fallbacks.push((id, hotkey, false));
            (self.on_event)(HotkeyEvent::Taken(hotkey));
        }
    }

    fn unregister(&mut self, slot: Slot) {
        let id = slot.id();

        self.fallbacks.retain(|(fallback, _, _)| *fallback != id);

        unsafe {
            let _ = UnregisterHotKey(None, id);
        }
    }

    fn poll_fallbacks(&mut self) {
        if self.fallbacks.is_empty() {
            return;
        }

        let modifiers = (is_down(CONTROL), is_down(ALT), is_down(SHIFT));
        let mut fired = Vec::new();

        for (id, hotkey, was_down) in &mut self.fallbacks {
            let down = hotkey.is_down(modifiers);

            if down && !*was_down {
                fired.push(*id);
            }

            *was_down = down;
        }

        for slot in fired.into_iter().filter_map(Slot::from_id) {
            (self.on_event)(slot.event());
        }
    }

    /// Só vale tecla que desceu agora: o clique em "Alterar" e o que já estava apertado não
    /// viram atalho.
    fn capture(&mut self, slot: Slot) {
        let modifiers = (is_down(CONTROL), is_down(ALT), is_down(SHIFT));
        let mut captured = None;

        for key in 0x08_u16..=0xFE {
            let down = is_down(i32::from(key));

            if down && !self.pressed[key as usize] && !is_modifier(key) {
                captured = Some(key);
            }

            self.pressed[key as usize] = down;
        }

        let Some(key) = captured else { return };

        self.capturing = None;

        if key == ESCAPE {
            self.register(slot);
            (self.on_event)(HotkeyEvent::CaptureCancelled);
        } else {
            // A interface grava a escolha e chama `set`, que registra a combinação nova.
            (self.on_event)(HotkeyEvent::Captured(slot, Hotkey { control: modifiers.0, alt: modifiers.1, shift: modifiers.2, key }));
        }
    }
}

fn is_down(key: i32) -> bool {
    let state = unsafe { GetAsyncKeyState(key) };

    state as u16 & 0x8000 != 0
}

/// Modificadores, botões do mouse e as teclas do Windows não são atalho sozinhos.
fn is_modifier(key: u16) -> bool {
    matches!(key, 0x01..=0x06 | 0x10..=0x12 | 0x5B | 0x5C | 0xA0..=0xA5)
}

/// O nome que o teclado da pessoa dá à tecla ("F10", "Z", "Page Down").
fn key_name(key: u16) -> String {
    // Setas, Insert, Delete, Home, End e Page Up/Down moram no bloco estendido: sem o bit 24
    // o Windows responde com o nome da tecla do teclado numérico.
    let extended = matches!(key, 0x21..=0x2E);
    let scan = unsafe { MapVirtualKeyW(u32::from(key), MAPVK_VK_TO_VSC) };
    let mut buffer = [0_u16; 64];
    let parameter = ((scan << 16) | if extended { 1 << 24 } else { 0 }) as i32;
    let length = unsafe { GetKeyNameTextW(parameter, &mut buffer) };

    if length > 0 {
        String::from_utf16_lossy(&buffer[..length as usize])
    } else {
        format!("Tecla {key}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_save_hotkey_reads_like_shadowplay() {
        assert_eq!(Hotkey::SAVE_DEFAULT.label(), "Alt + F10");
        assert_eq!(Hotkey::OVERLAY_DEFAULT.label(), "Alt + Z");
    }

    #[test]
    fn registered_modifiers_never_repeat() {
        assert_eq!(Hotkey::OVERLAY_DEFAULT.modifiers(), MOD_ALT | MOD_NOREPEAT);
    }
}
