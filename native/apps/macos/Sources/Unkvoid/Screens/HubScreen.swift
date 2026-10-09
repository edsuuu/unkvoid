import SwiftUI

/// O Hub em quatro colunas coladas, separadas só pela cor: o trilho de servidores (72), os
/// canais (240), o centro (chat ou chamada) e os membros (240). Na voz, o centro vira a chamada
/// e o chat da voz abre à direita, no lugar dos membros.
struct HubScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 0) {
            if let newer = model.newerVersion {
                UpdateBanner(version: newer.version, url: newer.url)
            }

            columns
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.surfaceChat)
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
        HStack(spacing: 0) {
            ServerRail()

            if model.treeLoading {
                LoadingColumns()
            } else if model.home || model.tree == nil {
                HomeView()
                    .padding(12)
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
        case .invitePeople: InvitePeopleModal()
        case .logs: LogsModal()
        case nil: EmptyView()
        }
    }
}

/// Canais à esquerda; no meio o chat do canal de texto (com os membros à direita) ou a chamada
/// do canal de voz (com o chat da voz à direita, de 360).
private struct ServerView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 0) {
            ChannelColumn()

            if let voice = model.voiceChannel, model.stageOpen {
                CallView(channel: voice)

                if model.voiceChatOpen {
                    ChatPanel(chat: model.voiceChat) { model.voiceChatOpen = false }
                        .frame(width: Theme.Size.voiceChat)
                }
            } else {
                ChatPanel(chat: model.chat)

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

/// A chamada: fundo preto, o cabeçalho transparente por cima (o canal e o botão do chat), a
/// grade de quem está lá e das telas, e a barra de controles embaixo.
private struct CallView: View {
    @EnvironmentObject private var model: AppModel

    let channel: Channel

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Icon(name: .speaker, size: 24).foregroundStyle(Theme.inkSoft)

                Text(channel.name)
                    .font(Theme.header)
                    .foregroundStyle(Theme.inkStrong)
                    .lineLimit(1)

                Spacer(minLength: 0)

                Button {
                    model.voiceChatOpen.toggle()
                } label: {
                    Icon(name: .chat, size: 24)
                }
                .buttonStyle(IconButton(tone: model.voiceChatOpen ? .on : .idle))
                .overlay(alignment: .topTrailing) {
                    if !model.voiceChatOpen, model.voiceChat.unread > 0 {
                        Circle().fill(Theme.danger).frame(width: 10, height: 10).offset(x: 2, y: -2)
                    }
                }
                .help(model.voiceChatOpen ? "Fechar o chat" : "Chat")
            }
            .padding(.horizontal, 16)
            .frame(height: Theme.Size.header)

            if let roomError = model.roomError {
                Text(roomError)
                    .font(Theme.sans(14))
                    .foregroundStyle(Theme.danger)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 16)
                    .onTapGesture { model.dismissRoomError() }
            }

            CallGrid(channel: channel)
                .padding(.horizontal, 16)
                .padding(.vertical, 8)

            CallControls()
                .padding(.bottom, 16)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.surfaceCall)
    }
}

/// O esqueleto do `treeLoading`: as colunas no lugar, sem conteúdo ainda.
private struct LoadingColumns: View {
    var body: some View {
        HStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 12) {
                Skeleton(height: 20, width: 140)
                    .padding(.horizontal, 16)
                    .frame(height: Theme.Size.header)

                ForEach(0 ..< 4, id: \.self) { _ in
                    Skeleton(height: 20, width: 160).padding(.horizontal, 16)
                }

                Spacer(minLength: 0)
            }
            .frame(width: Theme.Size.side)
            .background(Theme.surfaceSide)

            Skeleton(height: 20, width: 120)
                .padding(16)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
    }
}

struct Skeleton: View {
    var height: CGFloat
    var width: CGFloat?

    var body: some View {
        RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous)
            .fill(Theme.hover)
            .frame(width: width, height: height)
            .frame(maxWidth: width == nil ? .infinity : nil, alignment: .leading)
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
                .font(Theme.sans(14))
                .foregroundStyle(.white)

            Text(version).codeChip(size: 12)

            Button("Baixar") {
                NSWorkspace.shared.open(url)
            }
            .buttonStyle(GhostButton(padding: EdgeInsets(top: 4, leading: 12, bottom: 4, trailing: 12)))

            Spacer(minLength: 0)

            Button {
                model.newerVersion = nil
            } label: {
                Icon(name: .close, size: 16).foregroundStyle(.white)
            }
            .buttonStyle(.pointer)
            .help("Agora não")
        }
        .padding(.horizontal, 16)
        .frame(height: 36)
        .background(Theme.brand)
    }
}
