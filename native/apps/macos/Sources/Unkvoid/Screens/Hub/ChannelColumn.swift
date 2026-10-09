import SwiftUI

/// A coluna de canais: 240 de largura, `surfaceSide`. O cabeçalho de 48 com o nome do servidor
/// (que abre o menu dele), as categorias "Canais de texto" e "Canais de voz" com as linhas de
/// 32, quem está em cada voz embaixo do canal, e a barra do usuário colada no fim.
struct ChannelColumn: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        if let tree = model.tree {
            VStack(spacing: 0) {
                ServerHeader(tree: tree)

                ScrollView {
                    VStack(alignment: .leading, spacing: 0) {
                        let loose = tree.children(of: nil)

                        if tree.categories.isEmpty || !loose.isEmpty {
                            ChannelCategory(name: "Canais de texto", kind: "text", parent: nil, channels: loose.filter { !$0.isVoice }, empty: "Nenhum canal de texto visível.") { channel in
                                TextChannelRow(channel: channel, active: channel.id == model.channel?.id && !model.stageOpen)
                            }

                            ChannelCategory(name: "Canais de voz", kind: "voice", parent: nil, channels: loose.filter(\.isVoice), empty: "Nenhum canal de voz visível.") { channel in
                                VoiceChannelRow(channel: channel)
                            }
                        }

                        ForEach(tree.categories) { category in
                            ChannelCategory(name: category.name, kind: "text", parent: category.id, channels: tree.children(of: category.id), empty: "Nenhum canal visível.") { channel in
                                if channel.isVoice {
                                    VoiceChannelRow(channel: channel)
                                } else {
                                    TextChannelRow(channel: channel, active: channel.id == model.channel?.id && !model.stageOpen)
                                }
                            }
                            .contextMenu {
                                ChannelMenuItems(channel: category)
                            }
                        }
                    }
                    .padding(.top, 8)
                    .padding(.bottom, 16)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .scrollIndicators(.never)
                .frame(maxHeight: .infinity)

                UserBar()
            }
            .frame(width: Theme.Size.side)
            .background(Theme.surfaceSide)
        }
    }
}

/// O cabeçalho de 48: o nome do servidor e a seta que abre o menu dele.
private struct ServerHeader: View {
    @EnvironmentObject private var model: AppModel

    let tree: ServerTree

    @State private var hovering = false

    var body: some View {
        Menu {
            ServerMenuItems(server: ServerSummary(id: tree.id, name: tree.name, owner_id: tree.owner_id, icon_url: tree.icon_url, last_accessed_at: nil))
        } label: {
            HStack(spacing: 8) {
                Text(tree.name)
                    .font(Theme.header)
                    .foregroundStyle(Theme.inkStrong)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)

                Icon(name: .chevronDown, size: 18)
                    .foregroundStyle(Theme.inkStrong)
            }
            .padding(.horizontal, 16)
            .frame(height: Theme.Size.header)
            .frame(maxWidth: .infinity)
            .background(hovering ? Theme.hover : .clear)
            .contentShape(Rectangle())
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .menuIndicator(.hidden)
        .onHover { hovering = $0 }
        .overlay(alignment: .bottom) {
            Rectangle().fill(Color.black.opacity(0.2)).frame(height: 1)
        }
        .help("Menu do servidor")
    }
}

/// Uma categoria: o rótulo em caixa alta com a seta que recolhe e, sob o mouse, o "+" de
/// criar canal. Sem canal visível, a frase que diz isso.
private struct ChannelCategory<Row: View>: View {
    @EnvironmentObject private var model: AppModel

    let name: String
    let kind: String
    let parent: String?
    let channels: [Channel]
    let empty: String
    @ViewBuilder let row: (Channel) -> Row

    @State private var collapsed = false
    @State private var hovering = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 2) {
                Button {
                    withAnimation(.easeOut(duration: 0.15)) { collapsed.toggle() }
                } label: {
                    HStack(spacing: 2) {
                        Icon(name: .chevronRight, size: 12)
                            .rotationEffect(.degrees(collapsed ? 0 : 90))

                        Text(name)
                            .labelMono()
                            .lineLimit(1)
                    }
                    .foregroundStyle(hovering ? Theme.ink : Theme.inkDim)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.pointer)
                .help(collapsed ? "Mostrar a categoria" : "Recolher a categoria")

                if model.abilities.allows("manageChannels"), hovering {
                    Button {
                        model.channelEditor = ChannelEditor(channel: nil, kind: kind, parent: parent)
                    } label: {
                        Icon(name: .plus, size: 16)
                    }
                    .buttonStyle(IconButton(side: 20))
                    .help("Criar canal")
                }
            }
            .padding(.leading, 8)
            .padding(.trailing, 8)
            .frame(height: 24)
            .padding(.top, 16)
            .onHover { hovering = $0 }

            if !collapsed {
                if channels.isEmpty {
                    Text(empty)
                        .font(Theme.meta)
                        .foregroundStyle(Theme.inkDim)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 6)
                } else {
                    ForEach(channels) { channel in
                        row(channel)
                    }
                }
            }
        }
    }
}

/// O menu de contexto de um canal (ou de uma categoria): editar, convidar, e excluir, para quem pode.
private struct ChannelMenuItems: View {
    @EnvironmentObject private var model: AppModel

    let channel: Channel

    var body: some View {
        if model.abilities.allows("manageChannels") {
            Button(channel.isCategory ? "Editar categoria" : "Editar canal") { model.channelEditor = ChannelEditor(channel: channel, kind: channel.type) }
        }

        if model.abilities.allows("createInvite"), !channel.isCategory {
            Button("Criar convite") { model.modal = .invitePeople }
        }

        if model.abilities.allows("manageChannels") {
            Divider()

            Button(channel.isCategory ? "Excluir categoria" : "Excluir canal", role: .destructive) { model.deleteChannel(channel) }
        }
    }
}

/// Os dois ícones de 16 que aparecem sob o mouse à direita da linha: convidar e editar.
private struct ChannelRowTools: View {
    @EnvironmentObject private var model: AppModel

    let channel: Channel

    var body: some View {
        HStack(spacing: 4) {
            if model.abilities.allows("createInvite") {
                Button {
                    model.modal = .invitePeople
                } label: {
                    Icon(name: .userPlus, size: 16)
                }
                .buttonStyle(IconButton(side: 20))
                .help("Criar convite")
            }

            if model.abilities.allows("manageChannels") {
                Button {
                    model.channelEditor = ChannelEditor(channel: channel, kind: channel.type)
                } label: {
                    Icon(name: .gear, size: 16)
                }
                .buttonStyle(IconButton(side: 20))
                .help("Editar canal")
            }
        }
    }
}

/// A linha de 32 de um canal de texto: `#`, o nome, e as ferramentas sob o mouse.
private struct TextChannelRow: View {
    @EnvironmentObject private var model: AppModel

    let channel: Channel
    let active: Bool

    @State private var hovering = false

    var body: some View {
        Button {
            Task { await model.openChannel(channel) }
        } label: {
            HStack(spacing: 6) {
                Icon(name: .hash, size: 20)
                    .foregroundStyle(Theme.inkDim)

                Text(channel.name)
                    .font(Theme.list)
                    .foregroundStyle(active ? Theme.inkStrong : hovering ? Theme.ink : Theme.inkDim)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)

                if hovering {
                    ChannelRowTools(channel: channel)
                }
            }
            .padding(.horizontal, 8)
            .frame(height: Theme.Size.row)
            .background(active ? Theme.selected : hovering ? Theme.hover : .clear, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
            .contentShape(Rectangle())
        }
        .buttonStyle(.pointer)
        .padding(.horizontal, 8)
        .padding(.vertical, 1)
        .onHover { hovering = $0 }
        .contextMenu {
            ChannelMenuItems(channel: channel)
        }
    }
}

/// A linha de um canal de voz e, embaixo dela, quem está lá: avatar de 24 com o anel de quem
/// fala, o nome, e à direita o microfone e o fone cortados, o "AO VIVO" e a câmera.
private struct VoiceChannelRow: View {
    @EnvironmentObject private var model: AppModel

    let channel: Channel

    @State private var hovering = false

    private var here: Bool {
        model.voiceChannel?.id == channel.id
    }

    var body: some View {
        let people = model.voicePeople(in: channel)

        VStack(alignment: .leading, spacing: 0) {
            Button {
                Task { await model.joinVoice(channel) }
            } label: {
                HStack(spacing: 6) {
                    Icon(name: .speaker, size: 20)
                        .foregroundStyle(Theme.inkDim)

                    Text(channel.name)
                        .font(Theme.list)
                        .foregroundStyle(here ? Theme.inkStrong : hovering ? Theme.ink : Theme.inkDim)
                        .lineLimit(1)
                        .frame(maxWidth: .infinity, alignment: .leading)

                    if model.voiceTarget == channel.id || (here && model.reconnecting) {
                        ProgressView().controlSize(.mini)
                    } else if hovering {
                        ChannelRowTools(channel: channel)
                    } else if let limit = channel.user_limit {
                        Text("\(people.count)/\(limit)")
                            .font(Theme.meta)
                            .foregroundStyle(Theme.inkDim)
                    }
                }
                .padding(.horizontal, 8)
                .frame(height: Theme.Size.row)
                .background(here ? Theme.selected : hovering ? Theme.hover : .clear, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
                .contentShape(Rectangle())
            }
            .buttonStyle(.pointer)
            .padding(.horizontal, 8)
            .padding(.vertical, 1)
            .onHover { hovering = $0 }
            .help(here ? "Ping \(model.ping.map(String.init) ?? "--") ms" : "Entrar na voz")
            .contextMenu {
                ChannelMenuItems(channel: channel)
            }

            ForEach(people) { person in
                VoicePersonRow(channel: channel, person: person)
            }
        }
    }
}

/// Uma pessoa embaixo do canal de voz: linha de 30, recuo de 36.
private struct VoicePersonRow: View {
    @EnvironmentObject private var model: AppModel

    let channel: Channel
    let person: VoicePerson

    @State private var hovering = false

    private var me: Bool {
        person.user_id == model.user?.id
    }

    private var member: Member? {
        model.tree?.members.first { $0.user_id == person.user_id }
    }

    private var here: Bool {
        model.voiceChannel?.id == channel.id
    }

    var body: some View {
        let speaking = model.isSpeaking(person.user_id)
        let live = person.sources?.contains("screen") == true

        Button {
            model.memberMenu = member
        } label: {
            HStack(spacing: 8) {
                Avatar(name: member?.displayName ?? person.name, url: member?.avatar_url, size: 24, mine: me)
                    .overlay(Circle().strokeBorder(Theme.online, lineWidth: 2).padding(-2).opacity(speaking ? 1 : 0))
                    .animation(.easeOut(duration: 0.1), value: speaking)

                Text(member?.displayName ?? person.name)
                    .font(Theme.voicePerson)
                    .foregroundStyle(hovering ? Theme.ink : Theme.inkSoft)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)

                // O fone cortado fica por cima do microfone cortado: quem não ouve também
                // não conversa, e um ícone só diz as duas coisas.
                if member?.server_deaf == true || (me && model.deafened) {
                    Icon(name: .headphonesOff, size: 16)
                        .foregroundStyle(member?.server_deaf == true ? Theme.danger : Theme.inkDim)
                        .help(member?.server_deaf == true ? "Ensurdecido pelo servidor" : "Ensurdecido")
                } else if member?.server_mute == true || (me ? model.micShownOff : person.muted == true) {
                    Icon(name: .micOff, size: 16)
                        .foregroundStyle(member?.server_mute == true ? Theme.danger : Theme.inkDim)
                        .help(member?.server_mute == true ? "Mutado pelo servidor" : "Mutado")
                }

                if live {
                    Button {
                        Task { await watch() }
                    } label: {
                        Text("AO VIVO")
                            .font(Theme.sans(11, .bold))
                            .foregroundStyle(.white)
                            .padding(.horizontal, 4)
                            .frame(height: 16)
                            .background(Theme.live, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
                    }
                    .buttonStyle(.pointer)
                    .help(here ? "Assistir transmissão" : "Assistir transmissão — entra na voz")
                }

                if person.sources?.contains("camera") == true {
                    Icon(name: .camera, size: 16)
                        .foregroundStyle(Theme.inkDim)
                        .help("Câmera ligada")
                }
            }
            .padding(.leading, 28)
            .padding(.trailing, 8)
            .frame(height: Theme.Size.voiceRow)
            .background(hovering ? Theme.hover : .clear, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
            .contentShape(Rectangle())
        }
        .buttonStyle(.pointer)
        .disabled(member == nil)
        .padding(.horizontal, 8)
        .onHover { hovering = $0 }
        .help(member == nil ? person.name : "Ações do membro")
        .contextMenu {
            if let member {
                MemberMenuItems(member: member)
            }
        }
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
