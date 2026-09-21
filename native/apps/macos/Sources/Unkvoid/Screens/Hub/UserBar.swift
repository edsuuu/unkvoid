import SwiftUI

/// A barra de baixo, no lugar do `ui/components/hub/VoicePanel.tsx`: quem você é, o
/// microfone, o áudio e o menu da conta.
///
/// Fora de um canal de voz os dois botões ficam apagados, como no React. Dentro, o bloco
/// "Voz conectada" aparece em cima, com desconectar, câmera e compartilhar. A setinha ao
/// lado de cada botão é o que o Mac ganha a mais — ela abre a lista de aparelhos que o
/// CoreAudio enxerga, sem depender de estar numa voz para escolher.
struct UserBar: View {
    @EnvironmentObject private var model: AppModel
    @State private var open: Picker?
    @State private var popoverFrame = CGRect.zero

    private enum Picker {
        case microphone
        case speaker
    }

    /// O painel é alinhado pela base da barra; subi-lo a altura dela mais um respiro é o
    /// que o põe **acima** da barra, e não por cima dela. 12 + 30 + 12 de padding e avatar,
    /// e mais o bloco da voz (duas fileiras de 32, a divisória e os respiros) quando ele existe.
    private var barHeight: CGFloat {
        inVoice ? 62 + 95 : 62
    }

    var body: some View {
        VStack(spacing: 10) {
            if let voice = model.voiceChannel {
                connected(to: voice)

                Divider().overlay(Theme.line)
            }

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

                device(.microphone, icon: micOff ? .micOff : .mic, off: micOff, enabled: !inVoice || model.mine.canSpeak, hint: micHint, choose: "Escolher o microfone") {
                    Task { await model.toggleMute() }
                }

                device(.speaker, icon: model.deafened ? .headphonesOff : .headphones, off: model.deafened, enabled: true, hint: model.deafened ? "Voltar a ouvir" : "Ensurdecer: não ouvir ninguém", choose: "Escolher a saída de áudio") {
                    Task { await model.toggleDeafen() }
                }

                SmallButton(icon: .gear, active: false, hint: "Configurações") {
                    open = nil
                    model.modal = .account
                }
            }
        }
        .padding(12)
        .glass()
        .overlay(alignment: .bottomTrailing) { popover.reportsFrame(to: $popoverFrame).offset(y: -barHeight) }
        .closesOnOutsideClick(active: open != nil, panel: popoverFrame) { open = nil }
    }

    private var inVoice: Bool {
        model.voiceChannel != nil
    }

    private var micOff: Bool {
        inVoice ? !model.mine.mic || model.mine.micMuted || !model.mine.canSpeak : model.mutedAtRest
    }

    private var speaking: Bool {
        inVoice && !micOff && model.micLevel > 0.02
    }

    private var micHint: String {
        if inVoice, !model.mine.canSpeak {
            return "Você não tem permissão para falar neste canal"
        }

        return micOff ? "Ativar o microfone" : "Mutar o microfone"
    }

    /// O bloco de cima do `VoicePanel.tsx`: o sinal, "Voz conectada", sair, e a fileira de
    /// câmera e compartilhar.
    private func connected(to voice: Channel) -> some View {
        VStack(spacing: 10) {
            HStack(spacing: 10) {
                Icon(name: .signal, size: 15)
                    .foregroundStyle(model.reconnecting ? Theme.inkDim : Theme.online)

                VStack(alignment: .leading, spacing: 2) {
                    Text(model.reconnecting ? "Reconectando…" : "Voz conectada")
                        .font(Theme.sans(13, .semibold))
                        .foregroundStyle(model.reconnecting ? Theme.danger : Theme.online)

                    Text(voice.name)
                        .font(Theme.mono(10.5))
                        .foregroundStyle(Theme.inkDim)
                        .lineLimit(1)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
                .onTapGesture { model.stageOpen = true }

                Button {
                    Task { await model.leaveVoice() }
                } label: {
                    Icon(name: .phoneOff, size: 14)
                        .foregroundStyle(Theme.periwinkle)
                }
                .buttonStyle(IconButton(side: 28, radius: 9))
                .help("Desconectar da voz")
            }

            HStack(spacing: 6) {
                Button {
                    Task { await model.toggleCamera() }
                } label: {
                    Icon(name: model.mine.camera ? .camera : .cameraOff, size: 16)
                        .foregroundStyle(model.mine.camera ? Theme.inkStrong : Theme.inkIcon)
                        .frame(maxWidth: .infinity)
                        .frame(height: 32)
                        .background(model.mine.camera ? AnyShapeStyle(Theme.brandGradient) : AnyShapeStyle(Theme.chrome), in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 9, style: .continuous)
                                .strokeBorder(model.mine.camera ? Theme.brand.opacity(0.6) : Theme.lineStrong, lineWidth: 1)
                        )
                }
                .buttonStyle(.pointer)
                .disabled(!model.mine.canVideo)
                .opacity(model.mine.canVideo ? 1 : 0.4)
                .help(model.mine.camera ? "Desligar a câmera" : "Ligar a câmera")

                shareButton
            }
        }
    }

    private var shareButton: some View {
            Button {
                if model.mine.sharing {
                    Task { await model.stopSharing() }
                } else {
                    Task { await model.openShare() }
                }
            } label: {
                HStack(spacing: 8) {
                    if model.shareStarting {
                        ProgressView().controlSize(.small)
                    } else {
                        Icon(name: .screen, size: 16)
                    }

                    Text(model.mine.sharing ? "Parar" : "Tela")
                        .font(Theme.sans(12, .medium))
                }
                .foregroundStyle(model.mine.sharing ? Theme.inkStrong : Theme.inkIcon)
                .frame(maxWidth: .infinity)
                .frame(height: 32)
                .background(model.mine.sharing ? AnyShapeStyle(Theme.brandGradient) : AnyShapeStyle(Theme.chrome), in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 9, style: .continuous)
                        .strokeBorder(model.mine.sharing ? Theme.brand.opacity(0.6) : Theme.lineStrong, lineWidth: 1)
                )
            }
            .buttonStyle(.pointer)
            .disabled(model.shareStarting || !model.mine.canShare)
            .opacity(model.mine.canShare ? 1 : 0.4)
            .help(model.mine.canShare ? "Compartilhar tela" : "Você não tem permissão para transmitir neste canal")
    }

    /// O par "ligar/desligar" e a setinha, colados como nos apps de chamada: o botão à esquerda faz
    /// a ação, a setinha à direita abre a escolha do aparelho.
    private func device(
        _ picker: Picker,
        icon: IconName,
        off: Bool,
        enabled: Bool,
        hint: String,
        choose: String,
        action: @escaping () -> Void
    ) -> some View {
        HStack(spacing: 0) {
            SmallButton(icon: icon, active: false, danger: off, ringed: picker == .microphone && speaking, hint: hint, action: action)
                .disabled(!enabled)

            Chevron(open: open == picker, hint: choose) {
                model.refreshDevices()
                open = open == picker ? nil : picker
            }
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
        case nil:
            EmptyView()
        }
    }
}

/// O botão de 26 da barra: menor que o `.btn-icon`, sem moldura, como no React.
private struct SmallButton: View {
    var icon: IconName
    var active: Bool
    var danger = false
    /// O anel verde de quem está falando.
    var ringed = false
    var hint: String
    var action: () -> Void

    @Environment(\.isEnabled) private var enabled
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            Icon(name: icon, size: 16.5)
                .scaleEffect(hovering ? 1.12 : 1)
                .foregroundStyle(danger ? Theme.danger : ringed ? Theme.online : active || hovering ? Theme.inkStrong : Theme.inkIcon)
                .frame(width: 28, height: 28)
                .background(active || hovering ? Theme.row : .clear, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .strokeBorder(ringed ? Theme.online.opacity(0.6) : .clear, lineWidth: 1)
                )
        }
        .buttonStyle(.pointer)
        .opacity(enabled ? 1 : 0.4)
        .onHover { hovering = $0 && enabled }
        .animation(.easeOut(duration: 0.12), value: hovering)
        .help(hint)
    }
}

/// A setinha ao lado do microfone e do fone: acende e cresce sob o mouse, como o botão dela.
private struct Chevron: View {
    var open: Bool
    var hint: String
    var action: () -> Void

    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            Icon(name: .chevronDown, size: 12.5)
                .scaleEffect(hovering ? 1.15 : 1)
                .foregroundStyle(open || hovering ? Theme.inkStrong : Theme.inkDim)
                .frame(width: 18, height: 28)
                .background(open || hovering ? Theme.row : .clear, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                .contentShape(Rectangle())
        }
        .buttonStyle(.pointer)
        .onHover { hovering = $0 }
        .animation(.easeOut(duration: 0.12), value: hovering)
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
        .buttonStyle(.pointer)
        .onHover { hovering = $0 }
    }
}
