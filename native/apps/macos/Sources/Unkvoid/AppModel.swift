import AppKit
import Foundation

enum Screen {
    case entry
    case hub
    case room
    case offline
    case updating
}

enum EntryAction {
    case create
    case join
}

enum HubModal {
    case account
    case serverSettings
}

/// O que a janela desenha, e o caminho de volta para o núcleo.
///
/// Aqui não se decide nada: o que é um código de sala válido, como se gera um, o que vai
/// no `join` e quem está na sala é do `shared/core`. Esta classe pega clique, manda para
/// lá e publica o que voltou.
@MainActor
final class AppModel: ObservableObject {
    @Published private(set) var screen: Screen = .updating
    @Published private(set) var busy: EntryAction?
    @Published private(set) var room: String?
    /// Ida e volta até o SFU. Enquanto não há medida a barra mostra `-- ms`, que é o
    /// que o React faz — o espaço já fica reservado e a barra não salta depois.
    @Published private(set) var ping: Int?
    @Published private(set) var roomError: String?
    @Published private(set) var entryError = ""
    /// O erro fica no campo que errou: o núcleo diz qual é (`nameIsEmpty`, `codeIsInvalid`)
    /// e a tela só o coloca no lugar.
    @Published private(set) var nameError = ""
    @Published private(set) var codeError = ""
    /// O núcleo diz de que campo é a recusa do Laravel (`{invalid: {field, message}}`), e
    /// o texto já vem escrito em português. Acender os dois campos esconderia qual errou.
    @Published private(set) var emailError = ""
    @Published private(set) var passwordError = ""
    @Published private(set) var updateStatus = "Conectando ao núcleo…"
    @Published private(set) var offlineStatus = ""
    @Published private(set) var lastEvent: String?
    @Published private(set) var eventCount = 0
    @Published private(set) var signedIn = false
    @Published private(set) var loginError = ""
    @Published private(set) var googleWaiting = false
    @Published var email = ""
    @Published var password = ""
    @Published var name = ""
    @Published var code = ""

    @Published private(set) var user: User?
    @Published private(set) var servers: [ServerSummary] = []
    @Published private(set) var serversLoading = false
    @Published private(set) var serversFailed = false
    @Published private(set) var tree: ServerTree?
    @Published private(set) var treeLoading = false
    @Published private(set) var channel: Channel?
    @Published private(set) var messages: [Message] = []
    @Published private(set) var messagesLoading = false
    @Published private(set) var messagesFailed = false
    @Published private(set) var sending = false
    @Published private(set) var recentRooms: [String] = []
    @Published private(set) var enteredRoomAt: Date?
    @Published private(set) var notice: String?
    @Published var home = true
    @Published var railOpen = true
    @Published var membersOpen = true
    @Published var modal: HubModal?

    /// O microfone e a saída escolhidos nesta sessão. Guardar a escolha entre uma abertura
    /// e outra é do `shared/core` (é preferência, como as do `Voice.ts`), e a ABI ainda não
    /// tem por onde — por isso aqui ela vive só enquanto a janela estiver aberta.
    @Published private(set) var microphones: [AudioDevice] = []
    @Published private(set) var speakers: [AudioDevice] = []
    @Published var microphone: AudioDevice.ID?
    @Published var speaker: AudioDevice.ID?

    private let core: Core?
    private let url: String
    private var pump: Task<Void, Never>?

    /// O núcleo não bloqueia a fila de eventos, então alguém tem de perguntar. Vinte vezes
    /// por segundo é bem mais do que um olho percebe e bem menos do que um quadro custa.
    private static let pumpInterval = Duration.milliseconds(50)

    init(url: String) {
        self.url = url
        core = try? Core()
    }

    deinit {
        pump?.cancel()
    }

    func start() async {
        guard let core else {
            screen = .offline
            offlineStatus = "o núcleo não subiu nesta máquina."

            return
        }

        updateStatus = "Conectando em \(url)…"

        let connected = await offMain { core.connect(to: self.url) }

        guard connected else {
            screen = .offline
            offlineStatus = "o SFU em \(url) não respondeu."

            return
        }

        startPump()
        refreshDevices()

        // Quem tem token guardado não devia ver o login de novo, e é o núcleo quem diz em
        // que tela se abre — a regra é dele (`Screen::home`), não desta classe.
        if await readState(), await ask("useServer", ["url": Self.server])["ok"] as? Bool == true {
            await loadServers()
        }
    }

    /// O que o núcleo diz que vale agora: a tela, o nome e se há conta.
    @discardableResult
    private func readState() async -> Bool {
        let state = await ask("state")

        name = state["name"] as? String ?? ""
        room = state["room"] as? String
        signedIn = state["signedIn"] as? Bool == true

        switch state["screen"] as? String {
        case "hub": screen = .hub
        case "room": screen = .room
        case "offline": screen = .offline
        case "updating": screen = .updating
        default: screen = .entry
        }

        return signedIn
    }

    func retry() async {
        screen = .updating
        await start()
    }

    func createRoom() async {
        await enterRoom(.create, "createRoom")
    }

    func joinRoom() async {
        await enterRoom(.join, "joinRoom")
    }

    func leaveRoom() async {
        _ = await ask("leaveRoom")

        roomError = nil
        enteredRoomAt = nil

        // Onde se cai ao sair da sala é decisão do núcleo, e não desta classe.
        await readState()
    }

    /// Entrar ou criar conta: o núcleo decide qual, e a diferença é só o caminho.
    func signIn(registering: Bool) async {
        loginError = ""
        emailError = ""
        passwordError = ""

        let answer = await ask("useServer", ["url": Self.server])

        guard answer["ok"] as? Bool == true else {
            loginError = "Não deu para falar com o servidor."

            return
        }

        let entered = await ask(registering ? "register" : "login", [
            "email": email,
            "password": password,
            "device": Self.device,
        ])

        if let invalid = entered["invalid"] as? [String: Any] {
            let message = invalid["message"] as? String ?? Self.sentence(for: "invalid")

            switch invalid["field"] as? String {
            case "email": emailError = message
            case "password": passwordError = message
            default: loginError = message
            }

            return
        }

        if let failed = entered["failed"] as? String {
            loginError = Self.sentence(for: failed)

            return
        }

        // Só o `ok` do núcleo entra: sem ele — uma ação que a ABI não conhece, por exemplo
        // — a pessoa entraria sem token nenhum e só descobriria na primeira tela vazia.
        guard entered["ok"] as? Bool == true else {
            loginError = Self.sentence(for: "")

            return
        }

        user = decode(entered["user"])
        signedIn = true
        password = ""
        screen = .hub

        await loadServers()
    }

    func googleLogin() async {
        googleWaiting = true
        loginError = "O login com Google ainda não está ligado neste app."
        googleWaiting = false
    }

    /// O motivo que o núcleo devolve, virado em frase. O núcleo não manda texto de tela —
    /// ele manda o porquê, e cada sistema escreve do seu jeito.
    static func sentence(for failure: String) -> String {
        switch failure {
        case "unreachable": "Não deu para falar com o servidor."
        case "signedOut": "Sua sessão expirou. Entre de novo."
        case "notAllowed": "Você não tem permissão para isso."
        case "gone": "Isso não existe mais."
        case "invalid": "Confira o que você digitou."
        case "tooFast": "Muitas tentativas. Espere um pouco."
        default: "Algo deu errado. Tente de novo."
        }
    }

    static let device = "macOS"

    static let server = ProcessInfo.processInfo.environment["UNKVOID_SERVER"] ?? "http://127.0.0.1:8000"

    func openHub() {
        screen = .hub
    }

    func openEntry() {
        screen = .entry
    }

    func dismissRoomError() {
        roomError = nil
    }

    /// Entrar numa sala é decisão do app (`unkvoid_app`), e não uma ação do SFU: quem
    /// valida o código, sorteia um e guarda a lista das recentes é o `shared/core`.
    private func enterRoom(_ action: EntryAction, _ command: String) async {
        guard busy == nil else {
            return
        }

        busy = action
        entryError = ""
        nameError = ""
        codeError = ""

        let answer = await ask(command, ["name": name, "code": code])

        busy = nil

        if let refused = answer["refused"] as? String {
            switch refused {
            case "nameIsEmpty": nameError = "Escreva o seu nome."
            case "codeIsInvalid": codeError = "Esse código não serve."
            default: entryError = Self.sentence(for: "")
            }

            return
        }

        guard let opened = answer["room"] as? String else {
            entryError = Self.sentence(for: answer["failed"] as? String ?? "")

            return
        }

        room = opened
        roomError = nil
        enteredRoomAt = Date()
        screen = .room
    }

    func openRoom(_ code: String) async {
        self.code = code

        await joinRoom()
    }

    func loadRecentRooms() async {
        recentRooms = decode(await ask("recentRooms")["rooms"]) ?? []
    }

    func loadServers() async {
        serversLoading = true

        let answer = await ask("servers")

        serversLoading = false
        serversFailed = answer["servers"] == nil

        if serversFailed {
            warn(answer)

            return
        }

        servers = decode(answer["servers"]) ?? []
    }

    func openServer(_ id: Int) async {
        guard tree?.id != id || home else {
            return
        }

        home = false
        treeLoading = true

        let answer = await ask("server", ["id": id])

        treeLoading = false

        guard let opened: ServerTree = decode(answer["server"]) else {
            warn(answer)
            home = true

            return
        }

        tree = opened

        if let first = opened.textChannels.first {
            await openChannel(first)
        } else {
            channel = nil
            messages = []
        }
    }

    func showHome() async {
        home = true
        channel = nil
        messages = []

        await loadRecentRooms()
    }

    func openChannel(_ opened: Channel) async {
        channel = opened
        messages = []

        guard !opened.isVoice else {
            // Voz é do `shared/core` e ele ainda não a tem: ver o relatório no `README.md`.
            notice = "A voz ainda não está ligada neste app."

            return
        }

        messagesLoading = true

        let answer = await ask("messages", ["channel": opened.id])

        messagesLoading = false
        messagesFailed = answer["messages"] == nil

        guard channel?.id == opened.id else {
            return
        }

        if messagesFailed {
            warn(answer)

            return
        }

        messages = decode(answer["messages"]) ?? []
    }

    func sendMessage(_ body: String) async -> Bool {
        guard let channel, !sending else {
            return false
        }

        sending = true

        let answer = await ask("sendMessage", ["channel": channel.id, "body": body])

        sending = false

        guard let sent: Message = decode(answer["message"]) else {
            warn(answer)

            return false
        }

        messages.append(sent)

        return true
    }

    func signOut() async {
        _ = await ask("signOut")

        user = nil
        signedIn = false
        servers = []
        tree = nil
        channel = nil
        messages = []
        home = true
        modal = nil
        // ponytail: `signOut` (e `login`) limpam o token no núcleo mas não mexem na tela,
        // então quem a move é esta classe. Teto: a regra de onde se cai fica em dois
        // lugares. Saída: o `ffi.rs` chamar `app.show(Screen::home(...))` nas duas ações, e
        // aqui virar um `readState()`, como em `leaveRoom`.
        screen = .entry
    }

    func copy(_ text: String, _ said: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        notice = said
    }

    func dismissNotice() {
        notice = nil
    }

    func refreshDevices() {
        microphones = Audio.inputs()
        speakers = Audio.outputs()
    }

    /// A resposta que não veio virando frase. O motivo é uma palavra do núcleo; caminho,
    /// endereço e status ficam no log dele.
    private func warn(_ answer: [String: Any]) {
        notice = answer["invalid"] as? String ?? Self.sentence(for: answer["failed"] as? String ?? "")
    }

    /// O JSON do núcleo virando tipo. A lista de um item existe porque `JSONSerialization`
    /// só serializa objeto ou array no topo, e o que vem pode ser qualquer um dos dois.
    private func decode<Value: Decodable>(_ value: Any?) -> Value? {
        guard let value, !(value is NSNull), let data = try? JSONSerialization.data(withJSONObject: [value]) else {
            return nil
        }

        return (try? JSONDecoder().decode([Value].self, from: data))?.first
    }

    /// As decisões do app (`unkvoid_app`), que não passam pelo SFU. Sempre fora da thread
    /// que desenha: as ações que falam com o Laravel bloqueiam até ele responder.
    private func ask(_ action: String, _ data: [String: Any] = [:]) async -> [String: Any] {
        guard let core else {
            return ["failed": "unreachable"]
        }

        let payload = JSONPayload(data)

        let answered: JSONPayload = await offMain {
            JSONPayload((try? core.app(action, payload.value)) ?? ["failed": "unreachable"])
        }

        return answered.value
    }

    private func startPump() {
        pump?.cancel()
        pump = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else {
                    return
                }

                await self.drain()

                try? await Task.sleep(for: AppModel.pumpInterval)
            }
        }
    }

    private func drain() async {
        guard let core else {
            return
        }

        while let name = await offMain({ core.nextEvent()?["event"] as? String }) {
            lastEvent = name
            eventCount += 1
        }
    }
}

/// `unkvoid_call` entra no runtime do Tokio e espera a resposta do servidor. Chamar isso
/// na main thread é a janela congelando enquanto a rede pensa.
private func offMain<Value: Sendable>(_ work: @escaping @Sendable () -> Value) async -> Value {
    await Task.detached(priority: .userInitiated, operation: work).value
}

/// `[String: Any]` não atravessa isolamento de ator; o JSON que ele carrega é imutável e
/// só um lado o toca por vez, então o `unchecked` aqui é verdade, não promessa vazia.
private struct JSONPayload: @unchecked Sendable {
    let value: [String: Any]

    init(_ value: [String: Any]) {
        self.value = value
    }
}
