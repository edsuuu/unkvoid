import SwiftUI

/// `modals/ServerSettingsModal.tsx`: a barra lateral com as abas que esta pessoa pode ver e,
/// ao lado, a aba aberta. No rodapé, sair (ou excluir, se for o dono) e salvar o nome.
struct ServerSettingsModal: View {
    @EnvironmentObject private var model: AppModel

    @State private var tab = Tab.overview
    @State private var name = ""

    private enum Tab: String, CaseIterable {
        case overview = "Visão geral"
        case members = "Membros"
        case roles = "Cargos"
        case bans = "Banidos"
        case audit = "Auditoria"

        var icon: IconName {
            switch self {
            case .overview: .gear
            case .members: .users
            case .roles: .crown
            case .bans: .logout
            case .audit: .logs
            }
        }

        /// O que é preciso poder para a aba aparecer.
        var needs: String? {
            switch self {
            case .overview, .members: nil
            case .roles: "manageRoles"
            case .bans: "banMembers"
            case .audit: "viewAuditLog"
            }
        }
    }

    var body: some View {
        if let tree = model.tree {
            ModalFrame(title: "Configurações do servidor", subtitle: tree.name, width: 760, onClose: { model.modal = nil }) {
                HStack(alignment: .top, spacing: 20) {
                    VStack(spacing: 4) {
                        ForEach(allowed, id: \.self) { item in
                            Button {
                                tab = item
                            } label: {
                                HStack(spacing: 10) {
                                    Icon(name: item.icon, size: 14)

                                    Text(item.rawValue)
                                        .font(Theme.sans(12.5))
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                }
                                .foregroundStyle(shown == item ? Theme.inkStrong : Theme.inkIcon)
                                .rowItem(selected: shown == item)
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .frame(width: 170)

                    Rectangle().fill(Theme.line).frame(width: 1)

                    VStack(alignment: .leading, spacing: 0) {
                        switch shown {
                        case .overview: OverviewTab(name: $name)
                        case .members: MembersTab()
                        case .roles: RolesTab()
                        case .bans: BansTab()
                        case .audit: AuditTab()
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                }
                .frame(minHeight: 320, alignment: .top)
            } footer: {
                Button(model.abilities.owner ? "Excluir servidor" : "Sair do servidor") {
                    model.abilities.owner ? model.deleteServer() : model.leaveServer()
                }
                .buttonStyle(.plain)
                .font(Theme.sans(12.5))
                .foregroundStyle(Theme.inkDim)

                Spacer(minLength: 0)

                if model.abilities.allows("manageServer") {
                    Button("Salvar") {
                        Task { await save(tree) }
                    }
                    .buttonStyle(PrimaryButton(wide: false, font: Theme.sans(13, .semibold)))
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                } else {
                    Button("Fechar") {
                        model.modal = nil
                    }
                    .buttonStyle(GhostButton())
                }
            }
            .onAppear { name = tree.name }
        }
    }

    private var allowed: [Tab] {
        Tab.allCases.filter { $0.needs.map(model.abilities.allows) ?? true }
    }

    private var shown: Tab {
        allowed.contains(tab) ? tab : .overview
    }

    private func save(_ tree: ServerTree) async {
        let wanted = name.trimmingCharacters(in: .whitespaces)

        if wanted == tree.name {
            model.modal = nil
        } else if await model.renameServer(wanted) {
            model.modal = nil
        }
    }
}

private struct OverviewTab: View {
    @EnvironmentObject private var model: AppModel

    @Binding var name: String

    @State private var busy = false
    @FocusState private var naming: Bool

    var body: some View {
        if let tree = model.tree {
            let manage = model.abilities.allows("manageServer")

            VStack(alignment: .leading, spacing: 8) {
                Text("Ícone e nome").labelMono()

                HStack(spacing: 12) {
                    Button {
                        busy = true

                        Task {
                            await model.changeServerIcon()
                            busy = false
                        }
                    } label: {
                        Avatar(name: tree.name, url: tree.icon_url, size: 56, mine: true, square: true)
                            .overlay {
                                if busy {
                                    ProgressView().controlSize(.small)
                                }
                            }
                    }
                    .buttonStyle(.plain)
                    .disabled(!manage || busy)
                    .help(manage ? "Trocar o ícone do servidor" : "Só quem gerencia o servidor troca o ícone")

                    VStack(alignment: .leading, spacing: 6) {
                        TextField("", text: $name)
                            .field(focused: naming)
                            .focused($naming)
                            .disabled(!manage)
                            .onChange(of: name) { name = String(name.prefix(60)) }

                        if manage, tree.icon_url != nil {
                            Button("Remover o ícone") {
                                Task { await model.removeServerIcon() }
                            }
                            .buttonStyle(.plain)
                            .font(Theme.sans(11.5))
                            .foregroundStyle(Theme.inkDim)
                        }
                    }
                }

                if let invite = tree.invite_code {
                    Text("Convite").labelMono()
                        .padding(.top, 16)

                    HStack(spacing: 8) {
                        Text(invite)
                            .codeChip()
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .textSelection(.enabled)

                        Button {
                            model.copyInvite()
                        } label: {
                            HStack(spacing: 6) {
                                Icon(name: .copy, size: 13)

                                Text("Copiar")
                            }
                        }
                        .buttonStyle(GhostButton())

                        if model.abilities.allows("createInvite") {
                            Button("Regenerar") {
                                Task { await model.renewInvite() }
                            }
                            .buttonStyle(GhostButton())
                            .help("Gera um código novo e invalida o antigo")
                        }
                    }
                }
            }
        }
    }
}

private struct MembersTab: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        if let tree = model.tree {
            let members = tree.members.sorted { $0.displayName.localizedCaseInsensitiveCompare($1.displayName) == .orderedAscending }

            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    Text("Membros").labelMono()

                    Spacer(minLength: 0)

                    Text("\(members.count)")
                        .font(Theme.mono(10))
                        .foregroundStyle(Theme.inkDim)
                }
                .padding(.bottom, 2)

                ForEach(members) { member in
                    Button {
                        model.memberMenu = member
                    } label: {
                        row(member, in: tree)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private func row(_ member: Member, in tree: ServerTree) -> some View {
        let me = member.user_id == model.user?.id
        let top = tree.topRole(of: member)

        return HStack(spacing: 10) {
            Avatar(name: member.displayName, url: member.avatar_url, size: 26, mine: me, status: model.online.contains("\(member.user_id)"))

            Text(member.displayName)
                .font(Theme.sans(13))
                .foregroundStyle(me ? Theme.inkStrong : Theme.inkIcon)
                .lineLimit(1)

            if me {
                Text("você")
                    .font(Theme.mono(9.5))
                    .foregroundStyle(Theme.inkDim)
            }

            Spacer(minLength: 0)

            if member.server_mute {
                Icon(name: .micOff, size: 13).foregroundStyle(Theme.danger).help("Mutado no servidor")
            }

            if member.server_deaf {
                Icon(name: .headphonesOff, size: 13).foregroundStyle(Theme.danger).help("Ensurdecido no servidor")
            }

            Circle()
                .fill(Theme.hex(top?.color ?? tree.everyone?.color) ?? Theme.inkDim)
                .frame(width: 7, height: 7)

            Text(member.is_owner ? "dono" : top?.name ?? tree.everyone?.name ?? "@everyone")
                .font(Theme.mono(9.5))
                .foregroundStyle(member.is_owner ? Theme.lilac2 : Theme.inkDim)

            Icon(name: .dots, size: 13).foregroundStyle(Theme.inkDim)
        }
        .rowItem()
        .contentShape(Rectangle())
    }
}

private struct RolesTab: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        if let tree = model.tree {
            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    Text("Cargos").labelMono()

                    Spacer(minLength: 0)

                    Button("Novo cargo") {
                        model.roleEditor = RoleEditor(role: nil)
                    }
                    .buttonStyle(GhostButton(font: Theme.sans(11.5), padding: EdgeInsets(top: 4, leading: 8, bottom: 4, trailing: 8)))
                }
                .padding(.bottom, 2)

                ForEach(model.abilities.roles, id: \.id) { row in
                    if let role = tree.roles.first(where: { $0.id == row.id }) {
                        line(role, row)
                    }
                }
            }
        }
    }

    private func line(_ role: Role, _ row: Abilities.RoleRow) -> some View {
        HStack(spacing: 8) {
            Circle()
                .fill(Theme.hex(role.color) ?? Theme.inkDim)
                .frame(width: 8, height: 8)

            Text(role.name)
                .font(Theme.sans(13))
                .foregroundStyle(Theme.inkBody)
                .lineLimit(1)

            Text("· \(role.position)")
                .font(Theme.mono(10))
                .foregroundStyle(Theme.inkDim)

            Spacer(minLength: 0)

            if let up = row.up {
                small("↑", "Subir") { await model.moveRole(role, to: up) }
            }

            if let down = row.down {
                small("↓", "Descer") { await model.moveRole(role, to: down) }
            }

            if row.editable {
                small("Editar", "Editar") { model.roleEditor = RoleEditor(role: role) }
            }

            if row.editable, role.is_everyone != true {
                small("Apagar", "Apagar") { model.deleteRole(role) }
            }
        }
        .rowItem()
    }

    private func small(_ label: String, _ hint: String, _ action: @escaping @MainActor () async -> Void) -> some View {
        Button(label) {
            Task { await action() }
        }
        .buttonStyle(GhostButton(font: Theme.sans(11.5), padding: EdgeInsets(top: 4, leading: 8, bottom: 4, trailing: 8)))
        .help(hint)
    }
}

private struct BansTab: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        let bans = model.tree?.bans ?? []

        VStack(alignment: .leading, spacing: 6) {
            Text("Banidos").labelMono()
                .padding(.bottom, 2)

            if bans.isEmpty {
                Text("Ninguém banido.")
                    .font(Theme.sans(12.5))
                    .foregroundStyle(Theme.inkDim)
            }

            ForEach(bans) { ban in
                HStack(spacing: 10) {
                    Text(ban.name)
                        .font(Theme.sans(13))
                        .foregroundStyle(Theme.inkBody)
                        .lineLimit(1)
                        .frame(maxWidth: .infinity, alignment: .leading)

                    Text(ban.reason ?? "")
                        .font(Theme.sans(12))
                        .foregroundStyle(Theme.inkDim)
                        .lineLimit(1)
                        .frame(maxWidth: .infinity, alignment: .leading)

                    Button("Perdoar") {
                        Task { await model.unban(ban) }
                    }
                    .buttonStyle(GhostButton(font: Theme.sans(11.5), padding: EdgeInsets(top: 4, leading: 8, bottom: 4, trailing: 8)))
                }
                .rowItem()
            }
        }
    }
}

private struct AuditTab: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Auditoria").labelMono()
                .padding(.bottom, 2)

            if model.auditsLoading {
                ForEach(0 ..< 3, id: \.self) { _ in Skeleton(height: 40) }
            } else if model.auditsFailed {
                VStack(spacing: 8) {
                    Text("Não deu para carregar a auditoria.")
                        .font(Theme.sans(13))
                        .foregroundStyle(Theme.danger)

                    Button("Tentar de novo") {
                        Task { await model.loadAudits() }
                    }
                    .buttonStyle(GhostButton())
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 24)
            } else if model.audits.isEmpty {
                Text("Nada registrado ainda.")
                    .font(Theme.sans(12.5))
                    .foregroundStyle(Theme.inkDim)
            }

            ForEach(model.audits) { entry in
                HStack(alignment: .top, spacing: 10) {
                    Avatar(name: entry.actor?.name ?? "?", url: entry.actor?.avatar_url, size: 26)

                    VStack(alignment: .leading, spacing: 2) {
                        (Text(entry.actor?.name ?? "alguém").fontWeight(.medium) + Text(" \(entry.summary)"))
                            .font(Theme.sans(12.5))
                            .foregroundStyle(Theme.inkBody)
                            .lineLimit(2)

                        Text(Clock.short(entry.at))
                            .font(Theme.mono(9.5))
                            .foregroundStyle(Theme.inkDim)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .rowItem()
            }
        }
        .task { await model.loadAudits() }
    }
}
