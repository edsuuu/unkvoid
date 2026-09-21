import AppKit
import UserNotifications

/// O aviso do sistema para o que chega com a janela fora da frente — uma mensagem direta,
/// um pedido de amizade. Só existe dentro do `.app`: o centro de notificações do macOS
/// derruba um executável solto que tente usá-lo.
@MainActor
enum SystemNotices {
    private static var bundled: Bool {
        Bundle.main.bundlePath.hasSuffix(".app")
    }

    static func ask() {
        guard bundled else {
            return
        }

        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    /// Com a janela na frente o aviso de dentro dela basta.
    static func show(_ title: String, _ body: String) {
        guard bundled, !NSApp.isActive else {
            return
        }

        let content = UNMutableNotificationContent()

        content.title = title
        content.body = body

        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
    }
}
