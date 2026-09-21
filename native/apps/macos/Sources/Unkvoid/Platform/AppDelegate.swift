import AppKit

/// Um executável do SwiftPM não é um `.app` empacotado: sem isto ele sobe sem menu, sem
/// ícone na doca e com a janela atrás de quem estiver na frente.
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApplication.shared.setActivationPolicy(.regular)
        NSApplication.shared.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }
}

enum Launch {
    static let defaultSocketUrl = "ws://127.0.0.1:3000/sfu"

    /// Enquanto o núcleo não expõe o `GET /api/config` do Laravel, o endereço do SFU entra
    /// pela linha de comando. Ver o relatório em `README.md`.
    static func socketUrl() -> String {
        CommandLine.arguments.dropFirst().first(where: { $0.contains("://") }) ?? defaultSocketUrl
    }
}
