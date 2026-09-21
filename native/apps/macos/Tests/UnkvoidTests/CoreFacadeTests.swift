import XCTest

@testable import Unkvoid

/// A menor coisa que quebra se a ponte para o Rust quebrar: a ABI linkou, o handle nasce,
/// e cada string que o núcleo devolve volta uma vez só.
///
/// Não precisa de SFU no ar de propósito — um teste que só passa com servidor não é uma
/// verificação, é um lembrete.
final class CoreFacadeTests: XCTestCase {
    func testTheCoreIsBornAlive() throws {
        XCTAssertNoThrow(try Core())
    }

    func testCallingBeforeConnectingFailsInsteadOfCrashing() throws {
        let core = try Core()

        XCTAssertThrowsError(try core.call("ping"))
    }

    func testAnEmptyQueueHasNoEvent() throws {
        let core = try Core()

        XCTAssertNil(core.nextEvent())
    }

    func testConnectingNowhereAnswersFalse() throws {
        let core = try Core()

        XCTAssertFalse(core.connect(to: "ws://127.0.0.1:1/sfu"))
    }

    /// Se `unkvoid_string_free` sumir de um caminho, ou for chamado duas vezes, é aqui que
    /// o processo cai — o ciclo é curto de propósito para o teste continuar rápido.
    func testHandlesChurnWithoutLeakingOrDoubleFreeing() throws {
        for _ in 0 ..< 50 {
            let core = try Core()

            XCTAssertNil(core.nextEvent())
            XCTAssertThrowsError(try core.call("ping", ["a": 1]))
        }
    }
}
