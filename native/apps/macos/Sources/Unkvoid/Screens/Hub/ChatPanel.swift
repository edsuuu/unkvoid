import SwiftUI
import UniformTypeIdentifiers

/// `ui/components/hub/ChatPanel.tsx`: cabeçalho, lista rolada até o fim, a resposta e as
/// imagens esperando envio, e o campo de escrever. Serve ao canal de texto e, com `onClose`,
/// ao chat da voz ao lado do palco.
struct ChatPanel: View {
    @EnvironmentObject private var model: AppModel
    @ObservedObject var chat: ChatRoom

    var onClose: (() -> Void)?

    @State private var draft = ""
    @State private var dropping = false
    @State private var lightbox: URL?
    @FocusState private var writing: Bool

    var body: some View {
        VStack(spacing: 12) {
            if let channel = chat.channel {
                header(channel)

                list(channel)

                if let replyTo = chat.replyTo {
                    replying(to: replyTo)
                }

                if !chat.images.isEmpty {
                    attachments
                }

                composer(channel)
            } else {
                Text(model.tree?.textChannels.isEmpty == false
                    ? "Escolha um canal de texto à esquerda."
                    : "Este servidor ainda não tem canal de texto.")
                    .font(Theme.sans(13))
                    .foregroundStyle(Theme.inkDim)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .glass()
        .overlay(
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .strokeBorder(dropping ? Theme.brand.opacity(0.6) : .clear, lineWidth: 1)
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
        VStack(spacing: 12) {
            HStack(spacing: 8) {
                if channel.isVoice {
                    Icon(name: .speaker, size: 14).foregroundStyle(Theme.online)
                } else {
                    Text("#")
                        .font(Theme.mono(14))
                        .foregroundStyle(Theme.lilac2)
                }

                Text(channel.name)
                    .font(Theme.sans(14, .semibold))
                    .foregroundStyle(Theme.ink)

                if let topic = channel.topic, !topic.isEmpty {
                    Rectangle().fill(Theme.line).frame(width: 1, height: 14)

                    Text(topic)
                        .font(Theme.sans(12.5))
                        .foregroundStyle(Theme.inkSoft)
                        .lineLimit(1)
                }

                Spacer(minLength: 0)

                if onClose == nil {
                    Button {
                        model.membersOpen.toggle()
                    } label: {
                        Icon(name: .users, size: 14)
                    }
                    .buttonStyle(IconButton(side: 28, radius: 9))
                    .help("Mostrar ou esconder os membros")
                }

                if model.abilities.allows("manageChannels") {
                    Button {
                        model.channelEditor = ChannelEditor(channel: channel, kind: channel.type)
                    } label: {
                        Icon(name: .edit, size: 13)
                    }
                    .buttonStyle(IconButton(side: 28, radius: 9))
                    .help("Editar canal")
                }

                if let onClose {
                    Button(action: onClose) {
                        Icon(name: .close, size: 14).foregroundStyle(Theme.inkDim)
                    }
                    .buttonStyle(.pointer)
                    .help("Fechar o chat")
                }
            }

            Divider().overlay(Theme.line)
        }
    }

    private func list(_ channel: Channel) -> some View {
        ScrollViewReader { scroller in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 10) {
                    if chat.messages.first != nil {
                        older
                    }

                    // Os esqueletos vão num `VStack` próprio, e não soltos na lista: com
                    // `ForEach(0 ..< 3)` a identidade deles é `Int`, a mesma de `Message.id`.
                    if chat.loading, chat.messages.isEmpty {
                        VStack(alignment: .leading, spacing: 10) {
                            ForEach(0 ..< 3, id: \.self) { _ in
                                HStack(spacing: 10) {
                                    Circle().fill(Color.white.opacity(0.07)).frame(width: 28, height: 28)
                                    Skeleton(height: 48, width: 220)
                                }
                            }
                        }
                    }

                    if !chat.loading, chat.messages.isEmpty {
                        VStack(spacing: 8) {
                            Text(chat.failed ? "Não deu para carregar as mensagens de \(label(channel))." : "Nenhuma mensagem ainda em \(label(channel)).")
                                .font(Theme.sans(13))
                                .foregroundStyle(chat.failed ? Theme.danger : Theme.inkDim)

                            if chat.failed {
                                Button("Tentar de novo") {
                                    Task { await chat.open(channel) }
                                }
                                .buttonStyle(GhostButton())
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .center)
                        .padding(.vertical, 40)
                    }

                    ForEach(chat.messages) { message in
                        MessageRow(chat: chat, message: message, unreadMark: message.id == chat.newFrom) { lightbox = $0 }
                            .id(message.id)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .scrollIndicators(.automatic)
            .frame(maxHeight: .infinity)
            .onChange(of: chat.messages.last?.id) { scrollToEnd(scroller) }
            .onChange(of: channel.id) { scrollToEnd(scroller) }
            .task(id: channel.id) { scrollToEnd(scroller) }
        }
    }

    /// Aparecer no topo da lista é ter rolado até lá: é a hora de buscar as mais antigas.
    private var older: some View {
        HStack(spacing: 10) {
            Circle().fill(Color.white.opacity(0.07)).frame(width: 28, height: 28)
            Skeleton(height: 40, width: 220)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .frame(height: chat.loadingOlder ? 48 : 14, alignment: .top)
        .opacity(chat.loadingOlder ? 1 : 0)
        .clipped()
        .onAppear {
                Task { _ = await chat.loadOlder() }
            }
    }

    private func replying(to message: Message) -> some View {
        HStack(spacing: 8) {
            Icon(name: .arrowLeft, size: 12)
                .rotationEffect(.degrees(90))
                .foregroundStyle(Theme.inkDim)

            Text("Respondendo").foregroundStyle(Theme.inkDim)

            Text(message.user.name).fontWeight(.medium).foregroundStyle(Theme.ink)

            Text(message.body.isEmpty ? "imagem" : message.body)
                .foregroundStyle(Theme.inkDim)
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)

            Button {
                chat.replyTo = nil
            } label: {
                Icon(name: .close, size: 13).foregroundStyle(Theme.inkDim)
            }
            .buttonStyle(.pointer)
            .help("Cancelar a resposta")
        }
        .font(Theme.sans(12))
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(Theme.row, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Theme.lineStrong, lineWidth: 1))
    }

    private var attachments: some View {
        HStack(spacing: 8) {
            ForEach(chat.images, id: \.self) { file in
                AsyncImage(url: file) { image in
                    image.resizable().scaledToFill()
                } placeholder: {
                    Theme.row
                }
                .frame(width: 64, height: 64)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Theme.lineStrong, lineWidth: 1))
                .overlay(alignment: .topTrailing) {
                    Button {
                        chat.detach(file)
                    } label: {
                        Icon(name: .close, size: 11)
                            .foregroundStyle(Theme.inkIcon)
                            .frame(width: 20, height: 20)
                            .background(Theme.popoverFill, in: Circle())
                            .overlay(Circle().strokeBorder(Theme.lineStrong, lineWidth: 1))
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
            HStack(alignment: .bottom, spacing: 8) {
                Button {
                    chat.attach(pick())
                } label: {
                    Icon(name: .plus, size: 15)
                }
                .buttonStyle(IconButton(side: 40, radius: 12))
                .disabled(chat.images.count >= ChatRoom.maxImages)
                .help("Anexar imagem (até \(ChatRoom.maxImages))")

                TextField("Escreva em \(label(channel))", text: $draft, axis: .vertical)
                    .lineLimit(1 ... 6)
                    .field(focused: writing)
                    .focused($writing)
                    .onSubmit(send)
                    .onPasteCommand(of: [.image, .fileURL]) { _ in
                        if let pasted = ImageShrinker.pasted() {
                            chat.attach([pasted])
                        }
                    }

                Button(chat.sending ? "Enviando…" : "Enviar", action: send)
                    .buttonStyle(PrimaryButton(wide: false))
                    .disabled(chat.sending || (draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && chat.images.isEmpty))
            }
        } else {
            Text("Você não pode escrever em \(label(channel)).")
                .font(Theme.sans(12.5))
                .foregroundStyle(Theme.inkDim)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 10)
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

/// `MessageRow.tsx`: foto, nome, hora, a mensagem a que responde, o corpo e as imagens. Com
/// o mouse em cima aparecem responder, editar (só o que é seu) e apagar (seu, ou de quem
/// gerencia mensagens no canal).
private struct MessageRow: View {
    @ObservedObject var chat: ChatRoom

    var message: Message
    var unreadMark: Bool
    var open: (URL) -> Void

    @State private var hovering = false
    @State private var menuOpen = false
    @State private var editing = false
    @State private var draft = ""
    @FocusState private var writing: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if unreadMark {
                HStack(spacing: 8) {
                    Rectangle().fill(Theme.danger.opacity(0.6)).frame(height: 1)

                    Text("NOVAS MENSAGENS")
                        .font(Theme.sans(9.5, .semibold))
                        .foregroundStyle(Theme.inkStrong)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 2)
                        .background(Theme.danger, in: Capsule())
                }
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
            Icon(name: .users, size: 13)

            (Text("@\(message.user.name)").foregroundStyle(Theme.inkIcon) + Text(" chegou no servidor"))

            Text(Clock.short(message.created_at)).font(Theme.mono(9.5))
        }
        .font(Theme.sans(12))
        .foregroundStyle(Theme.inkDim)
        .frame(maxWidth: .infinity)
        .padding(.vertical, 2)
    }

    private var written: some View {
        let mine = chat.isMine(message)

        return HStack(alignment: .top, spacing: 10) {
            Avatar(name: message.user.name, url: message.user.avatar_url, size: 30, mine: mine)

            VStack(alignment: .leading, spacing: 2) {
                if let reply = message.reply_to {
                    HStack(spacing: 6) {
                        Icon(name: .arrowLeft, size: 11).rotationEffect(.degrees(90))

                        Text(reply.name).foregroundStyle(Theme.inkIcon)

                        Text(reply.body.isEmpty ? "imagem" : reply.body).lineLimit(1)
                    }
                    .font(Theme.sans(11))
                    .foregroundStyle(Theme.inkDim)
                }

                HStack(spacing: 6) {
                    Text(message.user.name)
                        .font(Theme.sans(12.5, .semibold))
                        .foregroundStyle(Theme.inkBody)

                    Text(Clock.short(message.created_at))
                        .font(Theme.sans(11))
                        .foregroundStyle(Theme.inkDim)

                    if message.edited_at != nil {
                        Text("editado")
                            .font(Theme.sans(10))
                            .foregroundStyle(Theme.inkIcon)
                            .padding(.vertical, 2)
                            .padding(.horizontal, 6)
                            .background(Theme.row, in: Capsule())
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
                    Text(message.body)
                        .font(Theme.sans(13))
                        .foregroundStyle(Theme.inkBody)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }

                if let files = message.files, !files.isEmpty {
                    pictures(files)
                }
            }

            // Os três pontinhos têm lugar próprio na linha: o texto quebra antes deles, e nada
            // fica por cima do que a pessoa escreveu.
            more(mine: mine)
                .opacity((hovering || menuOpen) && !editing ? 1 : 0)
        }
        .padding(.vertical, 2)
        .padding(.horizontal, 4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(hovering || menuOpen ? Color.white.opacity(0.03) : .clear, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .onHover { hovering = $0 }
    }

    /// `MessageImages.tsx`: as imagens da mensagem, que abrem grandes ao clicar.
    private func pictures(_ files: [MessageFile]) -> some View {
        HStack(alignment: .top, spacing: 6) {
            ForEach(files) { file in
                if let url = URL(string: file.url) {
                    Button {
                        open(url)
                    } label: {
                        AsyncImage(url: url) { image in
                            image.resizable().scaledToFill()
                        } placeholder: {
                            Theme.row
                        }
                        .frame(width: files.count == 1 ? 320 : 160, height: files.count == 1 ? 200 : 120)
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Theme.lineStrong, lineWidth: 1))
                    }
                    .buttonStyle(.pointer)
                }
            }
        }
        .padding(.top, 4)
    }

    private func more(mine: Bool) -> some View {
        Button {
            menuOpen.toggle()
        } label: {
            Icon(name: .dots, size: 14)
        }
        .buttonStyle(IconButton(side: 26, radius: 8))
        .help("Opções da mensagem")
        .popover(isPresented: $menuOpen, arrowEdge: .bottom) {
            PopoverBox(width: 180) {
                if chat.canSend {
                    option(.arrowLeft, "Responder") { chat.replyTo = message }
                }

                if mine, !message.body.isEmpty {
                    option(.edit, "Editar") {
                        draft = message.body
                        editing = true
                        writing = true
                    }
                }

                if chat.canDelete(message) {
                    option(.trash, "Apagar", tint: Theme.danger) {
                        Task { await chat.delete(message) }
                    }
                }
            }
        }
    }

    private func option(_ icon: IconName, _ label: String, tint: Color = Theme.inkIcon, _ work: @escaping () -> Void) -> some View {
        MenuRow(icon: icon, label: label, tint: tint) {
            menuOpen = false
            work()
        }
    }
}

/// `common/Lightbox.tsx`: a imagem grande por cima de tudo; clicar fora ou Esc fecha.
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
