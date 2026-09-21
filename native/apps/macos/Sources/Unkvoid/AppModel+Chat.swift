import Foundation

/// O chat em tempo real: o que o SFU repassa do Laravel para os canais em que se está.
///
/// Inscrição é do socket. Ele cai e volta sozinho dentro do núcleo (`realtime.lost`,
/// `realtime.back`), e na volta esta classe se apresenta e se inscreve de novo.
extension AppModel {
    /// Apresenta esta conta ao SFU e entra no canal dela (`user.<id>`), por onde chega o
    /// que é só desta pessoa — ser removida de um servidor, por exemplo.
    func connectChat() async {
        guard let user, await ask("identify")["ok"] as? Bool == true else {
            return
        }

        SystemNotices.ask()

        await follow("user.\(user.id)")
        await loadSocial()

        if let tree {
            await follow("server.\(tree.id)")

            followedVoice = []

            await followVoiceChannels()
        }

        for room in [chat, voiceChat] {
            if let channel = room.channel {
                await follow("channel.\(channel.id)")
            }
        }
    }

    /// Entra num canal do tempo real. A resposta de um canal de servidor traz quem está
    /// online agora.
    func follow(_ channel: String) async {
        let answer = await send("subscribe", ["channel": channel])

        if channel.hasPrefix("server."), let members = answer["members"] as? [[String: Any]] {
            online = Set(members.compactMap { $0["id"].map { "\($0)" } })
        }
    }

    func unfollow(_ channel: String) async {
        _ = await send("unsubscribe", ["channel": channel])
    }

    func heardFromChat(_ event: [String: Any]) {
        let data = event["data"] as? [String: Any] ?? [:]
        let from = event["channel"] as? String

        switch event["event"] as? String {
        case "realtime.back":
            Task { await connectChat() }
        case let name? where name.hasPrefix("Message"):
            if !chat.heard(name, from: from, data) {
                _ = voiceChat.heard(name, from: from, data)
            }
        case "VoiceStateUpdated":
            voiceChanged(data)
        case "ServerUpdated":
            Task { await reloadTree() }
        case "MemberRemoved":
            Task { await removedFromServer(data["server_id"] as? Int) }
        case "presence.joining":
            if from?.hasPrefix("server.") == true, let id = data["id"] {
                online.insert("\(id)")
            }
        case "presence.leaving":
            if from?.hasPrefix("server.") == true, let id = data["id"] {
                online.remove("\(id)")
            }
        case let name?:
            heardFromSocial(name, data)
        default:
            break
        }
    }

    /// Segue cada canal de voz do servidor aberto, e larga os que sumiram. É por `channel.<id>`
    /// que o Laravel avisa quem entrou e saiu da voz.
    func followVoiceChannels() async {
        let visible = Set(tree?.voiceChannels.map(\.id) ?? [])

        for gone in followedVoice.subtracting(visible) where gone != voiceChannel?.id {
            await unfollow("channel.\(gone)")
        }

        for fresh in visible.subtracting(followedVoice) {
            await follow("channel.\(fresh)")
        }

        followedVoice = visible
    }

    /// Alguém entrou ou saiu de um canal de voz: a lista embaixo do canal muda na hora.
    private func voiceChanged(_ data: [String: Any]) {
        guard let channel = data["channel_id"] as? String, let person = data["user_id"] as? Int, tree?.channels.contains(where: { $0.id == channel }) == true else {
            return
        }

        var people = (tree?.voice?[channel] ?? []).filter { $0.user_id != person }

        if data["event"] as? String == "joined" {
            people.append(VoicePerson(user_id: person, name: data["name"] as? String ?? "", sources: [], muted: false))
        }

        var voice = tree?.voice ?? [:]

        voice[channel] = people
        tree?.voice = voice
    }

    /// Quem aparece embaixo de um canal de voz. No canal em que se está, a lista vem da sala —
    /// você primeiro, e cada pessoa com o que está mandando —, que é mais rápida e mais exata
    /// do que esperar o Laravel saber. Nos outros, vem da árvore do servidor.
    func voicePeople(in channel: Channel) -> [VoicePerson] {
        let known = tree?.voice?[channel.id] ?? []

        guard voiceChannel?.id == channel.id, let user else {
            return known
        }

        let me = VoicePerson(user_id: user.id, name: user.name, sources: mySources, muted: !mine.mic || mine.micMuted)
        let others = peers.filter { !$0.selfPeer }.compactMap { peer -> VoicePerson? in
            guard let account = peer.userId, account.hasPrefix("user:"), let id = Int(account.dropFirst(5)) else {
                return nil
            }

            return VoicePerson(user_id: id, name: peer.name, sources: peer.producers.map(\.source), muted: peer.micOff)
        }

        // Entrando, a sala ainda não respondeu: vale o que a árvore sabia, sem repetir ninguém.
        return [me] + (peers.isEmpty ? known.filter { $0.user_id != user.id } : others)
    }

    private var mySources: [String] {
        (mine.sharing ? ["screen"] : []) + (mine.camera ? ["camera"] : []) + (mine.mic ? ["mic"] : [])
    }

    /// O servidor mudou (canal novo, cargo, alguém entrou na voz): a árvore é pequena, e
    /// buscá-la de novo é mais simples do que aplicar cada mudança à mão.
    func reloadTree() async {
        guard let id = tree?.id else {
            return
        }

        let answer = await ask("server", ["id": id])

        guard let fresh: ServerTree = decode(answer["server"]) else {
            return
        }

        tree = fresh
        abilities = decode(answer["abilities"]) ?? abilities

        await followVoiceChannels()

        await chat.refresh(from: fresh)
        await voiceChat.refresh(from: fresh)
    }

    private func removedFromServer(_ id: Int?) async {
        await loadServers()

        if tree?.id == id {
            tree = nil
            await showHome()
            say("Você não está mais nesse servidor.")
        }
    }

    /// Uma ação do SFU no socket do tempo real (`unkvoid_call`), fora da thread que desenha.
    private func send(_ action: String, _ data: [String: Any]) async -> [String: Any] {
        guard let core else {
            return [:]
        }

        let payload = JSONPayload(data)

        return await offMain { JSONPayload((try? core.call(action, payload.value)) ?? [:]) }.value
    }
}
