//! A ponte entre o clique e o núcleo.
//!
//! O Slint desenha numa thread só e não fala `async`; o núcleo é `async` e não pode
//! desenhar. Aqui o trabalho vai para o runtime do Tokio e o resultado volta pela fila do
//! laço de eventos da janela — sem isso, um `GET` lento congelaria a tela inteira.
//!
//! Nada aqui decide: tudo que é decisão (o código vale? onde se cai ao sair? quem está na
//! sala?) é chamada ao `core_app`.

use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use core_app::api::{Api, HttpError};
use core_app::app::EntryRefusal;
use core_app::models::{Channel, ChannelKind, RoomIdentity, ServerSummary, User};
use core_app::protocol::local;
use core_app::reconnect::Backoff;
use core_app::session::Session;
use core_app::{App, Failure, Screen};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel, Weak};
use storage::Storage;
use tokio::runtime::Runtime;

use crate::devices::{self, Device};
use crate::{AppWindow, ChannelRow, DeviceRow, MemberRow, MessageRow, PeerRow, ServerRow, Ui};

const DEFAULT_SERVER: &str = "https://unkvoid.com";

/// ponytail: o app desenha a sala e fala com o SFU, mas ainda não captura nem toca mídia —
/// `shared/capture` e `shared/media` não estão ligados aqui. Teto: tela, câmera e som só
/// existem como botão. A saída é portar o `sending`/`streaming`/`watching` do `apps/linux`
/// com o encoder do Windows, e aí estes três botões passam a chamar o que eles publicam.
const NO_CAPTURE: &str = "A captura ainda não está ligada nesta versão do app.";

pub struct Bridge {
    runtime: Runtime,
    core: Arc<App>,
    api: Arc<Api>,
    window: Weak<AppWindow>,
    /// De onde sai o WebSocket: vem do `GET /api/config`, e até ele responder não há sala.
    sfu: Arc<Mutex<Option<String>>>,
    session: Arc<Mutex<Option<Arc<Session>>>>,
    /// O que a tela mostra por índice, e o que o servidor conhece por identificador.
    servers: Arc<Mutex<Vec<ServerSummary>>>,
    channels: Arc<Mutex<Vec<Channel>>>,
    reading: Arc<Mutex<Option<String>>>,
    /// O que o popover mostrou por último, para o índice clicado virar um aparelho.
    microphones: Arc<Mutex<Vec<Device>>>,
    speakers: Arc<Mutex<Vec<Device>>>,
    /// ponytail: o aparelho escolhido só vive nesta sessão, porque ainda não há captura
    /// para consumi-lo. A saída é guardá-lo no `storage` quando ela existir.
    chosen: Arc<Mutex<(Option<String>, Option<String>)>>,
}

impl Bridge {
    pub fn new(window: Weak<AppWindow>) -> anyhow::Result<Rc<Self>> {
        let storage = Storage::open()?;
        let server = std::env::var("UNKVOID_SERVER").unwrap_or_else(|_| DEFAULT_SERVER.to_owned());

        Ok(Rc::new(Self {
            runtime: Runtime::new()?,
            core: Arc::new(App::new(storage)),
            api: Arc::new(Api::new(&server)?),
            window,
            sfu: Arc::default(),
            session: Arc::default(),
            servers: Arc::default(),
            channels: Arc::default(),
            reading: Arc::default(),
            microphones: Arc::default(),
            speakers: Arc::default(),
            chosen: Arc::default(),
        }))
    }

    /// Cada clique da tela, ligado ao que ele faz. É o único lugar que conhece os dois
    /// lados: daqui para baixo só há núcleo, e daqui para cima só há desenho.
    pub fn wire(self: &Rc<Self>, window: &AppWindow) {
        let ui = window.global::<Ui>();

        ui.set_saved_name(self.core.state().name.into());

        ui.on_create_room({
            let bridge = self.clone();

            move |name, code| bridge.enter(bridge.core.create_room(&name, &code), None)
        });

        ui.on_join_room({
            let bridge = self.clone();

            move |name, code| bridge.enter(bridge.core.join_room(&name, &code), None)
        });

        ui.on_sign_in({
            let bridge = self.clone();

            move |email, password, register| bridge.sign_in(&email, &password, register)
        });

        // ponytail: entrar pelo Google é o navegador do sistema e a volta por
        // `unkvoid://login?token=&state=`, que hoje só existe no app do Tauri. Teto: o
        // botão explica em vez de abrir. A saída é esse fluxo subir para o `shared/core`,
        // onde o macOS vai precisar dele igual.
        ui.on_google_sign_in({
            let window = self.window.clone();

            move || {
                complain(&window, "Entrar com o Google ainda não funciona aqui. Use e-mail e senha.")
            }
        });

        ui.on_sign_out({
            let bridge = self.clone();

            move || bridge.sign_out()
        });

        ui.on_clear_login_error({
            let window = self.window.clone();

            move |field| {
                let field = field.to_string();

                paint(&window, move |app| {
                    let ui = app.global::<Ui>();

                    if field == "email" {
                        ui.set_login_email_error(SharedString::new());
                    } else {
                        ui.set_login_password_error(SharedString::new());
                    }

                    ui.set_login_error(SharedString::new());
                });
            }
        });

        ui.on_show_entry({
            let bridge = self.clone();

            move || bridge.show(Screen::Entry)
        });

        ui.on_show_home({
            let bridge = self.clone();

            move || bridge.show(bridge.core.home())
        });

        ui.on_open_server({
            let bridge = self.clone();

            move |index| bridge.open_server(index)
        });

        ui.on_open_channel({
            let bridge = self.clone();

            move |index| bridge.open_channel(index)
        });

        ui.on_send_message({
            let bridge = self.clone();

            move |body| bridge.send_message(&body)
        });

        ui.on_leave_room({
            let bridge = self.clone();

            move || bridge.leave_room()
        });

        ui.on_toggle_share({
            let window = self.window.clone();

            move || complain(&window, NO_CAPTURE)
        });

        ui.on_toggle_camera({
            let window = self.window.clone();

            move || complain(&window, NO_CAPTURE)
        });

        ui.on_toggle_mic({
            let window = self.window.clone();

            move || {
                paint(&window, |app| {
                    let ui = app.global::<Ui>();

                    ui.set_mic_on(!ui.get_mic_on());
                });
            }
        });

        ui.on_toggle_deafen({
            let window = self.window.clone();

            move || {
                paint(&window, |app| {
                    let ui = app.global::<Ui>();

                    ui.set_deafened(!ui.get_deafened());
                });
            }
        });

        ui.on_list_microphones({
            let bridge = self.clone();

            move || bridge.show_devices(true)
        });

        ui.on_list_speakers({
            let bridge = self.clone();

            move || bridge.show_devices(false)
        });

        ui.on_use_microphone({
            let bridge = self.clone();

            move |index| bridge.choose(true, index)
        });

        ui.on_use_speaker({
            let bridge = self.clone();

            move |index| bridge.choose(false, index)
        });
    }

    /// A abertura: o servidor responde? Onde fica o SFU? O token guardado ainda vale?
    pub fn start(self: &Rc<Self>) {
        let (core, api, window) = (self.core.clone(), self.api.clone(), self.window.clone());
        let (sfu, servers) = (self.sfu.clone(), self.servers.clone());

        self.spawn(async move {
            let mut backoff = Backoff::default();

            while !api.reachable().await {
                let Some(wait) = backoff.next_delay() else {
                    show(&window, Screen::Offline, "O servidor não respondeu.".to_owned());

                    return;
                };

                show(
                    &window,
                    Screen::Updating,
                    format!(
                        "Sem resposta do servidor. Tentando de novo… (tentativa {})",
                        backoff.attempt
                    ),
                );

                tokio::time::sleep(wait).await;
            }

            match api.config().await {
                Ok(config) => *lock(&sfu) = Some(config.sfu),
                Err(failure) => complain(&window, said(&failure)),
            }

            let user = match core.token() {
                Some(token) => {
                    api.set_token(Some(token));

                    match api.me().await {
                        Ok(user) => Some(user),
                        Err(failure) => {
                            tracing::info!(?failure, "o token guardado não vale mais");
                            api.set_token(None);
                            core.set_token(None);

                            None
                        }
                    }
                }
                None => None,
            };

            landed(&core, &api, &window, &servers, user).await;
        });
    }

    fn sign_in(self: &Rc<Self>, email: &str, password: &str, register: bool) {
        let (core, api, window) = (self.core.clone(), self.api.clone(), self.window.clone());
        let (email, password) = (email.to_owned(), password.to_owned());
        let servers = self.servers.clone();

        paint(&self.window, |app| app.global::<Ui>().set_login_busy(true));

        self.spawn(async move {
            let device = device_name();
            let attempt = if register {
                api.register(&email, &password, &device).await
            } else {
                api.login(&email, &password, &device).await
            };

            paint(&window, |app| app.global::<Ui>().set_login_busy(false));

            match attempt {
                Ok(answer) => {
                    core.set_token(Some(&answer.token));

                    let user = match answer.user {
                        Some(user) => Some(user),
                        None => api.me().await.ok(),
                    };

                    landed(&core, &api, &window, &servers, user).await;
                }
                Err(failure) => refuse_login(&window, &failure),
            }
        });
    }

    fn sign_out(self: &Rc<Self>) {
        self.core.set_token(None);
        self.api.set_token(None);

        let window = self.window.clone();
        let landing = self.core.home();

        lock(&self.servers).clear();

        paint(&window, move |app| {
            let ui = app.global::<Ui>();

            ui.set_signed_in(false);
            ui.set_user_name(SharedString::new());
            ui.set_user_initial(SharedString::new());
            ui.set_servers(ModelRc::default());
            ui.set_channels(ModelRc::default());
            ui.set_members(ModelRc::default());
            ui.set_messages(ModelRc::default());
            ui.set_screen(named(landing).into());
        });
    }

    fn show(self: &Rc<Self>, screen: Screen) {
        // O núcleo é quem guarda em que tela o app está: escrever nos dois no mesmo lugar é
        // o que os impede de discordar.
        self.core.show(screen);

        paint(&self.window, move |app| app.global::<Ui>().set_screen(named(screen).into()));
    }

    fn open_server(self: &Rc<Self>, index: i32) {
        let Some(server) = at(&self.servers, index) else {
            return;
        };

        let (api, window) = (self.api.clone(), self.window.clone());
        let (channels, reading) = (self.channels.clone(), self.reading.clone());
        let known = self.servers.clone();
        let id = server.id;

        self.spawn(async move {
            match api.tree(id).await {
                Ok(tree) => {
                    let ordered = tree.ordered_channels();
                    let members: Vec<MemberRow> = tree
                        .members
                        .iter()
                        .map(|member| {
                            let name =
                                member.nickname.clone().unwrap_or_else(|| member.name.clone());

                            MemberRow {
                                initial: initial(&name),
                                name: name.into(),
                                note: if member.is_owner { "dono" } else { "" }.into(),
                            }
                        })
                        .collect();

                    let rows: Vec<ChannelRow> = ordered
                        .iter()
                        .map(|channel| ChannelRow {
                            name: channel.name.clone().into(),
                            voice: channel.kind == ChannelKind::Voice,
                            current: false,
                        })
                        .collect();

                    *lock(&channels) = ordered;
                    *lock(&reading) = None;

                    let chosen = lock(&known).iter().position(|known| known.id == id);
                    let servers = rows_of(&lock(&known), chosen);
                    let name = tree.name.clone();

                    paint(&window, move |app| {
                        let ui = app.global::<Ui>();

                        ui.set_server_name(name.into());
                        ui.set_servers(model(servers));
                        ui.set_channels(model(rows));
                        ui.set_members(model(members));
                        ui.set_messages(ModelRc::default());
                        ui.set_channel_name(SharedString::new());
                    });
                }
                Err(failure) => complain(&window, said(&failure)),
            }
        });
    }

    fn open_channel(self: &Rc<Self>, index: i32) {
        let Some(channel) = at(&self.channels, index) else {
            return;
        };

        // Compartilhar tela só existe dentro de um canal de voz, e entrar nele é a mesma
        // sala do código — com o token de 60 s no lugar do nome.
        if channel.kind == ChannelKind::Voice {
            self.enter(Ok(channel.id.clone()), Some(channel.id.clone()));

            return;
        }

        *lock(&self.reading) = Some(channel.id.clone());

        let (api, window) = (self.api.clone(), self.window.clone());
        let (id, name) = (channel.id.clone(), channel.name.clone());
        let chosen = index as usize;

        paint(&window, move |app| {
            let ui = app.global::<Ui>();
            let rows: Vec<ChannelRow> = ui
                .get_channels()
                .iter()
                .enumerate()
                .map(|(position, mut row)| {
                    row.current = position == chosen;

                    row
                })
                .collect();

            ui.set_channels(model(rows));
            ui.set_channel_name(format!("# {name}").into());
        });

        self.spawn(async move {
            read_channel(&api, &window, &id).await;
        });
    }

    fn send_message(self: &Rc<Self>, body: &str) {
        let Some(channel) = lock(&self.reading).clone() else {
            complain(&self.window, "Escolha um canal de texto primeiro.");

            return;
        };

        if body.trim().is_empty() {
            return;
        }

        let (api, window, body) = (self.api.clone(), self.window.clone(), body.to_owned());

        self.spawn(async move {
            if let Err(failure) = api.send_message(&channel, &body).await {
                complain(&window, said(&failure));

                return;
            }

            // Reler o canal em vez de emendar a mensagem na lista: o que aparece é o que o
            // servidor gravou, e não o que este app achou que mandou.
            read_channel(&api, &window, &channel).await;
        });
    }

    fn enter(self: &Rc<Self>, opened: Result<String, EntryRefusal>, voice: Option<String>) {
        let room = match opened {
            Ok(room) => room,
            Err(refusal) => {
                complain(&self.window, refused(refusal));

                return;
            }
        };

        let Some(url) = lock(&self.sfu).clone() else {
            complain(&self.window, "O servidor ainda não disse onde fica o SFU.");

            return;
        };

        let window = self.window.clone();
        let held = self.session.clone();
        let identity = self.identity(&room, voice);

        paint(&window, |app| app.global::<Ui>().set_entry_busy(true));

        self.spawn(async move {
            let joined = Session::join(&url, &room, identity).await;

            paint(&window, |app| app.global::<Ui>().set_entry_busy(false));

            let (session, mut events) = match joined {
                Ok(joined) => joined,
                Err(failure) => {
                    let reason = sentence(Failure::from_error(&failure));

                    complain(&window, format!("Não deu para entrar na sala. {reason}"));

                    return;
                }
            };

            *lock(&held) = Some(session.clone());

            let (code, can_speak) = (room.clone(), session.can("speak"));
            let peers = peer_rows(&session);

            paint(&window, move |app| {
                let ui = app.global::<Ui>();

                ui.set_complaint(SharedString::new());
                ui.set_room_code(code.into());
                ui.set_can_speak(can_speak);
                ui.set_peers(model(peers));
                ui.set_screen("room".into());
            });

            // O socket fechado encerra a fila, e é aí que este laço termina.
            while let Some(event) = events.recv().await {
                let changed = session.apply(&event);

                match event.name.as_str() {
                    local::SESSION_LOST => complain(&window, "A sala caiu. Voltando…"),
                    local::SESSION_REJOINED => complain(&window, ""),
                    local::SESSION_GONE => complain(
                        &window,
                        "A sala não voltou. Entre de novo quando a internet estabilizar.",
                    ),
                    _ => {}
                }

                if changed {
                    let peers = peer_rows(&session);

                    paint(&window, move |app| app.global::<Ui>().set_peers(model(peers)));
                }
            }
        });
    }

    /// Quem esta pessoa é para o SFU, **perguntado de novo a cada entrada**: o token de voz
    /// vale 60 s, e guardar o primeiro faria toda reconexão levar um token vencido.
    fn identity(self: &Rc<Self>, room: &str, voice: Option<String>) -> core_app::Identity {
        let (api, core) = (self.api.clone(), self.core.clone());
        let room = room.to_owned();

        Arc::new(move || {
            let (api, core, room, voice) = (api.clone(), core.clone(), room.clone(), voice.clone());

            Box::pin(async move {
                let identity = if let Some(channel) = voice {
                    RoomIdentity::Account { token: asked(api.voice_token(&channel).await)? }
                } else if api.signed_in() {
                    RoomIdentity::Account { token: asked(api.room_token(&room).await)? }
                } else {
                    RoomIdentity::Guest {
                        room: room.clone(),
                        name: core.state().name,
                        install_id: core.install_id(),
                    }
                };

                Ok(identity)
            })
        })
    }

    fn leave_room(self: &Rc<Self>) {
        self.core.leave_room();

        let (window, landing) = (self.window.clone(), self.core.home());
        let held = lock(&self.session).take();

        self.spawn(async move {
            if let Some(session) = held
                && let Err(failure) = session.leave().await
            {
                tracing::warn!(%failure, "a saída da sala não foi confirmada");
            }

            paint(&window, move |app| {
                let ui = app.global::<Ui>();

                ui.set_peers(ModelRc::default());
                ui.set_room_code(SharedString::new());
                ui.set_complaint(SharedString::new());
                ui.set_screen(named(landing).into());
            });
        });
    }

    /// A lista é lida na hora de abrir: aparelho ligado depois que o app abriu tem de
    /// aparecer sem reiniciar nada. Ler o WASAPI bloqueia, e por isso não é na thread da
    /// tela.
    fn show_devices(self: &Rc<Self>, microphone: bool) {
        let (window, chosen) = (self.window.clone(), self.chosen.clone());
        let known = self.listed(microphone);

        self.spawn(async move {
            let found = if microphone { devices::microphones() } else { devices::speakers() };
            let picked = {
                let held = lock(&chosen);

                if microphone { held.0.clone() } else { held.1.clone() }
            };

            let rows: Vec<DeviceRow> = found
                .iter()
                .map(|device| DeviceRow {
                    label: device.label.clone().into(),
                    current: match &picked {
                        Some(id) => *id == device.id,
                        None => device.default,
                    },
                })
                .collect();

            *lock(&known) = found;

            paint(&window, move |app| {
                let ui = app.global::<Ui>();

                if microphone {
                    ui.set_microphones(model(rows));
                } else {
                    ui.set_speakers(model(rows));
                }
            });
        });
    }

    fn choose(self: &Rc<Self>, microphone: bool, index: i32) {
        let Some(device) = at(&self.listed(microphone), index) else {
            return;
        };

        let mut held = lock(&self.chosen);

        if microphone {
            held.0 = Some(device.id);
        } else {
            held.1 = Some(device.id);
        }

        tracing::info!(microphone, label = %device.label, "aparelho escolhido");
    }

    fn listed(&self, microphone: bool) -> Arc<Mutex<Vec<Device>>> {
        if microphone { self.microphones.clone() } else { self.speakers.clone() }
    }

    fn spawn<F>(&self, work: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.runtime.spawn(work);
    }
}

/// Um `Mutex` envenenado aqui é uma tarefa que caiu no meio de uma troca de sessão. O app
/// continuar com a sala que tem é melhor do que fechar a janela na cara de quem está nela.
fn lock<T>(cell: &Arc<Mutex<T>>) -> MutexGuard<'_, T> {
    cell.lock().unwrap_or_else(PoisonError::into_inner)
}

fn at<T: Clone>(cell: &Arc<Mutex<Vec<T>>>, index: i32) -> Option<T> {
    usize::try_from(index).ok().and_then(|index| lock(cell).get(index).cloned())
}

/// O caminho de volta para a tela. Toda novidade passa por aqui porque a janela só aceita
/// ser mexida na thread dela.
fn paint(window: &Weak<AppWindow>, work: impl FnOnce(&AppWindow) + Send + 'static) {
    if let Err(failure) = window.upgrade_in_event_loop(move |window| work(&window)) {
        tracing::warn!(%failure, "a janela não recebeu a novidade");
    }
}

fn complain(window: &Weak<AppWindow>, message: impl Into<String>) {
    let message = message.into();

    paint(window, move |app| app.global::<Ui>().set_complaint(message.into()));
}

fn show(window: &Weak<AppWindow>, screen: Screen, status: String) {
    paint(window, move |app| {
        let ui = app.global::<Ui>();

        ui.set_status(status.into());
        ui.set_screen(named(screen).into());
    });
}

/// Onde o app cai quando o servidor responde: com conta, nos servidores; sem conta, no
/// código. Quem decide é o núcleo.
async fn landed(
    core: &Arc<App>,
    api: &Arc<Api>,
    window: &Weak<AppWindow>,
    known: &Arc<Mutex<Vec<ServerSummary>>>,
    user: Option<User>,
) {
    let landing = core.home();
    let name = user.as_ref().map(|user| user.name.clone()).unwrap_or_default();
    let signed_in = user.is_some();
    let saved = core.state().name;

    core.show(landing);

    paint(window, move |app| {
        let ui = app.global::<Ui>();

        ui.set_signed_in(signed_in);
        ui.set_user_initial(initial(&name));
        ui.set_user_name(name.into());
        ui.set_saved_name(saved.into());
        ui.set_login_error(SharedString::new());
        ui.set_screen(named(landing).into());
    });

    if landing == Screen::Hub {
        match api.servers().await {
            Ok(servers) => {
                let rows = rows_of(&servers, None);

                *lock(known) = servers;

                paint(window, move |app| app.global::<Ui>().set_servers(model(rows)));
            }
            Err(failure) => complain(window, said(&failure)),
        }
    }
}

async fn read_channel(api: &Arc<Api>, window: &Weak<AppWindow>, channel: &str) {
    match api.messages(channel).await {
        Ok(messages) => {
            let rows: Vec<MessageRow> = messages
                .iter()
                .map(|message| MessageRow {
                    initial: initial(&message.user.name),
                    author: message.user.name.clone().into(),
                    body: message.body.clone().into(),
                    at: message.created_at.get(11..16).unwrap_or_default().into(),
                })
                .collect();

            paint(window, move |app| app.global::<Ui>().set_messages(model(rows)));
        }
        Err(failure) => complain(window, said(&failure)),
    }
}

fn rows_of(servers: &[ServerSummary], chosen: Option<usize>) -> Vec<ServerRow> {
    servers
        .iter()
        .enumerate()
        .map(|(index, server)| ServerRow {
            initial: initial(&server.name),
            name: server.name.clone().into(),
            current: Some(index) == chosen,
        })
        .collect()
}

fn peer_rows(session: &Arc<Session>) -> Vec<PeerRow> {
    session
        .peers()
        .iter()
        .map(|peer| PeerRow {
            initial: initial(&peer.name),
            name: peer.name.clone().into(),
            note: if peer.reconnecting {
                "voltando"
            } else if peer.sharing() {
                "transmitindo"
            } else {
                ""
            }
            .into(),
            mine: peer.self_peer,
        })
        .collect()
}

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows))
}

/// A inicial do avatar. O Slint não recorta string: quem sabe onde um caractere começa e
/// termina é o Rust.
fn initial(name: &str) -> SharedString {
    name.chars().next().map(|letter| letter.to_uppercase().to_string()).unwrap_or_default().into()
}

/// O nome de cada tela. A tradução mora num lugar só para uma tela nova não poder ser
/// esquecida aqui.
fn named(screen: Screen) -> &'static str {
    match screen {
        Screen::Entry => "entry",
        Screen::Hub => "hub",
        Screen::Room => "room",
        Screen::Offline => "offline",
        Screen::Updating => "updating",
    }
}

/// O erro de validação pinta o campo que o causou: a frase solta no rodapé, longe do que
/// foi digitado, não diz o que consertar.
fn refuse_login(window: &Weak<AppWindow>, error: &HttpError) {
    let (field, message) = match error {
        HttpError::Invalid { field, message } => (field.clone(), message.clone()),
        HttpError::Failed(failure) => (String::new(), sentence(*failure).to_owned()),
    };

    paint(window, move |app| {
        let ui = app.global::<Ui>();

        ui.set_login_email_error(SharedString::new());
        ui.set_login_password_error(SharedString::new());
        ui.set_login_error(SharedString::new());

        match field.as_str() {
            "email" => ui.set_login_email_error(message.into()),
            "password" => ui.set_login_password_error(message.into()),
            _ => ui.set_login_error(message.into()),
        }
    });
}

/// O motivo por trás de uma falha de chamada, para ele atravessar um `anyhow` sem virar
/// "confira a sua internet" no caminho.
fn asked<T>(answer: Result<T, HttpError>) -> anyhow::Result<T> {
    answer.map_err(|error| match error {
        HttpError::Failed(failure) => anyhow::Error::new(failure),
        HttpError::Invalid { .. } => anyhow::Error::new(Failure::Invalid),
    })
}

/// A frase é sempre da interface: o núcleo diz o motivo, e só o erro de validação do
/// Laravel chega pronto para ser lido.
fn said(error: &HttpError) -> String {
    match error {
        HttpError::Invalid { message, .. } => message.clone(),
        HttpError::Failed(failure) => sentence(*failure).to_owned(),
    }
}

/// Um motivo, uma frase. Motivo novo no núcleo para de compilar aqui — que é o ponto.
fn sentence(failure: Failure) -> &'static str {
    match failure {
        Failure::Unreachable => "Não deu para falar com o servidor. Confira a sua internet.",
        Failure::SignedOut => "Sua sessão terminou. Entre de novo.",
        Failure::NotAllowed => "Você não tem permissão para isso.",
        Failure::Gone => "Isso não existe mais.",
        Failure::Invalid => "O que foi digitado não serve.",
        Failure::ServerBroke => "O servidor teve um problema. Tente de novo em instantes.",
        Failure::TooFast => "Tentativas demais. Espere um pouco e tente de novo.",
    }
}

/// A frase da recusa é da interface: o núcleo diz o motivo, não o texto.
fn refused(refusal: EntryRefusal) -> &'static str {
    match refusal {
        EntryRefusal::NameIsEmpty => "Escolha um nome primeiro.",
        EntryRefusal::CodeIsInvalid => {
            "Use 3–32 caracteres: letras, números e hífens (sem hífen no começo ou fim)."
        }
    }
}

fn device_name() -> String {
    std::env::var("COMPUTERNAME").map(|host| format!("windows-{host}")).unwrap_or("windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_failure_reaches_the_screen_as_a_status_code_or_an_address() {
        // O dono não quer caminho, endereço nem número na janela: isso é log.
        for failure in [
            Failure::Unreachable,
            Failure::SignedOut,
            Failure::NotAllowed,
            Failure::Gone,
            Failure::Invalid,
            Failure::ServerBroke,
            Failure::TooFast,
        ] {
            let written = sentence(failure);

            assert!(!written.is_empty());
            assert!(!written.contains("http"), "vazou endereço: {written}");
            assert!(
                !written.chars().any(|letter| letter.is_ascii_digit()),
                "vazou número: {written}"
            );
        }
    }

    #[test]
    fn every_refusal_has_a_sentence_the_person_can_act_on() {
        for refusal in [EntryRefusal::NameIsEmpty, EntryRefusal::CodeIsInvalid] {
            assert!(!refused(refusal).is_empty());
        }
    }

    #[test]
    fn the_initial_is_a_whole_letter() {
        // Cortar por byte partiria o "Ã" ao meio, e o avatar mostraria lixo.
        assert_eq!(initial("Ângela"), "Â");
        assert_eq!(initial(""), "");
    }

    #[test]
    fn every_screen_has_a_name_the_window_knows() {
        for screen in
            [Screen::Entry, Screen::Hub, Screen::Room, Screen::Offline, Screen::Updating]
        {
            assert!(!named(screen).is_empty());
        }
    }
}
