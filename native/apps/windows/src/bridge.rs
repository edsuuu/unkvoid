//! A ponte entre o clique e o núcleo.
//!
//! O Slint desenha numa thread só e não fala `async`; o núcleo é `async` e não pode
//! desenhar. Aqui o trabalho vai para o runtime do Tokio e o resultado volta pela fila do
//! laço de eventos da janela — sem isso, um `GET` lento congelaria a tela inteira.
//!
//! Nada aqui decide: tudo que é decisão (o código vale? onde se cai ao sair? quem está na
//! sala?) é chamada ao `core_app`.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use core_app::api::{Api, HttpError};
use core_app::app::EntryRefusal;
use core_app::chimes::Chime;
use core_app::models::{
    Channel, ChannelKind, Conversation, DirectMessage, Friendship, FriendshipStatus, Person, RoomIdentity,
    ServerSummary, ServerTree, User, VoicePerson,
};
use core_app::permissions::{self, MemberActions};
use core_app::sharing::InputMode;
use core_app::realtime::{self, Realtime, Reading};
use core_app::reconnect::Backoff;
use core_app::resume::{self, Resume, VoiceSeat};
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
    ActionsRow, AppWindow, AuditRow, BanRow, ChannelRow, ConversationRow, DeviceRow, FriendRow, MemberGroupRow,
    MemberRow, MessageRow, PeerRow, PermissionRow, RoleRow, ServerRow, TileRow, ToastRow, Ui, VoicePersonRow,
};

/// Os avisos na tela, e o número do próximo.
type Toasts = Arc<Mutex<(i32, Vec<ToastRow>)>>;

/// Um aviso local na fila do tempo real: a conta entrou no ar, ou a árvore do servidor
/// chegou — hora de acertar quais canais se segue.
const FOLLOW: &str = r#"{"event":"live.follow"}"#;

const DEFAULT_SERVER: &str = "https://unkvoid.com";

/// Quanto se espera a sala se despedir do servidor antes de entregar o app ao instalador. Sem
/// a despedida, quem assiste fica vendo a tela parada até o servidor desistir de esperar.
const LEAVING: std::time::Duration = std::time::Duration::from_secs(3);

/// ponytail: a câmera do Windows ainda não existe — o `capture` não abre webcam aqui, e o
/// `Room` só aceita câmera no macOS. Teto: quem está no Windows vê a câmera dos outros mas
/// não liga a dele. A saída é a captura por Media Foundation empurrando `room.show`.
const NO_CAPTURE: &str = "A câmera ainda não está ligada nesta versão do app.";

/// As falhas que a sala anuncia, na frase do Mac.
fn room_failure(what: &str) -> &'static str {
    match what {
        "watch" => "Não deu para assistir a uma das transmissões.",
        "mic" => "Não deu para abrir o microfone.",
        "shareClosed" => "A janela que você compartilhava foi fechada, e a transmissão parou.",
        "serverMuted" => "Um moderador silenciou o seu microfone.",
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
    /// A sala aberta de verdade, pelo código ou pelo id do canal: é o que diz se entrar é
    /// voltar a ela ou trocar de sala.
    entered: Arc<Mutex<Option<String>>>,
    /// A entrada que ainda espera o servidor.
    entering: Arc<Mutex<Entering>>,
    /// Os contadores da última volta da linha de números.
    counted: Arc<Mutex<std::collections::HashMap<String, media::Counters>>>,
    /// Desde quando se está na sala. O relógio da barra conta a partir daqui.
    since: Arc<Mutex<Option<std::time::Instant>>>,
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
    /// O tempo real do chat e da presença, aberto enquanto há conta.
    live: Arc<Mutex<Option<Arc<Realtime>>>>,
    /// Os canais que o tempo real segue agora, fora o `user.{id}` da conta.
    followed: Arc<Mutex<BTreeSet<String>>>,
    /// Quem está online no servidor aberto.
    online: Arc<Mutex<HashSet<i64>>>,
    /// O que o "tem certeza?" vai fazer quando a pessoa confirmar: a ação e o id.
    pending: Arc<Mutex<Option<(String, i64)>>>,
    toasts: Toasts,
    /// A versão nova já baixada e conferida, com o número dela: o que o botão verde instala.
    installer: Arc<Mutex<Option<(PathBuf, String)>>>,
    /// Onde se estava antes da atualização, esperando a abertura terminar para voltar lá.
    resumed: Arc<Mutex<Option<Resume>>>,
    /// A tela que volta ao ar assim que a sala retomada abrir.
    resumed_share: Arc<Mutex<Option<capture::CaptureConfig>>>,
    /// O servidor do canal de voz em que se está: é ele que a tela abre ao voltar.
    voice_server: Arc<Mutex<Option<i64>>>,
}

/// A preferência de voz guardada (`unkvoid:voice`), a mesma chave do app de hoje.
const VOICE_KEY: &str = "unkvoid:voice";

/// Os bits de permissão na ordem da tela de cargos, com o nome em português.
const PERMISSIONS: &[(i64, &str)] = &[
    (permissions::ADMINISTRATOR, "Administrador"),
    (permissions::MANAGE_SERVER, "Gerenciar servidor"),
    (permissions::MANAGE_ROLES, "Gerenciar cargos"),
    (permissions::MANAGE_CHANNELS, "Gerenciar canais"),
    (permissions::KICK_MEMBERS, "Expulsar membros"),
    (permissions::BAN_MEMBERS, "Banir membros"),
    (permissions::CREATE_INVITE, "Criar convite"),
    (permissions::VIEW_AUDIT_LOG, "Ver registro de auditoria"),
    (permissions::VIEW_CHANNEL, "Ver canais"),
    (permissions::SEND_MESSAGES, "Enviar mensagens"),
    (permissions::MANAGE_MESSAGES, "Gerenciar mensagens"),
    (permissions::CONNECT, "Conectar à voz"),
    (permissions::SPEAK, "Falar"),
    (permissions::STREAM, "Compartilhar tela"),
    (permissions::VIDEO, "Câmera"),
    (permissions::MUTE_MEMBERS, "Mutar membros"),
    (permissions::DEAFEN_MEMBERS, "Ensurdecer membros"),
    (permissions::MOVE_MEMBERS, "Mover e desconectar membros"),
];

impl Bridge {
    pub fn new(window: Weak<AppWindow>) -> anyhow::Result<Rc<Self>> {
        let storage = Storage::open()?;
        let (core, api) = (Arc::new(App::new(storage)), Arc::new(Api::new(&server())?));

        core.keep_session(&api, {
            let window = window.clone();

            move || paint(&window, |app| app.global::<Ui>().invoke_session_ended())
        });

        Ok(Rc::new(Self {
            runtime: Runtime::new()?,
            core,
            api,
            window,
            sfu: Arc::default(),
            room: Arc::default(),
            watch: Arc::default(),
            microphone: Arc::default(),
            stage: Arc::default(),
            voice: Arc::default(),
            voice_channel: Arc::default(),
            entered: Arc::default(),
            entering: Arc::default(),
            counted: Arc::default(),
            since: Arc::default(),
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
            live: Arc::default(),
            followed: Arc::default(),
            online: Arc::default(),
            pending: Arc::default(),
            toasts: Arc::default(),
            installer: Arc::default(),
            resumed: Arc::default(),
            resumed_share: Arc::default(),
            voice_server: Arc::default(),
        }))
    }

    /// Cada clique da tela, ligado ao que ele faz. É o único lugar que conhece os dois
    /// lados: daqui para baixo só há núcleo, e daqui para cima só há desenho.
    pub fn wire(self: &Rc<Self>, window: &AppWindow) {
        let ui = window.global::<Ui>();

        ui.set_saved_name(self.core.state().name.into());

        every(std::time::Duration::from_secs(1), {
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

        every(std::time::Duration::from_secs(1), {
            let (window, watch, room, counted) = (self.window.clone(), self.watch.clone(), self.room.clone(), self.counted.clone());
            let (runtime, away_for) = (self.runtime.handle().clone(), Arc::new(std::sync::atomic::AtomicU32::new(0)));

            move || {
                let (Some(app), Some(room)) = (window.upgrade(), lock(&room).clone()) else {
                    return;
                };

                // Dois segundos fora antes de pausar: um alt-tab rápido não pode custar um
                // quadro-chave na volta.
                let away = app.window().is_minimized() || !app.window().is_visible();
                let seconds = if away {
                    away_for.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
                } else {
                    away_for.store(0, std::sync::atomic::Ordering::Relaxed);

                    0
                };

                runtime.spawn({
                    let room = room.clone();

                    async move { room.set_away(seconds >= 2).await }
                });

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

            move |name, code| bridge.enter(bridge.core.create_room(&bridge.room_name(&name), &code), None)
        });

        ui.on_join_room({
            let bridge = self.clone();

            move |name, code| bridge.enter(bridge.core.join_room(&bridge.room_name(&name), &code), None)
        });

        ui.on_sign_in({
            let bridge = self.clone();

            move |email, password, register| bridge.sign_in(&email, &password, register)
        });

        ui.on_google_sign_in({
            let bridge = self.clone();

            move || bridge.google_sign_in()
        });

        ui.on_rename({
            let bridge = self.clone();

            move |name| bridge.rename(&name)
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

            move |code| bridge.enter(bridge.core.join_room(&bridge.room_name(""), &code), None)
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

        ui.on_dismiss_toast({
            let (window, toasts) = (self.window.clone(), self.toasts.clone());

            move |id| dismiss(&window, &toasts, id)
        });

        ui.on_open_share({
            let bridge = self.clone();

            move || bridge.open_share()
        });

        ui.on_pick_share_tab({
            let bridge = self.clone();

            move |tab| bridge.pick_share_tab(&tab)
        });

        ui.on_confirm_share({
            let bridge = self.clone();

            move || bridge.confirm_share()
        });

        ui.on_session_ended({
            let bridge = self.clone();

            move || bridge.end_session()
        });

        ui.on_heard_live({
            let bridge = self.clone();

            move |line| bridge.heard_live(&line)
        });

        ui.on_visit_home({
            let bridge = self.clone();

            move || {
                // Com conta a casinha é a Home do hub; sem conta, a entrada — que é a Home de
                // quem não tem servidor. Nos dois a sala segue no ar.
                if bridge.api.signed_in() {
                    paint(&bridge.window, |app| app.global::<Ui>().set_screen("hub".into()));
                    bridge.show_hub_home();
                } else {
                    paint(&bridge.window, |app| app.global::<Ui>().set_screen("entry".into()));
                    paint_recent(&bridge.core, &bridge.window);
                }
            }
        });

        ui.on_back_to_room({
            let bridge = self.clone();

            move || bridge.back_to_room()
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
                let (toggled, volume) = {
                    let mut stage = lock(&bridge.stage);

                    (stage.toggle_heard(&producer), stage.volume(&producer))
                };
                let Some((audio, heard)) = toggled else {
                    return;
                };

                if let Some(room) = lock(&bridge.room).clone() {
                    room.mute_watched(&audio, !heard);
                }

                if let Some(watch) = lock(&bridge.watch).as_ref() {
                    watch.speaker().set_volume(&audio, f32::from(volume) / 100.0);
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
                let focused = {
                    let mut stage = lock(&bridge.stage);

                    stage.toggle_full(&producer);
                    stage.full_producer()
                };

                paint_stage(&bridge.window, &bridge.stage);
                bridge.with_room(move |room| async move { room.set_focus(focused).await });
            }
        });

        ui.on_set_volume({
            let bridge = self.clone();

            move |producer, level| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let volume = level.round().clamp(0.0, 100.0) as u8;
                let Some((audio, heard)) = lock(&bridge.stage).set_volume(&producer, volume) else {
                    return;
                };

                if let (Some(heard), Some(room)) = (heard, lock(&bridge.room).clone()) {
                    room.mute_watched(&audio, !heard);
                }

                if let Some(watch) = lock(&bridge.watch).as_ref() {
                    watch.speaker().set_volume(&audio, f32::from(volume) / 100.0);
                }

                paint_stage(&bridge.window, &bridge.stage);
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

        ui.on_install_update({
            let bridge = self.clone();

            move || bridge.install_update()
        });

        ui.on_resume_session({
            let bridge = self.clone();

            move || bridge.resume_session()
        });

        ui.on_open_modal({
            let bridge = self.clone();

            move |kind, index| bridge.open_modal(&kind, index)
        });

        ui.on_ask({
            let bridge = self.clone();

            move |action, id| bridge.ask(&action, id)
        });

        ui.on_confirm({
            let bridge = self.clone();

            move || bridge.confirm()
        });

        ui.on_rename_server({
            let bridge = self.clone();

            move |name| bridge.manage("updateServer", serde_json::json!({ "name": name.trim() }), serde_json::json!({}), None)
        });

        ui.on_regenerate_invite({
            let bridge = self.clone();

            move || bridge.regenerate_invite()
        });

        ui.on_update_channel({
            let bridge = self.clone();

            move |index, name, topic, limit| bridge.update_channel(index, &name, &topic, limit)
        });

        ui.on_open_server_settings({
            let bridge = self.clone();

            move |tab| bridge.open_server_settings(&tab)
        });

        ui.on_select_role({
            let bridge = self.clone();

            move |index| bridge.select_role(index)
        });

        ui.on_create_role({
            let bridge = self.clone();

            move |name| {
                let name = name.trim().to_owned();

                if !name.is_empty() {
                    bridge.manage("createRole", serde_json::json!({ "name": name, "permissions": 0 }), serde_json::json!({}), None);
                }
            }
        });

        ui.on_delete_role({
            let bridge = self.clone();

            move |role| bridge.manage("deleteRole", serde_json::Value::Null, serde_json::json!({ "role": role }), None)
        });

        ui.on_toggle_permission({
            let bridge = self.clone();

            move |bit| bridge.toggle_permission(bit)
        });

        ui.on_unban({
            let bridge = self.clone();

            move |user| bridge.manage("unban", serde_json::Value::Null, serde_json::json!({ "user": user }), None)
        });

        ui.on_server_mute({
            let bridge = self.clone();

            move |user, muted| {
                bridge.manage("updateMember", serde_json::json!({ "server_mute": muted }), serde_json::json!({ "user": user }), None);
            }
        });

        ui.on_disconnect_voice({
            let bridge = self.clone();

            move |user| bridge.move_voice(user, None)
        });

        ui.on_move_voice({
            let bridge = self.clone();

            move |user, index| bridge.move_voice(user, Some(index))
        });

        ui.on_message_user({
            let bridge = self.clone();

            move |user, name| {
                bridge.show_hub_home();
                bridge.talk_with(Person { id: i64::from(user), name: name.to_string(), avatar_url: None });
            }
        });

        ui.on_mute_person({
            let bridge = self.clone();

            move |user, muted| bridge.mute_person(i64::from(user), muted)
        });

        ui.on_set_person_volume({
            let bridge = self.clone();

            move |user, level| {
                let microphone = lock(&bridge.voice).microphone_of(i64::from(user));

                // ponytail: o `Speaker` de hoje corta em 100%; os 200% do Discord chegam
                // com o `Playout` do núcleo, que ganha o volume por pessoa.
                if let (Some(microphone), Some(watch)) = (microphone, lock(&bridge.watch).as_ref()) {
                    watch.speaker().set_volume(&microphone, level.min(1.0));
                }
            }
        });

        ui.on_set_input_mode({
            let bridge = self.clone();

            move |mode, sensitivity| bridge.set_input_mode(&mode, i64::from(sensitivity))
        });

        ui.on_set_voice_flag({
            let bridge = self.clone();

            move |key, on| bridge.set_voice_flag(&key, on)
        });

        ui.on_talk({
            let bridge = self.clone();

            move |talking| {
                if let Some(room) = lock(&bridge.room).clone() {
                    room.talk(talking);
                }
            }
        });

        self.paint_voice_preferences();
    }

    /// A abertura: o servidor responde? Onde fica o SFU? O token guardado ainda vale? Aberto,
    /// volta para onde se estava se foi a atualização que fechou o app, e passa a procurar a
    /// próxima versão.
    pub fn start(self: &Rc<Self>) {
        let (core, api, window) = (self.core.clone(), self.api.clone(), self.window.clone());
        let (sfu, live) = (self.sfu.clone(), self.live.clone());
        let landing = self.landing();
        let installer = self.installer.clone();
        let returning = {
            let resume = resume::take(&self.core);
            let returning = resume.is_some();

            *lock(&self.resumed) = resume;

            returning
        };

        #[cfg(target_os = "windows")]
        self.spawn({
            let api = self.api.clone();

            async move { report_errors(&api).await }
        });

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

            if updating(&api, &window).await {
                return;
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
            let account = user.as_ref().map(|user| user.id);

            landed(&core, &api, &window, &landing, user).await;

            // O tempo real abre com conta ou sem: sem conta ele só ouve o canal das versões.
            go_live(&api, &sfu, &live, &window, account).await;

            if returning {
                paint(&window, |app| app.global::<Ui>().invoke_resume_session());
            }

            // Anunciada pelo servidor e ainda não instalada (a abertura não conseguiu baixar):
            // o botão verde volta sem ninguém perguntar ao site.
            if core_app::update::announced(&core).is_some() {
                prepare_update(&api, &window, &installer).await;
            }
        });
    }

    fn sign_in(self: &Rc<Self>, email: &str, password: &str, register: bool) {
        let (core, api, window) = (self.core.clone(), self.api.clone(), self.window.clone());
        let (email, password) = (email.to_owned(), password.to_owned());
        let (sfu, live) = (self.sfu.clone(), self.live.clone());
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

                    arrive(&core, &api, &window, &landing, &sfu, &live, user).await;
                }
                Err(failure) => refuse_login(&window, &failure),
            }
        });
    }

    /// Entrar com o Google: o navegador abre na conta do Google, e o site devolve o token a uma
    /// porta local deste app — o `core_app::google`, o mesmo caminho do Mac. A pessoa tem até
    /// cinco minutos para escolher a conta; o e-mail e senha seguem livres enquanto isso.
    fn google_sign_in(self: &Rc<Self>) {
        let (core, api, window) = (self.core.clone(), self.api.clone(), self.window.clone());
        let (sfu, live) = (self.sfu.clone(), self.live.clone());
        let landing = self.landing();

        self.spawn(async move {
            let login = match core_app::google::GoogleLogin::start(&server()).await {
                Ok(login) => login,
                Err(failure) => {
                    tracing::warn!(%failure, "google: a porta do retorno não abriu");
                    complain(&window, "Não deu para abrir o login do Google. Tente de novo.");

                    return;
                }
            };

            open_in_browser(&login.url);

            let Ok((token, refresh)) = login.wait().await else {
                complain(&window, "O login com o Google não voltou do navegador. Tente de novo.");

                return;
            };

            // A aba tenta se fechar sozinha, mas o navegador pode recusar: o app vem para a
            // frente de qualquer jeito, e a pessoa não fica olhando para o navegador.
            #[cfg(target_os = "windows")]
            let _ = slint::invoke_from_event_loop(crate::clips::show_window);

            core.set_token(Some(&token));
            api.adopt(&token, refresh.as_deref());

            let user = api.me().await.ok();

            arrive(&core, &api, &window, &landing, &sfu, &live, user).await;
        });
    }

    /// Troca o apelido da conta. O erro de validação do Laravel vai para baixo do campo; o
    /// resto, para a mesma linha.
    fn rename(self: &Rc<Self>, name: &str) {
        let name = name.trim().to_owned();
        let (api, window) = (self.api.clone(), self.window.clone());

        paint(&window, |app| app.global::<Ui>().set_nickname_busy(true));

        self.spawn(async move {
            let renamed = api.rename(&name).await;

            paint(&window, move |app| {
                let ui = app.global::<Ui>();

                ui.set_nickname_busy(false);

                match renamed {
                    Ok(user) => {
                        ui.set_nickname_error(SharedString::new());
                        ui.set_nickname_pending(!user.nickname_confirmed);
                        ui.set_user_initial(initial(&user.name));
                        ui.set_user_name(user.name.into());
                    }
                    Err(failure) => ui.set_nickname_error(said(&failure).into()),
                }
            });
        });
    }

    /// Sair nunca depende da rede: a tela volta ao login na hora, e os tokens caem no
    /// servidor em segundo plano.
    fn sign_out(self: &Rc<Self>) {
        self.core.set_token(None);
        self.spawn(self.api.sign_out());

        if let Some(live) = lock(&self.live).take() {
            live.close();
        }

        lock(&self.followed).clear();
        lock(&self.online).clear();

        // Sem conta o tempo real continua, só para o aviso de versão nova.
        let (api, sfu, live, window) = (self.api.clone(), self.sfu.clone(), self.live.clone(), self.window.clone());

        self.spawn(async move { go_live(&api, &sfu, &live, &window, None).await });

        let window = self.window.clone();
        let landing = self.core.home();

        lock(&self.servers).clear();

        paint(&window, move |app| {
            let ui = app.global::<Ui>();

            ui.set_signed_in(false);
            ui.set_nickname_pending(false);
            ui.set_user_name(SharedString::new());
            ui.set_user_initial(SharedString::new());
            ui.set_servers(ModelRc::default());
            ui.set_text_channels(ModelRc::default());
            ui.set_voice_channels(ModelRc::default());
            ui.set_member_groups(ModelRc::default());
            ui.set_messages(ModelRc::default());
            ui.set_complaint(SharedString::new());
            ui.set_screen(named(landing).into());
        });
    }

    /// A sessão acabou sozinha — o token não renovou, ou a conta saiu por outro lugar: a
    /// janela volta ao login com o motivo, como se a pessoa tivesse saído.
    fn end_session(self: &Rc<Self>) {
        self.sign_out();
        paint(&self.window, |app| app.global::<Ui>().set_login_error("Sua sessão terminou. Entre de novo.".into()));
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

        // Na Home não há servidor aberto: o tempo real larga os canais dele, como o React.
        *lock(&self.opened) = None;
        *lock(&self.reading) = None;
        lock(&self.online).clear();
        self.follow();

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

    /// Um evento do tempo real ou da sala, já na thread da janela. O que ele significa quem
    /// diz é o núcleo (`realtime::read`); aqui só se relê, toca e avisa.
    fn heard_live(self: &Rc<Self>, line: &str) {
        let Ok(update) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };
        let (event, channel, data) = (update["event"].as_str().unwrap_or_default(), update["channel"].as_str(), &update["data"]);

        match event {
            "live.follow" => return self.follow(),
            // Vale sem conta: é o canal público, e o `me` abaixo não importa para ele.
            "ReleasePublished" => return self.release_announced(data),
            "room.chime" => {
                if let Ok(chime) = serde_json::from_value::<Chime>(data["chime"].clone()) {
                    self.chime(chime);
                }

                return;
            }
            "room.notice" => return self.notify(data["text"].as_str().unwrap_or_default(), false),
            "room.moved" => return self.moved(data),
            _ => {}
        }

        // Presença que chega atrasada do servidor anterior não vale para o aberto agora.
        if event.starts_with("presence.") && channel != lock(&self.opened).map(|server| format!("server.{server}")).as_deref() {
            return;
        }

        let Some(me) = *lock(&self.me) else {
            return;
        };
        let talking = lock(&self.talking).as_ref().map(|person| person.id);

        self.act(realtime::read(event, channel, data, me, talking));
    }

    /// O servidor avisou pelo tempo real que saiu versão nova: fica anotada na configuração
    /// (sai de lá quando estiver instalada) e desce calada até o botão verde aparecer.
    fn release_announced(self: &Rc<Self>, data: &serde_json::Value) {
        let Some(version) = core_app::update::newer_in(data) else {
            return;
        };

        if lock(&self.installer).as_ref().is_some_and(|(_, ready)| *ready == version) {
            return;
        }

        core_app::update::announce(&self.core, &version);

        let (api, window, ready) = (self.api.clone(), self.window.clone(), self.installer.clone());

        self.spawn(async move { prepare_update(&api, &window, &ready).await });
    }

    fn act(self: &Rc<Self>, reading: Reading) {
        if let Some(chime) = reading.chime {
            self.chime(chime);
        }

        if let Some(notice) = &reading.notice {
            self.notify(&notice.text, notice.error);
        }

        if let Some(channel) = &reading.messages_of {
            self.messages_changed(channel, reading.unread);
        }

        if reading.direct || reading.catch_up {
            self.direct_changed(reading.direct_with);
        }

        if reading.friends || reading.catch_up {
            self.load_friends();
        }

        if reading.tree || reading.catch_up {
            self.refresh_tree();
        }

        if let Some(server) = reading.removed_from {
            self.removed_from(server);
        }

        if let Some(presence) = &reading.presence {
            presence.apply(&mut lock(&self.online));
            self.paint_members();
        }

        if reading.catch_up {
            for channel in [lock(&self.reading).clone(), lock(&self.voice_channel).clone()].into_iter().flatten() {
                self.messages_changed(&channel, false);
            }
        }
    }

    /// As mensagens de um canal mudaram: relê se ele está na tela; no chat da voz fechado,
    /// conta a não lida.
    fn messages_changed(self: &Rc<Self>, channel: &str, unread: bool) {
        let (api, window, mine) = (self.api.clone(), self.window.clone(), *lock(&self.me));
        let owned = channel.to_owned();

        if lock(&self.reading).as_deref() == Some(channel) {
            let tree = self.tree();

            self.spawn(async move { read_channel(&api, &window, &owned, mine, tree).await });

            return;
        }

        if lock(&self.voice_channel).as_deref() != Some(channel) {
            return;
        }

        let Some(app) = self.window.upgrade() else {
            return;
        };
        let ui = app.global::<Ui>();

        if ui.get_voice_chat_open() {
            let tree = self.tree();

            self.spawn(async move { read_voice_chat(&api, &window, &owned, mine, tree).await });
        } else if unread {
            ui.set_voice_chat_unread(ui.get_voice_chat_unread() + 1);
        }
    }

    /// Uma conversa direta mudou: relê a aberta, se for ela, e a lista com as não lidas.
    fn direct_changed(self: &Rc<Self>, with: Option<i64>) {
        let (api, window, held) = (self.api.clone(), self.window.clone(), self.conversations.clone());
        let open = lock(&self.talking).clone().filter(|person| with.is_none_or(|with| with == person.id));

        self.spawn(async move {
            if let Some(person) = open
                && let Ok(messages) = api.direct_messages(person.id).await
            {
                let _ = api.read_conversation(person.id).await;

                refresh_direct(&window, messages);
            }

            if let Ok(conversations) = api.conversations().await {
                show_conversations(&window, &held, conversations);
            }
        });
    }

    /// A árvore do servidor aberto mudou: relê sem perder o canal que se está lendo.
    fn refresh_tree(self: &Rc<Self>) {
        let Some(server) = *lock(&self.opened) else {
            return;
        };
        let (api, window, opening) = (self.api.clone(), self.window.clone(), self.opening());

        self.spawn(async move { show_tree(&api, &window, &opening, server, true).await });
    }

    /// Expulso ou banido: o servidor some da lista, e se era o aberto, a Home abre.
    fn removed_from(self: &Rc<Self>, server: i64) {
        if *lock(&self.opened) == Some(server) {
            self.show_hub_home();

            return;
        }

        let (api, window, known, me) = (self.api.clone(), self.window.clone(), self.servers.clone(), self.me.clone());

        self.spawn(async move {
            if let Ok(servers) = api.servers().await {
                let rows = rows_of(&servers, None, *lock(&me));

                *lock(&known) = servers;
                paint(&window, move |app| app.global::<Ui>().set_servers(model(rows)));
            }
        });
    }

    fn paint_members(self: &Rc<Self>) {
        let Some(tree) = lock(&self.opened).and_then(|server| self.api.known_tree(server)) else {
            return;
        };
        let groups = member_groups(&tree, &lock(&self.online), *lock(&self.me));

        paint(&self.window, move |app| app.global::<Ui>().set_member_groups(model_of_groups(groups)));
    }

    /// Acerta os canais que o tempo real segue com o que está aberto: o servidor (presença e
    /// mudanças), cada canal de voz dele (quem entra e sai), o canal de texto lido e o chat da
    /// voz em que se está.
    fn follow(self: &Rc<Self>) {
        let Some(live) = lock(&self.live).clone() else {
            return;
        };
        let mut wanted = BTreeSet::new();

        if let Some(server) = *lock(&self.opened) {
            wanted.insert(format!("server.{server}"));

            if let Some(tree) = self.api.known_tree(server) {
                for channel in tree.channels.iter().filter(|channel| channel.kind == ChannelKind::Voice) {
                    wanted.insert(format!("channel.{}", channel.id));
                }
            }
        }

        for channel in [lock(&self.reading).clone(), lock(&self.voice_channel).clone()].into_iter().flatten() {
            wanted.insert(format!("channel.{channel}"));
        }

        let (gone, fresh): (Vec<String>, Vec<String>) = {
            let mut followed = lock(&self.followed);
            let gone = followed.difference(&wanted).cloned().collect();
            let fresh = wanted.difference(&followed).cloned().collect();

            *followed = wanted;

            (gone, fresh)
        };

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

    fn chime(&self, chime: Chime) {
        crate::sound::chime(lock(&self.chosen).1.clone(), chime.samples());
    }

    fn notify(&self, text: &str, error: bool) {
        notify(&self.window, &self.toasts, text, error);
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
            online: self.online.clone(),
            voice: self.voice.clone(),
        }
    }

    fn open_server(self: &Rc<Self>, index: i32) {
        let Some(server) = at(&self.servers, index) else {
            return;
        };

        let (api, window) = (self.api.clone(), self.window.clone());
        let id = server.id;

        *lock(&self.opened) = Some(id);
        lock(&self.online).clear();

        let opening = self.opening();

        // O que o núcleo já guardou vai para a tela antes do pedido: a coluna de canais
        // não pisca vazia ao trocar de servidor.
        if let Some(tree) = self.api.known_tree(id) {
            paint_tree(&window, &opening, &tree, false);
        }

        self.spawn(async move {
            show_tree(&api, &window, &opening, id, false).await;
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
                Ok(()) => show_tree(&api, &window, &opening, server, true).await,
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

        let tree = self.tree();
        let listed = lock(&self.channels).clone();
        let topic = channel.topic.clone().unwrap_or_default();

        paint(&window, move |app| {
            let ui = app.global::<Ui>();
            let (text, voice) = split_channels(&listed, Some(chosen), tree.as_ref(), mine);

            ui.set_text_channels(model(text));
            ui.set_voice_channels(model(voice));
            ui.set_channel_name(name.into());
            ui.set_channel_topic(topic.into());
        });

        let tree = self.tree();

        self.follow();
        self.spawn(async move {
            read_channel(&api, &window, &id, mine, tree).await;
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

        let tree = self.tree();

        self.spawn(async move {
            if let Err(failure) = api.edit_message(i64::from(id), &body).await {
                complain(&window, said(&failure));

                return;
            }

            read_channel(&api, &window, &channel, mine, tree).await;
        });
    }

    fn delete_message(self: &Rc<Self>, id: i32) {
        let Some(channel) = lock(&self.reading).clone() else {
            return;
        };

        let (api, window) = (self.api.clone(), self.window.clone());
        let mine = *lock(&self.me);

        let tree = self.tree();

        self.spawn(async move {
            if let Err(failure) = api.delete_message(i64::from(id)).await {
                complain(&window, said(&failure));

                return;
            }

            read_channel(&api, &window, &channel, mine, tree).await;
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

        let tree = self.tree();

        self.spawn(async move {
            if let Err(failure) = api.send_message(&channel, &body).await {
                complain(&window, said(&failure));

                return;
            }

            // Reler o canal em vez de emendar a mensagem na lista: o que aparece é o que o
            // servidor gravou, e não o que este app achou que mandou.
            read_channel(&api, &window, &channel, mine, tree).await;
        });
    }

    /// O nome de quem entra numa sala por código. Com conta é sempre o apelido da conta: o
    /// que se digitou antes do login, ou o guardado da última sala, não vale — e as "Últimas
    /// salas" da Home, que não têm campo, entravam sem nome nenhum.
    fn room_name(&self, typed: &str) -> String {
        let Some(app) = self.window.upgrade() else {
            return typed.to_owned();
        };
        let ui = app.global::<Ui>();

        if ui.get_signed_in() { ui.get_user_name().into() } else { typed.to_owned() }
    }

    fn enter(self: &Rc<Self>, opened: Result<String, EntryRefusal>, voice: Option<String>) {
        self.connect(opened, voice, None);
    }

    /// Entrar num canal de voz **sem sair do hub**: é o que o React faz, e o que o Discord
    /// fez antes dele. Quem está dentro aparece embaixo do nome do canal, e a tela continua
    /// sendo a do servidor.
    fn join_voice(self: &Rc<Self>, channel: &Channel) {
        self.join_voice_channel(&channel.id, &channel.name);
    }

    /// O canal é sempre do servidor aberto: é na árvore dele que se clica.
    fn join_voice_channel(self: &Rc<Self>, id: &str, name: &str) {
        *lock(&self.voice_channel) = Some(id.to_owned());
        *lock(&self.voice_server) = *lock(&self.opened);
        self.follow();

        // O clique vale na hora: o canal se marca e o painel diz "Conectando…" enquanto o
        // token, o SFU e o microfone acontecem por trás.
        let (shown_id, shown_name) = (id.to_owned(), name.to_owned());

        paint(&self.window, move |app| {
            let ui = app.global::<Ui>();

            ui.set_voice_channel(shown_id.into());
            ui.set_voice_name(shown_name.into());
            ui.set_voice_state("connecting".into());
        });
        self.connect(Ok(id.to_owned()), Some(id.to_owned()), Some(name.to_owned()));
    }

    /// Abre ou fecha o chat da voz. Abrir relê o canal e zera as não lidas; aberto, o tempo
    /// real mantém a conversa em dia.
    fn toggle_voice_chat(self: &Rc<Self>) {
        let Some(app) = self.window.upgrade() else {
            return;
        };
        let ui = app.global::<Ui>();
        let open = !ui.get_voice_chat_open();

        ui.set_voice_chat_open(open);

        if open {
            ui.set_voice_chat_unread(0);
        }

        if let (true, Some(channel)) = (open, lock(&self.voice_channel).clone()) {
            let (api, window, mine, tree) = (self.api.clone(), self.window.clone(), *lock(&self.me), self.tree());

            self.spawn(async move { read_voice_chat(&api, &window, &channel, mine, tree).await });
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

        let tree = self.tree();

        self.spawn(async move {
            if let Err(failure) = api.send_message(&channel, &body).await {
                complain(&window, said(&failure));

                return;
            }

            read_voice_chat(&api, &window, &channel, mine, tree).await;
        });
    }

    /// Sai da voz ou da sala por código, o que estiver aberto. É o que fechar a janela faz: ela
    /// só se esconde (os Clips seguem na bandeja), e a chamada não pode ficar aberta sem ela.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub fn hang_up(self: &Rc<Self>) {
        if lock(&self.voice_channel).is_some() {
            self.leave_voice();
        } else if lock(&self.room).is_some() {
            self.leave_room();
        }
    }

    /// Sai da voz e continua no servidor. É o fone cortado da barra de baixo.
    fn leave_voice(self: &Rc<Self>) {
        let held = self.close_room();

        self.forget_voice();
        self.chime(Chime::Left);

        self.spawn(async move {
            if let Some(room) = held {
                room.leave().await;
            }
        });
    }

    /// O canal de voz sai da barra de baixo e do tempo real.
    fn forget_voice(self: &Rc<Self>) {
        *lock(&self.voice_channel) = None;
        self.follow();

        paint(&self.window, |app| {
            let ui = app.global::<Ui>();

            ui.set_voice_channel(SharedString::new());
            ui.set_voice_name(SharedString::new());
            ui.set_voice_state(SharedString::new());
            ui.set_stage_open(false);
            ui.set_voice_chat_open(false);
            ui.set_voice_chat_unread(0);
            ui.set_voice_messages(ModelRc::default());
        });
    }

    /// De volta à sala que ficou no ar: a por código volta a tomar a janela, e o canal de voz
    /// abre o palco dele no hub.
    fn back_to_room(self: &Rc<Self>) {
        let in_voice = lock(&self.voice).inside;

        paint(&self.window, move |app| {
            let ui = app.global::<Ui>();

            if in_voice {
                ui.set_screen("hub".into());
                ui.set_stage_open(true);
            } else {
                ui.set_screen("room".into());
            }
        });
    }

    /// Uma sala por vez, como o núcleo faz na ABI do Mac: entrar em outra sai desta antes.
    /// Sem isto a de antes seguia viva no SFU, ao lado da nova, e a pessoa aparecia duas
    /// vezes. Devolve a sala para quem entra esperar a despedida dela.
    fn leave_for_another(self: &Rc<Self>, into_voice: bool) -> Option<Arc<Room>> {
        lock(&self.entered).as_ref()?;

        let was_voice = lock(&self.voice).inside;
        let held = self.close_room();

        if was_voice && !into_voice {
            self.forget_voice();
        }

        if !was_voice {
            paint(&self.window, |app| app.global::<Ui>().set_room_code(SharedString::new()));
        }

        held
    }

    /// Fecha deste lado o que a sala abriu — o microfone, o que se assiste, o palco — e
    /// devolve a sala para quem chama avisar o servidor da saída.
    fn close_room(self: &Rc<Self>) -> Option<Arc<Room>> {
        let held = lock(&self.room).take();

        *lock(&self.entered) = None;
        lock(&self.entering).cancel();

        // Soltar o que se assiste junta as threads das telas e do som, e uma tela 4K pode estar no
        // meio de um quadro: na thread da janela, com o cadeado na mão, isso a congelava ao sair.
        let (microphone, watch) = (lock(&self.microphone).take(), lock(&self.watch).take());

        std::thread::spawn(move || drop((microphone, watch)));
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

    /// Onde se está agora, do jeito que a versão nova precisa para voltar: a sala por código
    /// ou o canal de voz, e a tela no ar. Fora de sala não há para onde voltar.
    fn resume_point(self: &Rc<Self>) -> Option<Resume> {
        let share = lock(&self.room).as_ref()?.sharing_recipe().as_ref().map(core_app::sharing::choice_of);
        let channel = lock(&self.voice_channel).clone();

        Some(match channel {
            Some(channel) => Resume {
                room: channel,
                name: String::new(),
                voice: Some(VoiceSeat {
                    name: self.window.upgrade().map(|app| app.global::<Ui>().get_voice_name().to_string()).unwrap_or_default(),
                    server: *lock(&self.voice_server),
                }),
                share,
                saved_at: 0,
            },
            None => {
                let state = self.core.state();

                Resume { room: state.room?, name: state.name, voice: None, share, saved_at: 0 }
            }
        })
    }

    /// O botão verde da barra: guarda onde se está, se despede da sala e entrega o app ao
    /// instalador, que o fecha e abre a versão nova — e ela volta para cá. Recusado o aviso do
    /// administrador, a volta é na hora, nesta mesma versão.
    fn install_update(self: &Rc<Self>) {
        let Some((installer, version)) = lock(&self.installer).clone() else {
            return;
        };

        if let Some(point) = self.resume_point() {
            resume::save(&self.core, &point);
        }

        let held = self.close_room();
        let (core, window, resumed, toasts) = (self.core.clone(), self.window.clone(), self.resumed.clone(), self.toasts.clone());

        show(&window, Screen::Updating, format!("Instalando a versão {version}…"));

        self.spawn(async move {
            if let Some(room) = held
                && tokio::time::timeout(LEAVING, room.leave()).await.is_err()
            {
                tracing::warn!("atualização: a sala não se despediu a tempo");
            }

            clips_saved().await;

            // O botão verde é clicado com a janela na frente: a versão nova volta com ela.
            if install(&installer, false) {
                return;
            }

            *lock(&resumed) = resume::take(&core);
            show(&window, core.home(), String::new());
            notify(&window, &toasts, "A atualização não foi instalada. Ela fica pronta no botão verde.", true);
            paint(&window, |app| app.global::<Ui>().invoke_resume_session());
        });
    }

    /// Volta para onde se estava antes da atualização. Quem assistia volta assistindo sem
    /// nada guardado: entrar na sala já abre as telas de quem está transmitindo.
    fn resume_session(self: &Rc<Self>) {
        let Some(point) = lock(&self.resumed).take() else {
            return;
        };

        *lock(&self.resumed_share) = point.share.as_ref().map(core_app::sharing::capture_config);

        let Some(seat) = point.voice else {
            let opened = self.core.join_room(&point.name, &point.room);

            return self.enter(opened, None);
        };
        let index = seat.server.and_then(|server| lock(&self.servers).iter().position(|known| known.id == server));

        if let Some(index) = index.and_then(|index| i32::try_from(index).ok()) {
            self.open_server(index);
        }

        self.join_voice_channel(&point.room, &seat.name);
    }

    fn connect(
        self: &Rc<Self>,
        opened: Result<String, EntryRefusal>,
        voice: Option<String>,
        staying: Option<String>,
    ) {
        paint_recent(&self.core, &self.window);

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

        // A casinha leva à Home com a sala no ar, e o código digitado de novo — ou a sala das
        // recentes — é o caminho de volta, e não uma segunda sessão.
        if lock(&self.entered).as_deref() == Some(room.as_str()) {
            return self.back_to_room();
        }

        let previous = self.leave_for_another(staying.is_some());

        // O `entered` só vale quando a entrada termina: dois pedidos seguidos — o duplo clique
        // num código das recentes — abriam duas sessões, e o SFU derrubava a primeira com
        // "entrou por outro lugar", com a outra viva por baixo da tela.
        if !lock(&self.entering).begin(&room) {
            return;
        }

        let entering = self.entering.clone();
        let window = self.window.clone();
        let identity = self.identity(&room, voice);
        let in_voice = staying.is_some();
        let entered = self.entered.clone();
        let (held, watch, stage, voice, started) = (
            self.room.clone(),
            self.watch.clone(),
            self.stage.clone(),
            self.voice.clone(),
            self.since.clone(),
        );
        let microphone = self.microphone.clone();
        let (microphone_device, speaker_device) = lock(&self.chosen).clone();
        // Tirada já: se esta entrada falhar, a próxima que a pessoa fizer à mão não pode sair
        // transmitindo sozinha.
        let resumed_share = lock(&self.resumed_share).take();
        let (voice_channel, input_mode) = (self.voice_channel.clone(), self.input_mode());

        // Da escolha do canal até o microfone abrir, o botão não pinta mudo.
        lock(&voice).opening = in_voice;
        paint(&window, |app| app.global::<Ui>().set_entry_busy(true));

        self.spawn(async move {
            if let Some(previous) = previous {
                previous.leave().await;
            }

            let (updates, heard) = std::sync::mpsc::channel();
            let attempt = Room::enter(&url, &room, identity, updates).await;

            paint(&window, |app| app.global::<Ui>().set_entry_busy(false));

            // Outra sala foi pedida, ou a pessoa saiu, enquanto esta esperava o servidor: quem
            // manda na tela agora é a outra, e esta se despede.
            if !lock(&entering).finish(&room) {
                if let Ok((opened, _)) = attempt {
                    opened.leave().await;
                }

                return;
            }

            let (opened, media) = match attempt {
                Ok(entered) => entered,
                Err(failure) => {
                    lock(&voice).opening = false;
                    paint_voice(&window, &voice);

                    let reason = sentence(Failure::from_error(&failure));

                    complain(&window, format!("Não deu para entrar na sala. {reason}"));

                    // A entrada falhou: a pessoa sai da lista do canal, como entrou.
                    if in_voice {
                        *lock(&voice_channel) = None;
                        paint(&window, |app| {
                            let ui = app.global::<Ui>();

                            ui.set_voice_channel(SharedString::new());
                            ui.set_voice_state(SharedString::new());
                        });
                    }

                    return;
                }
            };

            *lock(&held) = Some(opened.clone());
            *lock(&entered) = Some(room.clone());
            // Desde a primeira pessoa, como a duração de uma chamada: quem entra depois vê o
            // tempo de quem já estava.
            *lock(&started) = Some(opened.started().unwrap_or_else(std::time::Instant::now));

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

            let speaker = Arc::new(Speaker::start(speaker_device.clone()));

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
            }, {
                let (window, cell, pending) = (window.clone(), watch.clone(), Arc::new(std::sync::atomic::AtomicBool::new(false)));

                move || {
                    // Um aviso por vez na fila: com a janela atrasada, os quadros se juntam
                    // no próximo desenho em vez de empilhar avisos.
                    if pending.swap(true, std::sync::atomic::Ordering::AcqRel) {
                        return;
                    }

                    let (window, cell, pending) = (window.clone(), cell.clone(), pending.clone());

                    let _ = slint::invoke_from_event_loop(move || {
                        pending.store(false, std::sync::atomic::Ordering::Release);
                        draw_fresh(&window, &cell);
                    });
                }
            }, {
                let room = Arc::downgrade(&opened);

                move |producer: &str| {
                    if let Some(room) = room.upgrade() {
                        room.request_keyframe(producer);
                    }
                }
            }));

            listen(heard, window.clone(), stage.clone(), voice.clone());

            if in_voice {
                crate::sound::chime(speaker_device.clone(), Chime::Joined.samples());
            }

            let code = room.clone();

            paint(&window, move |app| {
                let ui = app.global::<Ui>();

                ui.set_complaint(SharedString::new());

                match staying {
                    // Canal de voz: o hub continua na tela, e o canal aberto se marca.
                    Some(name) => {
                        ui.set_voice_channel(code.clone().into());
                        ui.set_voice_name(name.into());
                        ui.set_voice_state("connected".into());
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
                open_microphone(opened.clone(), microphone, voice.clone(), window.clone(), microphone_device, input_mode).await;
            }

            lock(&voice).opening = false;
            paint_voice(&window, &voice);

            if let Some(recipe) = resumed_share
                && let Err(failure) = opened.share(recipe).await
            {
                tracing::warn!(%failure, "retomada: a tela não voltou ao ar");
                complain(&window, room_failure("share"));
            }
        });
    }

    /// O "Parar": desliga a transmissão. Sem transmissão no ar, abre o seletor — começar
    /// sempre passa pela escolha da tela, como no React.
    fn toggle_share(self: &Rc<Self>) {
        if !lock(&self.voice).mine.sharing {
            return self.open_share();
        }

        self.with_room(|room| async move { room.stop_sharing().await });
    }

    /// Abre o seletor com a qualidade da última vez e lista o que dá para compartilhar. Listar
    /// e tirar as prévias leva segundos, e fica numa thread: a janela abre na hora, com o
    /// esqueleto do React no lugar dos cartões.
    fn open_share(self: &Rc<Self>) {
        let (quality, fps) = self.core.share_quality();

        paint(&self.window, move |app| {
            let ui = app.global::<Ui>();

            ui.set_share_quality(quality.into());
            ui.set_share_fps(fps.into());
            ui.set_share_tab("display".into());
            ui.set_share_source(SharedString::new());
            ui.set_share_displays(ModelRc::default());
            ui.set_share_windows(ModelRc::default());
            ui.set_share_loading(true);
            ui.set_share_open(true);
        });

        let window = self.window.clone();

        // Com o portal do Wayland quem escolhe a tela ou a janela é o sistema, ao confirmar:
        // não há o que listar nem prévia que tirar, e o seletor mostra só as opções.
        if capture::uses_system_picker() {
            paint(&self.window, |app| {
                let ui = app.global::<Ui>();
                let system = Source {
                    value: "display:0".into(),
                    label: "A tela ou a janela que você escolher".into(),
                    detail: "O sistema abre o seletor ao confirmar".into(),
                };

                ui.set_share_source(system.value.clone().into());
                ui.set_share_displays(model(vec![system.row()]));
                ui.set_share_loading(false);
            });

            return;
        }

        std::thread::spawn(move || {
            let listed = core_app::sharing::displays().unwrap_or_else(|failure| {
                tracing::warn!(%failure, "seletor: não deu para listar as telas");

                serde_json::Value::Null
            });
            let mut displays = sources_of(&listed["displays"], |display| Source {
                value: format!("display:{}", display["id"]),
                label: String::new(),
                detail: format!("{}×{}", display["width"], display["height"]),
            });

            // O id é o número que o Windows deu ao monitor, não a posição: o rótulo é a posição.
            for (index, display) in displays.iter_mut().enumerate() {
                display.label = format!("Tela {}", index + 1);
            }
            let windows = sources_of(&listed["windows"], |shown| Source {
                value: format!("window:{}", shown["id"]),
                label: shown["title"].as_str().unwrap_or_default().to_owned(),
                detail: shown["application"].as_str().unwrap_or_default().to_owned(),
            });
            let windows: Vec<Source> = windows.into_iter().filter(|shown| !shown.label.trim().is_empty()).take(MAX_WINDOWS).collect();
            let previewed: Vec<String> = displays.iter().map(|display| display.value.clone()).collect();

            paint(&window, move |app| {
                let ui = app.global::<Ui>();

                ui.set_share_source(displays.first().map(|display| display.value.clone()).unwrap_or_default().into());
                ui.set_share_displays(model(displays.into_iter().map(Source::row).collect()));
                ui.set_share_windows(model(windows.into_iter().map(Source::row).collect()));
                ui.set_share_loading(false);
            });

            load_previews(&window, previewed);
        });
    }

    /// Troca a aba. Os aplicativos só ganham prévia aqui, e só os quatro primeiros: cada uma
    /// é uma captura curta, e dezenas delas travariam o seletor.
    fn pick_share_tab(self: &Rc<Self>, tab: &str) {
        let Some(app) = self.window.upgrade() else {
            return;
        };
        let ui = app.global::<Ui>();
        let items = if tab == "window" { ui.get_share_windows() } else { ui.get_share_displays() };
        let first = items.row_data(0).map(|row| row.value).unwrap_or_default();

        ui.set_share_tab(tab.into());
        ui.set_share_source(first);

        if tab != "window" {
            return;
        }

        let wanted: Vec<String> = (0..items.row_count().min(MAX_WINDOW_PREVIEWS))
            .filter_map(|index| items.row_data(index))
            .filter(|row| !row.has_preview)
            .map(|row| row.value.to_string())
            .collect();
        let window = self.window.clone();

        std::thread::spawn(move || load_previews(&window, wanted));
    }

    /// Transmite o que foi escolhido. Com a tela no ar e o mesmo áudio, troca a fonte sem
    /// derrubar ninguém (`change_quality`); mudando o áudio, para e recomeça, como o React.
    fn confirm_share(self: &Rc<Self>) {
        let Some(app) = self.window.upgrade() else {
            return;
        };
        let ui = app.global::<Ui>();
        let (quality, fps) = (ui.get_share_quality().to_string(), ui.get_share_fps().to_string());
        let recipe = core_app::sharing::capture_config(&serde_json::json!({
            "source": ui.get_share_source().to_string(),
            "quality": quality,
            "fps": fps.parse::<u64>().unwrap_or(60),
            "audio": ui.get_share_audio(),
            "muteCalls": ui.get_share_mute_calls(),
        }));
        let window = self.window.clone();

        ui.set_share_open(false);
        self.core.set_share_quality(&quality, &fps);

        self.with_room(move |room| async move {
            let live = room.sharing_recipe();
            let same_sound = live.as_ref().is_some_and(|live| {
                (live.capture_audio, live.mute_listed_apps) == (recipe.capture_audio, recipe.mute_listed_apps)
            });

            // No Wayland a fonte é a que o seletor do sistema deu: trocar só a qualidade vale
            // com a tela no ar; trocar de tela é parar e começar de novo, que reabre o seletor.
            let source = (!capture::uses_system_picker()).then_some(recipe.source);
            let started = if same_sound {
                room.change_quality(recipe.quality, recipe.frame_rate, source).await
            } else {
                if live.is_some() {
                    room.stop_sharing().await;
                }

                // O seletor do sistema (o portal do Wayland) abre aqui, antes da captura, e
                // espera a pessoa escolher — por isso fora do laço do Tokio. No X11, no Windows
                // e no macOS é uma chamada vazia.
                let prepared = match tokio::task::block_in_place(|| capture::prepare(&recipe)) {
                    Ok(prepared) => prepared,
                    Err(failure) => {
                        tracing::warn!(%failure, "o seletor de tela não abriu");
                        complain(&window, "Não deu para escolher a tela.");

                        return;
                    }
                };
                let shared = room.share(recipe).await;

                // A captura consumiu a sessão escolhida; largá-la antes fecharia o que o seletor
                // abriu, e o sistema ficaria dizendo que a tela está sendo compartilhada.
                drop(prepared);

                shared
            };

            if let Err(failure) = started {
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
        let (device, input_mode) = (lock(&self.chosen).0.clone(), self.input_mode());

        lock(&voice).opening = true;
        paint_voice(&window, &voice);

        self.spawn(async move {
            open_microphone(room, cell, voice.clone(), window.clone(), device, input_mode).await;

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

    /// A árvore do servidor aberto, como o núcleo a guardou.
    fn tree(&self) -> Option<ServerTree> {
        lock(&self.opened).and_then(|server| self.api.known_tree(server))
    }

    fn voice_preference(&self) -> serde_json::Value {
        self.core.preference(VOICE_KEY).unwrap_or(serde_json::Value::Null)
    }

    /// O modo do microfone guardado: detecção de voz com a sensibilidade, apertar para
    /// falar, ou sempre aberto. É o núcleo (`InputMode`) que fecha e abre o microfone.
    fn input_mode(&self) -> InputMode {
        let voice = self.voice_preference();

        InputMode::parse(voice["mode"].as_str().unwrap_or("open"), voice["sensitivity"].as_u64().unwrap_or(35))
    }

    /// Voz e vídeo na tela, a partir do que está guardado.
    fn paint_voice_preferences(self: &Rc<Self>) {
        let voice = self.voice_preference();
        let flag = |key: &str, default: bool| voice[key].as_bool().unwrap_or(default);
        let mode = voice["mode"].as_str().unwrap_or("open").to_owned();
        let sensitivity = i32::try_from(voice["sensitivity"].as_i64().unwrap_or(35)).unwrap_or(35);
        let (mute_on_join, echo, noise, gain) =
            (flag("muteOnJoin", false), flag("echoCancel", true), flag("noiseSuppress", true), flag("autoGain", true));

        lock(&self.voice).mute_on_join = mute_on_join;

        paint(&self.window, move |app| {
            let ui = app.global::<Ui>();

            ui.set_input_mode(mode.into());
            ui.set_sensitivity(sensitivity);
            ui.set_mute_on_join(mute_on_join);
            ui.set_echo_cancel(echo);
            ui.set_noise_suppress(noise);
            ui.set_auto_gain(gain);
        });
    }

    fn save_voice(self: &Rc<Self>, change: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>)) {
        let mut voice = self.voice_preference();

        if !voice.is_object() {
            voice = serde_json::json!({});
        }

        if let Some(fields) = voice.as_object_mut() {
            change(fields);
        }

        self.core.set_preference(VOICE_KEY, voice);
        self.paint_voice_preferences();
    }

    fn set_input_mode(self: &Rc<Self>, mode: &str, sensitivity: i64) {
        let mode = mode.to_owned();

        self.save_voice(move |voice| {
            voice.insert("mode".into(), mode.into());
            voice.insert("sensitivity".into(), sensitivity.clamp(0, 100).into());
        });

        if let Some(room) = lock(&self.room).clone() {
            room.set_input_mode(self.input_mode());
        }
    }

    // ponytail: "echoCancel", "noiseSuppress" e "autoGain" só ficam guardadas até o
    // `VoiceProcessor` do `shared/media` ler a preferência; a chave já é a dele.
    fn set_voice_flag(self: &Rc<Self>, key: &str, on: bool) {
        let key = key.to_owned();

        self.save_voice(move |voice| {
            voice.insert(key, on.into());
        });
    }

    /// Abre um modal. Os de canal levam o canal clicado para os campos.
    fn open_modal(self: &Rc<Self>, kind: &str, index: i32) {
        let (kind, channel) = (kind.to_owned(), at(&self.channels, index));

        paint(&self.window, move |app| {
            let ui = app.global::<Ui>();

            match &channel {
                Some(channel) => {
                    ui.set_modal_channel_index(index);
                    ui.set_modal_channel_name(channel.name.clone().into());
                    ui.set_modal_channel_topic(channel.topic.clone().unwrap_or_default().into());
                    ui.set_modal_channel_limit(i32::try_from(channel.user_limit.unwrap_or(0)).unwrap_or(0));
                    ui.set_modal_channel_voice(channel.kind == ChannelKind::Voice);
                }
                None => {
                    ui.set_modal_channel_index(-1);
                    ui.set_modal_channel_voice(false);
                }
            }

            ui.set_modal(kind.into());
        });
    }

    /// O "tem certeza?" antes do que não volta. A frase é montada aqui; o que fazer fica
    /// guardado para o `confirm`.
    fn ask(self: &Rc<Self>, action: &str, id: i32) {
        let server = self.tree().map(|tree| tree.name).unwrap_or_default();
        let person = self.member_name(i64::from(id));
        let channel = at(&self.channels, id).map(|channel| channel.name).unwrap_or_default();
        let (title, text, button) = match action {
            "delete-channel" => (
                format!("Excluir #{channel}"),
                format!("Tem certeza de que quer excluir #{channel}? Isso não pode ser desfeito."),
                "Excluir canal",
            ),
            "delete-server" => (
                "Excluir servidor".to_owned(),
                format!("Tem certeza de que quer excluir {server}? Isso não pode ser desfeito."),
                "Excluir servidor",
            ),
            "leave-server" => (
                format!("Sair de {server}"),
                format!("Tem certeza de que quer sair de {server}? Para voltar, vai precisar de um convite novo."),
                "Sair do servidor",
            ),
            "kick" => (
                format!("Expulsar {person}"),
                format!("Tem certeza de que quer expulsar {person}? Com um convite a pessoa volta."),
                "Expulsar",
            ),
            "ban" => (
                format!("Banir {person}"),
                format!("Tem certeza de que quer banir {person}? A pessoa não volta nem com convite, até ser perdoada."),
                "Banir",
            ),
            _ => return,
        };

        *lock(&self.pending) = Some((action.to_owned(), i64::from(id)));

        paint(&self.window, move |app| {
            let ui = app.global::<Ui>();

            ui.set_confirm_title(title.into());
            ui.set_confirm_text(text.into());
            ui.set_confirm_button(button.into());
            ui.set_modal("confirm".into());
        });
    }

    fn confirm(self: &Rc<Self>) {
        let Some((action, id)) = lock(&self.pending).take() else {
            return;
        };

        match action.as_str() {
            "delete-channel" => {
                if let Some(channel) = at(&self.channels, i32::try_from(id).unwrap_or(-1)) {
                    self.manage("deleteChannel", serde_json::Value::Null, serde_json::json!({ "channel": channel.id }), None);
                }
            }
            "delete-server" => self.manage("deleteServer", serde_json::Value::Null, serde_json::json!({}), None),
            "leave-server" => self.manage("leaveServer", serde_json::json!({}), serde_json::json!({}), None),
            "kick" => self.manage("kickMember", serde_json::Value::Null, serde_json::json!({ "user": id }), None),
            "ban" => self.manage("ban", serde_json::json!({}), serde_json::json!({ "user": id }), None),
            _ => {}
        }
    }

    fn member_name(&self, user: i64) -> String {
        self.tree()
            .and_then(|tree| tree.members.iter().find(|member| member.user_id == user).map(|member| core_app::members::display_name(member).to_owned()))
            .unwrap_or_default()
    }

    /// Uma escrita no servidor aberto pelo mapa de rotas do núcleo. `params` ganha o
    /// `server` sozinho. Depois a árvore é relida; apagar ou sair do servidor volta à Home.
    fn manage(self: &Rc<Self>, route: &'static str, body: serde_json::Value, mut params: serde_json::Value, success: Option<String>) {
        let Some(server) = *lock(&self.opened) else {
            return;
        };

        params["server"] = server.into();

        let (api, window, opening, landing) = (self.api.clone(), self.window.clone(), self.opening(), self.landing());
        let selected = self.window.upgrade().map(|app| app.global::<Ui>().get_selected_role()).unwrap_or(-1);
        let gone = matches!(route, "deleteServer" | "leaveServer");
        let toasts = self.toasts.clone();

        self.spawn(async move {
            if let Err(failure) = api.perform(route, &params, &body).await {
                return complain(&window, said(&failure));
            }

            if let Some(text) = success {
                notify(&window, &toasts, &text, false);
            }

            if gone {
                refresh_servers(&api, &window, &landing).await;

                return;
            }

            show_tree(&api, &window, &opening, server, true).await;

            if route == "updateServer" {
                refresh_servers(&api, &window, &landing).await;
                paint(&window, |app| app.global::<Ui>().set_in_server(true));
            }

            // Mexeu em cargo: a lista de permissões do cargo escolhido acompanha.
            if route.contains("Role")
                && let Some(tree) = api.known_tree(server)
            {
                paint_role_permissions(&window, &tree, selected);
            }
        });
    }

    fn regenerate_invite(self: &Rc<Self>) {
        let Some(server) = *lock(&self.opened) else {
            return;
        };
        let (api, window) = (self.api.clone(), self.window.clone());

        self.spawn(async move {
            match api.regenerate_invite(server).await {
                Ok(code) => paint(&window, move |app| app.global::<Ui>().set_invite_code(code.into())),
                Err(failure) => complain(&window, said(&failure)),
            }
        });
    }

    fn update_channel(self: &Rc<Self>, index: i32, name: &str, topic: &str, limit: i32) {
        let Some(channel) = at(&self.channels, index) else {
            return;
        };
        let name = name.trim();

        if name.is_empty() {
            return;
        }

        let body = if channel.kind == ChannelKind::Voice {
            serde_json::json!({ "name": name, "user_limit": (limit > 0).then_some(limit) })
        } else {
            serde_json::json!({ "name": name, "topic": topic.trim() })
        };

        self.manage("updateChannel", body, serde_json::json!({ "channel": channel.id }), None);
    }

    /// Abre as configurações do servidor numa aba, buscando o que a aba mostra.
    fn open_server_settings(self: &Rc<Self>, tab: &str) {
        let Some(server) = *lock(&self.opened) else {
            return;
        };
        let (api, window, tab) = (self.api.clone(), self.window.clone(), tab.to_owned());
        let listing = tab.clone();

        paint(&self.window, move |app| {
            let ui = app.global::<Ui>();

            ui.set_server_settings_tab(tab.into());
            ui.set_server_settings_open(true);
        });

        match listing.as_str() {
            "bans" => self.spawn(async move {
                match api.perform("bans", &serde_json::json!({ "server": server }), &serde_json::Value::Null).await {
                    Ok(listed) => {
                        let rows: Vec<BanRow> = listed["data"]
                            .as_array()
                            .or(listed.as_array())
                            .into_iter()
                            .flatten()
                            .map(|ban| BanRow {
                                user_id: i32::try_from(ban["user_id"].as_i64().unwrap_or_default()).unwrap_or_default(),
                                name: ban["name"].as_str().unwrap_or_default().into(),
                                reason: ban["reason"].as_str().unwrap_or_default().into(),
                            })
                            .collect();

                        paint(&window, move |app| app.global::<Ui>().set_bans(model(rows)));
                    }
                    Err(failure) => complain(&window, said(&failure)),
                }
            }),
            "audits" => self.spawn(async move {
                match api.perform("audits", &serde_json::json!({ "server": server }), &serde_json::Value::Null).await {
                    Ok(listed) => {
                        let rows: Vec<AuditRow> = listed["data"]
                            .as_array()
                            .or(listed.as_array())
                            .into_iter()
                            .flatten()
                            .map(|entry| AuditRow {
                                at: at_of(entry["at"].as_str().unwrap_or_default()),
                                text: format!(
                                    "{} {}",
                                    entry["actor"]["name"].as_str().unwrap_or("Alguém"),
                                    entry["summary"].as_str().unwrap_or_default()
                                )
                                .into(),
                            })
                            .collect();

                        paint(&window, move |app| app.global::<Ui>().set_audits(model(rows)));
                    }
                    Err(failure) => complain(&window, said(&failure)),
                }
            }),
            _ => {}
        }
    }

    /// Os cargos do mais alto para o mais baixo, como a tela lista.
    fn roles(&self) -> Vec<core_app::models::Role> {
        let mut roles = self.tree().map(|tree| tree.roles).unwrap_or_default();

        roles.sort_by_key(|role| std::cmp::Reverse(role.position));

        roles
    }

    fn select_role(self: &Rc<Self>, index: i32) {
        if let Some(tree) = self.tree() {
            paint_role_permissions(&self.window, &tree, index);
        }
    }

    fn toggle_permission(self: &Rc<Self>, bit: i32) {
        let selected = self.window.upgrade().map(|app| app.global::<Ui>().get_selected_role()).unwrap_or(-1);
        let Some(role) = usize::try_from(selected).ok().and_then(|index| self.roles().get(index).cloned()) else {
            return;
        };
        let permissions = role.permissions ^ i64::from(bit);

        self.manage("updateRole", serde_json::json!({ "permissions": permissions }), serde_json::json!({ "role": role.id }), None);
    }

    /// Desconecta (`to` vazio) ou move uma pessoa da voz em que está. Quem decide se pode é
    /// o Laravel; a pessoa movida entra no destino sozinha, pelo `moved` do SFU.
    fn move_voice(self: &Rc<Self>, user: i32, to: Option<i32>) {
        let user = i64::from(user);
        let Some(tree) = self.tree() else {
            return;
        };
        let Some(from) = tree.voice.iter().find(|(_, people)| people.iter().any(|person| person.user_id == user)).map(|(channel, _)| channel.clone()) else {
            return complain(&self.window, "Essa pessoa não está em nenhuma voz agora.");
        };
        let name = self.member_name(user);
        let params = serde_json::json!({ "channel": from, "user": user });

        match to.and_then(|index| at(&self.channels, index)) {
            Some(target) => self.manage(
                "moveVoiceMember",
                serde_json::json!({ "channel_id": target.id }),
                params,
                Some(format!("{name} foi movido para {}.", target.name)),
            ),
            None if to.is_some() => {}
            None => self.manage("disconnectFromVoice", serde_json::Value::Null, params, None),
        }
    }

    /// Cala uma pessoa só para mim: o microfone dela deixa de tocar aqui.
    fn mute_person(self: &Rc<Self>, user: i64, muted: bool) {
        let microphone = {
            let mut voice = lock(&self.voice);

            if muted {
                voice.muted_people.insert(user);
            } else {
                voice.muted_people.remove(&user);
            }

            voice.microphone_of(user)
        };

        if let (Some(microphone), Some(room)) = (microphone, lock(&self.room).clone()) {
            room.mute_watched(&microphone, muted);
        }

        paint_voice(&self.window, &self.voice);
    }

    /// Um moderador moveu esta pessoa: a sala de antes já parou no núcleo; aqui se fecha o
    /// que ficou aberto, sem o toque de saída, e se entra no destino com token novo.
    fn moved(self: &Rc<Self>, data: &serde_json::Value) {
        let to = data["to"].as_str().unwrap_or_default();
        let by = data["by"].as_str().unwrap_or("Um moderador").to_owned();
        let destination = lock(&self.channels).iter().find(|channel| channel.id == to).cloned();
        let held = self.close_room();

        self.spawn(async move {
            if let Some(room) = held {
                room.leave().await;
            }
        });

        match destination {
            Some(channel) => {
                self.join_voice(&channel);
                self.notify(&format!("{by} moveu você para {}.", channel.name), false);
            }
            None => {
                *lock(&self.voice_channel) = None;
                self.follow();
                paint(&self.window, |app| {
                    let ui = app.global::<Ui>();

                    ui.set_voice_channel(SharedString::new());
                    ui.set_voice_state(SharedString::new());
                    ui.set_stage_open(false);
                });
                self.notify("Você foi movido para um canal que não enxerga.", true);
            }
        }
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
        paint_recent(&self.core, &self.window);

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

        tracing::info!(microphone, label = %device.label, "aparelho escolhido");

        {
            let mut held = lock(&self.chosen);

            if microphone {
                held.0 = Some(device.id.clone());
            } else {
                held.1 = Some(device.id.clone());
            }
        }

        // O que está aberto passa para o aparelho escolhido na hora, e não só na próxima entrada.
        if !microphone {
            if let Some(watch) = lock(&self.watch).as_ref() {
                watch.speaker().use_device(Some(device.id));
            }

            return;
        }

        let (cell, window) = (self.microphone.clone(), self.window.clone());

        if lock(&cell).is_none() {
            return;
        }

        let Some(room) = lock(&self.room).clone() else {
            return;
        };

        self.spawn(async move {
            drop(lock(&cell).take());

            let speaking = room.clone();
            let started = tokio::task::block_in_place(|| Microphone::start(Some(device.id), move |samples| speaking.speak(samples)));

            match started {
                Ok(opened) => *lock(&cell) = Some(opened),
                Err(failure) => {
                    tracing::warn!(failure = %format!("{failure:#}"), "o microfone escolhido não abriu");
                    complain(&window, room_failure("mic"));
                }
            }
        });
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

/// Quantos aplicativos o seletor lista, e quantos ganham prévia — os números do React.
const MAX_WINDOWS: usize = 12;
const MAX_WINDOW_PREVIEWS: usize = 4;

/// Uma fonte do seletor ainda sem imagem: a linha do Slint (com `image`) só nasce na janela.
struct Source {
    value: String,
    label: String,
    detail: String,
}

impl Source {
    fn row(self) -> crate::SourceRow {
        crate::SourceRow {
            value: self.value.into(),
            label: self.label.into(),
            detail: self.detail.into(),
            preview: Image::default(),
            has_preview: false,
        }
    }
}

fn sources_of(listed: &serde_json::Value, source: impl Fn(&serde_json::Value) -> Source) -> Vec<Source> {
    listed.as_array().map(|items| items.iter().map(source).collect()).unwrap_or_default()
}

/// Tira a prévia de cada fonte, uma por vez, e a põe no cartão dela assim que fica pronta.
/// Fonte sem prévia (janela minimizada, captura recusada) fica com "sem prévia".
fn load_previews(window: &Weak<AppWindow>, sources: Vec<String>) {
    for value in sources {
        let source = core_app::sharing::capture_config(&serde_json::json!({ "source": value })).source;
        let jpeg = capture::PlatformCapturer::preview(source).unwrap_or_default();

        if jpeg.is_empty() {
            continue;
        }

        let Ok(decoded) = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg) else {
            tracing::warn!(value, "seletor: a prévia não abriu");

            continue;
        };
        let decoded = decoded.to_rgb8();
        let buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::clone_from_slice(decoded.as_raw(), decoded.width(), decoded.height());

        paint(window, move |app| {
            let ui = app.global::<Ui>();

            for items in [ui.get_share_displays(), ui.get_share_windows()] {
                let found = (0..items.row_count()).find_map(|index| items.row_data(index).filter(|row| row.value == value).map(|row| (index, row)));

                if let Some((index, mut row)) = found {
                    row.preview = Image::from_rgb8(buffer.clone());
                    row.has_preview = true;
                    items.set_row_data(index, row);
                }
            }
        });
    }
}

/// Abre o tempo real da conta e segue o canal dela (`user.{id}`: amizades, mensagens
/// diretas, expulsões). Cada evento vai para a janela, e da janela para a ponte.
async fn go_live(
    api: &Arc<Api>,
    sfu: &Arc<Mutex<Option<String>>>,
    live: &Arc<Mutex<Option<Arc<Realtime>>>>,
    window: &Weak<AppWindow>,
    account: Option<i64>,
) {
    let Some(url) = lock(sfu).clone() else {
        return;
    };

    if lock(live).is_some() {
        return;
    }

    let (updates, heard) = std::sync::mpsc::channel::<String>();
    let realtime = match Realtime::connect(&url, api.clone(), updates.clone()).await {
        Ok(realtime) => realtime,
        Err(failure) => {
            tracing::warn!(failure = %format!("{failure:#}"), "tempo real: não conectou");

            return;
        }
    };

    if let Some(account) = account
        && let Err(failure) = realtime.subscribe(&format!("user.{account}")).await
    {
        tracing::warn!(%failure, "tempo real: o canal da conta não abriu");
    }

    *lock(live) = Some(realtime);

    let window = window.clone();
    let spawned = std::thread::Builder::new().name("unkvoid-tempo-real".into()).spawn(move || {
        for line in heard {
            paint(&window, move |app| app.global::<Ui>().invoke_heard_live(line.into()));
        }
    });

    if let Err(failure) = spawned {
        tracing::warn!(%failure, "a thread do tempo real não subiu");
    }

    let _ = updates.send(FOLLOW.to_owned());
}

/// Um aviso no pé da janela, que some sozinho: 3,5 s, ou 6 s o de erro — os tempos do React.
fn notify(window: &Weak<AppWindow>, toasts: &Toasts, text: &str, error: bool) {
    let id = {
        let mut held = lock(toasts);

        held.0 += 1;

        let id = held.0;

        held.1.push(ToastRow { id, text: text.into(), error });

        id
    };

    paint_toasts(window, toasts);

    let (window, toasts) = (window.clone(), toasts.clone());

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(if error { 6_000 } else { 3_500 }));
        dismiss(&window, &toasts, id);
    });
}

fn dismiss(window: &Weak<AppWindow>, toasts: &Toasts, id: i32) {
    lock(toasts).1.retain(|toast| toast.id != id);
    paint_toasts(window, toasts);
}

fn paint_toasts(window: &Weak<AppWindow>, toasts: &Toasts) {
    let rows = lock(toasts).1.clone();

    paint(window, move |app| app.global::<Ui>().set_toasts(model(rows)));
}

/// A conversa aberta relida pelo tempo real: só as mensagens mudam — a aba e a tela ficam
/// onde a pessoa está.
fn refresh_direct(window: &Weak<AppWindow>, messages: Vec<DirectMessage>) {
    let rows = direct_rows(&messages);

    paint(window, move |app| app.global::<Ui>().set_direct_messages(model(rows)));
}

fn direct_rows(messages: &[DirectMessage]) -> Vec<MessageRow> {
    messages
        .iter()
        .map(|message| MessageRow {
            id: 0,
            initial: initial(&message.sender.name),
            author: message.sender.name.clone().into(),
            body: message.body.clone().into(),
            at: at_of(&message.created_at),
            mine: message.mine,
            continued: false,
            color: slint::Color::default(),
            colored: false,
        })
        .collect()
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
    let rows = direct_rows(&messages);

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
    let pending = user.as_ref().is_some_and(|user| !user.nickname_confirmed);

    // Quem sou eu decide se um pedido de amizade chegou ou saiu — e isso é lido em toda
    // lista de amigos daqui para a frente.
    let mine = user.as_ref().map(|user| user.id);

    *lock(me) = mine;

    core.show(landing);

    paint(window, move |app| {
        let ui = app.global::<Ui>();

        ui.set_signed_in(signed_in);
        ui.set_nickname_pending(pending);
        ui.set_user_initial(initial(&name));
        ui.set_user_name(name.into());
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
    }

    paint_recent(core, window);
}

/// As três últimas salas, criadas ou visitadas: é o que o dono quer ver, e o núcleo guarda
/// mais. Pintadas na abertura, ao entrar e ao sair de uma sala — com o nome salvo junto,
/// porque a entrada renasce a cada volta e a pastilha entra com o nome do campo.
fn paint_recent(core: &Arc<App>, window: &Weak<AppWindow>) {
    let recent: Vec<String> = core.recent_rooms().into_iter().take(3).collect();
    let recent: Vec<Vec<String>> = recent.chunks(2).map(<[String]>::to_vec).collect();
    let saved = core.state().name;

    paint(window, move |app| {
        let ui = app.global::<Ui>();

        ui.set_recent_rooms(model(code_lines(recent)));
        ui.set_saved_name(saved.into());
    });
}

async fn read_channel(api: &Arc<Api>, window: &Weak<AppWindow>, channel: &str, me: Option<i64>, tree: Option<ServerTree>) {
    match api.messages(channel).await {
        Ok(messages) => {
            let rows = message_rows(&messages, me, tree.as_ref());

            paint(window, move |app| app.global::<Ui>().set_messages(model(rows)));
        }
        Err(failure) => complain(window, said(&failure)),
    }
}

async fn read_voice_chat(api: &Arc<Api>, window: &Weak<AppWindow>, channel: &str, me: Option<i64>, tree: Option<ServerTree>) {
    match api.messages(channel).await {
        Ok(messages) => {
            let rows = message_rows(&messages, me, tree.as_ref());

            paint(window, move |app| app.global::<Ui>().set_voice_messages(model(rows)));
        }
        Err(failure) => complain(window, said(&failure)),
    }
}

/// As mensagens como o Discord as agrupa: a de quem acabou de falar (mesma pessoa, até 7
/// minutos depois) vem sem avatar nem nome, e o nome leva a cor do cargo mais alto.
fn message_rows(messages: &[core_app::models::Message], me: Option<i64>, tree: Option<&ServerTree>) -> Vec<MessageRow> {
    let color_of_user = |user: i64| -> Option<slint::Color> {
        let tree = tree?;
        let member = tree.members.iter().find(|member| member.user_id == user)?;

        core_app::members::top_role(tree, member)?.color.as_deref().and_then(color_of)
    };

    #[allow(clippy::cast_possible_truncation)]
    messages
        .iter()
        .enumerate()
        .map(|(index, message)| {
            let color = color_of_user(message.user.id);
            let previous = index.checked_sub(1).and_then(|index| messages.get(index));

            MessageRow {
                id: message.id as i32,
                initial: initial(&message.user.name),
                author: message.user.name.clone().into(),
                body: if message.kind == "join" { format!("{} chegou no servidor!", message.user.name).into() } else { message.body.clone().into() },
                at: message.created_at.get(11..16).unwrap_or_default().into(),
                mine: Some(message.user.id) == me,
                continued: previous.is_some_and(|previous| continues(previous, message)),
                colored: color.is_some(),
                color: color.unwrap_or_default(),
            }
        })
        .collect()
}

/// A mensagem continua a anterior: mesma pessoa, mesmo dia, menos de 7 minutos depois.
fn continues(previous: &core_app::models::Message, message: &core_app::models::Message) -> bool {
    let minute_of = |stamp: &str| -> Option<i64> {
        let hours: i64 = stamp.get(11..13)?.parse().ok()?;
        let minutes: i64 = stamp.get(14..16)?.parse().ok()?;

        Some(hours * 60 + minutes)
    };

    previous.user.id == message.user.id
        && previous.kind == message.kind
        && previous.created_at.get(..10) == message.created_at.get(..10)
        && matches!((minute_of(&previous.created_at), minute_of(&message.created_at)), (Some(before), Some(now)) if (0..7).contains(&(now - before)))
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
    online: Arc<Mutex<HashSet<i64>>>,
    /// A voz recebe da árvore o que eu posso com cada pessoa, para o menu da lista.
    voice: Arc<Mutex<Voice>>,
}

/// Busca a árvore e a desenha. `keep` é a releitura do tempo real: o canal que se está lendo
/// continua aberto. Chegada a árvore, o tempo real acerta os canais que segue.
async fn show_tree(api: &Arc<Api>, window: &Weak<AppWindow>, opening: &Opening, id: i64, keep: bool) {
    let tree = match api.tree(id).await {
        Ok(tree) => tree,
        Err(failure) => return complain(window, said(&failure)),
    };

    paint_tree(window, opening, &tree, keep);
    paint(window, |app| app.global::<Ui>().invoke_heard_live(FOLLOW.into()));
}

/// A árvore na tela. Fica separada do pedido porque o que já está em mãos é desenhado antes
/// dele — e o mesmo desenho serve às duas horas.
fn paint_tree(window: &Weak<AppWindow>, opening: &Opening, tree: &ServerTree, keep: bool) {
    let (channels, reading, known, me) =
        (&opening.channels, &opening.reading, &opening.known, opening.me);
    let ordered = tree.ordered_channels();
    let groups = member_groups(tree, &lock(&opening.online), me);
    let kept = if keep {
        lock(reading).clone().and_then(|id| ordered.iter().position(|channel| channel.id == id))
    } else {
        None
    };

    let listed = ordered.clone();

    *lock(channels) = ordered;

    // Na releitura, o canal lido que sumiu (apagado, ou escondido de você) fecha.
    if kept.is_none() {
        *lock(reading) = None;
    }

    let chosen = lock(known).iter().position(|server| server.id == tree.id);
    let servers = rows_of(&lock(known), chosen, me);
    let name = tree.name.clone();
    let invite = tree.invite_code.clone().unwrap_or_default();
    let topic = kept.and_then(|index| listed.get(index)).and_then(|channel| channel.topic.clone()).unwrap_or_default();
    let abilities = tree.abilities();
    let can = |flag: &str| abilities.can.contains(&flag);
    let flags = (
        abilities.owner,
        can("manageServer"),
        can("manageChannels"),
        can("manageRoles"),
        can("createInvite"),
        can("banMembers"),
        can("viewAuditLog"),
    );
    let roles = role_rows(tree);

    // A voz guarda o que eu posso com cada pessoa: a lista de quem está no canal desenha
    // o menu por aqui, sem a árvore na mão.
    {
        let mut voice = lock(&opening.voice);

        voice.moderation = tree.members.iter().map(|member| (member.user_id, tree.member_actions(member))).collect();
        voice.server_muted = tree.members.iter().filter(|member| member.server_mute).map(|member| member.user_id).collect();
    }

    let tree = tree.clone();

    paint(window, move |app| {
        let ui = app.global::<Ui>();
        let (text, voice) = split_channels(&listed, kept, Some(&tree), me);

        ui.set_server_name(name.into());
        ui.set_servers(model(servers));
        ui.set_text_channels(model(text));
        ui.set_voice_channels(model(voice));
        ui.set_member_groups(model_of_groups(groups));
        ui.set_invite_code(invite.into());
        ui.set_channel_topic(topic.into());
        ui.set_roles(model(roles));
        ui.set_is_owner(flags.0);
        ui.set_can_manage_server(flags.1);
        ui.set_can_manage_channels(flags.2);
        ui.set_can_manage_roles(flags.3);
        ui.set_can_invite(flags.4);
        ui.set_can_ban(flags.5);
        ui.set_can_audit(flags.6);
        ui.set_in_server(true);

        if kept.is_none() {
            ui.set_messages(ModelRc::default());
            ui.set_channel_name(SharedString::new());
        }
    });
}

/// Os cargos do mais alto para o mais baixo, com o que o núcleo diz que dá para mexer.
fn role_rows(tree: &ServerTree) -> Vec<RoleRow> {
    let editable: HashMap<i64, bool> = tree.role_rows().into_iter().map(|row| (row.id, row.editable)).collect();
    let mut roles: Vec<&core_app::models::Role> = tree.roles.iter().collect();

    roles.sort_by_key(|role| std::cmp::Reverse(role.position));

    roles
        .into_iter()
        .map(|role| {
            let color = role.color.as_deref().and_then(color_of);

            RoleRow {
                id: i32::try_from(role.id).unwrap_or_default(),
                name: role.name.clone().into(),
                colored: color.is_some(),
                color: color.unwrap_or_default(),
                everyone: role.is_everyone,
                editable: editable.get(&role.id).copied().unwrap_or(false),
            }
        })
        .collect()
}

/// Os bits do cargo escolhido (pela posição na lista, do mais alto para o mais baixo).
fn paint_role_permissions(window: &Weak<AppWindow>, tree: &ServerTree, index: i32) {
    let mut roles: Vec<&core_app::models::Role> = tree.roles.iter().collect();

    roles.sort_by_key(|role| std::cmp::Reverse(role.position));

    let rows: Vec<PermissionRow> = usize::try_from(index)
        .ok()
        .and_then(|index| roles.get(index))
        .map(|role| {
            PERMISSIONS
                .iter()
                .map(|(bit, label)| PermissionRow {
                    bit: i32::try_from(*bit).unwrap_or_default(),
                    label: (*label).into(),
                    on: role.permissions & bit != 0,
                })
                .collect()
        })
        .unwrap_or_default();

    paint(window, move |app| {
        let ui = app.global::<Ui>();

        ui.set_selected_role(index);
        ui.set_role_permissions(model(rows));
    });
}

/// O que eu posso com uma pessoa, na linha que a tela lê.
fn actions_row(actions: Option<MemberActions>, server_muted: bool) -> ActionsRow {
    let actions = actions.unwrap_or_default();

    ActionsRow {
        mute: actions.mute,
        disconnect: actions.disconnect,
        kick: actions.kick,
        ban: actions.ban,
        server_muted,
    }
}

/// Um grupo de membros ainda sem modelo: o `ModelRc` só nasce na thread da janela.
struct MemberLines {
    label: String,
    color: Option<slint::Color>,
    offline: bool,
    members: Vec<MemberRow>,
}

/// A lista de membros nos grupos do React, decididos pelo núcleo.
fn member_groups(tree: &ServerTree, online: &HashSet<i64>, me: Option<i64>) -> Vec<MemberLines> {
    core_app::members::group(tree, online)
        .into_iter()
        .map(|group| {
            let members = group
                .members
                .iter()
                .map(|member| {
                    let name = core_app::members::display_name(member);
                    let voice = tree
                        .voice
                        .iter()
                        .find(|(_, people)| people.iter().any(|person| person.user_id == member.user_id))
                        .and_then(|(channel, people)| {
                            let name = tree.channels.iter().find(|known| known.id == *channel)?.name.clone();
                            let live = people.iter().any(|person| person.user_id == member.user_id && person.sources.iter().any(|source| source == "screen"));

                            Some(if live { "Transmitindo".to_owned() } else { format!("Na voz: {name}") })
                        });

                    MemberRow {
                        user_id: i32::try_from(member.user_id).unwrap_or_default(),
                        initial: initial(name),
                        name: name.into(),
                        owner: member.is_owner,
                        mine: Some(member.user_id) == me,
                        note: voice.unwrap_or_default().into(),
                        actions: actions_row(Some(tree.member_actions(member)), member.server_mute),
                    }
                })
                .collect();

            MemberLines {
                color: group.color.as_deref().and_then(color_of),
                offline: group.key == core_app::members::OFFLINE_KEY,
                label: group.label,
                members,
            }
        })
        .collect()
}

fn model_of_groups(groups: Vec<MemberLines>) -> ModelRc<MemberGroupRow> {
    model(
        groups
            .into_iter()
            .map(|group| MemberGroupRow {
                label: group.label.into(),
                color: group.color.unwrap_or_default(),
                colored: group.color.is_some(),
                offline: group.offline,
                members: model(group.members),
            })
            .collect(),
    )
}

/// A cor de um cargo, que o Laravel manda como `#rrggbb`.
fn color_of(hex: &str) -> Option<slint::Color> {
    let digits = u32::from_str_radix(hex.strip_prefix('#')?, 16).ok()?;
    let [_, red, green, blue] = digits.to_be_bytes();

    (hex.len() == 7).then(|| slint::Color::from_rgb_u8(red, green, blue))
}

fn split_channels(
    ordered: &[Channel],
    chosen: Option<usize>,
    tree: Option<&ServerTree>,
    me: Option<i64>,
) -> (Vec<ChannelRow>, Vec<ChannelRow>) {
    let mut text = Vec::new();
    let mut voice = Vec::new();
    let empty = HashMap::<String, Vec<VoicePerson>>::new();
    let people = tree.map_or(&empty, |tree| &tree.voice);
    let actions_of = |user: i64| -> ActionsRow {
        let member = tree.and_then(|tree| tree.members.iter().find(|member| member.user_id == user));

        actions_row(
            member.and_then(|member| tree.map(|tree| tree.member_actions(member))),
            member.is_some_and(|member| member.server_mute),
        )
    };

    for (index, channel) in ordered.iter().enumerate() {
        let inside: Vec<VoicePersonRow> = people
            .get(&channel.id)
            .into_iter()
            .flatten()
            .map(|person| VoicePersonRow {
                user_id: i32::try_from(person.user_id).unwrap_or_default(),
                initial: initial(&person.name),
                name: person.name.clone().into(),
                mine: Some(person.user_id) == me,
                muted: person.muted,
                camera: person.sources.iter().any(|source| source == "camera"),
                live: person.sources.iter().any(|source| source == "screen"),
                actions: actions_of(person.user_id),
            })
            .collect();
        let row = ChannelRow {
            index: index as i32,
            id: channel.id.clone().into(),
            name: channel.name.clone().into(),
            topic: channel.topic.clone().unwrap_or_default().into(),
            limit: i32::try_from(channel.user_limit.unwrap_or(0)).unwrap_or(0),
            voice: channel.kind == ChannelKind::Voice,
            current: Some(index) == chosen,
            people: model(inside),
            can_move_from: permissions::has(channel.permissions, permissions::MOVE_MEMBERS),
            can_move_here: permissions::has(channel.permissions, permissions::MOVE_MEMBERS | permissions::CONNECT),
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
    input_mode: InputMode,
) {
    let (can_speak, muted_at_rest) = {
        let voice = lock(&voice);

        // "Silenciar ao entrar" vale como o mudo guardado: o microfone abre, mas calado.
        (voice.mine.can_speak, voice.muted_at_rest || voice.mute_on_join)
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
            room.set_input_mode(input_mode);

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
                    let percent = data["percent"].as_u64().map_or(0, |percent| u8::try_from(percent).unwrap_or(u8::MAX));
                    let changed = {
                        let mut voice = lock(&voice);
                        let before = voice.speaking_myself();

                        voice.hear_myself(percent, std::time::Instant::now());
                        before != voice.speaking_myself()
                    };

                    // O medidor da sensibilidade, só enquanto as configurações estão abertas.
                    let level = f32::from(percent) / 100.0;

                    paint(&window, move |app| {
                        let ui = app.global::<Ui>();

                        if ui.get_settings_open() {
                            ui.set_mic_level(level);
                        }
                    });

                    if changed {
                        paint_voice(&window, &voice);
                    }
                }
                "room.ping" => {
                    if let Some(milliseconds) = data["ms"].as_u64() {
                        let said = format!("{milliseconds} ms");
                        let measured = i32::try_from(milliseconds).unwrap_or(i32::MAX);
                        let bars = i32::from(data["bars"].as_u64().and_then(|bars| u8::try_from(bars).ok()).unwrap_or_else(|| core_app::room::signal_bars(milliseconds)));

                        paint(&window, move |app| {
                            let ui = app.global::<Ui>();

                            ui.set_ping(said.into());
                            ui.set_ping_ms(measured);
                            ui.set_ping_bars(bars);
                        });
                    }
                }
                "room.session" => match data["state"].as_str().unwrap_or_default() {
                    "lost" => paint(&window, |app| {
                        let ui = app.global::<Ui>();

                        ui.set_reconnecting(true);

                        if ui.get_voice_channel() != "" {
                            ui.set_voice_state("reconnecting".into());
                        }
                    }),
                    "rejoined" => {
                        paint(&window, |app| {
                            let ui = app.global::<Ui>();

                            ui.set_reconnecting(false);

                            if ui.get_voice_channel() != "" {
                                ui.set_voice_state("connected".into());
                            }
                        });
                        complain(&window, "");
                    }
                    "moved" => {
                        let line = serde_json::json!({ "event": "room.moved", "data": data }).to_string();

                        paint(&window, move |app| app.global::<Ui>().invoke_heard_live(line.into()));
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
                // O toque e o aviso da sala vão para a ponte, que sabe a saída e os avisos.
                "room.chime" | "room.notice" => {
                    paint(&window, move |app| app.global::<Ui>().invoke_heard_live(said.into()));
                }
                _ => {}
            }
        }
    });

    if let Err(failure) = spawned {
        tracing::warn!(%failure, "a thread dos avisos da sala não subiu");
    }
}

/// Um tique que sobrevive ao arraste da janela. Arrastar prende o laço de eventos do
/// Windows num laço modal do sistema, e o `slint::Timer` para ali — o relógio da sala, a
/// linha de números e quem assiste congelavam. A fila de eventos, não: ela continua sendo
/// despachada, então o tempo é contado numa thread e o trabalho entra por ela. A thread
/// acaba quando o laço de eventos acaba.
fn every(period: std::time::Duration, work: impl Fn() + Send + Sync + 'static) {
    let work = Arc::new(work);

    std::thread::spawn(move || {
        loop {
            std::thread::sleep(period);

            let work = Arc::clone(&work);

            if slint::invoke_from_event_loop(move || work()).is_err() {
                break;
            }
        }
    });
}

/// Leva o quadro mais novo de cada tela para o cartão dela. Roda na thread da janela.
fn draw_fresh(window: &Weak<AppWindow>, watch: &Arc<Mutex<Option<Watch>>>) {
    let fresh = match lock(watch).as_ref() {
        Some(watch) => watch.fresh(),
        None => return,
    };

    let Some(app) = window.upgrade() else {
        return;
    };
    let tiles = app.global::<Ui>().get_tiles();

    for (producer, buffer) in fresh {
        let found = (0..tiles.row_count())
            .find_map(|index| tiles.row_data(index).filter(|row| row.producer == producer).map(|row| (index, row)));

        if let Some((index, mut row)) = found {
            row.frame = Image::from_rgba8(buffer);
            row.has_frame = true;
            tiles.set_row_data(index, row);
        }
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
    let (placed, focusing, full, pending) = {
        let stage = lock(stage);

        (stage.placed(), stage.focusing(), stage.full_screen(), stage.pending())
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
                    volume: i32::from(placed.volume),
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

        ui.set_focusing(focusing);
        ui.set_full_screen(full);
        ui.set_pending_tiles(i32::try_from(pending).unwrap_or_default());
    });
}

fn peer_rows(voice: &Voice) -> Vec<PeerRow> {
    voice
        .peers
        .iter()
        .map(|peer| {
            let user = Voice::user_of(peer);

            PeerRow {
            user_id: user.and_then(|user| i32::try_from(user).ok()).unwrap_or_default(),
            camera: peer.producers.iter().any(|producer| producer.source == "camera"),
            local_muted: user.is_some_and(|user| voice.muted_people.contains(&user)),
            actions: actions_row(
                user.and_then(|user| voice.moderation.get(&user).copied()).filter(|_| !peer.self_peer),
                user.is_some_and(|user| voice.server_muted.contains(&user)),
            ),
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
            sharing: peer.sharing(),
            reconnecting: peer.reconnecting,
            speaking: voice.is_speaking(peer),
            muted: if peer.self_peer {
                voice.mic_shown_off()
            } else {
                !peer.producers.iter().any(|producer| producer.source == "mic" && !producer.paused)
            },
            }
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

/// O que deu erro no log vai ao site de meio em meio minuto, enquanto o app estiver aberto: o
/// problema de quem usa chega a quem conserta sem ninguém pedir arquivo. O pedaço só sai do
/// pendente quando o site confirma, então um envio que falhou vai de novo na volta seguinte.
#[cfg(target_os = "windows")]
async fn report_errors(api: &Api) {
    let folder = crate::clips::shell::local_folder();

    loop {
        if let Some(pending) = crate::logbook::unreported(&folder)
            && api.report_error(env!("CARGO_PKG_VERSION"), std::env::consts::OS, &pending.log).await
        {
            pending.sent();
        }

        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

/// Instalado pela Microsoft Store, quem atualiza é a Store: o app não procura versão no site,
/// nem baixa o instalador do site por cima do pacote.
fn updated_by_the_store() -> bool {
    #[cfg(target_os = "windows")]
    return crate::clips::shell::packaged();

    #[cfg(not(target_os = "windows"))]
    false
}

/// Há versão nova? Baixa com a barra na tela, confere a assinatura e entrega ao instalador,
/// que troca o app e o abre de novo — como fazia o atualizador do Tauri. `true` quando o app
/// está de saída. Falhou em qualquer ponto, abre na versão que tem: atualizar nunca impede de
/// usar.
///
/// ponytail: só na abertura; o React procura também de seis em seis horas. Vale trazer
/// quando alguém passar dias com o app aberto sem sala.
async fn updating(api: &Api, window: &Weak<AppWindow>) -> bool {
    if updated_by_the_store() {
        return false;
    }

    show(window, Screen::Updating, "Procurando atualizações…".to_owned());

    let Some(release) = api.newer_release(core_app::update::PLATFORM).await else {
        return false;
    };
    let mut said = String::new();
    let installer = core_app::update::fetch(api, &release, |downloaded, total| {
        let percent = total
            .filter(|total| *total > 0)
            .map(|total| (downloaded * 100 / total).min(100));
        let status = match percent {
            Some(percent) => format!("Baixando a atualização… {percent}%"),
            None => format!("Baixando a atualização… {:.1} MB", downloaded as f64 / 1024.0 / 1024.0),
        };

        // Um pedaço chega a cada poucos KB: pintar só quando o texto muda.
        if status != said {
            said.clone_from(&status);
            paint(window, move |app| {
                let ui = app.global::<Ui>();

                ui.set_status(status.into());
                ui.set_update_progress(percent.map_or(-1.0, |percent| percent as f32));
            });
        }
    })
    .await;

    paint(window, |app| app.global::<Ui>().set_update_progress(-1.0));

    let Some(installer) = installer else {
        return false;
    };

    show(window, Screen::Updating, format!("Instalando a versão {}…", release.version));

    clips_saved().await;

    // Na abertura, escondida quando veio do logon: a versão nova volta do mesmo jeito.
    install(&installer, std::env::args().any(|argument| argument == "--background"))
}

/// Um replay sendo gravado no disco morreria no meio junto com o app, e o MP4 ficaria
/// quebrado: a troca de versão espera ele terminar.
async fn clips_saved() {
    #[cfg(target_os = "windows")]
    while crate::clips::saving() {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

/// Baixa calada a versão nova e, conferida a assinatura, mostra o botão verde na barra: quem
/// escolhe a hora de reiniciar é a pessoa, como no Discord — ninguém cai da sala porque saiu
/// uma versão. Quem chama é o aviso do servidor pelo tempo real, e não um relógio: o app só
/// pergunta ao site quando há o que perguntar.
async fn prepare_update(api: &Api, window: &Weak<AppWindow>, ready: &Arc<Mutex<Option<(PathBuf, String)>>>) {
    if updated_by_the_store() {
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

    paint(window, move |app| {
        let ui = app.global::<Ui>();

        ui.set_update_version(version.into());
        ui.set_update_ready(true);
    });
}

/// Abre o instalador e sai, como o atualizador do Tauri: `/P` sem perguntas, `/UPDATE` é
/// troca e não instalação nova, `/R` reabre o app no fim. Numa conta de administrador o app já
/// roda elevado (os Clips precisam), e o instalador herda o nível sem UAC; numa conta comum o
/// Windows pede a senha do administrador, e recusar só deixa esta versão. Com o app escondido
/// na bandeja (`hidden`), a troca é toda silenciosa (`/S`, nem a barra de progresso aparece,
/// que podia subir por cima de um jogo) e ele volta do mesmo jeito (`/BACKGROUND`).
#[cfg(target_os = "windows")]
fn install(installer: &std::path::Path, hidden: bool) -> bool {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR, w};

    let file = HSTRING::from(installer.as_os_str());
    let parameters = if hidden {
        w!("/S /UPDATE /R /BACKGROUND")
    } else {
        w!("/P /UPDATE /R")
    };
    let opened = unsafe { ShellExecuteW(None, w!("open"), &file, parameters, PCWSTR::null(), SW_SHOWNORMAL) };

    // Acima de 32 é sucesso: é assim que o ShellExecute responde desde sempre.
    if opened.0 as isize <= 32 {
        tracing::warn!(code = opened.0 as isize, "atualização: o instalador não abriu");

        return false;
    }

    std::process::exit(0);
}

#[cfg(not(target_os = "windows"))]
fn install(_installer: &std::path::Path, _hidden: bool) -> bool {
    false
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

/// O site que o app usa: o de produção, ou o de `UNKVOID_SERVER` para a pilha local.
fn server() -> String {
    std::env::var("UNKVOID_SERVER").unwrap_or_else(|_| DEFAULT_SERVER.to_owned())
}

/// Com a conta aberta: a tela do hub e o tempo real da conta no lugar do sem conta, que só
/// ouvia o canal das versões.
async fn arrive(
    core: &Arc<App>,
    api: &Arc<Api>,
    window: &Weak<AppWindow>,
    landing: &Landing,
    sfu: &Arc<Mutex<Option<String>>>,
    live: &Arc<Mutex<Option<Arc<Realtime>>>>,
    user: Option<User>,
) {
    let account = user.as_ref().map(|user| user.id);

    landed(core, api, window, landing, user).await;

    if account.is_some() {
        if let Some(guest) = lock(live).take() {
            guest.close();
        }

        go_live(api, sfu, live, window, account).await;
    }
}

/// Abre um endereço no navegador da pessoa sem o administrador do app. Quem abre é o Explorer
/// da área de trabalho, que roda sem elevação, a pedido do app pela automação do shell
/// (`IShellDispatch2::ShellExecute`). Abrir direto daria um navegador elevado, que briga com o
/// perfil do navegador já aberto; e chamar o `explorer.exe` com o endereço, com o app elevado,
/// abria o gerenciador de arquivos no lugar do navegador.
#[cfg(target_os = "windows")]
fn open_in_browser(url: &str) {
    let url = url.to_owned();
    let spawned = std::thread::Builder::new().name("navegador".into()).spawn(move || {
        if let Err(failure) = open_through_desktop(&url) {
            tracing::warn!(%failure, "navegador: a área de trabalho não abriu o endereço, abrindo direto");
            open_directly(&url);
        }
    });

    if let Err(failure) = spawned {
        tracing::warn!(%failure, "navegador: a thread não subiu");
    }
}

#[cfg(not(target_os = "windows"))]
fn open_in_browser(_url: &str) {}

/// O COM desta thread em volta do pedido ao shell da área de trabalho.
#[cfg(target_os = "windows")]
fn open_through_desktop(url: &str) -> windows::core::Result<()> {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};

    // SAFETY: COM de apartamento único nesta thread, só dela, e desfeito antes de ela acabar.
    unsafe {
        let started = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let opened = desktop_shell_execute(url);

        if started.is_ok() {
            CoUninitialize();
        }

        opened
    }
}

/// O caminho do shell da área de trabalho: a janela do desktop, o navegador de pastas dela, a
/// vista e, por fim, o objeto de automação do Explorer, que executa como o próprio Explorer.
#[cfg(target_os = "windows")]
fn desktop_shell_execute(url: &str) -> windows::core::Result<()> {
    use windows::Win32::System::Com::{CLSCTX_LOCAL_SERVER, CoCreateInstance, IDispatch, IServiceProvider};
    use windows::Win32::System::Variant::{VARIANT, VT_I4};
    use windows::Win32::UI::Shell::{
        IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellView, IShellWindows, SID_STopLevelBrowser,
        SVGIO_BACKGROUND, SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellWindows,
    };
    use windows::core::{BSTR, Interface};

    // SAFETY: chamadas COM com o COM já aberto nesta thread; o `VARIANT` do desktop é o
    // `CSIDL_DESKTOP` (zero) marcado como inteiro, com o resto zerado pelo `default`.
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
        let mut desktop = VARIANT::default();

        (*desktop.Anonymous.Anonymous).vt = VT_I4;

        let mut window = 0;
        let found = windows.FindWindowSW(&desktop, &VARIANT::default(), SWC_DESKTOP, &mut window, SWFO_NEEDDISPATCH)?;
        let browser: IShellBrowser = found.cast::<IServiceProvider>()?.QueryService(&SID_STopLevelBrowser)?;
        let view: IShellView = browser.QueryActiveShellView()?;
        let background: IDispatch = view.GetItemObject(SVGIO_BACKGROUND)?;
        let shell: IShellDispatch2 = background.cast::<IShellFolderViewDual>()?.Application()?.cast()?;

        shell.ShellExecute(&BSTR::from(url), &VARIANT::default(), &VARIANT::default(), &VARIANT::default(), &VARIANT::default())
    }
}

/// O último recurso: o navegador sai com o nível do app, mas o login não fica sem navegador.
#[cfg(target_os = "windows")]
fn open_directly(url: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR, w};

    // SAFETY: as duas cadeias vivem até o fim da chamada, que não guarda nenhuma delas.
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(url), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// A sala que está a caminho do servidor. Só a última pedida vale: a de antes, quando chega,
/// se despede sozinha.
#[derive(Debug, Default)]
struct Entering(Option<String>);

impl Entering {
    /// Falso quando esta sala já está a caminho: o segundo pedido não abre outra sessão.
    fn begin(&mut self, room: &str) -> bool {
        if self.0.as_deref() == Some(room) {
            return false;
        }

        self.0 = Some(room.to_owned());

        true
    }

    /// Verdadeiro quando a entrada que chegou ainda é a pedida.
    fn finish(&mut self, room: &str) -> bool {
        let wanted = self.0.as_deref() == Some(room);

        if wanted {
            self.0 = None;
        }

        wanted
    }

    fn cancel(&mut self) {
        self.0 = None;
    }
}

/// O sistema e o nome da máquina, como aparecem na lista de sessões da conta.
fn device_name() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|host| host.trim().to_owned())
        .filter(|host| !host.is_empty());

    match host {
        Some(host) => format!("{}-{host}", std::env::consts::OS),
        None => std::env::consts::OS.to_owned(),
    }
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
    fn a_second_request_for_the_room_on_its_way_opens_no_second_session() {
        let mut entering = Entering::default();

        assert!(entering.begin("mg6gag7qik00"));
        assert!(!entering.begin("mg6gag7qik00"), "o duplo clique não abre outra sessão");
        assert!(entering.finish("mg6gag7qik00"));
        assert!(entering.begin("mg6gag7qik00"), "depois de chegar, pedir de novo volta a valer");

        assert!(entering.begin("a593mzl95t6p"));
        assert!(!entering.finish("mg6gag7qik00"), "a sala trocada no caminho se despede ao chegar");
        assert!(entering.finish("a593mzl95t6p"));

        assert!(entering.begin("m4nj0b8eo7qk"));
        entering.cancel();
        assert!(!entering.finish("m4nj0b8eo7qk"), "quem saiu antes de a sala chegar não entra nela");
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

    /// Abre uma aba de verdade, pelo Explorer da área de trabalho: é o caminho do login com o
    /// Google. `cargo test -p unkvoid-windows -- --ignored desktop_opens`.
    #[test]
    #[ignore]
    #[cfg(target_os = "windows")]
    fn the_desktop_opens_a_page_in_the_browser() {
        open_through_desktop("https://unkvoid.com").expect("o Explorer da área de trabalho abriu o endereço");
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
