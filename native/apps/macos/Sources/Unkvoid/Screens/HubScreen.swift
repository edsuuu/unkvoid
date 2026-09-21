import SwiftUI

/// O Hub, igual a `ui/components/hub/HubScreen.tsx`: a trilha dos servidores à esquerda e,
/// ao lado, ou a Home ou o servidor aberto. `flex h-full gap-3 p-3`.
struct HubScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 12) {
            if let newer = model.newerVersion {
                UpdateBanner(version: newer.version, url: newer.url)
            }

            columns
        }
        .padding(12)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .overlay { modal }
        .overlay {
            if let editor = model.channelEditor {
                ChannelModal(editor: editor).id(editor.id)
            }
        }
        .overlay {
            if let editor = model.roleEditor {
                RoleModal(editor: editor).id(editor.id)
            }
        }
        .overlay {
            if let member = model.memberMenu {
                MemberMenu(member: member).id(member.user_id)
            }
        }
        .overlay {
            if model.user?.nickname_confirmed == false {
                NicknameModal()
            }
        }
        .overlay {
            if let confirmation = model.confirmation {
                ConfirmDialog(confirmation: confirmation)
            }
        }
        .task { await model.loadRecentRooms() }
    }

    private var columns: some View {
        HStack(spacing: 12) {
            ServerRail()

            if model.treeLoading {
                LoadingColumns()
            } else if model.focusedRoom, model.voiceChannel != nil, !model.home {
                FocusedRoom()
            } else if model.home || model.tree == nil {
                HomeView()
            } else {
                ServerView()
            }
        }
    }

    @ViewBuilder
    private var modal: some View {
        switch model.modal {
        case .account: UserSettingsModal()
        case .serverSettings: ServerSettingsModal()
        case .invite: InviteModal()
        case .logs: LogsModal()
        case nil: EmptyView()
        }
    }
}

/// `ServerView.tsx`: canais à esquerda, o chat no meio, os membros à direita.
private struct ServerView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 12) {
            ChannelColumn()

            if let voice = model.voiceChannel, model.stageOpen {
                VStack(spacing: 12) {
                    if model.inviteBanner {
                        InviteBanner()
                    }

                    VoiceStage(channel: voice)
                }

                if model.voiceChatOpen {
                    ChatPanel(chat: model.voiceChat) { model.voiceChatOpen = false }
                        .frame(width: 320)
                }
            } else {
                VStack(spacing: 12) {
                    if model.inviteBanner {
                        InviteBanner()
                    }

                    ChatPanel(chat: model.chat)
                }

                if model.membersOpen {
                    MemberList()
                }
            }
        }
        .overlay {
            if model.shareOpen {
                ShareModal()
            }
        }
    }
}

/// O palco da voz dentro do servidor: o nome do canal, o caminho de volta ao chat e as
/// transmissões de quem está lá.
private struct VoiceStage: View {
    @EnvironmentObject private var model: AppModel

    let channel: Channel

    var body: some View {
        VStack(spacing: 12) {
            HStack(spacing: 8) {
                Icon(name: .speaker, size: 15).foregroundStyle(Theme.online)

                Text(channel.name)
                    .font(Theme.sans(14, .semibold))
                    .foregroundStyle(Theme.ink)
                    .lineLimit(1)

                Spacer(minLength: 0)

                PeopleMenu()

                Button {
                    model.voiceChatOpen.toggle()
                } label: {
                    HStack(spacing: 6) {
                        Icon(name: .chat, size: 13)

                        Text("Chat da voz")

                        Badge(count: model.voiceChatOpen ? 0 : model.voiceChat.unread)
                    }
                }
                .buttonStyle(GhostButton(font: Theme.sans(12), padding: EdgeInsets(top: 6, leading: 10, bottom: 6, trailing: 10)))
                .help(model.voiceChatOpen ? "Fechar o chat desta voz" : "Ver o chat desta voz")

                Button {
                    model.focusedRoom = true
                } label: {
                    HStack(spacing: 6) {
                        Icon(name: .focus, size: 13)

                        Text("Sala focada")
                    }
                }
                .buttonStyle(GhostButton(font: Theme.sans(12), padding: EdgeInsets(top: 6, leading: 10, bottom: 6, trailing: 10)))
                .help("Mudar visual para focado")

                Button("Voltar ao chat") {
                    model.stageOpen = false
                }
                .buttonStyle(GhostButton(font: Theme.sans(12), padding: EdgeInsets(top: 6, leading: 10, bottom: 6, trailing: 10)))
            }

            if let roomError = model.roomError {
                Text(roomError)
                    .font(Theme.sans(12.5))
                    .foregroundStyle(Theme.danger)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .onTapGesture { model.dismissRoomError() }
            }

            Stage()
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .glass()
    }
}

/// O esqueleto do `treeLoading`: as mesmas caixas cinzas que o React mostra enquanto o
/// servidor não chegou.
private struct LoadingColumns: View {
    var body: some View {
        HStack(spacing: 12) {
            VStack(spacing: 12) {
                Skeleton(height: 24, width: 160)
                    .padding(16)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .glass()

                VStack(spacing: 10) {
                    ForEach(0 ..< 4, id: \.self) { _ in Skeleton(height: 36) }

                    Spacer(minLength: 0)
                }
                .padding(16)
                .frame(maxHeight: .infinity)
                .glass()
            }
            .frame(width: 300)

            Skeleton(height: 24, width: 128)
                .padding(20)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                .glass()
        }
    }
}

struct Skeleton: View {
    var height: CGFloat
    var width: CGFloat?

    var body: some View {
        RoundedRectangle(cornerRadius: 8, style: .continuous)
            .fill(Color.white.opacity(0.07))
            .frame(width: width, height: height)
            .frame(maxWidth: width == nil ? .infinity : nil, alignment: .leading)
    }
}

/// O convite do servidor que acabou de nascer, para copiar e mandar.
private struct InviteBanner: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 12) {
            Text("Convite da sala")
                .font(Theme.sans(13))
                .foregroundStyle(Theme.inkSoft)

            Text(model.tree?.invite_code ?? "").codeChip()

            Button {
                model.copyInvite()
            } label: {
                HStack(spacing: 6) {
                    Icon(name: .copy, size: 12)

                    Text("Copiar")
                }
            }
            .buttonStyle(GhostButton(font: Theme.sans(12), padding: EdgeInsets(top: 6, leading: 10, bottom: 6, trailing: 10)))

            Spacer(minLength: 0)

            Button {
                model.inviteBanner = false
            } label: {
                Icon(name: .close, size: 14).foregroundStyle(Theme.inkDim)
            }
            .buttonStyle(.plain)
            .help("Fechar")
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .glass(radius: 16)
    }
}

/// `hub/FocusedRoom.tsx`: a voz tomando o Hub — a barra da sala em cima, o palco embaixo e,
/// se aberto, o chat da voz ao lado.
private struct FocusedRoom: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 12) {
            VStack(spacing: 12) {
                VoiceToolbar()

                Stage()
            }

            if model.voiceChatOpen {
                ChatPanel(chat: model.voiceChat) { model.voiceChatOpen = false }
                    .frame(width: 320)
            }
        }
        .overlay {
            if model.shareOpen {
                ShareModal()
            }
        }
    }
}

/// `RoomToolbar.tsx` no modo `voice`: voltar aos canais, o canal, quem está, o ping, e câmera,
/// microfone, áudio, chat, tela e sair.
private struct VoiceToolbar: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 8) {
            Button {
                model.focusedRoom = false
            } label: {
                Icon(name: .arrowLeft, size: 14)
            }
            .buttonStyle(IconButton(side: 30, radius: 9, tone: .on))
            .help("Voltar para os canais do servidor")

            Icon(name: .speaker, size: 14).foregroundStyle(Theme.online)

            Text(model.voiceChannel?.name ?? "")
                .font(Theme.sans(13.5, .semibold))
                .foregroundStyle(Theme.ink)
                .lineLimit(1)

            PeopleMenu()

            Text(model.ping.map { "\($0) ms" } ?? "-- ms")
                .font(Theme.mono(10.5))
                .monospacedDigit()
                .foregroundStyle(Theme.inkDim)
                .help("Ida e volta até o servidor de mídia")

            Spacer(minLength: 0)

            button(model.mine.camera ? .camera : .cameraOff, model.mine.camera ? "Desligar a câmera" : "Ligar a câmera", tone: model.mine.camera ? .on : .idle, enabled: model.mine.canVideo) {
                await model.toggleCamera()
            }

            button(micOff ? .micOff : .mic, micOff ? "Ativar o microfone" : "Mutar o microfone", tone: micOff ? .off : .idle, enabled: model.mine.canSpeak) {
                await model.toggleMute()
            }

            button(model.deafened ? .headphonesOff : .headphones, model.deafened ? "Voltar a ouvir" : "Ensurdecer: não ouvir ninguém", tone: model.deafened ? .off : .idle, enabled: true) {
                await model.toggleDeafen()
            }

            button(.chat, model.voiceChatOpen ? "Fechar o chat desta voz" : "Ver o chat desta voz", tone: model.voiceChatOpen ? .on : .idle, enabled: true) {
                model.voiceChatOpen.toggle()
            }
            .overlay(alignment: .topTrailing) {
                if !model.voiceChatOpen, model.voiceChat.unread > 0 {
                    Circle().fill(Theme.danger).frame(width: 10, height: 10).offset(x: 3, y: -3)
                }
            }

            button(.screen, model.mine.sharing ? "Parar de transmitir" : "Compartilhar tela", tone: model.mine.sharing ? .on : .idle, enabled: model.mine.canShare) {
                if model.mine.sharing {
                    await model.stopSharing()
                } else {
                    await model.openShare()
                }
            }

            Button {
                Task { await model.leaveVoice() }
            } label: {
                Icon(name: .phoneOff, size: 17)
                    .foregroundStyle(Theme.inkStrong)
                    .frame(width: 34, height: 34)
                    .background(Theme.danger, in: RoundedRectangle(cornerRadius: 11, style: .continuous))
            }
            .buttonStyle(.plain)
            .help("Sair da voz")
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .glass(radius: 16)
    }

    private var micOff: Bool {
        !model.mine.mic || model.mine.micMuted || !model.mine.canSpeak
    }

    private func button(_ icon: IconName, _ hint: String, tone: IconButton.Tone, enabled: Bool, _ action: @escaping @MainActor () async -> Void) -> some View {
        Button {
            Task { await action() }
        } label: {
            Icon(name: icon, size: 16)
        }
        .buttonStyle(IconButton(tone: tone))
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.4)
        .help(hint)
    }
}

/// Há uma versão mais nova publicada: o aviso leva ao instalador, e some se a pessoa dispensar.
private struct UpdateBanner: View {
    @EnvironmentObject private var model: AppModel

    let version: String
    let url: URL

    var body: some View {
        HStack(spacing: 12) {
            Text("Há uma versão nova do Unkvoid")
                .font(Theme.sans(13))
                .foregroundStyle(Theme.inkSoft)

            Text(version).codeChip(size: 12)

            Button("Baixar") {
                NSWorkspace.shared.open(url)
            }
            .buttonStyle(GhostButton(font: Theme.sans(12), padding: EdgeInsets(top: 6, leading: 10, bottom: 6, trailing: 10)))

            Spacer(minLength: 0)

            Button {
                model.newerVersion = nil
            } label: {
                Icon(name: .close, size: 14).foregroundStyle(Theme.inkDim)
            }
            .buttonStyle(.plain)
            .help("Agora não")
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .glass(radius: 16)
    }
}
