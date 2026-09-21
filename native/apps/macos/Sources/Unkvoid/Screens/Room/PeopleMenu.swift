import SwiftUI

/// `PeopleMenu.tsx`: os avatares em pílula com a contagem, e a lista ao clicar.
struct PeopleMenu: View {
    @EnvironmentObject private var model: AppModel

    @State private var open = false

    var body: some View {
        Button {
            open.toggle()
        } label: {
            HStack(spacing: 10) {
                HStack(spacing: -8) {
                    ForEach(model.peers.prefix(3)) { peer in
                        Avatar(name: peer.name.isEmpty ? "?" : peer.name, size: 26, mine: peer.selfPeer)
                    }
                }

                Text("\(model.peers.count)")
                    .font(Theme.sans(12))
                    .foregroundStyle(Theme.inkIcon)
            }
            .padding(.leading, 6)
            .padding(.trailing, 12)
            .padding(.vertical, 5)
            .background(Theme.row, in: Capsule())
            .overlay(Capsule().strokeBorder(Theme.lineStrong, lineWidth: 1))
        }
        .buttonStyle(.pointer)
        .help("Quem está na sala")
        .popover(isPresented: $open, arrowEdge: .bottom) {
            PopoverBox(width: 288) {
                Text("Na sala")
                    .labelMono()
                    .padding(.horizontal, 8)
                    .padding(.vertical, 6)

                if model.peers.isEmpty {
                    Text("Nenhuma pessoa conectada.")
                        .font(Theme.sans(12.5))
                        .foregroundStyle(Theme.inkSoft)
                        .padding(8)
                }

                ForEach(model.peers) { peer in
                    row(peer)
                }
            }
        }
    }

    private func row(_ peer: RoomPeer) -> some View {
        HStack(spacing: 10) {
            Avatar(name: peer.name.isEmpty ? "?" : peer.name, size: 28, mine: peer.selfPeer)
                .opacity(peer.reconnecting ? 0.45 : 1)

            VStack(alignment: .leading, spacing: 1) {
                Text(peer.selfPeer ? "\(peer.name) (você)" : peer.name)
                    .font(Theme.sans(13, .medium))
                    .foregroundStyle(Theme.ink)
                    .lineLimit(1)

                if peer.reconnecting {
                    Text("reconectando…")
                        .font(Theme.mono(10))
                        .foregroundStyle(Theme.inkDim)
                }
            }

            Spacer(minLength: 0)

            if peer.producers.contains(where: { $0.source == "mic" }), peer.micOff {
                Icon(name: .micOff, size: 13)
                    .foregroundStyle(Theme.danger)
                    .help("Microfone mutado")
            }

            if let closed = model.pendingTiles.first(where: { $0.peerId == peer.peerId }) {
                Button("Assistir") {
                    Task { await model.watch(closed) }
                }
                .buttonStyle(GhostButton(font: Theme.sans(11), padding: EdgeInsets(top: 4, leading: 8, bottom: 4, trailing: 8)))
            }

            if peer.sharing {
                Text("AO VIVO")
                    .font(Theme.mono(9, .semibold))
                    .foregroundStyle(Theme.inkStrong)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Theme.danger, in: RoundedRectangle(cornerRadius: 5, style: .continuous))
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
    }
}
