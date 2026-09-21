import Foundation

/// Amigos e mensagens diretas: o que a Home mostra além dos servidores. Tudo chega pelo
/// canal `user.<id>` do tempo real; o que está aqui é só guardar o que chegou.
extension AppModel {
    /// Uma rota do Laravel pelo nome (`routes.rs` do núcleo). Devolve o `data`, ou `nil`
    /// depois de avisar a pessoa do porquê.
    @discardableResult
    func api(_ name: String, _ params: [String: Any] = [:], body: [String: Any]? = nil, quiet: Bool = false) async -> Any? {
        var request: [String: Any] = ["name": name, "params": params]

        if let body {
            request["body"] = body
        }

        let answer = await ask("api", request)

        guard answer["ok"] as? Bool == true else {
            if !quiet {
                warn(answer)
            }

            return nil
        }

        return answer["data"] ?? NSNull()
    }

    func loadSocial() async {
        friendsLoading = true

        let list = await api("friends", quiet: true)

        friendsLoading = false
        friendsFailed = list == nil
        friends = decode(list) ?? []

        await loadConversations()
    }

    func loadConversations() async {
        let list = await api("conversations", quiet: true)

        conversationsFailed = list == nil
        conversations = decode(list) ?? []
    }

    func other(in friendship: Friendship) -> Person {
        friendship.requester.id == user?.id ? friendship.addressee : friendship.requester
    }

    var incomingFriends: [Friendship] {
        friends.filter { $0.status == "pending" && $0.addressee.id == user?.id }
    }

    var outgoingFriends: [Friendship] {
        friends.filter { $0.status == "pending" && $0.requester.id == user?.id }
    }

    func requestFriend(_ email: String) async -> Bool {
        guard let added: Friendship = decode(await api("addFriend", body: ["email": email])) else {
            return false
        }

        keep(added)

        return true
    }

    func answer(_ friendship: Friendship, with action: String) async {
        if let updated: Friendship = decode(await api("answerFriend", ["friendship": friendship.id], body: ["action": action])) {
            keep(updated)
        }
    }

    func remove(_ friendship: Friendship) async {
        if await api("removeFriend", ["friendship": friendship.id]) != nil {
            friends.removeAll { $0.id == friendship.id }
        }
    }

    private func keep(_ friendship: Friendship) {
        if let index = friends.firstIndex(where: { $0.id == friendship.id }) {
            friends[index] = friendship
        } else {
            friends.insert(friendship, at: 0)
        }
    }

    func openDirect(with person: Person) async {
        home = true
        directPerson = person
        directMessages = []
        directLoading = true

        let list = await api("directMessages", ["user": person.id], quiet: true)

        guard directPerson?.id == person.id else {
            return
        }

        directLoading = false
        directFailed = list == nil
        directMessages = decode(list) ?? []

        await markRead(person)
    }

    func closeDirect() {
        directPerson = nil
        directMessages = []
    }

    func sendDirect(_ body: String) async -> Bool {
        guard let person = directPerson, let sent: DirectMessage = decode(await api("sendDirect", ["user": person.id], body: ["body": body])) else {
            return false
        }

        received(sent, with: person)

        return true
    }

    func editDirect(_ message: DirectMessage, to body: String) async -> Bool {
        guard let edited: DirectMessage = decode(await api("editDirect", ["message": message.id], body: ["body": body])) else {
            return false
        }

        if let index = directMessages.firstIndex(where: { $0.id == edited.id }) {
            directMessages[index] = edited
        }

        return true
    }

    func deleteDirect(_ message: DirectMessage) async {
        if await api("deleteDirect", ["message": message.id]) != nil {
            directMessages.removeAll { $0.id == message.id }
        }
    }

    /// O que o canal `user.<id>` traz para a Home.
    func heardFromSocial(_ name: String, _ data: [String: Any]) {
        switch name {
        case "FriendshipUpdated":
            guard let friendship: Friendship = decode(data["friendship"]) else {
                return
            }

            if data["removed"] as? Bool == true {
                friends.removeAll { $0.id == friendship.id }

                return
            }

            let known = friends.contains { $0.id == friendship.id }

            keep(friendship)

            if !known, friendship.status == "pending", friendship.addressee.id == user?.id {
                say("\(friendship.requester.name) quer ser seu amigo")
                SystemNotices.show("Pedido de amizade", "\(friendship.requester.name) quer ser seu amigo")
            }
        case "DirectMessageCreated", "DirectMessageUpdated":
            guard let message: DirectMessage = decode(data["message"]), let recipient: Person = decode(data["recipient"]) else {
                return
            }

            let mine = message.sender.id == user?.id
            let person = mine ? recipient : message.sender

            if name == "DirectMessageUpdated" {
                if let index = directMessages.firstIndex(where: { $0.id == message.id }) {
                    directMessages[index] = message
                }

                return
            }

            received(message, with: person)

            if !mine, noticePreferences.sounds {
                Sounds.message()
            }

            if !mine, directPerson?.id != person.id, noticePreferences.directMessages {
                say("\(message.sender.name): \(message.body.prefix(60))")
                SystemNotices.show(message.sender.name, String(message.body.prefix(120)))
            }
        case "DirectMessageDeleted":
            directMessages.removeAll { $0.id == data["id"] as? Int }
        default:
            break
        }
    }

    /// Uma mensagem direta, de quem quer que tenha vindo: entra na conversa aberta (uma vez
    /// só — a resposta do envio e o tempo real trazem a mesma) e sobe a conversa na lista.
    private func received(_ message: DirectMessage, with person: Person) {
        let mine = message.sender.id == user?.id
        let open = directPerson?.id == person.id

        if open, !directMessages.contains(where: { $0.id == message.id }) {
            directMessages.append(message)
        }

        var conversation = conversations.first { $0.id == person.id } ?? DirectConversation(user: person, last: nil, unread: 0)

        guard conversation.last?.id != message.id else {
            return
        }

        conversation.last = .init(id: message.id, body: message.body, mine: mine)
        conversation.unread = open || mine ? 0 : conversation.unread + 1
        conversations.removeAll { $0.id == person.id }
        conversations.insert(conversation, at: 0)

        if open, !mine {
            Task { await markRead(person) }
        }
    }

    private func markRead(_ person: Person) async {
        if let index = conversations.firstIndex(where: { $0.id == person.id }) {
            conversations[index].unread = 0
        }

        await api("readDirect", ["user": person.id], quiet: true)
    }

    func createServer(named name: String) async -> Bool {
        guard let created: ServerSummary = decode(await api("createServer", body: ["name": name])) else {
            return false
        }

        await loadServers()
        await openServer(created.id)

        inviteBanner = tree?.invite_code != nil

        return true
    }

    func acceptInvite(_ code: String) async -> Bool {
        guard let joined: ServerSummary = decode(await api("acceptInvite", ["code": code])) else {
            return false
        }

        await loadServers()
        await openServer(joined.id)

        return true
    }
}
