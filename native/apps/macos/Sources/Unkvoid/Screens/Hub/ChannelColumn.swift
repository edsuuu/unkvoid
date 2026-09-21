import SwiftUI

/// `ui/components/hub/ChannelColumn.tsx`: 300 de largura, três caixas de vidro empilhadas
/// com 12 entre elas — o nome do servidor, a lista de canais e a barra do usuário.
struct ChannelColumn: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        if let tree = model.tree {
            VStack(spacing: 12) {
                HStack(spacing: 10) {
                    Text(tree.name)
                        .font(Theme.sans(17, .semibold))
                        .tracking(-0.3)
                        .foregroundStyle(Theme.ink)
                        .lineLimit(1)
                        .frame(maxWidth: .infinity, alignment: .leading)

                    Button {
                        model.modal = .serverSettings
                    } label: {
                        Icon(name: .dots, size: 15)
                    }
                    .buttonStyle(IconButton(side: 28, radius: 9))
                    .help("Configurações do servidor")
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 14)
                .glass()

                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        ChannelGroup(section: "Canais de texto", empty: tree.textChannels.isEmpty ? "Nenhum canal de texto visível." : nil) {
                            ForEach(tree.textChannels) { item in
                                TextChannelRow(channel: item, active: item.id == model.channel?.id)
                            }
                        }

                        ChannelGroup(section: "Canais de voz", empty: tree.voiceChannels.isEmpty ? "Nenhum canal de voz visível." : nil) {
                            ForEach(tree.voiceChannels) { item in
                                VoiceChannelRow(channel: item, people: tree.voice?[item.id] ?? [])
                            }
                        }
                    }
                    .padding(16)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .scrollIndicators(.never)
                .frame(maxHeight: .infinity)
                .glass()

                UserBar()
            }
            .frame(width: 300)
        }
    }
}

/// Um bloco da lista: o rótulo mono lá em cima e as linhas embaixo, com 8 de respiro.
private struct ChannelGroup<Content: View>: View {
    var section: String
    var empty: String?
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(section).labelMono()

            if let empty {
                Text(empty)
                    .font(Theme.sans(12))
                    .foregroundStyle(Theme.inkDim)
            } else {
                VStack(spacing: 6) {
                    content
                }
            }
        }
    }
}

private struct TextChannelRow: View {
    var channel: Channel
    var active: Bool

    @EnvironmentObject private var model: AppModel

    var body: some View {
        Button {
            Task { await model.openChannel(channel) }
        } label: {
            HStack(spacing: 10) {
                Text("#")
                    .font(Theme.mono(11))
                    .foregroundStyle(active ? Theme.lilac2 : Theme.inkDim)

                Text(channel.name)
                    .font(Theme.sans(13, active ? .medium : .regular))
                    .foregroundStyle(active ? Theme.inkStrong : Theme.inkIcon)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .rowItem(selected: active)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// `VoiceChannelItem.tsx`: o canal e, indentado embaixo, quem já está lá dentro.
private struct VoiceChannelRow: View {
    var channel: Channel
    var people: [VoicePerson]

    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Button {
                Task { await model.openChannel(channel) }
            } label: {
                HStack(spacing: 8) {
                    Icon(name: .speaker, size: 14)
                        .foregroundStyle(Theme.inkDim)

                    Text(channel.name)
                        .font(Theme.sans(13))
                        .foregroundStyle(Theme.inkIcon)
                        .lineLimit(1)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("Entrar na voz")

            ForEach(people) { person in
                HStack(spacing: 8) {
                    Avatar(name: person.name, size: 22, mine: person.user_id == model.user?.id)

                    Text(person.name)
                        .font(Theme.sans(12.5))
                        .foregroundStyle(Theme.inkIcon)
                        .lineLimit(1)

                    if person.muted == true {
                        Icon(name: .micOff, size: 12).foregroundStyle(Theme.danger)
                    }

                    if person.sources?.contains("camera") == true {
                        Icon(name: .camera, size: 12).foregroundStyle(Theme.inkDim)
                    }

                    Spacer(minLength: 0)
                }
                .padding(.leading, 20)
            }
        }
        .padding(.vertical, 10)
        .padding(.horizontal, 12)
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(Theme.lineSoft, lineWidth: 1)
        )
    }
}
