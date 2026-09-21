import SwiftUI

/// `home/FriendsPanel.tsx`: amigos, pedidos pendentes e bloqueados, e o campo de adicionar
/// pelo e-mail.
struct FriendsPanel: View {
    @EnvironmentObject private var model: AppModel

    @State private var tab = "accepted"
    @State private var email = ""
    @State private var busy = false
    @FocusState private var typing: Bool

    private static let tabs = [("accepted", "Amigos"), ("pending", "Pendentes"), ("blocked", "Bloqueados")]

    var body: some View {
        VStack(spacing: 12) {
            HStack(spacing: 6) {
                ForEach(Self.tabs, id: \.0) { key, label in
                    Button {
                        tab = key
                    } label: {
                        HStack(spacing: 6) {
                            Text(label)

                            if key == "pending" {
                                Badge(count: model.incomingFriends.count)
                            }
                        }
                        .font(Theme.sans(12.5, .medium))
                        .foregroundStyle(tab == key ? Theme.inkStrong : Theme.inkDim)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 7)
                        .background(tab == key ? Theme.row : .clear, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                    }
                    .buttonStyle(.plain)
                }

                Spacer(minLength: 0)
            }

            Divider().overlay(Theme.line)

            HStack(spacing: 8) {
                TextField("E-mail de quem você quer adicionar", text: $email)
                    .field(focused: typing)
                    .focused($typing)
                    .onSubmit(add)

                Button(action: add) {
                    HStack(spacing: 8) {
                        if busy {
                            ProgressView().controlSize(.small)
                        }

                        Text("Adicionar")
                    }
                }
                .buttonStyle(PrimaryButton())
                .fixedSize()
                .disabled(busy)
            }

            ScrollView {
                VStack(spacing: 6) {
                    if model.friendsLoading {
                        ForEach(0 ..< 2, id: \.self) { _ in Skeleton(height: 48) }
                    } else if model.friendsFailed {
                        VStack(spacing: 8) {
                            Text("Não deu para carregar os seus amigos.")
                                .font(Theme.sans(13))
                                .foregroundStyle(Theme.danger)

                            Button("Tentar de novo") {
                                Task { await model.loadSocial() }
                            }
                            .buttonStyle(GhostButton())
                        }
                        .padding(.vertical, 40)
                    } else if rows.isEmpty {
                        Text(empty)
                            .font(Theme.sans(13))
                            .foregroundStyle(Theme.inkDim)
                            .padding(.vertical, 40)
                    }

                    ForEach(rows) { friendship in
                        row(friendship)
                    }
                }
                .frame(maxWidth: .infinity)
            }
            .frame(maxHeight: .infinity)
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .glass()
    }

    private var rows: [Friendship] {
        switch tab {
        case "pending": model.incomingFriends + model.outgoingFriends
        case "blocked": model.friends.filter { $0.status == "blocked" }
        default: model.friends.filter { $0.status == "accepted" }
        }
    }

    private var empty: String {
        switch tab {
        case "pending": "Nenhum pedido pendente."
        case "blocked": "Ninguém bloqueado."
        default: "Você ainda não tem amigos por aqui. Adicione pelo e-mail."
        }
    }

    private func row(_ friendship: Friendship) -> some View {
        let person = model.other(in: friendship)
        let waiting = friendship.requester.id == model.user?.id

        return HStack(spacing: 10) {
            Avatar(name: person.name, url: person.avatar_url, size: 30)

            VStack(alignment: .leading, spacing: 1) {
                Text(person.name)
                    .font(Theme.sans(13, .medium))
                    .foregroundStyle(Theme.ink)
                    .lineLimit(1)

                if tab == "pending" {
                    Text(waiting ? "aguardando resposta" : "quer ser seu amigo")
                        .font(Theme.sans(11.5))
                        .foregroundStyle(Theme.inkDim)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            if tab == "accepted" {
                small(.chat, "Mandar mensagem") { await model.openDirect(with: person) }
            }

            if tab == "pending", !waiting {
                Button("Aceitar") {
                    Task { await model.answer(friendship, with: "accept") }
                }
                .buttonStyle(GhostButton(font: Theme.sans(12)))
            }

            if tab != "blocked" {
                small(.close, "Bloquear") { await model.answer(friendship, with: "block") }
            }

            small(.trash, "Desfazer") { await model.remove(friendship) }
        }
        .rowItem()
    }

    private func small(_ icon: IconName, _ hint: String, _ action: @escaping () async -> Void) -> some View {
        Button {
            Task { await action() }
        } label: {
            Icon(name: icon, size: 13)
        }
        .buttonStyle(IconButton(side: 28, radius: 8))
        .help(hint)
    }

    private func add() {
        let wanted = email.trimmingCharacters(in: .whitespacesAndNewlines)

        guard !wanted.isEmpty, !busy else {
            return
        }

        busy = true

        Task {
            if await model.requestFriend(wanted) {
                email = ""
            }

            busy = false
        }
    }
}
