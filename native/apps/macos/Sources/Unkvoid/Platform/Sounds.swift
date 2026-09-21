import AppKit

/// Os sons curtos do app: alguém entrou, alguém saiu, mensagem nova. São os do próprio
/// sistema — a pessoa já os conhece, e eles respeitam o volume de alertas dela.
@MainActor
enum Sounds {
    static func joined() {
        NSSound(named: "Pop")?.play()
    }

    static func left() {
        NSSound(named: "Bottle")?.play()
    }

    static func message() {
        NSSound(named: "Tink")?.play()
    }
}
