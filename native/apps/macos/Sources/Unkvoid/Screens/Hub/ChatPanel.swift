import SwiftUI

/// `ui/components/hub/ChatPanel.tsx`: cabeçalho, lista rolada até o fim e o campo de
/// escrever. `glass p-4` com 12 entre as três partes.
struct ChatPanel: View {
    @EnvironmentObject private var model: AppModel
    @State private var draft = ""
    @FocusState private var writing: Bool

    var body: some View {
        VStack(spacing: 12) {
            if let channel = model.channel {
                header(channel)

                list(channel)

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

                Button {
                    model.membersOpen.toggle()
                } label: {
                    Icon(name: .users, size: 14)
                }
                .buttonStyle(IconButton(side: 28, radius: 9))
                .help("Mostrar ou esconder os membros")
            }

            Divider().overlay(Theme.line)
        }
    }

    private func list(_ channel: Channel) -> some View {
        ScrollViewReader { scroller in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 10) {
                    // Os esqueletos vão num `VStack` próprio, e não soltos na lista: com
                    // `ForEach(0 ..< 3)` a identidade deles é `Int`, a mesma de
                    // `Message.id`, e o SwiftUI reaproveitava o esqueleto 1 e 2 para as
                    // mensagens 1 e 2 — que ficavam dois retângulos cinza para sempre.
                    if model.messagesLoading, model.messages.isEmpty {
                        VStack(alignment: .leading, spacing: 10) {
                            ForEach(0 ..< 3, id: \.self) { _ in
                                HStack(spacing: 10) {
                                    Circle().fill(Color.white.opacity(0.07)).frame(width: 28, height: 28)
                                    Skeleton(height: 48, width: 220)
                                }
                            }
                        }
                    }

                    if !model.messagesLoading, model.messages.isEmpty {
                        Text(model.messagesFailed
                            ? "Não deu para carregar as mensagens."
                            : "Nenhuma mensagem ainda em \(label(channel)).")
                            .font(Theme.sans(13))
                            .foregroundStyle(model.messagesFailed ? Theme.danger : Theme.inkDim)
                            .frame(maxWidth: .infinity, alignment: .center)
                            .padding(.vertical, 40)
                    }

                    ForEach(model.messages) { message in
                        MessageRow(message: message, mine: message.user.id == model.user?.id)
                            .id(message.id)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .scrollIndicators(.automatic)
            .frame(maxHeight: .infinity)
            .onChange(of: model.messages.count) { scrollToEnd(scroller) }
            .onChange(of: channel.id) { scrollToEnd(scroller) }
            .task(id: channel.id) { scrollToEnd(scroller) }
        }
    }

    private func composer(_ channel: Channel) -> some View {
        HStack(alignment: .bottom, spacing: 8) {
            TextField("Escreva em \(label(channel))", text: $draft, axis: .vertical)
                .lineLimit(1 ... 6)
                .field(focused: writing)
                .focused($writing)
                .onSubmit(send)

            Button(model.sending ? "Enviando…" : "Enviar", action: send)
                .buttonStyle(PrimaryButton())
                .fixedSize()
                .disabled(model.sending || draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
    }

    private func label(_ channel: Channel) -> String {
        channel.isVoice ? channel.name : "#\(channel.name)"
    }

    private func scrollToEnd(_ scroller: ScrollViewProxy) {
        guard let last = model.messages.last else {
            return
        }

        scroller.scrollTo(last.id, anchor: .bottom)
    }

    private func send() {
        let body = draft

        guard !body.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return
        }

        draft = ""

        Task {
            // Mandar de volta o que não saiu: perder o que a pessoa escreveu por causa de
            // uma recusa do servidor é o pior jeito de falhar.
            if await !model.sendMessage(body) {
                draft = draft.isEmpty ? body : draft
            }
        }
    }
}

/// `MessageRow.tsx`: foto, nome, hora e o corpo. O corpo é a única coisa que se seleciona.
private struct MessageRow: View {
    var message: Message
    var mine: Bool

    private static let reader: ISO8601DateFormatter = {
        let reader = ISO8601DateFormatter()

        reader.formatOptions = [.withInternetDateTime]

        return reader
    }()

    private static let writer: DateFormatter = {
        let writer = DateFormatter()

        writer.locale = Locale(identifier: "pt_BR")
        // A mesma ordem do `toLocaleString('pt-BR', …)` do React: dia antes da hora.
        writer.dateFormat = "dd/MM, HH:mm"

        return writer
    }()

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Avatar(name: message.user.name, url: message.user.avatar_url, size: 30, mine: mine)

            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(message.user.name)
                        .font(Theme.sans(12.5, .semibold))
                        .foregroundStyle(Theme.inkBody)

                    Text(when)
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

                if !message.body.isEmpty {
                    Text(message.body)
                        .font(Theme.sans(13))
                        .foregroundStyle(Theme.inkBody)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
        .padding(.vertical, 1)
        .padding(.horizontal, 4)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var when: String {
        guard let date = Self.reader.date(from: message.created_at) else {
            return ""
        }

        return Self.writer.string(from: date)
    }
}
