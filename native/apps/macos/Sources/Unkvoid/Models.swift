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

    var isVoice: Bool {
        type == "voice"
    }
}

struct Role: Decodable, Sendable, Identifiable, Equatable {
    let id: Int
    let name: String
    let color: String?
    let position: Int
    let is_everyone: Bool?
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
    let sources: [String]?
    let muted: Bool?

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
    let voice: [String: [VoicePerson]]?

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

struct Message: Decodable, Sendable, Identifiable, Equatable {
    let id: Int
    let channel_id: String
    let user: Person
    let body: String
    let edited_at: String?
    let created_at: String
}
