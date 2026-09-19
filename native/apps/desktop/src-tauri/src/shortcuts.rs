use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// Uma tecla que a pessoa escolheu e o que ela faz.
#[derive(Debug, Deserialize)]
pub struct Binding {
    pub action: String,
    pub accelerator: String,
}

/// O que a interface recebe quando a tecla vai ou volta. `pressed` existe por causa do
/// falar-apertando: a mesma tecla abre o microfone na descida e fecha na subida.
#[derive(Clone, Serialize)]
struct Fired {
    action: String,
    pressed: bool,
}

/// O que registrou de verdade. A interface precisa saber uma por uma: se o
/// falar-apertando não pegou a tecla, ela tem de deixar o microfone aberto em vez de
/// esperar para sempre por uma tecla que não chega.
#[derive(Default, Serialize)]
pub struct Registered {
    registered: Vec<String>,
    failed: Vec<String>,
}

fn fire(app: &AppHandle, action: &str, pressed: bool) {
    if let Err(error) = app.emit("shortcut", Fired { action: action.to_owned(), pressed }) {
        tracing::warn!(%error, "atalho: a interface não recebeu a tecla");
    }
}

/// Registra os atalhos no sistema, trocando os de antes.
///
/// Atalho de sistema, e não `keydown` na janela, porque mutar o microfone só serve se
/// funcionar com o jogo na frente — e aí o app não recebe tecla nenhuma.
///
/// Uma tecla recusada (outro programa já a tomou, ou o sistema não conhece o nome) não
/// derruba as outras: cada uma é tentada por si, e a resposta diz quais valeram.
///
/// No Windows o falar-apertando não passa por aqui, e sim por `talk`: o registro de
/// atalho de lá (`RegisterHotKey`) engole a tecla — com tecla solta para falar, ela morria
/// no jogo — e não conhece botão de mouse.
#[tauri::command]
pub fn set_shortcuts(app: AppHandle, bindings: Vec<Binding>) -> Result<Registered, String> {
    let manager = app.global_shortcut();

    manager.unregister_all().map_err(|error| error.to_string())?;

    #[cfg(target_os = "windows")]
    talk::stop();

    let mut result = Registered::default();

    for binding in bindings {
        if binding.accelerator.trim().is_empty() {
            continue;
        }

        #[cfg(target_os = "windows")]
        if binding.action == talk::ACTION {
            match talk::watch(&app, &binding.accelerator) {
                Ok(()) => result.registered.push(binding.action),
                Err(error) => {
                    tracing::warn!(%error, accelerator = %binding.accelerator, "atalho: a tecla de falar não pôde ser vigiada");
                    result.failed.push(binding.action);
                }
            }

            continue;
        }

        let action = binding.action.clone();
        let handle = app.clone();

        match manager.on_shortcut(binding.accelerator.as_str(), move |_app, _shortcut, event| {
            fire(&handle, &action, event.state() == ShortcutState::Pressed);
        }) {
            Ok(()) => result.registered.push(binding.action),
            Err(error) => {
                tracing::warn!(%error, accelerator = %binding.accelerator, "atalho recusado pelo sistema");
                result.failed.push(binding.action);
            }
        }
    }

    Ok(result)
}

/// A tecla de falar no Windows: em vez de registrar atalho, uma thread pergunta ao sistema
/// se a tecla está para baixo (`GetAsyncKeyState`). Perguntar não consome nada: o jogo
/// continua recebendo a tecla, e botão de mouse é tecla como outra qualquer.
///
/// ponytail: com o jogo rodando como administrador o Windows esconde o estado das teclas de
/// um processo sem elevação, e a tecla de falar para de responder enquanto o jogo está na
/// frente (o Discord tem o mesmo limite). A saída é rodar o app elevado.
#[cfg(any(target_os = "windows", test))]
mod talk {
    #[cfg(target_os = "windows")]
    pub const ACTION: &str = "talk";

    const SHIFT: &[u16] = &[0x10];
    const CONTROL: &[u16] = &[0x11];
    const ALT: &[u16] = &[0x12];

    /// A tecla Windows não tem código que valha pelas duas, como os outros têm.
    const SUPER: &[u16] = &[0x5B, 0x5C];

    /// O que tem de estar para baixo, em virtual-keys do Windows. Cada modificador é uma
    /// lista de alternativas: a da esquerda ou a da direita.
    #[derive(Debug, PartialEq)]
    pub struct Keys {
        key: u16,
        modifiers: Vec<&'static [u16]>,
    }

    /// `Control+Shift+KeyV`, `KeyV`, `Mouse4`: modificadores, e a tecla por último. `None`
    /// para o que não tem virtual-key conhecida — a ação vai em `failed`, e a interface
    /// deixa o microfone aberto.
    pub fn parse(accelerator: &str) -> Option<Keys> {
        let mut parts: Vec<&str> = accelerator.split('+').map(str::trim).collect();
        let key = virtual_key(parts.pop()?)?;

        let modifiers = parts
            .into_iter()
            .map(|modifier| match modifier {
                "Super" => Some(SUPER),
                "Control" | "CmdOrCtrl" => Some(CONTROL),
                "Alt" => Some(ALT),
                "Shift" => Some(SHIFT),
                _ => None,
            })
            .collect::<Option<_>>()?;

        Some(Keys { key, modifiers })
    }

    /// O `KeyboardEvent.code` que a interface manda, na mesma tabela (a do teclado
    /// americano) que o plugin usa para as outras ações: a tecla que vale é a que tem
    /// aquela letra, como no mutar e no ensurdecer.
    fn virtual_key(code: &str) -> Option<u16> {
        let numbered = |prefix: &str| code.strip_prefix(prefix)?.parse::<u16>().ok();

        if let Some(&[letter @ b'A'..=b'Z']) = code.strip_prefix("Key").map(str::as_bytes) {
            return Some(u16::from(letter));
        }

        if let Some(&[digit @ b'0'..=b'9']) = code.strip_prefix("Digit").map(str::as_bytes) {
            return Some(u16::from(digit));
        }

        if let Some(digit @ 0..=9) = numbered("Numpad") {
            return Some(0x60 + digit);
        }

        if let Some(number @ 1..=24) = numbered("F") {
            return Some(0x70 + number - 1);
        }

        Some(match code {
            "Mouse3" => 0x04,
            "Mouse4" => 0x05,
            "Mouse5" => 0x06,
            "NumpadEnter" => 0x0D,
            "Space" => 0x20,
            "PageUp" => 0x21,
            "PageDown" => 0x22,
            "End" => 0x23,
            "Home" => 0x24,
            "ArrowLeft" => 0x25,
            "ArrowUp" => 0x26,
            "ArrowRight" => 0x27,
            "ArrowDown" => 0x28,
            "Insert" => 0x2D,
            "NumpadMultiply" => 0x6A,
            "NumpadAdd" => 0x6B,
            "NumpadSubtract" => 0x6D,
            "NumpadDecimal" => 0x6E,
            "NumpadDivide" => 0x6F,
            "Semicolon" => 0xBA,
            "Equal" => 0xBB,
            "Comma" => 0xBC,
            "Minus" => 0xBD,
            "Period" => 0xBE,
            "Slash" => 0xBF,
            "Backquote" => 0xC0,
            "BracketLeft" => 0xDB,
            "Backslash" => 0xDC,
            "BracketRight" => 0xDD,
            "Quote" => 0xDE,
            _ => return None,
        })
    }

    /// Apertado é a tecla e todos os modificadores pedidos para baixo. Modificador a mais
    /// não impede: a pessoa segura Shift para correr no jogo e fala ao mesmo tempo.
    pub fn held(keys: &Keys, down: impl Fn(u16) -> bool) -> bool {
        down(keys.key)
            && keys.modifiers.iter().all(|alternatives| alternatives.iter().any(|&key| down(key)))
    }

    #[cfg(target_os = "windows")]
    pub use watcher::{stop, watch};

    #[cfg(target_os = "windows")]
    mod watcher {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Mutex, PoisonError};
        use std::thread::JoinHandle;
        use std::time::Duration;

        use tauri::AppHandle;
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

        use super::{ACTION, held, parse};

        /// Vinte milissegundos: o atraso que a voz ganha na descida da tecla, e 50
        /// perguntas por segundo de meia dúzia de teclas não custam nada.
        const POLL: Duration = Duration::from_millis(20);

        /// A thread só existe enquanto há tecla de falar para vigiar. Ninguém a espera ao
        /// fechar o app: ela não segura nada, e morre com o processo.
        static WATCH: Mutex<Option<(Arc<AtomicBool>, JoinHandle<()>)>> = Mutex::new(None);

        pub fn stop() {
            let watch = WATCH.lock().unwrap_or_else(PoisonError::into_inner).take();

            if let Some((stopping, thread)) = watch {
                stopping.store(true, Ordering::Relaxed);

                // Esperar custa até uma volta, e garante que a vigia velha não solta um
                // evento atrasado por cima da nova.
                if thread.join().is_err() {
                    tracing::warn!("atalho: a vigia da tecla de falar morreu em pânico");
                }
            }
        }

        pub fn watch(app: &AppHandle, accelerator: &str) -> Result<(), String> {
            let keys = parse(accelerator).ok_or("tecla sem virtual-key conhecida")?;

            stop();

            let stopping = Arc::new(AtomicBool::new(false));
            let stopped = Arc::clone(&stopping);
            let app = app.clone();

            let thread = std::thread::Builder::new()
                .name("unkvoid-talk-key".into())
                .spawn(move || {
                    let mut pressed = false;

                    while !stopped.load(Ordering::Relaxed) {
                        // O bit alto é "para baixo agora", e num `i16` ele é o sinal.
                        let now = held(&keys, |key| unsafe { GetAsyncKeyState(i32::from(key)) } < 0);

                        if now != pressed {
                            pressed = now;
                            super::super::fire(&app, ACTION, pressed);
                        }

                        std::thread::sleep(POLL);
                    }

                    // Trocar de tecla com ela apertada não pode deixar o microfone aberto.
                    if pressed {
                        super::super::fire(&app, ACTION, false);
                    }
                })
                .map_err(|error| error.to_string())?;

            *WATCH.lock().unwrap_or_else(PoisonError::into_inner) = Some((stopping, thread));

            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{Keys, held, parse};

        #[test]
        fn the_accelerator_becomes_the_keys_to_watch() {
            let bare = |key| Some(Keys { key, modifiers: vec![] });

            assert_eq!(parse("KeyV"), bare(0x56));
            assert_eq!(parse("Digit7"), bare(0x37));
            assert_eq!(parse("F13"), bare(0x7C));
            assert_eq!(parse("Numpad0"), bare(0x60));
            assert_eq!(parse("Space"), bare(0x20));
            assert_eq!(parse("Backquote"), bare(0xC0));
            assert_eq!(parse("Mouse3"), bare(0x04));
            assert_eq!(parse("Mouse4"), bare(0x05));
            assert_eq!(parse("Mouse5"), bare(0x06));

            assert_eq!(
                parse("Super+Control+Alt+Shift+KeyV"),
                Some(Keys { key: 0x56, modifiers: vec![super::SUPER, super::CONTROL, super::ALT, super::SHIFT] }),
            );
            assert_eq!(parse("CmdOrCtrl+Mouse4"), Some(Keys { key: 0x05, modifiers: vec![super::CONTROL] }));

            // O que o `USABLE` da interface deixa passar e não tem virtual-key, e o que nem
            // é atalho: tudo isso vai para `failed`.
            for refused in ["", "Shift+", "Shift", "F0", "F25", "NumpadEqual", "Numpad10", "KeyÇ", "Keyv", "Mouse2", "Hyper+KeyV"] {
                assert_eq!(parse(refused), None, "{refused} não devia ter virado tecla");
            }
        }

        #[test]
        fn held_needs_the_key_and_every_asked_modifier_and_ignores_the_extra_ones() {
            let keys = parse("Control+Super+Mouse4").expect("atalho válido");
            let with = |down: &'static [u16]| move |key: u16| down.contains(&key);

            assert!(held(&keys, with(&[0x05, 0x11, 0x5B])));
            assert!(held(&keys, with(&[0x05, 0x11, 0x5C])), "a tecla Windows da direita também vale");
            assert!(held(&keys, with(&[0x05, 0x11, 0x5C, 0x10])), "Shift a mais não impede");

            assert!(!held(&keys, with(&[0x05, 0x11])), "faltou a tecla Windows");
            assert!(!held(&keys, with(&[0x11, 0x5B])), "faltou o botão");
            assert!(!held(&keys, with(&[])));

            let bare = parse("KeyV").expect("atalho válido");

            assert!(held(&bare, with(&[0x56, 0x10, 0x11])), "tecla solta com o jogo segurando Shift e Ctrl");
        }

        /// A tabela é número escrito à mão; aqui ela é conferida com os nomes da Microsoft.
        #[cfg(target_os = "windows")]
        #[test]
        fn the_table_matches_the_names_windows_gives() {
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                VK_ADD, VK_CONTROL, VK_DECIMAL, VK_DIVIDE, VK_DOWN, VK_END, VK_F1, VK_F24, VK_HOME, VK_INSERT, VK_LEFT,
                VK_LWIN, VK_MBUTTON, VK_MENU, VK_MULTIPLY, VK_NEXT, VK_NUMPAD0, VK_NUMPAD9, VK_OEM_1, VK_OEM_2,
                VK_OEM_3, VK_OEM_4, VK_OEM_5, VK_OEM_6, VK_OEM_7, VK_OEM_COMMA, VK_OEM_MINUS, VK_OEM_PERIOD,
                VK_OEM_PLUS, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_RWIN, VK_SHIFT, VK_SPACE, VK_SUBTRACT, VK_UP,
                VK_XBUTTON1, VK_XBUTTON2,
            };

            let named = [
                ("Mouse3", VK_MBUTTON), ("Mouse4", VK_XBUTTON1), ("Mouse5", VK_XBUTTON2), ("NumpadEnter", VK_RETURN),
                ("Space", VK_SPACE), ("PageUp", VK_PRIOR), ("PageDown", VK_NEXT), ("End", VK_END), ("Home", VK_HOME),
                ("ArrowLeft", VK_LEFT), ("ArrowUp", VK_UP), ("ArrowRight", VK_RIGHT), ("ArrowDown", VK_DOWN),
                ("Insert", VK_INSERT), ("Numpad0", VK_NUMPAD0), ("Numpad9", VK_NUMPAD9), ("NumpadMultiply", VK_MULTIPLY),
                ("NumpadAdd", VK_ADD), ("NumpadSubtract", VK_SUBTRACT), ("NumpadDecimal", VK_DECIMAL),
                ("NumpadDivide", VK_DIVIDE), ("F1", VK_F1), ("F24", VK_F24), ("Semicolon", VK_OEM_1),
                ("Equal", VK_OEM_PLUS), ("Comma", VK_OEM_COMMA), ("Minus", VK_OEM_MINUS), ("Period", VK_OEM_PERIOD),
                ("Slash", VK_OEM_2), ("Backquote", VK_OEM_3), ("BracketLeft", VK_OEM_4), ("Backslash", VK_OEM_5),
                ("BracketRight", VK_OEM_6), ("Quote", VK_OEM_7),
            ];

            for (code, name) in named {
                assert_eq!(parse(code), Some(Keys { key: name.0, modifiers: vec![] }), "{code}");
            }

            assert_eq!(super::SHIFT, [VK_SHIFT.0]);
            assert_eq!(super::CONTROL, [VK_CONTROL.0]);
            assert_eq!(super::ALT, [VK_MENU.0]);
            assert_eq!(super::SUPER, [VK_LWIN.0, VK_RWIN.0]);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use tauri_plugin_global_shortcut::Shortcut;

    /// O que a interface escreve tem de ser o que o sistema entende: a tecla vem do
    /// `KeyboardEvent.code` do webview, e é esse nome que o plugin espera.
    #[test]
    fn the_keys_the_interface_writes_are_the_ones_the_system_understands() {
        for accelerator in ["CmdOrCtrl+Shift+KeyM", "Control+Alt+KeyD", "F13", "Alt+Space", "KeyV"] {
            assert!(Shortcut::from_str(accelerator).is_ok(), "o sistema recusou {accelerator}");
        }

        assert!(Shortcut::from_str("").is_err(), "vazio não é atalho");
        assert!(Shortcut::from_str("Shift+").is_err(), "modificador sozinho não é atalho");
    }
}
