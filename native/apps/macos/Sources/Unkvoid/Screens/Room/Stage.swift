import SwiftUI

/// `Stage.tsx`: o cartão largo quando ninguém transmite, e a grade de telas quando há.
struct Stage: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        if model.tiles.isEmpty {
            EmptyStage()
        } else {
            VStack(spacing: 10) {
                header

                TileGrid()
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private var header: some View {
        HStack(spacing: 8) {
            Text("Transmissões").labelMono()

            Text(model.tiles.count == 1 ? "1 tela" : "\(model.tiles.count) telas")
                .font(Theme.mono(10))
                .foregroundStyle(Theme.lilac2)
                .padding(.horizontal, 8)
                .padding(.vertical, 2)
                .background(Theme.brand.opacity(0.15), in: Capsule())
                .overlay(Capsule().strokeBorder(Theme.brand.opacity(0.3), lineWidth: 1))

            if model.reconnecting {
                Text("reconectando…")
                    .font(Theme.mono(10.5))
                    .foregroundStyle(Theme.danger)
            }

            Spacer(minLength: 0)

            if !model.pendingTiles.isEmpty {
                small("Assistir quem falta") { await model.watch(nil) }
            }

            if model.focusedTile != nil {
                small("Sair do foco") { model.focusedTile = nil }
            }
        }
    }

    private func small(_ label: String, _ action: @escaping @MainActor () async -> Void) -> some View {
        Button(label) {
            Task { await action() }
        }
        .buttonStyle(GhostButton(font: Theme.sans(11.5), padding: EdgeInsets(top: 6, leading: 10, bottom: 6, trailing: 10)))
    }
}

private struct EmptyStage: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        let pending = !model.pendingTiles.isEmpty

        VStack(spacing: 0) {
            ZStack {
                Icon(name: .screen, size: 26)
                    .foregroundStyle(Theme.lilac2)
            }
            .frame(width: 64, height: 64)
            .background(Theme.brand.opacity(0.15), in: RoundedRectangle(cornerRadius: 20, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 20, style: .continuous)
                    .strokeBorder(Theme.brand.opacity(0.3), lineWidth: 1)
            )
            .padding(.bottom, 20)

            Text(pending ? "Alguém está compartilhando, mas a tela não está aberta." : model.mine.sharing ? "Você está transmitindo." : "Ninguém está compartilhando ainda.")
                .font(Theme.sans(19, .semibold))
                .tracking(-0.3)
                .foregroundStyle(Theme.ink)
                .multilineTextAlignment(.center)

            if model.mine.sharing, !pending {
                Text("A sua tela não aparece aqui para não gastar um decoder à toa.")
                    .font(Theme.sans(13))
                    .foregroundStyle(Theme.inkSoft)
                    .padding(.top, 10)
            } else if !pending, let room = model.room, model.voiceChannel == nil {
                HStack(spacing: 5) {
                    Text("Mande o código")

                    Text(room).codeChip(size: 12)

                    Text("para quem você quer aqui.")
                }
                .font(Theme.sans(13))
                .foregroundStyle(Theme.inkSoft)
                .padding(.top, 10)
            }

            HStack(spacing: 8) {
                if pending {
                    Button("Assistir") {
                        Task { await model.watch(nil) }
                    }
                    .buttonStyle(GhostButton(font: Theme.sans(13)))
                }

                if model.mine.sharing {
                    Button("Ver o que a sala vê") {
                        Task { await model.toggleSelfView() }
                    }
                    .buttonStyle(GhostButton(font: Theme.sans(13)))
                }

                if model.mine.canShare, !model.mine.sharing {
                    Button("Iniciar compartilhamento") {
                        Task { await model.openShare() }
                    }
                    .buttonStyle(PrimaryButton(wide: false, font: Theme.sans(13, .semibold)))
                    .disabled(model.shareStarting)
                }
            }
            .padding(.top, 20)

            if model.reconnecting {
                Text("reconectando…")
                    .font(Theme.mono(11))
                    .foregroundStyle(Theme.danger)
                    .padding(.top, 16)
            }
        }
        .padding(36)
        .frame(maxWidth: 520)
        .glass(radius: 24, shadowed: true)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// A grade: uma coluna com uma tela, duas até quatro, três daí em diante. Com foco, a tela
/// escolhida ocupa o palco e as outras viram uma fila de miniaturas embaixo. Em tela cheia
/// só a escolhida aparece.
private struct TileGrid: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        let full = model.tiles.first { $0.id == model.fullscreenTile }
        let focused = model.tiles.first { $0.id == model.focusedTile }
        let others = model.tiles.filter { $0.id != focused?.id }

        if let full {
            StreamTile(tile: full, thumb: false)
        } else if let focused, !others.isEmpty {
            VStack(spacing: 10) {
                StreamTile(tile: focused, thumb: false)

                HStack(spacing: 10) {
                    ForEach(others) { tile in
                        StreamTile(tile: tile, thumb: true)
                    }
                }
                .frame(height: 104)
            }
        } else {
            let columns = model.tiles.count <= 1 ? 1 : model.tiles.count <= 4 ? 2 : 3

            Grid(horizontalSpacing: 10, verticalSpacing: 10) {
                ForEach(rows(of: model.tiles, by: columns), id: \.first?.id) { row in
                    GridRow {
                        ForEach(row) { tile in
                            StreamTile(tile: tile, thumb: false)
                        }
                    }
                }
            }
        }
    }

    private func rows(of tiles: [RoomTile], by columns: Int) -> [[RoomTile]] {
        stride(from: 0, to: tiles.count, by: columns).map { Array(tiles[$0 ..< min($0 + columns, tiles.count)]) }
    }
}

/// `StreamTile.tsx`: a tela de alguém, com o nome, o selo e os botões por cima — som e
/// volume, pausar, focar, tela cheia e fechar.
private struct StreamTile: View {
    @EnvironmentObject private var model: AppModel

    let tile: RoomTile
    let thumb: Bool

    @State private var hovering = false
    @State private var volumeOpen = false

    var body: some View {
        ZStack(alignment: .bottom) {
            if let media = model.media {
                VideoSurface(sink: media.sink(for: tile.producerId), look: tile.camera ? model.imageFilters.camera : model.imageFilters.screen)
            }

            if tile.paused == true {
                Color.black.opacity(0.6)

                Text("pausado")
                    .font(Theme.mono(11))
                    .foregroundStyle(Theme.inkIcon)
                    .frame(maxHeight: .infinity)
            }

            bar
        }
        .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .strokeBorder(model.focusedTile == tile.id ? Theme.brand.opacity(0.6) : Theme.lineStrong, lineWidth: 1)
        )
        .contentShape(Rectangle())
        .onTapGesture(count: 2) {
            if !thumb {
                model.toggleFullscreen(tile)
            }
        }
        .onTapGesture {
            if thumb {
                model.focus(tile)
            }
        }
        .onHover { hovering = $0 }
        .help(thumb ? "Focar esta tela" : "")
    }

    private var bar: some View {
        HStack(spacing: 6) {
            Text(tile.camera ? "CÂMERA" : tile.mine == true ? "VOCÊ" : "AO VIVO")
                .font(Theme.mono(9, .semibold))
                .foregroundStyle(Theme.inkStrong)
                .padding(.horizontal, 6)
                .padding(.vertical, 2)
                .background(tile.camera || tile.mine == true ? Theme.brand : Theme.danger, in: RoundedRectangle(cornerRadius: 5, style: .continuous))

            Text(tile.label)
                .font(Theme.sans(12, .medium))
                .foregroundStyle(Theme.inkStrong)
                .lineLimit(1)

            if let watching = model.watchers[tile.producerId], !watching.isEmpty {
                HStack(spacing: 4) {
                    Icon(name: .eye, size: 11)

                    Text("\(watching.count)")
                }
                .font(Theme.mono(10))
                .foregroundStyle(Theme.inkIcon)
                .help("Assistindo agora: \(watching.joined(separator: ", "))")
            }

            Spacer(minLength: 0)

            if !thumb, hovering || volumeOpen {
                controls
            }
        }
        .padding(10)
        .background(LinearGradient(colors: [.clear, .black.opacity(0.7)], startPoint: .top, endPoint: .bottom))
    }

    @ViewBuilder
    private var controls: some View {
        if tile.audio != nil {
            let listening = model.heardTiles.contains(tile.id)

            button(listening ? .speaker : .speakerOff, listening ? "Mutar o áudio desta tela" : "Ativar o áudio desta tela", tone: listening ? .idle : .off) {
                await model.toggleHeard(tile)
            }
        }

        button(.sliders, "Volume e imagem — só do seu lado, não mudam o que os outros veem", tone: volumeOpen ? .on : .idle) {
            volumeOpen.toggle()
        }
        .popover(isPresented: $volumeOpen, arrowEdge: .top) {
            adjustments
        }

        if tile.mine != true {
            button(tile.paused == true ? .play : .eye, tile.paused == true ? "Retomar" : "Pausar: para de receber sem sair da sala") {
                await model.togglePause(tile)
            }
        }

        button(.focus, model.focusedTile == tile.id ? "Sair do foco" : "Focar esta tela", tone: model.focusedTile == tile.id ? .on : .idle) {
            model.focus(tile)
        }

        button(.fullscreen, model.fullscreenTile == tile.id ? "Sair da tela cheia" : "Tela cheia") {
            model.toggleFullscreen(tile)
        }

        button(.close, tile.mine == true ? "Ocultar minha tela" : "Fechar esta transmissão (continua ao vivo para os outros)") {
            if tile.mine == true {
                await model.toggleSelfView()
            } else {
                await model.close(tile)
            }
        }
    }

    /// O painel do cartão: o volume desta tela e a imagem de todas as telas (ou de todas as câmeras).
    private var adjustments: some View {
        let look: WritableKeyPath<ImageFilters, ImageFilters.Look> = tile.camera ? \.camera : \.screen

        return PopoverBox(width: 220) {
            if tile.audio != nil {
                slider("Volume", Binding(get: { Double(model.tileVolumes[tile.id] ?? 1) * 100 }, set: { model.setVolume(tile, Float($0 / 100)) }), 0 ... 100)
            }

            slider("Brilho", filter(look.appending(path: \.brightness)), 40 ... 160)
            slider("Contraste", filter(look.appending(path: \.contrast)), 40 ... 160)
            slider("Saturação", filter(look.appending(path: \.saturation)), 0 ... 200)
            slider("Desfoque", filter(look.appending(path: \.blur)), 0 ... 20)

            Button("Voltar ao padrão") {
                model.imageFilters[keyPath: look] = ImageFilters.Look()
            }
            .buttonStyle(.pointer)
            .font(Theme.sans(11.5))
            .foregroundStyle(Theme.inkDim)
            .padding(.top, 4)
        }
    }

    private func filter(_ path: WritableKeyPath<ImageFilters, Double>) -> Binding<Double> {
        Binding(get: { model.imageFilters[keyPath: path] }, set: { model.imageFilters[keyPath: path] = $0 })
    }

    private func slider(_ label: String, _ value: Binding<Double>, _ range: ClosedRange<Double>) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label).labelMono()

            Slider(value: value, in: range).tint(Theme.brand)
        }
    }

    private func button(_ icon: IconName, _ hint: String, tone: IconButton.Tone = .idle, _ action: @escaping @MainActor () async -> Void) -> some View {
        Button {
            Task { await action() }
        } label: {
            Icon(name: icon, size: 13)
        }
        .buttonStyle(IconButton(side: 28, radius: 8, tone: tone))
        .help(hint)
    }
}
