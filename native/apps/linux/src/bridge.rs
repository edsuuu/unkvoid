//! A ponte entre o clique e o núcleo.
//!
//! O GTK desenha numa thread só e não fala `async`; o núcleo é `async` e não pode desenhar.
//! Aqui o trabalho vai para o runtime do Tokio e o resultado volta numa fila que a thread
//! da tela consome — sem isso, um `GET` lento congelaria a janela inteira.
//!
//! Nada aqui decide: tudo que é decisão (o código vale? onde se cai ao sair? quem está na
//! sala?) é chamada ao `core_app`.

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use capture::{CaptureConfig, CaptureEvent, CaptureSource, PlatformCapturer};

use core_app::api::{Api, HttpError};
use core_app::app::EntryRefusal;
use core_app::models::{
    ChannelKind, Conversation, DirectMessage, Friendship, Message, Person, RoomIdentity, ServerSummary,
    ServerTree, User,
};
pub use core_app::models::Peer;
use core_app::reconnect::Backoff;
use core_app::room::Room;
use core_app::{App, Failure, Screen};
use serde_json::Value;
use storage::Storage;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::devices;
use crate::streaming::{self, Mine, Tile};
use crate::watching::Watch;

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
    Friends(Vec<Friendship>),
    Conversations(Vec<Conversation>),
    /// A conversa aberta: com quem, e o que já foi dito.
    Direct { person: Person, messages: Vec<DirectMessage> },
    /// Entrou numa sala. `voice` diz o nome do canal quando se entrou pela voz de um
    /// servidor: aí a tela continua sendo o hub, como no React.
    Joined { room: String, voice: Option<String>, peers: Vec<Peer> },
    /// Saiu da voz e continua no servidor.
    VoiceLeft,
    Peers(Vec<Peer>),
    /// As transmissões que estão sendo assistidas agora, uma por cartão.
    Tiles(Vec<Tile>),
    /// O que esta pessoa está mandando, e o que ela tem permissão de mandar.
    Mine(Mine),
    /// O ida e volta até o SFU, em milissegundos, de 5 em 5 segundos.
    Ping(u64),
    /// Trocar de tela sem nada novo para mostrar: sair da sala, ir para os servidores.
    Show(Screen),
    /// O servidor tirou esta sessão da sala, com a frase de por quê.
    ThrownOut(&'static str),
}

pub struct Bridge {
    runtime: Runtime,
    core: Arc<App>,
    api: Arc<Api>,
    /// De onde sai o WebSocket: vem do `GET /api/config`, e até ele responder não há sala.
    /// Em `Mutex` porque quem o descobre é o runtime e quem o usa é a tela.
    sfu: Arc<Mutex<Option<String>>>,
    /// A sala aberta, por código ou canal de voz: o `Room` do núcleo, o mesmo do macOS e do
    /// Windows.
    room: Arc<Mutex<Option<Arc<Room>>>>,
    /// O que se assiste: os decodificadores e os tocadores de cada transmissão.
    watch: Arc<Mutex<Option<Watch>>>,
    /// A captura do microfone, que sobe pela sala com `speak`.
    microphone: Arc<Mutex<Option<PlatformCapturer>>>,
    /// O último `room.mine`: é dele que o microfone e a câmera sabem se alternam ou abrem.
    mine: Arc<Mutex<Mine>>,
    /// Surdo vale fora da sala também: quem entra surdo continua surdo.
    deafened: Arc<AtomicBool>,
    /// A sala aberta é a de um canal de voz, e não uma sala por código.
    in_voice: Arc<AtomicBool>,
    /// Qual servidor está aberto: é nele que um canal novo nasce.
    opened: Arc<Mutex<Option<i64>>>,
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
            room: Arc::default(),
            watch: Arc::default(),
            microphone: Arc::default(),
            mine: Arc::default(),
            deafened: Arc::default(),
            in_voice: Arc::default(),
            opened: Arc::default(),
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
                    let _ = screen.send(Update::Servers(servers.clone()));

                    // As árvores vêm atrás, sem ninguém esperar por elas: quando o clique
                    // acontecer, os canais já estão em mãos. Quem as guarda é o núcleo.
                    api.warm_trees(&servers.iter().map(|server| server.id).collect::<Vec<_>>()).await;
                }
                Err(failure) => {
                    let _ = screen.send(Update::Complaint(said(&failure)));
                }
            }
        });
    }

    /// Cria um servidor. Ele já nasce com um canal de texto e um de voz — por isso a lista
    /// é recarregada e a árvore aberta em seguida: é o que o React faz.
    pub fn create_server(self: &Rc<Self>, name: &str) {
        let (api, screen, name) = (self.api.clone(), self.to_screen.clone(), name.to_owned());

        self.spawn(async move {
            match api.create_server(&name).await {
                Ok(server) => entered(&api, &screen, server.id).await,
                Err(failure) => {
                    let _ = screen.send(Update::Complaint(said(&failure)));
                }
            }
        });
    }

    /// Entra por convite. O código é o que o dono mandou, não o do servidor.
    pub fn join_invite(self: &Rc<Self>, code: &str) {
        let (api, screen, code) = (self.api.clone(), self.to_screen.clone(), code.trim().to_owned());

        self.spawn(async move {
            match api.join_invite(&code).await {
                Ok(server) => entered(&api, &screen, server.id).await,
                Err(failure) => {
                    let _ = screen.send(Update::Complaint(said(&failure)));
                }
            }
        });
    }

    pub fn load_friends(self: &Rc<Self>) {
        let (api, screen) = (self.api.clone(), self.to_screen.clone());

        self.spawn(async move {
            match api.friends().await {
                Ok(friends) => {
                    let _ = screen.send(Update::Friends(friends));
                }
                Err(failure) => {
                    let _ = screen.send(Update::Complaint(said(&failure)));
                }
            }
        });
    }

    /// Pede amizade pelo e-mail. Quem diz se a pessoa existe é o servidor.
    pub fn add_friend(self: &Rc<Self>, email: &str) {
        let (api, screen, email) = (self.api.clone(), self.to_screen.clone(), email.trim().to_owned());

        self.spawn(async move {
            match api.add_friend(&email).await {
                Ok(_) => match api.friends().await {
                    Ok(friends) => {
                        let _ = screen.send(Update::Friends(friends));
                    }
                    Err(failure) => {
                        let _ = screen.send(Update::Complaint(said(&failure)));
                    }
                },
                Err(failure) => {
                    let _ = screen.send(Update::Complaint(said(&failure)));
                }
            }
        });
    }

    pub fn answer_friend(self: &Rc<Self>, friendship: i64, accept: bool) {
        let (api, screen) = (self.api.clone(), self.to_screen.clone());

        self.spawn(async move {
            if let Err(failure) = api.answer_friend(friendship, accept).await {
                let _ = screen.send(Update::Complaint(said(&failure)));

                return;
            }

            if let Ok(friends) = api.friends().await {
                let _ = screen.send(Update::Friends(friends));
            }
        });
    }

    pub fn load_conversations(self: &Rc<Self>) {
        let (api, screen) = (self.api.clone(), self.to_screen.clone());

        self.spawn(async move {
            match api.conversations().await {
                Ok(conversations) => {
                    let _ = screen.send(Update::Conversations(conversations));
                }
                Err(failure) => {
                    let _ = screen.send(Update::Complaint(said(&failure)));
                }
            }
        });
    }

    /// Abre a conversa com alguém e a marca como lida: o contador só zera assim.
    pub fn open_conversation(self: &Rc<Self>, person: Person) {
        let (api, screen) = (self.api.clone(), self.to_screen.clone());

        self.spawn(async move {
            match api.direct_messages(person.id).await {
                Ok(messages) => {
                    let _ = api.read_conversation(person.id).await;
                    let _ = screen.send(Update::Direct { person, messages });

                    if let Ok(conversations) = api.conversations().await {
                        let _ = screen.send(Update::Conversations(conversations));
                    }
                }
                Err(failure) => {
                    let _ = screen.send(Update::Complaint(said(&failure)));
                }
            }
        });
    }

    pub fn send_direct(self: &Rc<Self>, person: Person, body: &str) {
        let (api, screen, body) = (self.api.clone(), self.to_screen.clone(), body.to_owned());

        self.spawn(async move {
            if let Err(failure) = api.send_direct(person.id, &body).await {
                let _ = screen.send(Update::Complaint(said(&failure)));

                return;
            }

            if let Ok(messages) = api.direct_messages(person.id).await {
                let _ = screen.send(Update::Direct { person, messages });
            }

            if let Ok(conversations) = api.conversations().await {
                let _ = screen.send(Update::Conversations(conversations));
            }
        });
    }

    pub fn open_server(self: &Rc<Self>, server: i64) {
        let (api, screen) = (self.api.clone(), self.to_screen.clone());
        *lock(&self.opened) = Some(server);

        // O que o núcleo já guardou vai para a tela antes do pedido: a coluna de canais não
        // pisca vazia ao trocar de servidor.
        if let Some(tree) = self.api.known_tree(server) {
            let _ = screen.send(Update::Tree(Box::new(tree)));
        }

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

    /// Cria um canal no servidor aberto. A árvore volta inteira: é ela que diz a posição
    /// do canal novo entre os outros.
    pub fn create_channel(self: &Rc<Self>, name: &str, kind: ChannelKind) {
        let Some(server) = *lock(&self.opened) else {
            return;
        };

        let (api, screen, name) = (self.api.clone(), self.to_screen.clone(), name.trim().to_owned());

        self.spawn(async move {
            match api.create_channel(server, &name, kind).await {
                Ok(()) => entered(&api, &screen, server).await,
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

    /// Editar e apagar a própria mensagem. O que aparece na tela é o que o servidor gravou:
    /// o canal é relido em seguida, como no envio.
    pub fn edit_message(self: &Rc<Self>, channel: &str, message: i64, body: &str) {
        let (api, screen, body) = (self.api.clone(), self.to_screen.clone(), body.trim().to_owned());
        let channel = channel.to_owned();

        self.spawn(async move {
            if let Err(failure) = api.edit_message(message, &body).await {
                let _ = screen.send(Update::Complaint(said(&failure)));

                return;
            }

            if let Ok(messages) = api.messages(&channel).await {
                let _ = screen.send(Update::Messages(messages));
            }
        });
    }

    pub fn delete_message(self: &Rc<Self>, channel: &str, message: i64) {
        let (api, screen) = (self.api.clone(), self.to_screen.clone());
        let channel = channel.to_owned();

        self.spawn(async move {
            if let Err(failure) = api.delete_message(message).await {
                let _ = screen.send(Update::Complaint(said(&failure)));

                return;
            }

            if let Ok(messages) = api.messages(&channel).await {
                let _ = screen.send(Update::Messages(messages));
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
    /// Entrar num canal de voz **sem sair do hub**: é o que o React faz, e o que o Discord
    /// fez antes dele. Quem está dentro aparece embaixo do nome do canal.
    pub fn join_voice(self: &Rc<Self>, channel: &str, name: &str) {
        if lock(&self.room).is_some() {
            self.leave_voice();
        }

        self.connect(Ok(channel.to_owned()), Some(channel.to_owned()), Some(name.to_owned()));
    }

    /// Sai da voz e continua no servidor. É o fone cortado da barra de baixo.
    pub fn leave_voice(self: &Rc<Self>) {
        let held = self.close_room();

        let _ = self.to_screen.send(Update::VoiceLeft);

        self.spawn(async move {
            if let Some(room) = held {
                room.leave().await;
            }
        });
    }

    fn enter(self: &Rc<Self>, opened: Result<String, EntryRefusal>, voice: Option<String>) {
        self.connect(opened, voice, None);
    }

    fn connect(
        self: &Rc<Self>,
        opened: Result<String, EntryRefusal>,
        voice: Option<String>,
        staying: Option<String>,
    ) {
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
        let (held, watch, microphone, mine) = (self.room.clone(), self.watch.clone(), self.microphone.clone(), self.mine.clone());
        let deafened = self.deafened.load(Ordering::Relaxed);
        let identity = self.identity(&room, voice);

        self.in_voice.store(staying.is_some(), Ordering::Relaxed);

        self.spawn(async move {
            let (updates, heard) = std::sync::mpsc::channel();
            let (opened, media) = match Room::enter(&url, &room, identity, updates).await {
                Ok(entered) => entered,
                Err(failure) => {
                    let reason = sentence(Failure::from_error(&failure));

                    let _ = screen
                        .send(Update::Complaint(format!("Não deu para entrar na sala. {reason}")));

                    return;
                }
            };

            if deafened {
                opened.deafen(true);
            }

            *lock(&held) = Some(opened.clone());
            *lock(&watch) = Some(Watch::start(media, |_, _| {}));
            *lock(&mine) = streaming::mine_of(&opened.mine());

            listen(heard, screen.clone(), mine.clone());

            let in_voice = staying.is_some();
            let peers = streaming::peers_of(&opened.peers());

            let _ = screen.send(Update::Joined { room, voice: staying, peers });
            let _ = screen.send(Update::Tiles(streaming::tiles_of(&opened.tiles())));
            let _ = screen.send(Update::Mine(*lock(&mine)));

            // Entrar na voz abre o microfone, como no Mac e no React: quem entra já é ouvido.
            // A sala por código é só a tela.
            if in_voice && lock(&mine).can_speak {
                open_microphone(&opened, &microphone, &screen).await;
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
        let Some(room) = lock(&self.room).clone() else {
            return;
        };

        let screen = self.to_screen.clone();

        self.spawn(async move {
            let config = screen_config();

            // No Wayland quem escolhe a tela é o seletor do sistema, e ele abre aqui — antes
            // de ligar a captura. Sem esta chamada o `start` não acha sessão nenhuma e o
            // compartilhamento morre em toda área de trabalho moderna do Linux. No X11 é
            // uma chamada vazia. `block_in_place` porque o portal espera a pessoa responder.
            let prepared = match tokio::task::block_in_place(|| capture::prepare(&config)) {
                Ok(prepared) => prepared,
                Err(failure) => {
                    tracing::warn!(%failure, "o seletor de tela não abriu");

                    let _ = screen.send(Update::Complaint("Não deu para escolher a tela.".into()));

                    return;
                }
            };

            let shared = room.share(config).await;

            // A captura consumiu a sessão escolhida; largá-la antes fecharia o que o seletor
            // abriu, e o sistema ficaria dizendo que a tela está sendo compartilhada.
            drop(prepared);

            if let Err(failure) = shared {
                tracing::warn!(%failure, "a tela não subiu");

                let _ = screen.send(Update::Complaint(
                    "Não deu para compartilhar a tela. Confira se o GStreamer está instalado.".into(),
                ));
            }
        });
    }

    pub fn stop_sharing(self: &Rc<Self>) {
        self.with_room(|room| async move { room.stop_sharing().await });
    }

    /// Procura de novo quem está transmitindo. O `newProducer` já faz isso sozinho; este
    /// caminho existe para quando o evento se perdeu, e é o "Atualizar" da lista de pessoas.
    pub fn refresh_watch(self: &Rc<Self>) {
        self.with_room(|room| async move { room.watch(None).await });
    }

    pub fn toggle_camera(self: &Rc<Self>) {
        let on = lock(&self.mine).camera;
        let screen = self.to_screen.clone();

        self.with_room(move |room| async move {
            if on {
                room.close_captured_camera().await;

                return;
            }

            if let Err(failure) = room.open_captured_camera(camera_config()).await {
                tracing::warn!(%failure, "a câmera não subiu");

                let _ = screen.send(Update::Complaint("Não deu para abrir a câmera.".into()));
            }
        });
    }

    /// Com o microfone aberto, alterna o mudo; fechado, abre. O mudo é do núcleo: manda
    /// silêncio e avisa a sala, e é o `room.mine` que redesenha o botão.
    pub fn toggle_mic(self: &Rc<Self>) {
        let (screen, microphone, mine) = (self.to_screen.clone(), self.microphone.clone(), *lock(&self.mine));

        self.with_room(move |room| async move {
            if mine.mic {
                room.mute_microphone(!mine.mic_muted).await;
            } else {
                open_microphone(&room, &microphone, &screen).await;
            }
        });
    }

    /// Escolher o microfone: o padrão do sistema muda, e a captura reabre para pegá-lo. O
    /// producer continua o mesmo, então a sala não vê ninguém entrar e sair.
    pub fn use_microphone(self: &Rc<Self>, name: &str) {
        if !devices::use_microphone(name) {
            let _ = self.to_screen.send(Update::Complaint("Não deu para trocar o microfone.".into()));

            return;
        }

        let Some(room) = lock(&self.room).clone() else {
            return;
        };

        let mut held = lock(&self.microphone);

        if let Some(mut capturer) = held.take() {
            let _ = capturer.stop();

            match capture_microphone(&room) {
                Ok(capturer) => *held = Some(capturer),
                Err(failure) => {
                    tracing::warn!(%failure, "o microfone novo não abriu");

                    let _ = self.to_screen.send(Update::Complaint("O microfone escolhido não abriu.".into()));
                }
            }
        }
    }

    /// Escolher a saída de áudio. Os tocadores reabrem: quem já está tocando não se muda de
    /// aparelho sozinho.
    pub fn use_speaker(self: &Rc<Self>, name: &str) {
        if !devices::use_speaker(name) {
            let _ = self.to_screen.send(Update::Complaint("Não deu para trocar a saída de áudio.".into()));

            return;
        }

        if let Some(watch) = lock(&self.watch).as_ref() {
            watch.reopen_sound();
        }
    }

    /// Ensurdecer cala só o áudio que chega: pausar o vídeo faria esperar keyframe na volta.
    pub fn toggle_deafen(self: &Rc<Self>) -> bool {
        let deafened = !self.deafened.load(Ordering::Relaxed);

        self.deafened.store(deafened, Ordering::Relaxed);

        if let Some(room) = lock(&self.room).as_ref() {
            room.deafen(deafened);
        }

        deafened
    }

    pub fn is_deafened(&self) -> bool {
        self.deafened.load(Ordering::Relaxed)
    }

    /// O que chegou de vídeo desde o último desenho. É a janela que chama, no relógio dela.
    pub fn fresh_frames(&self) -> Vec<(String, Vec<u8>)> {
        lock(&self.watch).as_ref().map(Watch::fresh_frames).unwrap_or_default()
    }

    /// Tirado da sala pelo servidor: sai do que estiver aberto e diz por quê.
    pub fn thrown_out(self: &Rc<Self>, message: &str) {
        if self.in_voice.load(Ordering::Relaxed) {
            self.leave_voice();
        } else {
            self.leave_room();
        }

        let _ = self.to_screen.send(Update::Complaint(message.to_owned()));
    }

    pub fn leave_room(self: &Rc<Self>) {
        self.core.leave_room();

        let (screen, landing) = (self.to_screen.clone(), self.home());
        let held = self.close_room();

        let _ = screen.send(Update::Tiles(Vec::new()));
        let _ = screen.send(Update::Mine(Mine::default()));
        let _ = screen.send(Update::Show(landing));

        self.spawn(async move {
            if let Some(room) = held {
                room.leave().await;
            }
        });
    }

    /// Fecha deste lado o que a sala abriu — o microfone e os tocadores — e devolve a sala
    /// para quem chama avisar o servidor da saída.
    fn close_room(&self) -> Option<Arc<Room>> {
        if let Some(mut capturer) = lock(&self.microphone).take() {
            let _ = capturer.stop();
        }

        drop(lock(&self.watch).take());
        *lock(&self.mine) = Mine::default();
        self.in_voice.store(false, Ordering::Relaxed);

        lock(&self.room).take()
    }

    /// Um clique que vira pedido à sala aberta, fora da thread da janela.
    fn with_room<F, Work>(&self, work: F)
    where
        F: FnOnce(Arc<Room>) -> Work,
        Work: std::future::Future<Output = ()> + Send + 'static,
    {
        if let Some(room) = lock(&self.room).clone() {
            self.spawn(work(room));
        }
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
/// Servidor recém-criado ou recém-entrado: a lista muda e a árvore dele abre. Está aqui e
/// não no `Bridge` porque o `Rc` dele não atravessa a thread do runtime.
async fn entered(api: &Arc<Api>, screen: &UnboundedSender<Update>, server: i64) {
    if let Ok(servers) = api.servers().await {
        let _ = screen.send(Update::Servers(servers));
    }

    match api.tree(server).await {
        Ok(tree) => {
            let _ = screen.send(Update::Tree(Box::new(tree)));
        }
        Err(failure) => {
            let _ = screen.send(Update::Complaint(said(&failure)));
        }
    }
}

fn lock<T>(cell: &Arc<Mutex<T>>) -> MutexGuard<'_, T> {
    cell.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A tela principal em 1080p60, com o som do sistema e o cursor.
fn screen_config() -> CaptureConfig {
    CaptureConfig {
        quality: capture::Quality::Hd1080,
        source: CaptureSource::PrimaryDisplay,
        frame_rate: 60,
        capture_audio: true,
        mute_listed_apps: true,
        show_cursor: true,
    }
}

fn camera_config() -> CaptureConfig {
    CaptureConfig {
        source: CaptureSource::Camera(0),
        capture_audio: false,
        ..CaptureConfig::default()
    }
}

/// O microfone do sistema, capturado pelo GStreamer, subindo pela sala.
fn capture_microphone(room: &Arc<Room>) -> Result<PlatformCapturer, capture::CaptureError> {
    let speaking = Arc::clone(room);
    let config = CaptureConfig {
        source: CaptureSource::Microphone,
        capture_audio: true,
        ..CaptureConfig::default()
    };

    PlatformCapturer::start(&config, move |event| {
        if let CaptureEvent::Audio(block) = event {
            speaking.speak(&block.samples);
        }
    })
}

/// Abre o microfone na sala e a captura que o alimenta.
async fn open_microphone(room: &Arc<Room>, cell: &Arc<Mutex<Option<PlatformCapturer>>>, screen: &UnboundedSender<Update>) {
    if let Err(failure) = room.open_microphone().await {
        tracing::warn!(%failure, "a sala não abriu o microfone");

        let _ = screen.send(Update::Complaint("Não deu para abrir o microfone.".into()));

        return;
    }

    // Abrir o `gst-launch` bloqueia; o tokio é avisado para não esperar esta thread.
    match tokio::task::block_in_place(|| capture_microphone(room)) {
        Ok(capturer) => *lock(cell) = Some(capturer),
        Err(failure) => {
            tracing::warn!(%failure, "a captura do microfone não abriu");
            room.close_microphone().await;

            let _ = screen.send(Update::Complaint("Não deu para abrir o microfone.".into()));
        }
    }
}

/// Os avisos da sala, numa thread só deles, virando os `Update` que as telas já entendem.
/// A fila fecha quando a sala acaba, e a thread acaba junto.
fn listen(heard: std::sync::mpsc::Receiver<String>, screen: UnboundedSender<Update>, mine: Arc<Mutex<Mine>>) {
    std::thread::spawn(move || {
        for said in heard {
            let Ok(update) = serde_json::from_str::<Value>(&said) else {
                continue;
            };

            if let Some(update) = translate(&update, &mine) {
                let _ = screen.send(update);
            }
        }
    });
}

fn translate(update: &Value, mine: &Arc<Mutex<Mine>>) -> Option<Update> {
    let data = &update["data"];

    Some(match update["event"].as_str()? {
        "room.peers" => Update::Peers(streaming::peers_of(data)),
        "room.tiles" => Update::Tiles(streaming::tiles_of(data)),
        "room.mine" => {
            let now = streaming::mine_of(data);

            *lock(mine) = now;

            Update::Mine(now)
        }
        "room.ping" => Update::Ping(data["ms"].as_u64()?),
        "room.failed" => Update::Complaint(
            match data["what"].as_str()? {
                "watch" => "Não deu para assistir a uma das transmissões.",
                "mic" => "Não deu para abrir o microfone.",
                _ => "Não deu para compartilhar a tela.",
            }
            .into(),
        ),
        "room.session" => match data["state"].as_str()? {
            "lost" => Update::Complaint("A sala caiu. Voltando…".into()),
            "rejoined" => Update::Complaint(String::new()),
            "gone" => Update::Complaint("A sala não voltou. Entre de novo quando a internet estabilizar.".into()),
            "replaced" => Update::ThrownOut("Esta conta entrou na sala por outro lugar."),
            "kicked" => Update::ThrownOut("Você foi removido desta sala."),
            _ => return None,
        },
        _ => return None,
    })
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
