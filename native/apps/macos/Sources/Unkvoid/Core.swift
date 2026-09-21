import Foundation
import UnkvoidCore

/// O núcleo em Rust, do jeito que o Swift prefere ver: sem ponteiro solto e sem
/// preocupação com quem libera o quê.
///
/// Nada de regra aqui dentro. Quem decide o que é uma sala, quem pode falar e quando
/// reconectar é o `shared/core`; esta camada traduz tipos e nada mais.
final class Core: @unchecked Sendable {
    private let handle: OpaquePointer?

    /// `unkvoid_call` pega o `Handle` por `&mut` e `unkvoid_next_event` consome um
    /// `Receiver`: dois threads ali dentro ao mesmo tempo é corrida de dados no Rust. A
    /// fila de eventos é lida num laço próprio enquanto uma ação está em voo, então o
    /// encontro acontece de verdade — o cadeado é o que o torna seguro.
    private let gate = NSLock()

    enum Failure: Error {
        case coreUnavailable
        case notConnected
        case server(String)
    }

    init() throws {
        guard let created = unkvoid_core_new() else {
            throw Failure.coreUnavailable
        }

        handle = created
    }

    deinit {
        unkvoid_core_free(handle)
    }

    func connect(to url: String) -> Bool {
        gate.lock()

        defer { gate.unlock() }

        return unkvoid_connect(handle, url)
    }

    @discardableResult
    func call(_ action: String, _ data: [String: Any] = [:]) throws -> [String: Any] {
        let payload = try JSONSerialization.data(withJSONObject: data)

        gate.lock()

        let answer = unkvoid_call(handle, action, String(decoding: payload, as: UTF8.self))

        gate.unlock()

        guard let answer else {
            throw Failure.notConnected
        }

        // O Rust alocou; devolver é obrigação nossa, e um `defer` sobrevive a qualquer
        // caminho de saída daqui, inclusive um `throw` no meio.
        defer { unkvoid_string_free(answer) }

        let decoded = try JSONSerialization.jsonObject(with: Data(String(cString: answer).utf8))

        guard let object = decoded as? [String: Any] else {
            return [:]
        }

        if let message = object["error"] as? String {
            throw Failure.server(message)
        }

        return object
    }

    /// O que o servidor mandou sem ninguém pedir. Devolve `nil` quando a fila está vazia.
    /// As decisões do app: tela, sala, conta, servidores, mensagens. Bloqueia quando a
    /// ação precisa do servidor — nunca chame da thread que desenha.
    @discardableResult
    func app(_ action: String, _ data: [String: Any] = [:]) throws -> [String: Any] {
        let payload = try JSONSerialization.data(withJSONObject: data)

        gate.lock()
        defer { gate.unlock() }

        guard let answer = unkvoid_app(handle, action, String(decoding: payload, as: UTF8.self)) else {
            throw Failure.notConnected
        }

        defer { unkvoid_string_free(answer) }

        let decoded = try JSONSerialization.jsonObject(with: Data(String(cString: answer).utf8))

        return decoded as? [String: Any] ?? [:]
    }

    func nextEvent() -> [String: Any]? {
        gate.lock()

        let raw = unkvoid_next_event(handle)

        gate.unlock()

        guard let raw else {
            return nil
        }

        defer { unkvoid_string_free(raw) }

        let decoded = try? JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8))

        return decoded as? [String: Any]
    }
}

extension Core.Failure: LocalizedError {
    var errorDescription: String? {
        switch self {
        case .coreUnavailable: "o núcleo não subiu"
        case .notConnected: "o núcleo não está conectado"
        case let .server(message): message
        }
    }
}
