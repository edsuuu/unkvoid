import SwiftUI

/// `ui/components/hub/ChannelColumn.tsx`: 300 de largura, três caixas de vidro empilhadas
/// com 12 entre elas — o nome do servidor, a lista de canais e a barra do usuário.
struct ChannelColumn: View {
    @EnvironmentObject private var model: AppModel

    private var manage: Bool {
        model.abilities.allows("manageChannels")
    }

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
                        ChannelGroup(section: "Canais de texto", empty: tree.textChannels.isEmpty ? "Nenhum canal de texto visível." : nil, create: manage ? "text" : nil) {
                            ForEach(tree.textChannels) { item in
                                TextChannelRow(channel: item, active: item.id == model.channel?.id)
                            }
                        }

                        ChannelGroup(section: "Canais de voz", empty: tree.voiceChannels.isEmpty ? "Nenhum canal de voz visível." : nil, create: manage ? "voice" : nil) {
                            ForEach(tree.voiceChannels) { item in
                                VoiceChannelRow(channel: item)
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
    @EnvironmentObject private var model: AppModel

    var section: String
    var empty: String?
    /// O tipo de canal que o "+" cria; `nil` para quem não gerencia canais.
    var create: String?
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(section).labelMono()

                Spacer(minLength: 0)

                if let create {
                    Button {
                        model.channelEditor = ChannelEditor(channel: nil, kind: create)
                    } label: {
                        Icon(name: .plus, size: 11)
                    }
                    .buttonStyle(IconButton(side: 20, radius: 7))
                    .help(create == "voice" ? "Criar canal de voz" : "Criar canal de texto")
                }
            }

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
        .buttonStyle(.pointer)
        .contextMenu {
            if model.abilities.allows("manageChannels") {
                Button("Editar canal") {
                    model.channelEditor = ChannelEditor(channel: channel, kind: channel.type)
                }
            }
        }
    }
}

/// `VoiceChannelItem.tsx`: o canal e, indentado embaixo, quem está lá dentro. No canal em que
/// se está o cartão fica lilás, com o tempo de conexão e o botão da sala focada.
private struct VoiceChannelRow: View {
    var channel: Channel

    @EnvironmentObject private var model: AppModel
    @State private var hovered: Int?

    var body: some View {
        let people = model.voicePeople(in: channel)

        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Button {
                    Task { await model.joinVoice(channel) }
                } label: {
                    HStack(spacing: 8) {
                        Icon(name: .speaker, size: 14)
                            .foregroundStyle(here ? Theme.lilac2 : Theme.inkDim)

                        Text(channel.name)
                            .font(Theme.sans(13, here ? .medium : .regular))
                            .foregroundStyle(here ? Theme.inkBody : Theme.inkIcon)
                            .lineLimit(1)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    // A área de clique vai até a moldura do canal (10 em cima e embaixo, 12 à
                    // esquerda): o respiro entra no botão e sai de volta do desenho.
                    .padding(.vertical, 10)
                    .padding(.leading, 12)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.pointer)
                .padding(.vertical, -10)
                .padding(.leading, -12)
                .help(here ? "Ping \(model.ping.map(String.init) ?? "--") ms" : "Entrar na voz")

                if model.voiceTarget == channel.id || (here && model.reconnecting) {
                    ProgressView().controlSize(.mini)
                } else if here {
                    Button {
                        model.focusedRoom = true
                        model.stageOpen = true
                    } label: {
                        Icon(name: .focus, size: 11).foregroundStyle(Theme.lilac2)
                    }
                    .buttonStyle(.pointer)
                    .frame(width: 22, height: 22)
                    .background(Theme.brand.opacity(0.15), in: RoundedRectangle(cornerRadius: 7, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 7, style: .continuous).strokeBorder(Theme.brand.opacity(0.35), lineWidth: 1))
                    .help("Mudar visual para focado")

                    if let since = model.enteredRoomAt {
                        Text(since, style: .timer)
                            .font(Theme.mono(9.5))
                            .monospacedDigit()
                            .foregroundStyle(Theme.inkDim)
                    }
                }
            }

            if !people.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(people) { person in
                        row(person)
                    }
                }
                .padding(.leading, 20)
            }
        }
        .padding(.vertical, 10)
        .padding(.horizontal, 12)
        .background(here ? Theme.brand.opacity(0.1) : .clear, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(here ? Theme.brand.opacity(0.3) : Theme.lineSoft, lineWidth: 1)
        )
        .contextMenu {
            if model.abilities.allows("manageChannels") {
                Button("Editar canal") {
                    model.channelEditor = ChannelEditor(channel: channel, kind: channel.type)
                }
            }
        }
    }

    private var here: Bool {
        model.voiceChannel?.id == channel.id
    }

    private func row(_ person: VoicePerson) -> some View {
        let me = person.user_id == model.user?.id
        let member = model.tree?.members.first { $0.user_id == person.user_id }
        let live = person.sources?.contains("screen") == true

        return HStack(spacing: 8) {
            Button {
                model.memberMenu = member
            } label: {
                HStack(spacing: 8) {
                    Avatar(name: person.name, url: member?.avatar_url, size: 22, mine: me)
                        .overlay(Circle().strokeBorder(Theme.online, lineWidth: 2).padding(-2).opacity(model.isSpeaking(person.user_id) ? 1 : 0))
                        .animation(.easeOut(duration: 0.12), value: model.isSpeaking(person.user_id))

                    Text(person.name)
                        .font(Theme.sans(12.5))
                        .foregroundStyle(me ? Theme.inkBody : Theme.inkIcon)
                        .lineLimit(1)

                    // O fone cortado fica por cima do microfone cortado: quem não ouve também
                    // não conversa, e um ícone só diz as duas coisas.
                    if me, model.deafened {
                        Icon(name: .headphonesOff, size: 12).foregroundStyle(Theme.danger).help("Áudio mutado")
                    } else if me ? model.micShownOff : person.muted == true {
                        Icon(name: .micOff, size: 12).foregroundStyle(Theme.danger).help("Microfone mutado")
                    }

                    if person.sources?.contains("camera") == true {
                        Icon(name: .camera, size: 12).foregroundStyle(Theme.inkDim).help("Câmera ligada")
                    }

                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.pointer)
            .disabled(member == nil)
            .help(member == nil ? person.name : "Ações do membro")

            if member != nil, !me, hovered == person.user_id {
                Button {
                    model.memberMenu = member
                } label: {
                    Icon(name: .dots, size: 13)
                }
                .buttonStyle(IconButton(side: 22, radius: 7))
                .help("Banir, expulsar, desconectar e mais")
            }

            if live {
                Button {
                    Task { await watch() }
                } label: {
                    HStack(spacing: 4) {
                        Circle().fill(.white).frame(width: 5, height: 5)

                        Text("AO VIVO")
                    }
                    .font(Theme.mono(9, .semibold))
                    .foregroundStyle(Theme.inkStrong)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Theme.danger, in: RoundedRectangle(cornerRadius: 5, style: .continuous))
                }
                .buttonStyle(.pointer)
                .help(here ? "Assistir transmissão — abre aqui do lado" : "Assistir transmissão — entra na voz e abre ao lado")
            }
        }
        .onHover { hovered = $0 ? person.user_id : (hovered == person.user_id ? nil : hovered) }
    }

    /// Fora da voz, entra; dentro, abre o palco e reabre o que tinha sido fechado.
    private func watch() async {
        guard here else {
            await model.joinVoice(channel)

            return
        }

        model.stageOpen = true

        await model.watch(nil)
    }
}
