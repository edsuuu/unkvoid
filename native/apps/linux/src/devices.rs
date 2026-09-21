//! Que microfone escutar e por onde sair o som.
//!
//! Quem captura e quem toca são `gst-launch` filhos com `pulsesrc`/`pulsesink` sem
//! `device=`: os dois seguem o padrão do PulseAudio. Então escolher aqui é mudar esse
//! padrão, pelo `pactl`, que é a mesma ferramenta que o sistema usa.
//!
//! ponytail: o padrão é do sistema inteiro, não só deste app — trocar o microfone aqui
//! troca o de quem mais estiver gravando. A saída é `device=` nos dois pipelines do
//! `shared/capture`, que aí passam a receber o nome escolhido.

use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// O nome que o PulseAudio entende. Nunca aparece na tela.
    pub name: String,
    /// O nome que a pessoa lê.
    pub label: String,
}

/// Os microfones. O monitor de uma saída também é uma "source" para o PulseAudio, e
/// oferecê-lo como microfone faria a pessoa transmitir o próprio alto-falante de volta.
pub fn microphones() -> Vec<Device> {
    listed("sources").into_iter().filter(|device| !device.name.ends_with(".monitor")).collect()
}

pub fn speakers() -> Vec<Device> {
    listed("sinks")
}

pub fn current_microphone() -> Option<String> {
    default_of("get-default-source")
}

pub fn current_speaker() -> Option<String> {
    default_of("get-default-sink")
}

pub fn use_microphone(name: &str) -> bool {
    pactl(&["set-default-source", name]).is_some()
}

pub fn use_speaker(name: &str) -> bool {
    pactl(&["set-default-sink", name]).is_some()
}

/// O `pactl list` sai em blocos; o que interessa é o par `Name`/`Description` de cada um.
/// A descrição é o que a pessoa reconhece ("Webcam C920"), e o nome é o que o PulseAudio
/// aceita de volta.
fn listed(kind: &str) -> Vec<Device> {
    pactl(&["list", kind]).as_deref().map(pairs).unwrap_or_default()
}

fn pairs(output: &str) -> Vec<Device> {
    let mut devices = Vec::new();
    let mut name: Option<String> = None;

    for line in output.lines() {
        let line = line.trim();

        if let Some(found) = line.strip_prefix("Name: ") {
            name = Some(found.to_owned());
        } else if let Some(description) = line.strip_prefix("Description: ")
            && let Some(name) = name.take()
        {
            devices.push(Device { name, label: description.to_owned() });
        }
    }

    devices
}

fn default_of(action: &str) -> Option<String> {
    pactl(&[action]).map(|name| name.trim().to_owned()).filter(|name| !name.is_empty())
}

/// Sem PulseAudio não há lista, e a interface mostra que não há. Não é falha: é uma
/// máquina onde a escolha não existe.
fn pactl(arguments: &[&str]) -> Option<String> {
    let output = Command::new("pactl").args(arguments).output().ok()?;

    if !output.status.success() {
        tracing::warn!(?arguments, "o pactl recusou");

        return None;
    }

    String::from_utf8(output.stdout).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_reads_the_pairs_and_never_shows_the_internal_name() {
        let devices = pairs(
            "Source #1\n\tState: SUSPENDED\n\tName: alsa_output.pci-0000.analog-stereo.monitor\n\t\
             Description: Monitor of Alto-falantes\n\nSource #2\n\tName: alsa_input.usb-C920\n\t\
             Description: Webcam C920 Analógico Estéreo\n",
        );

        assert_eq!(devices.len(), 2);
        assert_eq!(devices[1].label, "Webcam C920 Analógico Estéreo");

        let microphones: Vec<&Device> =
            devices.iter().filter(|device| !device.name.ends_with(".monitor")).collect();

        assert_eq!(microphones.len(), 1);
        assert_eq!(microphones[0].name, "alsa_input.usb-C920");
    }
}
