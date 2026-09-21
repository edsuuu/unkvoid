//! O que o Laravel e o SFU dizem, do jeito que as três interfaces vão ler.
//!
//! Os nomes dos campos são os que trafegam na rede — por isso `snake_case` e por isso
//! `avatar_url` e não `avatarUrl`. Renomear aqui quebraria o contrato de `docs/CONTRATO.md`
//! sem avisar ninguém.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub avatar_uploaded: bool,
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub nickname_confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerSummary {
    pub id: i64,
    pub name: String,
    pub owner_id: i64,
    pub icon_url: Option<String>,
    pub last_accessed_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChannelKind {
    Text,
    Voice,
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
    /// A permissão já calculada pelo Laravel para quem pediu. A interface só esconde botão.
    pub permissions: i64,
    #[serde(default)]
    pub overwrites: Vec<Overwrite>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Member {
    pub user_id: i64,
    pub name: String,
    pub avatar_url: Option<String>,
    pub nickname: Option<String>,
    #[serde(default)]
    pub role_ids: Vec<i64>,
    #[serde(default)]
    pub server_mute: bool,
    #[serde(default)]
    pub server_deaf: bool,
    #[serde(default)]
    pub is_owner: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoicePerson {
    pub user_id: i64,
    pub name: String,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub muted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub avatar_url: Option<String>,
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
        let mut ordered = self.channels.clone();

        ordered.sort_by_key(|channel| (channel.kind == ChannelKind::Voice, channel.position));

        ordered
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

/// Como este app se apresenta ao SFU: sem conta vale o código e o nome digitado; com conta
/// vale o token de 60 s que o Laravel assinou.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum RoomIdentity {
    #[serde(rename_all = "camelCase")]
    Guest {
        room: String,
        name: String,
        install_id: String,
    },
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
}

/// O que a interface desenha. Uma tela por estado, e a interface nunca inventa um sexto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Entry,
    Hub,
    Room,
    Offline,
    Updating,
}

impl Screen {
    /// Onde se cai ao sair de uma sala, e onde se abre o app depois da atualização. Quem
    /// tem conta volta para os servidores; quem não tem volta para o código.
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
            permissions: 0,
            overwrites: Vec::new(),
        }
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
}
