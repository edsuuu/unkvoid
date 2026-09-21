import SwiftUI

/// O Hub, igual a `ui/components/hub/HubScreen.tsx`: a trilha dos servidores à esquerda e,
/// ao lado, ou a Home ou o servidor aberto. `flex h-full gap-3 p-3`.
struct HubScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 12) {
            ServerRail()

            if model.treeLoading {
                LoadingColumns()
            } else if model.home || model.tree == nil {
                HomeView()
            } else {
                ServerView()
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .overlay(alignment: .top) { Notice() }
        .overlay { modal }
        .task { await model.loadRecentRooms() }
    }

    @ViewBuilder
    private var modal: some View {
        switch model.modal {
        case .account: UserSettingsModal()
        case .serverSettings: ServerSettingsModal()
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

            ChatPanel()

            if model.membersOpen {
                MemberList()
            }
        }
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

/// O aviso curto que some sozinho — o `Toasts.tsx` do React.
private struct Notice: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        if let notice = model.notice {
            Text(notice)
                .font(Theme.sans(13))
                .foregroundStyle(Theme.inkBody)
                .padding(.vertical, 10)
                .padding(.horizontal, 16)
                .popoverPanel()
                .padding(.top, 8)
                .onTapGesture { model.dismissNotice() }
                .task(id: notice) {
                    try? await Task.sleep(for: .seconds(4))
                    model.dismissNotice()
                }
        }
    }
}
