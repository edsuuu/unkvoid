import AppKit
import Combine
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

enum HubModal: Equatable {
    case account
    case serverSettings
    case invite
    case logs
}

/// O que a janela desenha, e o caminho de volta para o núcleo.
///
/// Aqui não se decide nada: o que é um código de sala válido, como se gera um, o que vai
/// no `join` e quem está na sala é do `shared/core`. Esta classe pega clique, manda para
/// lá e publica o que voltou.
@MainActor
final class AppModel: ObservableObject {
    /// Trocar de tela apaga o que a anterior estava dizendo: o erro de entrar numa sala não
    /// pode reaparecer no Hub, nem o aviso do Hub dentro da sala.
    @Published private(set) var screen: Screen = .updating {
        didSet {
            if oldValue != screen {
                forgetErrors()
            }
        }
    }
    @Published private(set) var busy: EntryAction?
    @Published private(set) var room: String?
    /// Ida e volta até o SFU. Enquanto não há medida a barra mostra `-- ms`, que é o
    /// que o React faz — o espaço já fica reservado e a barra não salta depois.
    @Published var ping: Int?
    /// De 1 a 4, contado pelo núcleo a partir do ping: a interface só escolhe a cor.
    @Published var signalBars: Int?
    @Published var roomError: String?
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

    @Published var user: User?
    @Published var servers: [ServerSummary] = []
    @Published private(set) var serversLoading = false
    @Published private(set) var serversFailed = false
    @Published var tree: ServerTree?
    @Published private(set) var treeLoading = false
    @Published private(set) var recentRooms: [String] = []
    @Published var enteredRoomAt: Date?
    @Published var notice: String?
    @Published var imageFilters = ImageFilters() {
        didSet {
            if oldValue != imageFilters {
                remember("unkvoid:image", imageFilters.saved)
            }
        }
    }

    @Published var voicePreferences = VoicePreferences()
    @Published var noticePreferences = NoticePreferences()
    @Published var nicknameError = ""
    @Published var nicknameBusy = false
    let hotkeys = Hotkeys()

    @Published var abilities = Abilities()
    @Published var audits: [Audit] = []
    @Published var auditsLoading = false
    @Published var auditsFailed = false
    /// O que a pessoa precisa confirmar antes de acontecer: apagar, sair, expulsar.
    @Published var confirmation: Confirmation?
    @Published var channelEditor: ChannelEditor?
    @Published var roleEditor: RoleEditor?
    @Published var memberMenu: Member?
    @Published var homeTab = HomeTab.servers
    @Published var friends: [Friendship] = []
    @Published var friendsLoading = false
    @Published var friendsFailed = false
    @Published var conversations: [DirectConversation] = []
    @Published var conversationsFailed = false
    @Published var directPerson: Person?
    @Published var directMessages: [DirectMessage] = []
    @Published var directLoading = false
    @Published var directFailed = false

    /// Quem está com o app aberto neste servidor, pela presença do tempo real.
    @Published var online: Set<String> = []
    @Published var home = true
    @Published var railOpen = true {
        didSet { remember("unkvoid:rail", railOpen) }
    }

    @Published var membersOpen = true {
        didSet { remember("unkvoid:members", membersOpen) }
    }

    /// A versão mais nova publicada, quando há: o aviso no topo do Hub leva ao instalador.
    @Published var newerVersion: (version: String, url: URL)?
    @Published var modal: HubModal?

    /// O microfone e a saída em uso, como o CoreAudio os numera agora. A escolha que fica
    /// guardada é o nome do aparelho, em `voicePreferences`.
    @Published private(set) var microphones: [AudioDevice] = []
    @Published private(set) var speakers: [AudioDevice] = []
    @Published var microphone: AudioDevice.ID? {
        didSet { Task { await microphoneChanged() } }
    }

    @Published var speaker: AudioDevice.ID? {
        didSet { media?.sound.use(speaker: speaker) }
    }

    /// O canal de voz em que se está. A sala dele é a mesma `Room` do núcleo, desenhada
    /// dentro do servidor em vez de tomar a janela.
    @Published var voiceChannel: Channel?
    @Published var voiceJoining = false
    /// O canal em que se está entrando, para a rodinha aparecer no item certo.
    @Published var voiceTarget: String?
    /// Os canais de voz seguidos no tempo real, para ver quem entra e sai de cada um.
    var followedVoice: Set<String> = []
    @Published var stageOpen = false

    /// A sala aberta, como o núcleo a anuncia. Quem decide cada coisa aqui é ele; a tela
    /// só redesenha o que chega.
    @Published var peers: [RoomPeer] = []
    @Published var tiles: [RoomTile] = []
    /// O que está ao vivo e a pessoa fechou: volta pelo "Assistir".
    @Published var pendingTiles: [RoomTile] = []
    @Published var fullscreenTile: String?
    /// Quem está assistindo a cada tela, pelo producer dela.
    @Published var watchers: [String: [String]] = [:]
    /// A voz tomando o Hub inteiro, com a barra da sala em cima.
    @Published var focusedRoom = false
    /// O convite do servidor recém-criado, para mandar a alguém já.
    @Published var inviteBanner = false
    /// O volume de cada pessoa na voz, só deste lado, pela chave `user:<id>`.
    @Published var voiceVolumes: [String: Float] = [:]
    /// O volume de cada tela, de 0 a 1, só deste lado.
    @Published var tileVolumes: [String: Float] = [:]
    @Published var sharePreviews: [String: NSImage] = [:]
    @Published var mine = Mine()
    @Published var reconnecting = false
    @Published var deafened = false
    /// Entre o clique no canal e o microfone abrir: o botão ainda não tem o que mostrar, e
    /// pintá-lo de "mudo" nesse meio segundo é o pisca que ninguém pediu.
    @Published var micOpening = false
    /// Os producers de microfone de quem está falando agora, medidos no som que chega.
    @Published var speakingProducers: Set<String> = []
    /// Mudo escolhido fora de uma sala: não há microfone para calar ainda, então fica guardado
    /// e vale no instante em que ele abrir. Dentro da sala quem sabe é o `mine.micMuted`.
    @Published var mutedAtRest = false
    @Published var micLevel: Float = 0
    /// O mesmo nível na escala de 0 a 100 da sensibilidade.
    @Published var micPercent = 0
    @Published var focusedTile: String?
    /// As telas cujo som a pessoa ligou: ele chega mudo por regra.
    @Published var heardTiles: Set<String> = []

    /// O seletor de "compartilhar tela". A escolha vive só enquanto a janela está aberta.
    @Published var shareOpen = false
    @Published var shareLoading = false
    @Published var shareStarting = false
    @Published var shareTab = "display"
    @Published var shareDisplays: [ShareSource] = []
    @Published var shareWindows: [ShareSource] = []
    @Published var shareSource: String?
    @Published var shareQuality = "1080"
    @Published var shareFps = 60
    @Published var shareAudio = true
    @Published var shareMuteCalls = true

    let media: MediaRouter?

    /// O chat do canal de texto aberto, e o do canal de voz em que se está. O que muda neles
    /// redesenha quem observa este modelo, para a coluna dos canais acender o canal certo.
    private(set) lazy var chat = watched(ChatRoom(model: self))
    private(set) lazy var voiceChat = watched(ChatRoom(model: self))
    /// O painel de chat ao lado do palco da voz.
    @Published var voiceChatOpen = false

    private var watching: [AnyCancellable] = []

    let core: Core?
    private let url: String
    private var pump: Task<Void, Never>?

    /// O núcleo não bloqueia a fila de eventos, então alguém tem de perguntar. Vinte vezes
    /// por segundo é bem mais do que um olho percebe e bem menos do que um quadro custa.
    private static let pumpInterval = Duration.milliseconds(50)

    init(url: String) {
        self.url = url

        let core = try? Core()

        self.core = core
        media = core.map(MediaRouter.init)

        media?.sound.onSpeaking = { [weak self] producer, speaking in
            Task { @MainActor in
                if speaking {
                    self?.speakingProducers.insert(producer)
                } else {
                    self?.speakingProducers.remove(producer)
                }
            }
        }
    }

    deinit {
        pump?.cancel()
    }

    private func watched(_ room: ChatRoom) -> ChatRoom {
        room.objectWillChange.sink { [weak self] in self?.objectWillChange.send() }.store(in: &watching)

        return room
    }

    /// O canal de texto aberto. Quem o abre e fecha é o `chat`.
    var channel: Channel? {
        chat.channel
    }

    /// Quem sabe onde o SFU fica é o Laravel. Sem ele (sala por código, servidor fora do ar)
    /// vale o endereço da linha de comando: esse caminho não pode depender de conta.
    func start() async {
        guard let core else {
            screen = .offline
            offlineStatus = "o núcleo não subiu nesta máquina."

            return
        }

        await loadPreferences()

        let reachable = await ask("useServer", ["url": Self.server])["ok"] as? Bool == true
        let announced = reachable ? await ask("config")["sfu"] as? String : nil
        let sfu = Launch.chosenSocketUrl() ?? announced ?? url

        updateStatus = "Conectando em \(sfu)…"

        let connected = await offMain { core.connect(to: sfu) }

        guard connected else {
            screen = .offline
            offlineStatus = "o servidor de mídia não respondeu."

            return
        }

        startPump()

        // Quem tem token guardado não devia ver o login de novo, e é o núcleo quem diz em
        // que tela se abre — a regra é dele (`Screen::home`), não desta classe.
        if await readState(), reachable {
            await restoreAccount()
        }

        if let join = Launch.autoJoin() {
            name = join.name
            code = join.code

            await joinRoom()
        }

        if let wanted = Launch.autoOpen(), signedIn {
            await openServer(wanted.server)

            if let channel = tree?.voiceChannels.first(where: { $0.id == wanted.voice }) {
                await joinVoice(channel)
            }
        }
    }

    /// A conta do token guardado. Se ele não vale mais, o núcleo o apaga e a tela volta
    /// para onde quem não tem conta começa.
    private func restoreAccount() async {
        let answer = await ask("me")

        guard let restored: User = decode(answer["user"]) else {
            if answer["failed"] as? String == "signedOut" {
                await readState()
            }

            return
        }

        user = restored

        await loadServers()
        await connectChat()
        await checkForUpdate()
    }

    /// O que o núcleo diz que vale agora: a tela, o nome e se há conta.
    @discardableResult
    func readState() async -> Bool {
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
        closeRoom()

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
        password = ""

        await readState()
        await loadServers()
        await connectChat()
        await checkForUpdate()
    }

    /// Abre o navegador na conta do Google e espera a pessoa voltar. O token chega ao núcleo
    /// por uma porta local; esta classe só abre o endereço e publica quem entrou.
    func googleLogin() async {
        guard !googleWaiting else {
            return
        }

        loginError = ""

        guard await ask("useServer", ["url": Self.server])["ok"] as? Bool == true,
              let address = await ask("googleStart")["url"] as? String,
              let url = URL(string: address)
        else {
            loginError = Self.sentence(for: "unreachable")

            return
        }

        googleWaiting = true
        NSWorkspace.shared.open(url)

        let entered = await ask("googleWait")

        googleWaiting = false

        guard let signed: User = decode(entered["user"]) else {
            loginError = "O login com Google não terminou. Tente de novo."

            return
        }

        NSApp.activate(ignoringOtherApps: true)

        user = signed

        await readState()
        await loadServers()
        await connectChat()
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

    /// O site de verdade, salvo quem desenvolve: o `run.sh` passa `UNKVOID_SERVER` com o
    /// Laravel local. O `.app` instalado pelo site não tem variável nenhuma.
    static let server = ProcessInfo.processInfo.environment["UNKVOID_SERVER"] ?? "https://unkvoid.com"

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

        await openedRoom()
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

        // O núcleo guarda a árvore de cada servidor desde a abertura: a resposta volta na hora
        // e os canais não piscam. O esqueleto só aparece se a rede for mesmo necessária.
        let slow = Task {
            try await Task.sleep(for: .milliseconds(150))
            treeLoading = true
        }

        let answer = await ask("server", ["id": id, "known": true])

        slow.cancel()
        treeLoading = false

        guard let opened: ServerTree = decode(answer["server"]) else {
            warn(answer)
            home = true

            return
        }

        if let previous = tree?.id, previous != opened.id {
            await unfollow("server.\(previous)")
        }

        tree = opened
        abilities = decode(answer["abilities"]) ?? Abilities()
        online = []

        await follow("server.\(opened.id)")
        await followVoiceChannels()

        if let first = opened.textChannels.first {
            await openChannel(first)
        } else {
            await chat.close()
        }

        if answer["known"] as? Bool == true {
            await reloadTree()
        }
    }

    func showHome() async {
        home = true

        await chat.close()

        await loadRecentRooms()
    }

    func openChannel(_ opened: Channel) async {
        guard !opened.isVoice else {
            await joinVoice(opened)

            return
        }

        stageOpen = false

        await chat.open(opened)
    }

    func signOut() async {
        await leaveVoice()

        _ = await ask("signOut")

        user = nil
        signedIn = false
        servers = []
        tree = nil
        home = true
        modal = nil

        await chat.close()

        await readState()
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
    func warn(_ answer: [String: Any]) {
        let invalid = (answer["invalid"] as? [String: Any])?["message"] as? String

        notice = invalid ?? Self.sentence(for: answer["failed"] as? String ?? "")
    }

    /// Tudo o que alguma tela estava avisando. Cada erro tem o lugar dele — o campo, a sala, o
    /// aviso da tela — e nenhum atravessa para a tela seguinte.
    func forgetErrors() {
        notice = nil
        roomError = nil
        entryError = ""
        nameError = ""
        codeError = ""
        loginError = ""
        emailError = ""
        passwordError = ""
        nicknameError = ""
    }

    /// O JSON do núcleo virando tipo. A lista de um item existe porque `JSONSerialization`
    /// só serializa objeto ou array no topo, e o que vem pode ser qualquer um dos dois.
    func decode<Value: Decodable>(_ value: Any?) -> Value? {
        guard let value, !(value is NSNull), let data = try? JSONSerialization.data(withJSONObject: [value]) else {
            return nil
        }

        return (try? JSONDecoder().decode([Value].self, from: data))?.first
    }

    /// As decisões do app (`unkvoid_app`), que não passam pelo SFU. Sempre fora da thread
    /// que desenha: as ações que falam com o Laravel bloqueiam até ele responder.
    func ask(_ action: String, _ data: [String: Any] = [:]) async -> [String: Any] {
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

        while let event = await offMain({ core.nextEvent().map(JSONPayload.init) }) {
            lastEvent = event.value["event"] as? String
            eventCount += 1

            heard(event.value)
        }
    }

    func say(_ sentence: String) {
        notice = sentence
    }

    func complain(_ sentence: String?) {
        roomError = sentence
    }
}

/// `unkvoid_call` entra no runtime do Tokio e espera a resposta do servidor. Chamar isso
/// na main thread é a janela congelando enquanto a rede pensa.
func offMain<Value: Sendable>(_ work: @escaping @Sendable () -> Value) async -> Value {
    await Task.detached(priority: .userInitiated, operation: work).value
}

/// `[String: Any]` não atravessa isolamento de ator; o JSON que ele carrega é imutável e
/// só um lado o toca por vez, então o `unchecked` aqui é verdade, não promessa vazia.
struct JSONPayload: @unchecked Sendable {
    let value: [String: Any]

    init(_ value: [String: Any]) {
        self.value = value
    }
}
