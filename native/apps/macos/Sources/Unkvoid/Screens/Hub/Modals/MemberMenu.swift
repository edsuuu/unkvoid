import SwiftUI

/// `modals/MemberMenu.tsx`: quem é a pessoa, mandar mensagem, e o que dá para fazer com ela
/// aqui — apelido, cargos, mutar, ensurdecer, tirar da voz, expulsar e banir. O que aparece
/// é o que o núcleo calculou; quem autoriza é o Laravel.
struct MemberMenu: View {
    @EnvironmentObject private var model: AppModel

    let member: Member

    @State private var nickname = ""
    @State private var note = ""
    @State private var banning = false
    @State private var reason = ""

    var body: some View {
        let actions = model.abilities.actions(on: member)
        let this = member.user_id == model.user?.id
        let badges = (model.tree?.roles ?? []).filter { $0.is_everyone != true && member.role_ids.contains($0.id) }

        ZStack {
            Color.black.opacity(0.001)
                .ignoresSafeArea()
                .onTapGesture { model.memberMenu = nil }

            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 10) {
                    Avatar(name: member.displayName, url: member.avatar_url, size: 38, mine: this)

                    VStack(alignment: .leading, spacing: 1) {
                        Text(member.displayName)
                            .font(Theme.sans(13.5, .semibold))
                            .foregroundStyle(Theme.ink)
                            .lineLimit(1)

                        if member.nickname != nil {
                            Text(member.name)
                                .font(Theme.sans(11))
                                .foregroundStyle(Theme.inkDim)
                        }

                        if member.is_owner {
                            Text("dono do servidor").labelMono()
                        }
                    }
                }

                if !badges.isEmpty {
                    FlowRow(spacing: 4) {
                        ForEach(badges) { role in
                            Text(role.name)
                                .font(Theme.sans(10.5))
                                .foregroundStyle(Theme.hex(role.color) ?? Theme.inkIcon)
                                .padding(.horizontal, 8)
                                .padding(.vertical, 2)
                                .overlay(Capsule().strokeBorder(Theme.hex(role.color) ?? Theme.lineStrong, lineWidth: 1))
                        }
                    }
                }

                if !this, model.voiceProducer(of: member) != nil {
                    Divider().overlay(Theme.line)

                    HStack(spacing: 8) {
                        Icon(name: .speaker, size: 13).foregroundStyle(Theme.inkIcon)

                        Slider(value: Binding(
                            get: { Double(model.voiceVolumes["user:\(member.user_id)"] ?? 1) },
                            set: { model.setVoiceVolume(member, Float($0)) }
                        ), in: 0 ... 1)
                            .tint(Theme.brand)

                        Text("\(Int((model.voiceVolumes["user:\(member.user_id)"] ?? 1) * 100))%")
                            .font(Theme.mono(10.5))
                            .foregroundStyle(Theme.inkDim)
                            .frame(width: 36, alignment: .trailing)
                    }
                    .help("Volume desta pessoa — só do seu lado")
                }

                if !this {
                    Divider().overlay(Theme.line)

                    line("Mensagem para \(member.name)", text: $note, button: "Enviar") {
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
                        .font(Theme.sans(12))
                        .foregroundStyle(Theme.inkDim)
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

                if actions.mute || actions.deafen || actions.disconnect || actions.kick || actions.ban {
                    Divider().overlay(Theme.line)

                    if actions.mute {
                        item(member.server_mute ? "Desmutar no servidor" : "Mutar no servidor") {
                            await model.updateMember(member, ["server_mute": !member.server_mute])
                        }
                    }

                    if actions.deafen {
                        item(member.server_deaf ? "Devolver o áudio" : "Ensurdecer no servidor") {
                            await model.updateMember(member, ["server_deaf": !member.server_deaf])
                        }
                    }

                    if actions.disconnect {
                        item("Desconectar da sala de voz") { await model.disconnectFromVoice(member) }
                    }

                    if actions.kick {
                        item("Expulsar do servidor", danger: true) { model.kick(member) }
                    }

                    if actions.ban, !banning {
                        item("Banir do servidor…", danger: true) { banning = true }
                    }

                    if actions.ban, banning {
                        line("Motivo (opcional)", text: $reason, button: "Banir") {
                            await model.ban(member, reason: reason)
                        }
                    }
                }
            }
            .padding(12)
            .frame(width: 272)
            .popoverPanel()
        }
        .onAppear { nickname = member.nickname ?? "" }
    }

    private var roles: some View {
        let assignable = Set(model.abilities.roles.filter(\.assignable).map(\.id))
        let offered = (model.tree?.roles ?? []).filter { assignable.contains($0.id) }

        return VStack(alignment: .leading, spacing: 4) {
            if !offered.isEmpty {
                Divider().overlay(Theme.line)

                Text("Cargos").labelMono()
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
                        .font(Theme.sans(12.5))
                        .foregroundStyle(Theme.hex(role.color) ?? Theme.inkIcon)
                }
                .toggleStyle(.checkbox)
            }
        }
    }

    private func line(_ placeholder: String, text: Binding<String>, button: String, _ action: @escaping @MainActor () async -> Void) -> some View {
        HStack(spacing: 6) {
            TextField(placeholder, text: text)
                .textFieldStyle(.plain)
                .font(Theme.sans(12.5))
                .padding(.horizontal, 10)
                .padding(.vertical, 7)
                .background(Theme.fieldFill, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 9, style: .continuous).strokeBorder(Theme.fieldLine, lineWidth: 1))
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
                .buttonStyle(PrimaryButton(wide: false, font: Theme.sans(12, .semibold)))
            }
        }
    }

    private func item(_ label: String, danger: Bool = false, _ action: @escaping @MainActor () async -> Void) -> some View {
        Button {
            Task { await action() }
        } label: {
            Text(label)
                .font(Theme.sans(12.5))
                .foregroundStyle(danger ? Theme.danger : Theme.inkIcon)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.vertical, 7)
                .padding(.horizontal, 10)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
