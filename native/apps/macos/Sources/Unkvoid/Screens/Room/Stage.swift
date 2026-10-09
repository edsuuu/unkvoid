import SwiftUI

/// O palco da sala por código: o cartão largo quando ninguém transmite, e a grade de telas
/// quando há. A chamada de um canal de voz é o `CallGrid`, logo abaixo.
struct Stage: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        if model.tiles.isEmpty {
            EmptyStage()
        } else {
            VStack(spacing: 8) {
                header

                CardGrid(cards: model.tiles.map(CallCard.tile), focused: model.focusedTile, fullscreen: model.fullscreenTile)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private var header: some View {
        HStack(spacing: 8) {
            Text("Transmissões").labelMono()

            Text(model.tiles.count == 1 ? "1 tela" : "\(model.tiles.count) telas")
                .font(Theme.meta)
                .foregroundStyle(Theme.brandText)

            if model.reconnecting {
                Text("Reconectando…")
                    .font(Theme.meta)
                    .foregroundStyle(Theme.idle)
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
        .buttonStyle(GhostButton(padding: EdgeInsets(top: 4, leading: 12, bottom: 4, trailing: 12)))
    }
}

private struct EmptyStage: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        let pending = !model.pendingTiles.isEmpty

        VStack(spacing: 0) {
            Icon(name: .screen, size: 32)
                .foregroundStyle(Theme.inkStrong)
                .frame(width: 68, height: 68)
                .background(Theme.hover, in: Circle())
                .padding(.bottom, 20)

            Text(pending ? "Alguém está compartilhando, mas a tela não está aberta." : model.mine.sharing ? "Você está transmitindo." : "Ninguém está compartilhando ainda.")
                .font(Theme.title)
                .foregroundStyle(Theme.inkStrong)
                .multilineTextAlignment(.center)

            if model.mine.sharing, !pending {
                Text("A sua tela não aparece aqui para não gastar um decoder à toa.")
                    .font(Theme.sans(14))
                    .foregroundStyle(Theme.inkDim)
                    .padding(.top, 8)
            } else if !pending, let room = model.room, model.voiceChannel == nil {
                HStack(spacing: 5) {
                    Text("Mande o código")

                    Text(room).codeChip(size: 13)

                    Text("para quem você quer aqui.")
                }
                .font(Theme.sans(14))
                .foregroundStyle(Theme.inkDim)
                .padding(.top, 8)
            }

            HStack(spacing: 8) {
                if pending {
                    Button("Assistir transmissão") {
                        Task { await model.watch(nil) }
                    }
                    .buttonStyle(GhostButton())
                }

                if model.mine.sharing {
                    Button("Ver o que a sala vê") {
                        Task { await model.toggleSelfView() }
                    }
                    .buttonStyle(GhostButton())
                }

                if model.mine.canShare, !model.mine.sharing {
                    Button("Compartilhar tela") {
                        Task { await model.openShare() }
                    }
                    .buttonStyle(PrimaryButton(wide: false))
                    .disabled(model.shareStarting)
                }
            }
            .padding(.top, 20)

            if model.reconnecting {
                Text("Reconectando…")
                    .font(Theme.meta)
                    .foregroundStyle(Theme.idle)
                    .padding(.top, 16)
            }
        }
        .padding(36)
        .frame(maxWidth: 520)
        .surface()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// Uma peça da grade da chamada: uma tela ao vivo, uma tela fechada que dá para reabrir, ou
/// uma pessoa sem vídeo (o cartão com o avatar).
enum CallCard: Identifiable {
    case tile(RoomTile)
    case pending(RoomTile)
    case person(VoicePerson)

    var id: String {
        switch self {
        case let .tile(tile), let .pending(tile): tile.id
        case let .person(person): "user:\(person.user_id)"
        }
    }
}

/// A conta da grade: `ceil(sqrt(n))` colunas, e o cartão do tamanho que cabe na largura e na
/// altura ao mesmo tempo, em 16:9.
enum CallLayout {
    static let gap: CGFloat = 8

    static func columns(for count: Int) -> Int {
        count <= 1 ? 1 : Int(Double(count).squareRoot().rounded(.up))
    }

    static func rows(_ cards: [CallCard]) -> [[CallCard]] {
        let columns = columns(for: cards.count)

        return stride(from: 0, to: cards.count, by: columns).map { Array(cards[$0 ..< min($0 + columns, cards.count)]) }
    }

    static func cardWidth(in box: CGSize, count: Int) -> CGFloat {
        let columns = CGFloat(columns(for: count))
        let rows = (CGFloat(count) / columns).rounded(.up)
        let byWidth = (box.width - gap * (columns - 1)) / columns
        let byHeight = (box.height - gap * (rows - 1)) / rows * 16 / 9

        return max(80, min(byWidth, byHeight))
    }
}

/// A chamada de um canal de voz: cada pessoa num cartão (o vídeo da câmera, ou o avatar), mais
/// as telas compartilhadas. Vazia, convida.
struct CallGrid: View {
    @EnvironmentObject private var model: AppModel

    let channel: Channel

    var body: some View {
        let cards = cards

        if cards.isEmpty {
            VStack(spacing: 16) {
                Text("Ninguém por aqui ainda.")
                    .font(Theme.title)
                    .foregroundStyle(Theme.inkStrong)

                if model.abilities.allows("createInvite") {
                    Button("Convidar pessoas") { model.modal = .invitePeople }
                        .buttonStyle(PrimaryButton(wide: false))
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            CardGrid(cards: cards, focused: model.focusedTile, fullscreen: model.fullscreenTile)
        }
    }

    /// Quem tem a câmera aberta aparece pelo vídeo, e não duas vezes.
    private var cards: [CallCard] {
        let withCamera = Set(model.tiles.filter(\.camera).compactMap { tile -> Int? in
            if tile.mine == true {
                return model.user?.id
            }

            let account = model.peers.first { $0.peerId == tile.peerId }?.userId ?? ""

            return account.hasPrefix("user:") ? Int(account.dropFirst(5)) : nil
        })

        return model.tiles.map(CallCard.tile)
            + model.pendingTiles.map(CallCard.pending)
            + model.voicePeople(in: channel).filter { !withCamera.contains($0.user_id) }.map(CallCard.person)
    }
}

/// A grade em si. Com foco, o cartão escolhido ocupa o palco e os outros viram uma faixa de 128
/// embaixo; em tela cheia só o escolhido aparece.
private struct CardGrid: View {
    let cards: [CallCard]
    let focused: String?
    let fullscreen: String?

    var body: some View {
        let full = cards.first { $0.id == fullscreen }
        let big = cards.first { $0.id == focused }
        let others = cards.filter { $0.id != big?.id }

        if let full {
            CardView(card: full, thumb: false)
        } else if let big, !others.isEmpty {
            VStack(spacing: CallLayout.gap) {
                CardView(card: big, thumb: false)

                HStack(spacing: CallLayout.gap) {
                    ForEach(others) { card in
                        CardView(card: card, thumb: true)
                            .aspectRatio(16 / 9, contentMode: .fit)
                    }
                }
                .frame(height: 128)
            }
        } else {
            GeometryReader { box in
                let width = CallLayout.cardWidth(in: box.size, count: cards.count)

                VStack(spacing: CallLayout.gap) {
                    ForEach(CallLayout.rows(cards), id: \.first?.id) { row in
                        HStack(spacing: CallLayout.gap) {
                            ForEach(row) { card in
                                CardView(card: card, thumb: false)
                                    .frame(width: width, height: width * 9 / 16)
                            }
                        }
                    }
                }
                .frame(width: box.size.width, height: box.size.height)
            }
        }
    }
}

private struct CardView: View {
    let card: CallCard
    let thumb: Bool

    var body: some View {
        switch card {
        case let .tile(tile): StreamTile(tile: tile, thumb: thumb)
        case let .pending(tile): PendingTile(tile: tile)
        case let .person(person): PersonCard(person: person)
        }
    }
}

/// O rótulo embaixo à esquerda de um cartão: fundo preto a 60%, raio 4, 13/600 branco.
private struct CardLabel<Trailing: View>: View {
    let text: String
    @ViewBuilder let trailing: Trailing

    var body: some View {
        HStack(spacing: 6) {
            Text(text)
                .font(Theme.sans(13, .semibold))
                .foregroundStyle(.white)
                .lineLimit(1)

            trailing
        }
        .padding(.horizontal, 8)
        .frame(height: 24)
        .background(Color.black.opacity(0.6), in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
    }
}

/// A pessoa sem vídeo: o avatar de 80 no centro do cartão, e o anel de quem fala.
private struct PersonCard: View {
    @EnvironmentObject private var model: AppModel

    let person: VoicePerson

    private var member: Member? {
        model.tree?.members.first { $0.user_id == person.user_id }
    }

    var body: some View {
        let me = person.user_id == model.user?.id
        let speaking = model.isSpeaking(person.user_id)
        let muted = me ? model.micShownOff : person.muted == true

        ZStack(alignment: .bottomLeading) {
            Theme.surfaceTile

            Avatar(name: member?.displayName ?? person.name, url: member?.avatar_url, size: 80, mine: me, ring: Theme.surfaceTile)
                .frame(maxWidth: .infinity, maxHeight: .infinity)

            CardLabel(text: member?.displayName ?? person.name) {
                if muted {
                    Icon(name: .micOff, size: 16).foregroundStyle(Theme.danger)
                }
            }
            .padding(8)
        }
        .clipShape(RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous)
                .strokeBorder(Theme.online, lineWidth: 2)
                .opacity(speaking ? 1 : 0)
        )
        .animation(.easeOut(duration: 0.1), value: speaking)
        .contentShape(Rectangle())
        .onTapGesture { model.memberMenu = member }
        .contextMenu {
            if let member {
                MemberMenuItems(member: member)
            }
        }
    }
}

/// A tela que está ao vivo e a pessoa fechou: escurecida, com o botão de voltar a assistir.
private struct PendingTile: View {
    @EnvironmentObject private var model: AppModel

    let tile: RoomTile

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            Theme.surfaceTile

            Button("Assistir transmissão") {
                Task { await model.watch(tile) }
            }
            .buttonStyle(PrimaryButton(wide: false))
            .frame(maxWidth: .infinity, maxHeight: .infinity)

            CardLabel(text: tile.label) {
                LiveBadge()
            }
            .padding(8)
        }
        .clipShape(RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous))
    }
}

/// O selo "AO VIVO".
struct LiveBadge: View {
    var body: some View {
        Text("AO VIVO")
            .font(Theme.sans(11, .bold))
            .foregroundStyle(.white)
            .padding(.horizontal, 4)
            .frame(height: 16)
            .background(Theme.live, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
    }
}

/// A tela de alguém, com o nome, o selo e os botões por cima — som e volume, pausar, focar,
/// tela cheia e fechar.
private struct StreamTile: View {
    @EnvironmentObject private var model: AppModel

    let tile: RoomTile
    let thumb: Bool

    @State private var hovering = false
    @State private var volumeOpen = false

    var body: some View {
        ZStack(alignment: .bottom) {
            Theme.surfaceTile

            if let media = model.media {
                VideoSurface(
                    sink: media.sink(for: tile.producerId),
                    look: tile.camera ? model.imageFilters.camera : model.imageFilters.screen,
                    mirrored: tile.camera && tile.mine == true
                )
            }

            if tile.paused == true {
                Color.black.opacity(0.6)

                Text("Pausado")
                    .font(Theme.button)
                    .foregroundStyle(Theme.inkSoft)
                    .frame(maxHeight: .infinity)
            }

            bar
        }
        .clipShape(RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous)
                .strokeBorder(Theme.brand, lineWidth: 2)
                .opacity(model.focusedTile == tile.id ? 1 : 0)
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
            CardLabel(text: tile.label) {
                if tile.camera {
                    Icon(name: .camera, size: 14).foregroundStyle(.white)
                } else if tile.mine != true {
                    LiveBadge()
                }

                if let watching = model.watchers[tile.producerId], !watching.isEmpty {
                    HStack(spacing: 3) {
                        Icon(name: .eye, size: 12)

                        Text("\(watching.count)")
                    }
                    .font(Theme.meta)
                    .foregroundStyle(Theme.inkSoft)
                    .help("Assistindo agora: \(watching.joined(separator: ", "))")
                }
            }

            Spacer(minLength: 0)

            if !thumb, hovering || volumeOpen {
                controls
            }
        }
        .padding(8)
        .background(LinearGradient(colors: [.clear, .black.opacity(0.6)], startPoint: .top, endPoint: .bottom))
    }

    @ViewBuilder
    private var controls: some View {
        if tile.audio != nil {
            let listening = model.heardTiles.contains(tile.id)

            button(listening ? .speaker : .speakerOff, listening ? "Mutar o áudio desta tela" : "Ativar o áudio desta tela", tone: listening ? .idle : .off) {
                await model.toggleHeard(tile)
            }
        }

        button(.sliders, "Volume e imagem — só do seu lado", tone: volumeOpen ? .on : .idle) {
            volumeOpen.toggle()
        }
        .popover(isPresented: $volumeOpen, arrowEdge: .top) {
            adjustments
        }

        if tile.mine != true {
            button(tile.paused == true ? .play : .eye, tile.paused == true ? "Retomar" : "Pausar: para de receber sem sair") {
                await model.togglePause(tile)
            }
        }

        button(.focus, model.focusedTile == tile.id ? "Sair do foco" : "Focar", tone: model.focusedTile == tile.id ? .on : .idle) {
            model.focus(tile)
        }

        button(.fullscreen, model.fullscreenTile == tile.id ? "Sair da tela cheia" : "Tela cheia") {
            model.toggleFullscreen(tile)
        }

        button(.close, tile.mine == true ? (tile.camera ? "Desligar a câmera" : "Ocultar minha tela") : "Fechar (continua ao vivo para os outros)") {
            if tile.mine == true, tile.camera {
                await model.toggleCamera()
            } else if tile.mine == true {
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
            .font(Theme.meta)
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
            Icon(name: icon, size: 16)
        }
        .buttonStyle(IconButton(side: 28, tone: tone))
        .background(Color.black.opacity(0.4), in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
        .help(hint)
    }
}

/// A barra de controles da chamada, no centro embaixo: botões redondos de 56 — câmera, tela,
/// microfone, ensurdecer e, separado, desconectar. Ligado é claro com o ícone escuro; mutado é
/// vermelho com o ícone branco.
struct CallControls: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 16) {
            control(model.mine.camera ? .camera : .cameraOff, model.mine.camera ? "Desligar a câmera" : "Ligar a câmera", look: model.mine.camera ? .on : .idle, enabled: model.mine.canVideo) {
                await model.toggleCamera()
            }

            control(.screen, model.mine.sharing ? "Parar de compartilhar" : "Compartilhar tela", look: model.mine.sharing ? .on : .idle, enabled: model.mine.canShare && !model.shareStarting) {
                if model.mine.sharing {
                    await model.stopSharing()
                } else {
                    await model.openShare()
                }
            }

            control(micOff ? .micOff : .mic, micOff ? "Desmutar" : "Mutar", look: micOff ? .off : .idle, enabled: model.mine.canSpeak) {
                await model.toggleMute()
            }

            control(model.deafened ? .headphonesOff : .headphones, model.deafened ? "Voltar a ouvir" : "Ensurdecer", look: model.deafened ? .off : .idle, enabled: true) {
                await model.toggleDeafen()
            }

            Spacer().frame(width: 8)

            control(.phoneOff, "Sair da voz", look: .leave, enabled: true) {
                await model.leaveVoice()
            }
        }
    }

    private var micOff: Bool {
        model.micShownOff
    }

    private enum Look {
        case idle
        case on
        case off
        case leave
    }

    private func control(_ icon: IconName, _ hint: String, look: Look, enabled: Bool, _ action: @escaping @MainActor () async -> Void) -> some View {
        Button {
            Task { await action() }
        } label: {
            Icon(name: icon, size: 24)
                .foregroundStyle(look == .on ? Color.black : look == .idle ? Theme.inkStrong : .white)
                .frame(width: Theme.Size.control, height: Theme.Size.control)
                .background(look == .on ? Theme.inkStrong : look == .idle ? Theme.surfaceSide : Theme.dangerFill, in: Circle())
                .contentShape(Circle())
        }
        .buttonStyle(.pointer)
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.4)
        .help(hint)
    }
}
