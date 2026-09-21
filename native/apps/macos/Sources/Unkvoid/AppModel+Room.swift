import AppKit
import Foundation

/// A sala aberta: o que o núcleo anuncia, e os cliques que voltam para ele.
extension AppModel {
    /// A sala acabou de abrir: a mídia começa a ser esvaziada e a tela pega o estado de agora,
    /// sem esperar o primeiro aviso.
    func openedRoom() async {
        media?.start()

        let now = await ask("room")

        peers = decode(now["peers"]) ?? []
        tiles = decode(now["tiles"]) ?? []
        pendingTiles = decode(now["pending"]) ?? []
        mine = decode(now["mine"]) ?? Mine()

        // Quem ensurdeceu fora da sala entra surdo: a sala nova nasce ouvindo.
        if deafened {
            _ = await ask("deafen", ["deafened": true])
        }
    }

    /// Entrar num canal de voz: a mesma sala, com o token de 60 s que o Laravel assina. Quem
    /// entra já é ouvido, como no app de hoje.
    func joinVoice(_ opened: Channel) async {
        guard voiceChannel?.id != opened.id else {
            stageOpen = true

            return
        }

        guard !voiceJoining else {
            return
        }

        await leaveVoice()

        voiceJoining = true
        voiceTarget = opened.id

        let answer = await ask("joinVoice", ["channel": opened.id])

        voiceJoining = false
        voiceTarget = nil

        guard answer["ok"] as? Bool == true else {
            warn(answer)

            return
        }

        voiceChannel = opened
        stageOpen = true
        enteredRoomAt = Date()

        await openedRoom()
        await voiceChat.open(opened)
        await openMicrophone()
    }

    func leaveVoice() async {
        guard voiceChannel != nil else {
            return
        }

        closeRoom()

        await voiceChat.close()

        _ = await ask("leaveRoom")
    }

    /// Trocar de microfone com ele aberto é fechar a captura e abrir na entrada nova; o
    /// producer no servidor é o mesmo.
    func microphoneChanged() async {
        guard mine.mic, let core, let media else {
            return
        }

        do {
            try media.sound.listen(on: microphone, cleaned: voicePreferences.noiseSuppression) { samples in
                core.speak(samples)
            }
        } catch {
            complain(Self.roomFailure("mic"))
        }
    }

    func closeRoom() {
        media?.stop()

        roomError = nil

        voiceChannel = nil
        voiceJoining = false
        stageOpen = false
        focusedRoom = false
        watchers = [:]
        voiceChatOpen = false

        peers = []
        tiles = []
        pendingTiles = []
        fullscreenTile = nil
        tileVolumes = [:]
        mutedAtRest = mine.mic && mine.micMuted
        mine = Mine()
        ping = nil
        signalBars = nil
        reconnecting = false
        micLevel = 0
        focusedTile = nil
        heardTiles = []
        shareOpen = false
        shareStarting = false
    }

    /// Um aviso da fila do núcleo. Os da sala redesenham a sala; os do chat vão para o chat.
    func heard(_ event: [String: Any]) {
        let data = event["data"] as? [String: Any] ?? [:]

        switch event["event"] as? String {
        case "room.peers":
            let before = Set(peers.map(\.id))

            peers = decode(data["peers"]) ?? peers

            if noticePreferences.sounds, !before.isEmpty {
                let now = Set(peers.map(\.id))

                if !now.subtracting(before).isEmpty {
                    Sounds.joined()
                } else if !before.subtracting(now).isEmpty {
                    Sounds.left()
                }
            }
        case "room.tiles":
            tiles = decode(data["tiles"]) ?? tiles
            pendingTiles = decode(data["pending"]) ?? []
            media?.keep(Set(tiles.map(\.producerId)))

            if let fullscreenTile, !tiles.contains(where: { $0.id == fullscreenTile }) {
                self.fullscreenTile = nil
            }

            if let focusedTile, !tiles.contains(where: { $0.id == focusedTile }) {
                self.focusedTile = nil
            }
        case "room.mine":
            mine = decode(data) ?? mine
        case "room.level":
            micLevel = (data["level"] as? NSNumber)?.floatValue ?? 0
            micPercent = data["percent"] as? Int ?? 0
        case "room.ping":
            ping = data["ms"] as? Int
            signalBars = data["bars"] as? Int
        case "room.watchers":
            if let producer = data["producerId"] as? String {
                watchers[producer] = (data["watchers"] as? [[String: Any]] ?? []).compactMap { $0["name"] as? String }
            }
        case "room.session":
            sessionChanged(data["state"] as? String)
        case "room.failed":
            complain(Self.roomFailure(data["what"] as? String))
        default:
            heardFromChat(event)
        }
    }

    private func sessionChanged(_ state: String?) {
        switch state {
        case "lost":
            reconnecting = true
        case "rejoined":
            reconnecting = false
            complain(nil)
        case "gone":
            reconnecting = false
            complain("A sala não voltou. Entre de novo quando a internet estabilizar.")
        case "replaced":
            Task { await thrownOut("Esta conta entrou na sala por outro lugar.") }
        case "kicked":
            Task { await thrownOut("Você foi removido desta sala.") }
        default:
            break
        }
    }

    /// O servidor tirou esta sessão da sala. A pessoa volta para onde estava antes, e o motivo
    /// vai como aviso da tela em que ela cai — a sala já não existe para mostrar erro nenhum.
    private func thrownOut(_ why: String) async {
        closeRoom()

        await voiceChat.close()

        _ = await ask("leaveRoom")

        enteredRoomAt = nil

        await readState()

        say(why)
    }

    private static func roomFailure(_ what: String?) -> String {
        switch what {
        case "watch": "Não deu para assistir a uma das transmissões."
        case "mic": "Não deu para abrir o microfone."
        default: "Não deu para compartilhar a tela."
        }
    }

    /// Abre o seletor e pergunta ao núcleo o que dá para compartilhar.
    func openShare() async {
        shareOpen = true
        shareLoading = true

        let found = await ask("displays")

        shareLoading = false

        let displays = found["displays"] as? [[String: Any]] ?? []
        let windows = found["windows"] as? [[String: Any]] ?? []

        shareDisplays = displays.enumerated().map { index, display in
            ShareSource(
                id: "display:\(display["id"] as? Int ?? 0)",
                label: displays.count == 1 ? "Tela inteira" : "Tela \(index + 1)",
                detail: "\(display["width"] as? Int ?? 0) × \(display["height"] as? Int ?? 0)"
            )
        }

        shareWindows = windows.map { window in
            ShareSource(
                id: "window:\(window["id"] as? Int ?? 0)",
                label: window["title"] as? String ?? "",
                detail: window["application"] as? String ?? ""
            )
        }

        if shareSource == nil || !(shareDisplays + shareWindows).contains(where: { $0.id == shareSource }) {
            shareSource = shareDisplays.first?.id
        }

        await loadPreviews()
    }

    /// A miniatura de cada origem, uma de cada vez: tirar a foto de uma janela leva tempo, e
    /// o seletor já está aberto e clicável enquanto elas chegam.
    private func loadPreviews() async {
        sharePreviews = [:]

        for source in shareDisplays + shareWindows.prefix(12) where shareOpen {
            guard
                let jpeg = await ask("preview", ["source": source.id])["jpeg"] as? String,
                let data = Data(base64Encoded: jpeg),
                let image = NSImage(data: data)
            else {
                continue
            }

            sharePreviews[source.id] = image
        }
    }

    /// Com a transmissão no ar, resolução e quadros mudam sem fechar nada.
    func changeQuality(_ quality: String, _ fps: Int) async {
        shareQuality = quality
        shareFps = fps

        remember("unkvoid:quality", quality)
        remember("unkvoid:fps", "\(fps)")

        if await ask("changeQuality", ["quality": quality, "fps": fps])["ok"] as? Bool != true {
            complain("Não deu para trocar a qualidade da transmissão.")
        }
    }

    func toggleSelfView() async {
        _ = await ask("selfView", ["wanted": mine.selfView != true])
    }

    func watch(_ tile: RoomTile?) async {
        _ = await ask("watch", tile.map { ["producerId": $0.producerId] } ?? [:])
    }

    func close(_ tile: RoomTile) async {
        _ = await ask("closeWatched", ["producerId": tile.producerId])
    }

    func togglePause(_ tile: RoomTile) async {
        _ = await ask("pauseWatched", ["producerId": tile.producerId, "paused": tile.paused != true])
    }

    func setVolume(_ tile: RoomTile, _ volume: Float) {
        guard let audio = tile.audio else {
            return
        }

        tileVolumes[tile.id] = volume
        media?.sound.setVolume(volume, of: audio)
    }

    /// A tela escolhida toma a janela, e a janela toma o monitor.
    /// O microfone de uma pessoa na voz, pela conta dela. `nil` se ela não está falando aqui.
    func voiceProducer(of member: Member) -> String? {
        peers.first { $0.userId == "user:\(member.user_id)" && !$0.selfPeer }?.producers.first { $0.source == "mic" }?.producerId
    }

    func setVoiceVolume(_ member: Member, _ volume: Float) {
        guard let producer = voiceProducer(of: member) else {
            return
        }

        voiceVolumes["user:\(member.user_id)"] = volume
        media?.sound.setVolume(volume, of: producer)
    }

    func toggleFullscreen(_ tile: RoomTile) {
        fullscreenTile = fullscreenTile == tile.id ? nil : tile.id

        let window = NSApp.keyWindow

        if (fullscreenTile != nil) != (window?.styleMask.contains(.fullScreen) == true) {
            window?.toggleFullScreen(nil)
        }
    }

    /// Transmitir o que foi escolhido. Trocar de origem com a transmissão no ar é parar e
    /// começar de novo: o encoder é aberto no tamanho da origem.
    func confirmShare() async {
        guard let shareSource, !shareStarting else {
            return
        }

        remember("unkvoid:quality", shareQuality)
        remember("unkvoid:fps", "\(shareFps)")

        shareOpen = false
        shareStarting = true

        if mine.sharing {
            _ = await ask("stopSharing")
        }

        let answer = await ask("share", [
            "source": shareSource,
            "quality": shareQuality,
            "fps": shareFps,
            "audio": shareAudio,
            "muteCalls": shareMuteCalls,
        ])

        shareStarting = false

        if answer["ok"] as? Bool != true {
            complain("Não deu para compartilhar a tela. Confira a permissão de gravação de tela nas Configurações do Sistema.")
        }
    }

    func stopSharing() async {
        _ = await ask("stopSharing")
    }

    /// O som de uma tela chega mudo; o botão do cartão o liga só para esta pessoa.
    func toggleHeard(_ tile: RoomTile) async {
        guard let audio = tile.audio else {
            return
        }

        let listening = !heardTiles.contains(tile.id)

        if listening {
            heardTiles.insert(tile.id)
        } else {
            heardTiles.remove(tile.id)
        }

        _ = await ask("muteWatched", ["producerId": audio, "muted": !listening])
    }

    func focus(_ tile: RoomTile) {
        focusedTile = focusedTile == tile.id ? nil : tile.id
    }

    /// Entrar na voz abre o microfone, como no app de hoje: quem entra já é ouvido.
    func openMicrophone() async {
        guard mine.canSpeak, !mine.mic, let core, let media else {
            return
        }

        guard await ask("openMicrophone")["ok"] as? Bool == true else {
            complain(Self.roomFailure("mic"))

            return
        }

        await applyInputMode()

        do {
            try media.sound.listen(on: microphone, cleaned: voicePreferences.noiseSuppression) { samples in
                core.speak(samples)
            }
        } catch {
            _ = await ask("closeMicrophone")

            complain(Self.roomFailure("mic"))

            return
        }

        if voicePreferences.muteOnJoin || mutedAtRest {
            _ = await ask("muteMicrophone", ["muted": true])
        }
    }

    func toggleMute() async {
        guard screen == .room || voiceChannel != nil else {
            mutedAtRest.toggle()

            return
        }

        guard mine.mic else {
            await openMicrophone()

            return
        }

        _ = await ask("muteMicrophone", ["muted": !mine.micMuted])
    }

    func toggleCamera() async {
        guard let core, let media else {
            return
        }

        if mine.camera {
            media.camera.stop()

            _ = await ask("closeCamera")

            return
        }

        let opened = await ask("openCamera", ["width": Camera.size.width, "height": Camera.size.height, "fps": Camera.frameRate])

        guard opened["ok"] as? Bool == true else {
            complain("Não deu para ligar a câmera.")

            return
        }

        media.camera.blurBackground(voicePreferences.blurBackground)

        do {
            try await media.camera.start { surface, time in
                core.show(surface, at: time)
            }
        } catch {
            _ = await ask("closeCamera")

            complain(error as? Camera.Failure == .notAllowed
                ? "O Unkvoid não tem permissão para usar a câmera. Autorize nas Configurações do Sistema."
                : "Não deu para ligar a câmera.")
        }
    }

    func toggleDeafen() async {
        deafened.toggle()

        _ = await ask("deafen", ["deafened": deafened])
    }
}
