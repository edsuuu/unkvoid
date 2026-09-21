import SwiftUI

/// `home/ServersHome.tsx`: criar uma sala com conta, a sala por código com as últimas
/// acessadas, e a lista das salas em que se está.
struct ServersHome: View {
    @EnvironmentObject private var model: AppModel

    @State private var name = ""
    @State private var busy = false
    @FocusState private var naming: Bool

    var body: some View {
        ScrollView {
            // As mesmas três regras do React: `flex-[0_1_360px]`, `flex-[1_1_360px]` e `flex-1`.
            FlexWrap(spacing: 12) {
                create.flex(basis: 360, grow: 0, minimum: 260)

                roomByCode.flex(basis: 360, grow: 1, minimum: 260)

                servers.flex(basis: 0, grow: 1, minimum: 280)
            }
        }
        .scrollIndicators(.never)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var create: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Home").labelMono()

            Text("Oi, \(model.user?.name ?? "").")
                .font(Theme.sans(17, .semibold))
                .tracking(-0.3)
                .foregroundStyle(Theme.ink)

            Text("Uma sala nova já vem com um canal de texto e um de voz. Depois é só mandar o convite.")
                .font(Theme.sans(13))
                .foregroundStyle(Theme.inkSoft)
                .fixedSize(horizontal: false, vertical: true)

            TextField("Nome da sala", text: $name)
                .field(focused: naming)
                .focused($naming)
                .onSubmit(submit)
                .onChange(of: name) { name = String(name.prefix(60)) }

            Button(action: submit) {
                HStack(spacing: 8) {
                    if busy {
                        ProgressView().controlSize(.small)
                    }

                    Text("Criar sala")
                }
                .frame(maxWidth: .infinity)
            }
            .buttonStyle(PrimaryButton(font: Theme.sans(13.5, .semibold)))
            .disabled(busy)

            Button {
                model.modal = .invite
            } label: {
                Text("Tenho um convite").frame(maxWidth: .infinity)
            }
            .buttonStyle(GhostButton())
        }
        .padding(20)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .glass()
    }

    private var roomByCode: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Só compartilhar a tela").labelMono()

            Text("Uma sala por código, sem servidor: quem tiver o código assiste.")
                .font(Theme.sans(13))
                .foregroundStyle(Theme.inkSoft)
                .fixedSize(horizontal: false, vertical: true)

            Button("Criar ou entrar com código") {
                model.openEntry()
            }
            .buttonStyle(PrimaryButton(font: Theme.sans(13.5, .semibold)))

            if !model.recentRooms.isEmpty {
                Text("Últimas salas acessadas").labelMono()
                    .padding(.top, 4)

                FlowRow(spacing: 8) {
                    ForEach(model.recentRooms, id: \.self) { code in
                        Button(code) {
                            Task { await model.openRoom(code) }
                        }
                        .buttonStyle(GhostButton(
                            font: Theme.mono(12.5),
                            padding: EdgeInsets(top: 6, leading: 12, bottom: 6, trailing: 12)
                        ))
                    }
                }
            }
        }
        .padding(20)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .glass()
    }

    private var servers: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Últimas salas").labelMono()
                .padding(.bottom, 4)

            if model.serversLoading, model.servers.isEmpty {
                ForEach(0 ..< 3, id: \.self) { _ in Skeleton(height: 48) }
            }

            if !model.serversLoading, model.servers.isEmpty {
                VStack(spacing: 8) {
                    Text(model.serversFailed ? "Não deu para carregar as suas salas." : "Nenhuma ainda. Crie uma ao lado ou entre com um convite.")
                        .font(Theme.sans(13))
                        .foregroundStyle(model.serversFailed ? Theme.danger : Theme.inkDim)

                    if model.serversFailed {
                        Button("Tentar de novo") {
                            Task { await model.loadServers() }
                        }
                        .buttonStyle(GhostButton())
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 24)
            }

            ForEach(model.servers) { server in
                Button {
                    Task { await model.openServer(server.id) }
                } label: {
                    HStack(spacing: 10) {
                        Avatar(name: server.name, url: server.icon_url, size: 32, square: true)

                        VStack(alignment: .leading, spacing: 2) {
                            Text(server.name)
                                .font(Theme.sans(13.5, .semibold))
                                .foregroundStyle(Theme.ink)
                                .lineLimit(1)

                            Text(server.owner_id == model.user?.id ? "dono" : "membro").labelMono(size: 9.5)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)

                        if let accessed = server.last_accessed_at {
                            Text(Clock.day(accessed))
                                .font(Theme.mono(10.5))
                                .foregroundStyle(Theme.inkDim)
                        }
                    }
                    .rowItem()
                    .contentShape(Rectangle())
                }
                .buttonStyle(.pointer)
            }
        }
        .padding(20)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .glass()
    }

    private func submit() {
        let wanted = name.trimmingCharacters(in: .whitespacesAndNewlines)

        guard !wanted.isEmpty, !busy else {
            return
        }

        busy = true

        Task {
            if await model.createServer(named: wanted) {
                name = ""
            }

            busy = false
        }
    }
}

/// `modals/ServerModal.tsx`: entrar numa sala pelo código do convite.
struct InviteModal: View {
    @EnvironmentObject private var model: AppModel

    @State private var code = ""
    @State private var busy = false
    @FocusState private var typing: Bool

    var body: some View {
        ModalFrame(
            title: "Entrar com um convite",
            subtitle: "Cole o código que mandaram para você.",
            width: 420,
            onClose: { model.modal = nil }
        ) {
            TextField("Código do convite", text: $code)
                .field(focused: typing)
                .focused($typing)
                .onSubmit(enter)
                .onAppear { typing = true }
        } footer: {
            Spacer(minLength: 0)

            Button("Cancelar") {
                model.modal = nil
            }
            .buttonStyle(GhostButton())

            Button("Entrar", action: enter)
                .buttonStyle(PrimaryButton(wide: false, font: Theme.sans(13, .semibold)))
                .disabled(busy || invite.isEmpty)
        }
    }

    /// Quem cola o link inteiro também entra: o código é o último pedaço.
    private var invite: String {
        code.trimmingCharacters(in: .whitespacesAndNewlines).split(separator: "/").last.map(String.init) ?? ""
    }

    private func enter() {
        guard !invite.isEmpty, !busy else {
            return
        }

        busy = true

        Task {
            if await model.acceptInvite(invite) {
                model.modal = nil
            }

            busy = false
        }
    }
}
