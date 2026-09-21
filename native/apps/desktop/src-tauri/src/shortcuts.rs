use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
#[cfg(target_os = "linux")]
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

/// Liga os atalhos, trocando os de antes.
///
/// Atalho de sistema, e não `keydown` na janela, porque mutar o microfone só serve se
/// funcionar com o jogo na frente — e aí o app não recebe tecla nenhuma.
///
/// Uma tecla recusada (outro programa já a tomou, ou o sistema não conhece o nome) não
/// derruba as outras: cada uma é tentada por si, e a resposta diz quais valeram.
///
/// No Windows nenhuma ação é registrada no sistema, todas vão para a vigia de `polling`: o
/// registro de lá (`RegisterHotKey`) engole a tecla — a bind apertada sem querer no meio
/// do jogo morria antes de chegar nele — e não conhece botão de mouse.
#[tauri::command]
pub fn set_shortcuts(app: AppHandle, bindings: Vec<Binding>) -> Result<Registered, String> {
    let bindings = bindings.into_iter().filter(|binding| !binding.accelerator.trim().is_empty());
    let mut result = Registered::default();

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let mut watched = Vec::new();

        for binding in bindings {
            match polling::parse(&binding.accelerator) {
                Some(keys) => {
                    result.registered.push(binding.action.clone());
                    watched.push((binding.action, keys));
                }
                None => {
                    tracing::warn!(accelerator = %binding.accelerator, "atalho: tecla sem virtual-key conhecida");
                    result.failed.push(binding.action);
                }
            }
        }

        polling::watch(&app, watched)?;
    }

    #[cfg(target_os = "linux")]
    {
        let manager = app.global_shortcut();

        manager.unregister_all().map_err(|error| error.to_string())?;

        for binding in bindings {
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
    }

    Ok(result)
}

/// Os atalhos no Windows: em vez de registrar no sistema, uma thread pergunta se as teclas
/// estão para baixo (`GetAsyncKeyState`). Perguntar não consome nada: o jogo continua
/// recebendo a tecla, e botão de mouse é tecla como outra qualquer.
///
/// ponytail: com o jogo rodando como administrador o Windows esconde o estado das teclas de
/// um processo sem elevação, e os atalhos param de responder enquanto o jogo está na
/// frente (os apps de chamada têm o mesmo limite). A saída é rodar o app elevado.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
mod polling {
    #[cfg(not(target_os = "macos"))]
    const SHIFT: &[u16] = &[0x10];
    #[cfg(not(target_os = "macos"))]
    const CONTROL: &[u16] = &[0x11];
    #[cfg(not(target_os = "macos"))]
    const ALT: &[u16] = &[0x12];

    /// A tecla Windows não tem código que valha pelas duas, como os outros têm.
    #[cfg(not(target_os = "macos"))]
    const SUPER: &[u16] = &[0x5B, 0x5C];

    /// O que tem de estar para baixo, em virtual-keys do Windows. Cada modificador é uma
    /// lista de alternativas: a da esquerda ou a da direita.
    #[derive(Debug, PartialEq)]
    pub struct Keys {
        key: u16,
        modifiers: Vec<&'static [u16]>,
    }

    /// `Control+Shift+KeyV`, `KeyV`, `Mouse4`: modificadores, e a tecla por último. `None`
    /// para o que não tem virtual-key conhecida — a ação vai em `failed`, e se for a de
    /// falar a interface deixa o microfone aberto.
    #[cfg(target_os = "macos")]
    pub fn parse(accelerator: &str) -> Option<Keys> {
        let split = core_app::keymap::split(accelerator)?;
        let key = core_app::keymap::macos_key(split.key)?;
        let modifiers = split
            .modifiers
            .into_iter()
            .map(core_app::keymap::macos_modifier)
            .collect::<Option<_>>()?;

        Some(Keys { key, modifiers })
    }

    #[cfg(not(target_os = "macos"))]
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
    /// americano) que o plugin usa no macOS e no Linux: a tecla que vale é a que tem
    /// aquela letra, igual nos três sistemas.
    #[cfg(not(target_os = "macos"))]
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
    /// não impede: a pessoa segura Shift para correr no jogo e fala, ou muta, ao mesmo tempo.
    pub fn held(keys: &Keys, down: impl Fn(u16) -> bool) -> bool {
        down(keys.key)
            && keys.modifiers.iter().all(|alternatives| alternatives.iter().any(|&key| down(key)))
    }

    /// Uma volta da vigia: confere cada ação com a leitura de agora, guarda em `pressed` e
    /// devolve só o que mudou. A interface quer a borda: tecla segurada não pode virar uma
    /// fila de "mutar" a cada 20 ms.
    pub fn edges<'watched>(
        watched: &'watched [(String, Keys)],
        pressed: &mut [bool],
        down: impl Fn(u16) -> bool,
    ) -> Vec<(&'watched str, bool)> {
        let mut changed = Vec::new();

        for ((action, keys), pressed) in watched.iter().zip(pressed) {
            let now = held(keys, &down);

            if now != *pressed {
                *pressed = now;
                changed.push((action.as_str(), now));
            }
        }

        changed
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub use watcher::watch;

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    mod watcher {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Mutex, PoisonError};
        use std::thread::JoinHandle;
        use std::time::Duration;

        use tauri::AppHandle;
        #[cfg(target_os = "windows")]
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

        use super::super::fire;
        use super::{Keys, edges};

        /// Vinte milissegundos: o atraso que a voz ganha na descida da tecla, e 50 voltas
        /// por segundo perguntando por três atalhos não custam nada.
        const POLL: Duration = Duration::from_millis(20);

        /// Uma thread só, para todas as ações, e só enquanto há atalho para vigiar. Ninguém
        /// a espera ao fechar o app: ela não segura nada, e morre com o processo.
        static WATCH: Mutex<Option<(Arc<AtomicBool>, JoinHandle<()>)>> = Mutex::new(None);

        fn stop() {
            let watch = WATCH.lock().unwrap_or_else(PoisonError::into_inner).take();

            if let Some((stopping, thread)) = watch {
                stopping.store(true, Ordering::Relaxed);

                // Esperar custa até uma volta, e garante que a vigia velha não solta um
                // evento atrasado por cima da nova.
                if thread.join().is_err() {
                    tracing::warn!("atalho: a vigia das teclas morreu em pânico");
                }
            }
        }

        /// Troca a vigia de antes por esta lista de `(ação, teclas)`; lista vazia só para.
        pub fn watch(app: &AppHandle, watched: Vec<(String, Keys)>) -> Result<(), String> {
            stop();

            if watched.is_empty() {
                return Ok(());
            }

            let stopping = Arc::new(AtomicBool::new(false));
            let stopped = Arc::clone(&stopping);
            let app = app.clone();

            let thread = std::thread::Builder::new()
                .name("unkvoid-shortcut-keys".into())
                .spawn(move || {
                    // O bit alto é "para baixo agora", e num `i16` ele é o sinal.
                    #[cfg(target_os = "windows")]
                    let down = |key: u16| unsafe { GetAsyncKeyState(i32::from(key)) } < 0;

                    // Lê o estado da tecla no HID, sem interceptar nada: o jogo na frente
                    // continua recebendo a tecla. É a diferença entre ler e registrar.
                    //
                    // Declarada à mão porque a crate `core-graphics` não expõe esta: ela
                    // cobre criar e mandar eventos, não consultar o teclado.
                    #[cfg(target_os = "macos")]
                    let down = |key: u16| {
                        /// 1 é `kCGEventSourceStateHIDSystemState`: o teclado de verdade,
                        /// e não os eventos que algum programa injetou.
                        const HID_SYSTEM_STATE: u32 = 1;

                        unsafe extern "C" {
                            fn CGEventSourceKeyState(state: u32, key: u16) -> bool;
                        }

                        unsafe { CGEventSourceKeyState(HID_SYSTEM_STATE, key) }
                    };
                    let mut pressed = vec![false; watched.len()];

                    // O que já está para baixo quando a vigia começa não é aperto: a interface
                    // grava a tecla na descida, e sem esta leitura jogada fora escolher a tecla
                    // de mutar já mutava, com o dedo ainda nela.
                    edges(&watched, &mut pressed, down);

                    while !stopped.load(Ordering::Relaxed) {
                        for (action, now) in edges(&watched, &mut pressed, down) {
                            fire(&app, action, now);
                        }

                        std::thread::sleep(POLL);
                    }

                    // Trocar de tecla com ela apertada não pode deixar o microfone aberto:
                    // uma leitura de "nada para baixo" solta o que estava apertado.
                    for (action, now) in edges(&watched, &mut pressed, |_| false) {
                        fire(&app, action, now);
                    }
                })
                .map_err(|error| error.to_string())?;

            *WATCH.lock().unwrap_or_else(PoisonError::into_inner) = Some((stopping, thread));

            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{Keys, edges, held};
        #[cfg(not(target_os = "macos"))]
        use super::parse;

        /// As teclas montadas à mão, e não pelo `parse`: `edges` e `held` não sabem de
        /// tabela de sistema nenhuma, e amarrá-los à do Windows faria o teste falhar no
        /// macOS por um motivo que não é o que ele verifica.
        fn watching(bindings: &[(&str, u16, &[&'static [u16]])]) -> Vec<(String, Keys)> {
            bindings
                .iter()
                .map(|&(action, key, modifiers)| {
                    (action.to_owned(), Keys { key, modifiers: modifiers.to_vec() })
                })
                .collect()
        }

        fn with(down: &'static [u16]) -> impl Fn(u16) -> bool {
            move |key| down.contains(&key)
        }

        #[cfg(not(target_os = "macos"))]
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
            const CONTROL_KEYS: &[u16] = &[0x11];
            const SUPER_KEYS: &[u16] = &[0x5B, 0x5C];

            let keys = Keys { key: 0x05, modifiers: vec![CONTROL_KEYS, SUPER_KEYS] };

            assert!(held(&keys, with(&[0x05, 0x11, 0x5B])));
            assert!(held(&keys, with(&[0x05, 0x11, 0x5C])), "a tecla Windows da direita também vale");
            assert!(held(&keys, with(&[0x05, 0x11, 0x5C, 0x10])), "Shift a mais não impede");

            assert!(!held(&keys, with(&[0x05, 0x11])), "faltou a tecla Windows");
            assert!(!held(&keys, with(&[0x11, 0x5B])), "faltou o botão");
            assert!(!held(&keys, with(&[])));

            let bare = Keys { key: 0x56, modifiers: vec![] };

            assert!(held(&bare, with(&[0x56, 0x10, 0x11])), "tecla solta com o jogo segurando Shift e Ctrl");
        }

        #[test]
        fn each_action_fires_on_its_own_edges_and_a_held_key_does_not_repeat() {
            const CONTROL_KEYS: &[u16] = &[0x11];
            const SHIFT_KEYS: &[u16] = &[0x10];

            let watched = watching(&[("mute", 0x4D, &[CONTROL_KEYS, SHIFT_KEYS]), ("talk", 0x05, &[])]);
            let mut pressed = vec![false; watched.len()];

            assert!(edges(&watched, &mut pressed, with(&[])).is_empty(), "nada apertado, nada a dizer");

            assert_eq!(edges(&watched, &mut pressed, with(&[0x05])), [("talk", true)]);
            assert!(edges(&watched, &mut pressed, with(&[0x05])).is_empty(), "tecla segurada não repete");

            assert_eq!(
                edges(&watched, &mut pressed, with(&[0x05, 0x11, 0x10, 0x4D])),
                [("mute", true)],
                "mutar no meio da fala não mexe na tecla de falar",
            );
            assert!(edges(&watched, &mut pressed, with(&[0x05, 0x11, 0x10, 0x4D])).is_empty());

            assert_eq!(edges(&watched, &mut pressed, with(&[0x05, 0x11, 0x10])), [("mute", false)]);
            assert_eq!(edges(&watched, &mut pressed, with(&[])), [("talk", false)]);
        }

        /// É com esta leitura que a vigia para: o que estava apertado sobe, o resto fica quieto.
        #[test]
        fn a_reading_with_nothing_down_releases_only_what_was_pressed() {
            const CONTROL_KEYS: &[u16] = &[0x11];

            let watched = watching(&[
                ("mute", 0x4D, &[CONTROL_KEYS]),
                ("deafen", 0x44, &[CONTROL_KEYS]),
                ("talk", 0x56, &[]),
            ]);
            let mut pressed = vec![false; watched.len()];

            assert_eq!(edges(&watched, &mut pressed, with(&[0x11, 0x4D, 0x56])), [("mute", true), ("talk", true)]);
            assert_eq!(edges(&watched, &mut pressed, |_| false), [("mute", false), ("talk", false)]);
            assert!(edges(&watched, &mut pressed, |_| false).is_empty());
        }

        /// O `RegisterHotKey` casava o conjunto exato de modificadores; a consulta não, porque
        /// modificador a mais não impede. Então um atalho contido em outro dispara junto com
        /// ele: `Control+Shift+KeyM` muta **e** ensurdece. Quem não quer isso escolhe teclas
        /// que não se contêm.
        #[test]
        fn a_binding_contained_in_another_fires_along_with_it() {
            const CONTROL_KEYS: &[u16] = &[0x11];
            const SHIFT_KEYS: &[u16] = &[0x10];

            let watched = watching(&[
                ("mute", 0x4D, &[CONTROL_KEYS]),
                ("deafen", 0x4D, &[CONTROL_KEYS, SHIFT_KEYS]),
            ]);
            let mut pressed = vec![false; watched.len()];

            assert_eq!(edges(&watched, &mut pressed, with(&[0x11, 0x4D])), [("mute", true)], "o menor sozinho");
            assert_eq!(
                edges(&watched, &mut pressed, with(&[0x11, 0x10, 0x4D])),
                [("deafen", true)],
                "o Shift chega depois: o mutar já estava apertado e não repete",
            );
            assert_eq!(edges(&watched, &mut pressed, with(&[])), [("mute", false), ("deafen", false)]);

            assert_eq!(
                edges(&watched, &mut pressed, with(&[0x11, 0x10, 0x4D])),
                [("mute", true), ("deafen", true)],
                "o maior de uma vez só aperta os dois",
            );
        }

        /// A tabela é número escrito à mão; aqui ela é conferida com os nomes da Microsoft.
        #[cfg(target_os = "windows")]
        #[cfg(not(target_os = "macos"))]
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
