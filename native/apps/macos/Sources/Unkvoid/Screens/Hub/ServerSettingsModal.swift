import SwiftUI

/// `ui/components/hub/modals/ServerSettingsModal.tsx`: a navegação de abas à esquerda e o
/// conteúdo à direita, numa caixa de 760 e 320 de altura mínima.
///
/// Só as abas que leem: visão geral, membros e cargos. Renomear, trocar o ícone, regenerar
/// o convite, criar cargo, expulsar, banir e a auditoria são escritas, e a ABI do núcleo
/// ainda não tem nenhuma delas — ver o relatório no `README.md` desta pasta.
struct ServerSettingsModal: View {
    @EnvironmentObject private var model: AppModel
    @State private var tab = Tab.overview

    private enum Tab: CaseIterable {
        case overview
        case members
        case roles

        var label: String {
            switch self {
            case .overview: "Visão geral"
            case .members: "Membros"
            case .roles: "Cargos"
            }
        }

        var icon: IconName {
            switch self {
            case .overview: .gear
            case .members: .users
            case .roles: .crown
            }
        }
    }

    var body: some View {
        if let tree = model.tree {
            ModalFrame(
                title: "Configurações do servidor",
                subtitle: tree.name,
                width: 760,
                onClose: { model.modal = nil }
            ) {
                HStack(alignment: .top, spacing: 20) {
                    VStack(spacing: 4) {
                        ForEach(Tab.allCases, id: \.self) { item in
                            Button {
                                tab = item
                            } label: {
                                HStack(spacing: 10) {
                                    Icon(name: item.icon, size: 14)

                                    Text(item.label)
                                        .font(Theme.sans(12.5))

                                    Spacer(minLength: 0)
                                }
                                .foregroundStyle(tab == item ? Theme.inkStrong : Theme.inkIcon)
                                .rowItem(selected: tab == item)
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }

                        Spacer(minLength: 0)
                    }
                    .frame(width: 170)

                    Rectangle().fill(Theme.line).frame(width: 1)

                    content(tree)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(minHeight: 320, alignment: .top)
            } footer: {
                Spacer(minLength: 0)

                Button("Fechar") {
                    model.modal = nil
                }
                .buttonStyle(GhostButton())
                .fixedSize()
            }
        }
    }

    @ViewBuilder
    private func content(_ tree: ServerTree) -> some View {
        switch tab {
        case .overview:
            VStack(alignment: .leading, spacing: 0) {
                Text("Ícone e nome").labelMono()
                    .padding(.bottom, 8)

                HStack(spacing: 12) {
                    Avatar(name: tree.name, url: tree.icon_url, size: 56, mine: true, square: true)

                    Text(tree.name)
                        .font(Theme.sans(14))
                        .foregroundStyle(Theme.inkStrong)
                        .padding(.vertical, 11)
                        .padding(.horizontal, 13)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Theme.fieldFill, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .strokeBorder(Theme.fieldLine, lineWidth: 1)
                        )
                }

                if let invite = tree.invite_code {
                    Text("Convite").labelMono()
                        .padding(.top, 24)
                        .padding(.bottom, 8)

                    HStack(spacing: 8) {
                        Text(invite)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .codeChip()

                        Button {
                            model.copy(invite, "Convite copiado")
                        } label: {
                            HStack(spacing: 6) {
                                Icon(name: .copy, size: 13)

                                Text("Copiar")
                            }
                        }
                        .buttonStyle(GhostButton())
                        .fixedSize()
                    }
                }
            }
        case .members:
            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    Text("Membros").labelMono()

                    Spacer(minLength: 0)

                    Text("\(tree.members.count)")
                        .font(Theme.mono(10))
                        .foregroundStyle(Theme.inkDim)
                }
                .padding(.bottom, 2)

                ForEach(tree.members.sorted(by: { $0.displayName.localizedCompare($1.displayName) == .orderedAscending })) { member in
                    HStack(spacing: 10) {
                        Avatar(name: member.displayName, url: member.avatar_url, size: 26, mine: member.user_id == model.user?.id)

                        Text(member.displayName)
                            .font(Theme.sans(13))
                            .foregroundStyle(member.user_id == model.user?.id ? Theme.inkStrong : Theme.inkIcon)
                            .lineLimit(1)
                            .frame(maxWidth: .infinity, alignment: .leading)

                        if member.server_mute {
                            Icon(name: .micOff, size: 13).foregroundStyle(Theme.danger)
                        }

                        if member.server_deaf {
                            Icon(name: .headphonesOff, size: 13).foregroundStyle(Theme.danger)
                        }

                        Circle()
                            .fill(Theme.hex(tree.topRole(of: member)?.color ?? tree.everyone?.color) ?? Theme.inkDim)
                            .frame(width: 7, height: 7)

                        Text(member.is_owner ? "dono" : tree.topRole(of: member)?.name ?? tree.everyone?.name ?? "@everyone")
                            .font(Theme.mono(9.5))
                            .foregroundStyle(member.is_owner ? Theme.lilac2 : Theme.inkDim)
                    }
                    .rowItem()
                }
            }
        case .roles:
            VStack(alignment: .leading, spacing: 6) {
                Text("Cargos").labelMono()
                    .padding(.bottom, 2)

                ForEach(tree.roles.sorted { $0.position > $1.position }) { role in
                    HStack(spacing: 10) {
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
                    }
                    .rowItem()
                }
            }
        }
    }
}
