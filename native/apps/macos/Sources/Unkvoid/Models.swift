import Foundation

/// O que o núcleo devolve, do jeito que a tela lê.
///
/// Os nomes dos campos são os que trafegam na rede, os mesmos de `shared/core/src/models.rs`
/// — por isso `avatar_url` e não `avatarUrl`. Renomear aqui quebraria a leitura em silêncio.
struct User: Decodable, Sendable, Equatable {
    let id: Int
    let name: String
    let email: String
    let avatar_url: String?
    /// A foto foi enviada pela pessoa (e dá para remover), e não herdada do Google.
    let avatar_uploaded: Bool?
    /// Conta nova nasce com um apelido tirado do e-mail, e a pessoa confirma ou troca.
    let nickname_confirmed: Bool?
}

struct ServerSummary: Decodable, Sendable, Identifiable, Equatable {
    let id: Int
    let name: String
    let owner_id: Int
    let icon_url: String?
    let last_accessed_at: String?
}

struct Channel: Decodable, Sendable, Identifiable, Equatable {
    let id: String
    let name: String
    let type: String
    let topic: String?
    let position: Int
    /// Os bits efetivos desta pessoa neste canal, já calculados pelo Laravel.
    let permissions: Int?
    let user_limit: Int?
    let overwrites: [Overwrite]?

    var isVoice: Bool {
        type == "voice"
    }
}

struct Overwrite: Decodable, Sendable, Equatable {
    let target_type: String
    let target_id: Int
    var allow: Int
    var deny: Int
}

struct Role: Decodable, Sendable, Identifiable, Equatable {
    let id: Int
    let name: String
    let color: String?
    let position: Int
    let permissions: Int?
    let is_everyone: Bool?
}

struct Ban: Decodable, Sendable, Identifiable, Equatable {
    let user_id: Int
    let name: String
    let reason: String?

    var id: Int {
        user_id
    }
}

struct Audit: Decodable, Sendable, Identifiable, Equatable {
    let id: String
    let at: String
    let actor: Person?
    let summary: String
}

/// O que o núcleo calculou que esta pessoa pode fazer no servidor aberto (`permissions.rs`).
/// A tela só esconde botão com isto; quem autoriza é o Laravel, em toda chamada.
struct Abilities: Decodable, Sendable, Equatable {
    struct MemberActions: Decodable, Sendable, Equatable {
        var nickname = false
        var mute = false
        var deafen = false
        var disconnect = false
        var kick = false
        var ban = false
        var roles = false

        var any: Bool {
            nickname || mute || deafen || disconnect || kick || ban || roles
        }
    }

    struct RoleRow: Decodable, Sendable, Equatable {
        let id: Int
        let editable: Bool
        let assignable: Bool
        let up: Int?
        let down: Int?
    }

    var can: [String] = []
    var owner = false
    var members: [String: MemberActions] = [:]
    var roles: [RoleRow] = []

    func allows(_ what: String) -> Bool {
        can.contains(what)
    }

    func actions(on member: Member) -> MemberActions {
        members["\(member.user_id)"] ?? MemberActions()
    }
}

struct Member: Decodable, Sendable, Identifiable, Equatable {
    let user_id: Int
    let name: String
    let avatar_url: String?
    let nickname: String?
    let role_ids: [Int]
    let server_mute: Bool
    let server_deaf: Bool
    let is_owner: Bool

    var id: Int {
        user_id
    }

    var displayName: String {
        nickname ?? name
    }
}

struct VoicePerson: Decodable, Sendable, Identifiable, Equatable {
    let user_id: Int
    let name: String
    var sources: [String]?
    var muted: Bool?

    var id: Int {
        user_id
    }
}

struct Membership: Decodable, Sendable, Equatable {
    let user_id: Int
    let permissions: Int
}

struct ServerTree: Decodable, Sendable, Identifiable, Equatable {
    let id: Int
    let name: String
    let owner_id: Int
    let invite_code: String?
    let icon_url: String?
    let me: Membership
    let roles: [Role]
    let channels: [Channel]
    let members: [Member]
    var voice: [String: [VoicePerson]]?
    let bans: [Ban]?

    var textChannels: [Channel] {
        channels.filter { !$0.isVoice }.sorted { $0.position < $1.position }
    }

    var voiceChannels: [Channel] {
        channels.filter(\.isVoice).sorted { $0.position < $1.position }
    }

    var everyone: Role? {
        roles.first { $0.is_everyone == true }
    }

    /// O cargo mais alto do membro que não seja o `@everyone` — é dele a cor do nome.
    func topRole(of member: Member) -> Role? {
        roles
            .filter { $0.is_everyone != true && member.role_ids.contains($0.id) }
            .max { $0.position < $1.position }
    }
}

struct Person: Decodable, Sendable, Equatable {
    let id: Int
    let name: String
    let avatar_url: String?
}

struct ReplyTo: Decodable, Sendable, Equatable {
    let id: Int
    let name: String
    let body: String
}

struct MessageFile: Decodable, Sendable, Identifiable, Equatable {
    let id: Int
    let url: String
}

struct Message: Decodable, Sendable, Identifiable, Equatable {
    let id: Int
    let channel_id: String
    /// `user` para o que alguém escreveu; `join` para o aviso de quem chegou no servidor.
    let type: String?
    let user: Person
    let reply_to: ReplyTo?
    let files: [MessageFile]?
    let body: String
    let edited_at: String?
    let created_at: String
}

/// A sala aberta, como o núcleo a anuncia (`room.peers`, `room.tiles`, `room.mine`). Aqui os
/// nomes são os do SFU, em camelCase — outra rede, outra convenção.
struct RoomProducer: Decodable, Sendable, Equatable {
    let producerId: String
    let kind: String
    let source: String
    let paused: Bool
}

struct RoomPeer: Decodable, Sendable, Identifiable, Equatable {
    let peerId: String
    let userId: String?
    let name: String
    let producers: [RoomProducer]
    let reconnecting: Bool
    let selfPeer: Bool

    var id: String {
        peerId
    }

    var sharing: Bool {
        producers.contains { $0.source == "screen" }
    }

    /// Sem microfone aberto ou com ele pausado, a sala o desenha fechado.
    var micOff: Bool {
        producers.first { $0.source == "mic" }?.paused ?? true
    }
}

struct RoomTile: Decodable, Sendable, Identifiable, Equatable {
    let producerId: String
    let peerId: String
    let label: String
    let camera: Bool
    /// A própria tela, em "ver o que a sala vê".
    let mine: Bool?
    let paused: Bool?
    /// O producer do som que acompanha esta tela, quando há.
    let audio: String?

    var id: String {
        producerId
    }
}

struct Mine: Decodable, Sendable, Equatable {
    var sharing = false
    var selfView: Bool? = false
    var mic = false
    var micMuted = false
    var camera = false
    var canShare = false
    var canSpeak = false
    var canVideo = false
}

/// Uma tela ou janela que dá para compartilhar, do jeito que o seletor a mostra.
struct ShareSource: Identifiable, Sendable, Equatable {
    /// `display:<id>` ou `window:<id>`: é o que volta para o núcleo na ação `share`.
    let id: String
    let label: String
    let detail: String
}

struct Friendship: Decodable, Sendable, Identifiable, Equatable {
    let id: Int
    let status: String
    let requester: Person
    let addressee: Person
}

struct DirectMessage: Decodable, Sendable, Identifiable, Equatable {
    let id: Int
    let body: String
    let created_at: String
    let edited_at: String?
    let sender: Person
}

struct DirectConversation: Decodable, Sendable, Identifiable, Equatable {
    struct Last: Decodable, Sendable, Equatable {
        let id: Int
        let body: String
        let mine: Bool
    }

    let user: Person
    var last: Last?
    var unread: Int

    var id: Int {
        user.id
    }
}

enum HomeTab {
    case servers
    case friends
}
