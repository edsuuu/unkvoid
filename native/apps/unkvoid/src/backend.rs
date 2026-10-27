//! O trabalho que espera a rede, fora da thread da janela.
//!
//! O egui desenha a cada quadro e não fala `async`; o núcleo é `async` e não desenha. Cada
//! pedido vai para o runtime do Tokio e a resposta volta como um [`Event`] na fila que a janela
//! esvazia no quadro seguinte — sem isso, um `GET` lento congelaria a tela inteira. Nada aqui
//! decide o que a tela mostra: isso é do `app.rs`.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use core_app::App as Core;
use core_app::activity::Activity;
use core_app::api::{Api, HttpError};
use core_app::models::{
    AuditEntry, ChannelKind, Conversation, DirectMessage, Friendship, InvitePreview, Message, Person, ServerInvite,
    Mutuals, ServerSound, ServerSummary, ServerTree, User,
};
use core_app::realtime::Realtime;
use core_app::failure::Failure;
use core_app::reconnect::{Backoff, MAX_ATTEMPTS};
use serde_json::Value;
use storage::Storage;
use tokio::runtime::Runtime;

const ACTIVITY_SHARING_KEY: &str = "unkvoid:activitySharing";
const ACTIVITY_POLL_SECONDS: u64 = 15;
const ACTIVITY_REFRESH_SECONDS: u64 = 45;
const HIDDEN_CONVERSATIONS_KEY: &str = "unkvoid:hiddenConversations";

use crate::system;
use crate::voice::Calls;
use crate::words::said;

/// O que volta da rede para a janela.
pub enum Event {
    /// A abertura ainda não terminou: o texto diz em que pé está.
    Status(String),
    /// O servidor não respondeu depois de todas as tentativas.
    Offline,
    /// A abertura ou o login terminou: com conta, a pessoa; sem conta, a entrada.
    Landed(Option<User>),
    LoginRefused(HttpError),
    /// A conta com o nome não nasceu: nome ou senha recusados, ou o servidor fora.
    StartRefused(HttpError),
    EmailAdded(Result<User, HttpError>),
    Servers(Vec<ServerSummary>),
    Tree(ServerTree),
    Messages {
        channel: String,
        messages: Vec<Message>,
    },
    ChannelRead(String),
    Older {
        channel: String,
        messages: Vec<Message>,
    },
    Conversations(Vec<Conversation>),
    Friends(Vec<Friendship>),
    Direct {
        person: Person,
        messages: Vec<DirectMessage>,
        mutuals: Mutuals,
    },
    DirectSent(i64),
    Renamed(Result<User, HttpError>),
    ServerInviteCreated { server: i64, invite: ServerInvite },
    ServerInvites { server: i64, invites: Vec<ServerInvite> },
    InvitePreview { code: String, preview: Result<InvitePreview, HttpError> },
    ServerIconPicked {
        path: PathBuf,
        bytes: Vec<u8>,
    },
    OpenServer(i64),
    /// Um erro que a pessoa precisa ler, já na frase dela.
    Complaint(String),
    /// Uma linha do tempo real, como o núcleo a entrega (`{event, channel, data}`).
    Live(String),
    SessionEnded,
    UpdateReady(String),
    /// Um aviso que não é erro, já na frase da pessoa.
    Notice(String),
    /// A voz mudou: a janela relê o `CallView`.
    Voice,
    /// Uma imagem pronta para ir junto da próxima mensagem: escolhida no "+" ou colada.
    Attached(PathBuf),
    /// As janelas e telas do seletor da transmissão, e depois a miniatura de cada uma.
    ShareSources(Vec<crate::app::ShareSource>),
    SharePreview(String, egui::ColorImage),
    Devices {
        microphones: Vec<crate::devices::Device>,
        speakers: Vec<crate::devices::Device>,
        cameras: Vec<capture::Camera>,
    },
    /// A conta relida depois de mudar: a foto nova.
    Me(User),
    Audits {
        server: i64,
        entries: Vec<AuditEntry>,
    },
    ServerSounds {
        server: i64,
        sounds: Vec<ServerSound>,
    },
    ServerSoundDecoded {
        server: i64,
        id: i64,
        volume: u8,
        samples: Arc<[f32]>,
    },
    ServerSoundPicked {
        server: i64,
        path: PathBuf,
    },
    ActivityChanged(Option<Activity>),
}

/// O que reler depois de uma escrita que deu certo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    /// A árvore do servidor: cargos, membros, canais e banimentos.
    Tree(i64),
    /// A árvore e o trilho: o nome e o ícone aparecem nos dois.
    Server(i64),
    /// O trilho: o servidor apagado sai dele.
    Servers,
    /// A conta: a foto nova.
    Me,
    Friends,
}

/// Quem leva o evento de volta e acorda a janela para ela o ler.
#[derive(Clone)]
pub struct Courier {
    events: Sender<Event>,
    context: egui::Context,
}

impl Courier {
    pub fn send(&self, event: Event) {
        if self.events.send(event).is_ok() {
            self.context.request_repaint();
        }
    }

    fn complain(&self, error: &HttpError) {
        self.send(Event::Complaint(said(error)));
    }
}

pub struct Backend {
    runtime: Runtime,
    core: Arc<Core>,
    api: Arc<Api>,
    courier: Courier,
    /// De onde sai o WebSocket: vem do `GET /api/config`.
    sfu: Arc<Mutex<Option<String>>>,
    /// O tempo real do chat e da presença, aberto enquanto há conta.
    live: Arc<Mutex<Option<Arc<Realtime>>>>,
    /// Os canais que o tempo real segue agora, fora o `user.{id}` da conta.
    followed: Arc<Mutex<BTreeSet<String>>>,
    /// A versão nova já baixada e conferida: o que o botão verde instala.
    installer: Arc<Mutex<Option<(PathBuf, String)>>>,
    calls: Calls,
    activity_monitor_stopped: Arc<AtomicBool>,
}

impl Backend {
    pub fn new(context: egui::Context) -> anyhow::Result<(Self, Receiver<Event>)> {
        let (events, inbox) = std::sync::mpsc::channel();
        let courier = Courier { events, context };
        let core = Arc::new(Core::new(Storage::open()?));
        let api = Arc::new(Api::new(&system::server())?);

        core.keep_session(&api, {
            let courier = courier.clone();

            move || courier.send(Event::SessionEnded)
        });

        let runtime = Runtime::new()?;
        let sfu: Arc<Mutex<Option<String>>> = Arc::default();
        let calls = Calls::new(
            runtime.handle().clone(),
            api.clone(),
            core.clone(),
            sfu.clone(),
            courier.clone(),
        );
        let activity_monitor_stopped = Arc::new(AtomicBool::new(false));
        let backend = Self {
            runtime,
            core,
            api,
            courier,
            sfu,
            live: Arc::default(),
            followed: Arc::default(),
            installer: Arc::default(),
            calls,
            activity_monitor_stopped: activity_monitor_stopped.clone(),
        };

        backend.start_activity_monitor();

        Ok((backend, inbox))
    }

    pub fn calls(&self) -> &Calls {
        &self.calls
    }

    /// A sala por código: a regra do código, do nome e das recentes mora no núcleo.
    pub fn rooms(&self) -> &Core {
        &self.core
    }

    pub fn activity_sharing_enabled(&self) -> bool {
        self.core
            .preference(ACTIVITY_SHARING_KEY)
            .and_then(|value| value.as_bool())
            .unwrap_or(true)
    }

    pub fn set_activity_sharing(&self, enabled: bool) {
        self.core
            .set_preference(ACTIVITY_SHARING_KEY, serde_json::json!(enabled));

        if !enabled {
            self.set_activity(None);
            self.courier.send(Event::ActivityChanged(None));
        }
    }

    pub fn hidden_conversations(&self) -> std::collections::HashSet<i64> {
        self.core
            .preference(HIDDEN_CONVERSATIONS_KEY)
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default()
    }

    pub fn set_hidden_conversations(&self, hidden: &std::collections::HashSet<i64>) {
        self.core.set_preference(HIDDEN_CONVERSATIONS_KEY, serde_json::json!(hidden));
    }

    fn set_activity(&self, activity: Option<Activity>) {
        let (api, courier, payload) = (
            self.api.clone(),
            self.courier.clone(),
            serde_json::json!({ "activity": activity }),
        );

        self.spawn(async move {
            if let Err(failure) = api.perform("setActivity", &serde_json::json!({}), &payload).await {
                courier.complain(&failure);
            }
        });
    }

    fn start_activity_monitor(&self) {
        let (core, api, courier, runtime, stopped) = (
            self.core.clone(),
            self.api.clone(),
            self.courier.clone(),
            self.runtime.handle().clone(),
            self.activity_monitor_stopped.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("unkvoid-activity-presence".into())
            .spawn(move || {
                let mut current_name: Option<String> = None;
                let mut current_activity: Option<Activity> = None;
                let mut last_refresh = std::time::Instant::now();

                while !stopped.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_secs(ACTIVITY_POLL_SECONDS));

                    let selected = if api.signed_in()
                        && core
                            .preference(ACTIVITY_SHARING_KEY)
                            .and_then(|value| value.as_bool())
                            .unwrap_or(true)
                    {
                        system::foreground_activity_name()
                    } else {
                        None
                    };
                    let changed = selected != current_name;

                    if changed {
                        current_name = selected.clone();
                        current_activity = selected.map(|name| Activity {
                            name,
                            started_at: chrono::Utc::now().to_rfc3339(),
                        });
                        courier.send(Event::ActivityChanged(current_activity.clone()));
                    }

                    let refresh_due = current_activity.is_some()
                        && last_refresh.elapsed()
                            >= std::time::Duration::from_secs(ACTIVITY_REFRESH_SECONDS);

                    if changed || refresh_due {
                        let activity = current_activity.clone();
                        let api = api.clone();

                        runtime.spawn(async move {
                            if let Err(failure) = api
                                .perform(
                                    "setActivity",
                                    &serde_json::json!({}),
                                    &serde_json::json!({ "activity": activity }),
                                )
                                .await
                            {
                                tracing::debug!(?failure, "presença: a atividade não foi atualizada");
                            }
                        });
                        last_refresh = std::time::Instant::now();
                    }
                }
            });

        if let Err(failure) = spawned {
            tracing::warn!(%failure, "presença: a thread de atividade não subiu");
        }
    }

    pub fn refresh_me(&self) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            match api.me().await {
                Ok(user) => courier.send(Event::Me(user)),
                Err(failure) => courier.complain(&failure),
            }
        });
    }

    pub fn send_voice_logs(&self) {
        let api = self.api.clone();
        let courier = self.courier.clone();

        self.spawn(async move {
            if system::report_errors_once(&api).await {
                courier.send(Event::Notice("Registros enviados.".to_owned()));
            } else {
                courier.send(Event::Complaint(
                    "Não foi possível enviar os registros agora.".to_owned(),
                ));
            }
        });
    }

    /// Os microfones e as saídas do sistema. Ler o WASAPI bloqueia: fora da thread da janela.
    pub fn devices(&self) {
        let courier = self.courier.clone();

        self.runtime.spawn_blocking(move || {
            courier.send(Event::Devices {
                microphones: crate::devices::microphones(),
                speakers: crate::devices::speakers(),
                cameras: capture::cameras(),
            });
        });
    }

    /// As telas e as janelas que dá para transmitir, e a miniatura de cada uma, uma por vez: o
    /// Windows leva até alguns décimos de segundo por janela, e a lista aparece antes delas.
    pub fn share_sources(&self) {
        // ponytail: miniatura só das primeiras janelas, sem atualizar enquanto o seletor está
        // aberto; o Discord refaz de segundo em segundo. Teto: a janela aberta depois de abrir
        // o seletor só aparece reabrindo.
        const MOST_PREVIEWS: usize = 16;

        let courier = self.courier.clone();

        self.runtime.spawn_blocking(move || {
            let displays = capture::PlatformCapturer::displays().unwrap_or_default();
            // A própria janela e o console dela não vão para a lista, como no Discord.
            let own = std::env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_default();
            let windows: Vec<capture::Window> = capture::PlatformCapturer::windows()
                .unwrap_or_default()
                .into_iter()
                .filter(|window| window.title != crate::NAME && window.title != own)
                .collect();
            let sources: Vec<crate::app::ShareSource> = displays
                .iter()
                .enumerate()
                .map(|(position, display)| crate::app::ShareSource {
                    key: format!("display:{}", display.id),
                    name: format!("Tela {}", position + 1),
                    screen: true,
                })
                .chain(windows.into_iter().map(|window| crate::app::ShareSource {
                    key: format!("window:{}", window.id),
                    name: if window.title.is_empty() {
                        window.application
                    } else {
                        window.title
                    },
                    screen: false,
                }))
                .collect();

            courier.send(Event::ShareSources(sources.clone()));

            for source in sources.iter().take(MOST_PREVIEWS + displays.len()) {
                let picked = core_app::sharing::capture_config(&serde_json::json!({ "source": source.key }));
                let Ok(jpeg) = capture::PlatformCapturer::preview(picked.source) else {
                    continue;
                };
                let Ok(decoded) = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg) else {
                    continue;
                };
                let small = decoded.thumbnail(448, 252).to_rgba8();
                let size = [small.width() as usize, small.height() as usize];

                courier.send(Event::SharePreview(
                    source.key.clone(),
                    egui::ColorImage::from_rgba_unmultiplied(size, small.as_raw()),
                ));
            }
        });
    }

    pub fn known_tree(&self, server: i64) -> Option<ServerTree> {
        self.api.known_tree(server)
    }

    fn spawn<F>(&self, work: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.runtime.spawn(work);
    }

    /// A abertura: o servidor responde? Há versão nova? Onde fica o SFU? O token guardado ainda
    /// vale? Terminada, o tempo real abre — com conta ou sem, para o aviso de versão nova.
    pub fn start(&self) {
        let (core, api, courier) = (self.core.clone(), self.api.clone(), self.courier.clone());
        let (sfu, live, installer) = (self.sfu.clone(), self.live.clone(), self.installer.clone());

        self.spawn({
            let api = self.api.clone();

            async move { system::report_errors(&api).await }
        });

        self.spawn(async move {
            let mut backoff = Backoff::default();

            while !api.reachable().await {
                let Some(wait) = backoff.next_delay() else {
                    courier.send(Event::Offline);

                    return;
                };

                courier.send(Event::Status(format!(
                    "Sem resposta do servidor. Tentando de novo… (tentativa {})",
                    backoff.attempt
                )));
                tokio::time::sleep(wait).await;
            }

            if system::updating(&api, &courier).await {
                return;
            }

            match api.config().await {
                Ok(config) => *lock(&sfu) = Some(config.sfu),
                Err(failure) => courier.complain(&failure),
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
            let account = user.as_ref().map(|user| user.id);

            courier.send(Event::Landed(user));
            go_live(&api, &sfu, &live, &courier, account).await;

            // Anunciada pelo servidor e ainda não instalada: o botão verde volta sem ninguém
            // perguntar ao site.
            if core_app::update::announced(&core).is_some() {
                prepare_update(&api, &courier, &installer).await;
            }
        });
    }

    pub fn sign_in(&self, email: String, password: String, register: bool) {
        let (core, api, courier) = (self.core.clone(), self.api.clone(), self.courier.clone());
        let (sfu, live) = (self.sfu.clone(), self.live.clone());

        self.spawn(async move {
            let device = system::device_name();
            let attempt = if register {
                api.register(&email, &password, &device).await
            } else {
                api.login(&email, &password, &device).await
            };

            match attempt {
                Ok(answer) => land(&core, &api, &sfu, &live, &courier, answer).await,
                Err(failure) => courier.send(Event::LoginRefused(failure)),
            }
        });
    }

    /// Entrar com o nome e uma senha: a conta nasce agora no servidor, e daí a entrada é a do
    /// login.
    pub fn start_with_name(&self, name: String, password: String) {
        let (core, api, courier) = (self.core.clone(), self.api.clone(), self.courier.clone());
        let (sfu, live) = (self.sfu.clone(), self.live.clone());

        self.spawn(async move {
            match api.start(&name, &password, &system::device_name()).await {
                Ok(answer) => land(&core, &api, &sfu, &live, &courier, answer).await,
                Err(failure) => courier.send(Event::StartRefused(failure)),
            }
        });
    }

    /// O e-mail para a conta que entrou com o nome.
    pub fn add_email(&self, email: String) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move { courier.send(Event::EmailAdded(api.add_email(&email).await)) });
    }

    /// Entrar com o Google: o navegador abre na conta do Google e o site devolve o token a uma
    /// porta local deste app. A pessoa tem cinco minutos; o e-mail e senha seguem livres.
    pub fn google_sign_in(&self) {
        let (core, api, courier) = (self.core.clone(), self.api.clone(), self.courier.clone());
        let (sfu, live) = (self.sfu.clone(), self.live.clone());

        self.spawn(async move {
            let login = match core_app::google::GoogleLogin::start(&system::server()).await {
                Ok(login) => login,
                Err(failure) => {
                    tracing::warn!(%failure, "google: a porta do retorno não abriu");
                    courier.send(Event::Complaint(
                        "Não deu para abrir o login do Google. Tente de novo.".into(),
                    ));

                    return;
                }
            };

            system::open_in_browser(&login.url);

            let Ok((token, refresh)) = login.wait().await else {
                courier.send(Event::Complaint(
                    "O login com o Google não voltou do navegador. Tente de novo.".into(),
                ));

                return;
            };

            core.set_token(Some(&token));
            api.adopt(&token, refresh.as_deref());

            let user = api.me().await.ok();

            arrive(&api, &sfu, &live, &courier, user).await;
        });
    }

    pub fn rename(&self, name: String) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move { courier.send(Event::Renamed(api.rename(&name).await)) });
    }

    /// Sair nunca depende da rede: a tela volta ao login na hora, e os tokens caem no servidor
    /// em segundo plano. Sem conta o tempo real reabre só para o aviso de versão nova.
    pub fn sign_out(&self) {
        self.set_activity(None);
        self.calls.leave();
        self.core.set_token(None);
        self.spawn(self.api.sign_out());

        if let Some(live) = lock(&self.live).take() {
            live.close();
        }

        lock(&self.followed).clear();

        let (api, sfu, live, courier) = (
            self.api.clone(),
            self.sfu.clone(),
            self.live.clone(),
            self.courier.clone(),
        );

        self.spawn(async move { go_live(&api, &sfu, &live, &courier, None).await });
    }

    pub fn servers(&self) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move { refresh_servers(&api, &courier).await });
    }

    pub fn tree(&self, server: i64) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            match api.tree(server).await {
                Ok(tree) => courier.send(Event::Tree(tree)),
                Err(failure) => courier.complain(&failure),
            }
        });
    }

    /// Recarrega os efeitos pertencentes ao servidor aberto.
    pub fn server_sounds(&self, server: i64) {
        let (api, courier) = (self.api.clone(), self.courier.clone());
        self.spawn(async move {
            let answer = match api
                .perform(
                    "serverSounds",
                    &serde_json::json!({ "server": server }),
                    &Value::Null,
                )
                .await
            {
                Ok(answer) => answer,
                Err(failure) => return courier.complain(&failure),
            };
            let sounds = answer.get("data").cloned().unwrap_or(answer);
            match serde_json::from_value::<Vec<ServerSound>>(sounds) {
                Ok(sounds) => courier.send(Event::ServerSounds { server, sounds }),
                Err(failure) => {
                    tracing::warn!(%failure, "a lista de sons do servidor veio inválida")
                }
            }
        });
    }

    pub fn server_invites(&self, server: i64) {
        let (api, courier) = (self.api.clone(), self.courier.clone());
        self.spawn(async move {
            let answer = match api.perform("invites", &serde_json::json!({ "server": server }), &Value::Null).await {
                Ok(answer) => answer,
                Err(failure) => return courier.complain(&failure),
            };
            let invites = answer.get("data").cloned().unwrap_or(answer);
            match serde_json::from_value::<Vec<ServerInvite>>(invites) {
                Ok(invites) => courier.send(Event::ServerInvites { server, invites }),
                Err(failure) => tracing::warn!(%failure, "a lista de convites veio inválida"),
            }
        });
    }

    pub fn create_server_invite(&self, server: i64, channel: Option<String>, max_age: i64, max_uses: i64) {
        let (api, courier) = (self.api.clone(), self.courier.clone());
        self.spawn(async move {
            let body = serde_json::json!({ "channel_id": channel, "max_age": max_age, "max_uses": max_uses });
            match api.perform("createInvite", &serde_json::json!({ "server": server }), &body).await {
                Ok(answer) => match serde_json::from_value::<ServerInvite>(answer.get("data").cloned().unwrap_or(answer)) {
                    Ok(invite) => courier.send(Event::ServerInviteCreated { server, invite }),
                    Err(failure) => tracing::warn!(%failure, "o convite criado veio inválido"),
                },
                Err(failure) => courier.complain(&failure),
            }
        });
    }

    pub fn set_invites_paused(&self, server: i64, paused: bool) {
        let (api, courier) = (self.api.clone(), self.courier.clone());
        self.spawn(async move {
            let route = if paused { "pauseInvites" } else { "resumeInvites" };
            match api.perform(route, &serde_json::json!({ "server": server }), &Value::Null).await {
                Ok(_) => {
                    if let Ok(tree) = api.tree(server).await { courier.send(Event::Tree(tree)); }
                    server_invites(&api, &courier, server).await;
                }
                Err(failure) => courier.complain(&failure),
            }
        });
    }

    pub fn delete_server_invite(&self, server: i64, invite: i64) {
        let (api, courier) = (self.api.clone(), self.courier.clone());
        self.spawn(async move {
            match api.perform("deleteInvite", &serde_json::json!({ "invite": invite }), &Value::Null).await {
                Ok(_) => server_invites(&api, &courier, server).await,
                Err(failure) => courier.complain(&failure),
            }
        });
    }

    pub fn pick_server_sound(&self, server: i64) {
        let courier = self.courier.clone();
        let spawned = std::thread::Builder::new()
            .name("seletor-de-som".into())
            .spawn(move || {
                if let Some(path) = system::pick_audio() {
                    courier.send(Event::ServerSoundPicked { server, path });
                }
            });
        if let Err(failure) = spawned {
            tracing::warn!(%failure, "o seletor de som não abriu");
        }
    }

    pub fn upload_server_sound(&self, server: i64, path: PathBuf, name: String, emoji: String) {
        let (api, courier) = (self.api.clone(), self.courier.clone());
        self.spawn(async move {
            let fields = serde_json::json!({ "name": name, "emoji": emoji, "volume": 100 });
            match api
                .upload(
                    "storeServerSound",
                    &serde_json::json!({ "server": server }),
                    "sound",
                    &[path],
                    &fields,
                )
                .await
            {
                Ok(_) => server_sounds(&api, &courier, server).await,
                Err(failure) => courier.complain(&failure),
            }
        });
    }

    pub fn delete_server_sound(&self, server: i64, sound: i64) {
        let (api, courier) = (self.api.clone(), self.courier.clone());
        self.spawn(async move {
            match api
                .perform(
                    "deleteServerSound",
                    &serde_json::json!({ "sound": sound }),
                    &Value::Null,
                )
                .await
            {
                Ok(_) => server_sounds(&api, &courier, server).await,
                Err(failure) => courier.complain(&failure),
            }
        });
    }

    pub fn play_server_sound(&self, server: i64, sound: ServerSound) {
        let courier = self.courier.clone();
        self.spawn(async move {
            let response = match reqwest::get(&sound.url).await {
                Ok(response) if response.status().is_success() => response,
                Ok(_) => return tracing::warn!(sound = sound.id, "o download do som foi recusado"),
                Err(failure) => {
                    tracing::warn!(%failure, "o som do servidor não baixou");
                    return;
                }
            };
            let bytes = match response.bytes().await {
                Ok(bytes) => bytes,
                Err(failure) => {
                    tracing::warn!(%failure, "o som do servidor não baixou");
                    return;
                }
            };
            match decode_server_sound(&bytes) {
                Some(samples) => courier.send(Event::ServerSoundDecoded {
                    server,
                    id: sound.id,
                    volume: sound.volume,
                    samples: samples.into(),
                }),
                None => tracing::warn!(sound = sound.id, "o arquivo de som não foi decodificado"),
            }
        });
    }

    pub fn messages(&self, channel: String) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move { read_channel(&api, &courier, channel).await });
    }

    pub fn mark_channel_read(&self, channel: String) {
        let (api, courier) = (self.api.clone(), self.courier.clone());
        self.spawn(async move {
            match api.read_channel(&channel).await {
                Ok(()) => courier.send(Event::ChannelRead(channel)),
                Err(failure) => courier.complain(&failure),
            }
        });
    }

    pub fn older_messages(&self, channel: String, before: i64) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            match api.older_messages(&channel, before).await {
                Ok(messages) => courier.send(Event::Older { channel, messages }),
                Err(failure) => courier.complain(&failure),
            }
        });
    }

    /// Reler o canal em vez de emendar a mensagem na lista: o que aparece é o que o servidor
    /// gravou, e não o que este app achou que mandou.
    pub fn send_message(&self, channel: String, body: String) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            if let Err(failure) = api.send_message(&channel, &body).await {
                return courier.complain(&failure);
            }

            read_channel(&api, &courier, channel).await;
        });
    }

    /// A mensagem com imagens: o formulário com o texto e os `images[]` que o servidor aceita.
    pub fn send_message_with_images(&self, channel: String, body: String, images: Vec<PathBuf>) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            let fields = if body.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::json!({ "body": body })
            };
            let sent = api
                .upload(
                    "sendMessage",
                    &serde_json::json!({ "channel": channel }),
                    "images[]",
                    &images,
                    &fields,
                )
                .await;

            for image in &images {
                if image.starts_with(std::env::temp_dir()) {
                    let _ = std::fs::remove_file(image);
                }
            }

            if let Err(failure) = sent {
                return courier.complain(&failure);
            }

            read_channel(&api, &courier, channel).await;
        });
    }

    /// O "+" da caixa de mensagem: o seletor do sistema bloqueia, então abre numa thread à parte.
    pub fn pick_attachment(&self) {
        let courier = self.courier.clone();

        std::thread::spawn(move || {
            if let Some(path) = system::pick_image() {
                courier.send(Event::Attached(path));
            }
        });
    }

    /// O Ctrl+V de um print: a imagem da área de transferência vira um arquivo para o envio. Sem
    /// imagem lá (só texto), não faz nada: o texto o próprio campo cola.
    pub fn paste_image(&self) {
        let courier = self.courier.clone();

        std::thread::spawn(move || {
            let Ok(image) = arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_image()) else {
                return;
            };

            match clipboard_file(&image) {
                Ok(path) => courier.send(Event::Attached(path)),
                Err(failure) => {
                    tracing::warn!(%failure, "colar: a imagem não virou arquivo");
                    courier.send(Event::Complaint("Não deu para colar a imagem.".into()));
                }
            }
        });
    }

    pub fn edit_message(&self, channel: String, message: i64, body: String) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            if let Err(failure) = api.edit_message(message, &body).await {
                return courier.complain(&failure);
            }

            read_channel(&api, &courier, channel).await;
        });
    }

    pub fn delete_message(&self, channel: String, message: i64) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            if let Err(failure) = api.delete_message(message).await {
                return courier.complain(&failure);
            }

            read_channel(&api, &courier, channel).await;
        });
    }

    pub fn create_server(&self, name: String, icon: Option<PathBuf>) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            let server = match api.create_server(&name).await {
                Ok(server) => server,
                Err(failure) => return courier.complain(&failure),
            };

            if let Some(icon) = icon
                && let Err(failure) = api
                    .upload(
                        "uploadServerIcon",
                        &serde_json::json!({ "server": server.id }),
                        "icon",
                        &[icon],
                        &Value::Null,
                    )
                    .await
            {
                courier.complain(&failure);
            }

            refresh_servers(&api, &courier).await;
            courier.send(Event::OpenServer(server.id));
        });
    }

    pub fn join_invite(&self, code: String) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            match api.join_invite(&code).await {
                Ok(server) => {
                    refresh_servers(&api, &courier).await;
                    courier.send(Event::OpenServer(server.id));
                }
                Err(failure) => courier.complain(&failure),
            }
        });
    }

    pub fn invite_preview(&self, code: String) {
        let (api, courier) = (self.api.clone(), self.courier.clone());
        self.spawn(async move {
            courier.send(Event::InvitePreview {
                preview: api.invite_preview(&code).await,
                code,
            });
        });
    }

    pub fn pick_server_icon(&self) {
        let courier = self.courier.clone();
        let spawned = std::thread::Builder::new()
            .name("seletor-icone-servidor".into())
            .spawn(move || {
                let Some(path) = system::pick_image() else {
                    return;
                };
                let bytes = match std::fs::read(&path) {
                    Ok(bytes) => bytes,
                    Err(failure) => {
                        tracing::warn!(%failure, "ícone do servidor: a prévia não pôde ser lida");
                        courier.send(Event::Complaint(
                            "Não deu para abrir essa imagem. Escolha outro arquivo.".into(),
                        ));
                        return;
                    }
                };

                courier.send(Event::ServerIconPicked { path, bytes });
            });

        if let Err(failure) = spawned {
            tracing::warn!(%failure, "o seletor do ícone do servidor não abriu");
        }
    }

    pub fn leave_server(&self, server: i64) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            match api.leave_server(server).await {
                Ok(()) => refresh_servers(&api, &courier).await,
                Err(failure) => courier.complain(&failure),
            }
        });
    }

    /// A árvore volta inteira depois de criar: é ela que diz a posição do canal novo.
    pub fn create_channel(
        &self,
        server: i64,
        name: String,
        kind: ChannelKind,
        parent: Option<String>,
    ) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            if let Err(failure) = api
                .create_channel(server, &name, kind, parent.as_deref())
                .await
            {
                return courier.complain(&failure);
            }

            match api.tree(server).await {
                Ok(tree) => courier.send(Event::Tree(tree)),
                Err(failure) => courier.complain(&failure),
            }
        });
    }

    /// Uma escrita qualquer pelo mapa de rotas do núcleo (`routes.rs`): cargo, membro, canal,
    /// banimento, servidor. Depois relê o que ela mudou — o tempo real também avisa, mas quem
    /// clicou vê na hora.
    pub fn request(&self, route: &'static str, params: Value, body: Value, refresh: Refresh) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            match api.perform(route, &params, &body).await {
                Ok(_) => reload(&api, &courier, refresh).await,
                Err(failure) => courier.complain(&failure),
            }
        });
    }

    /// Escolhe uma imagem no disco e a manda pela rota: a foto de perfil, o ícone do servidor.
    /// O seletor de arquivo do Windows quer uma thread de COM só dele, e prende quem o abre até a
    /// escolha: uma thread nova, e não a do Tokio nem a da janela.
    pub fn upload_image(
        &self,
        route: &'static str,
        params: Value,
        field: &'static str,
        refresh: Refresh,
    ) {
        let (api, courier, runtime) = (
            self.api.clone(),
            self.courier.clone(),
            self.runtime.handle().clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("seletor-de-imagem".into())
            .spawn(move || {
                let Some(path) = system::pick_image() else {
                    return;
                };

                runtime.block_on(async move {
                    match api
                        .upload(route, &params, field, &[path], &Value::Null)
                        .await
                    {
                        Ok(_) => reload(&api, &courier, refresh).await,
                        Err(failure) => courier.complain(&failure),
                    }
                });
            });

        if let Err(failure) = spawned {
            tracing::warn!(%failure, "o seletor de imagem não abriu");
        }
    }

    /// As 50 entradas mais recentes do registro de auditoria.
    pub fn audits(&self, server: i64) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            let answer = match api
                .perform(
                    "audits",
                    &serde_json::json!({ "server": server }),
                    &Value::Null,
                )
                .await
            {
                Ok(answer) => answer,
                Err(failure) => return courier.complain(&failure),
            };
            // A lista vem paginada pelo Laravel: as linhas moram em `data`.
            let lines = answer.get("data").cloned().unwrap_or(answer);

            match serde_json::from_value::<Vec<AuditEntry>>(lines) {
                Ok(entries) => courier.send(Event::Audits { server, entries }),
                Err(failure) => {
                    tracing::warn!(%failure, "auditoria: a resposta não tem o formato esperado")
                }
            }
        });
    }

    pub fn conversations(&self) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            if let Ok(conversations) = api.conversations().await {
                courier.send(Event::Conversations(conversations));
            }
        });
    }

    pub fn friends(&self) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            match api.friends().await {
                Ok(friends) => courier.send(Event::Friends(friends)),
                Err(failure) => courier.complain(&failure),
            }
        });
    }

    pub fn add_friend(&self, email: String) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            if let Err(failure) = api.add_friend(&email).await {
                return courier.complain(&failure);
            }

            courier.send(Event::Notice("Pedido de amizade enviado.".to_owned()));

            if let Ok(friends) = api.friends().await {
                courier.send(Event::Friends(friends));
            }
        });
    }

    pub fn add_friend_by_id(&self, user: i64) {
        let (api, courier) = (self.api.clone(), self.courier.clone());
        self.spawn(async move {
            if let Err(failure) = api.add_friend_by_id(user).await { return courier.complain(&failure); }
            if let Ok(friends) = api.friends().await { courier.send(Event::Friends(friends)); }
        });
    }

    pub fn answer_friend(&self, friendship: i64, accept: bool) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            if let Err(failure) = api.answer_friend(friendship, accept).await {
                return courier.complain(&failure);
            }

            if let Ok(friends) = api.friends().await {
                courier.send(Event::Friends(friends));
            }
        });
    }

    pub fn block_friend(&self, friendship: i64) {
        let (api, courier) = (self.api.clone(), self.courier.clone());
        self.spawn(async move {
            if let Err(failure) = api.block_friend(friendship).await {
                return courier.complain(&failure);
            }
            if let Ok(friends) = api.friends().await { courier.send(Event::Friends(friends)); }
        });
    }

    /// Abre a conversa e a marca como lida: o contador só zera assim.
    pub fn direct(&self, person: Person) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move { read_direct(&api, &courier, person).await });
    }

    pub fn send_direct(&self, person: Person, body: String) {
        let (api, courier) = (self.api.clone(), self.courier.clone());

        self.spawn(async move {
            if let Err(failure) = api.send_direct(person.id, &body).await {
                return courier.complain(&failure);
            }

            courier.send(Event::DirectSent(person.id));

            read_direct(&api, &courier, person).await;
        });
    }

    pub fn edit_direct(&self, person: Person, message: i64, body: String) {
        let (api, courier) = (self.api.clone(), self.courier.clone());
        self.spawn(async move {
            if let Err(failure) = api.edit_direct_message(message, &body).await {
                return courier.complain(&failure);
            }
            read_direct(&api, &courier, person).await;
        });
    }

    pub fn delete_direct(&self, person: Person, message: i64) {
        let (api, courier) = (self.api.clone(), self.courier.clone());
        self.spawn(async move {
            if let Err(failure) = api.delete_direct_message(message).await {
                return courier.complain(&failure);
            }
            read_direct(&api, &courier, person).await;
        });
    }

    /// Acerta os canais que o tempo real segue com os que a tela quer ouvir agora.
    pub fn follow(&self, wanted: BTreeSet<String>) {
        let Some(live) = lock(&self.live).clone() else {
            return;
        };
        let (gone, fresh): (Vec<String>, Vec<String>) = {
            let mut followed = lock(&self.followed);
            let gone = followed.difference(&wanted).cloned().collect();
            let fresh = wanted.difference(&followed).cloned().collect();

            *followed = wanted;

            (gone, fresh)
        };

        if gone.is_empty() && fresh.is_empty() {
            return;
        }

        self.spawn(async move {
            for channel in gone {
                live.unsubscribe(&channel).await;
            }

            for channel in fresh {
                if let Err(failure) = live.subscribe(&channel).await {
                    tracing::warn!(%failure, channel, "tempo real: o canal não abriu");
                }
            }
        });
    }

    /// O servidor avisou que saiu versão nova: fica anotada e desce calada até o botão verde.
    pub fn release_announced(&self, data: &serde_json::Value) {
        let Some(version) = core_app::update::newer_in(data) else {
            return;
        };

        if lock(&self.installer)
            .as_ref()
            .is_some_and(|(_, ready)| *ready == version)
        {
            return;
        }

        core_app::update::announce(&self.core, &version);

        let (api, courier, installer) = (
            self.api.clone(),
            self.courier.clone(),
            self.installer.clone(),
        );

        self.spawn(async move { prepare_update(&api, &courier, &installer).await });
    }

    /// O botão verde: entrega o app ao instalador, que o fecha e abre a versão nova. Recusado
    /// o aviso do administrador, segue nesta versão.
    pub fn install_update(&self) {
        let Some((installer, _)) = lock(&self.installer).clone() else {
            return;
        };

        if !system::install(&installer, false) {
            self.courier.send(Event::Complaint(
                "A atualização não foi instalada. Ela fica pronta no botão verde.".into(),
            ));
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.activity_monitor_stopped.store(true, Ordering::Relaxed);
    }
}

/// A resposta de um login, cadastro ou entrada só com o nome: o token fica, e a conta abre.
async fn land(
    core: &Arc<Core>,
    api: &Arc<Api>,
    sfu: &Arc<Mutex<Option<String>>>,
    live: &Arc<Mutex<Option<Arc<Realtime>>>>,
    courier: &Courier,
    answer: core_app::models::AuthToken,
) {
    core.set_token(Some(&answer.token));

    let user = match answer.user {
        Some(user) => Some(user),
        None => api.me().await.ok(),
    };

    arrive(api, sfu, live, courier, user).await;
}

/// Com a conta aberta: a tela do hub e o tempo real da conta no lugar do sem conta.
async fn arrive(
    api: &Arc<Api>,
    sfu: &Arc<Mutex<Option<String>>>,
    live: &Arc<Mutex<Option<Arc<Realtime>>>>,
    courier: &Courier,
    user: Option<User>,
) {
    let account = user.as_ref().map(|user| user.id);

    courier.send(Event::Landed(user));

    if account.is_some() {
        if let Some(guest) = lock(live).take() {
            guest.close();
        }

        go_live(api, sfu, live, courier, account).await;
    }
}

/// Abre o tempo real e segue o canal da conta (`user.{id}`: amizades, mensagens diretas,
/// expulsões). Cada linha vira um [`Event::Live`].
async fn go_live(
    api: &Arc<Api>,
    sfu: &Arc<Mutex<Option<String>>>,
    live: &Arc<Mutex<Option<Arc<Realtime>>>>,
    courier: &Courier,
    account: Option<i64>,
) {
    let Some(url) = lock(sfu).clone() else {
        return;
    };

    if lock(live).is_some() {
        return;
    }

    let (updates, heard) = std::sync::mpsc::channel::<String>();
    let mut backoff = Backoff::default();
    // Na abertura o Laravel ocupado (ou a rede piscando) recusava a primeira tentativa, e o app
    // seguia sem tempo real até ser reaberto. O Discord insiste; aqui também.
    let realtime = loop {
        match Realtime::connect(&url, api.clone(), updates.clone()).await {
            Ok(realtime) => break realtime,
            Err(failure) => {
                tracing::warn!(failure = %format!("{failure:#}"), "tempo real: não conectou");
                tokio::time::sleep(backoff.next_patient_delay()).await;

                if lock(live).is_some() || api.signed_in() != account.is_some() {
                    return;
                }
            }
        }
    };

    if let Some(account) = account
        && let Err(failure) = realtime.subscribe(&format!("user.{account}")).await
    {
        tracing::warn!(%failure, "tempo real: o canal da conta não abriu");
    }

    *lock(live) = Some(realtime);

    let courier = courier.clone();
    let spawned = std::thread::Builder::new()
        .name("unkvoid-tempo-real".into())
        .spawn(move || {
            for line in heard {
                courier.send(Event::Live(line));
            }
        });

    if let Err(failure) = spawned {
        tracing::warn!(%failure, "a thread do tempo real não subiu");
    }
}

async fn refresh_servers(api: &Arc<Api>, courier: &Courier) {
    let mut backoff = Backoff::default();

    loop {
        match api.servers().await {
            Ok(servers) => {
                let ids: Vec<i64> = servers.iter().map(|server| server.id).collect();

                courier.send(Event::Servers(servers));
                // O primeiro clique em qualquer servidor já encontra os canais.
                api.warm_trees(&ids).await;

                return;
            }
            // Sem isto, uma falha de rede na abertura deixava a barra sem servidor nenhum até
            // o app ser reaberto.
            Err(HttpError::Failed(Failure::Unreachable)) if backoff.attempt < MAX_ATTEMPTS => {
                tokio::time::sleep(backoff.next_patient_delay()).await;
            }
            Err(failure) => {
                courier.complain(&failure);

                return;
            }
        }
    }
}

async fn server_sounds(api: &Api, courier: &Courier, server: i64) {
    let answer = match api
        .perform(
            "serverSounds",
            &serde_json::json!({ "server": server }),
            &Value::Null,
        )
        .await
    {
        Ok(answer) => answer,
        Err(failure) => return courier.complain(&failure),
    };
    let sounds = answer.get("data").cloned().unwrap_or(answer);
    match serde_json::from_value::<Vec<ServerSound>>(sounds) {
        Ok(sounds) => courier.send(Event::ServerSounds { server, sounds }),
        Err(failure) => tracing::warn!(%failure, "a lista de sons do servidor veio inválida"),
    }
}

async fn server_invites(api: &Api, courier: &Courier, server: i64) {
    let answer = match api.perform("invites", &serde_json::json!({ "server": server }), &Value::Null).await {
        Ok(answer) => answer,
        Err(failure) => return courier.complain(&failure),
    };
    let invites = answer.get("data").cloned().unwrap_or(answer);
    match serde_json::from_value::<Vec<core_app::models::ServerInvite>>(invites) {
        Ok(invites) => courier.send(Event::ServerInvites { server, invites }),
        Err(failure) => tracing::warn!(%failure, "a lista de convites veio inválida"),
    }
}

fn decode_server_sound(bytes: &[u8]) -> Option<Vec<f32>> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::errors::Error;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let source = Box::new(std::io::Cursor::new(bytes.to_vec()));
    let stream = MediaSourceStream::new(source, Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;
    let mut format = probed.format;
    let track = format.default_track()?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .ok()?;
    let mut decoded_stereo = Vec::new();
    let mut source_rate = None;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(_) => return None,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(Error::DecodeError(_)) => continue,
            Err(_) => return None,
        };
        let rate = decoded.spec().rate;
        let channels = decoded.spec().channels.count();
        if rate == 0 || channels == 0 {
            return None;
        }
        source_rate = Some(rate);
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        buffer.copy_interleaved_ref(decoded);
        let samples = buffer.samples();
        for frame in samples.chunks_exact(channels) {
            let left = frame[0];
            let right = frame.get(1).copied().unwrap_or(left);
            decoded_stereo.extend([left, right]);
        }
    }
    let source_rate = source_rate? as usize;
    let source_frames = decoded_stereo.len() / 2;
    if source_frames == 0 {
        return None;
    }
    let output_frames = source_frames
        .saturating_mul(48_000)
        .div_ceil(source_rate)
        .min(240_000);
    let mut output = Vec::with_capacity(output_frames * 2);
    for output_frame in 0..output_frames {
        let position = output_frame as f64 * source_rate as f64 / 48_000.0;
        let first = (position.floor() as usize).min(source_frames - 1);
        let second = (first + 1).min(source_frames - 1);
        let fraction = (position - first as f64) as f32;
        for channel in 0..2 {
            let a = decoded_stereo[first * 2 + channel];
            let b = decoded_stereo[second * 2 + channel];
            output.push((a + (b - a) * fraction).clamp(-1.0, 1.0));
        }
    }
    Some(output)
}

async fn reload(api: &Arc<Api>, courier: &Courier, refresh: Refresh) {
    let server = match refresh {
        Refresh::Tree(server) => Some(server),
        Refresh::Server(server) => {
            refresh_servers(api, courier).await;

            Some(server)
        }
        Refresh::Servers => {
            refresh_servers(api, courier).await;

            None
        }
        Refresh::Me => {
            match api.me().await {
                Ok(user) => courier.send(Event::Me(user)),
                Err(failure) => courier.complain(&failure),
            }

            None
        }
        Refresh::Friends => {
            match api.friends().await {
                Ok(friends) => courier.send(Event::Friends(friends)),
                Err(failure) => courier.complain(&failure),
            }
            None
        }
    };

    if let Some(server) = server {
        match api.tree(server).await {
            Ok(tree) => courier.send(Event::Tree(tree)),
            Err(failure) => courier.complain(&failure),
        }
    }
}

/// O que o servidor aceita por imagem de mensagem.
const MOST_IMAGE_BYTES: u64 = 2 * 1024 * 1024;

/// O print colado como arquivo: PNG, que guarda o texto nítido; passando dos 2 MB que o
/// servidor aceita por imagem, JPEG, cada vez mais comprimido até caber.
fn clipboard_file(image: &arboard::ImageData<'_>) -> anyhow::Result<PathBuf> {
    let width = u32::try_from(image.width)?;
    let height = u32::try_from(image.height)?;
    let pixels = image::RgbaImage::from_raw(width, height, image.bytes.to_vec())
        .ok_or_else(|| anyhow::anyhow!("o tamanho da imagem colada não bate com os pixels"))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default();
    let png = std::env::temp_dir().join(format!("unkvoid-colado-{stamp}.png"));

    pixels.save_with_format(&png, image::ImageFormat::Png)?;

    if std::fs::metadata(&png)?.len() <= MOST_IMAGE_BYTES {
        return Ok(png);
    }

    std::fs::remove_file(&png)?;

    let jpeg = png.with_extension("jpg");
    let opaque = image::DynamicImage::ImageRgba8(pixels).to_rgb8();
    let mut quality = 85;

    loop {
        {
            let file = std::io::BufWriter::new(std::fs::File::create(&jpeg)?);

            image::codecs::jpeg::JpegEncoder::new_with_quality(file, quality).encode_image(&opaque)?;
        }

        if std::fs::metadata(&jpeg)?.len() <= MOST_IMAGE_BYTES || quality <= 40 {
            return Ok(jpeg);
        }

        quality -= 15;
    }
}

async fn read_channel(api: &Arc<Api>, courier: &Courier, channel: String) {
    match api.messages(&channel).await {
        Ok(messages) => courier.send(Event::Messages { channel, messages }),
        Err(failure) => courier.complain(&failure),
    }
}

async fn read_direct(api: &Arc<Api>, courier: &Courier, person: Person) {
    match api.direct_messages(person.id).await {
        Ok(messages) => {
            let _ = api.read_conversation(person.id).await;
            let mutuals = api.mutuals(person.id).await.unwrap_or_default();
            courier.send(Event::Direct { person, messages, mutuals });

            if let Ok(conversations) = api.conversations().await {
                courier.send(Event::Conversations(conversations));
            }
        }
        Err(failure) => courier.complain(&failure),
    }
}

/// Baixa calada a versão nova e, conferida a assinatura, mostra o botão verde: quem escolhe a
/// hora de reiniciar é a pessoa, como no Discord.
async fn prepare_update(
    api: &Api,
    courier: &Courier,
    ready: &Arc<Mutex<Option<(PathBuf, String)>>>,
) {
    if system::updated_elsewhere() {
        return;
    }

    let Some(release) = api.newer_release(core_app::update::PLATFORM).await else {
        return;
    };
    let Some(installer) = core_app::update::fetch(api, &release, |_, _| {}).await else {
        return;
    };
    let version = release.version.clone();

    tracing::info!(version, "atualização: pronta para instalar");
    *lock(ready) = Some((installer, version.clone()));
    courier.send(Event::UpdateReady(version));
}

/// Um `Mutex` envenenado é uma tarefa que caiu no meio de uma troca de sessão: o app continuar
/// com o que tem é melhor do que fechar a janela na cara de quem está nele.
fn lock<T>(cell: &Mutex<T>) -> MutexGuard<'_, T> {
    cell.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clipboard(width: usize, height: usize, pixel: impl FnMut(usize) -> [u8; 4]) -> arboard::ImageData<'static> {
        arboard::ImageData {
            width,
            height,
            bytes: (0..width * height).flat_map(pixel).collect::<Vec<u8>>().into(),
        }
    }

    #[test]
    fn a_pasted_print_stays_png_and_a_heavy_one_shrinks_to_jpeg_under_the_limit() {
        let small = clipboard_file(&clipboard(64, 48, |_| [30, 30, 34, 255])).expect("o print pequeno vira arquivo");

        assert_eq!(small.extension().and_then(|extension| extension.to_str()), Some("png"));
        std::fs::remove_file(small).ok();

        // Ruído não comprime: o PNG passa de 2 MB e tem de cair para JPEG.
        let mut seed = 7_u32;
        let heavy = clipboard_file(&clipboard(1600, 1200, |_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let [red, green, blue, _] = seed.to_le_bytes();

            [red, green, blue, 255]
        }))
        .expect("o print pesado vira arquivo");

        assert_eq!(heavy.extension().and_then(|extension| extension.to_str()), Some("jpg"));
        assert!(std::fs::metadata(&heavy).expect("o arquivo existe").len() <= MOST_IMAGE_BYTES);
        std::fs::remove_file(heavy).ok();
    }
}
