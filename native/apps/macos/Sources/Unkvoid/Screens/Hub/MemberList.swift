import SwiftUI

/// A lista de membros: 240 de largura, `surfaceSide`, agrupada pelo cargo mais alto de cada um
/// (os cargos de cima para baixo), depois "ONLINE" e "OFFLINE". A presença é a do tempo real
/// (`server.<id>`). Quem está offline fica a 30%.
struct MemberList: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        if let tree = model.tree {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(groups(of: tree), id: \.name) { group in
                        Text("\(group.name) — \(group.members.count)")
                            .labelMono()
                            .foregroundStyle(group.color ?? Theme.inkDim)
                            .lineLimit(1)
                            .padding(.top, 24)
                            .padding(.horizontal, 16)
                            .padding(.bottom, 4)

                        ForEach(group.members) { member in
                            row(member, tint: group.color, offline: group.offline, tree: tree)
                        }
                    }
                }
                .padding(.horizontal, 8)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .scrollIndicators(.never)
            .frame(width: Theme.Size.members)
            .background(Theme.surfaceSide)
        }
    }

    private func row(_ member: Member, tint: Color?, offline: Bool, tree: ServerTree) -> some View {
        Button {
            model.memberMenu = member
        } label: {
            HStack(spacing: 12) {
                Avatar(name: member.displayName, url: member.avatar_url, size: 32, mine: member.user_id == model.user?.id, status: !offline)

                VStack(alignment: .leading, spacing: 0) {
                    Text(member.displayName)
                        .font(Theme.list)
                        .foregroundStyle(offline ? Theme.inkDim : tint ?? Theme.ink)
                        .lineLimit(1)

                    if let voice = voiceChannel(of: member, in: tree) {
                        Text("Na voz: \(voice.name)")
                            .font(Theme.meta)
                            .foregroundStyle(Theme.inkDim)
                            .lineLimit(1)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                if member.is_owner {
                    Icon(name: .crown, size: 14)
                        .foregroundStyle(Theme.idle)
                        .help("Dono do servidor")
                }
            }
            .frame(height: Theme.Size.memberRow - 12)
            .rowItem()
            .contentShape(Rectangle())
        }
        .buttonStyle(.pointer)
        .opacity(offline ? 0.3 : 1)
        .help("Ações do membro")
        .contextMenu {
            MemberMenuItems(member: member)
        }
    }

    private func voiceChannel(of member: Member, in tree: ServerTree) -> Channel? {
        guard let id = tree.voice?.first(where: { $0.value.contains { $0.user_id == member.user_id } })?.key else {
            return nil
        }

        return tree.channels.first { $0.id == id }
    }

    private struct Group {
        let name: String
        let color: Color?
        var offline = false
        var members: [Member]
    }

    /// Cada um no grupo do seu cargo mais alto, os cargos de cima para baixo, e os nomes em
    /// ordem dentro de cada grupo.
    private func groups(of tree: ServerTree) -> [Group] {
        var byRole: [Int: Group] = [:]
        var loose: [Member] = []
        var offline: [Member] = []

        for member in tree.members.sorted(by: { $0.displayName.localizedCompare($1.displayName) == .orderedAscending }) {
            guard model.online.contains("\(member.user_id)") || member.user_id == model.user?.id else {
                offline.append(member)

                continue
            }

            guard let role = tree.topRole(of: member) else {
                loose.append(member)

                continue
            }

            byRole[role.id, default: Group(name: role.name, color: Theme.hex(role.color), members: [])].members.append(member)
        }

        let ranked = byRole
            .sorted { left, right in
                let leftPosition = tree.roles.first { $0.id == left.key }?.position ?? 0
                let rightPosition = tree.roles.first { $0.id == right.key }?.position ?? 0

                return leftPosition > rightPosition
            }
            .map(\.value)

        let everyone = loose.isEmpty ? [] : [Group(name: "Online", color: nil, members: loose)]
        let away = offline.isEmpty ? [] : [Group(name: "Offline", color: nil, offline: true, members: offline)]

        return ranked + everyone + away
    }
}

/// O menu de contexto de um membro, nativo: o caminho pelo teclado para o que a janelinha do
/// membro também faz. "Mover para" lista as vozes que a pessoa vê, menos a atual.
struct MemberMenuItems: View {
    @EnvironmentObject private var model: AppModel

    let member: Member

    var body: some View {
        let actions = model.abilities.actions(on: member)
        let destinations = model.moveDestinations(for: member)

        Button("Perfil e ações") { model.memberMenu = member }

        if actions.mute || actions.deafen || actions.disconnect {
            Divider()
        }

        if actions.mute {
            Button(member.server_mute ? "Desmutar no servidor" : "Mutar no servidor") {
                Task { await model.updateMember(member, ["server_mute": !member.server_mute]) }
            }
        }

        if actions.deafen {
            Button(member.server_deaf ? "Voltar a ouvir no servidor" : "Ensurdecer no servidor") {
                Task { await model.updateMember(member, ["server_deaf": !member.server_deaf]) }
            }
        }

        if !destinations.isEmpty {
            Menu("Mover para") {
                ForEach(destinations) { channel in
                    Button(channel.name) { Task { await model.moveToVoice(member, to: channel) } }
                }
            }
        }

        if actions.disconnect {
            Button("Desconectar") { Task { await model.disconnectFromVoice(member) } }
        }

        if actions.kick || actions.ban {
            Divider()
        }

        if actions.kick {
            Button("Expulsar \(member.displayName)", role: .destructive) { model.kick(member) }
        }

        if actions.ban {
            Button("Banir \(member.displayName)", role: .destructive) {
                model.confirm("Banir \(member.displayName)? A pessoa não consegue voltar até ser perdoada.", "Banir") {
                    await model.ban(member, reason: "")
                }
            }
        }
    }
}
