//! O que o Laravel e o SFU dizem, do jeito que as três interfaces vão ler.
//!
//! Os nomes dos campos são os que trafegam na rede — por isso `snake_case` e por isso
//! `avatar_url` e não `avatarUrl`. Renomear aqui quebraria o contrato de `docs/CONTRATO.md`
//! sem avisar ninguém.

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UserStatus {
    #[default]
    Online,
    Idle,
    Dnd,
    Invisible,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub status: UserStatus,
    #[serde(default)]
    pub activity: Option<crate::activity::Activity>,
    #[serde(default)]
    pub status_until: Option<String>,
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub avatar_uploaded: bool,
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub nickname_confirmed: bool,
    /// Nasceu com o nome e a senha e ainda não pôs e-mail: o `email` é o de mentira do
    /// servidor (`apelido@<uuid>.unkvoid.com`), e não se mostra.
    #[serde(default)]
    pub name_only: bool,
    #[serde(flatten)]
    pub profile: Profile,
}

/// O que o cartão de perfil mostra além do nome e da foto. Tudo opcional: conta antiga ou
/// servidor sem a coluna chegam sem nada disso.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub pronouns: Option<String>,
    /// `#rrggbb`; sem ela, a faixa sai da cor do nome.
    #[serde(default)]
    pub banner_color: Option<String>,
    #[serde(default)]
    pub custom_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerSummary {
    pub id: i64,
    pub name: String,
    pub owner_id: i64,
    pub icon_url: Option<String>,
    pub last_accessed_at: Option<String>,
    /// As não lidas dos canais que a pessoa vê: é o que acende a bolinha no trilho.
    #[serde(default)]
    pub unread: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChannelKind {
    Text,
    Voice,
    /// Só agrupa: não tem chat, voz nem limite de pessoas.
    Category,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OverwriteTarget {
    Role,
    Member,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Overwrite {
    pub target_type: OverwriteTarget,
    pub target_id: i64,
    pub allow: i64,
    pub deny: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Channel {
    /// ULID: o canal é o único que não usa inteiro.
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ChannelKind,
    pub topic: Option<String>,
    pub position: i64,
    pub user_limit: Option<i64>,
    /// A categoria em que o canal está; `None` é a raiz do servidor.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Mensagens de outras pessoas depois da marca de leitura; zero em categoria.
    #[serde(default)]
    pub unread: i64,
    /// A permissão já calculada pelo Laravel para quem pediu. A interface só esconde botão.
    pub permissions: i64,
    #[serde(default)]
    pub overwrites: Vec<Overwrite>,
}

/// Uma faixa da coluna de canais: a categoria (ou a raiz, sem ela) e os canais dela já na
/// ordem em que se desenham.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelSection {
    pub category: Option<Channel>,
    pub channels: Vec<Channel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Role {
    pub id: i64,
    pub name: String,
    pub color: Option<String>,
    pub position: i64,
    pub permissions: i64,
    #[serde(default)]
    pub is_everyone: bool,
    /// Quem tem o cargo aparece num grupo próprio na lista de membros.
    #[serde(default)]
    pub hoist: bool,
    #[serde(default)]
    pub mentionable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Member {
    pub user_id: i64,
    pub name: String,
    pub avatar_url: Option<String>,
    pub nickname: Option<String>,
    #[serde(default)]
    pub status: UserStatus,
    #[serde(default)]
    pub activity: Option<crate::activity::Activity>,
    #[serde(default)]
    pub role_ids: Vec<i64>,
    #[serde(default)]
    pub server_mute: bool,
    #[serde(default)]
    pub server_deaf: bool,
    #[serde(default)]
    pub is_owner: bool,
    #[serde(flatten)]
    pub profile: Profile,
    /// Quando a conta foi criada e quando entrou neste servidor (ISO 8601).
    #[serde(default)]
    pub member_since: Option<String>,
    #[serde(default)]
    pub joined_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoicePerson {
    pub user_id: i64,
    pub name: String,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub deafened: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub status: UserStatus,
    #[serde(default)]
    pub activity: Option<crate::activity::Activity>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub banner_color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutualServer {
    pub id: i64,
    pub name: String,
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Mutuals {
    #[serde(default)]
    pub mutual_friends: usize,
    #[serde(default)]
    pub mutual_servers: Vec<MutualServer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub id: i64,
    pub channel_id: String,
    /// `user` para o que alguém escreveu; `join` para o aviso de quem entrou no servidor.
    #[serde(rename = "type", default = "user_message")]
    pub kind: String,
    pub user: Person,
    #[serde(default)]
    pub reply_to: Option<ReplyTo>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub files: Vec<MessageFile>,
    pub edited_at: Option<String>,
    pub created_at: String,
}

fn user_message() -> String {
    "user".to_owned()
}

/// A mensagem a que outra responde, já resumida pelo Laravel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplyTo {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageFile {
    pub id: i64,
    pub url: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ban {
    pub user_id: i64,
    pub name: String,
    pub reason: Option<String>,
    pub banned_by: Option<i64>,
    #[serde(default)]
    pub created_at: String,
}

/// Um convite de uma pessoa, como os do Discord (`GET /api/servers/{server}/invites`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerInvite {
    pub id: i64,
    pub code: String,
    pub url: String,
    pub uses: i64,
    /// `None` é sem limite.
    pub max_uses: Option<i64>,
    /// `None` é sem validade.
    pub expires_at: Option<String>,
    pub channel: Option<InviteChannel>,
    pub creator: Option<Person>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvitePreview {
    pub server: InvitePreviewServer,
    pub members: i64,
    pub inviter: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvitePreviewServer {
    pub id: i64,
    pub name: String,
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InviteChannel {
    pub id: String,
    pub name: String,
}

/// Um som do soundboard do servidor. A `url` é pré-assinada e vence em 2 h.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerSound {
    pub id: i64,
    pub name: String,
    pub emoji: Option<String>,
    pub volume: u8,
    pub url: String,
}

/// Uma linha do registro de auditoria. O `summary` já vem em português, montado pelo Laravel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditEntry {
    pub id: String,
    pub at: String,
    pub event: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// `None` quando a conta de quem fez foi apagada.
    pub actor: Option<Person>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Membership {
    pub user_id: i64,
    pub permissions: i64,
    pub top_position: i64,
}

/// A resposta do `GET /api/servers/{server}`: o servidor inteiro numa tacada, já com a
/// permissão de quem pediu calculada pelo Laravel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerTree {
    pub id: i64,
    pub name: String,
    pub owner_id: i64,
    pub invite_code: Option<String>,
    #[serde(default)]
    pub invites_paused: bool,
    pub icon_url: Option<String>,
    pub me: Membership,
    #[serde(default)]
    pub roles: Vec<Role>,
    #[serde(default)]
    pub channels: Vec<Channel>,
    #[serde(default)]
    pub members: Vec<Member>,
    #[serde(default)]
    pub voice: std::collections::HashMap<String, Vec<VoicePerson>>,
    /// Só vem para quem pode banir.
    #[serde(default)]
    pub bans: Vec<Ban>,
}

impl ServerTree {
    /// Os canais na ordem em que a coluna os desenha: texto antes de voz, e dentro de cada
    /// grupo a posição que o servidor mandou.
    ///
    /// Aqui e não em cada interface: escrita três vezes, a mesma lista sairia em três
    /// ordens diferentes no primeiro ajuste.
    pub fn ordered_channels(&self) -> Vec<Channel> {
        let mut ordered: Vec<Channel> = self
            .channels
            .iter()
            .filter(|channel| channel.kind != ChannelKind::Category)
            .cloned()
            .collect();

        ordered.sort_by_key(|channel| (channel.kind == ChannelKind::Voice, channel.position));

        ordered
    }

    /// A coluna como o Discord a desenha: primeiro os canais soltos na raiz, depois cada
    /// categoria pela posição dela, e dentro de cada faixa texto antes de voz. Um canal que
    /// aponta para uma categoria que a pessoa não vê fica na raiz: escondê-lo junto apagaria um
    /// canal que ela pode abrir.
    pub fn sections(&self) -> Vec<ChannelSection> {
        let mut categories: Vec<&Channel> = self
            .channels
            .iter()
            .filter(|channel| channel.kind == ChannelKind::Category)
            .collect();

        categories.sort_by_key(|category| category.position);

        let visible = |parent: &Option<String>| {
            parent.as_ref().is_some_and(|parent| categories.iter().any(|category| &category.id == parent))
        };
        let ordered = self.ordered_channels();
        let mut sections = vec![ChannelSection {
            category: None,
            channels: ordered.iter().filter(|channel| !visible(&channel.parent_id)).cloned().collect(),
        }];

        for category in &categories {
            sections.push(ChannelSection {
                category: Some((*category).clone()),
                channels: ordered
                    .iter()
                    .filter(|channel| channel.parent_id.as_deref() == Some(category.id.as_str()))
                    .cloned()
                    .collect(),
            });
        }

        sections
    }
}

/// O que o `GET /api/config` entrega: onde fica o SFU desta instalação.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub sfu: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthToken {
    pub token: String,
    /// O token que troca o par quando o de acesso vence. Só vem para quem pede (`refresh`).
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub user: Option<User>,
}

/// Daqui para baixo é o que o SFU fala, e ele fala `camelCase`. Misturar com o que vem do
/// Laravel num `rename_all` só do módulo faria um dos dois parar de ler.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProducerInfo {
    pub producer_id: String,
    pub kind: String,
    pub source: String,
    #[serde(default)]
    pub paused: bool,
    /// Public MLS signature key of the producer's session, in base64url. It is untrusted SFU
    /// metadata and is checked against the authenticated MLS roster before deriving a key.
    #[serde(default)]
    pub e2ee_member_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
    pub peer_id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub producers: Vec<ProducerInfo>,
    /// Quem caiu e ainda está na carência: continua na lista, mas não está aqui agora.
    #[serde(default)]
    pub reconnecting: bool,
    #[serde(default, skip_deserializing)]
    pub self_peer: bool,
}

impl Peer {
    /// Transmitir é ter producer de tela. Guardar isso como campo daria duas verdades para
    /// manter em dia — a lista de producers já sabe.
    pub fn sharing(&self) -> bool {
        self.producers
            .iter()
            .any(|producer| producer.source == "screen")
    }
}

/// Quem entra: o visitante da sala por código, sem conta, ou o token curto assinado pelo
/// Laravel para entrar em um canal de voz.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum RoomIdentity {
    #[serde(rename_all = "camelCase")]
    Guest {
        room: String,
        name: String,
        install_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Account {
        token: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinResponse {
    #[serde(default)]
    pub resumed: bool,
    pub peer_id: String,
    /// The authenticated account bound by the SFU, used as the MLS BasicCredential identity.
    #[serde(default)]
    pub user_id: String,
    /// Random identifier for this live Room instance. It changes after the Room empties and is
    /// combined with the room code to derive the MLS group id.
    #[serde(default)]
    pub e2ee_session_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub resume_key: Option<String>,
    #[serde(default)]
    pub peers: Vec<Peer>,
    /// O que esta sessão pode fazer agora, segundo o token que acabou de ser aceito. É ele,
    /// e não os bits do canal, que decide mic, câmera e tela.
    #[serde(default)]
    pub can: Vec<String>,
    /// Há quanto tempo a sala existe, desde a primeira pessoa. Duração, e não hora: o relógio
    /// de cada máquina erra. `None` num SFU de antes do campo.
    #[serde(default)]
    pub elapsed_ms: Option<u64>,
}

/// O que a interface desenha. Uma tela por estado, e a interface nunca inventa um sexto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Entry,
    Hub,
    Offline,
    Updating,
}

impl Screen {
    /// Onde se abre o app depois da atualização: conta vai aos servidores, sem conta ao login.
    pub fn home(signed_in: bool) -> Self {
        if signed_in { Self::Hub } else { Self::Entry }
    }
}

/// O estado de uma amizade. O servidor manda o nome em minúsculas.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FriendshipStatus {
    Pending,
    Accepted,
    Blocked,
}

/// Um pedido de amizade, aceito ou não. `requester` é quem pediu — e é por ele que a tela
/// sabe se o convite é para responder ou para esperar.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Friendship {
    pub id: i64,
    pub status: FriendshipStatus,
    pub requester: Person,
    pub addressee: Person,
    #[serde(default)]
    pub responded_at: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// A última mensagem de uma conversa, que é o que a lista mostra embaixo do nome.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LastMessage {
    pub id: i64,
    pub body: String,
    pub created_at: String,
    pub mine: bool,
}

/// Uma conversa direta na lista: com quem, o que foi dito por último, e quantas faltam ler.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Conversation {
    pub user: Person,
    pub last: LastMessage,
    #[serde(default)]
    pub unread: i64,
}

/// Uma mensagem direta. `mine` vem do servidor: quem é você quem diz é ele.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectMessage {
    pub id: i64,
    pub body: String,
    pub created_at: String,
    #[serde(default)]
    pub edited_at: Option<String>,
    pub mine: bool,
    pub sender: Person,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_channel_reads_the_wire_format() {
        let raw = r#"{
            "id": "01jbqz3h7k9m2n4p6r8t0v1w3x",
            "name": "geral",
            "type": "text",
            "topic": null,
            "position": 0,
            "user_limit": null,
            "permissions": 1024,
            "overwrites": [{"target_type": "role", "target_id": 3, "allow": 1024, "deny": 0}]
        }"#;

        let channel: Channel = serde_json::from_str(raw).expect("parse");

        assert_eq!(channel.kind, ChannelKind::Text);
        assert_eq!(channel.permissions, 1024);
        assert_eq!(channel.overwrites[0].target_type, OverwriteTarget::Role);
    }

    #[test]
    fn missing_optional_fields_do_not_break_parsing() {
        let member: Member = serde_json::from_str(
            r#"{"user_id": 7, "name": "Ada", "avatar_url": null, "nickname": null}"#,
        )
        .expect("parse");

        assert!(member.role_ids.is_empty());
        assert!(!member.is_owner);
        assert_eq!(member.status, UserStatus::Online);
    }

    #[test]
    fn status_is_read_from_the_wire_and_legacy_payloads_default_online() {
        let user: User = serde_json::from_str(r#"{"id":1,"name":"Ada","email":"ada@example.test","avatar_url":null,"status":"dnd","status_until":"2026-10-14T20:00:00Z"}"#).expect("user");
        let old_user: User = serde_json::from_str(r#"{"id":1,"name":"Ada","email":"ada@example.test","avatar_url":null}"#).expect("old user");

        assert_eq!(user.status, UserStatus::Dnd);
        assert_eq!(user.status_until.as_deref(), Some("2026-10-14T20:00:00Z"));
        assert_eq!(old_user.status, UserStatus::Online);
    }

    #[test]
    fn the_sfu_speaks_camel_case_and_the_peer_reads_it() {
        let raw = r#"{
            "peerId": "abc",
            "userId": "user:12",
            "name": "Ada",
            "reconnecting": false,
            "producers": [{"producerId": "p1", "kind": "video", "source": "screen", "paused": false}]
        }"#;

        let peer: Peer = serde_json::from_str(raw).expect("parse");

        assert_eq!(peer.peer_id, "abc");
        assert!(peer.sharing());
    }

    #[test]
    fn a_peer_with_only_a_microphone_is_not_sharing() {
        let raw = r#"{"peerId":"abc","name":"Ada","producers":[{"producerId":"p1","kind":"audio","source":"mic"}]}"#;

        assert!(!serde_json::from_str::<Peer>(raw).expect("parse").sharing());
    }

    #[test]
    fn the_guest_identity_goes_out_as_the_sfu_expects() {
        let identity = RoomIdentity::Guest {
            room: "abc123".into(),
            name: "Ada".into(),
            install_id: "install-1".into(),
        };

        let sent = serde_json::to_value(&identity).expect("serialize");

        assert_eq!(sent["installId"], "install-1");
        assert!(sent.get("token").is_none());
    }

    #[test]
    fn leaving_a_room_lands_where_the_person_came_from() {
        assert_eq!(Screen::home(true), Screen::Hub);
        assert_eq!(Screen::home(false), Screen::Entry);
    }

    #[test]
    fn the_channels_come_out_with_text_first_and_in_position_order() {
        let tree = ServerTree {
            id: 1,
            name: "Casa".into(),
            owner_id: 1,
            invite_code: None,
            invites_paused: false,
            icon_url: None,
            me: Membership {
                user_id: 1,
                permissions: 0,
                top_position: 0,
            },
            roles: Vec::new(),
            members: Vec::new(),
            voice: std::collections::HashMap::new(),
            bans: Vec::new(),
            channels: vec![
                channel("voz", ChannelKind::Voice, 0),
                channel("geral", ChannelKind::Text, 1),
                channel("avisos", ChannelKind::Text, 0),
            ],
        };

        let names: Vec<String> = tree
            .ordered_channels()
            .into_iter()
            .map(|channel| channel.name)
            .collect();

        assert_eq!(names, ["avisos", "geral", "voz"]);
    }

    fn channel(name: &str, kind: ChannelKind, position: i64) -> Channel {
        Channel {
            id: name.into(),
            name: name.into(),
            kind,
            topic: None,
            position,
            user_limit: None,
            parent_id: None,
            unread: 0,
            permissions: 0,
            overwrites: Vec::new(),
        }
    }

    fn inside(mut channel: Channel, parent: &str) -> Channel {
        channel.parent_id = Some(parent.into());

        channel
    }

    #[test]
    fn the_sections_put_the_loose_channels_first_then_each_category_in_order() {
        let tree = ServerTree {
            id: 1,
            name: "Casa".into(),
            owner_id: 1,
            invite_code: None,
            invites_paused: false,
            icon_url: None,
            me: Membership { user_id: 1, permissions: 0, top_position: 0 },
            roles: Vec::new(),
            members: Vec::new(),
            voice: std::collections::HashMap::new(),
            bans: Vec::new(),
            channels: vec![
                channel("projetos", ChannelKind::Category, 1),
                channel("geral", ChannelKind::Category, 0),
                inside(channel("programando", ChannelKind::Voice, 0), "projetos"),
                inside(channel("config-pc", ChannelKind::Text, 1), "projetos"),
                inside(channel("chat", ChannelKind::Text, 0), "geral"),
                channel("regras", ChannelKind::Text, 0),
                inside(channel("oculta", ChannelKind::Text, 1), "categoria-que-nao-vejo"),
            ],
        };
        let sections: Vec<(Option<String>, Vec<String>)> = tree
            .sections()
            .into_iter()
            .map(|section| {
                (section.category.map(|category| category.name), section.channels.into_iter().map(|channel| channel.name).collect())
            })
            .collect();

        assert_eq!(
            sections,
            [
                (None, vec!["regras".to_owned(), "oculta".to_owned()]),
                (Some("geral".to_owned()), vec!["chat".to_owned()]),
                (Some("projetos".to_owned()), vec!["config-pc".to_owned(), "programando".to_owned()]),
            ],
            "a raiz vem antes, a categoria segue a posição, e o canal de categoria escondida não some"
        );
    }

    #[test]
    fn a_category_reads_from_the_wire_and_never_lands_among_the_channels() {
        let raw = r#"{"id":"01jbqz3h7k9m2n4p6r8t0v1w3y","name":"Projetos","type":"category","topic":null,
            "position":2,"user_limit":null,"parent_id":null,"unread":0,"permissions":256}"#;
        let category: Channel = serde_json::from_str(raw).expect("categoria");

        assert_eq!(category.kind, ChannelKind::Category);
    }

    #[test]
    fn the_channel_kind_round_trips_as_lowercase() {
        let voice = serde_json::to_string(&ChannelKind::Voice).expect("serialize");

        assert_eq!(voice, "\"voice\"");
    }

    /// As três respostas do Laravel, copiadas da API no ar. O que este teste protege é o
    /// formato: se um `Resource` mudar de campo, ele cai aqui e não na tela de alguém.
    #[test]
    fn friendship_conversation_and_direct_message_read_what_the_server_sends() {
        let friendship: Friendship = serde_json::from_str(
            r#"{"id":1,"status":"accepted",
                "requester":{"id":14,"name":"edson","avatar_url":null},
                "addressee":{"id":15,"name":"maria","avatar_url":null},
                "responded_at":"2026-09-21T09:44:52-03:00","created_at":"2026-09-21T09:44:20-03:00"}"#,
        )
        .expect("amizade");

        assert_eq!(friendship.status, FriendshipStatus::Accepted);
        assert_eq!(friendship.addressee.name, "maria");

        let conversation: Conversation = serde_json::from_str(
            r#"{"user":{"id":15,"name":"maria","avatar_url":null},
                "last":{"id":1,"body":"Oi","created_at":"2026-09-21T09:44:52-03:00","mine":true},
                "unread":0}"#,
        )
        .expect("conversa");

        assert_eq!(conversation.user.id, 15);
        assert!(conversation.last.mine);

        let direct: DirectMessage = serde_json::from_str(
            r#"{"id":1,"body":"Oi","created_at":"2026-09-21T09:44:52-03:00","edited_at":null,
                "mine":true,"sender":{"id":14,"name":"edson","avatar_url":null}}"#,
        )
        .expect("mensagem direta");

        assert_eq!(direct.sender.name, "edson");
        assert_eq!(direct.edited_at, None);
    }

    /// O pedido de amizade que ainda não foi respondido: é por `status` que a tela sabe se
    /// mostra "aceitar" ou "aguardando".
    #[test]
    fn a_pending_friendship_has_no_answer_yet() {
        let friendship: Friendship = serde_json::from_str(
            r#"{"id":1,"status":"pending",
                "requester":{"id":14,"name":"edson","avatar_url":null},
                "addressee":{"id":15,"name":"maria","avatar_url":null},
                "responded_at":null,"created_at":"2026-09-21T09:44:20-03:00"}"#,
        )
        .expect("amizade");

        assert_eq!(friendship.status, FriendshipStatus::Pending);
        assert_eq!(friendship.responded_at, None);
    }

    /// A linha da auditoria de quem apagou a conta chega sem `actor`, e o `type` do Laravel
    /// vira `kind`.
    #[test]
    fn an_audit_line_without_its_author_still_reads() {
        let entry: AuditEntry = serde_json::from_str(
            r#"{"id":"c3","at":"2026-10-14T08:00:00-03:00","event":"deleted","type":"Channel",
                "actor":null,"summary":"apagou o canal #geral"}"#,
        )
        .expect("auditoria");

        assert_eq!((entry.kind.as_str(), entry.actor), ("Channel", None));
    }
}
