import SwiftUI

/// As configurações da conta, no molde do Discord: um painel que toma quase a janela toda, as
/// seções agrupadas à esquerda, a seção aberta à direita e o "X / ESC" no canto. É o
/// `modals/UserSettingsModal.tsx` do React repartido em abas.
struct UserSettingsModal: View {
    @EnvironmentObject private var model: AppModel

    @State private var tab = Tab.account

    private enum Tab: String {
        case account = "Minha conta"
        case voice = "Voz e vídeo"
        case keys = "Teclas"
        case notices = "Notificações"

        var icon: IconName {
            switch self {
            case .account: .users
            case .voice: .mic
            case .keys: .sliders
            case .notices: .chat
            }
        }
    }

    private static let groups: [(title: String, tabs: [Tab])] = [
        ("Configurações de usuário", [.account]),
        ("Configurações do app", [.voice, .keys, .notices]),
    ]

    var body: some View {
        ZStack {
            Color(hex: 0x06050A).opacity(0.74)
                .ignoresSafeArea()
                .onTapGesture(perform: close)

            HStack(spacing: 0) {
                sidebar

                Rectangle().fill(Theme.line).frame(width: 1)

                ScrollView {
                    VStack(alignment: .leading, spacing: 20) {
                        Text(tab.rawValue)
                            .font(Theme.sans(20, .semibold))
                            .tracking(-0.3)
                            .foregroundStyle(Theme.ink)

                        switch tab {
                        case .account: AccountTab()
                        case .voice: VoiceTab()
                        case .keys: KeysTab()
                        case .notices: NoticesTab()
                        }
                    }
                    .frame(maxWidth: 740, alignment: .topLeading)
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                    .padding(.horizontal, 40)
                    .padding(.vertical, 36)
                }

                escape
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .glassPanel()
            .padding(24)
        }
        .onExitCommand(perform: close)
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 4) {
            ForEach(Self.groups, id: \.title) { group in
                Text(group.title).labelMono()
                    .padding(.horizontal, 10)
                    .padding(.top, 14)
                    .padding(.bottom, 4)

                ForEach(group.tabs, id: \.self) { item in
                    row(item.rawValue, icon: item.icon, selected: tab == item) {
                        tab = item
                    }
                }
            }

            Rectangle().fill(Theme.line).frame(height: 1)
                .padding(.vertical, 10)

            row("Logs", icon: .sliders, selected: false) {
                model.modal = .logs
            }

            row("Sair da conta", icon: .logout, selected: false, tint: Theme.danger) {
                close()

                Task { await model.signOut() }
            }

            Spacer(minLength: 0)

            Text(model.user?.name ?? "")
                .font(Theme.sans(11.5))
                .foregroundStyle(Theme.inkDim)
                .lineLimit(1)
                .padding(.horizontal, 10)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 22)
        .frame(width: 252, alignment: .topLeading)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Color.black.opacity(0.18))
    }

    private var escape: some View {
        VStack(spacing: 6) {
            Button(action: close) {
                Icon(name: .close, size: 14)
                    .foregroundStyle(Theme.inkSoft)
                    .frame(width: 34, height: 34)
                    .overlay(Circle().strokeBorder(Theme.inkDim, lineWidth: 1.5))
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Fechar as configurações")

            Text("ESC").labelMono()
        }
        .padding(.top, 36)
        .padding(.trailing, 28)
        .frame(maxHeight: .infinity, alignment: .top)
    }

    private func row(_ title: String, icon: IconName, selected: Bool, tint: Color? = nil, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Icon(name: icon, size: 14)

                Text(title)
                    .font(Theme.sans(13))
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .foregroundStyle(tint ?? (selected ? Theme.inkStrong : Theme.inkIcon))
            .rowItem(selected: selected)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func close() {
        model.modal = nil
    }
}

private struct AccountTab: View {
    @EnvironmentObject private var model: AppModel

    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Foto de perfil").labelMono()

            HStack(spacing: 12) {
                Button {
                    busy = true

                    Task {
                        await model.changeAvatar()
                        busy = false
                    }
                } label: {
                    Avatar(name: model.user?.name, url: model.user?.avatar_url, size: 56, mine: true)
                        .overlay {
                            if busy {
                                ProgressView().controlSize(.small)
                            }
                        }
                }
                .buttonStyle(.plain)
                .disabled(busy)
                .help("Trocar a sua foto")

                VStack(alignment: .leading, spacing: 4) {
                    Text("PNG, JPG ou WebP de até 2 MB.")
                        .font(Theme.sans(12.5))
                        .foregroundStyle(Theme.inkSoft)

                    if model.user?.avatar_uploaded == true {
                        Button("Remover a foto") {
                            Task { await model.removeAvatar() }
                        }
                        .buttonStyle(.plain)
                        .font(Theme.sans(11.5))
                        .foregroundStyle(Theme.inkDim)
                    }
                }
            }

            Text("Conta").labelMono()
                .padding(.top, 16)

            line("Apelido", model.user?.name ?? "")
            line("E-mail", model.user?.email ?? "")
        }
    }

    private func line(_ label: String, _ value: String) -> some View {
        HStack {
            Text(label)
                .font(Theme.sans(12.5))
                .foregroundStyle(Theme.inkSoft)

            Spacer(minLength: 0)

            Text(value)
                .font(Theme.sans(13))
                .foregroundStyle(Theme.ink)
                .textSelection(.enabled)
        }
        .rowItem()
    }
}

private struct VoiceTab: View {
    @EnvironmentObject private var model: AppModel

    private static let modes = [
        ("voice", "Detecção de voz", "abre o microfone quando você fala"),
        ("ptt", "Apertar para falar", "só abre enquanto a tecla estiver pressionada"),
        ("open", "Sempre aberto", "o microfone fica ligado o tempo todo"),
    ]

    var body: some View {
        let preferences = model.voicePreferences

        VStack(alignment: .leading, spacing: 8) {
            Text("Microfone").labelMono()

            devices(model.microphones, chosen: preferences.microphone, fallback: "Microfone padrão") { name in
                model.setVoice { $0.microphone = name }
            }

            meter(preferences)

            HStack(spacing: 8) {
                chip("Supressão de ruído", on: preferences.noiseSuppression) { model.setVoice { $0.noiseSuppression.toggle() } }
                chip("Silenciar ao entrar", on: preferences.muteOnJoin) { model.setVoice { $0.muteOnJoin.toggle() } }
            }
            .padding(.top, 6)

            Text("Como o microfone abre").labelMono()
                .padding(.top, 16)

            ForEach(Self.modes, id: \.0) { mode, label, hint in
                Button {
                    model.setVoice { $0.inputMode = mode }
                } label: {
                    HStack(spacing: 10) {
                        Circle()
                            .fill(preferences.inputMode == mode ? Theme.brand : Color.white.opacity(0.2))
                            .frame(width: 9, height: 9)

                        VStack(alignment: .leading, spacing: 1) {
                            Text(label).font(Theme.sans(12.5)).foregroundStyle(Theme.ink)

                            Text(hint).font(Theme.sans(11)).foregroundStyle(Theme.inkDim)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .rowItem(selected: preferences.inputMode == mode)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }

            if preferences.inputMode == "voice" {
                Text("Sensibilidade — abre acima de \(preferences.sensitivity)%")
                    .font(Theme.sans(12))
                    .foregroundStyle(Theme.inkSoft)
                    .padding(.top, 6)

                Slider(value: Binding(
                    get: { Double(preferences.sensitivity) },
                    set: { chosen in model.setVoice { $0.sensitivity = Int(chosen) } }
                ), in: 5 ... 90, step: 1)
                    .tint(Theme.brand)
            }

            if preferences.inputMode == "ptt", preferences.talk.isEmpty {
                Text("Escolha a tecla de apertar para falar na aba Teclas — sem ela o microfone fica sempre aberto.")
                    .font(Theme.sans(12))
                    .foregroundStyle(Theme.danger)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Text("Saída de áudio").labelMono()
                .padding(.top, 16)

            devices(model.speakers, chosen: preferences.speaker, fallback: "Saída padrão") { name in
                model.setVoice { $0.speaker = name }
            }

            Text("Vale para a voz das pessoas, o áudio das telas e os sons do app.")
                .font(Theme.sans(11.5))
                .foregroundStyle(Theme.inkDim)
        }
        .onAppear { model.refreshDevices() }
    }

    /// A barra de entrada com o traço vermelho da sensibilidade. Só anda dentro de uma voz: é
    /// lá que o microfone está aberto.
    private func meter(_ preferences: VoicePreferences) -> some View {
        HStack(spacing: 10) {
            Text("Entrada")
                .font(Theme.sans(12.5))
                .foregroundStyle(Theme.inkSoft)

            GeometryReader { bar in
                ZStack(alignment: .leading) {
                    Capsule().fill(Color.white.opacity(0.08))

                    Capsule()
                        .fill(model.micPercent >= preferences.sensitivity
                            ? AnyShapeStyle(LinearGradient(colors: [Theme.brand, Theme.online], startPoint: .leading, endPoint: .trailing))
                            : AnyShapeStyle(Color.white.opacity(0.25)))
                        .frame(width: bar.size.width * CGFloat(model.mine.mic ? model.micPercent : 0) / 100)

                    if preferences.inputMode == "voice" {
                        Rectangle()
                            .fill(Theme.danger)
                            .frame(width: 1)
                            .offset(x: bar.size.width * CGFloat(preferences.sensitivity) / 100)
                    }
                }
            }
            .frame(height: 7)
        }
        .padding(.top, 4)
        .help(model.mine.mic ? "" : "O nível aparece enquanto você está numa voz")
    }

    private func devices(_ found: [AudioDevice], chosen: String, fallback: String, pick: @escaping (String) -> Void) -> some View {
        Picker("", selection: Binding(get: { chosen }, set: pick)) {
            Text(fallback).tag("")

            ForEach(found) { device in
                Text(device.name).tag(device.name)
            }
        }
        .labelsHidden()
    }

    private func chip(_ label: String, on: Bool, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 6) {
                if on {
                    Icon(name: .check, size: 12)
                }

                Text(label)
            }
            .font(Theme.sans(12.5))
            .foregroundStyle(on ? Theme.inkStrong : Theme.inkIcon)
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            .background(on ? Theme.brand.opacity(0.2) : Theme.row, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(on ? Theme.brand.opacity(0.5) : Theme.lineStrong, lineWidth: 1))
        }
        .buttonStyle(.plain)
    }
}

private struct KeysTab: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        let preferences = model.voicePreferences

        VStack(alignment: .leading, spacing: 8) {
            row("Mutar o microfone", preferences.mute, bare: false) { key in model.setVoice { $0.mute = key } }
            row("Mutar o áudio de todos", preferences.deafen, bare: false) { key in model.setVoice { $0.deafen = key } }
            row("Apertar para falar", preferences.talk, bare: true) { key in model.setVoice { $0.talk = key } }

            Text("As teclas valem com o jogo na frente, e nunca tiram a tecla do jogo. Esc dentro do campo apaga a tecla.")
                .font(Theme.sans(11.5))
                .foregroundStyle(Theme.inkDim)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 4)

            if preferences.repeatsAKey {
                Text("Duas ações com a mesma tecla: o sistema só aceita a primeira.")
                    .font(Theme.sans(11.5))
                    .foregroundStyle(Theme.danger)
            }
        }
    }

    private func row(_ label: String, _ value: String, bare: Bool, change: @escaping (String) -> Void) -> some View {
        HStack(spacing: 8) {
            Text(label)
                .font(Theme.sans(12.5))
                .foregroundStyle(Theme.inkSoft)
                .frame(maxWidth: .infinity, alignment: .leading)

            KeybindField(value: value, bare: bare, change: change)
        }
    }
}

private struct NoticesTab: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Toggle("Sons do app (alguém entrou, alguém saiu, mensagem nova)", isOn: Binding(
                get: { model.noticePreferences.sounds },
                set: { wanted in model.setNotices { $0.sounds = wanted } }
            ))

            Toggle("Avisar de mensagem direta quando a conversa não está aberta", isOn: Binding(
                get: { model.noticePreferences.directMessages },
                set: { wanted in model.setNotices { $0.directMessages = wanted } }
            ))
        }
        .toggleStyle(.checkbox)
        .font(Theme.sans(13))
        .foregroundStyle(Theme.inkIcon)
        .tint(Theme.brand)
    }
}

/// `common/KeybindField.tsx`: clicar, apertar o atalho, e ele fica gravado. Tecla solta só
/// vale para "apertar para falar" (`bare`): nas outras ações ela roubaria a digitação.
struct KeybindField: View {
    @EnvironmentObject private var model: AppModel

    var value: String
    var bare: Bool
    var change: (String) -> Void

    @State private var capturing = false
    @State private var refused = ""
    @State private var monitor: Any?

    private static let symbols = ["CmdOrCtrl": "⌘", "Super": "⌘", "Control": "⌃", "Alt": "⌥", "Shift": "⇧"]

    var body: some View {
        VStack(alignment: .trailing, spacing: 4) {
            Button {
                capturing ? stop() : start()
            } label: {
                Text(capturing ? "aperte a tecla…" : label)
                    .font(Theme.mono(12))
                    .foregroundStyle(capturing ? Theme.lilac2 : value.isEmpty ? Theme.inkDim : Theme.ink)
                    .frame(minWidth: 150)
                    .padding(.vertical, 8)
                    .padding(.horizontal, 12)
                    .background(Theme.fieldFill, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(capturing ? Theme.brand.opacity(0.6) : Theme.fieldLine, lineWidth: 1))
            }
            .buttonStyle(.plain)

            if !refused.isEmpty {
                Text(refused)
                    .font(Theme.sans(11))
                    .foregroundStyle(Theme.danger)
            }
        }
        .onDisappear(perform: stop)
    }

    private var label: String {
        guard !value.isEmpty else {
            return "sem tecla"
        }

        return value.split(separator: "+").map { part in
            Self.symbols[String(part)] ?? part.replacingOccurrences(of: "Key", with: "").replacingOccurrences(of: "Digit", with: "")
        }.joined(separator: " ")
    }

    private func start() {
        capturing = true
        refused = ""

        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            Task { @MainActor in await pressed(event) }

            return nil
        }
    }

    private func stop() {
        capturing = false

        if let monitor {
            NSEvent.removeMonitor(monitor)
        }

        monitor = nil
    }

    private func pressed(_ event: NSEvent) async {
        if event.keyCode == 53 {
            change("")
            stop()

            return
        }

        guard let accelerator = await model.accelerator(from: event) else {
            refused = "essa tecla não dá para usar"

            return
        }

        guard bare || accelerator.contains("+") else {
            refused = "junte ⌘, ⌥, ⌃ ou ⇧ — tecla solta roubaria a digitação do sistema inteiro"

            return
        }

        change(accelerator)
        stop()
    }
}

/// `modals/NicknameModal.tsx`: a conta nova nasce com um apelido tirado do e-mail, e antes de
/// qualquer outra coisa a pessoa confirma ou troca. Não fecha sem decidir.
struct NicknameModal: View {
    @EnvironmentObject private var model: AppModel

    @State private var name = ""
    @FocusState private var typing: Bool

    var body: some View {
        ModalFrame(
            title: "Escolha seu apelido",
            subtitle: "É como as pessoas te acham. Sem espaço, e ninguém mais pode usar o mesmo.",
            width: 440,
            dismissable: false,
            onClose: {}
        ) {
            TextField("apelido", text: $name)
                .field(focused: typing, invalid: !model.nicknameError.isEmpty)
                .focused($typing)
                .fieldError(model.nicknameError)
                .onSubmit(confirm)
                .onChange(of: name) {
                    name = String(name.filter { !$0.isWhitespace }.prefix(32))
                    model.nicknameError = ""
                }
                .help("Letras, números, ponto e _ — sem espaço")
        } footer: {
            Button("Sair da conta") {
                Task { await model.signOut() }
            }
            .buttonStyle(.plain)
            .font(Theme.sans(12.5))
            .foregroundStyle(Theme.inkDim)

            Spacer(minLength: 0)

            Button(action: confirm) {
                HStack(spacing: 8) {
                    if model.nicknameBusy {
                        ProgressView().controlSize(.small)
                    }

                    Text("Confirmar")
                }
            }
            .buttonStyle(PrimaryButton(wide: false, font: Theme.sans(13, .semibold)))
            .disabled(model.nicknameBusy)
        }
        .onAppear {
            name = model.user?.name ?? ""
            typing = true
        }
    }

    private func confirm() {
        Task { await model.confirmNickname(name) }
    }
}

/// `layout/LogsModal.tsx`: o fim do registro do núcleo, para anexar quando algo não funciona.
struct LogsModal: View {
    @EnvironmentObject private var model: AppModel

    @State private var lines: [String] = []
    @State private var path = ""

    var body: some View {
        ModalFrame(title: "Logs", subtitle: path, width: 760, onClose: { model.modal = nil }) {
            Text(lines.isEmpty ? "Nada registrado ainda." : lines.joined(separator: "\n"))
                .font(Theme.mono(11))
                .foregroundStyle(Theme.inkIcon)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        } footer: {
            Button("Copiar tudo") {
                model.copy(lines.joined(separator: "\n"), "Logs copiados")
            }
            .buttonStyle(GhostButton())

            Button("Mostrar o arquivo") {
                NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
            }
            .buttonStyle(GhostButton())
            .disabled(path.isEmpty)

            Spacer(minLength: 0)

            Button("Fechar") {
                model.modal = nil
            }
            .buttonStyle(PrimaryButton(wide: false, font: Theme.sans(13, .semibold)))
        }
        .task {
            let tail = await model.ask("logs", ["lines": 400])

            lines = tail["lines"] as? [String] ?? []
            path = tail["path"] as? String ?? ""
        }
    }
}
