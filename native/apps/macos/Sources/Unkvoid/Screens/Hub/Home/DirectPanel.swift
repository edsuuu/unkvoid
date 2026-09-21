import SwiftUI

/// `home/DirectPanel.tsx`: a conversa com uma pessoa — cabeçalho, mensagens e o campo.
struct DirectPanel: View {
    @EnvironmentObject private var model: AppModel

    @State private var draft = ""
    @FocusState private var writing: Bool

    var body: some View {
        if let person = model.directPerson {
            VStack(spacing: 12) {
                HStack(spacing: 10) {
                    Avatar(name: person.name, url: person.avatar_url, size: 26)

                    Text(person.name)
                        .font(Theme.sans(14, .semibold))
                        .foregroundStyle(Theme.ink)

                    Spacer(minLength: 0)

                    Button {
                        model.closeDirect()
                    } label: {
                        Icon(name: .close, size: 14).foregroundStyle(Theme.inkDim)
                    }
                    .buttonStyle(.pointer)
                    .help("Fechar a conversa")
                }

                Divider().overlay(Theme.line)

                list(person)

                HStack(alignment: .bottom, spacing: 8) {
                    TextField("Escreva para \(person.name)", text: $draft, axis: .vertical)
                        .lineLimit(1 ... 6)
                        .field(focused: writing)
                        .focused($writing)
                        .onSubmit(send)

                    Button("Enviar", action: send)
                        .buttonStyle(PrimaryButton())
                        .fixedSize()
                        .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
            .padding(16)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .glass()
        }
    }

    private func list(_ person: Person) -> some View {
        ScrollViewReader { scroller in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 10) {
                    if model.directLoading {
                        VStack(alignment: .leading, spacing: 10) {
                            ForEach(0 ..< 3, id: \.self) { _ in Skeleton(height: 48, width: 260) }
                        }
                    } else if model.directFailed {
                        VStack(spacing: 8) {
                            Text("Não deu para carregar esta conversa.")
                                .font(Theme.sans(13))
                                .foregroundStyle(Theme.danger)

                            Button("Tentar de novo") {
                                Task { await model.openDirect(with: person) }
                            }
                            .buttonStyle(GhostButton())
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 40)
                    } else if model.directMessages.isEmpty {
                        Text("Nenhuma mensagem ainda com \(person.name).")
                            .font(Theme.sans(13))
                            .foregroundStyle(Theme.inkDim)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 40)
                    }

                    ForEach(model.directMessages) { message in
                        DirectRow(message: message, mine: message.sender.id == model.user?.id)
                            .id(message.id)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(maxHeight: .infinity)
            .onChange(of: model.directMessages.count) {
                if let last = model.directMessages.last {
                    scroller.scrollTo(last.id, anchor: .bottom)
                }
            }
        }
    }

    private func send() {
        let body = draft

        guard !body.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return
        }

        draft = ""

        Task {
            if await !model.sendDirect(body) {
                draft = draft.isEmpty ? body : draft
            }
        }
    }
}

/// `home/DirectRow.tsx`: a mensagem, e editar ou apagar quando é sua.
private struct DirectRow: View {
    @EnvironmentObject private var model: AppModel

    var message: DirectMessage
    var mine: Bool

    @State private var hovering = false
    @State private var editing = false
    @State private var draft = ""
    @FocusState private var writing: Bool

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Avatar(name: message.sender.name, url: message.sender.avatar_url, size: 30, mine: mine)

            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(message.sender.name)
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
                        if await model.editDirect(message, to: draft) {
                            editing = false
                        }
                    }
                } else {
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
        .background(hovering ? Theme.row.opacity(0.5) : .clear, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
        .overlay(alignment: .topTrailing) {
            if hovering, !editing, mine {
                HStack(spacing: 4) {
                    Button {
                        draft = message.body
                        editing = true
                        writing = true
                    } label: {
                        Icon(name: .edit, size: 12)
                    }
                    .buttonStyle(IconButton(side: 26, radius: 8))
                    .help("Editar")

                    Button {
                        Task { await model.deleteDirect(message) }
                    } label: {
                        Icon(name: .trash, size: 12)
                    }
                    .buttonStyle(IconButton(side: 26, radius: 8))
                    .help("Apagar")
                }
                .padding(.trailing, 4)
                .offset(y: -6)
            }
        }
        .onHover { hovering = $0 }
    }
}

/// O campo de editar uma mensagem no lugar: Enter salva, Esc cancela.
struct InlineEditor: View {
    @Binding var draft: String
    var original: String
    var writing: FocusState<Bool>.Binding
    var cancel: () -> Void
    var save: () async -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            TextField("", text: $draft, axis: .vertical)
                .lineLimit(1 ... 8)
                .field(focused: writing.wrappedValue)
                .focused(writing)
                .onSubmit(submit)
                .onExitCommand(perform: cancel)

            HStack(spacing: 8) {
                Button("Salvar", action: submit)
                    .buttonStyle(PrimaryButton())
                    .font(Theme.sans(12, .semibold))
                    .disabled(unchanged)

                Button("Cancelar", action: cancel)
                    .buttonStyle(GhostButton(font: Theme.sans(12)))

                Text("Enter salva · Esc cancela")
                    .font(Theme.mono(10))
                    .foregroundStyle(Theme.inkDim)
            }
        }
    }

    private var unchanged: Bool {
        draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || draft == original
    }

    private func submit() {
        guard !unchanged else {
            cancel()

            return
        }

        Task { await save() }
    }
}

/// A hora de uma mensagem, do jeito do `toLocaleString('pt-BR', …)` do React: dia antes da hora.
/// Só a tela lê as datas, então os formatadores moram na main.
@MainActor
enum Clock {
    private static let reader: ISO8601DateFormatter = {
        let reader = ISO8601DateFormatter()

        reader.formatOptions = [.withInternetDateTime]

        return reader
    }()

    private static let fractional: ISO8601DateFormatter = {
        let reader = ISO8601DateFormatter()

        reader.formatOptions = [.withInternetDateTime, .withFractionalSeconds]

        return reader
    }()

    private static let writer: DateFormatter = {
        let writer = DateFormatter()

        writer.locale = Locale(identifier: "pt_BR")
        writer.dateFormat = "dd/MM, HH:mm"

        return writer
    }()

    private static let day: DateFormatter = {
        let writer = DateFormatter()

        writer.locale = Locale(identifier: "pt_BR")
        writer.dateFormat = "dd/MM/yyyy"

        return writer
    }()

    static func date(_ text: String) -> Date? {
        reader.date(from: text) ?? fractional.date(from: text)
    }

    static func short(_ text: String) -> String {
        date(text).map(writer.string) ?? ""
    }

    static func day(_ text: String) -> String {
        date(text).map(day.string) ?? ""
    }
}
