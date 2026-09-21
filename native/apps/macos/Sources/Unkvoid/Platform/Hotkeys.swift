import CoreGraphics
import Foundation

/// As teclas que valem com o jogo na frente.
///
/// O teclado é **consultado**, a cada 20 ms, e nunca capturado: registrar o atalho no sistema
/// faria ele engolir a tecla, e apertar "mutar" não pode cancelar o movimento do personagem.
/// Quais números formam cada atalho é o núcleo quem diz (`keymap.rs`).
@MainActor
final class Hotkeys {
    struct Combo {
        let key: CGKeyCode
        /// Cada grupo é um modificador: basta uma tecla do grupo apertada.
        let modifiers: [[CGKeyCode]]

        var isDown: Bool {
            CGEventSource.keyState(.combinedSessionState, key: key)
                && modifiers.allSatisfy { group in group.contains { CGEventSource.keyState(.combinedSessionState, key: $0) } }
        }
    }

    private var pressed: [String: () -> Void] = [:]
    private var held: [String: (Bool) -> Void] = [:]
    private var combos: [String: Combo] = [:]
    private var down: Set<String> = []
    private var timer: Timer?

    /// Dispara uma vez a cada vez que o atalho desce.
    func onPress(_ name: String, _ combo: Combo?, _ action: @escaping () -> Void) {
        combos[name] = combo
        pressed[name] = action
        watch()
    }

    /// Avisa quando o atalho desce e quando sobe: é o "apertar para falar".
    func onHold(_ name: String, _ combo: Combo?, _ action: @escaping (Bool) -> Void) {
        combos[name] = combo
        held[name] = action
        watch()
    }

    private func watch() {
        guard timer == nil else {
            return
        }

        timer = Timer.scheduledTimer(withTimeInterval: 0.02, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.look() }
        }
    }

    private func look() {
        for (name, combo) in combos {
            let now = combo.isDown

            guard now != down.contains(name) else {
                continue
            }

            if now {
                down.insert(name)
                pressed[name]?()
            } else {
                down.remove(name)
            }

            held[name]?(now)
        }
    }
}
