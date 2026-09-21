import SwiftUI

/// `home/DirectColumn.tsx`: salas e amigos em cima, as conversas no meio e a barra de baixo.
struct DirectColumn: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 12) {
            VStack(spacing: 6) {
                tab("Salas", icon: .home, selected: model.homeTab == .servers && model.directPerson == nil, badge: 0) {
                    model.closeDirect()
                    model.homeTab = .servers
                }

                tab("Amigos", icon: .users, selected: model.homeTab == .friends && model.directPerson == nil, badge: model.incomingFriends.count) {
                    model.closeDirect()
                    model.homeTab = .friends
                }
            }
            .padding(12)
            .glass()

            ScrollView {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Mensagens diretas").labelMono()
                        .padding(.bottom, 4)

                    if model.conversationsFailed {
                        VStack(spacing: 8) {
                            Text("Não deu para carregar as conversas.")
                                .font(Theme.sans(12))
                                .foregroundStyle(Theme.danger)

                            Button("Tentar de novo") {
                                Task { await model.loadConversations() }
                            }
                            .buttonStyle(GhostButton(font: Theme.sans(12)))
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 16)
                    } else if model.conversations.isEmpty {
                        Text("Nenhuma conversa ainda.")
                            .font(Theme.sans(12))
                            .foregroundStyle(Theme.inkDim)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 16)
                    }

                    ForEach(model.conversations) { conversation in
                        row(conversation)
                    }
                }
                .padding(12)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .scrollIndicators(.never)
            .frame(maxHeight: .infinity)
            .glass()

            UserBar()
        }
        .frame(width: 260)
    }

    private func tab(_ label: String, icon: IconName, selected: Bool, badge: Int, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Icon(name: icon, size: 15)

                Text(label)
                    .font(Theme.sans(13))
                    .frame(maxWidth: .infinity, alignment: .leading)

                Badge(count: badge)
            }
            .foregroundStyle(selected ? Theme.inkStrong : Theme.inkIcon)
            .rowItem(selected: selected)
            .contentShape(Rectangle())
        }
        .buttonStyle(.pointer)
    }

    private func row(_ conversation: DirectConversation) -> some View {
        Button {
            Task { await model.openDirect(with: conversation.user) }
        } label: {
            HStack(spacing: 10) {
                Avatar(name: conversation.user.name, url: conversation.user.avatar_url, size: 28)

                VStack(alignment: .leading, spacing: 1) {
                    Text(conversation.user.name)
                        .font(Theme.sans(13, .medium))
                        .foregroundStyle(Theme.ink)
                        .lineLimit(1)

                    if let last = conversation.last {
                        Text((last.mine ? "você: " : "") + last.body)
                            .font(Theme.sans(11.5))
                            .foregroundStyle(Theme.inkDim)
                            .lineLimit(1)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                Badge(count: conversation.unread)
            }
            .rowItem(selected: model.directPerson?.id == conversation.id)
            .contentShape(Rectangle())
        }
        .buttonStyle(.pointer)
    }
}

/// A bolinha vermelha com a contagem: pedidos de amizade, mensagens não lidas.
struct Badge: View {
    var count: Int

    var body: some View {
        if count > 0 {
            Text("\(count)")
                .font(Theme.sans(10, .semibold))
                .foregroundStyle(Theme.inkStrong)
                .padding(.horizontal, 6)
                .padding(.vertical, 2)
                .background(Theme.danger, in: Capsule())
        }
    }
}
