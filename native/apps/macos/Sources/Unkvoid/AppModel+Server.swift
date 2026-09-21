import AppKit
import Foundation
import UniformTypeIdentifiers

struct Confirmation: Identifiable {
    let id = UUID()
    let question: String
    let action: String
    let confirmed: @MainActor () async -> Void
}

/// O canal que o modal está editando; `channel == nil` é canal novo, do tipo `kind`.
struct ChannelEditor: Identifiable {
    let id = UUID()
    let channel: Channel?
    let kind: String
}

struct RoleEditor: Identifiable {
    let id = UUID()
    let role: Role?
}

/// Escrever no servidor aberto: nome, ícone, convite, canais, cargos, membros, banidos e
/// auditoria. Depois de cada escrita a árvore é buscada de novo — o `ServerUpdated` do
/// tempo real faria o mesmo, mas quem clicou não espera por ele.
extension AppModel {
    /// Os seis bits que se sobrescrevem por canal, com o rótulo curto da grade.
    static let overwritable: [(bit: Int, label: String)] = [
        (1 << 8, "ver"), (1 << 9, "falar"), (1 << 11, "entrar"), (1 << 12, "voz"), (1 << 13, "tela"), (1 << 14, "cam"),
    ]

    static let permissionLabels: [(bit: Int, label: String)] = [
        (1 << 0, "Administrador"), (1 << 1, "Gerenciar servidor"), (1 << 2, "Gerenciar cargos"),
        (1 << 3, "Gerenciar canais"), (1 << 4, "Expulsar membros"), (1 << 5, "Banir membros"),
        (1 << 6, "Criar convite"), (1 << 7, "Ver auditoria"), (1 << 8, "Ver canais"),
        (1 << 9, "Enviar mensagens"), (1 << 10, "Gerenciar mensagens"), (1 << 11, "Conectar na voz"),
        (1 << 12, "Falar"), (1 << 13, "Compartilhar tela"), (1 << 14, "Câmera"),
        (1 << 15, "Mutar membros"), (1 << 16, "Ensurdecer membros"), (1 << 17, "Desconectar da voz"),
    ]

    func confirm(_ question: String, _ action: String, _ confirmed: @escaping @MainActor () async -> Void) {
        confirmation = Confirmation(question: question, action: action, confirmed: confirmed)
    }

    func renameServer(_ name: String) async -> Bool {
        guard let tree, await api("updateServer", ["server": tree.id], body: ["name": name]) != nil else {
            return false
        }

        await reloadTree()
        await loadServers()

        return true
    }

    /// Escolhe uma imagem no disco. Devolve `nil` se a pessoa desistiu ou se passou do tamanho.
    func pickImage(limit megabytes: Int) -> URL? {
        let panel = NSOpenPanel()

        panel.allowedContentTypes = [.png, .jpeg, .webP, .gif]
        panel.allowsMultipleSelection = false

        guard panel.runModal() == .OK, let url = panel.url else {
            return nil
        }

        let size = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0

        guard size <= megabytes * 1024 * 1024 else {
            say("a imagem precisa ter menos de \(megabytes) MB")

            return nil
        }

        return url
    }

    func upload(_ name: String, _ params: [String: Any], field: String, files: [URL], fields: [String: String] = [:]) async -> Any? {
        let answer = await ask("upload", ["name": name, "params": params, "field": field, "files": files.map(\.path), "fields": fields])

        guard answer["ok"] as? Bool == true else {
            warn(answer)

            return nil
        }

        return answer["data"] ?? NSNull()
    }

    func changeServerIcon() async {
        guard let tree, let file = pickImage(limit: 2) else {
            return
        }

        if await upload("uploadServerIcon", ["server": tree.id], field: "icon", files: [file]) != nil {
            await reloadTree()
            await loadServers()
        }
    }

    func removeServerIcon() async {
        guard let tree, await api("deleteServerIcon", ["server": tree.id]) != nil else {
            return
        }

        await reloadTree()
        await loadServers()
    }

    func renewInvite() async {
        guard let tree, await api("renewInvite", ["server": tree.id]) != nil else {
            return
        }

        await reloadTree()
    }

    func deleteServer() {
        guard let tree else {
            return
        }

        confirm("Apagar o servidor \"\(tree.name)\"? Isso não tem volta.", "Apagar") { [self] in
            await leaveOrDelete("deleteServer", tree.id)
        }
    }

    func leaveServer() {
        guard let tree else {
            return
        }

        confirm("Sair do servidor \"\(tree.name)\"? Para voltar, só com um convite.", "Sair") { [self] in
            await leaveOrDelete("leaveServer", tree.id)
        }
    }

    private func leaveOrDelete(_ route: String, _ server: Int) async {
        guard await api(route, ["server": server]) != nil else {
            return
        }

        await leaveVoice()

        modal = nil
        tree = nil

        await showHome()
        await loadServers()
    }

    func loadAudits() async {
        guard let tree else {
            return
        }

        audits = []
        auditsLoading = true

        let page = await api("audits", ["server": tree.id], quiet: true)

        auditsLoading = false
        auditsFailed = page == nil
        audits = decode((page as? [String: Any])?["data"] ?? page) ?? []
    }

    func unban(_ ban: Ban) async {
        guard let tree, await api("unban", ["server": tree.id, "user": ban.user_id]) != nil else {
            return
        }

        await reloadTree()
    }

    func moveRole(_ role: Role, to position: Int) async {
        if await api("updateRole", ["role": role.id], body: ["position": position]) != nil {
            await reloadTree()
        }
    }

    func saveRole(_ role: Role?, name: String, color: String, permissions: Int) async -> Bool {
        guard let tree else {
            return false
        }

        let everyone = role?.is_everyone == true
        let wanted = name.trimmingCharacters(in: .whitespacesAndNewlines)

        guard everyone || !wanted.isEmpty else {
            say("Dê um nome ao cargo.")

            return false
        }

        let body: [String: Any] = everyone ? ["permissions": permissions] : ["name": wanted, "color": color, "permissions": permissions]
        let saved = role.map { ("updateRole", ["role": $0.id]) } ?? ("createRole", ["server": tree.id])

        guard await api(saved.0, saved.1, body: body) != nil else {
            return false
        }

        await reloadTree()

        return true
    }

    func deleteRole(_ role: Role) {
        confirm("Apagar o cargo \"\(role.name)\"?", "Apagar") { [self] in
            if await api("deleteRole", ["role": role.id]) != nil {
                roleEditor = nil

                await reloadTree()
            }
        }
    }

    func saveChannel(_ channel: Channel?, name: String, kind: String, topic: String, limit: String) async -> Bool {
        guard let tree else {
            return false
        }

        let wanted = name.trimmingCharacters(in: .whitespacesAndNewlines)

        guard !wanted.isEmpty else {
            say("Dê um nome ao canal.")

            return false
        }

        let people = limit.trimmingCharacters(in: .whitespaces)

        guard people.isEmpty || (1 ... 99).contains(Int(people) ?? 0) else {
            say("O limite de pessoas vai de 1 a 99. Vazio é sem limite.")

            return false
        }

        var body: [String: Any] = [
            "name": wanted,
            "topic": topic.trimmingCharacters(in: .whitespaces).isEmpty ? NSNull() : topic.trimmingCharacters(in: .whitespaces),
            "user_limit": Int(people).map { $0 as Any } ?? NSNull(),
        ]

        if channel == nil {
            body["type"] = kind
        }

        let saved = channel.map { ("updateChannel", ["channel": $0.id]) } ?? ("createChannel", ["server": tree.id])

        guard await api(saved.0, saved.1, body: body) != nil else {
            return false
        }

        await reloadTree()

        return true
    }

    func deleteChannel(_ channel: Channel) {
        confirm("Apagar o canal \"\(channel.name)\"? As mensagens vão junto.", "Apagar") { [self] in
            if await api("deleteChannel", ["channel": channel.id]) != nil {
                channelEditor = nil

                await reloadTree()
            }
        }
    }

    /// Grava na hora o que uma célula da grade de "ocultar canal" passou a valer. Os dois
    /// zerados é o mesmo que não ter sobrescrita, e aí ela é apagada.
    func putOverwrite(_ channel: Channel, type: String, id: Int, allow: Int, deny: Int) async -> Bool {
        let params: [String: Any] = ["channel": channel.id, "type": type, "id": id]
        let saved = allow == 0 && deny == 0
            ? await api("deleteOverwrite", params)
            : await api("putOverwrite", params, body: ["allow": allow, "deny": deny])

        guard saved != nil else {
            return false
        }

        await reloadTree()

        return true
    }

    func updateMember(_ member: Member, _ body: [String: Any]) async {
        guard let tree, await api("updateMember", ["server": tree.id, "user": member.user_id], body: body) != nil else {
            return
        }

        await reloadTree()

        memberMenu = self.tree?.members.first { $0.user_id == member.user_id }
    }

    func disconnectFromVoice(_ member: Member) async {
        guard let channel = tree?.voice?.first(where: { $0.value.contains { $0.user_id == member.user_id } })?.key else {
            return
        }

        if await api("disconnectFromVoice", ["channel": channel, "user": member.user_id]) != nil {
            memberMenu = nil
        }
    }

    func kick(_ member: Member) {
        guard let tree else {
            return
        }

        confirm("Expulsar \(member.displayName) do servidor? A pessoa pode voltar com um convite.", "Expulsar") { [self] in
            if await api("kickMember", ["server": tree.id, "user": member.user_id]) != nil {
                memberMenu = nil

                await reloadTree()
            }
        }
    }

    func ban(_ member: Member, reason: String) async {
        guard let tree else {
            return
        }

        let why = reason.trimmingCharacters(in: .whitespacesAndNewlines)

        if await api("ban", ["server": tree.id, "user": member.user_id], body: why.isEmpty ? [:] : ["reason": why]) != nil {
            memberMenu = nil

            await reloadTree()
        }
    }

    func copyInvite() {
        if let code = tree?.invite_code {
            copy(code, "Convite copiado")
        }
    }
}
