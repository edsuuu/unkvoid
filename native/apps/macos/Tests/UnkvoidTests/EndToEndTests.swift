import Foundation
import Testing

@testable import Unkvoid

/// O caminho que a tela percorre, com a pilha local no ar. Pulado quando não há servidor:
/// um teste que falha por falta de ambiente esconde o que falha por defeito.
struct EndToEndTests {
    static let server = ProcessInfo.processInfo.environment["UNKVOID_SERVER"] ?? "http://127.0.0.1:8000"

    /// O núcleo grava na pasta do sistema, então sem isto o teste lê o estado real de quem
    /// roda: com um token guardado, `leaveRoom` cai no Hub em vez da Entry e o teste quebra
    /// por um motivo que não é o que ele verifica.
    private static let isolated: Bool = {
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

    @Test(.enabled(if: stackIsUp))
    func aRoomIsCreatedAndLeftThroughTheCore() throws {
        #expect(Self.isolated)

        let core = try Core()
        let created = try core.app("createRoom", ["name": "Ada", "code": ""])

        #expect(created["ok"] as? Bool == true)

        let code = try #require(created["room"] as? String)

        #expect(code.count == 12)
        #expect(try core.app("state")["screen"] as? String == "room")

        try core.app("leaveRoom")

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

        let answer = try core.app("config")
        let address = answer["sfu"] as? String ?? "ws://127.0.0.1:3000/sfu"

        #expect(core.connect(to: address), "não abriu o socket em \(address)")
        #expect(try core.call("ping").isEmpty)
    }

    @Test(.enabled(if: stackIsUp))
    func askingForServersWithoutAnAccountFailsWithoutLeakingAnything() throws {
        let core = try Core()

        try core.app("useServer", ["url": Self.server])

        let answer = try core.app("servers")
        let failed = answer["failed"] as? String

        #expect(failed != nil, "devia recusar sem conta")
        // O motivo é uma palavra; caminho, endereço e status ficam no log.
        #expect(failed?.contains("http") == false)
        #expect(failed?.rangeOfCharacter(from: .decimalDigits) == nil)
    }
}
