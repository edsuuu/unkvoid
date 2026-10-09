import SwiftUI
import UniformTypeIdentifiers

/// O chat de um canal, em `surfaceChat`: o cabeçalho de 48, a lista rolada até o fim, a
/// resposta e as imagens esperando envio, e o campo de escrever. Serve ao canal de texto e, com
/// `onClose`, ao chat da voz ao lado da chamada.
struct ChatPanel: View {
    @EnvironmentObject private var model: AppModel
    @ObservedObject var chat: ChatRoom

    var onClose: (() -> Void)?

    @State private var draft = ""
    @State private var dropping = false
    @State private var lightbox: URL?
    @FocusState private var writing: Bool

    var body: some View {
        VStack(spacing: 0) {
            if let channel = chat.channel {
                header(channel)

                list(channel)

                VStack(spacing: 8) {
                    if let replyTo = chat.replyTo {
                        replying(to: replyTo)
                    }

                    if !chat.images.isEmpty {
                        attachments
                    }

                    composer(channel)
                }
                .padding(.horizontal, 16)
                .padding(.bottom, 24)
            } else {
                Text(onClose != nil
                    ? "Abrindo o chat da voz…"
                    : model.tree?.textChannels.isEmpty == false
                        ? "Escolha um canal de texto à esquerda."
                        : "Este servidor ainda não tem canal de texto.")
                    .font(Theme.message)
                    .foregroundStyle(Theme.inkDim)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.surfaceChat)
        .overlay(
            Rectangle()
                .strokeBorder(dropping ? Theme.brand : .clear, lineWidth: 2)
        )
        .onDrop(of: [.fileURL], isTargeted: $dropping, perform: dropped)
        .overlay {
            if let lightbox {
                Lightbox(url: lightbox) { self.lightbox = nil }
            }
        }
        .onAppear { chat.visible = true }
        .onDisappear { chat.visible = false }
        .onChange(of: chat.channel?.id) { draft = "" }
    }

    private func header(_ channel: Channel) -> some View {
        HStack(spacing: 8) {
            Icon(name: channel.isVoice ? .speaker : .hash, size: 24)
                .foregroundStyle(Theme.inkDim)

            Text(channel.name)
                .font(Theme.header)
                .foregroundStyle(Theme.inkStrong)
                .lineLimit(1)

            if let topic = channel.topic, !topic.isEmpty {
                Rectangle().fill(Theme.line).frame(width: 1, height: 24)

                Text(topic)
                    .font(Theme.sans(14))
                    .foregroundStyle(Theme.inkDim)
                    .lineLimit(1)
            }

            Spacer(minLength: 0)

            if model.abilities.allows("manageChannels") {
                Button {
                    model.channelEditor = ChannelEditor(channel: channel, kind: channel.type)
                } label: {
                    Icon(name: .gear, size: 24)
                }
                .buttonStyle(IconButton())
                .help("Editar canal")
            }

            if onClose == nil {
                Button {
                    model.membersOpen.toggle()
                } label: {
                    Icon(name: .users, size: 24)
                }
                .buttonStyle(IconButton(tone: model.membersOpen ? .idle : .idle))
                .help(model.membersOpen ? "Esconder a lista de membros" : "Mostrar a lista de membros")
            }

            if let onClose {
                Button(action: onClose) {
                    Icon(name: .close, size: 20)
                }
                .buttonStyle(IconButton())
                .help("Fechar o chat")
            }
        }
        .padding(.horizontal, 16)
        .frame(height: Theme.Size.header)
        .overlay(alignment: .bottom) {
            Rectangle().fill(Color.black.opacity(0.2)).frame(height: 1)
        }
    }

    private func list(_ channel: Channel) -> some View {
        ScrollViewReader { scroller in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if chat.messages.first != nil {
                        older
                    }

                    // Os esqueletos vão num `VStack` próprio, e não soltos na lista: com
                    // `ForEach(0 ..< 3)` a identidade deles é `Int`, a mesma de `Message.id`.
                    if chat.loading, chat.messages.isEmpty {
                        VStack(alignment: .leading, spacing: 16) {
                            ForEach(0 ..< 3, id: \.self) { _ in
                                HStack(alignment: .top, spacing: 16) {
                                    Circle().fill(Theme.hover).frame(width: 40, height: 40)

                                    VStack(alignment: .leading, spacing: 8) {
                                        Skeleton(height: 14, width: 140)
                                        Skeleton(height: 14, width: 260)
                                    }
                                }
                            }
                        }
                        .padding(16)
                    }

                    if !chat.loading, chat.messages.isEmpty {
                        if chat.failed {
                            VStack(spacing: 8) {
                                Text("Não deu para carregar as mensagens de \(label(channel)).")
                                    .font(Theme.message)
                                    .foregroundStyle(Theme.danger)

                                Button("Tentar de novo") {
                                    Task { await chat.open(channel) }
                                }
                                .buttonStyle(GhostButton())
                            }
                            .frame(maxWidth: .infinity, alignment: .center)
                            .padding(.vertical, 40)
                        } else {
                            welcome(channel)
                        }
                    }

                    ForEach(Array(chat.messages.enumerated()), id: \.element.id) { index, message in
                        MessageRow(
                            chat: chat,
                            message: message,
                            continued: index > 0 && Self.continues(message, after: chat.messages[index - 1]),
                            unreadMark: message.id == chat.newFrom,
                            color: color(of: message)
                        ) { lightbox = $0 }
                            .id(message.id)
                    }
                }
                .padding(.bottom, 8)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .scrollIndicators(.automatic)
            .frame(maxHeight: .infinity)
            .onChange(of: chat.messages.last?.id) { scrollToEnd(scroller) }
            .onChange(of: channel.id) { scrollToEnd(scroller) }
            .task(id: channel.id) { scrollToEnd(scroller) }
        }
    }

    /// A mesma pessoa, menos de sete minutos depois, sem responder a ninguém: a mensagem é
    /// continuação da anterior, e sai sem foto nem nome.
    static func continues(_ message: Message, after previous: Message) -> Bool {
        guard message.user.id == previous.user.id, message.reply_to == nil, message.type != "join", previous.type != "join" else {
            return false
        }

        guard let now = Clock.date(message.created_at), let before = Clock.date(previous.created_at) else {
            return false
        }

        return now.timeIntervalSince(before) < 7 * 60
    }

    /// A cor do cargo mais alto de quem escreveu, para o nome.
    private func color(of message: Message) -> Color? {
        guard let tree = model.tree, let member = tree.members.first(where: { $0.user_id == message.user.id }) else {
            return nil
        }

        return Theme.hex(tree.topRole(of: member)?.color)
    }

    /// O começo de um canal sem mensagem nenhuma.
    private func welcome(_ channel: Channel) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Icon(name: channel.isVoice ? .speaker : .hash, size: 40)
                .foregroundStyle(Theme.inkStrong)
                .frame(width: 68, height: 68)
                .background(Theme.hover, in: Circle())

            Text("Bem-vindo(a) a \(label(channel))!")
                .font(Theme.welcome)
                .foregroundStyle(Theme.inkStrong)

            Text("Este é o começo do canal \(label(channel)).")
                .font(Theme.message)
                .foregroundStyle(Theme.inkDim)

            if model.abilities.allows("createInvite"), model.tree?.invite_code != nil {
                Button {
                    model.modal = .invitePeople
                } label: {
                    HStack(spacing: 8) {
                        Icon(name: .userPlus, size: 18)

                        Text("Convidar pessoas")
                    }
                }
                .buttonStyle(GhostButton())
                .padding(.top, 8)
            }
        }
        .padding(16)
        .padding(.top, 24)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// Aparecer no topo da lista é ter rolado até lá: é a hora de buscar as mais antigas.
    private var older: some View {
        HStack(alignment: .top, spacing: 16) {
            Circle().fill(Theme.hover).frame(width: 40, height: 40)

            VStack(alignment: .leading, spacing: 8) {
                Skeleton(height: 14, width: 140)
                Skeleton(height: 14, width: 260)
            }
        }
        .padding(.horizontal, 16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .frame(height: chat.loadingOlder ? 56 : 14, alignment: .top)
        .opacity(chat.loadingOlder ? 1 : 0)
        .clipped()
        .onAppear {
            Task { _ = await chat.loadOlder() }
        }
    }

    private func replying(to message: Message) -> some View {
        HStack(spacing: 8) {
            Text("Respondendo a").foregroundStyle(Theme.inkDim)

            Text(message.user.name).fontWeight(.semibold).foregroundStyle(Theme.ink)

            Text(message.body.isEmpty ? "imagem" : message.body)
                .foregroundStyle(Theme.inkDim)
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)

            Button {
                chat.replyTo = nil
            } label: {
                Icon(name: .close, size: 16).foregroundStyle(Theme.inkDim)
            }
            .buttonStyle(.pointer)
            .help("Cancelar a resposta")
        }
        .font(Theme.sans(14))
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .background(Theme.surfacePanel, in: UnevenRoundedRectangle(topLeadingRadius: Theme.Size.radiusLarge, topTrailingRadius: Theme.Size.radiusLarge))
        .padding(.bottom, -8)
    }

    private var attachments: some View {
        HStack(spacing: 8) {
            ForEach(chat.images, id: \.self) { file in
                AsyncImage(url: file) { image in
                    image.resizable().scaledToFill()
                } placeholder: {
                    Theme.hover
                }
                .frame(width: 64, height: 64)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous))
                .overlay(alignment: .topTrailing) {
                    Button {
                        chat.detach(file)
                    } label: {
                        Icon(name: .close, size: 12)
                            .foregroundStyle(Theme.inkSoft)
                            .frame(width: 20, height: 20)
                            .background(Theme.surfaceFloat, in: Circle())
                    }
                    .buttonStyle(.pointer)
                    .offset(x: 6, y: -6)
                    .help("Tirar a imagem")
                }
            }

            Spacer(minLength: 0)
        }
    }

    @ViewBuilder
    private func composer(_ channel: Channel) -> some View {
        if chat.canSend {
            HStack(alignment: .center, spacing: 12) {
                Button {
                    chat.attach(pick())
                } label: {
                    Icon(name: .plus, size: 14)
                        .foregroundStyle(Theme.surfaceInput)
                        .frame(width: 24, height: 24)
                        .background(Theme.inkSoft, in: Circle())
                }
                .buttonStyle(.pointer)
                .disabled(chat.images.count >= ChatRoom.maxImages)
                .help("Anexar imagem (até \(ChatRoom.maxImages))")

                TextField("Conversar em \(channel.isVoice ? "🔊 " : "")\(label(channel))", text: $draft, axis: .vertical)
                    .textFieldStyle(.plain)
                    .font(Theme.message)
                    .foregroundStyle(Theme.ink)
                    .lineLimit(1 ... 8)
                    .focused($writing)
                    .onSubmit(send)
                    .onPasteCommand(of: [.image, .fileURL]) { _ in
                        if let pasted = ImageShrinker.pasted() {
                            chat.attach([pasted])
                        }
                    }

                if chat.sending {
                    ProgressView().controlSize(.small)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 11)
            .frame(minHeight: 44)
            .background(Theme.surfaceInput, in: RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous))
        } else {
            Text("Você não tem permissão para enviar mensagens em \(label(channel)).")
                .font(Theme.sans(14))
                .foregroundStyle(Theme.inkDim)
                .frame(maxWidth: .infinity)
                .frame(minHeight: 44)
                .background(Theme.surfaceInput.opacity(0.5), in: RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous))
        }
    }

    private func label(_ channel: Channel) -> String {
        channel.isVoice ? channel.name : "#\(channel.name)"
    }

    private func scrollToEnd(_ scroller: ScrollViewProxy) {
        guard let last = chat.messages.last else {
            return
        }

        scroller.scrollTo(last.id, anchor: .bottom)
        chat.markRead()
    }

    private func pick() -> [URL] {
        let panel = NSOpenPanel()

        panel.allowedContentTypes = [.png, .jpeg, .webP, .gif]
        panel.allowsMultipleSelection = true

        return panel.runModal() == .OK ? panel.urls : []
    }

    private func dropped(_ providers: [NSItemProvider]) -> Bool {
        for provider in providers {
            _ = provider.loadObject(ofClass: URL.self) { file, _ in
                guard let file else {
                    return
                }

                Task { @MainActor in chat.attach([file]) }
            }
        }

        return !providers.isEmpty
    }

    private func send() {
        let body = draft

        guard !body.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !chat.images.isEmpty else {
            return
        }

        draft = ""

        Task {
            // Mandar de volta o que não saiu: perder o que a pessoa escreveu por causa de
            // uma recusa do servidor é o pior jeito de falhar.
            if await !chat.send(body) {
                draft = draft.isEmpty ? body : draft
            }
        }
    }
}

/// Uma mensagem: foto de 40, nome na cor do cargo, hora, a mensagem a que responde, o corpo e
/// as imagens. A continuação da mesma pessoa sai só com o corpo, recuada. Com o mouse em cima
/// aparecem responder, editar (só o que é seu) e apagar (seu, ou de quem gerencia mensagens).
private struct MessageRow: View {
    @ObservedObject var chat: ChatRoom

    var message: Message
    var continued: Bool
    var unreadMark: Bool
    var color: Color?
    var open: (URL) -> Void

    @State private var hovering = false
    @State private var editing = false
    @State private var draft = ""
    @FocusState private var writing: Bool

    private static let gutter: CGFloat = 72

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if unreadMark {
                HStack(spacing: 8) {
                    Rectangle().fill(Theme.danger).frame(height: 1)

                    Text("NOVAS")
                        .font(Theme.sans(11, .bold))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 6)
                        .frame(height: 16)
                        .background(Theme.danger, in: UnevenRoundedRectangle(bottomLeadingRadius: Theme.Size.radius, bottomTrailingRadius: Theme.Size.radius))
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 4)
            }

            if message.type == "join" {
                arrival
            } else {
                written
            }
        }
    }

    private var arrival: some View {
        HStack(spacing: 8) {
            Icon(name: .arrowLeft, size: 16)
                .rotationEffect(.degrees(180))
                .foregroundStyle(Theme.online)
                .frame(width: 40)

            (Text(message.user.name).foregroundStyle(Theme.inkStrong) + Text(" chegou no servidor."))

            Text(Clock.short(message.created_at)).font(Theme.meta)
        }
        .font(Theme.message)
        .foregroundStyle(Theme.inkDim)
        .padding(.horizontal, 16)
        .padding(.vertical, 4)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var written: some View {
        let mine = chat.isMine(message)

        return HStack(alignment: .top, spacing: 16) {
            if continued {
                Text(Clock.hour(message.created_at))
                    .font(Theme.sans(11))
                    .foregroundStyle(Theme.inkDim)
                    .frame(width: 40)
                    .padding(.top, 4)
                    .opacity(hovering ? 1 : 0)
            } else {
                Avatar(name: message.user.name, url: message.user.avatar_url, size: 40, mine: mine, ring: Theme.surfaceChat)
            }

            VStack(alignment: .leading, spacing: 2) {
                if let reply = message.reply_to {
                    HStack(spacing: 4) {
                        Icon(name: .arrowLeft, size: 12).rotationEffect(.degrees(90))

                        Text(reply.name).foregroundStyle(Theme.inkStrong)

                        Text(reply.body.isEmpty ? "imagem" : reply.body).lineLimit(1)
                    }
                    .font(Theme.sans(14))
                    .foregroundStyle(Theme.inkDim)
                }

                if !continued {
                    HStack(alignment: .firstTextBaseline, spacing: 4) {
                        Text(message.user.name)
                            .font(Theme.sans(16, .medium))
                            .foregroundStyle(color ?? Theme.inkStrong)

                        Text(Clock.short(message.created_at))
                            .font(Theme.meta)
                            .foregroundStyle(Theme.inkDim)
                            .padding(.leading, 4)
                    }
                }

                if editing {
                    InlineEditor(draft: $draft, original: message.body, writing: $writing) {
                        editing = false
                    } save: {
                        if await chat.edit(message, to: draft) {
                            editing = false
                        }
                    }
                } else if !message.body.isEmpty {
                    (Text(message.body) + Text(message.edited_at == nil ? "" : " (editado)").font(Theme.sans(10)).foregroundStyle(Theme.inkDim))
                        .font(Theme.message)
                        .foregroundStyle(Theme.ink)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }

                if let files = message.files, !files.isEmpty {
                    pictures(files)
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 2)
        .padding(.top, continued ? 0 : 15)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(hovering ? Color(hex: 0x020202).opacity(0.06) : .clear)
        .overlay(alignment: .topTrailing) {
            if hovering, !editing {
                actions(mine: mine)
                    .padding(.trailing, 16)
                    .offset(y: continued ? -12 : 0)
            }
        }
        .onHover { hovering = $0 }
    }

    /// `MessageImages.tsx`: as imagens da mensagem, que abrem grandes ao clicar.
    private func pictures(_ files: [MessageFile]) -> some View {
        HStack(alignment: .top, spacing: 8) {
            ForEach(files) { file in
                if let url = URL(string: file.url) {
                    Button {
                        open(url)
                    } label: {
                        AsyncImage(url: url) { image in
                            image.resizable().scaledToFill()
                        } placeholder: {
                            Theme.hover
                        }
                        .frame(width: files.count == 1 ? 400 : 200, height: files.count == 1 ? 300 : 150)
                        .clipShape(RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous))
                    }
                    .buttonStyle(.pointer)
                }
            }
        }
        .padding(.top, 4)
    }

    /// A barra que flutua no canto da mensagem sob o mouse: responder, editar, apagar.
    private func actions(mine: Bool) -> some View {
        HStack(spacing: 0) {
            if chat.canSend {
                action(.arrowLeft, "Responder") { chat.replyTo = message }
            }

            if mine, !message.body.isEmpty {
                action(.edit, "Editar") {
                    draft = message.body
                    editing = true
                    writing = true
                }
            }

            if chat.canDelete(message) {
                action(.trash, "Excluir mensagem", danger: true) {
                    Task { await chat.delete(message) }
                }
            }
        }
        .background(Theme.surfaceChat, in: RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous).strokeBorder(Theme.line, lineWidth: 1))
        .shadow(color: .black.opacity(0.16), radius: 4, y: 2)
    }

    private func action(_ icon: IconName, _ hint: String, danger: Bool = false, _ work: @escaping () -> Void) -> some View {
        Button(action: work) {
            Icon(name: icon, size: 20)
                .rotationEffect(.degrees(icon == .arrowLeft ? 90 : 0))
                .foregroundStyle(danger ? Theme.danger : Theme.inkSoft)
        }
        .buttonStyle(IconButton())
        .help(hint)
    }
}

/// A imagem grande por cima de tudo; clicar fora ou Esc fecha.
struct Lightbox: View {
    let url: URL
    let close: () -> Void

    var body: some View {
        ZStack {
            Color.black.opacity(0.85)
                .ignoresSafeArea()
                .onTapGesture(perform: close)

            AsyncImage(url: url) { image in
                image.resizable().scaledToFit()
            } placeholder: {
                ProgressView()
            }
            .padding(40)
        }
        .onExitCommand(perform: close)
    }
}
