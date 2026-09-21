import SwiftUI

/// A sala por código, igual a `ui/components/room/RoomScreen.tsx`: a barra de ferramentas
/// em cima, o aviso de erro quando há um, e o palco embaixo.
struct RoomScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 12) {
            if model.fullscreenTile == nil {
                RoomToolbar()
            }

            if let roomError = model.roomError {
                HStack(spacing: 12) {
                    Text(roomError)
                        .font(Theme.sans(13))
                        .frame(maxWidth: .infinity, alignment: .leading)

                    Button {
                        model.dismissRoomError()
                    } label: {
                        Icon(name: .close, size: 14)
                    }
                    .buttonStyle(.pointer)
                }
                .foregroundStyle(Theme.danger)
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .background(Theme.danger.opacity(0.1), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .strokeBorder(Theme.danger.opacity(0.35), lineWidth: 1)
                )
            }

            Stage()
        }
        .padding(12)
        .overlay {
            if model.shareOpen {
                ShareModal()
            }
        }
    }
}

/// `RoomToolbar.tsx` no modo `code`: o código para copiar, o tempo de sala e o botão de sair.
private struct RoomToolbar: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 8) {
            if model.signedIn {
                Button {
                    Task { await model.leaveRoom() }
                } label: {
                    Icon(name: .home, size: 14)
                }
                .buttonStyle(IconButton(side: 30, radius: 9))
                .help("Sair da sala e ir para a Home")
            }

            Text("Sala").labelMono()

            Button {
                model.copy(model.room ?? "", "Código copiado")
            } label: {
                HStack(spacing: 6) {
                    Text(model.room ?? "")

                    Icon(name: .copy, size: 12)
                }
                .codeChip(size: 12)
            }
            .buttonStyle(.pointer)
            .help("Copiar o código para mandar a alguém")

            PeopleMenu()

            if let since = model.enteredRoomAt {
                HStack(spacing: 8) {
                    LivePip()

                    Text(since, style: .timer)
                        .monospacedDigit()
                }
                .font(Theme.mono(11))
                .foregroundStyle(Theme.inkIcon)
                .padding(.vertical, 6)
                .padding(.horizontal, 12)
                .background(Theme.row, in: Capsule())
                .overlay(Capsule().strokeBorder(Theme.lineStrong, lineWidth: 1))
                .help("Tempo na sala")
            }

            // `-- ms` até haver medida, como o React faz: o espaço é reservado desde o
            // começo para a barra não saltar quando o primeiro número chega.
            Text(model.ping.map { "\($0) ms" } ?? "-- ms")
                .font(Theme.mono(10.5))
                .monospacedDigit()
                .foregroundStyle(Theme.inkDim)
                .help("Ida e volta até o servidor de mídia")

            Spacer(minLength: 0)

            HStack(spacing: 7) {
                ShareButton()

                Button {
                    Task { await model.leaveRoom() }
                } label: {
                    Icon(name: .phoneOff, size: 17)
                        .foregroundStyle(Theme.inkStrong)
                }
                .buttonStyle(RedButton())
                .help("Sair da sala")
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .glass(radius: 16)
    }
}

/// `ShareButton.tsx`: abre o seletor; com a transmissão no ar vira o menu dela, e o
/// "Parar" vermelho aparece ao lado.
private struct ShareButton: View {
    @EnvironmentObject private var model: AppModel

    @State private var open = false

    var body: some View {
        Button {
            if model.mine.sharing {
                open.toggle()
            } else {
                Task { await model.openShare() }
            }
        } label: {
            if model.shareStarting {
                ProgressView().controlSize(.small)
            } else {
                Icon(name: .screen, size: 16)
            }
        }
        .buttonStyle(IconButton(tone: model.mine.sharing ? .on : .idle))
        .disabled(model.shareStarting || !model.mine.canShare)
        .help(model.mine.sharing ? "Opções da transmissão" : "Compartilhar tela")
        .popover(isPresented: $open, arrowEdge: .bottom) {
            PopoverBox(width: 240) {
                Text("Você está transmitindo")
                    .labelMono()
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)

                HStack(spacing: 6) {
                    Picker("Qualidade", selection: Binding(get: { model.shareQuality }, set: { chosen in Task { await model.changeQuality(chosen, model.shareFps) } })) {
                        ForEach(["720", "1080", "1440", "2160"], id: \.self) { Text($0 == "2160" ? "4K" : "\($0)p").tag($0) }
                    }

                    Picker("FPS", selection: Binding(get: { model.shareFps }, set: { chosen in Task { await model.changeQuality(model.shareQuality, chosen) } })) {
                        ForEach([15, 30, 60], id: \.self) { Text("\($0) fps").tag($0) }
                    }
                }
                .labelsHidden()
                .padding(.horizontal, 8)
                .padding(.bottom, 6)

                MenuRow(icon: .eye, label: model.mine.selfView == true ? "Ocultar minha tela" : "Ver o que a sala vê") {
                    open = false

                    Task { await model.toggleSelfView() }
                }

                MenuRow(icon: .screen, label: "Mudar monitor ou aplicativo") {
                    open = false

                    Task { await model.openShare() }
                }

                MenuRow(icon: .close, label: "Parar de transmitir", tint: Theme.danger) {
                    open = false

                    Task { await model.stopSharing() }
                }
            }
        }

        if model.mine.sharing {
            Button {
                Task { await model.stopSharing() }
            } label: {
                Text("Parar")
                    .font(Theme.sans(12, .semibold))
                    .foregroundStyle(Theme.inkStrong)
                    .padding(.horizontal, 12)
                    .frame(height: 34)
                    .background(Theme.danger, in: RoundedRectangle(cornerRadius: 11, style: .continuous))
            }
            .buttonStyle(.pointer)
            .help("Parar de transmitir")
        }
    }
}

/// `animate-ping-soft`: o ponto verde com o halo que abre e some.
private struct LivePip: View {
    @State private var pulsing = false

    var body: some View {
        ZStack {
            Circle()
                .fill(Theme.online)
                .scaleEffect(pulsing ? 2.2 : 1)
                .opacity(pulsing ? 0 : 0.75)

            Circle().fill(Theme.online)
        }
        .frame(width: 7, height: 7)
        .onAppear {
            withAnimation(.easeOut(duration: 2.4).repeatForever(autoreverses: false)) {
                pulsing = true
            }
        }
    }
}

/// O botão vermelho de sair: `.btn-icon` com fundo chapado, como no React.
private struct RedButton: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .frame(width: 34, height: 34)
            .background(Theme.danger, in: RoundedRectangle(cornerRadius: 11, style: .continuous))
            .brightness(configuration.isPressed ? -0.05 : 0)
    }
}
