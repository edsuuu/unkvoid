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

/// Registra os atalhos no sistema, trocando os de antes.
///
/// Atalho de sistema, e não `keydown` na janela, porque mutar o microfone só serve se
/// funcionar com o jogo na frente — e aí o app não recebe tecla nenhuma.
///
/// Uma tecla recusada (outro programa já a tomou, ou o sistema não conhece o nome) não
/// derruba as outras: cada uma é tentada por si, e a resposta diz quais valeram.
#[tauri::command]
pub fn set_shortcuts(app: AppHandle, bindings: Vec<Binding>) -> Result<Registered, String> {
    let manager = app.global_shortcut();

    manager.unregister_all().map_err(|error| error.to_string())?;

    let mut result = Registered::default();

    for binding in bindings {
        if binding.accelerator.trim().is_empty() {
            continue;
        }

        let action = binding.action.clone();
        let handle = app.clone();

        match manager.on_shortcut(binding.accelerator.as_str(), move |_app, _shortcut, event| {
            let fired = Fired {
                action: action.clone(),
                pressed: event.state() == ShortcutState::Pressed,
            };

            if let Err(error) = handle.emit("shortcut", fired) {
                tracing::warn!(%error, "atalho: a interface não recebeu a tecla");
            }
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
