import Foundation
import Testing

@testable import Unkvoid

/// O caminho que a tela percorre, com a pilha local no ar. Pulado quando não há servidor:
/// um teste que falha por falta de ambiente esconde o que falha por defeito.
@Suite(.serialized)
struct EndToEndTests {
    static let server = ProcessInfo.processInfo.environment["UNKVOID_SERVER"] ?? "http://127.0.0.1:8000"

    /// O núcleo grava na pasta do sistema, então sem isto o teste lê o estado real de quem
    /// roda: com um token guardado, `leaveRoom` cai no Hub em vez da Entry e o teste quebra
    /// por um motivo que não é o que ele verifica.
    static let isolated: Bool = {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("unkvoid-tests-\(UUID().uuidString)")

        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        setenv("UNKVOID_STATE_DIR", dir.path, 1)

        return true
    }()

    static var stackIsUp: Bool {
        guard let url = URL(string: "\(server)/health") else {
            return false
        }

        var reachable = false
        let waiting = DispatchSemaphore(value: 0)
        var request = URLRequest(url: url)

        request.timeoutInterval = 2

        URLSession.shared.dataTask(with: request) { _, response, _ in
            reachable = (response as? HTTPURLResponse)?.statusCode == 200
            waiting.signal()
        }.resume()

        _ = waiting.wait(timeout: .now() + 3)

        return reachable
    }

    /// Um núcleo já ligado ao SFU. O endereço é o que o Laravel anuncia; `UNKVOID_SFU` o
    /// troca quando o SFU da pilha local é outro.
    ///
    /// O chat pede `announced`: o Laravel publica no SFU que ele conhece, e é só nesse que
    /// uma mensagem chega a quem está inscrito.
    private static func connected(announced only: Bool = false) throws -> Core {
        #expect(isolated)

        let core = try Core()

        // Outro teste pode ter deixado uma conta na pasta de estado. Dois núcleos com a mesma
        // conta na mesma sala se substituem no servidor; aqui cada um é um convidado.
        try core.app("signOut")
        try core.app("useServer", ["url": server])

        let announced = try core.app("config")["sfu"] as? String
        let chosen = only ? nil : ProcessInfo.processInfo.environment["UNKVOID_SFU"]
        let address = chosen ?? announced ?? "ws://127.0.0.1:3000/sfu"

        try #require(core.connect(to: address), "não abriu o socket em \(address)")

        return core
    }

    @Test(.enabled(if: stackIsUp))
    func aRoomIsCreatedAndLeftThroughTheCore() throws {
        let core = try Self.connected()
        let created = try core.app("createRoom", ["name": "Ada", "code": ""])

        #expect(created["ok"] as? Bool == true)

        let code = try #require(created["room"] as? String)

        #expect(code.count == 12)
        #expect(try core.app("state")["screen"] as? String == "room")

        let room = try core.app("room")
        let peers = try #require(room["peers"] as? [[String: Any]])

        #expect(peers.count == 1)
        #expect(peers.first?["selfPeer"] as? Bool == true, "a pessoa se vê na sala")
        #expect((room["mine"] as? [String: Any])?["canShare"] as? Bool == true)

        try core.app("leaveRoom")

        #expect(try core.app("state")["screen"] as? String == "entry")
    }

    /// Duas pessoas na mesma sala por código: cada uma vê a outra, e quem sai some da lista
    /// de quem ficou — pelo aviso do núcleo, sem ninguém perguntar de novo.
    @Test(.enabled(if: stackIsUp))
    func twoPeopleInTheSameRoomSeeEachOther() throws {
        let (ada, grace) = (try Self.connected(), try Self.connected())
        let code = try #require(try ada.app("createRoom", ["name": "Ada", "code": ""])["room"] as? String)

        #expect(try grace.app("joinRoom", ["name": "Grace", "code": code])["ok"] as? Bool == true)

        let names = (try grace.app("room")["peers"] as? [[String: Any]] ?? []).compactMap { $0["name"] as? String }

        #expect(Set(names) == ["Ada", "Grace"])

        try grace.app("leaveRoom")

        var alone = false

        for _ in 0 ..< 40 where !alone {
            Thread.sleep(forTimeInterval: 0.05)

            while let event = ada.nextEvent() {
                if event["event"] as? String == "room.peers", let data = event["data"] as? [String: Any] {
                    alone = (data["peers"] as? [[String: Any]])?.count == 1
                }
            }
        }

        #expect(alone, "a Ada não soube que a Grace saiu")

        try ada.app("leaveRoom")
    }

    /// Uma conta de teste já dentro, ou `nil` quando o banco local não tem as duas contas
    /// semeadas — aí os testes de conta são pulados em vez de falharem por falta de dado.
    ///
    /// Um núcleo por conta para a suíte inteira: o Laravel limita o login por minuto, e entrar
    /// de novo a cada teste fazia a suíte falhar por pressa, não por defeito.
    private static func signedIn(_ email: String, announced: Bool = false) throws -> Core? {
        let key = "\(email)|\(announced)"

        if let kept = accounts.value[key] {
            return kept
        }

        let core = try connected(announced: announced)
        let entered = try core.app("login", ["email": email, "password": password, "device": "teste"])

        guard entered["ok"] as? Bool == true else {
            return nil
        }

        accounts.value[key] = core

        return core
    }

    private final class Accounts: @unchecked Sendable {
        var value: [String: Core] = [:]
    }

    private static let accounts = Accounts()

    private static let password = ProcessInfo.processInfo.environment["UNKVOID_TEST_PASSWORD"] ?? ""

    /// O primeiro canal do tipo pedido num servidor em que as duas contas estão.
    private static func sharedChannel(_ core: Core, type: String) throws -> String? {
        let servers = try core.app("servers")["servers"] as? [[String: Any]] ?? []

        for server in servers {
            let tree = try core.app("server", ["id": server["id"] as? Int ?? 0])["server"] as? [String: Any] ?? [:]
            let channels = tree["channels"] as? [[String: Any]] ?? []

            if (tree["members"] as? [[String: Any]])?.count ?? 0 > 1, let found = channels.first(where: { $0["type"] as? String == type }) {
                return found["id"] as? String
            }
        }

        return nil
    }

    /// O chat em tempo real de ponta a ponta: a Ada escreve pelo Laravel, e o socket da Grace
    /// — identificado e inscrito no canal — ouve a mensagem sem perguntar.
    @Test(.enabled(if: stackIsUp && !password.isEmpty))
    func aMessageReachesWhoIsSubscribedToTheChannel() throws {
        guard
            let ada = try Self.signedIn("ada@teste.local", announced: true),
            let grace = try Self.signedIn("grace@teste.local", announced: true)
        else {
            return
        }

        let channel = try #require(try Self.sharedChannel(grace, type: "text"))

        #expect(try grace.app("identify")["ok"] as? Bool == true)
        #expect(try grace.call("subscribe", ["channel": "channel.\(channel)"])["channel"] as? String == "channel.\(channel)")

        let body = "tempo real \(UUID().uuidString.prefix(8))"
        let sent = try ada.app("sendMessage", ["channel": channel, "body": body])
        let id = try #require((sent["message"] as? [String: Any])?["id"] as? Int)

        var heard = false

        for _ in 0 ..< 60 where !heard {
            Thread.sleep(forTimeInterval: 0.05)

            while let event = grace.nextEvent() {
                let message = (event["data"] as? [String: Any])?["message"] as? [String: Any]

                heard = heard || (event["event"] as? String == "MessageSent" && message?["body"] as? String == body)
            }
        }

        #expect(heard, "a Grace não ouviu a mensagem da Ada")
        #expect(try ada.app("deleteMessage", ["id": id])["ok"] as? Bool == true)
    }

    /// A voz de ponta a ponta: a Ada entra no canal, abre o microfone e fala um tom; a Grace
    /// entra no mesmo canal e recebe som já decodificado.
    @Test(.enabled(if: stackIsUp && !password.isEmpty))
    func whoSpeaksInAVoiceChannelIsHeardByWhoIsThere() throws {
        guard let ada = try Self.signedIn("ada@teste.local"), let grace = try Self.signedIn("grace@teste.local") else {
            return
        }

        let channel = try #require(try Self.sharedChannel(ada, type: "voice"))

        #expect(try ada.app("joinVoice", ["channel": channel])["ok"] as? Bool == true)
        #expect((try ada.app("room")["mine"] as? [String: Any])?["canSpeak"] as? Bool == true)
        #expect(try ada.app("openMicrophone")["ok"] as? Bool == true)
        #expect(try grace.app("joinVoice", ["channel": channel])["ok"] as? Bool == true)

        let tone = (0 ..< 1920).map { Float(sin(Double($0) * 0.05)) * 0.4 }
        var blocks = 0

        for _ in 0 ..< 150 where blocks < 5 {
            tone.withUnsafeBufferPointer { ada.speak($0) }
            Thread.sleep(forTimeInterval: 0.02)

            if let media = grace.nextMedia(), case .audio = media.kind, !media.data.isEmpty {
                blocks += 1
            }
        }

        #expect(blocks >= 5, "a Grace recebeu \(blocks) blocos de som da Ada")

        try ada.app("leaveRoom")
        try grace.app("leaveRoom")
    }

    /// Escrever num servidor pelo mapa de rotas do núcleo: criar, ganhar um canal e um cargo,
    /// trocar o nome, e apagar. O que a pessoa pode fazer ali vem calculado junto da árvore.
    @Test(.enabled(if: stackIsUp && !password.isEmpty))
    func aServerIsCreatedChangedAndDeletedThroughTheCore() throws {
        guard let ada = try Self.signedIn("ada@teste.local") else {
            return
        }

        func call(_ name: String, _ params: [String: Any] = [:], _ body: [String: Any] = [:]) throws -> [String: Any] {
            let answer = try ada.app("api", ["name": name, "params": params, "body": body])

            #expect(answer["ok"] as? Bool == true, "\(name) falhou: \(answer)")

            return answer["data"] as? [String: Any] ?? [:]
        }

        let id = try #require(try call("createServer", [:], ["name": "Teste \(UUID().uuidString.prefix(6))"])["id"] as? Int)

        _ = try call("createChannel", ["server": id], ["name": "planos", "type": "text"])
        _ = try call("createRole", ["server": id], ["name": "Moderação", "color": "#8a7cf5", "permissions": 1 << 4])
        _ = try call("updateServer", ["server": id], ["name": "Renomeado"])

        let opened = try ada.app("server", ["id": id])
        let tree = try #require(opened["server"] as? [String: Any])
        let abilities = try #require(opened["abilities"] as? [String: Any])

        #expect(tree["name"] as? String == "Renomeado")
        #expect((tree["channels"] as? [[String: Any]])?.contains { $0["name"] as? String == "planos" } == true)
        #expect(abilities["owner"] as? Bool == true)
        #expect((abilities["can"] as? [String])?.contains("manageChannels") == true)

        #expect(try ada.app("api", ["name": "deleteServer", "params": ["server": id]])["ok"] as? Bool == true)
        #expect(try ada.app("api", ["name": "deleteServer", "params": ["server": "../me"]])["failed"] as? String == "invalid", "parâmetro hostil não sai do núcleo")
    }

    /// Compartilhar a tela de ponta a ponta: a Ada compartilha pela mesma ação que o botão
    /// usa, e a Grace recebe quadros H.264 inteiros, o primeiro deles um keyframe. Precisa da
    /// permissão de gravação de tela, e por isso só roda com `UNKVOID_TEST_SHARE=1`.
    @Test(.enabled(if: stackIsUp && ProcessInfo.processInfo.environment["UNKVOID_TEST_SHARE"] == "1"))
    func aSharedScreenArrivesAsWholeFrames() throws {
        let (ada, grace) = (try Self.connected(), try Self.connected())
        let code = try #require(try ada.app("createRoom", ["name": "Ada", "code": ""])["room"] as? String)

        #expect(try grace.app("joinRoom", ["name": "Grace", "code": code])["ok"] as? Bool == true)

        let displays = try ada.app("displays")["displays"] as? [[String: Any]] ?? []

        try #require(!displays.isEmpty, "o sistema não listou tela nenhuma: falta a permissão de gravação")

        let shared = try ada.app("share", ["quality": "720", "fps": 30, "audio": false])

        #expect(shared["ok"] as? Bool == true, "a tela não subiu: \(shared)")
        #expect((try ada.app("room")["mine"] as? [String: Any])?["sharing"] as? Bool == true)

        var frames = 0
        var firstWasKeyframe: Bool?

        for _ in 0 ..< 80 where frames < 20 {
            guard let media = grace.nextMedia(), case let .video(keyframe, _) = media.kind else {
                continue
            }

            firstWasKeyframe = firstWasKeyframe ?? keyframe
            frames += 1

            #expect(!VideoSink.nals(media.data).isEmpty, "o quadro veio sem NAL nenhum")
        }

        #expect(frames >= 20, "a Grace recebeu só \(frames) quadros")
        #expect(firstWasKeyframe == true, "o primeiro quadro tem de ser um keyframe, senão nada desenha")

        let tiles = try grace.app("room")["tiles"] as? [[String: Any]] ?? []

        #expect(tiles.count == 1 && tiles.first?["label"] as? String == "Ada")

        try ada.app("stopSharing")
        try ada.app("leaveRoom")
        try grace.app("leaveRoom")
    }

    /// Sem SFU não há sala: a tela não pode mudar para uma sala vazia.
    @Test(.enabled(if: stackIsUp))
    func aRoomWithoutAnSfuIsRefusedAndTheScreenStays() throws {
        #expect(Self.isolated)

        let core = try Core()

        // Outro teste pode ter deixado uma conta na pasta de estado, e com conta cai-se no Hub.
        try core.app("signOut")

        #expect(try core.app("createRoom", ["name": "Ada", "code": ""])["failed"] as? String == "unreachable")
        #expect(try core.app("state")["screen"] as? String == "entry")
    }

    @Test(.enabled(if: stackIsUp))
    func enteringWithoutANameIsRefusedWithAReasonAndNotATechnicalMessage() throws {
        let core = try Core()
        let answer = try core.app("createRoom", ["name": "   ", "code": ""])

        #expect(answer["refused"] as? String == "nameIsEmpty")
        #expect(answer["ok"] == nil)
    }

    @Test(.enabled(if: stackIsUp))
    func theSfuAddressComesFromLaravelAndTheSocketOpens() throws {
        let core = try Core()

        #expect(try core.app("useServer", ["url": Self.server])["ok"] as? Bool == true)

        let address = try #require(try core.app("config")["sfu"] as? String, "o Laravel não disse onde o SFU fica")

        #expect(address.hasPrefix("ws"))
        #expect(try Self.connected().call("ping").isEmpty)
    }

    @Test(.enabled(if: stackIsUp))
    func askingForServersWithoutAnAccountFailsWithoutLeakingAnything() throws {
        let core = try Core()

        try core.app("signOut")
        try core.app("useServer", ["url": Self.server])

        let answer = try core.app("servers")
        let failed = answer["failed"] as? String

        #expect(failed != nil, "devia recusar sem conta")
        // O motivo é uma palavra; caminho, endereço e status ficam no log.
        #expect(failed?.contains("http") == false)
        #expect(failed?.rangeOfCharacter(from: .decimalDigits) == nil)
    }
}
