//! O estado que a janela desenha e o que cada clique faz com ele.
//!
//! A tela (`ui/`) só desenha e anota [`Action`]s; elas são aplicadas aqui, depois do quadro,
//! com o estado livre — é o molde do Spotifast, e é o que deixa o desenho percorrer a árvore do
//! servidor sem brigar com quem a muda. O que espera a rede vai para o [`Backend`] e volta como
//! [`Event`].

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use core_app::api::HttpError;
use core_app::activity::Activity;
use core_app::models::{
    AuditEntry, Channel, ChannelKind, Conversation, DirectMessage, Friendship, Message, Mutuals,
    OverwriteTarget, Person, ServerInvite, ServerSound, ServerSummary, ServerTree, User, UserStatus,
};
use core_app::realtime::{self, Reading};

use crate::backend::{Backend, Event, Refresh};
use crate::voice::{CallView, Seat};
use crate::words::said;

/// Quantas mensagens o Laravel manda por página: menos que isso é o começo do canal.
const PAGE: usize = 50;

/// Os tempos do React: um aviso some em 3,5 s, um erro em 6 s.
const NOTICE_LIFE: Duration = Duration::from_millis(3_500);
const ERROR_LIFE: Duration = Duration::from_millis(6_000);

/// Em que pé está a janela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Starting(String),
    Offline,
    SignIn,
    Hub,
    /// Na sala por código, sem conta: o palco ocupa a janela, e sair volta para a entrada.
    Room,
}

/// O que o centro mostra quando nenhum servidor está aberto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Home {
    Friends(FriendsTab),
    Direct(Person),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FriendsTab {
    Online,
    All,
    Pending,
    Add,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modal {
    AddServer(AddServerStep),
    CreateChannel {
        server: i64,
        kind: ChannelKind,
        name: String,
        parent: Option<String>,
    },
    Invite {
        server: i64,
        code: Option<String>,
    },
    LeaveServer {
        server: i64,
        name: String,
    },
    Settings(SettingsPage),
    ServerSettings(ServerSettings),
    ChannelSettings(ChannelSettings),
    VoiceDebug(crate::ui::voice_debug::DebugPage),
    MemberProfile {
        server: i64,
        user: i64,
    },
    ServerSound {
        server: i64,
        path: PathBuf,
        name: String,
        emoji: String,
    },
    ProfileEditor(ProfileDraft),
    /// O "Compartilhe sua tela": as janelas e as telas para escolher, e o modo da transmissão.
    GoLive(GoLiveTab),
    /// O "Vamos ligar a câmera?", com a própria câmera na prévia antes de entrar na chamada.
    CameraPreview,
    /// A sala por código para quem tem conta: a mesma da entrada, com o nome da conta.
    CodeRoom,
}

/// As abas do seletor da transmissão, como no Discord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoLiveTab {
    Applications,
    Screens,
}

/// Uma janela ou tela que dá para transmitir. `key` é o `window:<id>` ou `display:<id>` que o
/// `core_app::sharing::capture_config` entende.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareSource {
    pub key: String,
    pub name: String,
    pub screen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddServerStep {
    Choose,
    Customize {
        name: String,
        icon: Option<ServerIconChoice>,
    },
    Join {
        invite: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerIconChoice {
    pub path: PathBuf,
    pub bytes: std::sync::Arc<[u8]>,
}

/// As configurações de um servidor: a página aberta e o que está sendo editado nela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSettings {
    pub server: i64,
    pub page: ServerPage,
    /// O nome como está no campo: difere do servidor enquanto não for salvo.
    pub name: String,
    /// O cargo aberto no editor, com o que já foi mexido nele.
    pub role: Option<RoleDraft>,
    /// O filtro da página de membros.
    pub search: String,
    /// "Criar cargo": o maior id de cargo no clique. O primeiro acima dele abre no editor
    /// quando a árvore voltar com ele, como no Discord.
    pub new_role_after: Option<i64>,
}

impl ServerSettings {
    pub fn new(tree: &ServerTree, page: ServerPage) -> Self {
        Self {
            server: tree.id,
            page,
            name: tree.name.clone(),
            role: None,
            search: String::new(),
            new_role_after: None,
        }
    }
}

/// As páginas das configurações do servidor, na ordem do menu da esquerda.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerPage {
    Overview,
    Soundboard,
    Roles,
    Members,
    Invites,
    Bans,
    Audit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleDraft {
    pub id: i64,
    pub tab: RoleTab,
    pub name: String,
    pub color: Option<String>,
    pub permissions: i64,
    pub hoist: bool,
    pub mentionable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleTab {
    Display,
    Permissions,
    Members,
}

/// As configurações de um canal ou de uma categoria.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelSettings {
    pub server: i64,
    pub channel: String,
    pub page: ChannelPage,
    pub name: String,
    pub topic: String,
    /// O limite de pessoas na voz; 0 é sem limite.
    pub user_limit: u32,
    /// A sobrescrita aberta na página de permissões, com o que já foi mexido nela.
    pub overwrite: Option<OverwriteDraft>,
}

impl ChannelSettings {
    pub fn new(server: i64, channel: &Channel) -> Self {
        Self {
            server,
            channel: channel.id.clone(),
            page: ChannelPage::Overview,
            name: channel.name.clone(),
            topic: channel.topic.clone().unwrap_or_default(),
            user_limit: channel
                .user_limit
                .and_then(|limit| u32::try_from(limit).ok())
                .unwrap_or(0),
            overwrite: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPage {
    Overview,
    Permissions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverwriteDraft {
    pub target: OverwriteTarget,
    pub id: i64,
    pub allow: i64,
    pub deny: i64,
}

/// A pergunta que aparece por cima de tudo antes de uma ação que não se desfaz com um clique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    Kick {
        server: i64,
        user: i64,
        name: String,
    },
    Ban {
        server: i64,
        user: i64,
        name: String,
        reason: String,
    },
    Nickname {
        server: i64,
        user: i64,
        name: String,
        value: String,
    },
    DeleteServer {
        server: i64,
        name: String,
        typed: String,
    },
    DeleteChannel {
        server: i64,
        channel: String,
        name: String,
    },
    DeleteRole {
        server: i64,
        role: i64,
        name: String,
    },
    Unfriend {
        friendship: i64,
        name: String,
    },
    Block {
        friendship: i64,
        name: String,
    },
}

/// As seções das configurações do usuário, na ordem do menu da esquerda.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    Account,
    Clips,
    /// Voz e vídeo é uma página só, como no Discord; a seção é aonde ela abre rolada.
    Voice(VoiceSection),
}

/// As seções da página Voz e vídeo, na ordem em que aparecem e no menu da esquerda.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum VoiceSection {
    #[default]
    Voice,
    Video,
    Sounds,
    Soundboard,
    Advanced,
}

impl VoiceSection {
    pub const ALL: [Self; 5] = [
        Self::Voice,
        Self::Video,
        Self::Sounds,
        Self::Soundboard,
        Self::Advanced,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Voice => "Voz",
            Self::Video => "Vídeo",
            Self::Sounds => "Sons",
            Self::Soundboard => "Painel de efeitos sonoros",
            Self::Advanced => "Avançado",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDraft {
    pub bio: String,
    pub pronouns: String,
    pub banner_color: String,
    pub custom_status: String,
}

impl ProfileDraft {
    pub fn from_user(user: &User) -> Self {
        Self {
            bio: user.profile.bio.clone().unwrap_or_default(),
            pronouns: user.profile.pronouns.clone().unwrap_or_default(),
            banner_color: user.profile.banner_color.clone().unwrap_or_default(),
            custom_status: user.profile.custom_status.clone().unwrap_or_default(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Login {
    pub email: String,
    pub password: String,
    pub register: bool,
    pub busy: bool,
    pub email_error: Option<String>,
    pub password_error: Option<String>,
    pub error: Option<String>,
    /// Entrar com o nome e uma senha, do lado do login: a conta nasce com eles.
    pub starting: bool,
    pub start_error: Option<String>,
    pub start_password: String,
    /// O nome de quem entra com ele, e o da sala por código (o modal do hub): o código
    /// digitado e as últimas salas.
    pub guest_name: String,
    pub room_code: String,
    pub room_error: Option<String>,
    pub recent_rooms: Vec<String>,
}

/// O apelido da conta, na tela de configurações e no aviso que não fecha enquanto o apelido
/// automático não for confirmado.
#[derive(Debug, Default, Clone)]
pub struct Nickname {
    pub value: String,
    pub error: Option<String>,
    pub busy: bool,
}

pub struct Toast {
    pub id: u64,
    pub text: String,
    pub error: bool,
    pub action_path: Option<std::path::PathBuf>,
    pub until: Instant,
}

/// O e-mail que a conta que nasceu com o nome põe nas configurações.
#[derive(Debug, Default, Clone)]
pub struct AddEmail {
    pub email: String,
    pub error: Option<String>,
    pub busy: bool,
}

/// O que um clique pede. Aplicado depois do quadro, em [`Unkvoid::apply`].
#[derive(Debug, Clone)]
pub enum Action {
    SignIn,
    /// Entrar com o nome e a senha (`Login::guest_name`, `Login::start_password`): a conta
    /// nasce agora.
    Start,
    /// Pôr o e-mail de `Unkvoid::add_email` na conta que nasceu com o nome.
    AddEmail,
    GoogleSignIn,
    SignOut,
    Retry,
    OpenHome,
    OpenServer(i64),
    OpenChannel(Channel),
    MarkChannelRead(String),
    Mention(String),
    SetPeerVolume(i64, u16),
    TogglePeerMute(i64),
    ToggleCategory(String),
    LoadOlder,
    SendMessage,
    /// O "Acene para dizer olá!" de quem chegou: manda o aceno no canal aberto.
    Wave,
    StartEdit(i64, String),
    CancelEdit,
    SaveEdit,
    DeleteMessage(i64),
    OpenFriends(FriendsTab),
    OpenDirect(Person),
    HideConversation(i64),
    SendDirect,
    /// A mensagem escrita no cartão de perfil, sem abrir a conversa.
    SendDirectTo(Person, String),
    AddFriend,
    AddFriendById(i64),
    AnswerFriend(i64, bool),
    BlockFriend(i64),
    Show(Modal),
    CloseModal,
    CreateServer,
    JoinInvite,
    JoinInviteCode(String),
    LoadInvitePreview(String),
    OpenUrl(String),
    PickServerIcon,
    CreateChannel,
    CreateServerInvite { server: i64, max_age: i64, max_uses: i64 },
    SetInvitesPaused { server: i64, paused: bool },
    DeleteServerInvite { server: i64, invite: i64 },
    LeaveServer(i64),
    Rename,
    ToggleMembers,
    ToggleNoiseTest,
    ToggleMute,
    ToggleDeafen,
    JoinVoice(Channel),
    /// Entrar na sala por código, sem conta: a digitada (vazia cria uma) ou uma das últimas.
    EnterRoom(Option<String>),
    /// O início da sala por código: a entrada, com a sala ainda no ar.
    ShowEntry,
    /// Da entrada de volta para a sala por código em que a pessoa continua.
    BackToRoom,
    /// De qualquer tela de volta à chamada: a sala por código ou o canal de voz do servidor.
    BackToCall,
    LeaveVoice,
    ToggleVoiceChat,
    SetUserStatus(UserStatus, Option<i64>),
    SetActivitySharing(bool),
    SendVoiceLogs,
    PlaySoundboard(core_app::chimes::SoundboardSound),
    LoadServerSounds(i64),
    PickServerSound(i64),
    SaveServerSound,
    DeleteServerSound(i64, i64),
    PlayServerSound(ServerSound),
    InstallUpdate,
    Dismiss(u64),
    CopyText(String),
    /// Um campo de Voz e vídeo (`unkvoid:voice`) com o valor novo.
    SetVoice(&'static str, serde_json::Value),
    /// O "Testar vídeo" de Voz e vídeo: abre ou fecha a câmera só para a prévia.
    TestVideo(bool),
    /// O "+" da caixa de mensagem: escolher uma imagem do disco.
    AttachImage,
    /// O Ctrl+V com um print na área de transferência.
    PasteImage,
    RemoveAttachment(usize),
    /// Transmitir a janela ou a tela escolhida no seletor, com o modo guardado.
    StartShare(String),
    StopShare,
    /// O botão da câmera: liga (pela prévia, se a pessoa a quer) ou desliga.
    ToggleCamera,
    /// O "Ativar câmera" da prévia.
    StartCamera,
    /// Clicar num vídeo do palco: foco nele, ou sai do foco.
    FocusTile(String),
    /// O botão de tela cheia de uma transmissão, ou o de sair dela.
    FullTile(String),
    /// O "Assistir" de uma transmissão no ar, ou o "Parar de assistir".
    WatchStream(String, bool),
    /// Assistir à própria transmissão, ou parar.
    SelfView(bool),
    /// O alto-falante no cartão de uma tela: liga ou desliga o som dela.
    StreamSound(String),
    /// O volume do som de uma tela, de 0 a 100, só deste lado.
    StreamVolume(String, u8),
    SetSound(core_app::chimes::Chime, bool),
    PreviewSound(core_app::chimes::Chime),
    /// Uma escrita pelo mapa de rotas do núcleo (`routes.rs`), e o que reler depois dela.
    Request {
        route: &'static str,
        params: serde_json::Value,
        body: serde_json::Value,
        refresh: Refresh,
    },
    /// Escolher uma imagem no disco e mandar pela rota: a foto de perfil, o ícone do servidor.
    UploadImage {
        route: &'static str,
        params: serde_json::Value,
        field: &'static str,
        refresh: Refresh,
    },
    Ask(Confirm),
    CloseConfirm,
}

impl Action {
    pub fn request(
        route: &'static str,
        params: serde_json::Value,
        body: serde_json::Value,
        refresh: Refresh,
    ) -> Self {
        Self::Request {
            route,
            params,
            body,
            refresh,
        }
    }
}

pub struct Unkvoid {
    backend: Backend,
    inbox: Receiver<Event>,
    pub actions: Vec<Action>,
    pub phase: Phase,
    pub me: Option<User>,
    status_refresh_pending: bool,
    pub login: Login,
    pub servers: Vec<ServerSummary>,
    pub opened: Option<i64>,
    pub tree: Option<ServerTree>,
    pub reading: Option<Channel>,
    pub messages: Vec<Message>,
    /// O começo do canal já está na tela: não há o que buscar para cima.
    pub beginning: bool,
    pub loading_older: bool,
    /// A altura do chat e onde se estava quando a página anterior foi pedida, para a tela não
    /// pular quando ela chegar.
    pub scroll_anchor_pending: Option<(f32, f32)>,
    pub scroll_anchor: Option<(f32, f32)>,
    pub chat_height: Option<f32>,
    pub composer: String,
    /// As imagens que vão junto da próxima mensagem, com a miniatura de cada uma.
    pub attachments: Vec<(PathBuf, egui::TextureHandle)>,
    /// Só no beta: a página das configurações do servidor que o `UNKVOID_BETA_ABRIR` pediu,
    /// aberta quando a árvore do servidor chegar.
    environment_server_page: Option<ServerPage>,
    pub(super) account_popup_requested: bool,
    pub editing: Option<(i64, String)>,
    /// Quem está online no servidor aberto.
    pub online: HashSet<i64>,
    /// O último "online" de cada servidor já aberto: voltar a ele mostra a lista de antes até a
    /// presença nova chegar, em vez de todo mundo piscar para offline.
    online_by_server: HashMap<i64, HashSet<i64>>,
    /// As não lidas por canal: chegam na árvore e crescem com o tempo real.
    pub unread: HashMap<String, i64>,
    pub collapsed: HashSet<String>,
    pub show_members: bool,
    /// A janela estreita esconde a lista de membros; o botão dela ainda a abre, sem mexer na
    /// escolha guardada para a janela larga.
    pub members_narrow: bool,
    /// Se a lista de membros cabe ao lado do chat neste quadro (a interface atualiza).
    pub members_room: bool,
    pub home: Home,
    pub conversations: Vec<Conversation>,
    pub friends: Vec<Friendship>,
    pub current_activity: Option<Activity>,
    pub activity_sharing: bool,
    friends_refresh_at: Instant,
    pub direct_messages: Vec<DirectMessage>,
    pub direct_composer: String,
    pub direct_loading: bool,
    pub direct_mutuals: Mutuals,
    pub direct_profile_open: bool,
    pub quick_conversation_open: bool,
    pub quick_conversation_query: String,
    pub direct_search_open: bool,
    pub direct_search_query: String,
    pub hidden_conversations: HashSet<i64>,
    pub friend_email: String,
    pub friends_search: String,
    pub modal: Option<Modal>,
    /// A pergunta por cima do modal: expulsar, banir, apagar.
    pub confirm: Option<Confirm>,
    /// O registro de auditoria do servidor com as configurações abertas.
    pub audits: Vec<AuditEntry>,
    pub nickname: Nickname,
    pub toasts: Vec<Toast>,
    next_toast: u64,
    /// A versão nova pronta para instalar: o botão verde da barra.
    pub update: Option<String>,
    /// A voz como ela está agora: o canal, o mudo, o surdo e quem fala. Relida a cada quadro.
    pub call: CallView,
    /// Dentro da chamada, o centro mostra o chat do canal de voz em vez dos cartões.
    pub voice_chat_open: bool,
    /// Com uma transmissão em tela cheia: se a janela já estava em tela cheia antes, para
    /// voltar a ela ao sair.
    pub window_was_full: Option<bool>,
    /// Os Clips abertos no lugar do hub ou da sala, como a aba Clips do app de antes: o botão de
    /// filme da barra de cima abre e fecha.
    pub clips_open: bool,
    /// Os aparelhos de som do sistema, lidos quando a página de Voz e vídeo abre.
    pub microphones: Vec<crate::devices::Device>,
    pub speakers: Vec<crate::devices::Device>,
    pub cameras: Vec<capture::Camera>,
    /// A lista de câmeras já foi lida (vazia antes disso não quer dizer "nenhuma câmera").
    pub cameras_known: bool,
    /// O "Testar vídeo" de Voz e vídeo ligado: a câmera aberta só para a prévia.
    pub video_test: bool,
    /// O último quadro da própria câmera, para a prévia e para o cartão na chamada.
    pub camera_picture: Option<egui::TextureHandle>,
    camera_serial: u64,
    /// O último quadro de cada vídeo assistido, pelo producer.
    pub pictures: HashMap<String, egui::TextureHandle>,
    /// A linha de números de cada tela assistida (`1080p · 60 fps · 4,2 Mb/s`), e se a perda
    /// pede o vermelho.
    pub stream_stats: HashMap<String, (String, bool)>,
    /// Os contadores de pacotes da amostra anterior: a perda da linha é a do último segundo.
    stream_counters: HashMap<String, media::Counters>,
    stream_sampled_at: Instant,
    /// As janelas e telas do seletor da transmissão, e a miniatura de cada uma.
    pub share_sources: Option<Vec<ShareSource>>,
    pub share_previews: HashMap<String, egui::TextureHandle>,
    /// A janela ou tela marcada no seletor, que o "Transmitir" manda.
    pub share_selected: Option<String>,
    context: egui::Context,
    /// O que está guardado em Voz e vídeo e em Sons, relido a cada quadro (fica em memória no
    /// núcleo), e o nível do microfone para o medidor.
    pub voice_settings: serde_json::Value,
    pub sound_settings: serde_json::Value,
    #[cfg(target_os = "windows")]
    pub clips_runtime: crate::clips::ClipsRuntime,
    pub call_level: f32,
    pub input_menu_open: bool,
    /// Algum menu do microfone desenhado aberto neste quadro: são dois (o cartão da conta e a
    /// barra da chamada), e o estado só se decide no fim do quadro, senão um fecha o do outro.
    pub input_menu_seen: bool,
    pub noise_test_active: bool,
    pub noise_test_seen: bool,
    pub soundboard_search: String,
    pub soundboard_collapsed: HashSet<String>,
    pub soundboard_scroll_to: Option<String>,
    pub invite_search: String,
    pub invite_sent: HashSet<i64>,
    pub invite_previews: HashMap<String, (Instant, Result<core_app::models::InvitePreview, core_app::api::HttpError>)>,
    pub invite_preview_pending: HashSet<String>,
    pub server_invite: Option<ServerInvite>,
    pub server_invites: Vec<ServerInvite>,
    pub invite_max_age: i64,
    pub invite_max_uses: i64,
    pub invite_editing: bool,
    pub settings_search: String,
    pub email_visible: bool,
    pub editing_account_name: bool,
    pub add_email: AddEmail,
    pub server_sounds: Vec<ServerSound>,
    pub server_sound_cache: HashMap<i64, std::sync::Arc<[f32]>>,
    /// O texto que a janela leva à área de transferência no próximo quadro.
    copied: Option<String>,
}

impl Unkvoid {
    pub fn new(context: &egui::Context) -> anyhow::Result<Self> {
        let (backend, inbox) = Backend::new(context.clone())?;
        let activity_sharing = backend.activity_sharing_enabled();
        let hidden_conversations = backend.hidden_conversations();

        backend.start();

        let backend_call = backend.calls().view();

        Ok(Self {
            backend,
            inbox,
            actions: Vec::new(),
            phase: Phase::Starting("Abrindo…".into()),
            me: None,
            status_refresh_pending: false,
            login: Login::default(),
            servers: Vec::new(),
            opened: None,
            tree: None,
            reading: None,
            messages: Vec::new(),
            beginning: false,
            loading_older: false,
            scroll_anchor_pending: None,
            scroll_anchor: None,
            chat_height: None,
            composer: String::new(),
            editing: None,
            online: HashSet::new(),
            online_by_server: HashMap::new(),
            unread: HashMap::new(),
            collapsed: HashSet::new(),
            show_members: true,
            members_narrow: false,
            members_room: true,
            home: Home::Friends(FriendsTab::All),
            conversations: Vec::new(),
            friends: Vec::new(),
            current_activity: None,
            activity_sharing,
            friends_refresh_at: Instant::now() + Duration::from_secs(60),
            direct_messages: Vec::new(),
            direct_composer: String::new(),
            direct_loading: false,
            direct_mutuals: Mutuals::default(),
            direct_profile_open: true,
            quick_conversation_open: false,
            quick_conversation_query: String::new(),
            direct_search_open: false,
            direct_search_query: String::new(),
            hidden_conversations,
            friend_email: String::new(),
            friends_search: String::new(),
            modal: None,
            confirm: None,
            audits: Vec::new(),
            nickname: Nickname::default(),
            toasts: Vec::new(),
            next_toast: 0,
            update: None,
            call: backend_call,
            voice_chat_open: false,
            window_was_full: None,
            clips_open: false,
            microphones: Vec::new(),
            cameras: Vec::new(),
            cameras_known: false,
            video_test: false,
            camera_picture: None,
            camera_serial: 0,
            pictures: HashMap::new(),
            stream_stats: HashMap::new(),
            stream_counters: HashMap::new(),
            stream_sampled_at: Instant::now(),
            share_sources: None,
            share_previews: HashMap::new(),
            share_selected: None,
            attachments: Vec::new(),
            environment_server_page: None,
            account_popup_requested: false,
            context: context.clone(),
            speakers: Vec::new(),
            voice_settings: serde_json::Value::Null,
            sound_settings: serde_json::Value::Null,
            #[cfg(target_os = "windows")]
            clips_runtime: crate::clips::ClipsRuntime::new(context),
            call_level: 0.0,
            input_menu_open: false,
            input_menu_seen: false,
            noise_test_active: false,
            noise_test_seen: false,
            soundboard_search: String::new(),
            soundboard_collapsed: HashSet::new(),
            soundboard_scroll_to: None,
            invite_search: String::new(),
            invite_sent: HashSet::new(),
            invite_previews: HashMap::new(),
            invite_preview_pending: HashSet::new(),
            server_invite: None,
            server_invites: Vec::new(),
            invite_max_age: 604800,
            invite_max_uses: 0,
            invite_editing: false,
            settings_search: String::new(),
            email_visible: false,
            editing_account_name: false,
            add_email: AddEmail::default(),
            server_sounds: Vec::new(),
            server_sound_cache: HashMap::new(),
            copied: None,
        })
    }

    /// A janela foi para a bandeja: a chamada acaba junto, porque o microfone aberto sem janela
    /// nenhuma na tela seria pior que o replay parado, e o player para.
    #[cfg(target_os = "windows")]
    pub fn hidden(&mut self) {
        self.backend.calls().leave();
        self.clips_runtime.close_player();
    }

    /// O que chegou da rede desde o quadro anterior.
    pub fn receive(&mut self) {
        #[cfg(target_os = "windows")]
        for notice in self.clips_runtime.poll() {
            if notice == "Clipe salvo" && self.clips_runtime.settings.save_chime {
                self.backend.calls().preview(core_app::chimes::Chime::Joined);
            }

            if let Some(path) = self.clips_runtime.take_last_saved() {
                self.notify_with_action(notice, false, path);
            } else {
                self.notify(notice, false);
            }
        }

        // A Galeria do painel do Alt+Z: a janela volta da bandeja já nos Clips.
        #[cfg(target_os = "windows")]
        if self.clips_runtime.take_open_gallery() {
            self.clips_open = true;
        }

        while let Ok(event) = self.inbox.try_recv() {
            self.handle(event);
        }

        let previous_seat = self.call.seat.clone();
        let previous_sharing = self.call.mine.sharing;
        self.call = self.backend.calls().view();

        // A sala por código acabou (o desligar, ou a entrada que falhou): quem tem conta volta
        // para o hub, quem não tem volta para a entrada.
        if self.phase == Phase::Room && self.call.seat.is_none() {
            if self.me.is_some() {
                self.phase = Phase::Hub;
            } else {
                self.show_entry();
            }
        }

        if previous_seat.is_some()
            && previous_seat == self.call.seat
            && previous_sharing != self.call.mine.sharing
        {
            self.backend.calls().chime(if self.call.mine.sharing {
                core_app::chimes::Chime::StreamStarted
            } else {
                core_app::chimes::Chime::StreamStopped
            });
        }
        self.refresh_pictures();

        if self.me.is_some() && Instant::now() >= self.friends_refresh_at {
            self.backend.friends();
            self.friends_refresh_at = Instant::now() + Duration::from_secs(60);
        }

        if self.call.talking
            && let Some(me) = self.my_id()
        {
            self.call.speaking.insert(me);
        }

        // A escolha dos aparelhos aparece também na setinha do microfone, fora das configurações.
        self.voice_settings = self.backend.calls().settings();

        if matches!(self.modal, Some(Modal::Settings(_)))
            || self.input_menu_open
            || self.noise_test_active
        {
            self.sound_settings = self.backend.calls().sounds();
            self.call_level = self.backend.calls().level();
        }

        let now = Instant::now();

        self.toasts.retain(|toast| toast.until > now);

        if !self.status_refresh_pending
            && self.me.as_ref().and_then(|user| user.status_until.as_deref())
                .and_then(|until| chrono::DateTime::parse_from_rfc3339(until).ok())
                .is_some_and(|until| until <= chrono::Utc::now())
        {
            self.status_refresh_pending = true;
            self.backend.refresh_me();
        }
    }

    /// As texturas dos vídeos: a própria câmera quando chegou quadro novo, e o último quadro de
    /// cada vídeo assistido. A de um vídeo que saiu do palco sai junto.
    fn refresh_pictures(&mut self) {
        let (serial, picture) = self.backend.calls().camera().picture();

        if serial != self.camera_serial {
            self.camera_serial = serial;
            self.camera_picture = picture.map(|picture| {
                texture(&self.context, self.camera_picture.take(), "own-camera", &picture)
            });
        }

        for (producer, picture) in self.backend.calls().pictures() {
            let old = self.pictures.remove(&producer);
            let fresh = texture(&self.context, old, &producer, &picture);

            self.pictures.insert(producer, fresh);
        }

        let alive: HashSet<&str> = self
            .call
            .tiles
            .iter()
            .map(|placed| placed.tile.producer_id.as_str())
            .collect();

        self.pictures
            .retain(|producer, _| alive.contains(producer.as_str()));

        // De segundo em segundo, os números das telas. Divididos pelo tempo que passou de fato: a
        // janela só redesenha quando há evento, e a amostra pode cair depois do segundo.
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.stream_sampled_at).as_secs_f64();

        if elapsed >= 1.0 {
            let mut stats = HashMap::new();
            let mut counters = HashMap::new();

            #[allow(clippy::cast_precision_loss)]
            for (producer, (frames, height, bytes), current) in self.backend.calls().stream_numbers() {
                let fps = (f64::from(frames) / elapsed).round() as u32;
                let megabits = bytes as f64 * 8.0 / elapsed / 1_000_000.0;
                let previous = self.stream_counters.get(&producer).copied().unwrap_or_default();
                let current = current.unwrap_or_default();
                let received = current.received.saturating_sub(previous.received);
                let lost = current.lost.saturating_sub(previous.lost);
                let line = crate::stage::stats_line(fps, height, megabits, received, lost);

                stats.insert(producer.clone(), line);
                counters.insert(producer, current);
            }

            self.stream_stats = stats;
            self.stream_counters = counters;
            self.stream_sampled_at = now;
        }
    }

    /// A câmera escolhida em Voz e vídeo, ou a primeira.
    fn chosen_camera(&self) -> Option<u32> {
        let saved = self.voice_settings["camera"]
            .as_u64()
            .and_then(|index| u32::try_from(index).ok());

        saved
            .filter(|index| self.cameras.iter().any(|camera| camera.index == *index))
            .or_else(|| self.cameras.first().map(|camera| camera.index))
            .or(crate::camera::PREVIEWS.then_some(0))
    }

    fn open_camera_preview(&mut self) {
        let Some(index) = self.chosen_camera() else {
            return self.notify("Nenhuma câmera encontrada.", true);
        };

        if let Err(failure) = self.backend.calls().camera().open(index, &self.context) {
            tracing::warn!(%failure, "câmera: a prévia não abriu");
            self.notify("Não deu para abrir a câmera.", true);
        }
    }

    /// Fecha a webcam que só a prévia usava; a da chamada fica.
    fn close_camera_preview(&mut self) {
        self.video_test = false;

        if !self.call.mine.camera {
            self.backend.calls().camera().close();
        }
    }

    /// Quanto falta para o próximo aviso sumir: a janela acorda sozinha nessa hora.
    pub fn next_wakeup(&self) -> Option<Duration> {
        let now = Instant::now();
        let toast_wakeup = self
            .toasts
            .iter()
            .map(|toast| toast.until.saturating_duration_since(now))
            .min();

        let status_wakeup = (!self.status_refresh_pending).then(|| self.me.as_ref()
            .and_then(|user| user.status_until.as_deref())
            .and_then(|until| chrono::DateTime::parse_from_rfc3339(until).ok())
            .map(|until| {
                let milliseconds = (until.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_milliseconds().max(0) as u64;
                Duration::from_millis(milliseconds)
            })).flatten();
        let friends_wakeup = self
            .me
            .as_ref()
            .map(|_| self.friends_refresh_at.saturating_duration_since(now));
        let wakeup = [toast_wakeup, status_wakeup, friends_wakeup]
            .into_iter()
            .flatten()
            .min();

        if self.call.seat.is_some() {
            Some(wakeup.map_or(Duration::from_secs(1), |wakeup| {
                wakeup.min(Duration::from_secs(1))
            }))
        } else {
            wakeup
        }
    }

    pub fn server_summary(&self, server: i64) -> Option<&ServerSummary> {
        self.servers.iter().find(|summary| summary.id == server)
    }

    /// As não lidas de um servidor no trilho. O aberto não acende: o que há nele aparece na
    /// coluna de canais.
    pub fn unread_of(&self, server: &ServerSummary) -> i64 {
        if self.opened == Some(server.id) {
            0
        } else {
            server.unread
        }
    }

    /// Os pedidos de amizade que esperam resposta minha: o número no botão Amigos.
    pub fn pending_requests(&self) -> i64 {
        let me = self.my_id();

        self.friends
            .iter()
            .filter(|friend| {
                friend.status == core_app::models::FriendshipStatus::Pending
                    && Some(friend.addressee.id) == me
            })
            .count() as i64
    }

    /// Quem está num canal de voz: a árvore do Laravel e, no canal da chamada aberta, também a
    /// própria pessoa e os participantes que o SFU já conhece. O aviso de quem entra chega pelo
    /// tempo real, e um aviso perdido (o socket voltando, por exemplo) deixaria alguém fora.
    pub fn voice_people(&self, tree: &ServerTree, channel_id: &str) -> Vec<core_app::models::VoicePerson> {
        let mut people = tree.voice.get(channel_id).cloned().unwrap_or_default();

        if !self.call.seat.as_ref().is_some_and(|seat| seat.channel == channel_id) {
            return people;
        }

        if let Some(me) = self.me.as_ref()
            && !people.iter().any(|person| person.user_id == me.id)
        {
            people.push(core_app::models::VoicePerson {
                user_id: me.id,
                name: me.name.clone(),
                sources: Vec::new(),
                muted: self.call.muted,
                deafened: self.call.deafened,
            });
        }

        for &user_id in self.call.accounts.values() {
            if people.iter().any(|person| person.user_id == user_id) {
                continue;
            }

            let Some(member) = tree.members.iter().find(|member| member.user_id == user_id) else {
                continue;
            };

            people.push(core_app::models::VoicePerson {
                user_id,
                name: core_app::members::display_name(member).to_owned(),
                sources: Vec::new(),
                muted: false,
                deafened: false,
            });
        }

        // A árvore não sabe de quem começou a transmitir ou ligou a câmera depois de entrar: na
        // chamada, a sala sabe, e o "AO VIVO" ao lado do nome sai dela.
        let mut live: Vec<(i64, &str)> = Vec::new();

        if let Some(me) = self.my_id() {
            if self.call.mine.sharing {
                live.push((me, "screen"));
            }

            if self.call.mine.camera {
                live.push((me, "camera"));
            }
        }

        for tile in self.call.tiles.iter().map(|placed| &placed.tile).chain(&self.call.pending) {
            if let Some(&user_id) = self.call.accounts.get(&tile.peer_id) {
                live.push((user_id, if tile.camera { "camera" } else { "screen" }));
            }
        }

        for (user_id, source) in live {
            if let Some(person) = people.iter_mut().find(|person| person.user_id == user_id)
                && !person.sources.iter().any(|known| known == source)
            {
                person.sources.push(source.to_owned());
            }
        }

        people
    }

    pub fn my_id(&self) -> Option<i64> {
        self.me.as_ref().map(|user| user.id)
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Status(status) => self.phase = Phase::Starting(status),
            Event::Offline => self.phase = Phase::Offline,
            Event::Landed(user) => self.landed(user),
            Event::LoginRefused(error) => self.refuse_login(&error),
            Event::StartRefused(error) => {
                self.login.starting = false;
                self.login.start_error = Some(match &error {
                    HttpError::Invalid { message, .. } => message.clone(),
                    _ => said(&error),
                });
            }
            Event::EmailAdded(added) => {
                self.add_email.busy = false;

                match added {
                    Ok(user) => {
                        self.me = Some(user);
                        self.add_email = AddEmail::default();
                        self.notify("E-mail salvo: a conta também entra por ele.", false);
                    }
                    Err(HttpError::Invalid { message, .. }) => self.add_email.error = Some(message),
                    Err(error) => self.add_email.error = Some(said(&error)),
                }
            }
            Event::Servers(servers) => self.servers_arrived(servers),
            Event::Tree(tree) => self.tree_arrived(tree),
            Event::Messages { channel, messages } => {
                if self
                    .reading
                    .as_ref()
                    .is_some_and(|reading| reading.id == channel)
                {
                    self.beginning = messages.len() < PAGE;
                    self.messages = messages;
                    self.unread.insert(channel, 0);
                }
            }
            Event::ChannelRead(channel) => {
                self.unread.insert(channel, 0);
            }
            Event::Older { channel, messages } => {
                if self
                    .reading
                    .as_ref()
                    .is_some_and(|reading| reading.id == channel)
                {
                    self.beginning = messages.len() < PAGE;
                    self.loading_older = false;
                    self.scroll_anchor = self.scroll_anchor_pending.take();

                    let mut joined = messages;

                    joined.append(&mut self.messages);
                    self.messages = joined;
                }
            }
            Event::Conversations(conversations) => {
                let previous_hidden = self.hidden_conversations.len();
                self.hidden_conversations.retain(|user| !conversations.iter().any(|conversation| conversation.user.id == *user && conversation.unread > 0));
                if self.hidden_conversations.len() != previous_hidden {
                    self.backend.set_hidden_conversations(&self.hidden_conversations);
                }
                self.conversations = conversations;
            }
            Event::Friends(friends) => {
                self.friends = friends;
                self.follow();
            }
            Event::ServerSounds { server, sounds } => {
                if self.opened == Some(server) {
                    self.server_sounds = sounds;
                }
            }
            Event::ServerSoundPicked { server, path } => {
                self.modal = Some(Modal::ServerSound {
                    server,
                    path,
                    name: String::new(),
                    emoji: "🎵".to_owned(),
                });
            }
            Event::ActivityChanged(activity) => self.current_activity = activity,
            Event::ServerSoundDecoded {
                server,
                id,
                volume,
                samples,
            } => {
                if self.opened == Some(server) {
                    self.server_sound_cache.insert(id, samples.clone());
                    self.backend
                        .calls()
                        .play_server_sound_samples(&samples, volume);
                }
            }
            Event::Direct { person, messages, mutuals } => {
                if self.home == Home::Direct(person) {
                    self.direct_messages = messages;
                    self.direct_mutuals = mutuals;
                    self.direct_loading = false;
                }
            }
            Event::DirectSent(user) => {
                if matches!(&self.modal, Some(Modal::Invite { .. })) {
                    self.invite_sent.insert(user);
                }

                // Na própria conversa a mensagem já aparece nela, como no Discord: o aviso fica
                // para quem mandou de fora (o cartão de perfil, o convite).
                if !matches!(&self.home, Home::Direct(person) if person.id == user) {
                    self.notify("Mensagem enviada.", false);
                }
            }
            Event::Renamed(renamed) => {
                self.nickname.busy = false;

                match renamed {
                    Ok(user) => {
                        self.nickname = Nickname {
                            value: user.name.clone(),
                            ..Nickname::default()
                        };
                        self.me = Some(user);
                        self.editing_account_name = false;
                    }
                    Err(error) => self.nickname.error = Some(said(&error)),
                }
            }
            Event::ServerInviteCreated { server, invite } => {
                if self.opened == Some(server) || matches!(&self.modal, Some(Modal::Invite { server: shown, .. }) if *shown == server) {
                    self.server_invite = Some(invite.clone());
                    self.server_invites.retain(|current| current.id != invite.id);
                    self.server_invites.insert(0, invite);
                }
            }
            Event::ServerInvites { server, invites } => {
                if self.opened == Some(server) || matches!(&self.modal, Some(Modal::Invite { server: shown, .. }) if *shown == server) {
                    self.server_invites = invites;
                }
            }
            Event::InvitePreview { code, preview } => {
                self.invite_preview_pending.remove(&code);
                self.invite_previews.insert(code, (Instant::now(), preview));
            }
            Event::ServerIconPicked { path, bytes } => {
                if let Some(Modal::AddServer(AddServerStep::Customize { icon, .. })) =
                    &mut self.modal
                {
                    *icon = Some(ServerIconChoice {
                        path,
                        bytes: bytes.into(),
                    });
                }
            }
            Event::OpenServer(server) => self.open_server(server),
            Event::Complaint(text) => {
                self.direct_loading = false;
                self.notify(text, true);
            }
            Event::Attached(path) => self.attach(path),
            Event::ShareSources(sources) => self.share_sources = Some(sources),
            Event::SharePreview(source, image) => {
                let preview = self.context.load_texture(
                    format!("share-{source}"),
                    image,
                    egui::TextureOptions::LINEAR,
                );

                self.share_previews.insert(source, preview);
            }
            Event::Live(line) => self.heard_live(&line),
            Event::SessionEnded => {
                self.sign_out();
                self.login.error = Some("Sua sessão terminou. Entre de novo.".into());
            }
            Event::UpdateReady(version) => self.update = Some(version),
            Event::Notice(text) => self.notify(text, false),
            Event::Voice => {}
            Event::Devices {
                microphones,
                speakers,
                cameras,
            } => {
                self.microphones = microphones;
                self.speakers = speakers;
                self.cameras = cameras;
                self.cameras_known = true;
            }
            Event::Me(user) => {
                self.current_activity = user.activity.clone();
                self.me = Some(user);
                self.status_refresh_pending = false;
            }
            Event::Audits { server, entries } => {
                if matches!(&self.modal, Some(Modal::ServerSettings(settings)) if settings.server == server)
                {
                    self.audits = entries;
                }
            }
        }
    }

    fn landed(&mut self, user: Option<User>) {
        self.login.busy = false;

        let Some(user) = user else {
            self.show_entry();

            return;
        };

        // O apelido sai do nome, mas nem sempre igual a ele (sem acento, com número se já
        // existe): é por ele que a conta entra de novo, então vale dizer qual ficou.
        if self.login.starting {
            self.notify(format!("Sua conta é {}: entre com esse apelido e a sua senha.", user.name), false);
        }

        self.nickname.value = user.name.clone();
        self.current_activity = user.activity.clone();
        self.me = Some(user);
        self.status_refresh_pending = false;
        self.phase = Phase::Hub;
        self.login = Login::default();
        self.backend.servers();
        self.backend.conversations();
        self.backend.friends();
        self.backend.devices();
    }

    /// O erro de validação pinta o campo que o causou: a frase solta no rodapé, longe do que
    /// foi digitado, não diz o que consertar.
    fn refuse_login(&mut self, error: &HttpError) {
        self.login.busy = false;
        self.login.email_error = None;
        self.login.password_error = None;
        self.login.error = None;

        match error {
            HttpError::Invalid { field, message } if field == "email" => {
                self.login.email_error = Some(message.clone())
            }
            HttpError::Invalid { field, message } if field == "password" => {
                self.login.password_error = Some(message.clone());
            }
            _ => self.login.error = Some(said(error)),
        }
    }

    fn servers_arrived(&mut self, servers: Vec<ServerSummary>) {
        let gone = self
            .opened
            .is_some_and(|opened| !servers.iter().any(|server| server.id == opened));

        if matches!(&self.modal, Some(Modal::ServerSettings(settings)) if !servers.iter().any(|server| server.id == settings.server))
        {
            self.modal = None;
        }

        let first = self.servers.is_empty();

        self.servers = servers;

        if gone {
            self.open_home();
        }

        if first && crate::BETA {
            self.open_from_environment();
        }
    }

    /// Só no beta: `UNKVOID_BETA_ABRIR` abre direto uma tela, para fotografar e comparar com o
    /// Discord sem clique (a tela do Windows bloqueada não deixa clicar). `servidor:<nome>`,
    /// `voz`, `transmitir`, `camera`, separados por vírgula.
    fn open_from_environment(&mut self) {
        let Ok(wanted) = std::env::var("UNKVOID_BETA_ABRIR") else {
            return;
        };

        for step in wanted.split(',').map(str::trim) {
            match step.split_once(':') {
                Some(("servidor", name)) => {
                    if let Some(server) = self.servers.iter().find(|server| server.name.to_lowercase().starts_with(&name.to_lowercase())) {
                        self.actions.push(Action::OpenServer(server.id));
                    }
                }
                Some(("voz", section)) => {
                    let section = match section {
                        "video" => VoiceSection::Video,
                        "sons" => VoiceSection::Sounds,
                        "painel" => VoiceSection::Soundboard,
                        "avancado" => VoiceSection::Advanced,
                        _ => VoiceSection::Voice,
                    };

                    self.actions
                        .push(Action::Show(Modal::Settings(SettingsPage::Voice(section))));
                }
                _ if step == "voz" => self.actions.push(Action::Show(Modal::Settings(
                    SettingsPage::Voice(VoiceSection::Voice),
                ))),
                _ if step == "transmitir" => self
                    .actions
                    .push(Action::Show(Modal::GoLive(GoLiveTab::Screens))),
                _ if step == "camera" => self.actions.push(Action::Show(Modal::CameraPreview)),
                _ if step == "conta" => self.account_popup_requested = true,
                Some(("config-servidor", page)) => {
                    self.environment_server_page = Some(match page {
                        "cargos" => ServerPage::Roles,
                        "membros" => ServerPage::Members,
                        "convites" => ServerPage::Invites,
                        "sons" => ServerPage::Soundboard,
                        "bans" => ServerPage::Bans,
                        "auditoria" => ServerPage::Audit,
                        _ => ServerPage::Overview,
                    });
                }
                _ => {}
            }
        }
    }

    /// A árvore do servidor aberto chegou (ou mudou): o canal lido continua aberto se ainda
    /// existe para esta pessoa; senão fecha, como no Discord quando o canal é escondido.
    fn tree_arrived(&mut self, tree: ServerTree) {
        if self.opened != Some(tree.id) {
            return;
        }

        if let Some(page) = self.environment_server_page.take() {
            self.actions
                .push(Action::Show(Modal::ServerSettings(ServerSettings::new(&tree, page))));
        }

        for channel in &tree.channels {
            self.unread.insert(channel.id.clone(), channel.unread);
        }

        if let Some(reading) = &self.reading {
            match tree
                .channels
                .iter()
                .find(|channel| channel.id == reading.id)
            {
                Some(fresh) => self.reading = Some(fresh.clone()),
                None => {
                    self.reading = None;
                    self.messages.clear();
                }
            }
        }

        if self.reading.is_none() {
            self.reading = first_text_channel(&tree);

            if let Some(channel) = &self.reading {
                self.backend.messages(channel.id.clone());
            }
        }

        self.tree = Some(tree);
        self.follow();
    }

    /// Um evento do tempo real. O que ele significa quem diz é o núcleo (`realtime::read`);
    /// aqui só se relê, toca e avisa.
    fn heard_live(&mut self, line: &str) {
        let Ok(update) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };
        let (event, channel, data) = (
            update["event"].as_str().unwrap_or_default(),
            update["channel"].as_str(),
            &update["data"],
        );

        if event == "ReleasePublished" {
            return self.backend.release_announced(data);
        }

        // Presença que chega atrasada do servidor anterior não vale para o aberto agora.
        if event.starts_with("presence.")
            && channel
                != self
                    .opened
                    .map(|server| format!("server.{server}"))
                    .as_deref()
        {
            return;
        }

        let Some(me) = self.my_id() else {
            return;
        };
        let talking = match &self.home {
            Home::Direct(person) if self.opened.is_none() => Some(person.id),
            _ => None,
        };

        self.act(realtime::read(event, channel, data, me, talking));
    }

    fn act(&mut self, reading: Reading) {
        let quiet = self.me.as_ref().is_some_and(|me| me.status == UserStatus::Dnd);

        if let Some(chime) = reading.chime
            && !(quiet && chime == core_app::chimes::Chime::Message)
        {
            self.backend.calls().chime(chime);
        }

        if let Some(notice) = reading.notice.filter(|_| !(quiet && reading.message_notice)) {
            self.notify(notice.text, notice.error);
        }

        if let Some((user, status)) = reading.status_update {
            if Some(user) == self.my_id() {
                self.backend.refresh_me();
            } else if self.tree.as_ref().is_some_and(|tree| tree.members.iter().any(|member| member.user_id == user)) {
                if status == UserStatus::Offline {
                    self.online.remove(&user);
                } else {
                    self.online.insert(user);
                }
                self.see_myself_online();
            }
        }

        if let Some(channel) = reading.messages_of {
            if self
                .reading
                .as_ref()
                .is_some_and(|reading| reading.id == channel)
            {
                self.backend.messages(channel);
            } else if reading.unread {
                *self.unread.entry(channel).or_default() += 1;
            }
        }

        if reading.direct || reading.catch_up {
            if let Home::Direct(person) = &self.home
                && reading.direct_with.is_none_or(|with| with == person.id)
            {
                self.backend.direct(person.clone());
            } else {
                self.backend.conversations();
            }
        }

        if reading.friends || reading.catch_up {
            self.backend.friends();
        }

        if (reading.tree || reading.catch_up)
            && let Some(server) = self.opened
        {
            self.backend.tree(server);
        }

        if let Some(server) = reading.removed_from {
            if self.opened == Some(server) {
                self.open_home();
            }

            self.backend.servers();
        }

        if let Some(presence) = &reading.presence {
            presence.apply(&mut self.online);
            self.see_myself_online();
        }

        if let Some(change) = &reading.voice_mute
            && let Some(person) = self
                .tree
                .as_mut()
                .and_then(|tree| tree.voice.get_mut(&change.channel_id))
                .and_then(|people| people.iter_mut().find(|person| person.user_id == change.user_id))
        {
            person.muted = change.muted;
            person.deafened = change.deafened;
        }

        if reading.catch_up
            && let Some(channel) = &self.reading
        {
            self.backend.messages(channel.id.clone());
        }
    }

    pub fn notify(&mut self, text: impl Into<String>, error: bool) {
        self.push_toast(text.into(), error, None);
    }

    pub fn notify_with_action(
        &mut self,
        text: impl Into<String>,
        error: bool,
        action_path: PathBuf,
    ) {
        self.push_toast(text.into(), error, Some(action_path));
    }

    fn push_toast(&mut self, text: String, error: bool, action_path: Option<PathBuf>) {
        self.next_toast += 1;
        self.toasts.push(Toast {
            id: self.next_toast,
            text,
            error,
            action_path,
            until: Instant::now() + if error { ERROR_LIFE } else { NOTICE_LIFE },
        });
    }

    /// Acerta o que o tempo real ouve com o que está aberto: o servidor (presença e mudanças) e
    /// cada canal dele — o de texto para as mensagens e as não lidas, o de voz para quem entra
    /// e sai.
    fn follow(&self) {
        let mut wanted = BTreeSet::new();

        if let Some(tree) = self
            .tree
            .as_ref()
            .filter(|tree| Some(tree.id) == self.opened)
        {
            wanted.insert(format!("server.{}", tree.id));

            for channel in tree
                .channels
                .iter()
                .filter(|channel| channel.kind != ChannelKind::Category)
            {
                wanted.insert(format!("channel.{}", channel.id));
            }
        }

        if let Some(me) = self.my_id() {
            for friendship in &self.friends {
                if friendship.status == core_app::models::FriendshipStatus::Accepted {
                    let friend = if friendship.requester.id == me { &friendship.addressee } else { &friendship.requester };
                    if friend.id != me {
                        wanted.insert(format!("friend.{}", friend.id));
                    }
                }
            }
        }

        self.backend.follow(wanted);
    }

    /// Aplica o que os cliques do quadro pediram.
    pub fn apply_actions(&mut self) {
        for action in std::mem::take(&mut self.actions) {
            self.apply(action);
        }

        // O medidor do menu do microfone liga ao abrir e desliga ao fechar, uma vez só.
        let open = std::mem::take(&mut self.input_menu_seen)
            || (std::mem::take(&mut self.noise_test_seen) && self.noise_test_active);

        if open != self.input_menu_open {
            self.input_menu_open = open;

            if open {
                self.backend.calls().start_meter();
            } else if !matches!(self.modal, Some(Modal::Settings(SettingsPage::Voice(_)))) {
                self.backend.calls().stop_meter();
            }
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::SignIn => self.sign_in(),
            Action::Start => {
                let name = self.login.guest_name.trim().to_owned();

                if !name.is_empty() && !self.login.start_password.is_empty() && !self.login.starting {
                    self.login.starting = true;
                    self.login.start_error = None;
                    self.backend.start_with_name(name, self.login.start_password.clone());
                }
            }
            Action::AddEmail => {
                let email = self.add_email.email.trim().to_owned();

                if !email.is_empty() && !self.add_email.busy {
                    self.add_email.busy = true;
                    self.add_email.error = None;
                    self.backend.add_email(email);
                }
            }
            Action::GoogleSignIn => self.backend.google_sign_in(),
            Action::SignOut => self.sign_out(),
            Action::Retry => {
                self.phase = Phase::Starting("Abrindo…".into());
                self.backend.start();
            }
            Action::OpenHome => self.open_home(),
            Action::OpenServer(server) => self.open_server(server),
            Action::OpenChannel(channel) => self.open_channel(channel),
            Action::MarkChannelRead(channel) => {
                self.unread.insert(channel.clone(), 0);
                self.backend.mark_channel_read(channel);
            }
            Action::Mention(name) => self.composer = append_mention(&self.composer, &name),
            Action::SetPeerVolume(user, volume) => {
                self.backend.calls().set_peer_volume(user, volume)
            }
            Action::TogglePeerMute(user) => self.backend.calls().toggle_peer_mute(user),
            Action::ToggleCategory(category) => {
                if !self.collapsed.remove(&category) {
                    self.collapsed.insert(category);
                }
            }
            Action::LoadOlder => self.load_older(),
            Action::SendMessage => self.send_message(),
            Action::Wave => {
                if let Some(channel) = &self.reading {
                    self.backend.send_message(channel.id.clone(), "👋".to_owned());
                }
            }
            Action::StartEdit(message, body) => self.editing = Some((message, body)),
            Action::CancelEdit => self.editing = None,
            Action::SaveEdit => self.save_edit(),
            Action::DeleteMessage(message) => {
                // A conversa direta fica guardada em `home` com um servidor aberto por cima: só
                // vale quando a Home é o que está na tela.
                if self.opened.is_none()
                    && let Home::Direct(person) = &self.home
                {
                    self.backend.delete_direct(person.clone(), message);
                } else if let Some(channel) = &self.reading {
                    self.backend.delete_message(channel.id.clone(), message);
                }
            }
            Action::OpenFriends(tab) => {
                self.opened = None;
                self.home = Home::Friends(tab);
                self.backend.friends();
            }
            Action::OpenDirect(person) => self.open_direct(person),
            Action::HideConversation(user) => {
                self.hidden_conversations.insert(user);
                self.backend.set_hidden_conversations(&self.hidden_conversations);
            }
            Action::SendDirect => self.send_direct(),
            Action::SendDirectTo(person, body) => {
                self.backend.send_direct(person, body);
            }
            Action::AddFriend => {
                let email = std::mem::take(&mut self.friend_email).trim().to_owned();

                // O "enviado" vem do núcleo depois da resposta: "ninguém com esse apelido" não
                // pode chegar depois de um "enviado".
                if !email.is_empty() {
                    self.backend.add_friend(email);
                }
            }
            Action::AddFriendById(user) => self.backend.add_friend_by_id(user),
            Action::AnswerFriend(friendship, accept) => {
                self.backend.answer_friend(friendship, accept)
            }
            Action::BlockFriend(friendship) => self.backend.block_friend(friendship),
            Action::Show(modal) => self.show(modal),
            Action::CloseModal => {
                if self.video_test || matches!(self.modal, Some(Modal::CameraPreview)) {
                    self.close_camera_preview();
                }

                self.modal = None;
                self.backend.calls().stop_meter();
            }
            Action::AttachImage => self.backend.pick_attachment(),
            Action::PasteImage => self.backend.paste_image(),
            Action::RemoveAttachment(index) => {
                if index < self.attachments.len() {
                    drop(self.attachments.remove(index));
                }
            }
            Action::TestVideo(on) => {
                if on {
                    self.video_test = true;
                    self.open_camera_preview();
                } else {
                    self.close_camera_preview();
                }
            }
            Action::StartShare(source) => {
                let (quality, frame_rate) = crate::ui::go_live::share_mode(&self.voice_settings);
                let config = core_app::sharing::capture_config(&serde_json::json!({
                    "source": source,
                    "quality": quality,
                    "fps": frame_rate,
                    "audio": !self.voice_settings["shareMuted"].as_bool().unwrap_or(false),
                    "muteCalls": self.voice_settings["shareMuteDiscord"].as_bool().unwrap_or(true),
                }));

                self.modal = None;
                self.backend.calls().share(config);
            }
            Action::StopShare => self.backend.calls().stop_sharing(),
            Action::ToggleCamera => {
                if self.call.mine.camera {
                    self.backend.calls().stop_camera(self.video_test);
                } else if crate::camera::PREVIEWS
                    && self.voice_settings["alwaysPreviewVideo"]
                        .as_bool()
                        .unwrap_or(true)
                {
                    self.backend.devices();
                    self.show(Modal::CameraPreview);
                } else {
                    self.apply(Action::StartCamera);
                }
            }
            Action::StartCamera => {
                if matches!(self.modal, Some(Modal::CameraPreview)) {
                    self.modal = None;
                }

                match self.chosen_camera() {
                    Some(index) => self.backend.calls().start_camera(index, &self.context),
                    None => self.notify("Nenhuma câmera encontrada.", true),
                }
            }
            Action::FocusTile(producer) => self.backend.calls().focus(&producer),
            Action::FullTile(producer) => self.backend.calls().full(&producer),
            Action::WatchStream(producer, watching) => {
                self.backend.calls().watch_stream(&producer, watching);
            }
            Action::SelfView(wanted) => self.backend.calls().self_view(wanted),
            Action::StreamSound(producer) => self.backend.calls().toggle_stream_sound(&producer),
            Action::StreamVolume(producer, volume) => {
                self.backend.calls().set_stream_volume(&producer, volume);
            }
            Action::CreateServer => {
                if let Some(Modal::AddServer(AddServerStep::Customize { name, icon })) =
                    self.modal.take()
                {
                    let name = name.trim().to_owned();

                    if !name.is_empty() {
                        self.backend.create_server(name, icon.map(|icon| icon.path));
                    }
                }
            }
            Action::JoinInvite => {
                if let Some(Modal::AddServer(AddServerStep::Join { invite })) = self.modal.take() {
                    let code = invite_code(&invite);

                    if !code.is_empty() {
                        self.backend.join_invite(code);
                    }
                }
            }
            Action::JoinInviteCode(code) => self.backend.join_invite(code),
            Action::LoadInvitePreview(code) => {
                if !self.invite_preview_pending.contains(&code)
                    && self.invite_previews.get(&code).is_none_or(|(at, _)| at.elapsed() >= Duration::from_secs(60))
                {
                    self.invite_preview_pending.insert(code.clone());
                    self.backend.invite_preview(code);
                }
            }
            Action::OpenUrl(url) => crate::system::open_in_browser(&url),
            Action::PickServerIcon => self.backend.pick_server_icon(),
            Action::CreateChannel => {
                if let Some(Modal::CreateChannel {
                    server,
                    kind,
                    name,
                    parent,
                }) = self.modal.take()
                {
                    let name = name.trim().to_owned();

                    if !name.is_empty() {
                        self.backend.create_channel(server, name, kind, parent);
                    }
                }
            }
            Action::CreateServerInvite { server, max_age, max_uses } => {
                self.invite_max_age = max_age;
                self.invite_max_uses = max_uses;
                self.invite_editing = false;
                let channel = self.tree.as_ref().filter(|tree| tree.id == server).and_then(|tree| {
                    self.reading.as_ref().filter(|channel| channel.kind == ChannelKind::Text)
                        .or_else(|| tree.channels.iter().find(|channel| channel.kind == ChannelKind::Text))
                        .map(|channel| channel.id.clone())
                });
                self.server_invite = None;
                self.backend.create_server_invite(server, channel, max_age, max_uses);
            }
            Action::SetInvitesPaused { server, paused } => self.backend.set_invites_paused(server, paused),
            Action::DeleteServerInvite { server, invite } => self.backend.delete_server_invite(server, invite),
            Action::LeaveServer(server) => {
                self.modal = None;
                self.backend.leave_server(server);
            }
            Action::Rename => {
                let name = self.nickname.value.trim().to_owned();

                self.nickname.busy = true;
                self.nickname.error = None;
                self.backend.rename(name);
            }
            Action::ToggleMembers if self.members_room => self.show_members = !self.show_members,
            Action::ToggleMembers => self.members_narrow = !self.members_narrow,
            Action::ToggleNoiseTest => self.noise_test_active = !self.noise_test_active,
            Action::ToggleMute => self.backend.calls().toggle_mute(),
            Action::ToggleDeafen => self.backend.calls().toggle_deafen(),
            Action::JoinVoice(channel) => self.join_voice(channel),
            Action::EnterRoom(recent) => self.enter_room(recent),
            // O começo de quem tem conta é o hub, com a sala no ar por baixo.
            Action::ShowEntry if self.me.is_some() => self.phase = Phase::Hub,
            Action::ShowEntry => self.show_entry(),
            Action::BackToRoom => {
                if self.call.seat.is_some() {
                    self.phase = Phase::Room;
                    self.modal = None;
                }
            }
            Action::BackToCall => self.back_to_call(),
            Action::LeaveVoice => self.backend.calls().leave(),
            Action::ToggleVoiceChat => self.voice_chat_open = !self.voice_chat_open,
            Action::SetUserStatus(status, minutes) => {
                let status = serde_json::to_value(status).unwrap_or(serde_json::Value::String("online".into()));
                self.backend.request("updateMe", serde_json::json!({}), serde_json::json!({
                    "status": status,
                    "status_minutes": minutes,
                }), Refresh::Me);
            }
            Action::SetActivitySharing(enabled) => {
                self.activity_sharing = enabled;
                self.backend.set_activity_sharing(enabled);

                if !enabled {
                    self.current_activity = None;
                }
            }
            Action::SendVoiceLogs => self.backend.send_voice_logs(),
            Action::PlaySoundboard(sound) => {
                if !self.backend.calls().play_soundboard(sound) {
                    self.notify("Você está silenciado ou não pode falar neste canal.", true);
                }
            }
            Action::LoadServerSounds(server) => self.backend.server_sounds(server),
            Action::PickServerSound(server) => self.backend.pick_server_sound(server),
            Action::SaveServerSound => {
                if let Some(Modal::ServerSound {
                    server,
                    path,
                    name,
                    emoji,
                }) = self.modal.clone()
                {
                    let file_size = std::fs::metadata(&path)
                        .map(|metadata| metadata.len())
                        .unwrap_or(u64::MAX);
                    let supported = path
                        .extension()
                        .and_then(std::ffi::OsStr::to_str)
                        .is_some_and(|extension| {
                            ["mp3", "ogg", "wav"].contains(&extension.to_ascii_lowercase().as_str())
                        });
                    if file_size <= 512 * 1024 && supported && !name.trim().is_empty() {
                        self.backend.upload_server_sound(
                            server,
                            path,
                            name.trim().to_owned(),
                            emoji.trim().to_owned(),
                        );
                        self.modal = None;
                    } else {
                        self.notify(
                            "Escolha um MP3, OGG ou WAV de até 512 KB e dê um nome ao som.",
                            true,
                        );
                    }
                }
            }
            Action::DeleteServerSound(server, sound) => {
                self.backend.delete_server_sound(server, sound)
            }
            Action::PlayServerSound(sound) => {
                let marker = format!("server:{}", sound.id);
                let mut frequent = self.voice_settings["soundboardFrequent"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                frequent.retain(|value| value.as_str() != Some(marker.as_str()));
                frequent.insert(0, serde_json::Value::String(marker));
                frequent.truncate(6);
                self.backend
                    .calls()
                    .set("soundboardFrequent", serde_json::Value::Array(frequent));
                if let Some(samples) = self.server_sound_cache.get(&sound.id) {
                    self.backend
                        .calls()
                        .play_server_sound_samples(samples, sound.volume);
                } else if let Some(server) = self.opened {
                    self.backend.play_server_sound(server, sound);
                }
            }
            Action::InstallUpdate => self.backend.install_update(),
            Action::Dismiss(toast) => self.toasts.retain(|shown| shown.id != toast),
            Action::SetVoice(field, value) => self.backend.calls().set(field, value),
            Action::SetSound(chime, on) => self.backend.calls().set_sound(chime, on),
            Action::PreviewSound(chime) => self.backend.calls().preview(chime),
            Action::CopyText(text) => {
                self.copied = Some(text);
                self.notify("Copiado.", false);
            }
            Action::Request {
                route,
                params,
                body,
                refresh,
            } => self.backend.request(route, params, body, refresh),
            Action::UploadImage {
                route,
                params,
                field,
                refresh,
            } => self.backend.upload_image(route, params, field, refresh),
            Action::Ask(confirm) => self.confirm = Some(confirm),
            Action::CloseConfirm => self.confirm = None,
        }
    }

    /// O texto que a janela leva à área de transferência no próximo quadro.
    pub fn take_copied(&mut self) -> Option<String> {
        self.copied.take()
    }

    fn sign_in(&mut self) {
        let email = self.login.email.trim().to_owned();

        if email.is_empty() || self.login.password.is_empty() || self.login.busy {
            return;
        }

        self.login.busy = true;
        self.login.error = None;
        self.login.email_error = None;
        self.login.password_error = None;
        self.backend
            .sign_in(email, self.login.password.clone(), self.login.register);
    }

    /// Sair nunca depende da rede: a tela volta ao login na hora.
    fn sign_out(&mut self) {
        self.current_activity = None;
        self.backend.sign_out();
        self.me = None;
        self.status_refresh_pending = false;
        self.servers.clear();
        self.opened = None;
        self.tree = None;
        self.reading = None;
        self.messages.clear();
        self.conversations.clear();
        self.friends.clear();
        self.direct_messages.clear();
        self.direct_mutuals = Mutuals::default();
        self.online.clear();
        self.online_by_server.clear();
        self.unread.clear();
        self.modal = None;
        self.confirm = None;
        self.login = Login::default();
        self.show_entry();
    }

    /// O nome do canal no "Voz conectada" leva de volta à chamada, como no Discord. O servidor
    /// abre com a árvore guardada, e o canal de voz aberto é o que mostra o palco.
    fn back_to_call(&mut self) {
        let Some(seat) = self.call.seat.clone() else {
            return;
        };

        if seat.guest.is_some() {
            self.phase = Phase::Room;
            self.modal = None;
            return;
        }

        self.open_server(seat.server);

        if let Some(channel) = self.tree.as_ref().and_then(|tree| tree.channels.iter().find(|channel| channel.id == seat.channel)).cloned() {
            self.open_channel(channel);
        }
    }

    /// A entrada: o login e, do lado, a sala por código já com o nome e as últimas salas.
    fn show_entry(&mut self) {
        let rooms = self.backend.rooms();

        if self.login.guest_name.is_empty() {
            self.login.guest_name = rooms.guest_name();
        }

        self.login.recent_rooms = rooms.recent_rooms();
        self.phase = Phase::SignIn;
    }

    /// A sala por código sem conta: o núcleo valida e guarda; a voz entra como visitante.
    fn enter_room(&mut self, recent: Option<String>) {
        let name = self.login.guest_name.clone();
        let rooms = self.backend.rooms();
        let entered = match &recent {
            Some(code) => rooms.join_room(&name, code),
            None => rooms.create_room(&name, &self.login.room_code),
        };

        let code = match entered {
            Ok(code) => code,
            Err(refusal) => {
                self.login.room_error = Some(
                    match refusal {
                        core_app::EntryRefusal::NameIsEmpty => "Diga um nome para os outros verem.",
                        core_app::EntryRefusal::CodeIsInvalid => {
                            "O código tem de 3 a 32 letras, números ou hífens."
                        }
                    }
                    .into(),
                );

                return;
            }
        };

        self.login.room_error = None;
        self.login.room_code.clear();
        self.voice_chat_open = false;
        self.modal = None;
        self.phase = Phase::Room;
        self.backend.calls().join(Seat {
            channel: code.clone(),
            name: code,
            server: 0,
            guest: Some(name.trim().to_owned()),
        });
    }

    fn open_home(&mut self) {
        self.opened = None;
        self.tree = None;
        self.reading = None;
        self.messages.clear();
        self.online.clear();
        self.follow();
        self.backend.servers();
        self.backend.conversations();
    }

    /// O que o núcleo já guardou vai para a tela antes do pedido: a coluna de canais não pisca
    /// vazia ao trocar de servidor.
    fn open_server(&mut self, server: i64) {
        if self.opened == Some(server) {
            return;
        }

        self.opened = Some(server);
        self.tree = None;
        self.reading = None;
        self.messages.clear();
        self.editing = None;
        self.online = self
            .online_by_server
            .get(&server)
            .cloned()
            .unwrap_or_default();
        self.see_myself_online();

        if let Some(tree) = self.backend.known_tree(server) {
            self.tree_arrived(tree);
        }

        self.backend.tree(server);
    }

    fn open_channel(&mut self, channel: Channel) {
        // ponytail: a voz chega na fase 2; até lá o canal de voz só abre o chat dele, que o
        // Laravel já serve.
        if self
            .reading
            .as_ref()
            .is_some_and(|reading| reading.id == channel.id)
        {
            return;
        }

        self.messages.clear();
        self.beginning = false;
        self.loading_older = false;
        self.scroll_anchor = None;
        self.scroll_anchor_pending = None;
        self.chat_height = None;
        self.editing = None;
        self.composer.clear();
        self.backend.messages(channel.id.clone());
        self.unread.insert(channel.id.clone(), 0);
        self.reading = Some(channel);
    }

    fn load_older(&mut self) {
        if self.beginning || self.loading_older {
            return;
        }

        let (Some(channel), Some(first)) = (&self.reading, self.messages.first()) else {
            return;
        };

        self.loading_older = true;
        self.backend.older_messages(channel.id.clone(), first.id);
    }

    fn send_message(&mut self) {
        let Some(channel) = &self.reading else {
            return;
        };
        let body = self.composer.trim().to_owned();

        if body.is_empty() && self.attachments.is_empty() {
            return;
        }

        self.composer.clear();

        if self.attachments.is_empty() {
            self.backend.send_message(channel.id.clone(), body);
        } else {
            let images = std::mem::take(&mut self.attachments)
                .into_iter()
                .map(|(path, _)| path)
                .collect();

            self.backend
                .send_message_with_images(channel.id.clone(), body, images);
        }
    }

    /// Uma imagem para a próxima mensagem, com a miniatura que aparece em cima da caixa. Três
    /// por mensagem, o que o servidor aceita.
    fn attach(&mut self, path: PathBuf) {
        const MOST: usize = 3;

        if self.attachments.len() >= MOST {
            return self.notify("Até 3 imagens por mensagem.", true);
        }

        let Ok(decoded) = image::open(&path) else {
            return self.notify("Essa imagem não abriu.", true);
        };
        let small = decoded.thumbnail(320, 320).to_rgba8();
        let preview = self.context.load_texture(
            format!("anexo-{}", path.display()),
            egui::ColorImage::from_rgba_unmultiplied(
                [small.width() as usize, small.height() as usize],
                small.as_raw(),
            ),
            egui::TextureOptions::LINEAR,
        );

        self.attachments.push((path, preview));
    }

    fn save_edit(&mut self) {
        let Some((message, body)) = self.editing.take() else {
            return;
        };
        let body = body.trim().to_owned();

        if !body.is_empty() {
            if self.opened.is_none()
                && let Home::Direct(person) = &self.home
            {
                self.backend.edit_direct(person.clone(), message, body);
            } else if let Some(channel) = &self.reading {
                self.backend.edit_message(channel.id.clone(), message, body);
            }
        }
    }

    fn open_direct(&mut self, person: Person) {
        self.hidden_conversations.remove(&person.id);
        self.backend.set_hidden_conversations(&self.hidden_conversations);
        self.opened = None;
        self.tree = None;
        self.reading = None;
        self.direct_messages.clear();
        self.direct_loading = true;
        self.quick_conversation_open = false;
        self.quick_conversation_query.clear();
        self.direct_search_query.clear();
        self.direct_search_open = false;
        self.direct_profile_open = true;
        self.direct_composer.clear();
        self.home = Home::Direct(person.clone());
        self.follow();
        self.backend.direct(person);
    }

    fn send_direct(&mut self) {
        let Home::Direct(person) = &self.home else {
            return;
        };
        let body = self.direct_composer.trim().to_owned();

        if body.is_empty() {
            return;
        }

        self.direct_composer.clear();
        self.backend.send_direct(person.clone(), body);
    }

    fn show(&mut self, modal: Modal) {
        if let Modal::Invite { server, code: None } = &modal {
            self.invite_search.clear();
            self.invite_sent.clear();
            self.invite_max_age = 604800;
            self.invite_max_uses = 0;
            self.invite_editing = false;
            self.server_invite = None;
            let channel = self.tree.as_ref().filter(|tree| tree.id == *server).and_then(|tree| {
                self.reading.as_ref().filter(|channel| channel.kind == ChannelKind::Text)
                    .or_else(|| tree.channels.iter().find(|channel| channel.kind == ChannelKind::Text))
                    .map(|channel| channel.id.clone())
            });
            if self.tree.as_ref().is_some_and(|tree| tree.id == *server && tree.invites_paused) {
                self.modal = Some(Modal::Invite { server: *server, code: Some(String::new()) });
            } else {
                self.backend.create_server_invite(*server, channel, self.invite_max_age, self.invite_max_uses);
                self.backend.friends();
                self.modal = Some(modal);
            }
            return;
        }

        if let Modal::ServerSettings(settings) = &modal
            && settings.page == ServerPage::Audit
            && !matches!(&self.modal, Some(Modal::ServerSettings(shown)) if shown.page == ServerPage::Audit)
        {
            self.audits.clear();
            self.backend.audits(settings.server);
        }

        if let Modal::ServerSettings(settings) = &modal
            && settings.page == ServerPage::Invites
            && !matches!(&self.modal, Some(Modal::ServerSettings(shown)) if shown.server == settings.server && shown.page == ServerPage::Invites)
        {
            self.server_invites.clear();
            self.backend.server_invites(settings.server);
        }

        if let Modal::ServerSettings(settings) = &modal
            && settings.page == ServerPage::Soundboard
            && !matches!(&self.modal, Some(Modal::ServerSettings(shown)) if shown.server == settings.server && shown.page == ServerPage::Soundboard)
        {
            self.backend.server_sounds(settings.server);
        }

        if let Modal::GoLive(_) = modal
            && !matches!(self.modal, Some(Modal::GoLive(_)))
        {
            self.share_sources = None;
            self.share_selected = None;
            self.share_previews.clear();
            self.backend.share_sources();
        }

        if modal == Modal::CameraPreview {
            self.open_camera_preview();
        }

        // Com conta, o nome que a sala vê é o da conta.
        if modal == Modal::CodeRoom {
            if let Some(me) = &self.me {
                self.login.guest_name = me.name.clone();
            }

            self.login.room_error = None;
            self.login.recent_rooms = self.backend.rooms().recent_rooms();
        }

        // O player é uma janela do Windows por cima da nossa: ficaria na frente do modal.
        #[cfg(target_os = "windows")]
        self.clips_runtime.close_player();

        if let Modal::Settings(page) = modal {
            if !matches!(self.modal, Some(Modal::Settings(_))) {
                self.nickname = Nickname {
                    value: self
                        .me
                        .as_ref()
                        .map(|me| me.name.clone())
                        .unwrap_or_default(),
                    ..Nickname::default()
                };
            }

            // O microfone de teste só fica aberto com a página de Voz e vídeo na tela; a lista de
            // aparelhos é lida na hora de abrir, para o fone ligado agora aparecer.
            if matches!(page, SettingsPage::Voice(_)) {
                self.backend.calls().start_meter();
                self.backend.devices();
            } else {
                self.backend.calls().stop_meter();
            }
        }

        self.modal = Some(modal);
    }

    /// Entrar no canal de voz abre também o chat dele no centro: o palco das telas chega com a
    /// transmissão.
    fn join_voice(&mut self, channel: Channel) {
        let Some(server) = self.opened else {
            return;
        };

        self.voice_chat_open = false;
        self.backend.calls().join(Seat {
            channel: channel.id.clone(),
            name: channel.name.clone(),
            server,
            guest: None,
        });
        self.open_channel(channel);
    }

    /// Quem está com o app aberto está online: a própria pessoa aparece assim desde o primeiro
    /// quadro, sem esperar a presença do servidor. E o servidor aberto guarda o que se sabe dele.
    fn see_myself_online(&mut self) {
        if let Some(me) = self.my_id() {
            self.online.insert(me);
        }

        if let Some(server) = self.opened {
            self.online_by_server.insert(server, self.online.clone());
        }
    }

    /// A conta ainda não confirmou o apelido automático: o aviso que não fecha.
    pub fn needs_nickname(&self) -> bool {
        self.me.as_ref().is_some_and(|me| !me.nickname_confirmed)
    }
}

/// O primeiro canal de texto que a pessoa vê, na ordem da coluna: é o que abre com o servidor.
fn first_text_channel(tree: &ServerTree) -> Option<Channel> {
    tree.sections()
        .into_iter()
        .flat_map(|section| section.channels)
        .find(|channel| channel.kind == ChannelKind::Text)
}

/// O código de um convite colado inteiro (`https://unkvoid.com/convite/abc`) ou só o código.
fn invite_code(typed: &str) -> String {
    typed
        .trim()
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned()
}

pub fn append_mention(message: &str, name: &str) -> String {
    let message = message.trim_end();
    if message.is_empty() {
        format!("@{name} ")
    } else {
        format!("{message} @{name} ")
    }
}

/// A textura de um quadro de vídeo: refeita no lugar quando já existe, para a placa não
/// guardar uma nova a cada quadro. O quadro é opaco, e aí "pré-multiplicado" é o mesmo e não
/// passa pela conversão.
fn texture(
    context: &egui::Context,
    old: Option<egui::TextureHandle>,
    name: &str,
    picture: &crate::watching::Picture,
) -> egui::TextureHandle {
    let image = egui::ColorImage::from_rgba_premultiplied(
        [picture.width as usize, picture.height as usize],
        &picture.pixels,
    );

    match old {
        Some(mut texture) => {
            texture.set(image, egui::TextureOptions::LINEAR);

            texture
        }
        None => context.load_texture(name, image, egui::TextureOptions::LINEAR),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pasted_invite_link_becomes_its_code() {
        assert_eq!(
            invite_code("  https://unkvoid.com/convite/abcdef1234/ "),
            "abcdef1234"
        );
        assert_eq!(invite_code("abcdef1234"), "abcdef1234");
        assert_eq!(invite_code(""), "");
    }

    #[test]
    fn mentioning_appends_a_spaced_name_without_losing_the_draft() {
        assert_eq!(append_mention("", "Ana"), "@Ana ");
        assert_eq!(append_mention("olá  ", "Ana"), "olá @Ana ");
    }
}
