//! A voz: entrar num canal, o microfone, ouvir quem está lá e o que a sala avisa.
//!
//! Tudo aqui roda fora da thread da janela. O estado que a tela desenha mora em
//! [`CallState`], atrás de um `Mutex`; a janela lê uma cópia por quadro ([`Calls::view`]) e é
//! acordada pelo [`Courier`] quando algo muda. Quem decide o que a sala faz é o `Room` do núcleo;
//! aqui só se liga a sala ao som do sistema.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use core_app::App as Core;
use core_app::api::{Api, HttpError};
use core_app::chimes::{Chime, SOUNDS_KEY};
use core_app::models::{RoomIdentity, VoicePerson};
use core_app::room::{Mine, Room};
use core_app::sharing::InputMode;
use core_app::{Failure, Identity};

use crate::backend::{Courier, Event};
use crate::camera::OwnCamera;
use crate::sound::{Microphone, Speaker};
use crate::stage::{Placed, Stage, Tile, Voice, peers_of};
use crate::watching::{Picture, Watch};
use crate::words::sentence;

/// A preferência de voz guardada (`unkvoid:voice`), a mesma chave do app de antes.
const VOICE_KEY: &str = "unkvoid:voice";

/// O canal de voz em que se está, como a barra de baixo o mostra. Na sala por código, `channel`
/// e `name` são o código e `guest` é o nome com que o visitante entra, sem conta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    pub channel: String,
    pub name: String,
    pub server: i64,
    pub guest: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Connected,
    Reconnecting,
}

const PING_HISTORY_WINDOW: std::time::Duration = std::time::Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PingSample {
    pub at: Instant,
    pub milliseconds: u64,
}

#[derive(Debug, Default, Clone)]
pub struct PingHistory {
    pub samples: VecDeque<PingSample>,
}

impl PingHistory {
    fn record(&mut self, at: Instant, milliseconds: u64) {
        while self
            .samples
            .front()
            .is_some_and(|sample| at.saturating_duration_since(sample.at) > PING_HISTORY_WINDOW)
        {
            self.samples.pop_front();
        }

        self.samples.push_back(PingSample { at, milliseconds });
    }

    pub fn visible_since(&self, since: Instant) -> impl Iterator<Item = PingSample> + '_ {
        self.samples
            .iter()
            .copied()
            .filter(move |sample| sample.at >= since)
    }

    pub fn average_ms(&self) -> Option<u64> {
        let count = u128::try_from(self.samples.len()).ok()?;
        let sum: u128 = self
            .samples
            .iter()
            .map(|sample| u128::from(sample.milliseconds))
            .sum();

        u64::try_from((sum + count / 2) / count).ok()
    }

    pub fn latest_ms(&self) -> Option<u64> {
        self.samples.back().map(|sample| sample.milliseconds)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PingQuality {
    Good,
    Fair,
    Poor,
    Bad,
}

pub fn ping_quality(ping: Option<u64>) -> PingQuality {
    match ping {
        Some(milliseconds) if milliseconds < 100 => PingQuality::Good,
        Some(milliseconds) if milliseconds < 200 => PingQuality::Fair,
        Some(milliseconds) if milliseconds < 300 => PingQuality::Poor,
        Some(_) => PingQuality::Bad,
        None => PingQuality::Good,
    }
}

/// O que a tela desenha da voz: onde se está, se o microfone está mudo, se está surdo, quem
/// fala agora e o ping.
#[derive(Debug, Clone)]
pub struct CallView {
    pub seat: Option<Seat>,
    pub entered_at: Option<Instant>,
    pub status: Status,
    pub ping: Option<u64>,
    pub ping_history: PingHistory,
    pub sfu_host: Option<String>,
    pub muted: bool,
    pub deafened: bool,
    pub can_speak: bool,
    /// Quem fala agora, pelo id da conta. Eu mesmo incluído.
    pub speaking: HashSet<i64>,
    /// A própria pessoa está falando agora, pelo nível do microfone dela: o app junta o id dela
    /// em `speaking`, sem depender de achar o próprio peer na sala.
    pub talking: bool,
    /// O que esta pessoa manda e pode mandar: a tela, a câmera e as permissões delas.
    pub mine: Mine,
    /// Os vídeos assistidos (telas e câmeras) e as transmissões no ar ainda não abertas.
    pub tiles: Vec<Placed>,
    pub pending: Vec<Tile>,
    /// A conta de cada participante da sala, pelo peer: o vídeo vai para o cartão da pessoa.
    /// Visitante da sala por código entra pela chave do [`Voice::key_of`].
    pub accounts: HashMap<String, i64>,
    /// A própria pessoa no palco, pela mesma chave: sem conta, é por aqui que o cartão é o meu.
    pub me: Option<i64>,
    /// Quem o SFU conta na sala: a lista do palco na sala por código.
    pub people: Vec<VoicePerson>,
}

#[derive(Default)]
struct CallState {
    voice: Voice,
    seat: Option<Seat>,
    entered_at: Option<Instant>,
    status: Option<Status>,
    ping: Option<u64>,
    ping_history: PingHistory,
    /// O mudo de antes de ensurdecer: desensurdecer o devolve, como no Discord.
    muted_before_deafen: bool,
    /// O último nível do próprio microfone na sala, de 0 a 100.
    level: u8,
    stage: Stage,
}

/// O que uma voz aberta segura: a sala, o que se ouve e vê, e o microfone.
struct Call {
    room: Arc<Room>,
    watch: Option<Watch>,
    microphone: Option<Microphone>,
}

impl Call {
    /// Solta o microfone e o que se ouve numa thread à parte: juntar as threads das telas e do
    /// som pode esperar o fim de um quadro 4K, e a janela não pode congelar por isso.
    async fn hang_up(self) {
        let Self {
            room,
            watch,
            microphone,
        } = self;

        std::thread::spawn(move || drop((microphone, watch)));
        room.leave().await;
    }
}

#[derive(Clone)]
pub struct Calls {
    runtime: tokio::runtime::Handle,
    api: Arc<Api>,
    core: Arc<Core>,
    sfu: Arc<Mutex<Option<String>>>,
    courier: Courier,
    state: Arc<Mutex<CallState>>,
    call: Arc<Mutex<Option<Call>>>,
    /// O microfone de teste da tela de Voz e vídeo, fora de uma chamada, e o nível dele.
    meter: Arc<Mutex<Option<Microphone>>>,
    meter_level: Arc<AtomicU8>,
    soundboard_last: Arc<Mutex<Option<Instant>>>,
    camera: OwnCamera,
}

impl Calls {
    pub fn new(
        runtime: tokio::runtime::Handle,
        api: Arc<Api>,
        core: Arc<Core>,
        sfu: Arc<Mutex<Option<String>>>,
        courier: Courier,
    ) -> Self {
        let calls = Self {
            runtime,
            api,
            core,
            sfu,
            courier,
            state: Arc::default(),
            call: Arc::default(),
            meter: Arc::default(),
            meter_level: Arc::default(),
            soundboard_last: Arc::default(),
            camera: OwnCamera::default(),
        };

        lock(&calls.state).voice.mute_on_join =
            calls.preference()["muteOnJoin"].as_bool().unwrap_or(false);

        calls
    }

    pub fn view(&self) -> CallView {
        let sfu_host = lock(&self.sfu).as_deref().map(sfu_host);
        let state = lock(&self.state);
        let mut speaking: HashSet<i64> = state
            .voice
            .peers
            .iter()
            .filter(|peer| !peer.self_peer && state.voice.is_speaking(peer))
            .map(Voice::key_of)
            .collect();
        let me = state
            .voice
            .peers
            .iter()
            .find(|peer| peer.self_peer)
            .map(Voice::key_of);

        if state.voice.speaking_myself()
            && let Some(me) = me
        {
            speaking.insert(me);
        }

        CallView {
            mine: state.voice.mine,
            tiles: state.stage.placed(),
            pending: state.stage.pending().to_vec(),
            accounts: state
                .voice
                .peers
                .iter()
                .map(|peer| (peer.peer_id.clone(), Voice::key_of(peer)))
                .collect(),
            me,
            people: state.voice.people(),
            talking: state.voice.speaking_myself(),
            seat: state.seat.clone(),
            entered_at: state.entered_at,
            status: state.status.unwrap_or(Status::Connecting),
            ping: state.ping,
            ping_history: state.ping_history.clone(),
            sfu_host,
            muted: state.voice.mic_shown_off() || state.voice.deafened,
            deafened: state.voice.deafened,
            can_speak: !state.voice.inside || state.voice.mine.can_speak,
            speaking,
        }
    }

    /// Entrar num canal de voz **sem sair do servidor**: o clique vale na hora — a barra diz
    /// "Conectando…" enquanto o token, o SFU e o microfone acontecem por trás.
    pub fn join(&self, seat: Seat) {
        if lock(&self.state)
            .seat
            .as_ref()
            .is_some_and(|current| current.channel == seat.channel)
        {
            return;
        }

        let previous = lock(&self.call).take();

        {
            let mut state = lock(&self.state);

            state.voice.leave();
            state.voice.opening = true;
            state.seat = Some(seat.clone());
            state.entered_at = None;
            state.status = Some(Status::Connecting);
            state.ping = None;
            state.ping_history = PingHistory::default();
        }

        self.courier.send(Event::Voice);

        let calls = self.clone();

        self.runtime.spawn(async move {
            if let Some(previous) = previous {
                previous.hang_up().await;
            }

            calls.enter(seat).await;
        });
    }

    async fn enter(&self, seat: Seat) {
        let Some(url) = lock(&self.sfu).clone() else {
            return self.fail(
                &seat,
                "O servidor ainda não disse onde fica a voz. Tente de novo em instantes.".into(),
            );
        };
        let (updates, heard) = std::sync::mpsc::channel();
        let identity = match &seat.guest {
            Some(name) => guest(seat.channel.clone(), name.clone(), self.core.install_id()),
            None => identity(self.api.clone(), seat.channel.clone()),
        };
        let attempt = Room::enter(&url, &seat.channel, identity, updates).await;

        // Outro canal foi pedido, ou a pessoa saiu, enquanto este esperava o servidor: quem
        // manda agora é o outro, e este se despede.
        if lock(&self.state).seat.as_ref() != Some(&seat) {
            if let Ok((room, _)) = attempt {
                room.leave().await;
            }

            return;
        }

        let (room, media) = match attempt {
            Ok(entered) => entered,
            Err(failure) => {
                tracing::warn!(failure = %format!("{failure:#}"), "voz: a entrada falhou");

                return self.fail(
                    &seat,
                    format!(
                        "Não deu para entrar na voz. {}",
                        sentence(Failure::from_error(&failure))
                    ),
                );
            }
        };

        let deafened = {
            let mut state = lock(&self.state);

            state.voice.inside = true;
            state.voice.mine = serde_json::from_value(room.mine()).unwrap_or_default();
            state.voice.peers = peers_of(&room.peers());
            state.status = Some(Status::Connected);
            state.entered_at = Some(Instant::now());
            state.voice.deafened
        };

        // Quem ensurdeceu fora da voz entra surdo: a sala nova nasce ouvindo.
        if deafened {
            room.deafen(true);
        }

        let speaker = Speaker::start(self.chosen("speaker"));
        speaker.set_output_volume(self.output_volume());
        let watch = Watch::start(
            media,
            Arc::new(speaker),
            {
                let calls = self.clone();

                move |producer, speaking| {
                    {
                        let mut state = lock(&calls.state);

                        if speaking {
                            state.voice.speaking.insert(producer.to_owned());
                        } else {
                            state.voice.speaking.remove(producer);
                        }
                    }

                    calls.courier.send(Event::Voice);
                }
            },
            {
                let courier = self.courier.clone();

                move || courier.send(Event::Voice)
            },
            {
                let room = Arc::downgrade(&room);

                move |producer: &str| {
                    if let Some(room) = room.upgrade() {
                        room.request_keyframe(producer);
                    }
                }
            },
        );

        *lock(&self.call) = Some(Call {
            room: room.clone(),
            watch: Some(watch),
            microphone: None,
        });
        listen(heard, self.clone(), Arc::downgrade(&room));
        self.chime(Chime::Joined);

        // Entrar na voz abre o microfone, como no Discord: quem entra já é ouvido.
        self.open_microphone(&room).await;

        // A entrada nova nasce `false/false` no SFU: o estado de verdade vai logo em seguida.
        let (muted, deafened) = {
            let state = lock(&self.state);

            (state.voice.mic_shown_off() || state.voice.deafened, state.voice.deafened)
        };

        room.voice_state(muted, deafened).await;
        lock(&self.state).voice.opening = false;
        self.courier.send(Event::Voice);
    }

    /// Quem estava mudo fora da voz, ou marcou "Silenciar ao entrar", entra mudo.
    async fn open_microphone(&self, room: &Arc<Room>) {
        let (can_speak, muted, code_room) = {
            let state = lock(&self.state);

            (
                state.voice.mine.can_speak,
                state.voice.muted_at_rest || state.voice.mute_on_join || state.voice.deafened,
                state.seat.as_ref().is_some_and(|seat| seat.guest.is_some()),
            )
        };

        // A sala por código é só para compartilhar a tela: lá não há microfone.
        if !can_speak || code_room {
            return;
        }

        if let Err(failure) = room.open_microphone(self.core.voice_filter_config()).await {
            tracing::warn!(%failure, "voz: a sala não abriu o microfone");
            self.courier
                .send(Event::Complaint("Não deu para abrir o microfone.".into()));

            return;
        }

        let speaking = room.clone();
        let device = self.chosen("microphone");
        let started = tokio::task::block_in_place(|| {
            Microphone::start(device, move |samples| speaking.speak(samples))
        });

        match started {
            Ok(microphone) => {
                room.set_input_mode(self.input_mode());

                if muted {
                    room.mute_microphone(true).await;
                }

                if let Some(call) = lock(&self.call).as_mut() {
                    call.microphone = Some(microphone);
                }
            }
            Err(failure) => {
                tracing::warn!(failure = %format!("{failure:#}"), "voz: o microfone do sistema não abriu");
                room.close_microphone().await;
                self.courier
                    .send(Event::Complaint("Não deu para abrir o microfone.".into()));
            }
        }
    }

    fn fail(&self, seat: &Seat, said: String) {
        {
            let mut state = lock(&self.state);

            if state.seat.as_ref() == Some(seat) {
                state.seat = None;
                state.entered_at = None;
                state.status = None;
                state.ping_history = PingHistory::default();
                state.voice.leave();
            }
        }

        self.courier.send(Event::Complaint(said));
        self.courier.send(Event::Voice);
    }

    /// Sai da voz e continua no servidor: o fone cortado da barra.
    pub fn leave(&self) {
        self.close(true);
    }

    fn close(&self, chime: bool) {
        let held = lock(&self.call).take();
        let was_inside = {
            let mut state = lock(&self.state);
            let was_inside = state.seat.is_some();

            state.voice.leave();
            state.seat = None;
            state.entered_at = None;
            state.status = None;
            state.ping = None;
            state.ping_history = PingHistory::default();
            state.stage.clear();

            was_inside
        };

        self.camera.send_to(None);
        self.camera.close();

        if chime && was_inside {
            self.chime(Chime::Disconnected);
        }

        self.courier.send(Event::Voice);

        if let Some(call) = held {
            self.runtime.spawn(call.hang_up());
        }
    }

    /// O microfone da barra. Fora da voz guarda o mudo para a próxima; dentro, alterna o mudo
    /// do microfone aberto. Desmutar quem está surdo tira o surdo junto, como no Discord.
    pub fn toggle_mute(&self) {
        let retry = {
            let mut state = lock(&self.state);
            let retry = state.voice.inside
                && state.voice.mine.can_speak
                && !state.voice.mine.mic
                && !state.voice.opening;

            if retry {
                state.voice.muted_at_rest = false;
            }

            retry
        };

        // O microfone que não abriu na entrada (aparelho ocupado, a cifra ainda não pronta):
        // desmutar tenta abrir de novo, como no Discord. Antes só trocava o mudo de fora da
        // chamada, que ali não aparece, e o botão parecia travado.
        if retry && let Some(room) = self.room() {
            let calls = self.clone();

            self.runtime.spawn(async move {
                calls.open_microphone(&room).await;

                let deafened = lock(&calls.state).voice.deafened;

                room.voice_state(deafened, deafened).await;
                calls.courier.send(Event::Voice);
            });

            return;
        }

        let (room, muted_now) = {
            let mut state = lock(&self.state);
            let room = self
                .room()
                .filter(|_| state.voice.inside && state.voice.mine.mic);

            if state.voice.deafened {
                state.voice.deafened = false;

                if let Some(room) = &room {
                    room.deafen(false);
                }
            }

            match room {
                Some(room) => {
                    let muted = !state.voice.mine.mic_muted;

                    (Some(room), muted)
                }
                None => {
                    state.voice.muted_at_rest = !state.voice.muted_at_rest;

                    (None, state.voice.muted_at_rest)
                }
            }
        };

        if let Some(room) = room {
            self.runtime.spawn(async move {
                room.mute_microphone(muted_now).await;
                // Desmutar tira o surdo junto (acima): os dois vão juntos para quem vê a lista.
                room.voice_state(muted_now, false).await;
            });
        }

        self.chime(if muted_now {
            Chime::Muted
        } else {
            Chime::Unmuted
        });
        self.courier.send(Event::Voice);
    }

    /// Ensurdecer cala o que chega e o microfone junto; desensurdecer devolve o mudo de antes.
    /// Vale fora da voz também: quem entra surdo continua surdo.
    pub fn toggle_deafen(&self) {
        let (room, deafened, mute) = {
            let mut state = lock(&self.state);

            state.voice.deafened = !state.voice.deafened;

            let mute = if state.voice.deafened {
                state.muted_before_deafen = state.voice.mine.mic_muted || state.voice.muted_at_rest;

                true
            } else {
                state.muted_before_deafen
            };

            (
                self.room().filter(|_| state.voice.inside),
                state.voice.deafened,
                mute,
            )
        };

        if let Some(room) = room {
            room.deafen(deafened);
            self.runtime.spawn(async move {
                room.mute_microphone(mute).await;
                room.voice_state(mute, deafened).await;
            });
        }

        // O toque de ensurdecer toca mesmo surdo: é ele que confirma o clique.
        self.play(if deafened {
            Chime::Deafened
        } else {
            Chime::Undeafened
        });
        self.courier.send(Event::Voice);
    }

    fn room(&self) -> Option<Arc<Room>> {
        lock(&self.call).as_ref().map(|call| call.room.clone())
    }

    pub fn camera(&self) -> &OwnCamera {
        &self.camera
    }

    /// O último quadro de cada vídeo assistido que a janela ainda não desenhou.
    pub fn pictures(&self) -> Vec<(String, Picture)> {
        lock(&self.call)
            .as_ref()
            .and_then(|call| call.watch.as_ref())
            .map(Watch::fresh)
            .unwrap_or_default()
    }

    /// Os números de cada tela assistida desde a última pergunta (quadros, altura, bytes), com os
    /// contadores de pacotes da sala — esses são acumulados, a diferença fica com quem pergunta.
    #[allow(clippy::type_complexity)]
    pub fn stream_numbers(&self) -> Vec<(String, (u32, u32, u64), Option<media::Counters>)> {
        let call = lock(&self.call);
        let Some(Call { room, watch: Some(watch), .. }) = call.as_ref() else {
            return Vec::new();
        };

        watch
            .drawn()
            .into_iter()
            .map(|(producer, numbers)| {
                let counters = room.counters(&producer);

                (producer, numbers, counters)
            })
            .collect()
    }

    /// Começa a transmitir, ou troca o que está no ar. Mesma escolha de áudio: troca a tela, a
    /// resolução e os quadros sem derrubar a transmissão; outra escolha de áudio recomeça.
    pub fn share(&self, config: capture::CaptureConfig) {
        let Some(room) = self.room() else {
            return;
        };
        let calls = self.clone();

        self.runtime.spawn(async move {
            let live = room.sharing_recipe();
            let result = match live {
                Some(live) if live.capture_audio == config.capture_audio => {
                    room.change_quality(config.quality, config.frame_rate, Some(config.source))
                        .await
                }
                Some(_) => {
                    room.stop_sharing().await;
                    room.share(config).await
                }
                None => room.share(config).await,
            };

            if let Err(failure) = result {
                tracing::warn!(failure = %format!("{failure:#}"), "transmissão: não começou");
                calls.courier.send(Event::Complaint(format!(
                    "Não deu para transmitir. {}",
                    sentence(Failure::from_error(&failure))
                )));
            }

            calls.courier.send(Event::Voice);
        });
    }

    pub fn stop_sharing(&self) {
        let Some(room) = self.room() else {
            return;
        };
        let courier = self.courier.clone();

        self.runtime.spawn(async move {
            room.stop_sharing().await;
            courier.send(Event::Voice);
        });
    }

    /// Liga a câmera na chamada. No Windows a webcam abre aqui e cada quadro vai à sala pelo
    /// `OwnCamera`; o encoder abre no tamanho que ela entrega. No Linux a captura do núcleo
    /// abre a câmera sozinha.
    pub fn start_camera(&self, index: u32, context: &egui::Context) {
        let Some(room) = self.room() else {
            return;
        };

        // Fora do Windows a webcam não abre por aqui, e a falha é esperada: a captura abre.
        if let Err(failure) = self.camera.open(index, context)
            && crate::camera::PREVIEWS
        {
            tracing::warn!(%failure, "câmera: a webcam não abriu");
            self.courier
                .send(Event::Complaint("Não deu para abrir a câmera.".into()));

            return;
        }

        let calls = self.clone();

        self.runtime.spawn(async move {
            let opened = if crate::camera::PREVIEWS {
                let mut size = calls.camera.size();

                for _ in 0..40 {
                    if size.is_some() {
                        break;
                    }

                    tokio::time::sleep(Duration::from_millis(50)).await;
                    size = calls.camera.size();
                }

                open_fed_camera(&room, size.unwrap_or(crate::camera::SIZE)).await
            } else {
                room.open_captured_camera(capture::CaptureConfig {
                    source: capture::CaptureSource::Camera(index),
                    quality: capture::Quality::Hd720,
                    frame_rate: crate::camera::FRAME_RATE,
                    capture_audio: false,
                    ..capture::CaptureConfig::default()
                })
                .await
            };

            match opened {
                Ok(()) => {
                    calls.camera.send_to(Some(room));
                    calls.chime(Chime::CameraOn);
                }
                Err(failure) => {
                    tracing::warn!(failure = %format!("{failure:#}"), "câmera: a sala não abriu a câmera");
                    calls.camera.close();
                    calls.courier.send(Event::Complaint(format!(
                        "Não deu para ligar a câmera. {}",
                        sentence(Failure::from_error(&failure))
                    )));
                }
            }

            calls.courier.send(Event::Voice);
        });
    }

    /// Desliga a câmera da chamada. `keep_open` deixa a webcam acesa para a prévia de Voz e
    /// vídeo, se ela estiver testando.
    pub fn stop_camera(&self, keep_open: bool) {
        self.camera.send_to(None);

        if !keep_open {
            self.camera.close();
        }

        let Some(room) = self.room() else {
            return;
        };
        let calls = self.clone();

        self.runtime.spawn(async move {
            close_fed_camera(&room).await;
            calls.chime(Chime::CameraOff);
            calls.courier.send(Event::Voice);
        });
    }

    /// Clicar num vídeo põe ele no foco; de novo, sai do foco.
    pub fn focus(&self, producer: &str) {
        lock(&self.state).stage.toggle_focus(producer);
        self.show_stage();
    }

    /// O botão de tela cheia de uma transmissão; de novo, sai.
    pub fn full(&self, producer: &str) {
        lock(&self.state).stage.toggle_full(producer);
        self.show_stage();
    }

    /// Conta ao núcleo o vídeo à vista — o da tela cheia, senão o do foco — para ele pausar no
    /// servidor os outros.
    fn show_stage(&self) {
        let shown = {
            let state = lock(&self.state);

            state.stage.full_producer().or_else(|| {
                state
                    .stage
                    .placed()
                    .into_iter()
                    .find(|placed| placed.focused)
                    .map(|placed| placed.tile.producer_id)
            })
        };

        if let Some(room) = self.room() {
            self.runtime.spawn(async move { room.set_focus(shown).await });
        }

        self.courier.send(Event::Voice);
    }

    /// O "Assistir" de uma transmissão no ar, ou o "Parar de assistir".
    pub fn watch_stream(&self, producer: &str, watching: bool) {
        let Some(room) = self.room() else {
            return;
        };

        // Fechar a tela cala o som dela: o cartão some, e com ele o alto-falante que o desligaria.
        let audio = lock(&self.state).stage.tile(producer).and_then(|tile| tile.audio.clone());

        if !watching && let Some(audio) = audio {
            room.mute_watched(&audio, true);
        }

        let producer = producer.to_owned();

        self.runtime.spawn(async move {
            if watching {
                room.watch(Some(&producer)).await;
            } else {
                room.close_watched(&producer).await;
            }
        });
    }

    /// O "Assistir à minha tela" do cartão da própria transmissão, ou o "Parar de assistir".
    pub fn self_view(&self, wanted: bool) {
        if let Some(room) = self.room() {
            self.runtime.spawn(async move { room.set_self_view(wanted).await });
        }
    }

    /// O alto-falante no cartão de uma tela. O som dela chega mudo: é aqui que ele toca.
    pub fn toggle_stream_sound(&self, producer: &str) {
        let toggled = {
            let mut state = lock(&self.state);

            state.stage.toggle_heard(producer).map(|(audio, heard)| (audio, state.stage.volume(producer), Some(heard)))
        };

        if let Some((audio, volume, heard)) = toggled {
            self.hear_stream(&audio, volume, heard);
        }
    }

    pub fn set_stream_volume(&self, producer: &str, volume: u8) {
        let changed = lock(&self.state).stage.set_volume(producer, volume);

        if let Some((audio, heard)) = changed {
            self.hear_stream(&audio, volume, heard);
        }
    }

    /// Põe o volume no som de uma tela e, se mudou, liga ou cala ele.
    fn hear_stream(&self, audio: &str, volume: u8, heard: Option<bool>) {
        if let Some(call) = lock(&self.call).as_ref() {
            if let Some(watch) = call.watch.as_ref() {
                watch.speaker().set_volume(audio, f32::from(volume) / 100.0);
            }

            if let Some(heard) = heard {
                call.room.mute_watched(audio, !heard);
            }
        }

        self.courier.send(Event::Voice);
    }

    /// Um toque da voz: calado quando se está surdo ou quando a pessoa desligou aquele som.
    pub fn chime(&self, chime: Chime) {
        if !lock(&self.state).voice.deafened {
            self.play(chime);
        }
    }

    /// Toca sem olhar o surdo (o próprio clique de ensurdecer), mas respeitando o som desligado.
    fn play(&self, chime: Chime) {
        if chime.allowed(self.core.preference(SOUNDS_KEY).as_ref()) {
            crate::sound::chime(
                self.chosen("speaker"),
                chime.samples(),
                self.output_volume(),
            );
        }
    }

    /// A prévia da tela de Sons toca mesmo com o som desligado: é para ouvir antes de decidir.
    pub fn preview(&self, chime: Chime) {
        crate::sound::chime(
            self.chosen("speaker"),
            chime.samples(),
            self.output_volume(),
        );
    }

    pub fn play_soundboard(&self, sound: core_app::chimes::SoundboardSound) -> bool {
        let now = Instant::now();
        let eligible = {
            let state = lock(&self.state);
            state.voice.inside
                && state.voice.mine.can_speak
                && !state.voice.deafened
                && !state.voice.mine.mic_muted
        };
        if !eligible
            || lock(&self.soundboard_last).is_some_and(|last| {
                now.saturating_duration_since(last) < std::time::Duration::from_secs(1)
            })
        {
            return false;
        }
        let samples = sound.samples();
        let held_call = lock(&self.call);
        let Some(call) = held_call.as_ref() else {
            return false;
        };
        if call.microphone.is_none() || !call.room.inject_voice_audio(&samples) {
            return false;
        }
        *lock(&self.soundboard_last) = Some(now);
        crate::sound::chime(
            self.chosen("speaker"),
            samples,
            self.preference()["soundboardVolume"]
                .as_u64()
                .unwrap_or(100)
                .min(100) as u16,
        );
        true
    }

    pub fn play_server_sound_samples(&self, samples: &[f32], sound_volume: u8) -> bool {
        let now = Instant::now();
        let eligible = {
            let state = lock(&self.state);
            state.voice.inside
                && state.voice.mine.can_speak
                && !state.voice.deafened
                && !state.voice.mine.mic_muted
        };
        if !eligible
            || lock(&self.soundboard_last)
                .is_some_and(|last| now.saturating_duration_since(last) < Duration::from_secs(1))
        {
            return false;
        }
        let scaled: Vec<f32> = samples
            .iter()
            .map(|sample| (sample * f32::from(sound_volume.min(100)) / 100.0).clamp(-1.0, 1.0))
            .collect();
        let held_call = lock(&self.call);
        let Some(call) = held_call.as_ref() else {
            return false;
        };
        if call.microphone.is_none() || !call.room.inject_voice_audio(&scaled) {
            return false;
        }
        *lock(&self.soundboard_last) = Some(now);
        let local_volume = self.preference()["soundboardVolume"]
            .as_u64()
            .unwrap_or(100)
            .min(100) as u16;
        crate::sound::chime(self.chosen("speaker"), scaled, local_volume);
        true
    }

    pub fn set_peer_volume(&self, user: i64, volume: u16) {
        let saved = store_peer_volume(&self.preference(), user, volume);
        self.core.set_preference(VOICE_KEY, saved.clone());
        self.apply_peer_volume(user, peer_volume(&saved, user));
    }

    pub fn toggle_peer_mute(&self, user: i64) {
        let saved = toggle_peer_volume(&self.preference(), user);
        self.core.set_preference(VOICE_KEY, saved.clone());
        self.apply_peer_volume(user, peer_volume(&saved, user));
    }

    fn apply_peer_volume(&self, user: i64, volume: u16) {
        let producer = lock(&self.state).voice.microphone_of(user);
        let Some(producer) = producer else { return };
        if let Some(watch) = lock(&self.call)
            .as_ref()
            .and_then(|call| call.watch.as_ref())
        {
            watch
                .speaker()
                .set_volume(&producer, f32::from(volume) / 100.0);
        }
    }

    fn apply_known_peer_volumes(&self) {
        let peers: Vec<_> = {
            let state = lock(&self.state);
            state
                .voice
                .peers
                .iter()
                .filter_map(|peer| {
                    Some((
                        Voice::user_of(peer)?,
                        peer.producers
                            .iter()
                            .find(|producer| producer.source == "mic")?
                            .producer_id
                            .clone(),
                    ))
                })
                .collect()
        };
        let saved = self.preference();
        if let Some(watch) = lock(&self.call)
            .as_ref()
            .and_then(|call| call.watch.as_ref())
        {
            for (user, producer) in peers {
                watch
                    .speaker()
                    .set_volume(&producer, f32::from(peer_volume(&saved, user)) / 100.0);
            }
        }
    }

    fn preference(&self) -> serde_json::Value {
        self.core
            .preference(VOICE_KEY)
            .unwrap_or(serde_json::Value::Null)
    }

    fn output_volume(&self) -> u16 {
        read_volume(&self.preference()["outputVolume"])
    }

    /// As escolhas de Voz e vídeo, como estão guardadas.
    pub fn settings(&self) -> serde_json::Value {
        self.preference()
    }

    /// O aparelho escolhido (`microphone` ou `speaker`); ausente é o padrão do sistema.
    fn chosen(&self, field: &str) -> Option<String> {
        self.preference()[field]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
    }

    /// Guarda um campo de `unkvoid:voice` e o aplica na hora, se há uma chamada aberta: o
    /// microfone e a saída trocam sem sair da voz, e o modo vale no próximo quadro.
    pub fn set(&self, field: &str, value: serde_json::Value) {
        let mut saved = self.preference();

        if !saved.is_object() {
            saved = serde_json::json!({});
        }

        saved[field] = value.clone();
        self.core.set_preference(VOICE_KEY, saved);

        match field {
            "muteOnJoin" => lock(&self.state).voice.mute_on_join = value.as_bool().unwrap_or(false),
            "mode" | "sensitivity" => {
                if let Some(room) = self.room() {
                    room.set_input_mode(self.input_mode());
                }
            }
            "speaker" => {
                if let Some(call) = lock(&self.call).as_ref()
                    && let Some(watch) = &call.watch
                {
                    watch.speaker().use_device(self.chosen("speaker"));
                }
            }
            "inputVolume" => {
                if let Some(room) = self.room() {
                    room.set_input_volume(read_volume(&value));
                }
            }
            "outputVolume" => {
                if let Some(call) = lock(&self.call).as_ref()
                    && let Some(watch) = &call.watch
                {
                    watch.speaker().set_output_volume(self.output_volume());
                }
            }
            "microphone" => {
                self.reopen_microphone();

                if lock(&self.meter).is_some() {
                    self.stop_meter();
                    self.start_meter();
                }
            }
            _ => {}
        }

        self.courier.send(Event::Voice);
    }

    /// O microfone escolhido no meio da chamada: o de antes fecha e o novo abre na mesma sala.
    fn reopen_microphone(&self) {
        let open = lock(&self.call)
            .as_ref()
            .is_some_and(|call| call.microphone.is_some());
        let Some(room) = self.room().filter(|_| open) else {
            return;
        };
        let (call, courier, device) = (
            self.call.clone(),
            self.courier.clone(),
            self.chosen("microphone"),
        );

        self.runtime.spawn(async move {
            if let Some(held) = lock(&call).as_mut() {
                drop(held.microphone.take());
            }

            let speaking = room.clone();

            match tokio::task::block_in_place(|| Microphone::start(device, move |samples| speaking.speak(samples))) {
                Ok(microphone) => {
                    if let Some(held) = lock(&call).as_mut() {
                        held.microphone = Some(microphone);
                    }
                }
                Err(failure) => {
                    tracing::warn!(failure = %format!("{failure:#}"), "voz: o microfone escolhido não abriu");
                    courier.send(Event::Complaint("Não deu para abrir o microfone escolhido.".into()));
                }
            }
        });
    }

    /// Os sons ligados e desligados (`unkvoid:sounds`).
    pub fn sounds(&self) -> serde_json::Value {
        self.core
            .preference(SOUNDS_KEY)
            .unwrap_or_else(|| serde_json::json!({}))
    }

    pub fn set_sound(&self, chime: Chime, on: bool) {
        let mut sounds = self.sounds();

        if !sounds.is_object() {
            sounds = serde_json::json!({});
        }

        sounds[chime.name()] = serde_json::Value::Bool(on);
        self.core.set_preference(SOUNDS_KEY, sounds);
    }

    /// O nível do microfone para o medidor, de 0 a 1: o da sala numa chamada, senão o do
    /// microfone de teste.
    pub fn level(&self) -> f32 {
        let state = lock(&self.state);

        if state.voice.inside {
            f32::from(state.level) / 100.0
        } else {
            f32::from(self.meter_level.load(Ordering::Relaxed)) / 100.0
        }
    }

    /// Abre o microfone de teste da tela de Voz e vídeo. Na chamada não precisa: o nível vem da
    /// sala, e abrir o mesmo aparelho duas vezes pode ser recusado.
    pub fn start_meter(&self) {
        if lock(&self.state).voice.inside || lock(&self.meter).is_some() {
            return;
        }

        let level = self.meter_level.clone();
        let courier = self.courier.clone();
        let opened = Microphone::start(self.chosen("microphone"), move |samples| {
            level.store(loudness(samples), Ordering::Relaxed);
            courier.send(Event::Voice);
        });

        match opened {
            Ok(microphone) => *lock(&self.meter) = Some(microphone),
            Err(failure) => {
                tracing::warn!(failure = %format!("{failure:#}"), "voz: o microfone de teste não abriu")
            }
        }
    }

    pub fn stop_meter(&self) {
        drop(lock(&self.meter).take());
        self.meter_level.store(0, Ordering::Relaxed);
    }

    /// O modo do microfone guardado: detecção de voz com a sensibilidade, apertar para falar,
    /// ou sempre aberto. É o núcleo (`InputMode`) que fecha e abre o microfone.
    fn input_mode(&self) -> InputMode {
        let voice = self.preference();

        InputMode::parse(
            voice["mode"].as_str().unwrap_or("open"),
            voice["sensitivity"].as_u64().unwrap_or(35),
        )
    }

    /// Um moderador moveu esta pessoa: a sala de antes já parou no núcleo; aqui se fecha o que
    /// ficou aberto, sem o toque de saída, e se entra no destino com token novo.
    fn moved(&self, data: &serde_json::Value) {
        let to = data["to"].as_str().unwrap_or_default().to_owned();
        let by = data["by"].as_str().unwrap_or("Um moderador").to_owned();
        let server = lock(&self.state).seat.as_ref().map(|seat| seat.server);
        let destination = server
            .and_then(|server| self.api.known_tree(server).map(|tree| (server, tree)))
            .and_then(|(server, tree)| {
                tree.channels
                    .iter()
                    .find(|channel| channel.id == to)
                    .map(|channel| (server, channel.name.clone()))
            });

        self.close(false);

        match destination {
            Some((server, name)) => {
                self.chime(Chime::Moved);
                self.courier
                    .send(Event::Notice(format!("{by} moveu você para {name}.")));
                self.join(Seat {
                    channel: to,
                    name,
                    server,
                    guest: None,
                });
            }
            None => self.courier.send(Event::Complaint(
                "Você foi movido para um canal que não enxerga.".into(),
            )),
        }
    }
}

/// Quem esta pessoa é para o SFU, **perguntado de novo a cada entrada**: o token de voz vale
/// 60 s, e guardar o primeiro faria toda reconexão levar um token vencido.
fn identity(api: Arc<Api>, channel: String) -> Identity {
    Arc::new(move || {
        let (api, channel) = (api.clone(), channel.clone());

        Box::pin(async move {
            let token = api
                .voice_token(&channel)
                .await
                .map_err(|error| match error {
                    HttpError::Failed(failure) => anyhow::Error::new(failure),
                    HttpError::Invalid { .. } => anyhow::Error::new(Failure::Invalid),
                })?;

            Ok(RoomIdentity::Account { token })
        })
    })
}

/// O visitante da sala por código: sem token, só o código, o nome e a chave desta instalação.
fn guest(room: String, name: String, install_id: String) -> Identity {
    Arc::new(move || {
        let identity = RoomIdentity::Guest {
            room: room.clone(),
            name: name.clone(),
            install_id: install_id.clone(),
        };

        Box::pin(async move { Ok(identity) })
    })
}

/// Os avisos da sala, numa thread só deles: cada um muda o estado guardado e acorda a janela.
/// A fila fecha quando a sala acaba, e a thread acaba junto.
/// A câmera que a janela alimenta quadro a quadro (Windows e macOS).
#[cfg(any(target_os = "macos", target_os = "windows"))]
async fn open_fed_camera(room: &Room, size: (u32, u32)) -> anyhow::Result<()> {
    room.open_camera(size, crate::camera::FRAME_RATE).await
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
async fn open_fed_camera(_room: &Room, _size: (u32, u32)) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("aqui a câmera abre pela captura do núcleo"))
}

async fn close_fed_camera(room: &Room) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    room.close_camera().await;

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    room.close_captured_camera().await;
}

fn listen(heard: Receiver<String>, calls: Calls, room: Weak<Room>) {
    let spawned = std::thread::Builder::new().name("unkvoid-voz".into()).spawn(move || {
        for said in heard {
            let Ok(update) = serde_json::from_str::<serde_json::Value>(&said) else {
                continue;
            };
            let data = &update["data"];

            match update["event"].as_str().unwrap_or_default() {
                "room.peers" => {
                    lock(&calls.state).voice.peers = peers_of(data);
                    calls.apply_known_peer_volumes();
                }
                "room.mine" => lock(&calls.state).voice.mine = serde_json::from_value(data.clone()).unwrap_or_default(),
                "room.tiles" => lock(&calls.state).stage.set_tiles(data),
                "room.watchers" => lock(&calls.state).stage.set_watchers(data),
                "room.level" => {
                    let percent = data["percent"].as_u64().map_or(0, |percent| u8::try_from(percent).unwrap_or(u8::MAX));
                    let mut state = lock(&calls.state);

                    state.level = percent;
                    state.voice.hear_myself(percent, std::time::Instant::now());
                }
                "room.ping" => {
                    if let Some(milliseconds) = data["ms"].as_u64() {
                        let mut state = lock(&calls.state);
                        let at = Instant::now();

                        state.ping = Some(milliseconds);
                        state.ping_history.record(at, milliseconds);
                    }
                }
                "room.session" => match data["state"].as_str().unwrap_or_default() {
                    "lost" => lock(&calls.state).status = Some(Status::Reconnecting),
                    "rejoined" => lock(&calls.state).status = Some(Status::Connected),
                    "moved" => calls.moved(data),
                    "gone" => {
                        calls.close(true);
                        calls.courier.send(Event::Complaint("A voz não voltou. Entre de novo quando a internet estabilizar.".into()));
                    }
                    "replaced" => {
                        calls.close(false);
                        calls.courier.send(Event::Complaint("Esta conta entrou na voz por outro lugar.".into()));
                    }
                    "kicked" => {
                        calls.close(false);
                        calls.courier.send(Event::Complaint("Você foi desconectado da voz.".into()));
                    }
                    _ => {}
                },
                "room.failed" => calls.courier.send(Event::Complaint(room_failure(data["what"].as_str().unwrap_or_default()).into())),
                "room.chime" => {
                    if let Ok(chime) = serde_json::from_value::<Chime>(data["chime"].clone()) {
                        calls.chime(chime);
                    }
                }
                "room.notice" => calls.courier.send(Event::Notice(data["text"].as_str().unwrap_or_default().to_owned())),
                _ => continue,
            }

            calls.courier.send(Event::Voice);

            if room.upgrade().is_none() {
                break;
            }
        }
    });

    if let Err(failure) = spawned {
        tracing::warn!(%failure, "voz: a thread dos avisos da sala não subiu");
    }
}

/// O nível de um bloco do microfone, de 0 a 100, pelo pico em decibéis: -60 dB é zero e 0 dB é
/// cem, a escala que o medidor do Discord mostra.
fn loudness(samples: &[f32]) -> u8 {
    let peak = samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));

    if peak <= 0.0 {
        return 0;
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let percent = ((20.0 * peak.log10() + 60.0) / 60.0 * 100.0).clamp(0.0, 100.0) as u8;

    percent
}

fn read_volume(value: &serde_json::Value) -> u16 {
    u16::try_from(value.as_u64().unwrap_or(100).clamp(0, 200)).unwrap_or(200)
}

pub fn peer_volume(preferences: &serde_json::Value, user: i64) -> u16 {
    u16::try_from(
        preferences["userVolumes"][user.to_string()]
            .as_u64()
            .unwrap_or(100)
            .clamp(0, 200),
    )
    .unwrap_or(200)
}

fn store_peer_volume(preferences: &serde_json::Value, user: i64, volume: u16) -> serde_json::Value {
    let mut preferences = preferences.as_object().cloned().unwrap_or_default();
    let mut volumes = preferences
        .get("userVolumes")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    volumes.insert(user.to_string(), serde_json::json!(volume.clamp(0, 200)));
    preferences.insert("userVolumes".to_owned(), serde_json::Value::Object(volumes));
    serde_json::Value::Object(preferences)
}

fn toggle_peer_volume(preferences: &serde_json::Value, user: i64) -> serde_json::Value {
    let key = user.to_string();
    let current = peer_volume(preferences, user);
    let mut preferences = preferences.as_object().cloned().unwrap_or_default();
    let mut volumes = preferences
        .get("userVolumes")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut remembered = preferences
        .get("mutedUserVolumes")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let next = if current == 0 {
        remembered
            .remove(&key)
            .and_then(|value| value.as_u64())
            .map_or(100, |value| {
                u16::try_from(value.clamp(1, 200)).unwrap_or(100)
            })
    } else {
        remembered.insert(key.clone(), serde_json::json!(current));
        0
    };
    volumes.insert(key, serde_json::json!(next));
    preferences.insert("userVolumes".to_owned(), serde_json::Value::Object(volumes));
    preferences.insert(
        "mutedUserVolumes".to_owned(),
        serde_json::Value::Object(remembered),
    );
    serde_json::Value::Object(preferences)
}

fn sfu_host(url: &str) -> String {
    let authority = url
        .split_once("://")
        .map_or(url, |(_, address)| address)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();

    if authority.starts_with('[') {
        authority
            .split_once(']')
            .map_or_else(|| authority.to_owned(), |(host, _)| format!("{host}]"))
    } else {
        authority.split(':').next().unwrap_or_default().to_owned()
    }
}

/// As falhas que a sala anuncia, na frase de cada uma.
fn room_failure(what: &str) -> &'static str {
    match what {
        "watch" => "Não deu para assistir a uma das transmissões.",
        "mic" => "Não deu para abrir o microfone.",
        "camera" => "Não deu para ligar a câmera.",
        "shareClosed" => "A janela que você compartilhava foi fechada, e a transmissão parou.",
        "serverMuted" => "Um moderador silenciou o seu microfone.",
        _ => "Não deu para compartilhar a tela.",
    }
}

fn lock<T>(cell: &Mutex<T>) -> MutexGuard<'_, T> {
    cell.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_user_volume_defaults_clamps_and_restores_after_local_mute() {
        let preferences = serde_json::Value::Null;
        assert_eq!(peer_volume(&preferences, 12), 100);
        let preferences = store_peer_volume(&preferences, 12, 250);
        assert_eq!(peer_volume(&preferences, 12), 200);
        let muted = toggle_peer_volume(&preferences, 12);
        assert_eq!(peer_volume(&muted, 12), 0);
        assert_eq!(peer_volume(&toggle_peer_volume(&muted, 12), 12), 200);
    }

    #[test]
    fn ping_history_keeps_five_minutes_and_computes_the_average() {
        let now = Instant::now();
        let mut history = PingHistory::default();
        history.record(
            now - PING_HISTORY_WINDOW - std::time::Duration::from_secs(1),
            900,
        );
        history.record(now - std::time::Duration::from_secs(240), 200);
        // A janela corre com a amostra mais nova, que no uso chega a cada segundo: é o "agora".
        history.record(now, 400);

        assert_eq!(history.samples.len(), 2);
        assert_eq!(history.average_ms(), Some(300));
        assert_eq!(history.latest_ms(), Some(400));
    }

    #[test]
    fn ping_quality_uses_the_measured_boundaries() {
        for (ping, expected) in [
            (None, PingQuality::Good),
            (Some(99), PingQuality::Good),
            (Some(100), PingQuality::Fair),
            (Some(199), PingQuality::Fair),
            (Some(200), PingQuality::Poor),
            (Some(299), PingQuality::Poor),
            (Some(300), PingQuality::Bad),
        ] {
            assert_eq!(ping_quality(ping), expected);
        }
    }

    #[test]
    fn the_meter_reads_the_peak_in_decibels() {
        assert_eq!(loudness(&[0.0; 960]), 0, "silêncio é zero");
        assert_eq!(loudness(&[1.0, -1.0]), 100, "o pico cheio é cem");
        assert_eq!(loudness(&[0.001]), 0, "-60 dB é zero");
        assert_eq!(loudness(&[-0.0316]), 49, "-30 dB fica no meio");
    }

    #[test]
    fn every_room_failure_has_a_sentence() {
        for what in [
            "watch",
            "mic",
            "camera",
            "shareClosed",
            "serverMuted",
            "share",
        ] {
            let said = room_failure(what);

            assert!(
                !said.is_empty() && !said.chars().any(|letter| letter.is_ascii_digit()),
                "{what}: {said}"
            );
        }
    }
}
