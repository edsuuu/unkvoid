import Foundation

/// Os toques do app são os do núcleo (`chimes.rs`, os mesmos do React): a sala diz qual toque
/// (`room.chime`) e a interface só pede o PCM pelo nome e o toca pelo motor de saída da sala —
/// no fone escolhido, igual nas três plataformas.
@MainActor
enum Sounds {
    /// O motor por onde tocar. Sem ele (sem núcleo) o app fica em silêncio.
    static var output: Sound?

    private static var cached: [String: Data] = [:]

    static func play(_ chime: String) {
        guard let output else {
            return
        }

        if cached[chime] == nil {
            cached[chime] = Core.chime(chime)
        }

        guard let samples = cached[chime] else {
            return
        }

        output.chime(samples)
    }

    static func joined() {
        play("joined")
    }

    static func left() {
        play("left")
    }

    static func message() {
        play("message")
    }
}
