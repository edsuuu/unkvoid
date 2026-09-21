import Foundation
import IOSurface
import UnkvoidCore

/// O núcleo em Rust, do jeito que o Swift prefere ver: sem ponteiro solto e sem
/// preocupação com quem libera o quê.
///
/// Nada de regra aqui dentro. Quem decide o que é uma sala, quem pode falar e quando
/// reconectar é o `shared/core`; esta camada traduz tipos e nada mais.
final class Core: @unchecked Sendable {
    private let handle: OpaquePointer?

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

    /// Do lado do Rust tudo o que muda vive atrás de cadeado (`ffi.rs`), e é por isso que
    /// aqui não há nenhum: a fila de eventos e a de mídia são lidas enquanto uma ação está
    /// em voo, e um cadeado deste lado pararia a imagem a cada clique.
    func connect(to url: String) -> Bool {
        unkvoid_connect(handle, url)
    }

    @discardableResult
    func call(_ action: String, _ data: [String: Any] = [:]) throws -> [String: Any] {
        let payload = try JSONSerialization.data(withJSONObject: data)

        guard let answer = unkvoid_call(handle, action, String(decoding: payload, as: UTF8.self)) else {
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

        guard let answer = unkvoid_app(handle, action, String(decoding: payload, as: UTF8.self)) else {
            throw Failure.notConnected
        }

        defer { unkvoid_string_free(answer) }

        let decoded = try JSONSerialization.jsonObject(with: Data(String(cString: answer).utf8))

        return decoded as? [String: Any] ?? [:]
    }

    func nextEvent() -> [String: Any]? {
        guard let raw = unkvoid_next_event(handle) else {
            return nil
        }

        defer { unkvoid_string_free(raw) }

        let decoded = try? JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8))

        return decoded as? [String: Any]
    }
}

/// Um quadro de vídeo ou um bloco de som de uma transmissão que se está assistindo.
struct IncomingMedia {
    enum Kind {
        /// H.264 em Annex-B.
        case video(keyframe: Bool, timestamp: UInt32)
        /// PCM `Float` estéreo intercalado a 48 kHz.
        case audio
    }

    let producer: String
    let kind: Kind
    let data: Data
}

extension Core {
    /// Espera até 100 ms pelo próximo. Só a thread da mídia chama isto.
    func nextMedia() -> IncomingMedia? {
        var length = 0

        guard let block = unkvoid_next_media(handle, &length) else {
            return nil
        }

        defer { unkvoid_bytes_free(block, length) }

        let bytes = UnsafeBufferPointer(start: block, count: length)

        guard length >= 8 else {
            return nil
        }

        let idLength = Int(bytes[2]) | Int(bytes[3]) << 8
        let timestamp = UInt32(bytes[4]) | UInt32(bytes[5]) << 8 | UInt32(bytes[6]) << 16 | UInt32(bytes[7]) << 24

        guard length >= 8 + idLength else {
            return nil
        }

        return IncomingMedia(
            producer: String(decoding: bytes[8 ..< 8 + idLength], as: UTF8.self),
            kind: bytes[0] == 0 ? .video(keyframe: bytes[1] == 1, timestamp: timestamp) : .audio,
            data: Data(bytes[(8 + idLength)...])
        )
    }

    /// O núcleo fica com a posse do `IOSurface`: daí o `passRetained`.
    func show(_ surface: IOSurfaceRef, at nanoseconds: UInt64) {
        unkvoid_show(handle, Unmanaged.passRetained(surface).toOpaque(), nanoseconds)
    }

    func speak(_ samples: UnsafeBufferPointer<Float>) {
        unkvoid_speak(handle, samples.baseAddress, samples.count)
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
