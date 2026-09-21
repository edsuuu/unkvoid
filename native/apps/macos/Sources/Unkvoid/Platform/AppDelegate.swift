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

    /// O endereço do SFU vem do `GET /api/config` do Laravel; este é o de quando ele não
    /// responde, e pode vir pela linha de comando.
    static func socketUrl() -> String {
        chosenSocketUrl() ?? defaultSocketUrl
    }

    /// O SFU que quem abriu o app escolheu, pela linha de comando ou por `UNKVOID_SFU`. Vale
    /// mais do que o que o Laravel anuncia: é para desenvolver contra outro SFU.
    static func chosenSocketUrl() -> String? {
        CommandLine.arguments.dropFirst().first(where: { $0.contains("://") }) ?? ProcessInfo.processInfo.environment["UNKVOID_SFU"]
    }

    /// `UNKVOID_JOIN=codigo:nome` entra direto numa sala ao abrir. É para desenvolver: abrir
    /// duas janelas na mesma sala sem digitar nada em nenhuma.
    static func autoJoin() -> (code: String, name: String)? {
        guard let parts = ProcessInfo.processInfo.environment["UNKVOID_JOIN"]?.split(separator: ":", maxSplits: 1), parts.count == 2 else {
            return nil
        }

        return (String(parts[0]), String(parts[1]))
    }

    /// `UNKVOID_OPEN=servidor` abre esse servidor ao abrir, com a conta já guardada;
    /// `UNKVOID_OPEN=servidor:canal` também entra na voz. É para desenvolver.
    static func autoOpen() -> (server: Int, voice: String?)? {
        guard
            let parts = ProcessInfo.processInfo.environment["UNKVOID_OPEN"]?.split(separator: ":", maxSplits: 1),
            let first = parts.first,
            let server = Int(first)
        else {
            return nil
        }

        return (server, parts.count == 2 ? String(parts[1]) : nil)
    }
}
