import AppKit
import Foundation

/// O chat de um canal: as mensagens, a que se está respondendo, as imagens esperando envio e
/// o que chegou sem ser lido. O app tem dois ao mesmo tempo — o do canal de texto aberto e o
/// do canal de voz em que se está —, e por isso isto é um objeto, e não campos do `AppModel`.
@MainActor
final class ChatRoom: ObservableObject {
    static let maxImages = 3

    /// `MANAGE_MESSAGES` e `SEND_MESSAGES`. Só escondem botão: quem decide é o Laravel.
    private static let manageMessages = 1 << 10
    private static let sendMessages = 1 << 9
    private static let administrator = 1

    @Published private(set) var channel: Channel?
    @Published private(set) var messages: [Message] = []
    @Published private(set) var loading = false
    @Published private(set) var loadingOlder = false
    @Published private(set) var failed = false
    @Published private(set) var sending = false
    @Published var replyTo: Message?
    @Published private(set) var images: [URL] = []
    /// A primeira mensagem que chegou com o painel fechado ou rolado para cima.
    @Published private(set) var newFrom: Int?
    @Published private(set) var unread = 0
    /// O painel está na tela: o que chega com ele fechado conta como não lido.
    var visible = false {
        didSet {
            if visible {
                unread = 0
            }
        }
    }

    private unowned let model: AppModel
    private var exhausted = false

    init(model: AppModel) {
        self.model = model
    }

    var canSend: Bool {
        let bits = channel?.permissions ?? 0

        return bits & Self.administrator != 0 || bits & Self.sendMessages != 0
    }

    func isMine(_ message: Message) -> Bool {
        message.user.id == model.user?.id
    }

    func canDelete(_ message: Message) -> Bool {
        let bits = channel?.permissions ?? 0

        return isMine(message) || bits & Self.administrator != 0 || bits & Self.manageMessages != 0
    }

    func open(_ opened: Channel) async {
        if let previous = channel, previous.id != opened.id, !previous.isVoice {
            await model.unfollow("channel.\(previous.id)")
        }

        channel = opened
        messages = []
        replyTo = nil
        images = []
        newFrom = nil
        unread = 0
        exhausted = false
        loading = true

        let page = await model.api("messages", ["channel": opened.id], quiet: true)

        guard channel?.id == opened.id else {
            return
        }

        loading = false
        failed = page == nil
        messages = model.decode(page) ?? []

        await model.follow("channel.\(opened.id)")
    }

    /// Canal de voz fica seguido enquanto o servidor estiver aberto — é por ele que chega quem
    /// entra e sai da voz —, então fechar o chat dele não o larga.
    func close() async {
        if let channel, !channel.isVoice {
            await model.unfollow("channel.\(channel.id)")
        }

        channel = nil
        messages = []
        replyTo = nil
        images = []
        unread = 0
    }

    /// A árvore do servidor voltou: o canal pode ter mudado de nome ou de permissão, ou sumido.
    func refresh(from tree: ServerTree?) async {
        guard let channel else {
            return
        }

        guard let fresh = tree?.channels.first(where: { $0.id == channel.id }) else {
            await close()

            return
        }

        self.channel = fresh
    }

    /// As mensagens mais antigas, ao rolar até o topo. Devolve `false` quando não há mais.
    func loadOlder() async -> Bool {
        guard let channel, let first = messages.first, !loadingOlder, !exhausted else {
            return false
        }

        loadingOlder = true

        let page = await model.api("messages", ["channel": channel.id, "query": "before=\(first.id)"], quiet: true)
        let older: [Message] = model.decode(page) ?? []

        loadingOlder = false

        guard self.channel?.id == channel.id else {
            return false
        }

        exhausted = older.isEmpty
        messages = older + messages

        return !older.isEmpty
    }

    /// Anexa imagens à próxima mensagem, já no tamanho que o servidor aceita.
    func attach(_ files: [URL]) {
        guard canSend else {
            return
        }

        let room = Self.maxImages - images.count

        if files.count > room {
            model.say("no máximo \(Self.maxImages) imagens por mensagem")
        }

        for file in files.prefix(max(0, room)) {
            guard let fitted = ImageShrinker.fit(file) else {
                model.say("essa imagem não serve: use PNG, JPEG, WebP ou GIF de até 2 MB")

                continue
            }

            images.append(fitted)
        }
    }

    func detach(_ file: URL) {
        images.removeAll { $0 == file }
    }

    func send(_ body: String) async -> Bool {
        let text = body.trimmingCharacters(in: .whitespacesAndNewlines)

        guard let channel, !sending, !text.isEmpty || !images.isEmpty else {
            return false
        }

        sending = true

        let sent: Any?

        if images.isEmpty {
            sent = await model.api("sendMessage", ["channel": channel.id], body: ["body": text, "reply_to_id": replyTo.map { $0.id as Any } ?? NSNull()])
        } else {
            var fields: [String: String] = text.isEmpty ? [:] : ["body": text]

            fields["reply_to_id"] = replyTo.map { "\($0.id)" }
            sent = await model.upload("sendMessage", ["channel": channel.id], field: "images[]", files: images, fields: fields)
        }

        sending = false

        guard let message: Message = model.decode(sent) else {
            return false
        }

        replyTo = nil
        images = []
        append(message)

        return true
    }

    func edit(_ message: Message, to body: String) async -> Bool {
        let answer = await model.ask("editMessage", ["id": message.id, "body": body])

        guard let edited: Message = model.decode(answer["message"]) else {
            model.warn(answer)

            return false
        }

        append(edited)

        return true
    }

    func delete(_ message: Message) async {
        let answer = await model.ask("deleteMessage", ["id": message.id])

        guard answer["ok"] as? Bool == true else {
            model.warn(answer)

            return
        }

        messages.removeAll { $0.id == message.id }
    }

    /// Um aviso do tempo real para o canal deste chat. Devolve `true` se era daqui.
    func heard(_ name: String, from source: String?, _ data: [String: Any]) -> Bool {
        guard let channel, source == "channel.\(channel.id)" else {
            return false
        }

        switch name {
        case "MessageSent":
            guard let message: Message = model.decode(data["message"]) else {
                return true
            }

            let known = messages.contains { $0.id == message.id }

            append(message)

            if !known, !isMine(message), !visible {
                unread += 1
                newFrom = newFrom ?? message.id
            }
        case "MessageUpdated":
            if let message: Message = model.decode(data["message"]) {
                append(message)
            }
        case "MessageDeleted":
            messages.removeAll { $0.id == data["id"] as? Int }
        default:
            return false
        }

        return true
    }

    func markRead() {
        unread = 0
        newFrom = nil
    }

    /// Mensagem nova entra no fim; a que já existe (editada, ou a resposta do envio que o
    /// tempo real também trouxe) troca de lugar com ela mesma.
    private func append(_ message: Message) {
        if let index = messages.firstIndex(where: { $0.id == message.id }) {
            messages[index] = message
        } else {
            messages.append(message)
        }
    }
}

/// `ImageShrinker.ts`: a imagem sai em até 2 MB e 2560 px de lado, que é o que o Laravel
/// aceita. O que já cabe sobe como está; o resto vira JPEG, descendo a escala e a qualidade
/// até caber. GIF não se reencoda — perderia a animação.
enum ImageShrinker {
    private static let maxBytes = 2 * 1024 * 1024
    private static let maxSide: CGFloat = 2560
    private static let accepted = ["png", "jpg", "jpeg", "webp", "gif"]
    private static let steps: [(scale: CGFloat, quality: CGFloat)] = [(1, 0.9), (1, 0.75), (0.75, 0.75), (0.5, 0.7), (0.35, 0.6)]

    static func fit(_ file: URL) -> URL? {
        let kind = file.pathExtension.lowercased()
        let size = (try? file.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? .max

        if accepted.contains(kind), size <= maxBytes {
            return file
        }

        guard kind != "gif", let image = NSImage(contentsOf: file), let source = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
            return nil
        }

        let base = min(1, maxSide / CGFloat(max(source.width, source.height)))

        for step in steps {
            let (width, height) = (Int(CGFloat(source.width) * base * step.scale), Int(CGFloat(source.height) * base * step.scale))

            guard
                let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
            else {
                continue
            }

            context.interpolationQuality = .high
            context.draw(source, in: CGRect(x: 0, y: 0, width: width, height: height))

            guard
                let scaled = context.makeImage(),
                let data = NSBitmapImageRep(cgImage: scaled).representation(using: .jpeg, properties: [.compressionFactor: step.quality]),
                data.count <= maxBytes
            else {
                continue
            }

            let fitted = FileManager.default.temporaryDirectory.appendingPathComponent("unkvoid-\(UUID().uuidString).jpg")

            return (try? data.write(to: fitted)) == nil ? nil : fitted
        }

        return nil
    }

    /// A imagem que está na área de transferência, gravada num arquivo para poder subir.
    static func pasted() -> URL? {
        let board = NSPasteboard.general

        if let files = board.readObjects(forClasses: [NSURL.self], options: [.urlReadingContentsConformToTypes: ["public.image"]]) as? [URL], let file = files.first {
            return file
        }

        guard let image = NSImage(pasteboard: board), let tiff = image.tiffRepresentation, let data = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]) else {
            return nil
        }

        let file = FileManager.default.temporaryDirectory.appendingPathComponent("unkvoid-\(UUID().uuidString).png")

        return (try? data.write(to: file)) == nil ? nil : file
    }
}
