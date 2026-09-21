import SwiftUI

/// A sala por código, igual a `ui/components/room/RoomScreen.tsx`: a barra de ferramentas
/// em cima, o aviso de erro quando há um, e o palco embaixo.
///
/// O palco está no estado "ninguém está compartilhando" porque é a verdade: quem está na
/// sala e o vídeo de cada um saem do mapa de peers que o `SfuClient.ts` mantém, e isso é
/// do `shared/core` — ele ainda não tem. Ver o relatório no `README.md` desta pasta.
struct RoomScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 12) {
            RoomToolbar()

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
                    .buttonStyle(.plain)
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
            .buttonStyle(.plain)
            .help("Copiar o código para mandar a alguém")

            PeopleChip()

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

/// `Stage.tsx` sem nenhuma transmissão: o cartão largo no meio da tela.
private struct Stage: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 0) {
            ZStack {
                Icon(name: .screen, size: 26)
                    .foregroundStyle(Theme.lilac2)
            }
            .frame(width: 64, height: 64)
            .background(Theme.brand.opacity(0.15), in: RoundedRectangle(cornerRadius: 20, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 20, style: .continuous)
                    .strokeBorder(Theme.brand.opacity(0.3), lineWidth: 1)
            )
            .padding(.bottom, 20)

            Text("Ninguém está compartilhando ainda.")
                .font(Theme.sans(19, .semibold))
                .tracking(-0.3)
                .foregroundStyle(Theme.ink)
                .multilineTextAlignment(.center)

            if let room = model.room {
                HStack(spacing: 5) {
                    Text("Mande o código")

                    Text(room).codeChip(size: 12)

                    Text("para quem você quer aqui.")
                }
                .font(Theme.sans(13))
                .foregroundStyle(Theme.inkSoft)
                .padding(.top, 10)
            }

        }
        .padding(36)
        .frame(maxWidth: 520)
        .glass(radius: 24, shadowed: true)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// Quem está na sala, como o `PeopleMenu.tsx`: os avatares em pílula com a contagem.
///
/// A lista de quem está sai do `Roster` do `shared/core`, que existe mas ainda não
/// atravessa a ABI — então por enquanto a pílula mostra só quem está nesta máquina.
private struct PeopleChip: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 10) {
            Avatar(name: model.name.isEmpty ? "?" : model.name, size: 26, mine: true)

            Text("1")
                .font(Theme.sans(12))
                .foregroundStyle(Theme.inkIcon)
        }
        .padding(.leading, 6)
        .padding(.trailing, 12)
        .padding(.vertical, 5)
        .background(Theme.row, in: Capsule())
        .overlay(Capsule().strokeBorder(Theme.lineStrong, lineWidth: 1))
        .help("Quem está na sala")
    }
}

/// O botão de compartilhar, no lugar que ele ocupa no React — à esquerda do botão de sair.
///
/// Fica desligado porque a captura ainda não atravessa a ABI: `shared/capture` existe e
/// funciona, mas não há ação no núcleo que a ligue. Botão aceso que não transmite seria
/// pior do que botão apagado que diz por quê.
private struct ShareButton: View {
    var body: some View {
        Button {
        } label: {
            Icon(name: .screen, size: 16)
        }
        .buttonStyle(IconButton())
        .disabled(true)
        .help("Compartilhar tela — ainda não ligado neste app")
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
