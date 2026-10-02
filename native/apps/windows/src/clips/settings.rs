//! O que a pessoa escolheu nos Clips, guardado em `%APPDATA%\com.unkvoid.desktop\clips.json`,
//! ao lado do resto do Unkvoid. Na primeira vez vem do `settings.json` do UnkvoidClips, que era
//! um app separado.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use clips_engine::audio::{AudioSettings, MicrophoneSettings};
use clips_engine::capture::VideoSettings;
use clips_engine::recorder::RecorderSettings;

use clips_engine::hotkeys::Hotkey;
use super::shell;

pub const REPLAY_MINUTES: [u32; 4] = [5, 10, 20, 30];
pub const FRAME_RATES: [u32; 2] = [30, 60];

/// A taxa de cada qualidade em 1080p60. Outra resolução ou outro fps escala a partir daqui:
/// o que se quer manter é a quantidade de bits por pixel, não o número.
pub const QUALITY_BITRATES: [u32; 3] = [20_000_000, 35_000_000, 50_000_000];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Desligado até a pessoa ligar pelo Alt+Z ou pela janela, como no GeForce Experience.
    pub replay_enabled: bool,
    pub clips_folder: PathBuf,
    pub replay_minutes: u32,
    pub quality: usize,
    pub frame_rate: u32,
    pub monitor: Option<usize>,
    pub system_audio: bool,
    pub microphone: bool,
    pub microphone_device: Option<String>,
    pub noise_suppression: bool,
    pub save_hotkey: Hotkey,
    pub overlay_hotkey: Hotkey,
    pub start_with_windows: bool,
    /// A pessoa já ligou o replay alguma vez. Aí os atalhos ficam registrados mesmo com ele
    /// desligado, para o Alt+Z religar; quem nunca usou os Clips não perde tecla nenhuma.
    pub clips_user: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            replay_enabled: false,
            // A pasta escolhida no instalador; sem instalador, a de vídeos do Windows.
            clips_folder: shell::installer_clips_folder()
                .unwrap_or_else(|| shell::videos_folder().join("UnkvoidClips")),
            replay_minutes: 5,
            quality: 1,
            frame_rate: 60,
            monitor: None,
            system_audio: true,
            // Ligado de fábrica, com o filtro de ruído: a voz de quem joga é metade do clipe.
            // Só grava com o replay ligado, que a pessoa liga de propósito.
            microphone: true,
            microphone_device: None,
            noise_suppression: true,
            save_hotkey: Hotkey::SAVE_DEFAULT,
            overlay_hotkey: Hotkey::OVERLAY_DEFAULT,
            start_with_windows: true,
            clips_user: false,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let parse = |bytes: Vec<u8>| serde_json::from_slice::<Self>(&bytes).ok();

        if let Some(settings) = std::fs::read(path()).ok().and_then(parse) {
            return settings;
        }

        // Quem vem do UnkvoidClips já usa os Clips: os atalhos seguem registrados como lá.
        std::fs::read(shell::legacy_settings())
            .ok()
            .and_then(parse)
            .map(|settings| Self { clips_user: true, ..settings })
            .unwrap_or_default()
    }

    pub fn hotkeys_active(&self) -> bool {
        self.replay_enabled || self.clips_user
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = path();

        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }

        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;

        Ok(())
    }

    pub fn recorder(&self) -> RecorderSettings {
        RecorderSettings {
            enabled: self.replay_enabled,
            video: VideoSettings { monitor: self.monitor, frame_rate: self.frame_rate, bitrate: self.bitrate() },
            audio: AudioSettings {
                system: self.system_audio,
                microphone: self.microphone.then(|| MicrophoneSettings {
                    device: self.microphone_device.clone(),
                    noise_suppression: self.noise_suppression,
                }),
            },
            replay: Duration::from_secs(u64::from(self.replay_minutes) * 60),
        }
    }

    pub fn bitrate(&self) -> u32 {
        let (width, height) = clips_engine::capture::monitor_size(self.monitor).unwrap_or((1920, 1080));
        let base = QUALITY_BITRATES[self.quality.min(QUALITY_BITRATES.len() - 1)] as f64;
        let scale = f64::from(width * height) / (1920.0 * 1080.0) * f64::from(self.frame_rate) / 60.0;

        (base * scale).clamp(8e6, 150e6) as u32
    }

    /// Quanto o buffer ocupa em disco no pior caso: taxa de pico do encoder durante todo o
    /// replay, mais o som.
    pub fn buffer_estimate(&self) -> String {
        let bits = (f64::from(self.bitrate()) * 1.5 + 192_000.0) * f64::from(self.replay_minutes) * 60.0;
        let gigabytes = format!("{:.1}", bits / 8e9).replace('.', ",");

        format!(
            "O replay de {} min ocupa até {gigabytes} GB no disco, e o que passa do tempo é apagado sozinho.",
            self.replay_minutes
        )
    }
}

fn path() -> PathBuf {
    shell::roaming_folder().join("clips.json")
}
