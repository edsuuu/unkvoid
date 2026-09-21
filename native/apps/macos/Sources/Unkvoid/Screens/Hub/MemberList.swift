import SwiftUI

/// `ui/components/hub/MemberList.tsx`: 208 de largura, `glass p-3.5`, agrupada pelo cargo
/// mais alto de cada um e com o cargo dando a cor do grupo.
/// Quem está com o app aberto vem primeiro, e o resto vai para "Offline" no fim — a presença
/// é a do tempo real (`server.<id>`).
struct MemberList: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        if let tree = model.tree {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    ForEach(groups(of: tree), id: \.name) { group in
                        VStack(alignment: .leading, spacing: 8) {
                            Text("\(group.name) — \(group.members.count)")
                                .labelMono()
                                .foregroundStyle(group.color ?? Theme.inkDim)

                            VStack(spacing: 2) {
                                ForEach(group.members) { member in
                                    row(member, tint: group.color, tree: tree)
                                }
                            }
                        }
                    }
                }
                .padding(14)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .scrollIndicators(.never)
            .frame(width: 208)
            .glass()
        }
    }

    private func row(_ member: Member, tint: Color?, tree: ServerTree) -> some View {
        Button {
            model.memberMenu = member
        } label: {
            line(member, tint: tint)
        }
        .buttonStyle(.pointer)
        .help("Ações do membro")
    }

    private func line(_ member: Member, tint: Color?) -> some View {
        HStack(spacing: 10) {
            Avatar(name: member.displayName, url: member.avatar_url, size: 24, mine: member.user_id == model.user?.id, status: model.online.contains("\(member.user_id)"))

            Text(member.displayName)
                .font(Theme.sans(12.5))
                .foregroundStyle(tint ?? Theme.inkBody)
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)

            if member.is_owner {
                Icon(name: .crown, size: 12)
                    .foregroundStyle(Theme.lilac2)
                    .help("Dono do servidor")
            }
        }
        .rowItem()
        .contentShape(Rectangle())
    }

    private struct Group {
        let name: String
        let color: Color?
        var members: [Member]
    }

    /// Cada um no grupo do seu cargo mais alto, os cargos de cima para baixo, e os nomes em
    /// ordem dentro de cada grupo — a mesma ordem do `Members.group` do React.
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

        let everyone = loose.isEmpty ? [] : [Group(name: tree.everyone?.name ?? "Membros", color: nil, members: loose)]
        let away = offline.isEmpty ? [] : [Group(name: "Offline", color: nil, members: offline)]

        return ranked + everyone + away
    }
}
