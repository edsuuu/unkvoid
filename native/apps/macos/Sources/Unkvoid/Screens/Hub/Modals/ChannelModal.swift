import SwiftUI

/// `modals/ChannelModal.tsx`: criar ou editar um canal e, para quem gerencia cargos, a grade
/// de "ocultar canal" — cada célula alterna entre herdar, permitir e negar, e grava na hora.
struct ChannelModal: View {
    @EnvironmentObject private var model: AppModel

    let editor: ChannelEditor

    @State private var name = ""
    @State private var kind = "text"
    @State private var topic = ""
    @State private var limit = ""
    @FocusState private var naming: Bool

    var body: some View {
        ModalFrame(title: title, subtitle: nil, width: canHide ? 640 : 480, onClose: { model.channelEditor = nil }) {
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 8) {
                    TextField("Nome", text: $name)
                        .field(focused: naming)
                        .focused($naming)
                        .onSubmit(save)
                        .onChange(of: name) { name = String(name.prefix(40)) }

                    Picker("Tipo", selection: $kind) {
                        Text("Texto").tag("text")
                        Text("Voz").tag("voice")
                    }
                    .labelsHidden()
                    .fixedSize()
                    .disabled(editor.channel != nil)
                }

                TextField("Tópico (opcional)", text: $topic)
                    .field()
                    .onChange(of: topic) { topic = String(topic.prefix(200)) }

                if kind == "voice" {
                    TextField("Limite de pessoas", text: $limit)
                        .field()
                        .frame(width: 190)
                        .help("Vazio = sem limite")
                }

                if canHide, let channel {
                    OverwriteGrid(channel: channel)
                        .padding(.top, 12)
                }
            }
        } footer: {
            if let channel {
                Button("Apagar canal") {
                    model.deleteChannel(channel)
                }
                .buttonStyle(DangerButton())
            }

            Spacer(minLength: 0)

            Button("Cancelar") {
                model.channelEditor = nil
            }
            .buttonStyle(GhostButton())

            Button("Salvar", action: save)
                .buttonStyle(PrimaryButton(wide: false, font: Theme.sans(13, .semibold)))
        }
        .onAppear {
            name = editor.channel?.name ?? ""
            kind = editor.channel?.type ?? editor.kind
            topic = editor.channel?.topic ?? ""
            limit = editor.channel?.user_limit.map(String.init) ?? ""
            naming = true
        }
    }

    /// O canal como está na árvore agora: a grade grava na hora, e a árvore volta atualizada.
    private var channel: Channel? {
        editor.channel.flatMap { edited in model.tree?.channels.first { $0.id == edited.id } }
    }

    private var canHide: Bool {
        editor.channel != nil && model.abilities.allows("manageRoles")
    }

    private var title: String {
        if let channel = editor.channel {
            return "Canal: \(channel.name)"
        }

        return kind == "voice" ? "Novo canal de voz" : "Novo canal de texto"
    }

    private func save() {
        Task {
            if await model.saveChannel(editor.channel, name: name, kind: kind, topic: topic, limit: limit) {
                model.channelEditor = nil
            }
        }
    }
}

private struct OverwriteGrid: View {
    @EnvironmentObject private var model: AppModel

    let channel: Channel

    /// Quem foi escolhido no menu mas ainda não tem bit nenhum gravado.
    @State private var added: [Target] = []

    private struct Target: Identifiable, Hashable {
        let type: String
        let target: Int
        let name: String
        let color: String?
        let everyone: Bool

        var id: String {
            "\(type):\(target)"
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Ocultar canal").labelMono()

                Spacer(minLength: 0)

                Menu("Adicionar cargo ou membro…") {
                    ForEach(available) { target in
                        Button("\(target.type == "role" ? "cargo" : "membro"): \(target.name)") {
                            added.append(target)
                        }
                    }
                }
                .fixedSize()
                .disabled(available.isEmpty)
            }

            Text("Clique numa célula para alternar: — herda, ✓ permite, ✕ nega. Grava na hora.")
                .font(Theme.sans(12))
                .foregroundStyle(Theme.inkDim)

            Grid(horizontalSpacing: 4, verticalSpacing: 4) {
                GridRow {
                    Color.clear.frame(height: 1)

                    ForEach(AppModel.overwritable, id: \.bit) { flag in
                        Text(flag.label).labelMono()
                            .frame(width: 42)
                    }
                }

                ForEach(shown) { target in
                    GridRow {
                        Text(target.name)
                            .font(Theme.sans(13))
                            .foregroundStyle(Theme.hex(target.color) ?? Theme.inkBody)
                            .lineLimit(1)
                            .frame(maxWidth: .infinity, alignment: .leading)

                        ForEach(AppModel.overwritable, id: \.bit) { flag in
                            cell(target, flag.bit)
                        }
                    }
                }
            }
        }
    }

    private var targets: [Target] {
        let tree = model.tree

        return (tree?.roles ?? []).map { Target(type: "role", target: $0.id, name: $0.name, color: $0.color, everyone: $0.is_everyone == true) }
            + (tree?.members ?? []).map { Target(type: "member", target: $0.user_id, name: $0.displayName, color: nil, everyone: false) }
    }

    private var shown: [Target] {
        targets.filter { $0.everyone || written($0) != nil || added.contains($0) }
    }

    private var available: [Target] {
        targets.filter { !shown.contains($0) }
    }

    private func written(_ target: Target) -> Overwrite? {
        channel.overwrites?.first { $0.target_type == target.type && $0.target_id == target.target }
    }

    private func cell(_ target: Target, _ bit: Int) -> some View {
        let now = written(target)
        let (allow, deny) = (now?.allow ?? 0, now?.deny ?? 0)
        let state = allow & bit != 0 ? "allow" : deny & bit != 0 ? "deny" : "inherit"

        return Button {
            let next = state == "inherit" ? "allow" : state == "allow" ? "deny" : "inherit"

            Task {
                _ = await model.putOverwrite(
                    channel,
                    type: target.type,
                    id: target.target,
                    allow: next == "allow" ? allow | bit : allow & ~bit,
                    deny: next == "deny" ? deny | bit : deny & ~bit
                )
            }
        } label: {
            Text(state == "allow" ? "✓" : state == "deny" ? "✕" : "—")
                .font(Theme.sans(12))
                .foregroundStyle(state == "allow" ? Theme.back : state == "deny" ? Theme.inkStrong : Theme.inkDim)
                .frame(width: 42, height: 22)
                .background(state == "allow" ? Theme.online : state == "deny" ? Theme.danger : Theme.row, in: RoundedRectangle(cornerRadius: 5, style: .continuous))
        }
        .buttonStyle(.plain)
    }
}

/// `modals/RoleModal.tsx`: nome, cor e as dezoito permissões de um cargo.
struct RoleModal: View {
    @EnvironmentObject private var model: AppModel

    let editor: RoleEditor

    @State private var name = ""
    @State private var color = Color(hex: 0x8A7CF5)
    @State private var permissions = 0

    var body: some View {
        let everyone = editor.role?.is_everyone == true

        ModalFrame(title: editor.role.map { "Cargo: \($0.name)" } ?? "Novo cargo", subtitle: nil, width: 520, onClose: { model.roleEditor = nil }) {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    TextField("Nome do cargo", text: $name)
                        .field()
                        .disabled(everyone)
                        .onChange(of: name) { name = String(name.prefix(40)) }

                    ColorPicker("Cor", selection: $color, supportsOpacity: false)
                        .labelsHidden()
                        .disabled(everyone)
                        .help("Cor")
                }

                Text("Permissões").labelMono()
                    .padding(.top, 12)

                LazyVGrid(columns: [GridItem(.flexible(), alignment: .leading), GridItem(.flexible(), alignment: .leading)], spacing: 8) {
                    ForEach(AppModel.permissionLabels, id: \.bit) { flag in
                        Toggle(flag.label, isOn: Binding(
                            get: { permissions & flag.bit != 0 },
                            set: { permissions = $0 ? permissions | flag.bit : permissions & ~flag.bit }
                        ))
                        .toggleStyle(.checkbox)
                        .font(Theme.sans(13))
                        .foregroundStyle(Theme.inkIcon)
                    }
                }
            }
        } footer: {
            if let role = editor.role, !everyone {
                Button("Apagar") {
                    model.deleteRole(role)
                }
                .buttonStyle(DangerButton())
            }

            Spacer(minLength: 0)

            Button("Cancelar") {
                model.roleEditor = nil
            }
            .buttonStyle(GhostButton())

            Button("Salvar") {
                Task {
                    if await model.saveRole(editor.role, name: name, color: color.hexText, permissions: permissions) {
                        model.roleEditor = nil
                    }
                }
            }
            .buttonStyle(PrimaryButton(wide: false, font: Theme.sans(13, .semibold)))
        }
        .onAppear {
            name = editor.role?.name ?? ""
            color = Theme.hex(editor.role?.color) ?? Color(hex: 0x8A7CF5)
            permissions = editor.role?.permissions ?? 0
        }
    }
}

/// `layout/ConfirmDialog.tsx`: a pergunta antes do que não tem volta.
struct ConfirmDialog: View {
    @EnvironmentObject private var model: AppModel

    let confirmation: Confirmation

    var body: some View {
        ModalFrame(title: "Tem certeza?", subtitle: nil, width: 420, onClose: { model.confirmation = nil }) {
            Text(confirmation.question)
                .font(Theme.sans(13.5))
                .foregroundStyle(Theme.inkBody)
                .fixedSize(horizontal: false, vertical: true)
        } footer: {
            Spacer(minLength: 0)

            Button("Cancelar") {
                model.confirmation = nil
            }
            .buttonStyle(GhostButton())
            .keyboardShortcut(.cancelAction)

            Button(confirmation.action) {
                model.confirmation = nil

                Task { await confirmation.confirmed() }
            }
            .buttonStyle(DangerButton())
        }
    }
}
