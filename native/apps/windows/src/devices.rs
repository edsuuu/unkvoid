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

#[cfg(not(target_os = "windows"))]
pub fn microphones() -> Vec<Device> {
    Vec::new()
}

#[cfg(not(target_os = "windows"))]
pub fn speakers() -> Vec<Device> {
    Vec::new()
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
