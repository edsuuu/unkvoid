import SwiftUI

/// O trilho dos servidores: 72 de largura, ícones de 48, a pílula branca à esquerda que cresce
/// conforme o estado (8 não lida, 20 sob o mouse, 40 ativo). A Home em cima, uma divisória, os
/// servidores, e o "+" no fim.
struct ServerRail: View {
    @EnvironmentObject private var model: AppModel

    private var homeActive: Bool {
        model.home || model.tree == nil
    }

    var body: some View {
        ScrollView {
            VStack(spacing: 8) {
                RailItem(active: homeActive, label: "Mensagens diretas") {
                    Task { await model.showHome() }
                } badge: { lit in
                    Icon(name: .home, size: 24)
                        .foregroundStyle(lit ? .white : Theme.inkSoft)
                        .frame(width: Theme.Size.serverIcon, height: Theme.Size.serverIcon)
                        .background(lit ? Theme.brand : Theme.surfaceChat, in: RoundedRectangle(cornerRadius: lit ? 16 : 24, style: .continuous))
                }

                Rectangle()
                    .fill(Theme.line)
                    .frame(width: 32, height: 2)

                if model.serversLoading, model.servers.isEmpty {
                    ForEach(0 ..< 3, id: \.self) { _ in
                        Circle()
                            .fill(Theme.surfaceChat)
                            .frame(width: Theme.Size.serverIcon, height: Theme.Size.serverIcon)
                    }
                }

                ForEach(model.servers) { server in
                    let active = !model.home && model.tree?.id == server.id

                    RailItem(active: active, label: server.name) {
                        Task { await model.openServer(server.id) }
                    } badge: { lit in
                        Avatar(name: server.name, url: server.icon_url, size: Theme.Size.serverIcon, mine: lit, corner: lit ? 16 : 24)
                    }
                    .contextMenu {
                        ServerMenuItems(server: server)
                    }
                }

                RailItem(active: false, label: "Adicionar um servidor") {
                    model.modal = .invite
                } badge: { lit in
                    Icon(name: .plus, size: 24)
                        .foregroundStyle(lit ? .white : Theme.online)
                        .frame(width: Theme.Size.serverIcon, height: Theme.Size.serverIcon)
                        .background(lit ? Theme.online : Theme.surfaceChat, in: RoundedRectangle(cornerRadius: lit ? 16 : 24, style: .continuous))
                }
            }
            .padding(.vertical, 12)
        }
        .scrollIndicators(.never)
        .frame(width: Theme.Size.rail)
        .background(Theme.surfaceRail)
    }
}

/// Uma linha do trilho: a pílula à esquerda e o ícone de 48 no meio. `lit` é "ativo ou sob o
/// mouse": é quando o raio vira 16 e o fundo acende.
private struct RailItem<Badge: View>: View {
    var active: Bool
    var label: String
    var action: () -> Void
    @ViewBuilder var badge: (Bool) -> Badge

    @State private var hovering = false

    var body: some View {
        ZStack(alignment: .leading) {
            RoundedRectangle(cornerRadius: 2, style: .continuous)
                .fill(Theme.inkStrong)
                .frame(width: 4, height: active ? 40 : hovering ? 20 : 0)
                .animation(.easeOut(duration: 0.15), value: active)
                .animation(.easeOut(duration: 0.15), value: hovering)

            Button(action: action) {
                badge(active || hovering)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.pointer)
            .frame(maxWidth: .infinity)
            .onHover { hovering = $0 }
            .animation(.easeOut(duration: 0.15), value: hovering)
            .help(label)
            .accessibilityLabel(label)
        }
        .frame(width: Theme.Size.rail, height: Theme.Size.serverIcon)
    }
}

/// O menu de um servidor, no trilho e no cabeçalho da coluna de canais. O que aparece é o que
/// o núcleo disse que a pessoa pode fazer; quem autoriza é o Laravel.
struct ServerMenuItems: View {
    @EnvironmentObject private var model: AppModel

    let server: ServerSummary

    private var open: Bool {
        model.tree?.id == server.id
    }

    var body: some View {
        if open, model.abilities.allows("createInvite") {
            Button("Convidar pessoas") { model.modal = .invitePeople }
        }

        if open, model.abilities.allows("manageServer") || model.abilities.allows("manageRoles") || model.abilities.owner {
            Button("Configurações do servidor") { model.modal = .serverSettings }
        }

        if open, model.abilities.allows("manageChannels") {
            Button("Criar canal") { model.channelEditor = ChannelEditor(channel: nil, kind: "text") }
            Button("Criar categoria") { model.channelEditor = ChannelEditor(channel: nil, kind: "category") }
        }

        if !open {
            Button("Abrir servidor") { Task { await model.openServer(server.id) } }
        }

        if open, !model.abilities.owner {
            Divider()

            Button("Sair do servidor", role: .destructive) { model.leaveServer() }
        }
    }
}
