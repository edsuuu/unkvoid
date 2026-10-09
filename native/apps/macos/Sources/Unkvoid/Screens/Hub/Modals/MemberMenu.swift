import SwiftUI

/// A janelinha de um membro: quem é a pessoa, o volume dela, mandar mensagem, e o que dá para
/// fazer com ela aqui — apelido, cargos, mutar, ensurdecer, mover para outra voz, tirar da voz,
/// expulsar e banir. O que aparece é o que o núcleo calculou; quem autoriza é o Laravel.
struct MemberMenu: View {
    @EnvironmentObject private var model: AppModel

    let member: Member

    @State private var nickname = ""
    @State private var note = ""
    @State private var banning = false
    @State private var moving = false
    @State private var reason = ""

    var body: some View {
        let actions = model.abilities.actions(on: member)
        let this = member.user_id == model.user?.id
        let badges = (model.tree?.roles ?? []).filter { $0.is_everyone != true && member.role_ids.contains($0.id) }

        ZStack {
            Color.black.opacity(0.001)
                .ignoresSafeArea()
                .onTapGesture { model.memberMenu = nil }

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 12) {
                    Avatar(name: member.displayName, url: member.avatar_url, size: 40, mine: this, status: model.online.contains("\(member.user_id)") || this, ring: Theme.surfaceFloat)

                    VStack(alignment: .leading, spacing: 1) {
                        Text(member.displayName)
                            .font(Theme.header)
                            .foregroundStyle(Theme.inkStrong)
                            .lineLimit(1)

                        if member.nickname != nil {
                            Text(member.name)
                                .font(Theme.meta)
                                .foregroundStyle(Theme.inkDim)
                        }

                        if member.is_owner {
                            Text("dono do servidor").labelMono()
                        }
                    }
                }
                .padding(.horizontal, 8)
                .padding(.top, 4)
                .padding(.bottom, 8)

                if !badges.isEmpty {
                    FlowRow(spacing: 4) {
                        ForEach(badges) { role in
                            HStack(spacing: 4) {
                                Circle().fill(Theme.hex(role.color) ?? Theme.inkDim).frame(width: 12, height: 12)

                                Text(role.name)
                                    .font(Theme.meta)
                                    .foregroundStyle(Theme.ink)
                            }
                            .padding(.horizontal, 6)
                            .frame(height: 22)
                            .background(Theme.hover, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
                        }
                    }
                    .padding(.horizontal, 8)
                    .padding(.bottom, 8)
                }

                if !this, model.voiceProducer(of: member) != nil {
                    MenuDivider()

                    VStack(alignment: .leading, spacing: 2) {
                        Text("Volume do usuário").labelMono()

                        HStack(spacing: 8) {
                            Slider(value: Binding(
                                get: { Double(model.voiceVolumes["user:\(member.user_id)"] ?? 1) },
                                set: { model.setVoiceVolume(member, Float($0)) }
                            ), in: 0 ... 2)
                                .tint(Theme.brand)

                            Text("\(Int((model.voiceVolumes["user:\(member.user_id)"] ?? 1) * 100))%")
                                .font(Theme.meta)
                                .foregroundStyle(Theme.inkDim)
                                .frame(width: 40, alignment: .trailing)
                        }
                    }
                    .padding(.horizontal, 8)
                    .help("Volume desta pessoa — só do seu lado")
                }

                if !this {
                    MenuDivider()

                    line("Mensagem para \(member.displayName)", text: $note, button: "Enviar") {
                        let body = note.trimmingCharacters(in: .whitespacesAndNewlines)

                        guard !body.isEmpty else {
                            return
                        }

                        let person = Person(id: member.user_id, name: member.name, avatar_url: member.avatar_url)

                        await model.openDirect(with: person)

                        if await model.sendDirect(body) {
                            model.memberMenu = nil
                            model.modal = nil
                        }
                    }
                }

                if !actions.any, !this {
                    Text("Nada que você possa mudar nesta pessoa.")
                        .font(Theme.meta)
                        .foregroundStyle(Theme.inkDim)
                        .padding(8)
                }

                if actions.nickname {
                    line("Apelido", text: $nickname, button: "OK") {
                        let wanted = nickname.trimmingCharacters(in: .whitespaces)

                        await model.updateMember(member, ["nickname": wanted.isEmpty ? NSNull() : wanted])
                    }
                }

                if actions.roles {
                    roles
                }

                if actions.mute || actions.deafen || actions.disconnect {
                    MenuDivider()

                    if actions.mute {
                        MenuRow(label: member.server_mute ? "Desmutar no servidor" : "Mutar no servidor") {
                            Task { await model.updateMember(member, ["server_mute": !member.server_mute]) }
                        }
                    }

                    if actions.deafen {
                        MenuRow(label: member.server_deaf ? "Voltar a ouvir no servidor" : "Ensurdecer no servidor") {
                            Task { await model.updateMember(member, ["server_deaf": !member.server_deaf]) }
                        }
                    }

                    if actions.disconnect, !destinations.isEmpty {
                        MenuRow(icon: moving ? .chevronDown : .chevronRight, label: "Mover para") {
                            withAnimation(.easeOut(duration: 0.15)) { moving.toggle() }
                        }

                        if moving {
                            ForEach(destinations) { channel in
                                MenuRow(icon: .speaker, label: channel.name) {
                                    Task { await model.moveToVoice(member, to: channel) }
                                }
                                .padding(.leading, 16)
                            }
                        }
                    }

                    if actions.disconnect {
                        MenuRow(label: "Desconectar") {
                            Task { await model.disconnectFromVoice(member) }
                        }
                    }
                }

                if actions.kick || actions.ban {
                    MenuDivider()

                    if actions.kick {
                        MenuRow(label: "Expulsar \(member.displayName)", danger: true) { model.kick(member) }
                    }

                    if actions.ban, !banning {
                        MenuRow(label: "Banir \(member.displayName)", danger: true) { banning = true }
                    }

                    if actions.ban, banning {
                        line("Motivo (opcional)", text: $reason, button: "Banir") {
                            await model.ban(member, reason: reason)
                        }
                    }
                }
            }
            .padding(.vertical, 6)
            .padding(.horizontal, 8)
            .frame(width: 280)
            .popoverPanel()
        }
        .onAppear { nickname = member.nickname ?? "" }
    }

    private var destinations: [Channel] {
        model.moveDestinations(for: member)
    }

    private var roles: some View {
        let assignable = Set(model.abilities.roles.filter(\.assignable).map(\.id))
        let offered = (model.tree?.roles ?? []).filter { assignable.contains($0.id) }

        return VStack(alignment: .leading, spacing: 4) {
            if !offered.isEmpty {
                MenuDivider()

                Text("Cargos").labelMono().padding(.horizontal, 8)
            }

            ForEach(offered) { role in
                Toggle(isOn: Binding(
                    get: { member.role_ids.contains(role.id) },
                    set: { wanted in
                        let ids = wanted ? Array(Set(member.role_ids + [role.id])) : member.role_ids.filter { $0 != role.id }

                        Task { await model.updateMember(member, ["role_ids": ids]) }
                    }
                )) {
                    Text(role.name)
                        .font(Theme.button)
                        .foregroundStyle(Theme.hex(role.color) ?? Theme.inkSoft)
                }
                .toggleStyle(.checkbox)
                .padding(.horizontal, 8)
            }
        }
    }

    private func line(_ placeholder: String, text: Binding<String>, button: String, _ action: @escaping @MainActor () async -> Void) -> some View {
        HStack(spacing: 6) {
            TextField(placeholder, text: text)
                .textFieldStyle(.plain)
                .font(Theme.button)
                .foregroundStyle(Theme.ink)
                .padding(.horizontal, 10)
                .frame(height: Theme.Size.row)
                .background(Theme.surfaceInput, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
                .onSubmit { Task { await action() } }

            if button == "Banir" {
                Button(button) {
                    Task { await action() }
                }
                .buttonStyle(DangerButton())
            } else {
                Button(button) {
                    Task { await action() }
                }
                .buttonStyle(PrimaryButton(wide: false))
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
    }
}
