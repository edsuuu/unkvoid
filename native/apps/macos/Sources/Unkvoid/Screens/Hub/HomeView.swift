import SwiftUI

/// `ui/components/hub/home/ServersHome.tsx`: os cartões de vidro lado a lado — a sala por
/// código, com as últimas acessadas, e a lista das salas com conta.
///
/// O cartão de criar servidor do React não está aqui: criar servidor é uma ação que a ABI
/// do núcleo ainda não tem. Ver o relatório no `README.md` desta pasta.
struct HomeView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        ScrollView {
            HStack(alignment: .top, spacing: 12) {
                roomByCode

                servers
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .scrollIndicators(.never)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var roomByCode: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Só compartilhar a tela").labelMono()

            Text("Uma sala por código, sem servidor: quem tiver o código assiste.")
                .font(Theme.sans(13))
                .foregroundStyle(Theme.inkSoft)
                .fixedSize(horizontal: false, vertical: true)

            Button("Criar ou entrar com código") {
                model.openEntry()
            }
            .buttonStyle(PrimaryButton())

            if !model.recentRooms.isEmpty {
                Text("Últimas salas acessadas").labelMono()

                FlowRow(spacing: 8) {
                    ForEach(model.recentRooms, id: \.self) { code in
                        Button(code) {
                            Task { await model.openRoom(code) }
                        }
                        .buttonStyle(GhostButton(
                            font: Theme.mono(12.5),
                            padding: EdgeInsets(top: 6, leading: 12, bottom: 6, trailing: 12)
                        ))
                    }
                }
            }
        }
        .padding(20)
        .frame(width: 360, alignment: .leading)
        .glass()
    }

    private var servers: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Últimas salas").labelMono()
                .padding(.bottom, 4)

            if model.serversLoading, model.servers.isEmpty {
                ForEach(0 ..< 3, id: \.self) { _ in Skeleton(height: 48) }
            }

            if !model.serversLoading, model.servers.isEmpty {
                Text(model.serversFailed ? "Não deu para carregar as suas salas." : "Nenhuma ainda. Entre com um convite pelo site.")
                    .font(Theme.sans(13))
                    .foregroundStyle(model.serversFailed ? Theme.danger : Theme.inkDim)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .padding(.vertical, 24)
            }

            ForEach(model.servers) { server in
                Button {
                    Task { await model.openServer(server.id) }
                } label: {
                    HStack(spacing: 10) {
                        Avatar(name: server.name, url: server.icon_url, size: 32, square: true)

                        VStack(alignment: .leading, spacing: 2) {
                            Text(server.name)
                                .font(Theme.sans(13.5, .semibold))
                                .foregroundStyle(Theme.ink)
                                .lineLimit(1)

                            Text(server.owner_id == model.user?.id ? "dono" : "membro").labelMono()
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .rowItem()
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass()
    }
}

/// As fichas das salas recentes quebram linha quando não cabem — é o `flex-wrap` do React.
struct FlowRow: Layout {
    var spacing: CGFloat = 8

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        let rows = lines(subviews, within: width)
        let height = rows.reduce(0) { $0 + $1.height + spacing } - (rows.isEmpty ? 0 : spacing)

        return CGSize(width: width == .infinity ? rows.map(\.width).max() ?? 0 : width, height: max(0, height))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY

        for row in lines(subviews, within: bounds.width) {
            var x = bounds.minX

            for index in row.items {
                let size = subviews[index].sizeThatFits(.unspecified)

                subviews[index].place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
                x += size.width + spacing
            }

            y += row.height + spacing
        }
    }

    private struct Line {
        var items: [Int] = []
        var width: CGFloat = 0
        var height: CGFloat = 0
    }

    private func lines(_ subviews: Subviews, within width: CGFloat) -> [Line] {
        var rows: [Line] = []
        var current = Line()

        for index in subviews.indices {
            let size = subviews[index].sizeThatFits(.unspecified)

            if !current.items.isEmpty, current.width + spacing + size.width > width {
                rows.append(current)
                current = Line()
            }

            current.width += (current.items.isEmpty ? 0 : spacing) + size.width
            current.height = max(current.height, size.height)
            current.items.append(index)
        }

        return current.items.isEmpty ? rows : rows + [current]
    }
}
