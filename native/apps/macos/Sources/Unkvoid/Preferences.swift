import Foundation

/// As preferências de voz, com as chaves e a forma que o app de hoje já grava em
/// `unkvoid:voice` — quem atualizar não perde a escolha que tinha.
struct VoicePreferences: Equatable {
    /// O nome do aparelho, e não o número dele: o número muda a cada vez que o fone é plugado.
    var microphone = ""
    var speaker = ""
    var noiseSuppression = true
    var muteOnJoin = false
    var blurBackground = false
    /// `voice`, `ptt` ou `open`.
    var inputMode = "voice"
    var sensitivity = 35
    var mute = "CmdOrCtrl+Shift+KeyM"
    var deafen = "CmdOrCtrl+Shift+KeyD"
    var talk = ""

    init() {}

    init(_ saved: [String: Any]) {
        let keys = saved["keybinds"] as? [String: Any] ?? [:]

        microphone = saved["microphone"] as? String ?? microphone
        speaker = saved["speaker"] as? String ?? speaker
        noiseSuppression = saved["noiseSuppression"] as? Bool ?? noiseSuppression
        muteOnJoin = saved["muteOnJoin"] as? Bool ?? muteOnJoin
        blurBackground = saved["blurBackground"] as? Bool ?? blurBackground
        inputMode = saved["inputMode"] as? String ?? inputMode
        sensitivity = saved["sensitivity"] as? Int ?? sensitivity
        mute = keys["mute"] as? String ?? mute
        deafen = keys["deafen"] as? String ?? deafen
        talk = keys["talk"] as? String ?? talk
    }

    var saved: [String: Any] {
        [
            "microphone": microphone,
            "speaker": speaker,
            "noiseSuppression": noiseSuppression,
            "muteOnJoin": muteOnJoin,
            "blurBackground": blurBackground,
            "inputMode": inputMode,
            "sensitivity": sensitivity,
            "keybinds": ["mute": mute, "deafen": deafen, "talk": talk],
        ]
    }

    /// Duas ações na mesma tecla: só a primeira valeria.
    var repeatsAKey: Bool {
        let chosen = [mute, deafen, talk].filter { !$0.isEmpty }

        return Set(chosen).count != chosen.count
    }
}

struct NoticePreferences: Equatable {
    var sounds = true
    var directMessages = true

    init() {}

    init(_ saved: [String: Any]) {
        sounds = saved["sounds"] as? Bool ?? sounds
        directMessages = saved["directMessages"] as? Bool ?? directMessages
    }

    var saved: [String: Any] {
        ["sounds": sounds, "directMessages": directMessages]
    }
}

/// Brilho, contraste, saturação e desfoque de quem assiste — só deste lado, e um ajuste para
/// as telas e outro para as câmeras, como no app de hoje. 100 é "sem mexer".
struct ImageFilters: Equatable {
    struct Look: Equatable {
        var brightness = 100.0
        var contrast = 100.0
        var saturation = 100.0
        var blur = 0.0

        var untouched: Bool {
            self == Look()
        }

        init() {}

        init(_ saved: [String: Any]) {
            brightness = saved["brightness"] as? Double ?? brightness
            contrast = saved["contrast"] as? Double ?? contrast
            saturation = saved["saturation"] as? Double ?? saturation
            blur = saved["blur"] as? Double ?? blur
        }

        var saved: [String: Any] {
            ["brightness": brightness, "contrast": contrast, "saturation": saturation, "blur": blur]
        }
    }

    var screen = Look()
    var camera = Look()

    init() {}

    init(_ saved: [String: Any]) {
        screen = Look(saved["screen"] as? [String: Any] ?? [:])
        camera = Look(saved["camera"] as? [String: Any] ?? [:])
    }

    var saved: [String: Any] {
        ["screen": screen.saved, "camera": camera.saved]
    }
}
