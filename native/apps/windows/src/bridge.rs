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
use core_app::reconnect::Backoff;
use core_app::room::Room;
use core_app::{App, Failure, Screen};
use slint::{ComponentHandle, Image, Model, ModelRc, SharedString, VecModel, Weak};
use storage::Storage;
use tokio::runtime::Runtime;

use crate::devices::{self, Device};
use crate::sound::{Microphone, Speaker};
use crate::stage::{Stage, Voice, peers_of};
use crate::watching::Watch;
use crate::{
    AppWindow, ChannelRow, ConversationRow, DeviceRow, FriendRow, MemberRow, MessageRow, PeerRow, ServerRow, TileRow,
    Ui,
};

const DEFAULT_SERVER: &str = "https://unkvoid.com";

/// ponytail: a câmera do Windows ainda não existe — o `capture` não abre webcam aqui, e o
/// `Room` só aceita câmera no macOS. Teto: quem está no Windows vê a câmera dos outros mas
/// não liga a dele. A saída é a captura por Media Foundation empurrando `room.show`.
const NO_CAPTURE: &str = "A câmera ainda não está ligada nesta versão do app.";

/// As falhas que a sala anuncia, na frase do Mac.
fn room_failure(what: &str) -> &'static str {
    match what {
        "watch" => "Não deu para assistir a uma das transmissões.",
        "mic" => "Não deu para abrir o microfone.",
        _ => "Não deu para compartilhar a tela.",
    }
}

pub struct Bridge {
    runtime: Runtime,
    core: Arc<App>,
    api: Arc<Api>,
    window: Weak<AppWindow>,
    /// De onde sai o WebSocket: vem do `GET /api/config`, e até ele responder não há sala.
    sfu: Arc<Mutex<Option<String>>>,
    /// A sala aberta, por código ou canal de voz. É o mesmo `Room` que o macOS usa pela ABI.
    room: Arc<Mutex<Option<Arc<Room>>>>,
    /// O que se assiste: a fila de mídia da sala, os decodificadores e o alto-falante.
    watch: Arc<Mutex<Option<Watch>>>,
    microphone: Arc<Mutex<Option<Microphone>>>,
    stage: Arc<Mutex<Stage>>,
    voice: Arc<Mutex<Voice>>,
    /// O canal de voz em que se está: o chat da voz lê e escreve nele.
    voice_channel: Arc<Mutex<Option<String>>>,
    /// O tique que leva o quadro mais novo de cada tela para a janela.
    frames: slint::Timer,
    /// O tique de um segundo da linha de números, e os contadores da última volta dele.
    numbers: slint::Timer,
    counted: Arc<Mutex<std::collections::HashMap<String, media::Counters>>>,
    /// Desde quando se está na sala. O relógio da barra conta a partir daqui.
    since: Arc<Mutex<Option<std::time::Instant>>>,
    /// O tique de um segundo que escreve esse relógio. Vive enquanto a janela viver.
    clock: slint::Timer,
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
    /// O microfone e a saída escolhidos, pelo id do endpoint. Vazio é o padrão do sistema.
    ///
    /// ponytail: a escolha só vive nesta sessão do app. Teto: reabrir o app volta ao padrão.
    /// A saída é guardá-la no `storage`, como o nome.
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
            room: Arc::default(),
            watch: Arc::default(),
            microphone: Arc::default(),
            stage: Arc::default(),
            voice: Arc::default(),
            voice_channel: Arc::default(),
            frames: slint::Timer::default(),
            numbers: slint::Timer::default(),
            counted: Arc::default(),
            since: Arc::default(),
            clock: slint::Timer::default(),
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

        ui.set_tiles(ModelRc::new(VecModel::<TileRow>::default()));

        self.frames.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(16), {
            let (window, watch) = (self.window.clone(), self.watch.clone());

            move || {
                let fresh = match lock(&watch).as_ref() {
                    Some(watch) => watch.fresh(),
                    None => return,
                };

                if fresh.is_empty() {
                    return;
                }

                let Some(app) = window.upgrade() else {
                    return;
                };
                let tiles = app.global::<Ui>().get_tiles();

                for (producer, buffer) in fresh {
                    let found = (0..tiles.row_count()).find_map(|index| {
                        tiles.row_data(index).filter(|row| row.producer == producer).map(|row| (index, row))
                    });

                    if let Some((index, mut row)) = found {
                        row.frame = Image::from_rgb8(buffer);
                        row.has_frame = true;
                        tiles.set_row_data(index, row);
                    }
                }
            }
        });

        self.numbers.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(1), {
            let (window, watch, room, counted) = (self.window.clone(), self.watch.clone(), self.room.clone(), self.counted.clone());

            move || {
                let (Some(app), Some(room)) = (window.upgrade(), lock(&room).clone()) else {
                    return;
                };
                let drawn = match lock(&watch).as_ref() {
                    Some(watch) => watch.drawn(),
                    None => return,
                };
                let tiles = app.global::<Ui>().get_tiles();
                let mut counted = lock(&counted);

                for index in 0..tiles.row_count() {
                    let Some(mut row) = tiles.row_data(index) else {
                        continue;
                    };
                    let producer = row.producer.to_string();
                    let now = room.counters(&producer).unwrap_or_default();
                    let before = counted.insert(producer.clone(), now).unwrap_or_default();
                    let (frames, height) = drawn.get(&producer).copied().unwrap_or_default();
                    let (said, high) = crate::stage::stats_line(
                        frames,
                        height,
                        now.received.saturating_sub(before.received),
                        now.lost.saturating_sub(before.lost),
                    );

                    row.stats = said.into();
                    row.loss_high = high;
                    tiles.set_row_data(index, row);
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
            let bridge = self.clone();

            move || bridge.toggle_mic()
        });

        ui.on_toggle_voice_chat({
            let bridge = self.clone();

            move || bridge.toggle_voice_chat()
        });

        ui.on_send_voice_message({
            let bridge = self.clone();

            move |body| bridge.send_voice_message(&body)
        });

        ui.on_thrown_out({
            let bridge = self.clone();

            move |why| {
                bridge.thrown_out(if why == "replaced" {
                    "Esta conta entrou na sala por outro lugar."
                } else {
                    "Você foi removido desta sala."
                });
            }
        });

        ui.on_toggle_deafen({
            let bridge = self.clone();

            move || bridge.toggle_deafen()
        });

        ui.on_watch_pending({
            let bridge = self.clone();

            move || bridge.with_room(|room| async move { room.watch(None).await })
        });

        ui.on_toggle_self_view({
            let bridge = self.clone();

            move || {
                let wanted = !lock(&bridge.voice).mine.self_view;

                bridge.with_room(move |room| async move { room.set_self_view(wanted).await });
            }
        });

        ui.on_close_tile({
            let bridge = self.clone();

            move |producer| {
                let producer = producer.to_string();

                bridge.with_room(move |room| async move { room.close_watched(&producer).await });
            }
        });

        ui.on_pause_tile({
            let bridge = self.clone();

            move |producer| {
                let producer = producer.to_string();
                let paused = lock(&bridge.stage).tile(&producer).is_some_and(|tile| tile.paused);

                bridge.with_room(move |room| async move { room.pause_watched(&producer, !paused).await });
            }
        });

        ui.on_toggle_heard({
            let bridge = self.clone();

            move |producer| {
                let Some((audio, heard)) = lock(&bridge.stage).toggle_heard(&producer) else {
                    return;
                };

                if let Some(room) = lock(&bridge.room).clone() {
                    room.mute_watched(&audio, !heard);
                }

                paint_stage(&bridge.window, &bridge.stage);
            }
        });

        ui.on_toggle_focus({
            let bridge = self.clone();

            move |producer| {
                lock(&bridge.stage).toggle_focus(&producer);
                paint_stage(&bridge.window, &bridge.stage);
            }
        });

        ui.on_toggle_fullscreen({
            let bridge = self.clone();

            move |producer| {
                lock(&bridge.stage).toggle_full(&producer);
                paint_stage(&bridge.window, &bridge.stage);
            }
        });

        ui.on_set_volume({
            let bridge = self.clone();

            move |producer, level| {
                let audio = lock(&bridge.stage).tile(&producer).and_then(|tile| tile.audio.clone());

                if let (Some(audio), Some(watch)) = (audio, lock(&bridge.watch).as_ref()) {
                    watch.speaker().set_volume(&audio, level);
                }
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
            if lock(&self.voice_channel).as_deref() == Some(channel.id.as_str()) {
                paint(&self.window, |app| app.global::<Ui>().set_stage_open(true));

                return;
            }

            if lock(&self.room).is_some() {
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
        *lock(&self.voice_channel) = Some(channel.id.clone());
        self.connect(Ok(channel.id.clone()), Some(channel.id.clone()), Some(channel.name.clone()));
    }

    /// Abre ou fecha o chat da voz. Abrir relê o canal: sem tempo real aqui, o que aparece é
    /// o que o servidor tem agora.
    fn toggle_voice_chat(self: &Rc<Self>) {
        let Some(app) = self.window.upgrade() else {
            return;
        };
        let ui = app.global::<Ui>();
        let open = !ui.get_voice_chat_open();

        ui.set_voice_chat_open(open);

        if let (true, Some(channel)) = (open, lock(&self.voice_channel).clone()) {
            let (api, window, mine) = (self.api.clone(), self.window.clone(), *lock(&self.me));

            self.spawn(async move { read_voice_chat(&api, &window, &channel, mine).await });
        }
    }

    fn send_voice_message(self: &Rc<Self>, body: &str) {
        let Some(channel) = lock(&self.voice_channel).clone() else {
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

            read_voice_chat(&api, &window, &channel, mine).await;
        });
    }

    /// Sai da voz e continua no servidor. É o fone cortado da barra de baixo.
    fn leave_voice(self: &Rc<Self>) {
        let held = self.close_room();

        *lock(&self.voice_channel) = None;

        paint(&self.window, |app| {
            let ui = app.global::<Ui>();

            ui.set_voice_channel(SharedString::new());
            ui.set_voice_name(SharedString::new());
            ui.set_stage_open(false);
            ui.set_focused_room(false);
            ui.set_voice_chat_open(false);
            ui.set_voice_messages(ModelRc::default());
        });

        self.spawn(async move {
            if let Some(room) = held {
                room.leave().await;
            }
        });
    }

    /// Fecha deste lado o que a sala abriu — o microfone, o que se assiste, o palco — e
    /// devolve a sala para quem chama avisar o servidor da saída.
    fn close_room(self: &Rc<Self>) -> Option<Arc<Room>> {
        let held = lock(&self.room).take();

        drop(lock(&self.microphone).take());
        drop(lock(&self.watch).take());
        lock(&self.stage).clear();
        lock(&self.voice).leave();
        *lock(&self.since) = None;

        paint_stage(&self.window, &self.stage);
        paint_voice(&self.window, &self.voice);
        paint(&self.window, |app| {
            let ui = app.global::<Ui>();

            ui.set_elapsed("0:00:00".into());
            ui.set_ping("-- ms".into());
            ui.set_ping_ms(-1);
            ui.set_reconnecting(false);
        });

        held
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
        let identity = self.identity(&room, voice);
        let in_voice = staying.is_some();
        let (held, watch, stage, voice, started) = (
            self.room.clone(),
            self.watch.clone(),
            self.stage.clone(),
            self.voice.clone(),
            self.since.clone(),
        );
        let microphone = self.microphone.clone();
        let (microphone_device, speaker_device) = lock(&self.chosen).clone();

        // Da escolha do canal até o microfone abrir, o botão não pinta mudo.
        lock(&voice).opening = in_voice;
        paint(&window, |app| app.global::<Ui>().set_entry_busy(true));

        self.spawn(async move {
            let (updates, heard) = std::sync::mpsc::channel();
            let entered = Room::enter(&url, &room, identity, updates).await;

            paint(&window, |app| app.global::<Ui>().set_entry_busy(false));

            let (opened, media) = match entered {
                Ok(entered) => entered,
                Err(failure) => {
                    lock(&voice).opening = false;
                    paint_voice(&window, &voice);

                    let reason = sentence(Failure::from_error(&failure));

                    complain(&window, format!("Não deu para entrar na sala. {reason}"));

                    return;
                }
            };

            *lock(&held) = Some(opened.clone());
            *lock(&started) = Some(std::time::Instant::now());

            {
                let mut voice = lock(&voice);

                voice.inside = in_voice;
                voice.mine = serde_json::from_value(opened.mine()).unwrap_or_default();
                voice.peers = peers_of(&opened.peers());

                // Quem ensurdeceu fora da sala entra surdo: a sala nova nasce ouvindo.
                if voice.deafened {
                    opened.deafen(true);
                }
            }

            lock(&stage).set_tiles(&opened.tiles());

            let speaker = Arc::new(Speaker::start(speaker_device));

            *lock(&watch) = Some(Watch::start(media, speaker, {
                let (voice, window) = (voice.clone(), window.clone());

                move |producer, speaking| {
                    {
                        let mut voice = lock(&voice);

                        if speaking {
                            voice.speaking.insert(producer.to_owned());
                        } else {
                            voice.speaking.remove(producer);
                        }
                    }

                    paint_voice(&window, &voice);
                }
            }));

            listen(heard, window.clone(), stage.clone(), voice.clone());

            let code = room.clone();

            paint(&window, move |app| {
                let ui = app.global::<Ui>();

                ui.set_complaint(SharedString::new());

                match staying {
                    // Canal de voz: o hub continua na tela, e o canal aberto se marca.
                    Some(name) => {
                        ui.set_voice_channel(code.clone().into());
                        ui.set_voice_name(name.into());
                        ui.set_stage_open(true);
                    }
                    None => {
                        ui.set_room_code(code.into());
                        ui.set_screen("room".into());
                    }
                }
            });
            paint_voice(&window, &voice);
            paint_stage(&window, &stage);

            // Entrar na voz abre o microfone, como no Mac e no React: quem entra já é ouvido.
            if in_voice {
                open_microphone(opened, microphone, voice.clone(), window.clone(), microphone_device).await;
            }

            lock(&voice).opening = false;
            paint_voice(&window, &voice);
        });
    }

    /// Liga ou desliga a transmissão da tela. O botão só reflete o que de fato subiu: a
    /// tela vira violeta depois do `room.mine`, não no clique.
    fn toggle_share(self: &Rc<Self>) {
        let sharing = lock(&self.voice).mine.sharing;
        let window = self.window.clone();

        self.with_room(move |room| async move {
            if sharing {
                room.stop_sharing().await;

                return;
            }

            // A receita padrão: a tela principal em 1080p60, com o som do sistema.
            let recipe = core_app::sharing::capture_config(&serde_json::json!({}));

            if let Err(failure) = room.share(recipe).await {
                tracing::warn!(%failure, "a tela não subiu");
                complain(&window, room_failure("share"));
            }
        });
    }

    /// O microfone da barra de baixo. Fora de uma voz ele guarda o mudo para a próxima; na
    /// voz, abre se ainda não abriu, e depois alterna o mudo.
    fn toggle_mic(self: &Rc<Self>) {
        let (inside, mine) = {
            let voice = lock(&self.voice);

            (voice.inside, voice.mine)
        };
        let room = lock(&self.room).clone().filter(|_| inside);

        let Some(room) = room else {
            let mut voice = lock(&self.voice);

            voice.muted_at_rest = !voice.muted_at_rest;
            drop(voice);
            paint_voice(&self.window, &self.voice);

            return;
        };

        if mine.mic {
            self.spawn(async move { room.mute_microphone(!mine.mic_muted).await });

            return;
        }

        let (cell, voice, window) = (self.microphone.clone(), self.voice.clone(), self.window.clone());
        let device = lock(&self.chosen).0.clone();

        lock(&voice).opening = true;
        paint_voice(&window, &voice);

        self.spawn(async move {
            open_microphone(room, cell, voice.clone(), window.clone(), device).await;

            lock(&voice).opening = false;
            paint_voice(&window, &voice);
        });
    }

    /// Ensurdecer cala o que chega. Vale fora da sala também: quem entra surdo continua surdo.
    fn toggle_deafen(self: &Rc<Self>) {
        let deafened = {
            let mut voice = lock(&self.voice);

            voice.deafened = !voice.deafened;
            voice.deafened
        };

        if let Some(room) = lock(&self.room).clone() {
            room.deafen(deafened);
        }

        paint_voice(&self.window, &self.voice);
    }

    /// Um clique que vira pedido à sala aberta, fora da thread da janela.
    fn with_room<F, Work>(self: &Rc<Self>, work: F)
    where
        F: FnOnce(Arc<Room>) -> Work,
        Work: std::future::Future<Output = ()> + Send + 'static,
    {
        if let Some(room) = lock(&self.room).clone() {
            self.spawn(work(room));
        }
    }

    /// Tirado da sala pelo servidor: sai do que estiver aberto e diz por quê.
    fn thrown_out(self: &Rc<Self>, message: &'static str) {
        if lock(&self.voice).inside {
            self.leave_voice();
            complain(&self.window, message);
        } else {
            self.leave_room_saying(message);
        }
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
        self.leave_room_saying("");
    }

    fn leave_room_saying(self: &Rc<Self>, message: &'static str) {
        self.core.leave_room();

        let (window, landing) = (self.window.clone(), self.core.home());
        let held = self.close_room();

        self.spawn(async move {
            if let Some(room) = held {
                room.leave().await;
            }

            paint(&window, move |app| {
                let ui = app.global::<Ui>();

                ui.set_room_code(SharedString::new());
                ui.set_complaint(message.into());
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
            let rows = message_rows(&messages, me);

            paint(window, move |app| app.global::<Ui>().set_messages(model(rows)));
        }
        Err(failure) => complain(window, said(&failure)),
    }
}

async fn read_voice_chat(api: &Arc<Api>, window: &Weak<AppWindow>, channel: &str, me: Option<i64>) {
    match api.messages(channel).await {
        Ok(messages) => {
            let rows = message_rows(&messages, me);

            paint(window, move |app| app.global::<Ui>().set_voice_messages(model(rows)));
        }
        Err(failure) => complain(window, said(&failure)),
    }
}

fn message_rows(messages: &[core_app::models::Message], me: Option<i64>) -> Vec<MessageRow> {
    #[allow(clippy::cast_possible_truncation)]
    messages
        .iter()
        .map(|message| MessageRow {
            id: message.id as i32,
            initial: initial(&message.user.name),
            author: message.user.name.clone().into(),
            body: message.body.clone().into(),
            at: message.created_at.get(11..16).unwrap_or_default().into(),
            mine: Some(message.user.id) == me,
        })
        .collect()
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

/// Abre o microfone na sala e a captura do Windows que o alimenta. Quem estava mudo fora
/// da sala entra mudo, como no Mac.
async fn open_microphone(
    room: Arc<Room>,
    cell: Arc<Mutex<Option<Microphone>>>,
    voice: Arc<Mutex<Voice>>,
    window: Weak<AppWindow>,
    device: Option<String>,
) {
    let (can_speak, muted_at_rest) = {
        let voice = lock(&voice);

        (voice.mine.can_speak, voice.muted_at_rest)
    };

    if !can_speak {
        return;
    }

    if let Err(failure) = room.open_microphone().await {
        tracing::warn!(%failure, "a sala não abriu o microfone");
        complain(&window, room_failure("mic"));

        return;
    }

    let speaking = room.clone();
    let started = tokio::task::block_in_place(|| Microphone::start(device, move |samples| speaking.speak(samples)));

    match started {
        Ok(microphone) => {
            *lock(&cell) = Some(microphone);

            if muted_at_rest {
                room.mute_microphone(true).await;
            }
        }
        Err(failure) => {
            tracing::warn!(failure = %format!("{failure:#}"), "o microfone do Windows não abriu");
            room.close_microphone().await;
            complain(&window, room_failure("mic"));
        }
    }
}

/// Os avisos da sala, numa thread só deles: cada um muda o estado guardado e repinta o que
/// mudou. A fila fecha quando a sala acaba, e a thread acaba junto.
fn listen(heard: std::sync::mpsc::Receiver<String>, window: Weak<AppWindow>, stage: Arc<Mutex<Stage>>, voice: Arc<Mutex<Voice>>) {
    let spawned = std::thread::Builder::new().name("unkvoid-sala".into()).spawn(move || {
        for said in heard {
            let Ok(update) = serde_json::from_str::<serde_json::Value>(&said) else {
                continue;
            };
            let data = &update["data"];

            match update["event"].as_str().unwrap_or_default() {
                "room.peers" => {
                    lock(&voice).peers = peers_of(data);
                    paint_voice(&window, &voice);
                }
                "room.mine" => {
                    lock(&voice).mine = serde_json::from_value(data.clone()).unwrap_or_default();
                    paint_voice(&window, &voice);
                }
                "room.tiles" => {
                    lock(&stage).set_tiles(data);
                    paint_stage(&window, &stage);
                }
                "room.watchers" => {
                    lock(&stage).set_watchers(data);
                    paint_stage(&window, &stage);
                }
                "room.level" => {
                    #[allow(clippy::cast_possible_truncation)]
                    let level = data["level"].as_f64().unwrap_or(0.0) as f32;
                    let changed = {
                        let mut voice = lock(&voice);
                        let before = voice.speaking_myself();

                        voice.level = level;
                        before != voice.speaking_myself()
                    };

                    if changed {
                        paint_voice(&window, &voice);
                    }
                }
                "room.ping" => {
                    if let Some(milliseconds) = data["ms"].as_u64() {
                        let said = format!("{milliseconds} ms");
                        let measured = i32::try_from(milliseconds).unwrap_or(i32::MAX);

                        paint(&window, move |app| {
                            let ui = app.global::<Ui>();

                            ui.set_ping(said.into());
                            ui.set_ping_ms(measured);
                        });
                    }
                }
                "room.session" => match data["state"].as_str().unwrap_or_default() {
                    "lost" => paint(&window, |app| app.global::<Ui>().set_reconnecting(true)),
                    "rejoined" => {
                        paint(&window, |app| app.global::<Ui>().set_reconnecting(false));
                        complain(&window, "");
                    }
                    "gone" => {
                        paint(&window, |app| app.global::<Ui>().set_reconnecting(false));
                        complain(&window, "A sala não voltou. Entre de novo quando a internet estabilizar.");
                    }
                    "replaced" => paint(&window, |app| app.global::<Ui>().invoke_thrown_out("replaced".into())),
                    "kicked" => paint(&window, |app| app.global::<Ui>().invoke_thrown_out("kicked".into())),
                    _ => {}
                },
                "room.failed" => complain(&window, room_failure(data["what"].as_str().unwrap_or_default())),
                _ => {}
            }
        }
    });

    if let Err(failure) = spawned {
        tracing::warn!(%failure, "a thread dos avisos da sala não subiu");
    }
}

/// Pinta quem está na sala e a barra de baixo a partir do estado da voz.
fn paint_voice(window: &Weak<AppWindow>, voice: &Arc<Mutex<Voice>>) {
    let (rows, mic_off, speaking, deafened, inside, mine) = {
        let voice = lock(voice);

        (peer_rows(&voice), voice.mic_shown_off(), voice.speaking_myself(), voice.deafened, voice.inside, voice.mine)
    };

    paint(window, move |app| {
        let ui = app.global::<Ui>();

        ui.set_peers(model(rows));
        ui.set_mic_on(!mic_off);
        ui.set_speaking(speaking);
        ui.set_deafened(deafened);
        // Fora da sala o microfone é o mudo guardado, e esse sempre se clica.
        ui.set_can_speak(!inside || mine.can_speak);
        ui.set_can_share(mine.can_share);
        ui.set_sharing(mine.sharing);
        ui.set_self_view(mine.self_view);
    });
}

/// Pinta o palco. Com os mesmos cartões na mesma ordem, cada linha é trocada no lugar: o
/// cartão não é recriado, e não perde o hover nem o painel do volume aberto.
fn paint_stage(window: &Weak<AppWindow>, stage: &Arc<Mutex<Stage>>) {
    let (placed, (columns, lines), focusing, full, pending) = {
        let stage = lock(stage);

        (stage.placed(), stage.grid(), stage.focusing(), stage.full_screen(), stage.pending())
    };

    paint(window, move |app| {
        let ui = app.global::<Ui>();
        let current = ui.get_tiles();
        let before: Vec<TileRow> = (0..current.row_count()).filter_map(|index| current.row_data(index)).collect();
        let rows: Vec<TileRow> = placed
            .into_iter()
            .map(|placed| {
                let frame = before
                    .iter()
                    .find(|row| row.producer == placed.tile.producer_id.as_str() && row.has_frame)
                    .map(|row| row.frame.clone());

                TileRow {
                    producer: placed.tile.producer_id.as_str().into(),
                    label: placed.tile.label.as_str().into(),
                    initial: initial(&placed.tile.label),
                    mine: placed.tile.mine,
                    camera: placed.tile.camera,
                    paused: placed.tile.paused,
                    audio: placed.tile.audio.is_some(),
                    heard: placed.heard,
                    watchers: i32::try_from(placed.watchers.len()).unwrap_or(i32::MAX),
                    watcher_names: placed.watchers.join(", ").into(),
                    stats: before
                        .iter()
                        .find(|row| row.producer == placed.tile.producer_id.as_str())
                        .map(|row| row.stats.clone())
                        .unwrap_or_default(),
                    loss_high: before
                        .iter()
                        .find(|row| row.producer == placed.tile.producer_id.as_str())
                        .is_some_and(|row| row.loss_high),
                    has_frame: frame.is_some(),
                    frame: frame.unwrap_or_default(),
                    column: i32::try_from(placed.column).unwrap_or_default(),
                    line: i32::try_from(placed.line).unwrap_or_default(),
                    rank: i32::try_from(placed.rank).unwrap_or_default(),
                    focused: placed.focused,
                    full: placed.full,
                }
            })
            .collect();

        let same = before.len() == rows.len() && before.iter().zip(&rows).all(|(old, new)| old.producer == new.producer);

        if same {
            for (index, row) in rows.into_iter().enumerate() {
                current.set_row_data(index, row);
            }
        } else {
            ui.set_tiles(model(rows));
        }

        ui.set_grid_columns(i32::try_from(columns).unwrap_or(1));
        ui.set_grid_lines(i32::try_from(lines).unwrap_or(1));
        ui.set_focusing(focusing);
        ui.set_full_screen(full);
        ui.set_pending_tiles(i32::try_from(pending).unwrap_or_default());
    });
}

fn peer_rows(voice: &Voice) -> Vec<PeerRow> {
    voice
        .peers
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
            speaking: voice.is_speaking(peer),
            muted: if peer.self_peer {
                voice.mic_shown_off()
            } else {
                !peer.producers.iter().any(|producer| producer.source == "mic" && !producer.paused)
            },
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
