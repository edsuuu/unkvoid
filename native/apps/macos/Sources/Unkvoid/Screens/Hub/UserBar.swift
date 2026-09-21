import SwiftUI

/// A barra de baixo, no lugar do `ui/components/hub/VoicePanel.tsx`: quem você é, o
/// microfone, o áudio e o menu da conta.
///
/// Os dois botões nascem apagados porque no React eles também nascem: fora de um canal de
/// voz o `VoicePanel` os desenha `disabled`. A setinha ao lado de cada um é o que o Mac
/// ganha a mais — ela abre a lista de aparelhos que o CoreAudio enxerga, no molde do
/// app de chamada, sem depender de estar numa voz para escolher.
struct UserBar: View {
    @EnvironmentObject private var model: AppModel
    @State private var open: Picker?

    private enum Picker {
        case microphone
        case speaker
        case menu
    }

    /// O painel é alinhado pela base da barra; subi-lo a altura dela mais um respiro é o
    /// que o põe **acima** da barra, e não por cima dela. 12 + 30 + 12 de padding e avatar.
    private static let barHeight: CGFloat = 62

    var body: some View {
        HStack(spacing: 10) {
            Avatar(name: model.user?.name ?? "?", url: model.user?.avatar_url, size: 30, mine: true)

            VStack(alignment: .leading, spacing: 1) {
                Text(model.user?.name ?? "Conta conectada")
                    .font(Theme.sans(12.5, .semibold))
                    .foregroundStyle(Theme.ink)
                    .lineLimit(1)

                Text("Online")
                    .font(Theme.sans(10.5))
                    .foregroundStyle(Theme.inkDim)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            device(.microphone, icon: .mic, hint: "Escolher o microfone")
            device(.speaker, icon: .headphones, hint: "Escolher a saída de áudio")

            SmallButton(icon: .gear, active: open == .menu, hint: "Menu") {
                open = open == .menu ? nil : .menu
            }
        }
        .padding(12)
        .glass()
        .overlay(alignment: .bottomTrailing) { popover }
    }

    /// O par "ligar/desligar" e a setinha, colados como nos apps de chamada: o botão à esquerda faz
    /// a ação, a setinha à direita abre a escolha do aparelho.
    private func device(_ picker: Picker, icon: IconName, hint: String) -> some View {
        HStack(spacing: 0) {
            SmallButton(icon: icon, active: false, hint: hint) {}
                .disabled(true)

            Button {
                model.refreshDevices()
                open = open == picker ? nil : picker
            } label: {
                Icon(name: .chevronDown, size: 11)
                    .foregroundStyle(open == picker ? Theme.inkStrong : Theme.inkDim)
                    .frame(width: 14, height: 26)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help(hint)
        }
    }

    @ViewBuilder
    private var popover: some View {
        switch open {
        case .microphone:
            DeviceList(
                title: "Microfone",
                devices: model.microphones,
                chosen: model.microphone,
                fallback: "Microfone padrão"
            ) { chosen in
                model.microphone = chosen
                open = nil
            }
            .offset(y: -Self.barHeight)
        case .speaker:
            DeviceList(
                title: "Saída de áudio",
                devices: model.speakers,
                chosen: model.speaker,
                fallback: "Saída padrão"
            ) { chosen in
                model.speaker = chosen
                open = nil
            }
            .offset(y: -Self.barHeight)
        case .menu:
            PopoverBox(width: 224) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(model.user?.name ?? "Sem conta")
                        .font(Theme.sans(13, .semibold))
                        .foregroundStyle(Theme.ink)
                        .lineLimit(1)

                    Text(model.user == nil ? "usando sem login" : "conta conectada").labelMono()
                }
                .padding(.horizontal, 10)
                .padding(.bottom, 8)
                .frame(maxWidth: .infinity, alignment: .leading)

                Divider().overlay(Theme.line)

                MenuRow(icon: .gear, label: "Configurações da conta") {
                    open = nil
                    model.modal = .account
                }

                MenuRow(icon: .logout, label: "Sair da conta", tint: Theme.periwinkle) {
                    open = nil
                    Task { await model.signOut() }
                }
            }
            .offset(y: -Self.barHeight)
        case nil:
            EmptyView()
        }
    }
}

/// O botão de 26 da barra: menor que o `.btn-icon`, sem moldura, como no React.
private struct SmallButton: View {
    var icon: IconName
    var active: Bool
    var hint: String
    var action: () -> Void

    @Environment(\.isEnabled) private var enabled
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            Icon(name: icon, size: 15)
                .foregroundStyle(active ? Theme.inkStrong : Theme.inkIcon)
                .frame(width: 26, height: 26)
                .background(active || hovering ? Theme.row : .clear, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        }
        .buttonStyle(.plain)
        .opacity(enabled ? 1 : 0.4)
        .onHover { hovering = $0 && enabled }
        .help(hint)
    }
}

private struct DeviceList: View {
    var title: String
    var devices: [AudioDevice]
    var chosen: AudioDevice.ID?
    var fallback: String
    var pick: (AudioDevice.ID?) -> Void

    var body: some View {
        PopoverBox(width: 260) {
            Text(title).labelMono()
                .padding(.horizontal, 10)
                .padding(.bottom, 6)

            DeviceRow(label: fallback, chosen: chosen == nil) { pick(nil) }

            ForEach(devices) { device in
                DeviceRow(label: device.name, chosen: chosen == device.id) { pick(device.id) }
            }
        }
    }
}

private struct DeviceRow: View {
    var label: String
    var chosen: Bool
    var action: () -> Void

    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Circle()
                    .fill(chosen ? Theme.brand : Color.white.opacity(0.2))
                    .frame(width: 9, height: 9)

                Text(label)
                    .font(Theme.sans(12.5))
                    .foregroundStyle(chosen || hovering ? Theme.inkStrong : Theme.inkIcon)
                    .lineLimit(1)

                Spacer(minLength: 0)
            }
            .padding(.vertical, 8)
            .padding(.horizontal, 10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(hovering ? Theme.row : .clear, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
    }
}
