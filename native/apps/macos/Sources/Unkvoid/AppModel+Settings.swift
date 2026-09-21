import AppKit
import Foundation

/// As preferências guardadas e o que elas ligam: aparelho de som, como o microfone abre, as
/// teclas que valem com o jogo na frente, e os avisos. Quem guarda é o núcleo, na pasta e com
/// as chaves do app de hoje.
extension AppModel {
    func loadPreferences() async {
        voicePreferences = VoicePreferences(await preference("unkvoid:voice") as? [String: Any] ?? [:])
        noticePreferences = NoticePreferences(await preference("unkvoid:notifications") as? [String: Any] ?? [:])
        shareQuality = await preference("unkvoid:quality") as? String ?? shareQuality
        shareFps = Int(await preference("unkvoid:fps") as? String ?? "") ?? shareFps
        let (rail, members) = (await preference("unkvoid:rail") as? Bool, await preference("unkvoid:members") as? Bool)

        if let rail, rail != railOpen {
            railOpen = rail
        }

        if let members, members != membersOpen {
            membersOpen = members
        }

        imageFilters = ImageFilters(await preference("unkvoid:image") as? [String: Any] ?? [:])

        refreshDevices()
        applyDevices()
        await applyKeys()
    }

    func checkForUpdate() async {
        let found = await ask("update")

        if let version = found["version"] as? String, let address = found["url"] as? String, let url = URL(string: address) {
            newerVersion = (version, url)
        }
    }

    func preference(_ key: String) async -> Any? {
        let value = await ask("preference", ["key": key])["value"]

        return value is NSNull ? nil : value
    }

    func remember(_ key: String, _ value: Any) {
        Task { _ = await ask("setPreference", ["key": key, "value": value]) }
    }

    /// Muda uma preferência de voz, guarda, e faz valer na hora.
    func setVoice(_ change: (inout VoicePreferences) -> Void) {
        var next = voicePreferences

        change(&next)

        guard next != voicePreferences else {
            return
        }

        let before = voicePreferences

        voicePreferences = next
        remember("unkvoid:voice", next.saved)

        if before.microphone != next.microphone || before.speaker != next.speaker {
            applyDevices()
        }

        media?.camera.blurBackground(next.blurBackground)

        Task {
            if before.inputMode != next.inputMode || before.sensitivity != next.sensitivity {
                await applyInputMode()
            }

            if [before.mute, before.deafen, before.talk, before.inputMode] != [next.mute, next.deafen, next.talk, next.inputMode] {
                await applyKeys()
            }
        }
    }

    func setNotices(_ change: (inout NoticePreferences) -> Void) {
        change(&noticePreferences)
        remember("unkvoid:notifications", noticePreferences.saved)
    }

    /// O aparelho guardado pelo nome vira o número que o CoreAudio tem agora. Aparelho que
    /// não está plugado é o padrão do sistema, sem apagar a escolha.
    private func applyDevices() {
        microphone = microphones.first { $0.name == voicePreferences.microphone }?.id
        speaker = speakers.first { $0.name == voicePreferences.speaker }?.id
    }

    /// Apertar para falar sem tecla escolhida deixaria a pessoa muda para sempre: aí o
    /// microfone fica aberto, e a tela de configurações avisa.
    func applyInputMode() async {
        let mode = voicePreferences.inputMode == "ptt" && voicePreferences.talk.isEmpty ? "open" : voicePreferences.inputMode

        _ = await ask("inputMode", ["mode": mode, "sensitivity": voicePreferences.sensitivity])
    }

    private func applyKeys() async {
        hotkeys.onPress("mute", await combo(voicePreferences.mute)) { [weak self] in
            Task { await self?.toggleMute() }
        }

        hotkeys.onPress("deafen", await combo(voicePreferences.deafen)) { [weak self] in
            Task { await self?.toggleDeafen() }
        }

        hotkeys.onHold("talk", voicePreferences.inputMode == "ptt" ? await combo(voicePreferences.talk) : nil) { [weak self] talking in
            Task { _ = await self?.ask("talk", ["talking": talking]) }
        }
    }

    private func combo(_ accelerator: String) async -> Hotkeys.Combo? {
        guard !accelerator.isEmpty else {
            return nil
        }

        let keys = await ask("keys", ["accelerator": accelerator])

        guard let key = keys["key"] as? Int else {
            return nil
        }

        let modifiers = (keys["modifiers"] as? [[Int]] ?? []).map { $0.map { CGKeyCode($0) } }

        return Hotkeys.Combo(key: CGKeyCode(key), modifiers: modifiers)
    }

    /// O atalho que a pessoa acabou de apertar no campo de tecla, no formato que se guarda
    /// (`CmdOrCtrl+Shift+KeyM`). `nil` para tecla que o mapa do núcleo não conhece.
    func accelerator(from event: NSEvent) async -> String? {
        guard let name = await ask("keyName", ["code": Int(event.keyCode)])["name"] as? String else {
            return nil
        }

        let flags = event.modifierFlags
        let parts = [
            flags.contains(.command) ? "CmdOrCtrl" : nil,
            flags.contains(.control) ? "Control" : nil,
            flags.contains(.option) ? "Alt" : nil,
            flags.contains(.shift) ? "Shift" : nil,
            name,
        ]

        return parts.compactMap { $0 }.joined(separator: "+")
    }

    func changeAvatar() async {
        guard let file = pickImage(limit: 2), let updated: User = decode(await upload("uploadAvatar", [:], field: "avatar", files: [file])) else {
            return
        }

        user = updated
    }

    func removeAvatar() async {
        if let updated: User = decode(await api("deleteAvatar")) {
            user = updated
        } else {
            await refreshMe()
        }
    }

    /// O apelido é único e sem espaço; quem valida é o Laravel, e o erro dele volta para o campo.
    func confirmNickname(_ name: String) async {
        nicknameBusy = true
        nicknameError = ""

        let answer = await ask("api", ["name": "updateMe", "params": [:], "body": ["name": name]])

        nicknameBusy = false

        if let invalid = answer["invalid"] as? [String: Any] {
            nicknameError = invalid["message"] as? String ?? Self.sentence(for: "invalid")

            return
        }

        guard let updated: User = decode(answer["data"]) else {
            nicknameError = Self.sentence(for: answer["failed"] as? String ?? "")

            return
        }

        user = updated
    }

    func refreshMe() async {
        if let fresh: User = decode(await ask("me")["user"]) {
            user = fresh
        }
    }
}
