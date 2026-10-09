//! Os Clips: o replay instantâneo do Windows. A tela fica sempre gravando num buffer em disco,
//! e só vira arquivo quando a pessoa aperta o atalho ou clica no painel do Alt+Z.
//!
//! Veio do UnkvoidClips, que era um app separado. O motor mora em `shared/clips`; aqui fica o
//! que é do Windows e da janela: a aba, o painel, a bandeja, o aviso e o player.

mod backdrop;
mod player;
mod settings;
pub mod shell;
mod toast;

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use slint::{ComponentHandle, Model, ModelRc, SharedPixelBuffer, SharedString, VecModel};
use clips_engine::audio::Microphone;
use clips_engine::clip::ClipSummary;
use clips_engine::recorder::Recorder;
use windows::Win32::Foundation::HWND;

use clips_engine::gallery;
use clips_engine::hotkeys::{HotkeyEvent, Hotkeys, Slot};
use player::Player;
use settings::{FRAME_RATES, QUALITY_BITRATES, REPLAY_MINUTES, Settings};

use crate::{AppWindow, ClipItem, ClipsTray, ClipsUi, MonitorItem, OverlayWindow};

/// A opacidade do escuro atrás do painel do Alt+Z, de 0 a 255: o jogo aparece por trás.
const OVERLAY_OPACITY: u8 = 190;

thread_local! {
    static APP: OnceCell<Rc<App>> = const { OnceCell::new() };
}

/// O estado do app mora na thread da interface; as outras threads chegam aqui pelo
/// `invoke_from_event_loop`.
fn with_app(action: impl FnOnce(&Rc<App>)) {
    APP.with(|cell| {
        if let Some(app) = cell.get() {
            action(app);
        }
    });
}

fn later(action: impl FnOnce(&Rc<App>) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || with_app(action));
}

/// A pasta e o nome do clipe pela janela que estava na frente: o jogo, quase sempre.
fn target_of(window: HWND) -> gallery::Target {
    gallery::target(&shell::process_name(window), &shell::window_title(window), shell::is_fullscreen(window))
}

fn lock(recorder: &Mutex<Recorder>) -> MutexGuard<'_, Recorder> {
    recorder.lock().unwrap_or_else(PoisonError::into_inner)
}

struct App {
    window: AppWindow,
    overlay: OverlayWindow,
    _tray: ClipsTray,
    settings: RefCell<Settings>,
    recorder: Arc<Mutex<Recorder>>,
    hotkeys: Hotkeys,
    /// Todos os clipes da pasta; `clips` são os do jogo escolhido na galeria, na ordem dos cards.
    all_clips: RefCell<Vec<gallery::Clip>>,
    clips: RefCell<Vec<gallery::Clip>>,
    /// As pastas de jogo na ordem dos chips (a do clipe mais novo primeiro) e a escolhida;
    /// `None` é "Todos".
    games: RefCell<Vec<String>>,
    game: RefCell<Option<String>>,
    clip_model: Rc<VecModel<ClipItem>>,
    thumbnails: RefCell<HashMap<PathBuf, slint::Image>>,
    gallery_generation: Cell<u64>,
    microphones: RefCell<Vec<Microphone>>,
    monitors: RefCell<Vec<clips_engine::capture::MonitorInfo>>,
    player: RefCell<Option<(Player, PathBuf)>>,
    player_timer: slint::Timer,
    saving: Cell<bool>,
    return_focus: Cell<isize>,

    /// O painel do Alt+Z aberto: quando abriu (para a descida), em qual monitor, e o tamanho,
    /// o recorte e a posição já aplicados — a volta do relógio só chama o Windows quando algo
    /// mudou.
    overlay_timer: slint::Timer,
    overlay_opened_at: Cell<Option<Instant>>,
    overlay_monitor: Cell<(i32, i32, i32, i32)>,
    overlay_size: Cell<(i32, i32)>,
    overlay_shape: RefCell<Vec<shell::RoundedRect>>,
    overlay_position: Cell<Option<(i32, i32)>>,
    /// Se a descida desta abertura já terminou: o log do fim sai uma vez só.
    overlay_settled: Cell<bool>,
    /// A janela em primeiro plano na última volta do relógio: quando o jogo toma o foco de
    /// volta com o painel aberto, o log conta.
    overlay_foreground: Cell<isize>,
}

/// Liga os Clips: as escolhas, o replay, os atalhos e a bandeja. Roda antes de o núcleo falar
/// com o servidor, e não depende dele: sem internet, sem conta e com o servidor fora do ar, o
/// replay grava e a galeria abre igual.
pub fn start(window: &AppWindow) -> anyhow::Result<()> {
    let settings = Settings::load();

    shell::set_autostart(settings.start_with_windows);

    let recorder = Arc::new(Mutex::new(Recorder::start(shell::local_folder().join("clips-buffer"), settings.recorder())?));
    // Os atalhos só pegam as teclas com o replay ligado: quem usa o Unkvoid só para transmitir
    // não perde o Alt+Z da NVIDIA.
    let hotkeys = Hotkeys::start(settings.overlay_hotkey, settings.save_hotkey, settings.hotkeys_active(), |event| {
        later(move |app| app.on_hotkey(event))
    })?;
    let clip_model = Rc::new(VecModel::default());
    let app = Rc::new(App {
        window: window.clone_strong(),
        overlay: OverlayWindow::new()?,
        _tray: ClipsTray::new()?,
        settings: RefCell::new(settings),
        recorder: recorder.clone(),
        hotkeys,
        all_clips: RefCell::new(Vec::new()),
        clips: RefCell::new(Vec::new()),
        games: RefCell::new(Vec::new()),
        game: RefCell::new(None),
        clip_model: clip_model.clone(),
        thumbnails: RefCell::new(HashMap::new()),
        gallery_generation: Cell::new(0),
        microphones: RefCell::new(Vec::new()),
        monitors: RefCell::new(Vec::new()),
        player: RefCell::new(None),
        player_timer: slint::Timer::default(),
        saving: Cell::new(false),
        return_focus: Cell::new(0),
        overlay_timer: slint::Timer::default(),
        overlay_opened_at: Cell::new(None),
        overlay_monitor: Cell::new((0, 0, 0, 0)),
        overlay_size: Cell::new((0, 0)),
        overlay_shape: RefCell::new(Vec::new()),
        overlay_position: Cell::new(None),
        overlay_settled: Cell::new(false),
        overlay_foreground: Cell::new(0),
    });

    app.ui().set_available(true);
    app.ui().set_clips(ModelRc::from(clip_model));
    APP.with(|cell| {
        let _ = cell.set(app.clone());
    });
    bind(&app);
    app.load_settings_into_ui();
    app.show_status(true, None);

    // O vigia: a captura morre quando o driver de vídeo é atualizado ou o monitor some, e
    // religá-la sozinha é o que faz o replay estar lá na hora em que a pessoa precisa.
    std::thread::Builder::new().name("clips-watchdog".into()).spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(3));

            let (recording, problem) = {
                let mut recorder = lock(&recorder);
                let recording = recorder.keep_alive();

                (recording, recorder.problem().map(str::to_owned))
            };

            later(move |app| app.show_status(recording, problem));
        }
    })?;

    Ok(())
}

/// Um replay sendo gravado no disco agora. O atualizador do núcleo espera por ele antes de
/// fechar o app para trocar a versão.
static SAVING: AtomicBool = AtomicBool::new(false);

pub fn saving() -> bool {
    SAVING.load(Ordering::Relaxed)
}

/// Mostra a janela: a bandeja, o atalho do menu Iniciar com o app já aberto.
pub fn show_window() {
    with_app(|app| app.show_main());
}

/// A janela foi fechada (ela só se esconde, e o replay segue): o player para junto.
pub fn window_closed() {
    with_app(|app| app.close_player());
}

fn bind(app: &Rc<App>) {
    let ui = app.ui();

    ui.on_save_replay(|| with_app(|app| app.save_replay(gallery::Target::desktop())));
    ui.on_enable_replay(|| with_app(|app| app.set_replay_enabled(true)));
    ui.on_open_clip(|index| with_app(|app| app.open_clip(index as usize)));
    ui.on_delete_clip(|index| with_app(|app| app.delete_clip(index as usize)));
    ui.on_game_chosen(|| {
        with_app(|app| {
            let index = app.ui().get_game_index();
            let game = if index <= 0 { None } else { app.games.borrow().get(index as usize - 1).cloned() };

            *app.game.borrow_mut() = game;
            app.show_clips();
        });
    });
    ui.on_open_folder(|| with_app(|app| shell::open_folder(&app.settings.borrow().clips_folder)));
    ui.on_change_folder(|| with_app(|app| app.change_folder(app.window.window())));
    ui.on_settings_changed(|| with_app(|app| app.settings_changed()));
    ui.on_capture_hotkey(|which| {
        with_app(|app| {
            let slot = if which == "overlay" { Slot::Overlay } else { Slot::Save };

            app.ui().set_capturing_hotkey(which);
            app.hotkeys.capture_next(slot);
        });
    });
    ui.on_player_toggle(|| {
        with_app(|app| {
            if let Some((player, _)) = app.player.borrow().as_ref() {
                player.toggle();
            }
        });
    });
    ui.on_player_seek(|fraction| {
        with_app(|app| {
            if let Some((player, _)) = app.player.borrow().as_ref() {
                player.seek(fraction);
            }
        });
    });
    ui.on_player_set_volume(|volume| {
        with_app(|app| {
            app.ui().set_player_volume(volume);

            if let Some((player, _)) = app.player.borrow().as_ref() {
                player.set_volume(f64::from(volume) / 100.0);
            }
        });
    });
    ui.on_player_close(|| with_app(|app| app.close_player()));
    ui.on_player_show_in_folder(|| {
        with_app(|app| {
            if let Some((_, path)) = app.player.borrow().as_ref() {
                shell::show_in_folder(path);
            }
        });
    });
    ui.on_player_delete(|| {
        with_app(|app| {
            let open = app.player.borrow().as_ref().map(|(_, path)| path.clone());
            let index = open.and_then(|open| app.clips.borrow().iter().position(|clip| clip.path == open));

            // A confirmação é desenhada pelo Slint, e o vídeo é uma janela do Windows por cima
            // dela: o player fecha antes de perguntar.
            if let Some(index) = index {
                app.close_player();
                app.ui().set_page(0);
                app.ui().set_confirm_delete(index as i32);
            }
        });
    });

    app.overlay.on_save(|| {
        with_app(|app| {
            let game = app.dismiss_overlay(true, "salvar no painel");

            app.save_replay(target_of(game));
        });
    });
    app.overlay.on_open_gallery(|| {
        with_app(|app| {
            app.dismiss_overlay(false, "galeria");
            app.close_player();
            app.ui().set_open(true);
            app.ui().set_page(0);
            app.show_main();
        });
    });
    // O botão grande do replay faz o que importa na hora: liga, se está desligado; salva, se
    // está gravando.
    app.overlay.on_replay_clicked(|| {
        with_app(|app| {
            if app.saving.get() {
                return;
            }

            if app.settings.borrow().replay_enabled {
                let game = app.dismiss_overlay(true, "replay no painel");

                app.save_replay(target_of(game));
            } else {
                app.set_replay_enabled(true);
            }
        });
    });
    app.overlay.on_toggle_replay(|| with_app(|app| app.set_replay_enabled(app.overlay.get_enabled())));
    app.overlay.on_replay_length_changed(|| {
        with_app(|app| {
            let mut settings = app.settings.borrow().clone();

            settings.replay_minutes = REPLAY_MINUTES[(app.overlay.get_replay_index().max(0) as usize).min(REPLAY_MINUTES.len() - 1)];
            app.apply_settings(settings);
        });
    });
    app.overlay.on_quality_changed(|| {
        with_app(|app| {
            let mut settings = app.settings.borrow().clone();

            settings.quality = (app.overlay.get_quality_index().max(0) as usize).min(QUALITY_BITRATES.len() - 1);
            app.apply_settings(settings);
        });
    });
    app.overlay.on_fps_changed(|| {
        with_app(|app| {
            let mut settings = app.settings.borrow().clone();

            settings.frame_rate = FRAME_RATES[(app.overlay.get_fps_index().max(0) as usize).min(FRAME_RATES.len() - 1)];
            app.apply_settings(settings);
        });
    });
    app.overlay.on_change_folder(|| with_app(|app| app.change_folder(app.overlay.window())));
    app.overlay.on_dismiss(|| {
        with_app(|app| {
            app.dismiss_overlay(true, "esc");
        });
    });
    // Alt+F4 no painel: fecha como o Esc, levando o escuro junto.
    app.overlay.window().on_close_requested(|| {
        with_app(|app| {
            app.dismiss_overlay(true, "alt+f4");
        });

        slint::CloseRequestResponse::HideWindow
    });

    // Clicar no escuro, fora da barra e do quadro, fecha o painel e devolve o jogo.
    backdrop::on_click(|| {
        with_app(|app| {
            app.dismiss_overlay(true, "clique no escuro");
        });
    });

    app._tray.on_open(|| with_app(|app| app.show_main()));
    app._tray.on_save(|| with_app(|app| app.save_replay(gallery::Target::desktop())));
    app._tray.on_quit(|| with_app(|app| app.quit()));
    app.ui().on_quit(|| with_app(|app| app.quit()));
}

impl App {
    /// O estado dos Clips na janela do Unkvoid.
    fn ui(&self) -> ClipsUi<'_> {
        self.window.global::<ClipsUi>()
    }

    fn on_hotkey(&self, event: HotkeyEvent) {
        match event {
            HotkeyEvent::Save => {
                // Com o painel aberto, a janela da frente é ele: o jogo é o que estava antes.
                let game = if self.overlay.window().is_visible() { self.dismiss_overlay(true, "atalho de salvar") } else { shell::foreground_window() };

                self.save_replay(target_of(game));
            }
            HotkeyEvent::Overlay => self.toggle_overlay(),
            HotkeyEvent::Captured(slot, hotkey) => {
                let mut settings = self.settings.borrow().clone();
                let (chosen, other) = match slot {
                    Slot::Overlay => (&mut settings.overlay_hotkey, settings.save_hotkey),
                    Slot::Save => (&mut settings.save_hotkey, settings.overlay_hotkey),
                };

                self.ui().set_capturing_hotkey("".into());

                // A mesma combinação nos dois atalhos: o segundo registro falharia, e a tecla
                // faria as duas coisas. Fica a de antes.
                if hotkey == other {
                    toast::show(&format!("{} já é o outro atalho", hotkey.label()), "Escolha uma combinação diferente.");
                    self.hotkeys.set(slot, *chosen);

                    return;
                }

                *chosen = hotkey;
                self.hotkeys.set(slot, hotkey);
                self.apply_settings(settings);
            }
            HotkeyEvent::CaptureCancelled => self.ui().set_capturing_hotkey("".into()),
            HotkeyEvent::Taken(hotkey) => toast::show(
                &format!("{} já está em uso", hotkey.label()),
                "Outro programa registrou a combinação. Ela funciona, mas o jogo também a recebe.",
            ),
        }
    }

    fn show_main(&self) {
        self.load_monitors();
        self.refresh_gallery();

        let _ = self.window.show();

        if let Some(handle) = shell::window_handle(self.window.window()) {
            shell::restore_if_minimized(handle);
            shell::bring_to_front(handle);
        }
    }

    fn show_status(&self, recording: bool, problem: Option<String>) {
        let (enabled, minutes) = {
            let settings = self.settings.borrow();

            (settings.replay_enabled, settings.replay_minutes)
        };
        let status = match (enabled, recording) {
            (false, _) => "Replay desligado".to_owned(),
            (true, true) => format!("Gravando · replay de {minutes} min"),
            (true, false) => "Replay parado".to_owned(),
        };

        self.ui().set_recording(recording);
        self.ui().set_status(status.into());
        self.ui().set_problem(problem.unwrap_or_default().into());
    }

    /// Sair do app, pela bandeja ou pelas Configurações. Com um replay sendo salvo, não: o
    /// arquivo ficaria pela metade.
    fn quit(&self) {
        if self.saving.get() {
            toast::show("Ainda salvando o replay", "Espere terminar para sair do Unkvoid.");

            return;
        }

        let _ = slint::quit_event_loop();
    }

    fn set_replay_enabled(&self, enabled: bool) {
        let mut settings = self.settings.borrow().clone();

        settings.replay_enabled = enabled;
        tracing::info!(enabled, "replay: ligado ou desligado pela pessoa");
        self.apply_settings(settings);
        // O vigia confirma em até 3 s; até lá o status já diz o que a pessoa pediu.
        self.show_status(enabled, None);
    }

    /// O painel do Alt+Z mostra o mesmo que a janela: ligado, tempo, pasta, atalho.
    fn sync_overlay(&self) {
        let settings = self.settings.borrow();

        self.overlay.set_enabled(settings.replay_enabled);
        self.overlay.set_quality_index(settings.quality as i32);
        self.overlay.set_fps_index(FRAME_RATES.iter().position(|rate| *rate == settings.frame_rate).unwrap_or(1) as i32);
        self.overlay.set_replay_index(REPLAY_MINUTES.iter().position(|minutes| *minutes == settings.replay_minutes).unwrap_or(0) as i32);
        self.overlay.set_clips_folder(settings.clips_folder.display().to_string().into());
        self.overlay.set_save_hotkey(settings.save_hotkey.label().into());
        self.overlay.set_overlay_hotkey(settings.overlay_hotkey.label().into());
        self.overlay.set_saving(self.saving.get());
    }

    /// Os cards dos monitores, com a miniatura de cada um. Só quando a janela abre: a miniatura
    /// é um print da tela inteira, e o Windows pode ter trocado o principal ou desligado um
    /// monitor desde a última vez.
    fn load_monitors(&self) {
        let mut monitors = clips_engine::capture::monitors().unwrap_or_default();

        // O principal é o primeiro card, à esquerda, e a contagem segue dele.
        monitors.sort_by_key(|monitor| !monitor.primary);

        let items: Vec<MonitorItem> = monitors
            .iter()
            .enumerate()
            .map(|(position, monitor)| MonitorItem {
                label: format!("Monitor {}", position + 1).into(),
                detail: [monitor.name.clone(), format!("{}×{} · {} Hz", monitor.width, monitor.height, monitor.refresh_rate)]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ")
                    .into(),
                primary: monitor.primary,
                preview: shell::monitor_preview(monitor.handle, 320, 180)
                    .map(|(width, height, pixels)| {
                        slint::Image::from_rgba8(SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&pixels, width, height))
                    })
                    .unwrap_or_default(),
            })
            .collect();

        self.ui().set_monitors(ModelRc::from(Rc::new(VecModel::from(items))));
        *self.monitors.borrow_mut() = monitors;
        self.select_monitor();
    }

    /// O card escolhido, ou o principal do Windows quando a escolha é seguir o principal (ou
    /// aponta para um monitor que não está mais ligado).
    fn select_monitor(&self) {
        let monitors = self.monitors.borrow();
        let index = self
            .settings
            .borrow()
            .monitor
            .and_then(|chosen| monitors.iter().position(|monitor| monitor.index == chosen))
            .or_else(|| monitors.iter().position(|monitor| monitor.primary))
            .unwrap_or(0);

        self.ui().set_monitor_index(index as i32);
    }

    fn load_settings_into_ui(&self) {
        let settings = self.settings.borrow();
        let window = self.ui();
        let microphones = clips_engine::audio::microphones();
        let microphone_names: Vec<SharedString> = std::iter::once("Padrão do Windows".into())
            .chain(microphones.iter().map(|microphone| microphone.label.as_str().into()))
            .collect();
        let microphone_index = settings
            .microphone_device
            .as_ref()
            .and_then(|device| microphones.iter().position(|microphone| &microphone.id == device))
            .map_or(0, |index| index + 1);

        window.set_replay_enabled(settings.replay_enabled);
        window.set_replay_index(REPLAY_MINUTES.iter().position(|minutes| *minutes == settings.replay_minutes).unwrap_or(0) as i32);
        window.set_quality_index(settings.quality as i32);
        window.set_fps_index(FRAME_RATES.iter().position(|rate| *rate == settings.frame_rate).unwrap_or(1) as i32);
        window.set_system_audio(settings.system_audio);
        window.set_microphone(settings.microphone);
        window.set_microphones(ModelRc::from(Rc::new(VecModel::from(microphone_names))));
        window.set_microphone_index(microphone_index as i32);
        window.set_noise_suppression(settings.noise_suppression);
        window.set_save_hotkey(settings.save_hotkey.label().into());
        window.set_overlay_hotkey(settings.overlay_hotkey.label().into());
        window.set_clips_folder(settings.clips_folder.display().to_string().into());
        window.set_start_with_windows(settings.start_with_windows);
        window.set_buffer_estimate(settings.buffer_estimate().into());
        window.set_save_label(format!("Salvar últimos {} min", settings.replay_minutes).into());
        *self.microphones.borrow_mut() = microphones;
        drop(settings);
        self.select_monitor();
    }

    fn settings_changed(&self) {
        let window = self.ui();
        let mut settings = self.settings.borrow().clone();

        settings.replay_enabled = window.get_replay_enabled();
        settings.replay_minutes = REPLAY_MINUTES[(window.get_replay_index().max(0) as usize).min(REPLAY_MINUTES.len() - 1)];
        settings.quality = window.get_quality_index().max(0) as usize;
        settings.frame_rate = FRAME_RATES[(window.get_fps_index().max(0) as usize).min(FRAME_RATES.len() - 1)];
        // Escolher o principal é seguir o principal do Windows: se a pessoa trocar o principal
        // nas configurações da tela, o replay vai junto.
        // Sem a lista (a janela ainda não mostrou os monitores), a escolha fica como estava.
        settings.monitor = match self.monitors.borrow().get(window.get_monitor_index().max(0) as usize) {
            Some(monitor) if !monitor.primary => Some(monitor.index),
            Some(_) => None,
            None => settings.monitor,
        };
        settings.system_audio = window.get_system_audio();
        settings.microphone = window.get_microphone();
        settings.microphone_device = match window.get_microphone_index() {
            index if index <= 0 => None,
            index => self.microphones.borrow().get(index as usize - 1).map(|microphone| microphone.id.clone()),
        };
        settings.noise_suppression = window.get_noise_suppression();
        settings.start_with_windows = window.get_start_with_windows();
        self.apply_settings(settings);
    }

    fn apply_settings(&self, mut settings: Settings) {
        // Ligou uma vez, virou quem usa os Clips: os atalhos seguem registrados com o replay
        // desligado, para o Alt+Z religar pelo painel.
        settings.clips_user |= settings.replay_enabled;

        if let Err(error) = settings.save() {
            tracing::error!(error = %error, "configurações: não foi possível salvar");
        }

        shell::set_autostart(settings.start_with_windows);
        // Quem nunca ligou o replay não perde tecla nenhuma: o Alt+Z segue do jogo e da NVIDIA.
        self.hotkeys.set_active(settings.hotkeys_active());

        let recorder = self.recorder.clone();
        let recorder_settings = settings.recorder();

        // Religar a captura e o som leva de um instante a alguns segundos (o Windows abrindo
        // o microfone); a interface não espera por isso.
        std::thread::spawn(move || lock(&recorder).apply(recorder_settings));
        *self.settings.borrow_mut() = settings;
        self.load_settings_into_ui();
        self.sync_overlay();
    }

    /// O seletor de pasta abre por cima de quem pediu: a janela ou o painel do Alt+Z.
    fn change_folder(&self, owner: &slint::Window) {
        let current = self.settings.borrow().clips_folder.clone();
        let Some(owner) = shell::window_handle(owner) else { return };

        backdrop::set_enabled(false);

        let picked = shell::pick_folder(owner, &current);

        backdrop::set_enabled(true);

        let Some(folder) = picked else { return };
        let mut settings = self.settings.borrow().clone();

        settings.clips_folder = folder;
        self.apply_settings(settings);
        self.refresh_gallery();
    }

    fn toggle_overlay(&self) {
        if self.overlay.window().is_visible() {
            self.dismiss_overlay(true, "atalho do painel");

            return;
        }

        let foreground = shell::foreground_window();

        // O painel nunca pode minimizar o jogo: em tela cheia exclusiva ele nem abre, e o
        // Alt+Z salva direto, com o som de confirmação no lugar do aviso.
        // Desligado, o Alt+Z liga, já que o painel não abre para ligar por ele.
        if shell::exclusive_fullscreen() {
            if self.settings.borrow().replay_enabled {
                tracing::info!("painel: jogo em tela cheia exclusiva, salvando sem abrir o painel");
                self.save_replay(target_of(foreground));
            } else {
                self.set_replay_enabled(true);
                shell::play_sound(true);
            }

            return;
        }

        // Sempre no monitor principal, que é onde o jogo roda, e não no da janela que estava
        // na frente (o Discord no segundo monitor, por exemplo).
        let monitor = shell::primary_monitor_rect();
        let (x, y, width, height) = monitor;

        self.return_focus.set(foreground.0 as isize);
        self.overlay_monitor.set(monitor);
        self.overlay_size.set((0, 0));
        self.overlay_shape.borrow_mut().clear();
        self.overlay_position.set(None);
        self.overlay_settled.set(false);
        self.overlay_foreground.set(foreground.0 as isize);
        self.sync_overlay();
        self.overlay.set_panel_open(false);
        tracing::info!(title = %shell::window_title(foreground), "painel: aberto");

        // O escuro primeiro, apagado. A volta do relógio mede, posiciona e recorta a janela
        // antes de ela aparecer, com o conteúdo ainda todo acima dela: nada pisca. Depois do
        // `show` ela volta, porque o recorte precisa da janela já criada.
        backdrop::show(x, y, width, height, 0);
        self.overlay_opened_at.set(Some(Instant::now()));
        self.tick_overlay();

        let _ = self.overlay.show();

        if let Some(handle) = shell::window_handle(self.overlay.window()) {
            shell::hide_from_taskbar(handle);
            shell::suppress_frame(handle);
            shell::bring_to_front(handle);
            backdrop::keep_below(handle);
        }

        self.tick_overlay();
        self.overlay_timer.start(slint::TimerMode::Repeated, Duration::from_millis(16), || {
            with_app(|app| app.tick_overlay());
        });
    }

    /// Uma volta do relógio do painel aberto: acompanha o tamanho e o recorte (abrir
    /// Configurações aumenta o painel) e faz a descida e o escuro acenderem juntos.
    ///
    /// A janela fica parada no alto do monitor, e quem desce é o conteúdo, dentro dela. Uma
    /// janela que entra na tela vindo de fora não se pinta: o Slint redesenha só o que mudou, e
    /// o que ele desenhou com ela fora da tela se perdeu. A 0.1.2 mostrava só o escuro.
    fn tick_overlay(&self) {
        let window = self.overlay.window();
        let Some(opened_at) = self.overlay_opened_at.get() else {
            self.overlay_timer.stop();

            return;
        };
        let scale = window.scale_factor();
        let physical = |length: f32| (length * scale).round() as i32;
        let content_height = self.overlay.get_content_height();
        let size = (physical(self.overlay.get_content_width()), physical(content_height));

        if size.0 <= 0 || size.1 <= 0 {
            return;
        }

        // Desce do alto do monitor em 260 ms, desacelerando no fim: começa com o conteúdo
        // inteiro acima da janela e termina no lugar.
        let progress = (opened_at.elapsed().as_secs_f32() / 0.26).min(1.0);
        let eased = 1.0 - (1.0 - progress).powi(3);

        self.overlay.set_slide(-content_height * (1.0 - eased));
        backdrop::set_opacity((f32::from(OVERLAY_OPACITY) * eased) as u8);

        if self.overlay_size.get() != size {
            window.set_size(slint::PhysicalSize::new(size.0 as u32, size.1 as u32));
            self.overlay_size.set(size);
        }

        let (monitor_x, monitor_y, monitor_width, _) = self.overlay_monitor.get();
        let position = (monitor_x + (monitor_width - size.0) / 2, monitor_y);

        if self.overlay_position.get() != Some(position) {
            window.set_position(slint::PhysicalPosition::new(position.0, position.1));
            self.overlay_position.set(Some(position));
        }

        let overlay = &self.overlay;
        let mut shape = vec![(
            physical(overlay.get_toolbar_x()),
            physical(overlay.get_toolbar_y()),
            physical(overlay.get_toolbar_width()),
            physical(overlay.get_toolbar_height()),
            physical(22.0),
        )];

        if overlay.get_panel_open() {
            shape.push((
                physical(overlay.get_panel_x()),
                physical(overlay.get_panel_y()),
                physical(overlay.get_panel_width()),
                physical(overlay.get_panel_height()),
                physical(18.0),
            ));
        }

        // Antes do `show` a janela ainda não existe: o recorte fica para a volta seguinte.
        if *self.overlay_shape.borrow() != shape
            && let Some(handle) = shell::window_handle(window)
        {
            shell::shape_window(handle, &shape);
            *self.overlay_shape.borrow_mut() = shape;
        }

        let foreground = shell::foreground_window();

        if self.overlay_foreground.replace(foreground.0 as isize) != foreground.0 as isize {
            tracing::info!(title = %shell::window_title(foreground), "painel: primeiro plano mudou");
        }

        // Parou de descer: a ordem das janelas de novo (alguém pode ter subido no meio) e o
        // retrato de onde o painel ficou, para quando ele não aparecer.
        if progress >= 1.0
            && !self.overlay_settled.replace(true)
            && let Some(handle) = shell::window_handle(window)
        {
            backdrop::keep_below(handle);
            tracing::info!(window = %shell::describe_window(handle), above_backdrop = backdrop::is_below(handle), "painel: na tela");
        }
    }

    /// Fecha o painel e devolve a janela que estava na frente antes dele (o jogo), de onde sai
    /// a pasta e o nome do clipe salvo por ele.
    fn dismiss_overlay(&self, return_focus: bool, reason: &str) -> HWND {
        let previous = HWND(self.return_focus.get() as *mut _);
        let open_for = self.overlay_opened_at.get().map(|opened_at| opened_at.elapsed().as_millis());

        tracing::info!(reason, open_for_ms = ?open_for, "painel: fechado");

        self.overlay_timer.stop();
        self.overlay_opened_at.set(None);
        backdrop::hide();

        let _ = self.overlay.hide();

        if return_focus && !previous.is_invalid() {
            shell::restore_if_minimized(previous);
            shell::bring_to_front(previous);
        }

        previous
    }

    fn save_replay(&self, target: gallery::Target) {
        if !self.settings.borrow().replay_enabled {
            let settings = self.settings.borrow();
            let how = if settings.hotkeys_active() {
                format!("Ligue em {} para começar a gravar.", settings.overlay_hotkey.label())
            } else {
                "Ligue na aba Clips do Unkvoid para começar a gravar.".to_owned()
            };

            drop(settings);
            toast::show("O replay está desligado", &how);

            if shell::exclusive_fullscreen() {
                shell::play_sound(false);
            }

            return;
        }

        // O aviso de "Salvando" some em 3 s, e trinta minutos podem levar mais que isso.
        if self.saving.replace(true) {
            toast::show("Salvando replay…", "Ainda salvando o anterior.");

            return;
        }

        SAVING.store(true, Ordering::Relaxed);

        let recorder = self.recorder.clone();
        let settings = self.settings.borrow().clone();

        self.ui().set_saving(true);
        self.overlay.set_saving(true);
        // Na hora do atalho, e não só no fim: salvar trinta minutos leva segundos, e sem aviso a
        // pessoa aperta de novo achando que não pegou.
        toast::show("Salvando replay…", &format!("Últimos {} min · {}", settings.replay_minutes, target.title));

        std::thread::spawn(move || {
            shell::lower_thread_priority();

            let result = (|| -> anyhow::Result<(PathBuf, ClipSummary)> {
                let job = lock(&recorder).prepare_save(Duration::from_secs(u64::from(settings.replay_minutes) * 60))?;
                let folder = settings.clips_folder.join(&target.folder);

                std::fs::create_dir_all(&folder)?;

                let path = folder.join(gallery::file_name(&target.title));
                let summary = job.write(&path)?;

                Ok((path, summary))
            })();

            let outcome = result.map_err(|error| {
                tracing::error!(error = %format!("{error:#}"), "replay: não foi possível salvar");

                format!("{error:#}")
            });

            later(move |app| app.saved(outcome));
        });
    }

    fn saved(&self, outcome: Result<(PathBuf, ClipSummary), String>) {
        self.saving.set(false);
        SAVING.store(false, Ordering::Relaxed);
        self.ui().set_saving(false);
        self.overlay.set_saving(false);

        // Em tela cheia exclusiva o aviso na tela não aparece por cima do jogo.
        if shell::exclusive_fullscreen() {
            shell::play_sound(outcome.is_ok());
        }

        match outcome {
            Ok((path, summary)) => {
                let seconds = summary.duration.as_secs();
                let name = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();

                tracing::info!(path = %path.display(), seconds, bytes = summary.bytes, "replay: salvo");
                toast::show(
                    "Replay salvo",
                    &format!("{} · {}:{:02} · {} MB", name, seconds / 60, seconds % 60, summary.bytes / 1_000_000),
                );

                // Com a janela fechada (no jogo, quase sempre) a galeria fica para quando ela abrir:
                // ler todos os clipes e gerar miniaturas agora disputaria o disco com o jogo.
                if self.window.window().is_visible() {
                    self.refresh_gallery();
                }
            }
            Err(error) => toast::show("Não deu para salvar o replay", &error),
        }
    }

    fn refresh_gallery(&self) {
        let generation = self.gallery_generation.get() + 1;
        let folder = self.settings.borrow().clips_folder.clone();
        let cached: Vec<PathBuf> = self.thumbnails.borrow().keys().cloned().collect();

        self.gallery_generation.set(generation);

        // A duração sai do cabeçalho de cada MP4 e a miniatura do Explorer: com muitos clipes
        // isso leva segundos, e a janela abre na hora com o que já tem.
        std::thread::spawn(move || {
            shell::lower_thread_priority();

            let clips = gallery::list(&folder);
            let missing: Vec<PathBuf> = clips.iter().filter(|clip| !cached.contains(&clip.path)).map(|clip| clip.path.clone()).collect();

            later(move |app| app.gallery_listed(generation, clips));

            let _ = unsafe {
                windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED)
            };

            for path in missing {
                if let Some((width, height, pixels)) = shell::thumbnail(&path, 480, 270) {
                    let buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&pixels, width, height);

                    later(move |app| app.thumbnail_ready(path, buffer));
                }
            }
        });
    }

    fn gallery_listed(&self, generation: u64, clips: Vec<gallery::Clip>) {
        if generation != self.gallery_generation.get() {
            return;
        }

        *self.all_clips.borrow_mut() = clips;
        self.show_clips();
    }

    /// Os chips de jogo, com quantos clipes cada um tem, e os cards do jogo escolhido.
    fn show_clips(&self) {
        let all = self.all_clips.borrow();
        let mut games: Vec<(String, usize)> = Vec::new();

        for clip in all.iter() {
            match games.iter_mut().find(|(game, _)| *game == clip.game) {
                Some((_, count)) => *count += 1,
                None => games.push((clip.game.clone(), 1)),
            }
        }

        // O jogo escolhido some quando o último clipe dele é apagado: volta para "Todos".
        let mut chosen = self.game.borrow_mut();

        if chosen.as_ref().is_some_and(|chosen| !games.iter().any(|(game, _)| game == chosen)) {
            *chosen = None;
        }

        let clips: Vec<gallery::Clip> =
            all.iter().filter(|clip| chosen.as_ref().is_none_or(|chosen| clip.game == *chosen)).cloned().collect();
        let labels: Vec<SharedString> = if all.is_empty() {
            Vec::new()
        } else {
            std::iter::once(format!("Todos · {}", all.len()))
                .chain(games.iter().map(|(game, count)| format!("{} · {count}", gallery::game_label(game))))
                .map(Into::into)
                .collect()
        };
        let selected = chosen.as_ref().and_then(|chosen| games.iter().position(|(game, _)| game == chosen)).map_or(0, |index| index + 1);

        self.ui().set_games(ModelRc::from(Rc::new(VecModel::from(labels))));
        self.ui().set_game_index(selected as i32);
        *self.games.borrow_mut() = games.into_iter().map(|(game, _)| game).collect();

        let thumbnails = self.thumbnails.borrow();
        let items: Vec<ClipItem> = clips
            .iter()
            .map(|clip| ClipItem {
                title: clip.title.as_str().into(),
                date: clip.date.as_str().into(),
                details: clip.details.as_str().into(),
                thumbnail: thumbnails.get(&clip.path).cloned().unwrap_or_default(),
            })
            .collect();

        drop(thumbnails);

        // A confirmação de apagar guarda a posição do card: a lista nova pode ter mudado a ordem.
        let pending = self.ui().get_confirm_delete();

        if pending >= 0 {
            let path = self.clips.borrow().get(pending as usize).map(|clip| clip.path.clone());

            self.ui().set_confirm_delete(path.and_then(|path| clips.iter().position(|clip| clip.path == path)).map_or(-1, |index| index as i32));
        }

        self.clip_model.set_vec(items);
        *self.clips.borrow_mut() = clips;
    }

    fn thumbnail_ready(&self, path: PathBuf, buffer: SharedPixelBuffer<slint::Rgba8Pixel>) {
        let image = slint::Image::from_rgba8(buffer);

        self.thumbnails.borrow_mut().insert(path.clone(), image.clone());

        // O card pelo caminho: o chip de jogo pode ter mudado a ordem desde que a miniatura foi pedida.
        let Some(index) = self.clips.borrow().iter().position(|clip| clip.path == path) else { return };

        if let Some(mut item) = self.clip_model.row_data(index) {
            item.thumbnail = image;
            self.clip_model.set_row_data(index, item);
        }
    }

    fn open_clip(&self, index: usize) {
        let Some((path, title)) = self.clips.borrow().get(index).map(|clip| (clip.path.clone(), clip.title.clone())) else {
            return;
        };
        let Some(parent) = shell::window_handle(self.window.window()) else { return };

        self.close_player();

        match Player::open(parent, &path) {
            Ok(player) => {
                player.set_volume(f64::from(self.ui().get_player_volume()) / 100.0);
                *self.player.borrow_mut() = Some((player, path));
                self.ui().set_player_title(title.into());
                self.ui().set_page(1);
                self.player_timer.start(slint::TimerMode::Repeated, Duration::from_millis(100), || {
                    with_app(|app| app.tick_player());
                });
            }
            Err(error) => {
                tracing::error!(error = %format!("{error:#}"), "player: não abriu o clipe");
                toast::show("Não deu para abrir o clipe", "O arquivo pode estar corrompido ou aberto em outro programa.");
            }
        }
    }

    /// Acompanha o retângulo do vídeo (a janela pode ter mudado de tamanho) e o andamento.
    fn tick_player(&self) {
        let player = self.player.borrow();
        let Some((player, _)) = player.as_ref() else { return };
        let window = self.ui();
        let scale = self.window.window().scale_factor();

        player.place(
            (window.get_video_x() * scale) as i32,
            (window.get_video_y() * scale) as i32,
            (window.get_video_width() * scale) as i32,
            (window.get_video_height() * scale) as i32,
        );

        let (position, duration) = player.position();
        let clock = |seconds: f64| {
            let seconds = if seconds.is_finite() { seconds.max(0.0) as u64 } else { 0 };

            format!("{}:{:02}", seconds / 60, seconds % 60)
        };

        window.set_player_progress(if duration.is_finite() && duration > 0.0 { (position / duration) as f32 } else { 0.0 });
        window.set_player_time(format!("{} / {}", clock(position), clock(duration)).into());
        window.set_player_playing(player.is_playing());
    }

    fn close_player(&self) {
        self.player_timer.stop();
        self.player.borrow_mut().take();
    }

    /// "Procurar atualização": a pessoa pediu, então o que chegar instala na hora.
    fn delete_clip(&self, index: usize) {
        let Some(path) = self.clips.borrow().get(index).map(|clip| clip.path.clone()) else { return };

        if self.player.borrow().as_ref().is_some_and(|(_, open)| *open == path) {
            self.close_player();
            self.ui().set_page(0);
        }

        match shell::move_to_recycle_bin(&path) {
            Ok(()) => {
                self.thumbnails.borrow_mut().remove(&path);
                // O card sai na hora, sem esperar a leitura da pasta.
                self.all_clips.borrow_mut().retain(|clip| clip.path != path);
                self.show_clips();
                self.refresh_gallery();
            }
            Err(error) => {
                tracing::error!(error = %error, "galeria: não apagou o clipe");
                toast::show("Não deu para apagar o clipe", "Ele pode estar aberto em outro programa.");
            }
        }
    }
}
