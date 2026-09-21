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
use core_app::models::{
    Channel, ChannelKind, Conversation, DirectMessage, Friendship, FriendshipStatus, Person, RoomIdentity,
    ServerSummary, ServerTree, User,
};
use core_app::protocol::local;
use core_app::reconnect::Backoff;
use core_app::session::Session;
use core_app::{App, Failure, Screen};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel, Weak};
use storage::Storage;
use tokio::runtime::Runtime;

use crate::devices::{self, Device};
use crate::sharing;
use crate::{
    AppWindow, ChannelRow, ConversationRow, DeviceRow, FriendRow, MemberRow, MessageRow, PeerRow, ServerRow, Ui,
};

const DEFAULT_SERVER: &str = "https://unkvoid.com";

/// ponytail: a tela sobe, mas o app ainda não **assiste** ao que os outros mandam, e a
/// câmera e o microfone continuam só como botão. Teto: quem transmite é visto, quem olha
/// não vê. A saída é portar o `watching` do `apps/linux` e ligar `Source::Mic`/`Camera` ao
/// mesmo `core_app::sharing` que a tela já usa.
const NO_CAPTURE: &str = "A captura ainda não está ligada nesta versão do app.";

pub struct Bridge {
    runtime: Runtime,
    core: Arc<App>,
    api: Arc<Api>,
    window: Weak<AppWindow>,
    /// De onde sai o WebSocket: vem do `GET /api/config`, e até ele responder não há sala.
    sfu: Arc<Mutex<Option<String>>>,
    session: Arc<Mutex<Option<Arc<Session>>>>,
    /// Desde quando se está na sala. O relógio da barra conta a partir daqui.
    since: Arc<Mutex<Option<std::time::Instant>>>,
    /// O tique de um segundo que escreve esse relógio. Vive enquanto a janela viver.
    clock: slint::Timer,
    /// A sessão de mídia: um socket e uma chave SRTP para tudo o que sobe. É a mesma do
    /// `core_app::sharing` que o app do Tauri usa, e por isso a captura aqui é a de lá.
    media: Arc<core_app::sharing::ActiveSession>,
    /// O que a tela mostra por índice, e o que o servidor conhece por identificador.
    servers: Arc<Mutex<Vec<ServerSummary>>>,
    channels: Arc<Mutex<Vec<Channel>>>,
    reading: Arc<Mutex<Option<String>>>,
    /// Qual servidor está aberto: é nele que um canal novo nasce.
    opened: Arc<Mutex<Option<i64>>>,
    /// A Home: as conversas e as amizades que a tela mostra por índice, e com quem se está
    /// falando agora.
    conversations: Arc<Mutex<Vec<Conversation>>>,
    friends: Arc<Mutex<Vec<Friendship>>>,
    talking: Arc<Mutex<Option<Person>>>,
    /// Quem sou eu para o servidor. É o que decide se um pedido de amizade chegou ou saiu.
    me: Arc<Mutex<Option<i64>>>,
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
            since: Arc::default(),
            clock: slint::Timer::default(),
            media: Arc::default(),
            servers: Arc::default(),
            channels: Arc::default(),
            reading: Arc::default(),
            opened: Arc::default(),
            conversations: Arc::default(),
            friends: Arc::default(),
            talking: Arc::default(),
            me: Arc::default(),
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

        self.clock.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(1), {
            let (window, since) = (self.window.clone(), self.since.clone());

            move || {
                let Some(started) = *lock(&since) else {
                    return;
                };

                let seconds = started.elapsed().as_secs();
                let face = format!("{}:{:02}:{:02}", seconds / 3600, (seconds / 60) % 60, seconds % 60);

                if let Some(app) = window.upgrade() {
                    app.global::<Ui>().set_elapsed(face.into());
                }
            }
        });

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

        ui.on_create_channel({
            let bridge = self.clone();

            move |name, voice| bridge.create_channel(&name, voice)
        });

        ui.on_open_channel({
            let bridge = self.clone();

            move |index| bridge.open_channel(index)
        });

        ui.on_edit_message({
            let bridge = self.clone();

            move |id, body| bridge.edit_message(id, &body)
        });

        ui.on_delete_message({
            let bridge = self.clone();

            move |id| bridge.delete_message(id)
        });

        ui.on_send_message({
            let bridge = self.clone();

            move |body| bridge.send_message(&body)
        });

        ui.on_leave_voice({
            let bridge = self.clone();

            move || bridge.leave_voice()
        });

        ui.on_show_hub_home({
            let bridge = self.clone();

            move || bridge.show_hub_home()
        });

        ui.on_create_server({
            let bridge = self.clone();

            move |name| bridge.create_server(&name)
        });

        ui.on_join_invite({
            let bridge = self.clone();

            move |code| bridge.join_invite(&code)
        });

        ui.on_load_friends({
            let bridge = self.clone();

            move || bridge.load_friends()
        });

        ui.on_add_friend({
            let bridge = self.clone();

            move |email| bridge.add_friend(&email)
        });

        ui.on_answer_friend({
            let bridge = self.clone();

            move |friendship, accept| bridge.answer_friend(friendship, accept)
        });

        ui.on_open_conversation({
            let bridge = self.clone();

            move |index| bridge.open_conversation(index)
        });

        ui.on_talk_to_friend({
            let bridge = self.clone();

            move |index| bridge.talk_to_friend(index)
        });

        ui.on_send_direct({
            let bridge = self.clone();

            move |body| bridge.send_direct(&body)
        });

        ui.on_open_recent_room({
            let bridge = self.clone();

            move |code| bridge.enter(bridge.core.join_room("", &code), None)
        });

        ui.on_leave_room({
            let bridge = self.clone();

            move || bridge.leave_room()
        });

        ui.on_toggle_share({
            let bridge = self.clone();

            move || bridge.toggle_share()
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

        ui.on_open_settings({
            let bridge = self.clone();

            move || {
                // A lista é lida na hora de abrir: aparelho ligado depois que o app abriu
                // tem de aparecer sem reiniciar nada.
                bridge.show_devices(true);
                bridge.show_devices(false);
                paint(&bridge.window, |app| app.global::<Ui>().set_settings_open(true));
            }
        });

        ui.on_close_settings({
            let window = self.window.clone();

            move || paint(&window, |app| app.global::<Ui>().set_settings_open(false))
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
        let sfu = self.sfu.clone();
        let landing = self.landing();

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

            landed(&core, &api, &window, &landing, user).await;
        });
    }

    fn sign_in(self: &Rc<Self>, email: &str, password: &str, register: bool) {
        let (core, api, window) = (self.core.clone(), self.api.clone(), self.window.clone());
        let (email, password) = (email.to_owned(), password.to_owned());
        let landing = self.landing();

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

                    landed(&core, &api, &window, &landing, user).await;
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
            ui.set_text_channels(ModelRc::default());
            ui.set_voice_channels(ModelRc::default());
            ui.set_members(ModelRc::default());
            ui.set_messages(ModelRc::default());
            ui.set_complaint(SharedString::new());
            ui.set_screen(named(landing).into());
        });
    }

    fn show(self: &Rc<Self>, screen: Screen) {
        // O núcleo é quem guarda em que tela o app está: escrever nos dois no mesmo lugar é
        // o que os impede de discordar.
        self.core.show(screen);

        paint(&self.window, move |app| {
            let ui = app.global::<Ui>();

            // Um erro pertence à tela que o levantou. Sem isto, a recusa do login aparecia
            // dentro do hub, e a da sala aparecia na entrada.
            ui.set_complaint(SharedString::new());
            ui.set_screen(named(screen).into());
        });
    }

    /// Volta para a Home e recarrega o que ela mostra.
    fn show_hub_home(self: &Rc<Self>) {
        paint(&self.window, |app| app.global::<Ui>().set_in_server(false));

        let (api, window) = (self.api.clone(), self.window.clone());
        let (landing, held) = (self.landing(), self.conversations.clone());

        self.spawn(async move {
            refresh_servers(&api, &window, &landing).await;

            if let Ok(open) = api.conversations().await {
                show_conversations(&window, &held, open);
            }
        });
    }

    fn create_server(self: &Rc<Self>, name: &str) {
        let name = name.trim().to_owned();

        if name.is_empty() {
            return;
        }

        let (api, window) = (self.api.clone(), self.window.clone());
        let landing = self.landing();

        self.spawn(async move {
            match api.create_server(&name).await {
                Ok(_) => refresh_servers(&api, &window, &landing).await,
                Err(failure) => complain(&window, said(&failure)),
            }
        });
    }

    fn join_invite(self: &Rc<Self>, code: &str) {
        let code = code.trim().to_owned();

        if code.is_empty() {
            return;
        }

        let (api, window) = (self.api.clone(), self.window.clone());
        let landing = self.landing();

        self.spawn(async move {
            match api.join_invite(&code).await {
                Ok(_) => refresh_servers(&api, &window, &landing).await,
                Err(failure) => complain(&window, said(&failure)),
            }
        });
    }

    fn load_friends(self: &Rc<Self>) {
        let (api, window) = (self.api.clone(), self.window.clone());
        let (held, me) = (self.friends.clone(), self.me.clone());

        self.spawn(async move {
            match api.friends().await {
                Ok(friends) => show_friends(&window, &held, &me, friends),
                Err(failure) => complain(&window, said(&failure)),
            }
        });
    }

    fn add_friend(self: &Rc<Self>, email: &str) {
        let email = email.trim().to_owned();

        if email.is_empty() {
            return;
        }

        let (api, window) = (self.api.clone(), self.window.clone());
        let (held, me) = (self.friends.clone(), self.me.clone());

        self.spawn(async move {
            if let Err(failure) = api.add_friend(&email).await {
                complain(&window, said(&failure));

                return;
            }

            if let Ok(friends) = api.friends().await {
                show_friends(&window, &held, &me, friends);
            }
        });
    }

    fn answer_friend(self: &Rc<Self>, friendship: i32, accept: bool) {
        let (api, window) = (self.api.clone(), self.window.clone());
        let (held, me) = (self.friends.clone(), self.me.clone());

        self.spawn(async move {
            if let Err(failure) = api.answer_friend(i64::from(friendship), accept).await {
                complain(&window, said(&failure));

                return;
            }

            if let Ok(friends) = api.friends().await {
                show_friends(&window, &held, &me, friends);
            }
        });
    }


    fn open_conversation(self: &Rc<Self>, index: i32) {
        let Some(conversation) = at(&self.conversations, index) else {
            return;
        };

        self.talk_with(conversation.user);
    }

    fn talk_to_friend(self: &Rc<Self>, index: i32) {
        let Some(friend) = at(&self.friends, index) else {
            return;
        };

        let me = *lock(&self.me);
        let other = if Some(friend.requester.id) == me { friend.addressee } else { friend.requester };

        self.talk_with(other);
    }

    /// Abre a conversa com alguém e a marca como lida: o contador só zera assim.
    fn talk_with(self: &Rc<Self>, person: Person) {
        let (api, window) = (self.api.clone(), self.window.clone());
        let (talking, held) = (self.talking.clone(), self.conversations.clone());

        *lock(&talking) = Some(person.clone());

        self.spawn(async move {
            match api.direct_messages(person.id).await {
                Ok(messages) => {
                    let _ = api.read_conversation(person.id).await;

                    show_direct(&window, &person, messages);

                    if let Ok(conversations) = api.conversations().await {
                        show_conversations(&window, &held, conversations);
                    }
                }
                Err(failure) => complain(&window, said(&failure)),
            }
        });
    }

    fn send_direct(self: &Rc<Self>, written: &str) {
        let Some(person) = lock(&self.talking).clone() else {
            return;
        };

        let written = written.trim().to_owned();

        if written.is_empty() {
            return;
        }

        let (api, window) = (self.api.clone(), self.window.clone());
        let held = self.conversations.clone();

        self.spawn(async move {
            if let Err(failure) = api.send_direct(person.id, &written).await {
                complain(&window, said(&failure));

                return;
            }

            if let Ok(messages) = api.direct_messages(person.id).await {
                show_direct(&window, &person, messages);
            }

            if let Ok(conversations) = api.conversations().await {
                show_conversations(&window, &held, conversations);
            }
        });
    }

    fn landing(self: &Rc<Self>) -> Landing {
        Landing {
            known: self.servers.clone(),
            me: self.me.clone(),
            conversations: self.conversations.clone(),
        }
    }

    fn opening(self: &Rc<Self>) -> Opening {
        Opening {
            channels: self.channels.clone(),
            reading: self.reading.clone(),
            known: self.servers.clone(),
            me: *lock(&self.me),
        }
    }

    fn open_server(self: &Rc<Self>, index: i32) {
        let Some(server) = at(&self.servers, index) else {
            return;
        };

        let (api, window) = (self.api.clone(), self.window.clone());
        let opening = self.opening();
        let id = server.id;

        *lock(&self.opened) = Some(id);

        // O que o núcleo já guardou vai para a tela antes do pedido: a coluna de canais
        // não pisca vazia ao trocar de servidor.
        if let Some(tree) = self.api.known_tree(id) {
            paint_tree(&window, &opening, &tree);
        }

        self.spawn(async move {
            show_tree(&api, &window, &opening, id).await;
        });
    }

    fn create_channel(self: &Rc<Self>, name: &str, voice: bool) {
        let Some(server) = *lock(&self.opened) else {
            return;
        };

        let name = name.trim().to_owned();

        if name.is_empty() {
            return;
        }

        let kind = if voice { ChannelKind::Voice } else { ChannelKind::Text };
        let (api, window) = (self.api.clone(), self.window.clone());
        let opening = self.opening();

        self.spawn(async move {
            match api.create_channel(server, &name, kind).await {
                // A árvore volta inteira: é ela que diz a posição do canal novo entre os outros.
                Ok(()) => show_tree(&api, &window, &opening, server).await,
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
            if lock(&self.session).is_some() {
                self.leave_voice();
            }

            self.join_voice(&channel);

            return;
        }

        *lock(&self.reading) = Some(channel.id.clone());

        let (api, window) = (self.api.clone(), self.window.clone());
        let (id, name) = (channel.id.clone(), channel.name.clone());
        let (chosen, mine) = (index as usize, *lock(&self.me));

        let (text, voice) = split_channels(&lock(&self.channels), Some(chosen));

        paint(&window, move |app| {
            let ui = app.global::<Ui>();

            ui.set_text_channels(model(text));
            ui.set_voice_channels(model(voice));
            ui.set_channel_name(format!("# {name}").into());
        });

        self.spawn(async move {
            read_channel(&api, &window, &id, mine).await;
        });
    }

    /// Editar e apagar a própria mensagem. O que aparece na tela é o que o servidor gravou:
    /// o canal é relido em seguida, como no envio.
    fn edit_message(self: &Rc<Self>, id: i32, body: &str) {
        let Some(channel) = lock(&self.reading).clone() else {
            return;
        };

        let (api, window, body) = (self.api.clone(), self.window.clone(), body.trim().to_owned());
        let mine = *lock(&self.me);

        if body.is_empty() {
            return;
        }

        self.spawn(async move {
            if let Err(failure) = api.edit_message(i64::from(id), &body).await {
                complain(&window, said(&failure));

                return;
            }

            read_channel(&api, &window, &channel, mine).await;
        });
    }

    fn delete_message(self: &Rc<Self>, id: i32) {
        let Some(channel) = lock(&self.reading).clone() else {
            return;
        };

        let (api, window) = (self.api.clone(), self.window.clone());
        let mine = *lock(&self.me);

        self.spawn(async move {
            if let Err(failure) = api.delete_message(i64::from(id)).await {
                complain(&window, said(&failure));

                return;
            }

            read_channel(&api, &window, &channel, mine).await;
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
        let mine = *lock(&self.me);

        self.spawn(async move {
            if let Err(failure) = api.send_message(&channel, &body).await {
                complain(&window, said(&failure));

                return;
            }

            // Reler o canal em vez de emendar a mensagem na lista: o que aparece é o que o
            // servidor gravou, e não o que este app achou que mandou.
            read_channel(&api, &window, &channel, mine).await;
        });
    }

    fn enter(self: &Rc<Self>, opened: Result<String, EntryRefusal>, voice: Option<String>) {
        self.connect(opened, voice, None);
    }

    /// Entrar num canal de voz **sem sair do hub**: é o que o React faz, e o que o Discord
    /// fez antes dele. Quem está dentro aparece embaixo do nome do canal, e a tela continua
    /// sendo a do servidor.
    fn join_voice(self: &Rc<Self>, channel: &Channel) {
        self.connect(Ok(channel.id.clone()), Some(channel.id.clone()), Some(channel.name.clone()));
    }

    /// Sai da voz e continua no servidor. É o fone cortado da barra de baixo.
    fn leave_voice(self: &Rc<Self>) {
        let window = self.window.clone();
        let held = lock(&self.session).take();

        *lock(&self.since) = None;

        paint(&window, |app| {
            let ui = app.global::<Ui>();

            ui.set_voice_channel(SharedString::new());
            ui.set_voice_name(SharedString::new());
            ui.set_peers(ModelRc::default());
            ui.set_elapsed("0:00:00".into());
            ui.set_ping("-- ms".into());
            ui.set_ping_ms(-1);
            ui.set_sharing(false);
        });

        self.spawn(async move {
            if let Some(session) = held
                && let Err(failure) = session.leave().await
            {
                tracing::warn!(%failure, "a saída da voz não foi confirmada");
            }
        });
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
        let started = self.since.clone();
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
            *lock(&started) = Some(std::time::Instant::now());

            let (code, can_speak) = (room.clone(), session.can("speak"));
            let peers = peer_rows(&session);

            paint(&window, move |app| {
                let ui = app.global::<Ui>();

                ui.set_complaint(SharedString::new());
                ui.set_can_speak(can_speak);
                ui.set_peers(model(peers));

                match staying {
                    // Canal de voz: o hub continua na tela, e o canal aberto se marca.
                    Some(name) => {
                        ui.set_voice_channel(code.clone().into());
                        ui.set_voice_name(name.into());
                    }
                    None => {
                        ui.set_room_code(code.into());
                        ui.set_screen("room".into());
                    }
                }
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
                    local::PING_MEASURED => {
                        if let Some(milliseconds) = event.data.as_u64() {
                            let said = format!("{milliseconds} ms");
                            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                            let measured = milliseconds as i32;

                            paint(&window, move |app| {
                                let ui = app.global::<Ui>();

                                ui.set_ping(said.into());
                                ui.set_ping_ms(measured);
                            });
                        }
                    }
                    _ => {}
                }

                if changed {
                    let peers = peer_rows(&session);

                    paint(&window, move |app| app.global::<Ui>().set_peers(model(peers)));
                }
            }
        });
    }

    /// Liga ou desliga a transmissão da tela. O botão só reflete o que de fato subiu: a
    /// tela vira violeta depois do `producePlain`, não no clique.
    fn toggle_share(self: &Rc<Self>) {
        let Some(session) = lock(&self.session).clone() else {
            return;
        };

        let (media, window) = (self.media.clone(), self.window.clone());

        self.spawn(async move {
            let sharing = media.0.lock().await.screen.is_some();

            if sharing {
                sharing::stop_screen(&session, &media).await;
            } else if let Err(failure) = sharing::share_screen(&session, &media).await {
                complain(&window, sharing::said(&failure));
            }

            let live = media.0.lock().await.screen.is_some();

            paint(&window, move |app| app.global::<Ui>().set_sharing(live));
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

        *lock(&self.since) = None;

        paint(&window, |app| app.global::<Ui>().set_elapsed("0:00:00".into()));

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

/// Servidor recém-criado ou recém-entrado: a lista muda e a Home a mostra.
async fn refresh_servers(api: &Arc<Api>, window: &Weak<AppWindow>, landing: &Landing) {
    let (known, me) = (&landing.known, &landing.me);
    match api.servers().await {
        Ok(servers) => {
            *lock(known) = servers.clone();

            let rows = rows_of(&servers, None, *lock(me));

            api.warm_trees(&servers.iter().map(|server| server.id).collect::<Vec<_>>()).await;

            paint(window, move |app| {
                let ui = app.global::<Ui>();

                ui.set_servers(model(rows));
                ui.set_in_server(false);
            });
        }
        Err(failure) => complain(window, said(&failure)),
    }
}

/// As amizades, já decididas: quem pediu a quem é conta do Rust, não da tela.
fn show_friends(
    window: &Weak<AppWindow>,
    held: &Arc<Mutex<Vec<Friendship>>>,
    me: &Arc<Mutex<Option<i64>>>,
    friends: Vec<Friendship>,
) {
    let mine = *lock(me);

    *lock(held) = friends.clone();

    let waiting = friends
        .iter()
        .filter(|friend| friend.status == FriendshipStatus::Pending && Some(friend.addressee.id) == mine)
        .count();

    let rows: Vec<FriendRow> = friends
        .iter()
        .map(|friend| {
            let other = if Some(friend.requester.id) == mine { &friend.addressee } else { &friend.requester };
            let state = match friend.status {
                FriendshipStatus::Accepted => "accepted",
                FriendshipStatus::Blocked => "blocked",
                FriendshipStatus::Pending if Some(friend.addressee.id) == mine => "incoming",
                FriendshipStatus::Pending => "waiting",
            };

            FriendRow {
                id: friend.id as i32,
                #[allow(clippy::cast_possible_truncation)]
                user_id: other.id as i32,
                initial: initial(&other.name),
                name: other.name.clone().into(),
                state: state.into(),
            }
        })
        .collect();

    paint(window, move |app| {
        let ui = app.global::<Ui>();

        ui.set_friends(model(rows));
        ui.set_pending_count(waiting as i32);
    });
}

fn show_conversations(
    window: &Weak<AppWindow>,
    held: &Arc<Mutex<Vec<Conversation>>>,
    conversations: Vec<Conversation>,
) {
    *lock(held) = conversations.clone();

    let rows: Vec<ConversationRow> = conversations
        .iter()
        .map(|conversation| ConversationRow {
            user_id: conversation.user.id as i32,
            initial: initial(&conversation.user.name),
            name: conversation.user.name.clone().into(),
            last: if conversation.last.mine {
                format!("você: {}", conversation.last.body).into()
            } else {
                conversation.last.body.clone().into()
            },
            unread: conversation.unread as i32,
        })
        .collect();

    paint(window, move |app| app.global::<Ui>().set_conversations(model(rows)));
}

fn show_direct(window: &Weak<AppWindow>, person: &Person, messages: Vec<DirectMessage>) {
    let rows: Vec<MessageRow> = messages
        .iter()
        .map(|message| MessageRow {
            id: 0,
            initial: initial(&message.sender.name),
            author: message.sender.name.clone().into(),
            body: message.body.clone().into(),
            at: at_of(&message.created_at),
            mine: false,
        })
        .collect();

    let name: SharedString = person.name.clone().into();

    paint(window, move |app| {
        let ui = app.global::<Ui>();

        ui.set_talking_name(name);
        ui.set_direct_messages(model(rows));
        ui.set_in_server(false);
        ui.set_home_tab("direct".into());
    });
}

/// A hora que a linha mostra: o `HH:MM` do carimbo ISO, sem data e sem fuso.
fn at_of(stamp: &str) -> SharedString {
    stamp.split('T').nth(1).map(|time| &time[..5.min(time.len())]).unwrap_or_default().into()
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
        ui.set_complaint(SharedString::new());
        ui.set_screen(named(screen).into());
    });
}

/// Onde o app cai quando o servidor responde: com conta, nos servidores; sem conta, no
/// código. Quem decide é o núcleo.
/// O que a aterrissagem guarda: quem sou eu, os servidores, as conversas e as árvores deles.
#[derive(Clone)]
struct Landing {
    known: Arc<Mutex<Vec<ServerSummary>>>,
    me: Arc<Mutex<Option<i64>>>,
    conversations: Arc<Mutex<Vec<Conversation>>>,
}

async fn landed(core: &Arc<App>, api: &Arc<Api>, window: &Weak<AppWindow>, landing: &Landing, user: Option<User>) {
    let (known, me, conversations) = (&landing.known, &landing.me, &landing.conversations);
    let landing = core.home();
    let name = user.as_ref().map(|user| user.name.clone()).unwrap_or_default();
    let signed_in = user.is_some();
    let saved = core.state().name;

    // Quem sou eu decide se um pedido de amizade chegou ou saiu — e isso é lido em toda
    // lista de amigos daqui para a frente.
    let mine = user.as_ref().map(|user| user.id);

    *lock(me) = mine;

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
                let rows = rows_of(&servers, None, mine);

                paint(window, move |app| app.global::<Ui>().set_servers(model(rows)));
                api.warm_trees(&servers.iter().map(|server| server.id).collect::<Vec<_>>()).await;

                *lock(known) = servers;
            }
            Err(failure) => complain(window, said(&failure)),
        }

        if let Ok(open) = api.conversations().await {
            show_conversations(window, conversations, open);
        }

        // Só as três últimas: é o que o dono quer ver na Home, e o núcleo guarda mais.
        let recent: Vec<String> = core.recent_rooms().into_iter().take(3).collect();
        let recent: Vec<Vec<String>> = recent.chunks(2).map(<[String]>::to_vec).collect();

        paint(window, move |app| app.global::<Ui>().set_recent_rooms(model(code_lines(recent))));
    }
}

async fn read_channel(api: &Arc<Api>, window: &Weak<AppWindow>, channel: &str, me: Option<i64>) {
    match api.messages(channel).await {
        Ok(messages) => {
            #[allow(clippy::cast_possible_truncation)]
            let rows: Vec<MessageRow> = messages
                .iter()
                .map(|message| MessageRow {
                    id: message.id as i32,
                    initial: initial(&message.user.name),
                    author: message.user.name.clone().into(),
                    body: message.body.clone().into(),
                    at: message.created_at.get(11..16).unwrap_or_default().into(),
                    mine: Some(message.user.id) == me,
                })
                .collect();

            paint(window, move |app| app.global::<Ui>().set_messages(model(rows)));
        }
        Err(failure) => complain(window, said(&failure)),
    }
}

fn rows_of(servers: &[ServerSummary], chosen: Option<usize>, me: Option<i64>) -> Vec<ServerRow> {
    servers
        .iter()
        .enumerate()
        .map(|(index, server)| ServerRow {
            initial: initial(&server.name),
            name: server.name.clone().into(),
            current: Some(index) == chosen,
            note: if Some(server.owner_id) == me { "dono" } else { "membro" }.into(),
            at: server.last_accessed_at.as_deref().map(day).unwrap_or_default().into(),
        })
        .collect()
}

/// A data que a linha mostra, no formato que o React escreve com `toLocaleDateString('pt-BR')`.
/// O que chega é ISO, e só o dia interessa.
fn day(stamp: &str) -> String {
    let Some((date, _)) = stamp.split_once('T') else {
        return String::new();
    };

    let parts: Vec<&str> = date.split('-').collect();

    match parts.as_slice() {
        [year, month, day] => format!("{day}/{month}/{year}"),
        _ => String::new(),
    }
}

/// As últimas salas por código, quebradas em linhas de três: o Slint não embrulha sozinho, e
/// no React elas descem de linha quando não cabem. O `ModelRc` não atravessa thread, então
/// quem monta os modelos é a própria pintura.
fn code_lines(codes: Vec<Vec<String>>) -> Vec<ModelRc<SharedString>> {
    codes
        .into_iter()
        .map(|line| model(line.into_iter().map(SharedString::from).collect()))
        .collect()
}

/// A tela mostra texto e voz em duas seções, como o React; o núcleo só conhece uma lista.
/// Cada linha leva a posição dela na lista inteira, que é o que volta no clique.
/// Abre o servidor na tela: os canais, quem está dentro e o convite, tudo da mesma árvore.
/// O que desenhar um servidor precisa ter em mãos. Anda junto porque as duas horas em que
/// ele é desenhado — o que já estava guardado e o que o servidor respondeu — usam tudo.
#[derive(Clone)]
struct Opening {
    channels: Arc<Mutex<Vec<Channel>>>,
    reading: Arc<Mutex<Option<String>>>,
    known: Arc<Mutex<Vec<ServerSummary>>>,
    me: Option<i64>,
}

async fn show_tree(api: &Arc<Api>, window: &Weak<AppWindow>, opening: &Opening, id: i64) {
    let tree = match api.tree(id).await {
        Ok(tree) => tree,
        Err(failure) => return complain(window, said(&failure)),
    };

    paint_tree(window, opening, &tree);
}

/// A árvore na tela. Fica separada do pedido porque o que já está em mãos é desenhado antes
/// dele — e o mesmo desenho serve às duas horas.
fn paint_tree(window: &Weak<AppWindow>, opening: &Opening, tree: &ServerTree) {
    let (channels, reading, known, me) =
        (&opening.channels, &opening.reading, &opening.known, opening.me);
    let ordered = tree.ordered_channels();
    let members: Vec<MemberRow> = tree
        .members
        .iter()
        .map(|member| {
            let name = member.nickname.clone().unwrap_or_else(|| member.name.clone());

            MemberRow {
                initial: initial(&name),
                name: name.into(),
                note: if member.is_owner { "dono" } else { "" }.into(),
            }
        })
        .collect();

    let (text, voice) = split_channels(&ordered, None);

    *lock(channels) = ordered;
    *lock(reading) = None;

    let chosen = lock(known).iter().position(|server| server.id == tree.id);
    let servers = rows_of(&lock(known), chosen, me);
    let name = tree.name.clone();
    let invite = tree.invite_code.clone().unwrap_or_default();

    paint(window, move |app| {
        let ui = app.global::<Ui>();

        ui.set_server_name(name.into());
        ui.set_servers(model(servers));
        ui.set_text_channels(model(text));
        ui.set_voice_channels(model(voice));
        ui.set_members(model(members));
        ui.set_messages(ModelRc::default());
        ui.set_channel_name(SharedString::new());
        ui.set_invite_code(invite.into());
        ui.set_in_server(true);
    });
}

fn split_channels(ordered: &[Channel], chosen: Option<usize>) -> (Vec<ChannelRow>, Vec<ChannelRow>) {
    let mut text = Vec::new();
    let mut voice = Vec::new();

    for (index, channel) in ordered.iter().enumerate() {
        let row = ChannelRow {
            index: index as i32,
            id: channel.id.clone().into(),
            name: channel.name.clone().into(),
            voice: channel.kind == ChannelKind::Voice,
            current: Some(index) == chosen,
        };

        if row.voice {
            voice.push(row);
        } else {
            text.push(row);
        }
    }

    (text, voice)
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
