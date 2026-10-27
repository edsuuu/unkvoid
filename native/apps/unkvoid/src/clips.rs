//! Os Clips: o replay instantâneo do Windows. A tela fica sempre gravando num buffer em disco, e
//! só vira arquivo quando a pessoa aperta o atalho. O motor mora em `shared/clips`; aqui ficam as
//! preferências, os atalhos, o vigia que religa a captura, o aviso por cima do jogo e o que o
//! painel do Alt+Z (`overlay.rs`) pede.
//!
//! Tudo isto roda com a janela escondida na bandeja, que é o normal durante o jogo: o `poll` é
//! chamado pelo `logic` do eframe, que continua batendo sem a janela na tela.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use clips_engine::audio::{AudioSettings, Microphone, MicrophoneSettings};
use clips_engine::capture::{MonitorInfo, VideoSettings};
use clips_engine::gallery;
use clips_engine::hotkeys::{Hotkey, HotkeyEvent, Hotkeys, Slot};
use clips_engine::recorder::{Recorder, RecorderSettings};
use serde::{Deserialize, Serialize};

pub const REPLAY_MINUTES: [u32; 4] = [5, 10, 20, 30];
pub const FRAME_RATES: [u32; 2] = [30, 60];

/// A taxa de cada qualidade em 1080p60. Outra resolução ou outro fps escala a partir daqui: o que
/// se quer manter é a quantidade de bits por pixel, não o número.
const QUALITY_BITRATES: [u32; 3] = [20_000_000, 35_000_000, 50_000_000];

/// De quanto em quanto tempo o vigia confere se a captura ainda grava.
const WATCH_EVERY: Duration = Duration::from_secs(2);

/// Os nomes dos campos são os do `clips.json` do app de antes: quem atualiza leva o microfone, o
/// monitor e os atalhos que já tinha escolhido.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipsSettings {
    #[serde(rename = "replay_enabled", alias = "enabled")]
    pub enabled: bool,
    #[serde(rename = "clips_folder", alias = "folder")]
    pub folder: PathBuf,
    pub replay_minutes: u32,
    pub quality: usize,
    pub frame_rate: u32,
    /// `None` segue o monitor principal do Windows.
    pub monitor: Option<usize>,
    pub system_audio: bool,
    pub microphone: bool,
    pub microphone_device: Option<String>,
    pub noise_suppression: bool,
    pub save_hotkey: Hotkey,
    pub overlay_hotkey: Hotkey,
    pub save_chime: bool,
    pub start_with_windows: bool,
    /// A pessoa já ligou o replay alguma vez. Aí os atalhos ficam registrados mesmo com ele
    /// desligado, para o Alt+Z religar; quem nunca usou os Clips não perde tecla nenhuma.
    pub clips_user: bool,
}

impl Default for ClipsSettings {
    fn default() -> Self {
        Self {
            // Desligado até a pessoa ligar, como no GeForce Experience.
            enabled: false,
            folder: default_folder(),
            replay_minutes: 5,
            quality: 1,
            frame_rate: 60,
            monitor: None,
            system_audio: true,
            // Ligado de fábrica, com o filtro de ruído: a voz de quem joga é metade do clipe. Só
            // grava com o replay ligado, que a pessoa liga de propósito.
            microphone: true,
            microphone_device: None,
            noise_suppression: true,
            save_hotkey: Hotkey::SAVE_DEFAULT,
            overlay_hotkey: Hotkey::OVERLAY_DEFAULT,
            save_chime: true,
            start_with_windows: true,
            clips_user: false,
        }
    }
}

impl ClipsSettings {
    pub fn load() -> Self {
        let parse = |path: PathBuf| {
            std::fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
        };

        if let Some(settings) = parse(settings_path()) {
            return settings;
        }

        // O arquivo do UnkvoidClips, o app separado de antes: quem vem dele já usa os Clips. O
        // beta roda ao lado do app instalado, e herdar o replay ligado punha dois encoders na
        // placa.
        (!crate::BETA)
            .then(legacy_settings_path)
            .and_then(parse)
            .map(|settings| Self { clips_user: true, ..settings })
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = settings_path();
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("pasta das preferências inválida"))?;

        std::fs::create_dir_all(parent)?;
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;

        Ok(())
    }

    pub fn hotkeys_active(&self) -> bool {
        self.enabled || self.clips_user
    }

    pub fn replay(&self) -> Duration {
        Duration::from_secs(bounded_replay_minutes(self.replay_minutes) * 60)
    }

    fn recorder(&self) -> RecorderSettings {
        RecorderSettings {
            enabled: self.enabled,
            video: VideoSettings {
                monitor: self.monitor,
                frame_rate: self.frame_rate,
                bitrate: self.bitrate(),
            },
            audio: AudioSettings {
                system: self.system_audio,
                microphone: self.microphone.then(|| MicrophoneSettings {
                    device: self.microphone_device.clone(),
                    noise_suppression: self.noise_suppression,
                }),
            },
            replay: self.replay(),
        }
    }

    pub fn bitrate(&self) -> u32 {
        let (width, height) =
            clips_engine::capture::monitor_size(self.monitor).unwrap_or((1920, 1080));

        bitrate_for(self.quality, self.frame_rate, width, height)
    }

    /// Quanto o buffer ocupa em disco no pior caso: a taxa de pico do encoder durante o replay
    /// inteiro, mais o som.
    pub fn buffer_estimate(&self) -> String {
        let bits = (f64::from(self.bitrate()) * 1.5 + 192_000.0)
            * f64::from(self.replay_minutes)
            * 60.0;
        let gigabytes = format!("{:.1}", bits / 8e9).replace('.', ",");

        format!(
            "O replay de {} min ocupa até {gigabytes} GB no disco, e o que passa do tempo é apagado sozinho.",
            self.replay_minutes
        )
    }
}

fn bitrate_for(quality: usize, frame_rate: u32, width: u32, height: u32) -> u32 {
    let base = f64::from(QUALITY_BITRATES[quality.min(QUALITY_BITRATES.len() - 1)]);
    let scale = f64::from(width) * f64::from(height) / (1920.0 * 1080.0) * f64::from(frame_rate)
        / 60.0;

    (base * scale).clamp(8_000_000.0, 150_000_000.0) as u32
}

fn bounded_replay_minutes(minutes: u32) -> u64 {
    u64::from(minutes.clamp(5, 30))
}

fn settings_path() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default())
        .join(storage::APP_DIR)
        .join("clips.json")
}

fn legacy_settings_path() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default())
        .join("UnkvoidClips")
        .join("settings.json")
}

fn default_folder() -> PathBuf {
    crate::shell::installer_clips_folder()
        .unwrap_or_else(|| crate::shell::videos_folder().join("UnkvoidClips"))
}

pub fn rename_clip(path: &std::path::Path, title: &str) -> anyhow::Result<PathBuf> {
    let Some(safe) = clip_file_title(title) else {
        anyhow::bail!("O nome do clipe não pode ficar vazio.");
    };

    let renamed = path.with_file_name(format!("{safe}.mp4"));

    // O `rename` do Windows substitui o arquivo que já tem o nome: o outro clipe se perderia.
    // Só a mesma letra em outra caixa ("raid" → "Raid") é o próprio clipe.
    if renamed.exists() && !renamed.as_os_str().eq_ignore_ascii_case(path.as_os_str()) {
        anyhow::bail!("Já existe um clipe com esse nome.");
    }

    std::fs::rename(path, &renamed).map_err(|error| {
        tracing::warn!(%error, "clips: o clipe não foi renomeado");
        anyhow::anyhow!("Ele pode estar aberto em outro programa.")
    })?;

    Ok(renamed)
}

fn clip_file_title(title: &str) -> Option<String> {
    let safe: String = title
        .chars()
        .filter(|character| {
            !character.is_control()
                && !matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        })
        .take(80)
        .collect();
    let safe = safe.trim().trim_end_matches('.');

    (!safe.is_empty()).then(|| safe.to_owned())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// O que o vigia viu por último: se o replay grava, e o que a pessoa precisa saber se não.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Health {
    pub recording: bool,
    pub problem: Option<String>,
}

/// Um monitor do seletor, com a miniatura do que ele mostra agora.
pub struct Screen {
    pub info: MonitorInfo,
    pub label: String,
    pub detail: String,
    pub preview: Option<egui::TextureHandle>,
}

enum RuntimeEvent {
    Save,
    Overlay,
    Captured(Slot, Hotkey),
    CaptureCancelled,
    Taken(Hotkey),
    Failed(String),
    Saved(PathBuf),
    FolderSelected(Option<PathBuf>),
    Thumbnail(PathBuf, Option<(u32, u32, Vec<u8>)>),
}

pub struct ClipsRuntime {
    pub settings: ClipsSettings,
    context: egui::Context,
    last_saved: Option<PathBuf>,
    hotkey_error: Option<String>,
    /// Qual atalho está esperando a próxima combinação, para o botão dizer "Aperte as teclas…".
    pub capturing: Option<Slot>,
    /// O gravador é dividido com o vigia, que o religa quando a captura cai sozinha.
    recorder: Option<Arc<Mutex<Recorder>>>,
    health: Arc<Mutex<Health>>,
    saving: Arc<AtomicBool>,
    hotkeys: Option<Hotkeys>,
    events: std::sync::mpsc::Receiver<RuntimeEvent>,
    sender: std::sync::mpsc::SyncSender<RuntimeEvent>,
    player: Option<crate::clips_player::Player>,
    player_path: Option<PathBuf>,
    /// O volume do player, de 0 a 100. Começa baixo, como no app de antes: um clipe de jogo sai
    /// no volume do jogo. Vale enquanto o app está aberto.
    player_volume: u8,
    thumbnails: HashMap<PathBuf, egui::TextureHandle>,
    thumbnail_pending: HashSet<PathBuf>,
    pub screens: Vec<Screen>,
    pub microphones: Vec<Microphone>,
    /// O painel do Alt+Z aberto por cima do jogo.
    overlay: Option<crate::overlay::OverlayState>,
    /// A Galeria do painel pediu a página dos Clips: a janela a abre no mesmo quadro.
    open_gallery: bool,
    /// O seletor de pasta na tela, por cima do painel: o clique no escuro atrás dele não fecha o
    /// painel por baixo do diálogo.
    folder_picker_open: bool,
}

impl ClipsRuntime {
    pub fn new(context: &egui::Context) -> Self {
        let settings = ClipsSettings::load();
        let (sender, events) = std::sync::mpsc::sync_channel(16);
        let mut runtime = Self {
            settings,
            context: context.clone(),
            last_saved: None,
            hotkey_error: None,
            capturing: None,
            recorder: None,
            health: Arc::default(),
            saving: Arc::default(),
            hotkeys: None,
            events,
            sender,
            player: None,
            player_path: None,
            player_volume: 10,
            thumbnails: HashMap::new(),
            thumbnail_pending: HashSet::new(),
            screens: Vec::new(),
            microphones: Vec::new(),
            overlay: None,
            open_gallery: false,
            folder_picker_open: false,
        };

        runtime.configure();
        // A tarefa do logon é regravada a cada abertura, como no app de antes: é ela que abre o
        // app elevado e na bandeja. Fora da thread da janela, porque fala com o Agendador.
        let autostart = runtime.settings.start_with_windows;
        std::thread::spawn(move || crate::shell::set_autostart(autostart));

        runtime
    }

    pub fn update(&mut self, mut settings: ClipsSettings) {
        if settings.enabled {
            settings.clips_user = true;
        }

        if settings.start_with_windows != self.settings.start_with_windows {
            let autostart = settings.start_with_windows;
            std::thread::spawn(move || crate::shell::set_autostart(autostart));
        }

        self.settings = settings;

        if let Err(error) = self.settings.save() {
            tracing::warn!(error = %error, "clips: as preferências não foram gravadas");
        }

        self.configure();
    }

    fn configure(&mut self) {
        let recorder_settings = self.settings.recorder();

        match &self.recorder {
            Some(recorder) => lock(recorder).apply(recorder_settings),
            None => match Recorder::start(
                crate::shell::local_folder().join("clips-buffer"),
                recorder_settings,
            ) {
                Ok(recorder) => {
                    let recorder = Arc::new(Mutex::new(recorder));

                    watch(&recorder, self.health.clone(), self.context.clone());
                    self.recorder = Some(recorder);
                }
                Err(error) => {
                    tracing::warn!(error = %format!("{error:#}"), "clips: o replay não iniciou");
                    *lock(&self.health) = Health {
                        recording: false,
                        problem: Some("O gravador não abriu. Reinicie o Unkvoid.".into()),
                    };
                }
            },
        }

        match &self.hotkeys {
            Some(hotkeys) => {
                hotkeys.set(Slot::Save, self.settings.save_hotkey);
                hotkeys.set(Slot::Overlay, self.settings.overlay_hotkey);
                hotkeys.set_active(self.settings.hotkeys_active());
            }
            None => self.start_hotkeys(),
        }
    }

    fn start_hotkeys(&mut self) {
        let sender = self.sender.clone();
        let context = self.context.clone();
        let started = Hotkeys::start(
            self.settings.overlay_hotkey,
            self.settings.save_hotkey,
            self.settings.hotkeys_active(),
            move |event| {
                let event = match event {
                    HotkeyEvent::Save => RuntimeEvent::Save,
                    HotkeyEvent::Overlay => RuntimeEvent::Overlay,
                    HotkeyEvent::Captured(slot, hotkey) => RuntimeEvent::Captured(slot, hotkey),
                    HotkeyEvent::CaptureCancelled => RuntimeEvent::CaptureCancelled,
                    HotkeyEvent::Taken(hotkey) => RuntimeEvent::Taken(hotkey),
                };
                let _ = sender.try_send(event);
                context.request_repaint();
            },
        );

        match started {
            Ok(hotkeys) => self.hotkeys = Some(hotkeys),
            Err(error) => {
                self.hotkey_error = Some("Os atalhos não foram registrados. Reinicie o Unkvoid.".into());
                tracing::warn!(error = %error, "clips: os atalhos não iniciaram");
            }
        }
    }

    /// O "Salvar replay" da bandeja: o mesmo caminho do atalho.
    pub fn request_save(&self) {
        let _ = self.sender.try_send(RuntimeEvent::Save);
        self.context.request_repaint();
    }

    pub fn capture_hotkey(&mut self, slot: Slot) {
        if let Some(hotkeys) = &self.hotkeys {
            self.capturing = Some(slot);
            hotkeys.capture_next(slot);
        }
    }

    pub fn health(&self) -> Health {
        lock(&self.health).clone()
    }

    /// Um clipe está sendo escrito: a atualização espera, senão o instalador fecha o app no meio
    /// do arquivo.
    pub fn saving(&self) -> bool {
        self.saving.load(Ordering::Relaxed)
    }

    pub fn request_folder(&mut self) {
        self.folder_picker_open = true;

        let sender = self.sender.clone();
        let start = self.settings.folder.clone();
        let owner = crate::shell::foreground_window().0 as usize;
        let context = self.context.clone();

        std::thread::spawn(move || {
            let owner = windows::Win32::Foundation::HWND(owner as *mut std::ffi::c_void);
            let selected = crate::shell::pick_folder(owner, &start);
            let _ = sender.send(RuntimeEvent::FolderSelected(selected));
            context.request_repaint();
        });
    }

    pub fn take_open_gallery(&mut self) -> bool {
        std::mem::take(&mut self.open_gallery)
    }

    /// Os monitores e os microfones, lidos quando a página dos Clips abre: o principal é o
    /// primeiro card, e a contagem segue dele.
    pub fn load_devices(&mut self) {
        let mut monitors = clips_engine::capture::monitors().unwrap_or_default();

        monitors.sort_by_key(|monitor| !monitor.primary);
        self.screens = monitors
            .into_iter()
            .enumerate()
            .map(|(position, info)| {
                let preview = crate::shell::monitor_preview(info.handle, 320, 180).map(
                    |(width, height, pixels)| {
                        self.context.load_texture(
                            format!("clips-monitor-{}", info.index),
                            egui::ColorImage::from_rgba_unmultiplied(
                                [width as usize, height as usize],
                                &pixels,
                            ),
                            egui::TextureOptions::LINEAR,
                        )
                    },
                );
                let detail = [
                    info.name.clone(),
                    format!("{}×{} · {} Hz", info.width, info.height, info.refresh_rate),
                ]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");

                Screen {
                    label: format!("Monitor {}", position + 1),
                    detail,
                    preview,
                    info,
                }
            })
            .collect();
        self.microphones = clips_engine::audio::microphones();
    }

    /// O monitor gravado: o escolhido, ou o principal quando a escolha é seguir o principal (ou
    /// aponta para um monitor que não está mais ligado).
    pub fn selected_screen(&self) -> Option<usize> {
        self.settings
            .monitor
            .and_then(|chosen| self.screens.iter().position(|screen| screen.info.index == chosen))
            .or_else(|| self.screens.iter().position(|screen| screen.info.primary))
    }

    pub fn hotkey_error(&self) -> Option<&str> {
        self.hotkey_error.as_deref()
    }

    pub fn take_last_saved(&mut self) -> Option<PathBuf> {
        self.last_saved.take()
    }

    pub fn open_player(&mut self, path: &std::path::Path) -> anyhow::Result<()> {
        let player = crate::clips_player::Player::open(crate::shell::foreground_window(), path)?;
        player.set_volume(f64::from(self.player_volume) / 100.0);
        self.player = Some(player);
        self.player_path = Some(path.to_owned());
        Ok(())
    }

    pub fn player_path(&self) -> Option<&std::path::Path> {
        self.player_path.as_deref()
    }

    pub fn place_player(&self, rectangle: egui::Rect, pixels_per_point: f32) {
        if let Some(player) = &self.player {
            player.place(
                (rectangle.left() * pixels_per_point) as i32,
                (rectangle.top() * pixels_per_point) as i32,
                (rectangle.width() * pixels_per_point) as i32,
                (rectangle.height() * pixels_per_point) as i32,
            );
        }
    }

    pub fn toggle_player(&self) {
        if let Some(player) = &self.player {
            player.toggle();
        }
    }

    pub fn seek_player(&self, fraction: f32) {
        if let Some(player) = &self.player {
            player.seek(fraction);
        }
    }

    pub fn player_is_playing(&self) -> bool {
        self.player.as_ref().is_some_and(crate::clips_player::Player::is_playing)
    }

    /// Onde o vídeo está e quanto dura, em segundos.
    pub fn player_position(&self) -> (f64, f64) {
        self.player.as_ref().map_or((0.0, 0.0), crate::clips_player::Player::position)
    }

    pub fn player_volume(&self) -> u8 {
        self.player_volume
    }

    pub fn set_player_volume(&mut self, volume: u8) {
        self.player_volume = volume.min(100);

        if let Some(player) = &self.player {
            player.set_volume(f64::from(self.player_volume) / 100.0);
        }
    }

    pub fn close_player(&mut self) {
        self.player = None;
        self.player_path = None;
    }

    pub fn request_thumbnail(&mut self, path: &std::path::Path) {
        if self.thumbnails.contains_key(path) || !self.thumbnail_pending.insert(path.to_owned()) {
            return;
        }

        let sender = self.sender.clone();
        let path = path.to_owned();
        let context = self.context.clone();
        std::thread::spawn(move || {
            use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            let image = crate::shell::thumbnail(&path, 480, 270);
            let _ = sender.send(RuntimeEvent::Thumbnail(path, image));
            context.request_repaint();
        });
    }

    pub fn thumbnail(&self, path: &std::path::Path) -> Option<&egui::TextureHandle> {
        self.thumbnails.get(path)
    }

    /// O que os atalhos e as threads mandaram. Devolve os avisos para a janela.
    pub fn poll(&mut self) -> Vec<String> {
        let mut notices = Vec::new();

        while let Ok(event) = self.events.try_recv() {
            match event {
                // Com o painel aberto a janela da frente é ele: o atalho de salvar o fecha e salva
                // o jogo que estava na frente antes.
                RuntimeEvent::Save => match self.overlay.take() {
                    Some(overlay) => self.save(overlay.close(true), &mut notices),
                    None => self.save(crate::shell::foreground_window(), &mut notices),
                },
                RuntimeEvent::Overlay => self.toggle_overlay(&mut notices),
                RuntimeEvent::Captured(slot, hotkey) => self.captured(slot, hotkey, &mut notices),
                RuntimeEvent::CaptureCancelled => self.capturing = None,
                RuntimeEvent::Taken(hotkey) => {
                    self.hotkey_error = Some(format!(
                        "Outro programa registrou {}. Ela funciona, mas o jogo também a recebe.",
                        hotkey.label()
                    ));
                }
                RuntimeEvent::Failed(message) => {
                    crate::shell::play_sound(false);
                    crate::toast::show("O replay não foi salvo", "Confira o espaço no disco da pasta dos clipes.");
                    notices.push(message);
                }
                RuntimeEvent::Saved(path) => {
                    let game = path
                        .parent()
                        .and_then(|folder| folder.file_name())
                        .map(|folder| gallery::game_label(&folder.to_string_lossy()))
                        .unwrap_or_default();

                    crate::toast::show("Replay salvo", &game);
                    self.last_saved = Some(path);
                    notices.push("Clipe salvo".to_owned());
                }
                RuntimeEvent::FolderSelected(selected) => {
                    self.folder_picker_open = false;

                    if let Some(path) = selected {
                        self.update(ClipsSettings { folder: path, ..self.settings.clone() });
                    }
                }
                RuntimeEvent::Thumbnail(path, image) => {
                    self.thumbnail_pending.remove(&path);
                    if let Some((width, height, pixels)) = image {
                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [width as usize, height as usize],
                            &pixels,
                        );
                        let texture = self.context.load_texture(
                            format!("clip-thumbnail-{}", path.display()),
                            image,
                            egui::TextureOptions::LINEAR,
                        );
                        self.thumbnails.insert(path, texture);
                    }
                }
            }
        }

        self.show_overlay(&mut notices);

        notices
    }

    /// O Alt+Z: abre o painel, ou fecha o aberto. Em tela cheia exclusiva qualquer janela por
    /// cima tira o jogo da tela cheia e o minimiza: o atalho faz direto o que o botão do replay
    /// faria, com o som no lugar do painel.
    fn toggle_overlay(&mut self, notices: &mut Vec<String>) {
        if let Some(overlay) = self.overlay.take() {
            overlay.close(true);
        } else if crate::shell::exclusive_fullscreen() {
            if self.settings.enabled {
                self.save(crate::shell::foreground_window(), notices);
            } else {
                self.update(ClipsSettings { enabled: true, ..self.settings.clone() });
                crate::shell::play_sound(true);
            }
        } else {
            let overlay = crate::overlay::OverlayState::new(crate::shell::foreground_window());
            let (x, y, width, height) = overlay.monitor;

            crate::overlay::backdrop_show(x, y, width, height, &self.context);
            self.overlay = Some(overlay);
        }
    }

    /// O painel aberto, a cada quadro: no quadro em que ele não é desenhado, o egui fecha a janela.
    fn show_overlay(&mut self, notices: &mut Vec<String>) {
        use crate::overlay::Action;

        // Perguntado sempre, mesmo sem painel: um clique que sobrasse fecharia o próximo.
        let clicked = crate::overlay::take_backdrop_click();
        let Some(mut overlay) = self.overlay.take() else { return };

        if clicked && !self.folder_picker_open {
            overlay.close(true);

            return;
        }

        let context = self.context.clone();

        match crate::overlay::draw(&context, &mut overlay, &self.settings, self.saving()) {
            Action::None => {}
            Action::Close => {
                overlay.close(true);

                return;
            }
            Action::Replay if self.settings.enabled => {
                self.save(overlay.close(true), notices);

                return;
            }
            // Desligado, o botão liga e o painel fica aberto, mostrando que agora grava.
            Action::Replay => self.update(ClipsSettings { enabled: true, ..self.settings.clone() }),
            // A janela costuma estar escondida na bandeja. O foco não volta para o jogo: vai para
            // a janela que abre.
            Action::Gallery => {
                overlay.close(false);
                self.open_gallery = true;

                for command in [
                    egui::ViewportCommand::Visible(true),
                    egui::ViewportCommand::Minimized(false),
                    egui::ViewportCommand::Focus,
                ] {
                    context.send_viewport_cmd_to(egui::ViewportId::ROOT, command);
                }

                return;
            }
            Action::Settings(settings) => self.update(settings),
            Action::PickFolder => self.request_folder(),
        }

        self.overlay = Some(overlay);
    }

    fn captured(&mut self, slot: Slot, hotkey: Hotkey, notices: &mut Vec<String>) {
        self.capturing = None;

        let mut settings = self.settings.clone();
        let (chosen, other) = match slot {
            Slot::Overlay => (&mut settings.overlay_hotkey, settings.save_hotkey),
            Slot::Save => (&mut settings.save_hotkey, settings.overlay_hotkey),
        };

        // A mesma combinação nos dois atalhos: o segundo registro falharia, e a tecla faria as
        // duas coisas. Fica a de antes.
        if hotkey == other {
            self.hotkey_error = Some(format!("{} já é o outro atalho. Escolha uma combinação diferente.", hotkey.label()));

            if let Some(hotkeys) = &self.hotkeys {
                hotkeys.set(slot, *chosen);
            }

            return;
        }

        *chosen = hotkey;
        self.hotkey_error = None;
        self.update(settings);
        notices.push(format!("Atalho alterado para {}.", hotkey.label()));
    }

    /// Salva o replay do jogo em `window` (a pasta e o nome do clipe saem dele): o aviso aparece na
    /// hora, por cima do jogo, e o MP4 é montado numa thread à parte, com o gravador livre para
    /// seguir gravando.
    fn save(&mut self, window: windows::Win32::Foundation::HWND, notices: &mut Vec<String>) {
        if self.saving.load(Ordering::Relaxed) {
            return;
        }

        let Some(recorder) = self.recorder.clone() else {
            return;
        };

        if !self.settings.enabled {
            crate::toast::show(
                "O replay está desligado",
                &format!("Aperte {} para ligar.", self.settings.overlay_hotkey.label()),
            );

            return;
        }

        let prepared = lock(&recorder).prepare_save(self.settings.replay());
        let job = match prepared {
            Ok(job) => job,
            Err(error) => {
                tracing::warn!(error = %format!("{error:#}"), "clips: o replay não foi salvo");
                crate::shell::play_sound(false);
                crate::toast::show("O replay não foi salvo", "A gravação ainda não tem imagem. Tente de novo em instantes.");
                notices.push("O replay não foi salvo.".into());

                return;
            }
        };

        let game = gallery::target(
            &crate::shell::process_name(window),
            &crate::shell::window_title(window),
            crate::shell::is_fullscreen(window),
        );
        let folder = self.settings.folder.join(&game.folder);
        let output = folder.join(gallery::file_name(&game.title));
        let label = gallery::game_label(&game.folder);
        let (sender, context, saving) = (self.sender.clone(), self.context.clone(), self.saving.clone());

        crate::toast::show("Salvando replay…", &label);
        saving.store(true, Ordering::Relaxed);

        std::thread::spawn(move || {
            let result = std::fs::create_dir_all(&folder)
                .map_err(anyhow::Error::from)
                .and_then(|()| job.write(&output));
            let event = match result {
                Ok(_) => RuntimeEvent::Saved(output),
                Err(error) => {
                    tracing::warn!(error = %format!("{error:#}"), "clips: o MP4 não foi escrito");
                    RuntimeEvent::Failed("O replay não foi salvo. Confira o espaço no disco da pasta dos clipes.".into())
                }
            };

            saving.store(false, Ordering::Relaxed);
            let _ = sender.try_send(event);
            context.request_repaint();
        });
    }

    pub fn clips(&self) -> Vec<gallery::Clip> {
        gallery::list(&self.settings.folder)
    }
}

/// O vigia: a captura que cai sozinha (jogo que troca de resolução, driver que reinicia) é
/// religada sem a pessoa perceber, e o que ele viu fica para a página dos Clips. Some junto com o
/// gravador.
fn watch(recorder: &Arc<Mutex<Recorder>>, health: Arc<Mutex<Health>>, context: egui::Context) {
    let recorder = Arc::downgrade(recorder);

    std::thread::Builder::new()
        .name("clips-watch".into())
        .spawn(move || {
            while let Some(recorder) = recorder.upgrade() {
                let seen = {
                    let mut recorder = lock(&recorder);
                    let recording = recorder.keep_alive();

                    Health { recording, problem: recorder.problem().map(str::to_owned) }
                };

                drop(recorder);

                if *lock(&health) != seen {
                    *lock(&health) = seen;
                    context.request_repaint();
                }

                std::thread::sleep(WATCH_EVERY);
            }
        })
        .map(drop)
        .unwrap_or_else(|error| tracing::warn!(%error, "clips: o vigia da gravação não subiu"));
}

/// O que a bandeja e o aviso dizem do replay: ligado, desligado ou com problema.
pub fn status_line(settings: &ClipsSettings, health: &Health) -> String {
    match (settings.enabled, health) {
        (false, _) => "Replay desligado".into(),
        (true, Health { problem: Some(problem), .. }) => problem.clone(),
        (true, Health { recording: true, .. }) => format!("Gravando os últimos {} min", settings.replay_minutes),
        (true, _) => "Replay ligando…".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_uses_only_the_supported_quality_range_and_scales_with_the_frame_rate() {
        assert_eq!(bitrate_for(0, 60, 1920, 1080), 20_000_000);
        assert_eq!(bitrate_for(99, 60, 1920, 1080), 50_000_000);
        assert_eq!(bitrate_for(2, 30, 1920, 1080), 25_000_000);
    }

    #[test]
    fn clip_titles_cannot_escape_the_folder_or_use_windows_reserved_characters() {
        assert_eq!(
            clip_file_title("  Raid: <1/2>  ").as_deref(),
            Some("Raid 12")
        );
        assert_eq!(clip_file_title("  ...  "), None);
    }

    #[test]
    fn renaming_onto_another_clip_keeps_the_other_clip() {
        let folder = std::env::temp_dir().join(format!("unkvoid-rename-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("pasta de teste");
        let (raid, boss) = (folder.join("Raid.mp4"), folder.join("Boss.mp4"));
        std::fs::write(&raid, b"raid").expect("clipe 1");
        std::fs::write(&boss, b"boss").expect("clipe 2");

        assert!(rename_clip(&raid, "Boss").is_err(), "o outro clipe seria substituído");
        assert_eq!(std::fs::read(&boss).expect("o outro clipe"), b"boss");
        assert_eq!(rename_clip(&raid, "raid").expect("só a caixa muda"), folder.join("raid.mp4"));

        std::fs::remove_dir_all(&folder).expect("limpeza");
    }

    #[test]
    fn replay_is_off_by_default_with_the_microphone_on_and_duration_stays_inside_the_engine_limits() {
        let settings = ClipsSettings::default();

        assert!(!settings.enabled);
        assert!(settings.microphone && settings.noise_suppression);
        assert!(!settings.hotkeys_active(), "quem nunca usou os Clips não perde o Alt+Z");
        assert_eq!(bounded_replay_minutes(0), 5);
        assert_eq!(bounded_replay_minutes(15), 15);
        assert_eq!(bounded_replay_minutes(90), 30);
    }

    #[test]
    fn clips_preferences_round_trip_without_losing_the_folder_or_shortcut() {
        let settings = ClipsSettings {
            enabled: true,
            folder: PathBuf::from(r"D:\Jogos\Clipes"),
            replay_minutes: 10,
            quality: 2,
            monitor: Some(1),
            microphone: false,
            ..ClipsSettings::default()
        };

        let encoded = serde_json::to_vec(&settings).expect("preferências serializadas");
        let decoded: ClipsSettings =
            serde_json::from_slice(&encoded).expect("preferências legíveis");

        assert_eq!(decoded, settings);
    }

    /// O `clips.json` do app de antes (Slint) e o do app novo antes desta versão: os dois abrem
    /// com o que a pessoa tinha escolhido.
    #[test]
    fn the_previous_files_open_with_what_was_chosen() {
        let slint: ClipsSettings = serde_json::from_str(
            r#"{"replay_enabled":true,"clips_folder":"D:\\Clipes","microphone":false,"monitor":2,"clips_user":true}"#,
        )
        .expect("o do app de antes");
        let egui: ClipsSettings =
            serde_json::from_str(r#"{"enabled":true,"folder":"E:\\Clipes","replay_minutes":20}"#)
                .expect("o do app novo");

        assert!(slint.enabled && !slint.microphone && slint.clips_user);
        assert_eq!((slint.folder, slint.monitor), (PathBuf::from(r"D:\Clipes"), Some(2)));
        assert!(egui.enabled);
        assert_eq!((egui.folder, egui.replay_minutes), (PathBuf::from(r"E:\Clipes"), 20));
    }

    #[test]
    fn the_status_says_what_the_replay_is_doing() {
        let on = ClipsSettings { enabled: true, ..ClipsSettings::default() };

        assert_eq!(status_line(&ClipsSettings::default(), &Health::default()), "Replay desligado");
        assert_eq!(status_line(&on, &Health { recording: true, problem: None }), "Gravando os últimos 5 min");
        assert_eq!(
            status_line(&on, &Health { recording: false, problem: Some("Sem placa".into()) }),
            "Sem placa"
        );
    }
}
