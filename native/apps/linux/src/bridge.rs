//! A ponte entre o clique e o núcleo.
//!
//! O GTK desenha numa thread só e não fala `async`; o núcleo é `async` e não pode desenhar.
//! Aqui o trabalho vai para o runtime do Tokio e o resultado volta numa fila que a thread
//! da tela consome — sem isso, um `GET` lento congelaria a janela inteira.
//!
//! Nada aqui decide: tudo que é decisão (o código vale? onde se cai ao sair? quem está na
//! sala?) é chamada ao `core_app`.

use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use core_app::api::{Api, HttpError};
use core_app::app::EntryRefusal;
use core_app::models::{Message, Peer, RoomIdentity, ServerSummary, ServerTree, User};
use core_app::protocol::{action, local};
use core_app::reconnect::Backoff;
use core_app::session::Session;
use core_app::{App, Failure, Screen};
use media::Source;
use serde_json::json;
use storage::Storage;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::devices;
use crate::sending::{self, Sending};
use crate::streaming::{self, Mine, Tile};
use crate::watching::Watching;

const DEFAULT_SERVER: &str = "https://unkvoid.com";

/// O que o trabalho de fundo tem a dizer para a tela.
pub enum Update {
    /// O servidor respondeu: hora de sair da tela de abertura.
    Ready(Option<User>),
    Offline(String),
    /// A última coisa que deu errado, na frase que a pessoa lê.
    Complaint(String),
    /// O que deu errado ao entrar ou criar conta. Anda separado do `Complaint` porque no
    /// desenho são dois cartões, e o erro de um não se escreve no outro.
    LoginComplaint(String),
    Servers(Vec<ServerSummary>),
    Tree(Box<ServerTree>),
    Messages(Vec<Message>),
    Joined { room: String, peers: Vec<Peer> },
    Peers(Vec<Peer>),
    /// As transmissões que estão sendo assistidas agora, uma por cartão.
    Tiles(Vec<Tile>),
    /// O que esta pessoa está mandando, e o que ela tem permissão de mandar.
    Mine(Mine),
    /// O ida e volta até o SFU, em milissegundos, de 5 em 5 segundos.
    Ping(u64),
    /// Trocar de tela sem nada novo para mostrar: sair da sala, ir para os servidores.
    Show(Screen),
}

pub struct Bridge {
    runtime: Runtime,
    core: Arc<App>,
    api: Arc<Api>,
    /// De onde sai o WebSocket: vem do `GET /api/config`, e até ele responder não há sala.
    /// Em `Mutex` porque quem o descobre é o runtime e quem o usa é a tela.
    sfu: Arc<Mutex<Option<String>>>,
    session: Arc<Mutex<Option<Arc<Session>>>>,
    sending: Arc<Mutex<Sending>>,
    watching: Arc<Mutex<Watching>>,
    install_id: String,
    to_screen: UnboundedSender<Update>,
}

impl Bridge {
    pub fn new() -> anyhow::Result<(Rc<Self>, UnboundedReceiver<Update>)> {
        let storage = Storage::open()?;
        let server = std::env::var("UNKVOID_SERVER").unwrap_or_else(|_| DEFAULT_SERVER.to_owned());
        let (to_screen, updates) = unbounded_channel();
        let core = Arc::new(App::new(storage));
        let install_id = core.install_id();

        let bridge = Rc::new(Self {
            runtime: Runtime::new()?,
            core,
            api: Arc::new(Api::new(&server)?),
            sfu: Arc::new(Mutex::new(None)),
            session: Arc::new(Mutex::new(None)),
            sending: Arc::default(),
            watching: Arc::default(),
            install_id,
            to_screen,
        });

        Ok((bridge, updates))
    }

    pub fn name(&self) -> String {
        self.core.state().name
    }

    pub fn recent_rooms(&self) -> Vec<String> {
        self.core.recent_rooms()
    }

    /// Onde se cai ao sair de uma sala e ao abrir o app. Quem decide é o núcleo.
    pub fn home(&self) -> Screen {
        self.core.home()
    }

    /// O núcleo é quem guarda em que tela o app está — é o que o macOS e o Windows lêem
    /// pela ABI. Aqui ele é escrito no mesmo lugar em que a janela troca de tela, para os
    /// dois nunca discordarem.
    pub fn show(&self, screen: Screen) {
        self.core.show(screen);

        let _ = self.to_screen.send(Update::Show(screen));
    }

    pub fn show_home(&self) {
        self.show(self.home());
    }

    /// A abertura: o servidor responde? Onde fica o SFU? O token guardado ainda vale?
    pub fn start(self: &Rc<Self>) {
        let (core, api, screen) = (self.core.clone(), self.api.clone(), self.to_screen.clone());
        let sfu = self.sfu.clone();

        self.spawn(async move {
            let mut backoff = Backoff::default();

            while !api.reachable().await {
                let Some(wait) = backoff.next_delay() else {
                    let _ = screen.send(Update::Offline("O servidor não respondeu.".into()));

                    return;
                };

                let _ = screen.send(Update::Offline(format!(
                    "Sem resposta do servidor. Tentando de novo… (tentativa {})",
                    backoff.attempt
                )));

                tokio::time::sleep(wait).await;
            }

            match api.config().await {
                Ok(config) => *lock(&sfu) = Some(config.sfu),
                Err(failure) => {
                    let _ = screen.send(Update::Complaint(said(&failure)));
                }
            }

            let _ = screen.send(Update::Ready(restore(&core, &api).await));
        });
    }

    pub fn sign_in(self: &Rc<Self>, email: &str, password: &str, register: bool) {
        let (core, api, screen) = (self.core.clone(), self.api.clone(), self.to_screen.clone());
        let (email, password) = (email.to_owned(), password.to_owned());
        let device = device_name();

        self.spawn(async move {
            let attempt = if register {
                api.register(&email, &password, &device).await
            } else {
                api.login(&email, &password, &device).await
            };

            match attempt {
                Ok(answer) => {
                    core.set_token(Some(&answer.token));

                    let user = match answer.user {
                        Some(user) => Some(user),
                        None => api.me().await.ok(),
                    };

                    let _ = screen.send(Update::Ready(user));
                }
                Err(failure) => {
                    let _ = screen.send(Update::LoginComplaint(said(&failure)));
                }
            }
        });
    }

    pub fn sign_out(self: &Rc<Self>) {
        self.core.set_token(None);
        self.api.set_token(None);

        let _ = self.to_screen.send(Update::Ready(None));
    }

    pub fn load_servers(self: &Rc<Self>) {
        let (api, screen) = (self.api.clone(), self.to_screen.clone());

        self.spawn(async move {
            match api.servers().await {
                Ok(servers) => {
                    let _ = screen.send(Update::Servers(servers));
                }
                Err(failure) => {
                    let _ = screen.send(Update::Complaint(said(&failure)));
                }
            }
        });
    }

    pub fn open_server(self: &Rc<Self>, server: i64) {
        let (api, screen) = (self.api.clone(), self.to_screen.clone());

        self.spawn(async move {
            match api.tree(server).await {
                Ok(tree) => {
                    let _ = screen.send(Update::Tree(Box::new(tree)));
                }
                Err(failure) => {
                    let _ = screen.send(Update::Complaint(said(&failure)));
                }
            }
        });
    }

    pub fn open_channel(self: &Rc<Self>, channel: &str) {
        let (api, screen, channel) = (self.api.clone(), self.to_screen.clone(), channel.to_owned());

        self.spawn(async move {
            match api.messages(&channel).await {
                Ok(messages) => {
                    let _ = screen.send(Update::Messages(messages));
                }
                Err(failure) => {
                    let _ = screen.send(Update::Complaint(said(&failure)));
                }
            }
        });
    }

    pub fn send_message(self: &Rc<Self>, channel: &str, body: &str) {
        let (api, screen) = (self.api.clone(), self.to_screen.clone());
        let (channel, body) = (channel.to_owned(), body.to_owned());

        self.spawn(async move {
            if let Err(failure) = api.send_message(&channel, &body).await {
                let _ = screen.send(Update::Complaint(said(&failure)));

                return;
            }

            // Reler o canal em vez de emendar a mensagem na lista: o que aparece é o que o
            // servidor gravou, e não o que este app achou que mandou.
            match api.messages(&channel).await {
                Ok(messages) => {
                    let _ = screen.send(Update::Messages(messages));
                }
                Err(failure) => {
                    let _ = screen.send(Update::Complaint(said(&failure)));
                }
            }
        });
    }

    /// "Criar uma sala": sem código digitado, o núcleo sorteia um.
    pub fn create_room(self: &Rc<Self>, name: &str, typed: &str) {
        let opened = self.core.create_room(name, typed);

        self.enter(opened, None);
    }

    pub fn join_room(self: &Rc<Self>, name: &str, typed: &str) {
        let opened = self.core.join_room(name, typed);

        self.enter(opened, None);
    }

    /// Entrar num canal de voz é a mesma sala, com o token de 60 s no lugar do nome.
    pub fn join_voice(self: &Rc<Self>, channel: &str) {
        self.enter(Ok(channel.to_owned()), Some(channel.to_owned()));
    }

    fn enter(self: &Rc<Self>, opened: Result<String, EntryRefusal>, voice: Option<String>) {
        let room = match opened {
            Ok(room) => room,
            Err(refusal) => {
                let _ = self.to_screen.send(Update::Complaint(refused(refusal).to_owned()));

                return;
            }
        };

        let Some(url) = lock(&self.sfu).clone() else {
            let _ = self
                .to_screen
                .send(Update::Complaint("O servidor ainda não disse onde fica o SFU.".into()));

            return;
        };

        let screen = self.to_screen.clone();
        let (sending, watching) = (self.sending.clone(), self.watching.clone());
        let held = self.session.clone();
        let identity = self.identity(&room, voice);

        self.spawn(async move {
            let (session, mut events) = match Session::join(&url, &room, identity).await {
                Ok(joined) => joined,
                Err(failure) => {
                    let reason = sentence(Failure::from_error(&failure));

                    let _ = screen
                        .send(Update::Complaint(format!("Não deu para entrar na sala. {reason}")));

                    return;
                }
            };

            *lock(&held) = Some(session.clone());

            let _ = screen.send(Update::Joined { room, peers: session.peers() });

            settle(&session, &sending, &watching, &screen).await;

            let mut shown = Vec::new();

            // O socket fechado encerra a fila, e é aí que este laço termina.
            while let Some(event) = events.recv().await {
                let changed = session.apply(&event);

                match event.name.as_str() {
                    "newProducer" => consume_all(&session, &watching, &screen).await,
                    "producerClosed" => {
                        let producer_id = event.data["producerId"].as_str().unwrap_or_default();

                        lock(&watching).stop(Some(producer_id));
                    }
                    local::SESSION_LOST => {
                        let _ = screen.send(Update::Complaint("A sala caiu. Voltando…".into()));
                    }
                    local::SESSION_REJOINED => {
                        let _ = screen.send(Update::Complaint(String::new()));

                        // Entrada nova (a carência do servidor expirou) perdeu tudo o que
                        // estava aberto lá: o que ficou aqui só atrapalha.
                        if !session.resumed() {
                            republish(&session, &sending, &watching).await;
                        }

                        settle(&session, &sending, &watching, &screen).await;
                    }
                    local::SESSION_GONE => {
                        let _ = screen.send(Update::Complaint(
                            "A sala não voltou. Entre de novo quando a internet estabilizar.".into(),
                        ));
                    }
                    local::PING_MEASURED => {
                        if let Some(milliseconds) = event.data.as_u64() {
                            let _ = screen.send(Update::Ping(milliseconds));
                        }
                    }
                    _ => {}
                }

                if changed {
                    let _ = screen.send(Update::Peers(session.peers()));
                }

                let tiles = streaming::tiles(&session, &watching);

                if tiles != shown {
                    shown = tiles.clone();

                    let _ = screen.send(Update::Tiles(tiles));
                }
            }
        });
    }

    /// Quem esta pessoa é para o SFU, **perguntado de novo a cada entrada**: o token de voz
    /// vale 60 s, e guardar o primeiro faria toda reconexão levar um token vencido.
    fn identity(self: &Rc<Self>, room: &str, voice: Option<String>) -> core_app::Identity {
        let api = self.api.clone();
        let name = self.core.state().name;
        let (room, install_id) = (room.to_owned(), self.install_id.clone());

        Arc::new(move || {
            let (api, room, voice) = (api.clone(), room.clone(), voice.clone());
            let (name, install_id) = (name.clone(), install_id.clone());

            Box::pin(async move {
                identity(&api, &room, voice, &name, &install_id)
                    .await
                    .map_err(|failure| anyhow::Error::new(reason(&failure)))
            })
        })
    }

    /// Compartilhar a tela. O som dela vai junto, e chega mudo do outro lado.
    pub fn share_screen(self: &Rc<Self>) {
        let Some(session) = lock(&self.session).clone() else {
            return;
        };

        let (sending, screen) = (self.sending.clone(), self.to_screen.clone());

        self.spawn(async move {
            let config = sending::screen_config(capture::Quality::Hd1080, 60, true);
            let published = streaming::publish(
                &session,
                &sending,
                Source::Screen,
                config,
                Some(Source::Screen),
                Some(Source::ScreenAudio),
            )
            .await;

            if let Err(failure) = published {
                tracing::warn!(%failure, "a tela não subiu");

                let _ = screen.send(Update::Complaint(
                    "Não deu para compartilhar a tela. Confira se o GStreamer está instalado.".into(),
                ));
            }

            let _ = screen.send(Update::Mine(streaming::mine(&session, &sending)));
        });
    }

    pub fn stop_sharing(self: &Rc<Self>) {
        self.unpublish(Source::Screen);
    }

    /// Procura de novo quem está transmitindo. O `newProducer` já faz isso sozinho; este
    /// caminho existe para quando o evento se perdeu, e é o "Atualizar" da lista de pessoas.
    pub fn refresh_watch(self: &Rc<Self>) {
        let Some(session) = lock(&self.session).clone() else {
            return;
        };

        let (watching, screen) = (self.watching.clone(), self.to_screen.clone());

        self.spawn(async move {
            consume_all(&session, &watching, &screen).await;
        });
    }

    pub fn toggle_camera(self: &Rc<Self>) {
        if lock(&self.sending).is_live(Source::Camera) {
            self.unpublish(Source::Camera);

            return;
        }

        let Some(session) = lock(&self.session).clone() else {
            return;
        };

        let (sending, screen) = (self.sending.clone(), self.to_screen.clone());

        self.spawn(async move {
            let published = streaming::publish(
                &session,
                &sending,
                Source::Camera,
                sending::camera_config(0),
                Some(Source::Camera),
                None,
            )
            .await;

            if let Err(failure) = published {
                tracing::warn!(%failure, "a câmera não subiu");

                let _ = screen.send(Update::Complaint("Não deu para abrir a câmera.".into()));
            }

            let _ = screen.send(Update::Mine(streaming::mine(&session, &sending)));
        });
    }

    /// Mudo é mandar silêncio **e** avisar a sala: sem o silêncio o relógio de 30 s do SFU
    /// mataria o producer, e sem o aviso ninguém veria o microfone fechado.
    pub fn toggle_mic(self: &Rc<Self>) {
        let Some(session) = lock(&self.session).clone() else {
            return;
        };

        let muted = !lock(&self.sending).is_muted(Source::Mic);
        let (sending, screen) = (self.sending.clone(), self.to_screen.clone());

        lock(&self.sending).set_muted(Source::Mic, muted);

        self.spawn(async move {
            if !lock(&sending).is_live(Source::Mic) {
                open_mic(&session, &sending, &screen).await;
            } else {
                pause_mic(&session, &sending, muted).await;
            }

            let _ = screen.send(Update::Mine(streaming::mine(&session, &sending)));
        });
    }

    /// Escolher o microfone: o padrão do sistema muda, e a captura reabre para pegá-lo. O
    /// producer continua o mesmo, então a sala não vê ninguém entrar e sair.
    pub fn use_microphone(self: &Rc<Self>, name: &str) {
        if !devices::use_microphone(name) {
            let _ = self.to_screen.send(Update::Complaint("Não deu para trocar o microfone.".into()));

            return;
        }

        if let Err(failure) = lock(&self.sending).restart(Source::Mic, None, Some(Source::Mic)) {
            tracing::warn!(%failure, "o microfone novo não abriu");

            let _ = self.to_screen.send(Update::Complaint("O microfone escolhido não abriu.".into()));
        }
    }

    /// Escolher a saída de áudio. Os tocadores reabrem: quem já está tocando não se muda de
    /// aparelho sozinho.
    pub fn use_speaker(self: &Rc<Self>, name: &str) {
        if !devices::use_speaker(name) {
            let _ = self.to_screen.send(Update::Complaint("Não deu para trocar a saída de áudio.".into()));

            return;
        }

        lock(&self.watching).use_new_output();
    }

    /// Ensurdecer cala só o áudio que chega: pausar o vídeo faria esperar keyframe na volta.
    pub fn toggle_deafen(self: &Rc<Self>) -> bool {
        let mut watching = lock(&self.watching);
        let deafened = !watching.is_deafened();

        watching.deafen(deafened);

        deafened
    }

    pub fn is_deafened(&self) -> bool {
        lock(&self.watching).is_deafened()
    }

    /// O que chegou de vídeo desde o último desenho. É a janela que chama, no relógio dela.
    pub fn fresh_frames(&self) -> Vec<(String, Vec<u8>)> {
        lock(&self.watching).fresh_frames()
    }

    fn unpublish(self: &Rc<Self>, source: Source) {
        let Some(session) = lock(&self.session).clone() else {
            return;
        };

        let (sending, screen) = (self.sending.clone(), self.to_screen.clone());

        self.spawn(async move {
            streaming::unpublish(&session, &sending, source).await;

            let _ = screen.send(Update::Mine(streaming::mine(&session, &sending)));
        });
    }

    pub fn leave_room(self: &Rc<Self>) {
        self.core.leave_room();

        let (screen, landing) = (self.to_screen.clone(), self.home());

        // Cada origem é um `gst-launch` filho: sair da tela não o mata sozinho.
        lock(&self.watching).stop(None);

        let _ = screen.send(Update::Tiles(Vec::new()));

        let (sending, held) = (self.sending.clone(), lock(&self.session).take());

        self.spawn(async move {
            let Some(session) = held else {
                lock(&sending).stop_all();

                let _ = screen.send(Update::Show(landing));

                return;
            };

            let live = lock(&sending).live_sources();

            for source in live {
                streaming::unpublish(&session, &sending, source).await;
            }

            if let Err(failure) = session.leave().await {
                tracing::warn!(%failure, "a saída da sala não foi confirmada");
            }

            let _ = screen.send(Update::Mine(Mine::default()));
            let _ = screen.send(Update::Show(landing));
        });
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

/// O que fazer assim que a sala abre, e de novo a cada volta: assistir a quem já está
/// transmitindo e abrir o microfone.
async fn settle(
    session: &Arc<Session>,
    sending: &Arc<Mutex<Sending>>,
    watching: &Arc<Mutex<Watching>>,
    screen: &UnboundedSender<Update>,
) {
    consume_all(session, watching, screen).await;

    // Microfone aberto por padrão: quem entra na voz já é ouvido, como no app de hoje.
    if session.can("speak") && !lock(sending).is_live(Source::Mic) {
        open_mic(session, sending, screen).await;
    }

    let _ = screen.send(Update::Tiles(streaming::tiles(session, watching)));
    let _ = screen.send(Update::Mine(streaming::mine(session, sending)));
}

/// Assiste a tudo o que ainda não está sendo assistido. É idempotente de propósito: o
/// evento de producer novo e a volta depois de uma queda chamam a mesma coisa.
async fn consume_all(
    session: &Arc<Session>,
    watching: &Arc<Mutex<Watching>>,
    screen: &UnboundedSender<Update>,
) {
    let peers = session.peers();

    for producer in peers.iter().filter(|peer| !peer.self_peer).flat_map(|peer| &peer.producers) {
        if let Err(failure) = streaming::consume(session, watching, producer).await {
            tracing::warn!(%failure, producer = %producer.producer_id, "não deu para assistir");

            let _ = screen
                .send(Update::Complaint("Não deu para assistir a uma das transmissões.".into()));
        }
    }
}

/// Entrada nova depois de uma queda longa: o servidor não tem mais nada desta pessoa, e o
/// que estava capturando aqui precisa ser publicado do zero.
async fn republish(
    session: &Arc<Session>,
    sending: &Arc<Mutex<Sending>>,
    watching: &Arc<Mutex<Watching>>,
) {
    // O que se estava assistindo também morreu do lado de lá.
    lock(watching).stop(None);

    let sources = lock(sending).live_sources();
    let live: Vec<(Source, capture::CaptureConfig)> = sources
        .into_iter()
        .filter_map(|source| lock(sending).config_of(source).map(|config| (source, config)))
        .collect();

    for (source, _) in &live {
        lock(sending).stop(*source);
    }

    // A chave vai junto: o remetente novo recomeça a numeração, e a mesma chave com o
    // contador reiniciado repetiria o keystream.
    lock(sending).renew_key();

    for (source, config) in live {
        let (video, audio) = match source {
            Source::Screen => (Some(Source::Screen), Some(Source::ScreenAudio)),
            Source::Camera => (Some(Source::Camera), None),
            _ => (None, Some(Source::Mic)),
        };

        if let Err(failure) = streaming::publish(session, sending, source, config, video, audio).await {
            tracing::warn!(%failure, ?source, "a transmissão não voltou");
        }
    }
}

async fn open_mic(
    session: &Arc<Session>,
    sending: &Arc<Mutex<Sending>>,
    screen: &UnboundedSender<Update>,
) {
    let published = streaming::publish(
        session,
        sending,
        Source::Mic,
        sending::microphone_config(),
        None,
        Some(Source::Mic),
    )
    .await;

    if let Err(failure) = published {
        tracing::warn!(%failure, "o microfone não subiu");

        let _ = screen.send(Update::Complaint("Não deu para abrir o microfone.".into()));
    }
}

/// Mutar também pausa o producer: é o que faz a sala desenhar o microfone fechado.
async fn pause_mic(session: &Arc<Session>, sending: &Arc<Mutex<Sending>>, muted: bool) {
    let Some(producer_id) = lock(sending).producer_of(Source::Mic) else {
        return;
    };

    let acted = if muted { action::PAUSE_PRODUCER } else { action::RESUME_PRODUCER };

    if let Err(failure) = session.client().call(acted, json!({ "producerId": producer_id })).await {
        tracing::warn!(%failure, muted, "a sala não soube do microfone");
    }
}

async fn restore(core: &Arc<App>, api: &Arc<Api>) -> Option<User> {
    let token = core.token()?;

    api.set_token(Some(token));

    match api.me().await {
        Ok(user) => Some(user),
        Err(failure) => {
            // Token vencido ou revogado não é falha: é login de novo.
            tracing::info!(?failure, "o token guardado não vale mais");
            api.set_token(None);
            core.set_token(None);

            None
        }
    }
}

async fn identity(
    api: &Arc<Api>,
    room: &str,
    voice: Option<String>,
    name: &str,
    install_id: &str,
) -> Result<RoomIdentity, HttpError> {
    if let Some(channel) = voice {
        return Ok(RoomIdentity::Account { token: api.voice_token(&channel).await? });
    }

    if api.signed_in() {
        return Ok(RoomIdentity::Account { token: api.room_token(room).await? });
    }

    Ok(RoomIdentity::Guest {
        room: room.to_owned(),
        name: name.to_owned(),
        install_id: install_id.to_owned(),
    })
}

/// O motivo por trás de uma falha de chamada, para ele atravessar um `anyhow` sem virar
/// "confira a sua internet" no caminho.
fn reason(error: &HttpError) -> Failure {
    match error {
        HttpError::Failed(failure) => *failure,
        HttpError::Invalid { .. } => Failure::Invalid,
    }
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
    std::env::var("HOSTNAME").map(|host| format!("linux-{host}")).unwrap_or_else(|_| "linux".into())
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
            assert!(!written.chars().any(|letter| letter.is_ascii_digit()), "vazou número: {written}");
        }
    }

    #[test]
    fn the_only_server_text_the_person_reads_is_the_validation_one() {
        let invalid = HttpError::Invalid {
            field: "email".into(),
            message: "Este e-mail já está em uso.".into(),
        };

        assert_eq!(said(&invalid), "Este e-mail já está em uso.");
        assert_eq!(said(&HttpError::Failed(Failure::NotAllowed)), sentence(Failure::NotAllowed));
    }

    #[test]
    fn every_refusal_has_a_sentence_the_person_can_act_on() {
        // Se o núcleo ganhar um motivo novo, isto para de compilar — que é o ponto.
        for refusal in [EntryRefusal::NameIsEmpty, EntryRefusal::CodeIsInvalid] {
            assert!(!refused(refusal).is_empty());
        }
    }
}
