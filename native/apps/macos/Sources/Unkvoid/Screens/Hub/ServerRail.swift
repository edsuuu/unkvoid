import SwiftUI

/// `ui/components/hub/ServerRail.tsx`: a coluna dos servidores, 182 aberta e 58 fechada,
/// raio 18, com o botão de recolher em cima.
struct ServerRail: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                Button {
                    withAnimation(.easeOut(duration: 0.2)) { model.railOpen.toggle() }
                } label: {
                    Icon(name: .menu, size: 15)
                }
                .buttonStyle(IconButton())
                .help(model.railOpen ? "Recolher servidores" : "Expandir servidores")

                RailRow(active: model.home || model.tree == nil, label: "Home") {
                    Task { await model.showHome() }
                } badge: {
                    Icon(name: .home, size: 16)
                        .foregroundStyle(model.home || model.tree == nil ? Theme.inkStrong : Theme.inkIcon)
                        .frame(width: 34, height: 34)
                        .background(
                            model.home || model.tree == nil ? AnyShapeStyle(Theme.brandGradient) : AnyShapeStyle(Theme.chrome),
                            in: RoundedRectangle(cornerRadius: 11, style: .continuous)
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: 11, style: .continuous)
                                .strokeBorder(model.home || model.tree == nil ? Theme.brand.opacity(0.6) : Theme.lineStrong, lineWidth: 1)
                        )
                }

                Rectangle()
                    .fill(Theme.lineStrong)
                    .frame(height: 1)

                if model.serversLoading, model.servers.isEmpty {
                    ForEach(0 ..< 3, id: \.self) { _ in
                        RoundedRectangle(cornerRadius: 11, style: .continuous)
                            .fill(Color.white.opacity(0.07))
                            .frame(width: 34, height: 34)
                    }
                }

                ForEach(model.servers) { server in
                    let active = !model.home && model.tree?.id == server.id

                    RailRow(active: active, label: server.name) {
                        Task { await model.openServer(server.id) }
                    } badge: {
                        Avatar(name: server.name, url: server.icon_url, size: 34, mine: active, square: true)
                            .overlay(
                                RoundedRectangle(cornerRadius: 11, style: .continuous)
                                    .strokeBorder(active ? .clear : Color.white.opacity(0.08), lineWidth: 1)
                            )
                    }
                }
            }
            .padding(.vertical, 8)
            .padding(.horizontal, model.railOpen ? 8 : 12)
        }
        .scrollIndicators(.never)
        .frame(width: model.railOpen ? 182 : 58)
        .glass(radius: 18)
    }
}

/// A linha da trilha: o quadradinho e, quando ela está aberta, o nome ao lado.
private struct RailRow<Badge: View>: View {
    var active: Bool
    var label: String
    var action: () -> Void
    @ViewBuilder var badge: Badge

    @EnvironmentObject private var model: AppModel

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                badge

                if model.railOpen {
                    Text(label)
                        .font(Theme.sans(13))
                        .foregroundStyle(Theme.inkBody)
                        .lineLimit(1)
                        .truncationMode(.tail)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(label)
    }
}
