//! Que microfone escutar e por onde sair o som.
//!
//! A lista é a do sistema, lida pelo WASAPI na hora em que o popover abre. Escolher aqui
//! não mexe no padrão do Windows: guarda a escolha para a captura usar.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// O identificador do endpoint. Nunca aparece na tela.
    pub id: String,
    /// O nome que a pessoa lê.
    pub label: String,
    /// É o padrão do sistema hoje.
    pub default: bool,
}

#[cfg(target_os = "windows")]
pub use win::{microphones, speakers};

#[cfg(target_os = "linux")]
pub use linux::{microphones, speakers, use_microphone};

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn microphones() -> Vec<Device> {
    Vec::new()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn speakers() -> Vec<Device> {
    Vec::new()
}

/// No Linux a lista vem do `pactl`, que fala com o PulseAudio e com o `pipewire-pulse` do
/// mesmo jeito. O `id` é o nome que o PulseAudio aceita de volta: `--device=` no `pacat`
/// da saída, e o padrão do sistema no caso do microfone (`sound.rs`).
#[cfg(target_os = "linux")]
mod linux {
    use std::process::Command;

    use super::Device;

    /// O monitor de uma saída também é uma "source" para o PulseAudio, e oferecê-lo como
    /// microfone faria a pessoa transmitir o próprio alto-falante de volta.
    pub fn microphones() -> Vec<Device> {
        listed("sources", "get-default-source").into_iter().filter(|device| !device.id.ends_with(".monitor")).collect()
    }

    pub fn speakers() -> Vec<Device> {
        listed("sinks", "get-default-sink")
    }

    /// O `pulsesrc` do `shared/capture` lê sempre `@DEFAULT_SOURCE@`: escolher o microfone é
    /// trocar o padrão do sistema.
    ///
    /// ponytail: o padrão é de todo app que grava, não só deste. A saída é `device=` no
    /// pipeline do microfone do `shared/capture`, que aí passaria a receber o nome escolhido.
    pub fn use_microphone(name: &str) -> bool {
        pactl(&["set-default-source", name]).is_some()
    }

    fn listed(kind: &str, default: &str) -> Vec<Device> {
        let standard = pactl(&[default]).map(|name| name.trim().to_owned());

        pactl(&["list", kind]).as_deref().map(|output| pairs(output, standard.as_deref())).unwrap_or_default()
    }

    /// O `pactl list` sai em blocos; o que interessa é o par `Name`/`Description` de cada um.
    /// A descrição é o que a pessoa reconhece ("Webcam C920"), e o nome é o que o PulseAudio
    /// aceita de volta.
    fn pairs(output: &str, standard: Option<&str>) -> Vec<Device> {
        let mut devices = Vec::new();
        let mut name: Option<String> = None;

        for line in output.lines() {
            let line = line.trim();

            if let Some(found) = line.strip_prefix("Name: ") {
                name = Some(found.to_owned());
            } else if let Some(description) = line.strip_prefix("Description: ")
                && let Some(id) = name.take()
            {
                devices.push(Device { default: Some(id.as_str()) == standard, id, label: description.to_owned() });
            }
        }

        devices
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
        fn the_list_reads_the_pairs_marks_the_default_and_hides_the_monitor() {
            let devices = pairs(
                "Source #1\n\tState: SUSPENDED\n\tName: alsa_output.pci-0000.analog-stereo.monitor\n\t\
                 Description: Monitor of Alto-falantes\n\nSource #2\n\tName: alsa_input.usb-C920\n\t\
                 Description: Webcam C920 Analógico Estéreo\n",
                Some("alsa_input.usb-C920"),
            );

            assert_eq!(devices.len(), 2);
            assert_eq!(devices[1].label, "Webcam C920 Analógico Estéreo");
            assert!(devices[1].default && !devices[0].default);

            let microphones: Vec<&Device> = devices.iter().filter(|device| !device.id.ends_with(".monitor")).collect();

            assert_eq!(microphones.len(), 1);
            assert_eq!(microphones[0].id, "alsa_input.usb-C920");
        }
    }
}

#[cfg(target_os = "windows")]
mod win {
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        DEVICE_STATE_ACTIVE, EDataFlow, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
        eCapture, eConsole, eRender,
    };
    use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PropVariantClear};
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, STGM_READ,
    };
    use windows::Win32::System::Variant::VT_LPWSTR;

    use super::Device;

    pub fn microphones() -> Vec<Device> {
        unsafe { listed(eCapture) }
    }

    pub fn speakers() -> Vec<Device> {
        unsafe { listed(eRender) }
    }

    /// Sem WASAPI não há lista, e a interface mostra que não há. Não é falha: é uma máquina
    /// onde a escolha não existe.
    unsafe fn listed(flow: EDataFlow) -> Vec<Device> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let opened: windows::core::Result<IMMDeviceEnumerator> =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL);

            let Ok(enumerator) = opened else {
                tracing::warn!("o enumerador de áudio do Windows não abriu");

                return Vec::new();
            };

            let standard = enumerator
                .GetDefaultAudioEndpoint(flow, eConsole)
                .ok()
                .and_then(|device| identifier(&device));

            let Ok(endpoints) = enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) else {
                return Vec::new();
            };

            let mut devices = Vec::new();

            for index in 0..endpoints.GetCount().unwrap_or(0) {
                let Ok(endpoint) = endpoints.Item(index) else {
                    continue;
                };

                let (Some(id), Some(label)) = (identifier(&endpoint), friendly_name(&endpoint))
                else {
                    continue;
                };

                devices.push(Device { default: standard.as_ref() == Some(&id), id, label });
            }

            devices
        }
    }

    unsafe fn identifier(device: &IMMDevice) -> Option<String> {
        unsafe { device.GetId().ok().and_then(|id| id.to_string().ok()) }
    }

    /// O nome que a pessoa reconhece ("Microfone (Webcam C920)"). O `PROPVARIANT` volta
    /// alocado pelo COM e é liberado aqui: ler a lista a cada abertura do popover vazaria
    /// uma string por aparelho, por abertura.
    unsafe fn friendly_name(device: &IMMDevice) -> Option<String> {
        unsafe {
            let store = device.OpenPropertyStore(STGM_READ).ok()?;
            let mut value: PROPVARIANT = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
            let fields = &value.Anonymous.Anonymous;

            let name = if fields.vt == VT_LPWSTR {
                fields.Anonymous.pwszVal.to_string().ok()
            } else {
                None
            };

            let _ = PropVariantClear(&mut value);

            name
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    /// Contra o WASAPI de verdade: mostra o que esta máquina tem. Roda com
    /// `cargo test -p unkvoid-windows -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn the_system_lists_its_devices() {
        for device in super::microphones() {
            println!("microfone: {} ({}) padrão={}", device.label, device.id, device.default);
        }

        for device in super::speakers() {
            println!("saída: {} ({}) padrão={}", device.label, device.id, device.default);
        }
    }
}
