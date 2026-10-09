import SwiftUI

/// O rodapé da coluna de canais, em `surfacePanel`: o painel da voz quando se está numa (o
/// sinal, "Voz conectada", desconectar, câmera e tela) e a barra do usuário de 52 — quem você
/// é, o microfone, o fone e a engrenagem.
///
/// A setinha ao lado do microfone e do fone é o que o Mac ganha a mais: abre a lista de
/// aparelhos que o CoreAudio enxerga, sem depender de estar numa voz para escolher.
struct UserBar: View {
    @EnvironmentObject private var model: AppModel
    @State private var open: Picker?
    @State private var popoverFrame = CGRect.zero

    private enum Picker {
        case microphone
        case speaker
    }

    /// O painel é alinhado pela base da barra; subi-lo a altura dela mais um respiro é o
    /// que o põe **acima** da barra, e não por cima dela.
    private var barHeight: CGFloat {
        inVoice ? Theme.Size.userBar + 104 : Theme.Size.userBar
    }

    var body: some View {
        VStack(spacing: 0) {
            if let voice = model.voiceChannel ?? joining {
                VoicePanel(channel: voice)
            }

            HStack(spacing: 4) {
                Avatar(name: model.user?.name ?? "?", url: model.user?.avatar_url, size: 32, mine: true, status: true, ring: Theme.surfacePanel)
                    .padding(.trailing, 4)

                VStack(alignment: .leading, spacing: 0) {
                    Text(model.user?.name ?? "Conta conectada")
                        .font(Theme.sans(14, .semibold))
                        .foregroundStyle(Theme.inkStrong)
                        .lineLimit(1)

                    Text("Online")
                        .font(Theme.meta)
                        .foregroundStyle(Theme.inkDim)
                        .lineLimit(1)
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                device(.microphone, icon: micOff ? .micOff : .mic, off: micOff, enabled: !inVoice || model.mine.canSpeak, hint: micHint, choose: "Escolher o microfone") {
                    Task { await model.toggleMute() }
                }

                device(.speaker, icon: model.deafened ? .headphonesOff : .headphones, off: model.deafened, enabled: true, hint: model.deafened ? "Voltar a ouvir" : "Ensurdecer", choose: "Escolher a saída de áudio") {
                    Task { await model.toggleDeafen() }
                }

                SmallButton(icon: .gear, active: false, hint: "Configurações do usuário") {
                    open = nil
                    model.modal = .account
                }
            }
            .padding(.horizontal, 8)
            .frame(height: Theme.Size.userBar)
        }
        .background(Theme.surfacePanel)
        .overlay(alignment: .bottomTrailing) { popover.reportsFrame(to: $popoverFrame).offset(x: -8, y: -barHeight) }
        .closesOnOutsideClick(active: open != nil, panel: popoverFrame) { open = nil }
    }

    /// O canal em que se está entrando: o painel da voz aparece já no clique, com
    /// "Conectando…", e vira "Voz conectada" quando o SFU responde.
    private var joining: Channel? {
        model.tree?.voiceChannels.first { $0.id == model.voiceTarget }
    }

    private var inVoice: Bool {
        model.voiceChannel != nil || joining != nil
    }

    private var micOff: Bool {
        model.micShownOff
    }

    private var speaking: Bool {
        model.user.map { model.isSpeaking($0.id) } ?? false
    }

    private var micHint: String {
        if inVoice, !model.mine.canSpeak {
            return "Você não tem permissão para falar neste canal"
        }

        return micOff ? "Desmutar" : "Mutar"
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

/// O painel da voz, acima da barra: o sinal e o estado, o canal e o servidor, desconectar, e
/// os dois botões meio a meio de câmera e tela.
private struct VoicePanel: View {
    @EnvironmentObject private var model: AppModel

    let channel: Channel

    var body: some View {
        VStack(spacing: 8) {
            HStack(spacing: 8) {
                SignalBars(
                    bars: model.reconnecting ? nil : model.signalBars,
                    hint: model.voiceJoining ? "Conectando…" : model.reconnecting ? "Reconectando…" : model.ping.map { "\($0) ms" } ?? "Medindo o ping…"
                )

                VStack(alignment: .leading, spacing: 0) {
                    Text(model.voiceJoining ? "Conectando…" : model.reconnecting ? "Reconectando…" : "Voz conectada")
                        .font(Theme.sans(14, .semibold))
                        .foregroundStyle(model.voiceJoining || model.reconnecting ? Theme.idle : Theme.online)

                    Text("\(channel.name) / \(model.tree?.name ?? "")")
                        .font(Theme.meta)
                        .foregroundStyle(Theme.inkDim)
                        .lineLimit(1)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
                .onTapGesture { model.stageOpen = true }
                .help("Abrir a chamada")

                Button {
                    Task { await model.leaveVoice() }
                } label: {
                    Icon(name: .phoneOff, size: 20)
                }
                .buttonStyle(IconButton(tone: .off))
                .help("Desconectar")
            }

            HStack(spacing: 8) {
                half(model.mine.camera ? .camera : .cameraOff, "Câmera", on: model.mine.camera, enabled: model.mine.canVideo && !model.voiceJoining, hint: !model.mine.canVideo ? "Você não tem permissão para ligar a câmera neste canal" : model.mine.camera ? "Desligar a câmera" : "Ligar a câmera") {
                    await model.toggleCamera()
                }

                half(.screen, model.mine.sharing ? "Parar" : "Tela", on: model.mine.sharing, enabled: model.mine.canShare && !model.shareStarting, hint: !model.mine.canShare ? "Você não tem permissão para transmitir neste canal" : model.mine.sharing ? "Parar de compartilhar" : "Compartilhar tela") {
                    if model.mine.sharing {
                        await model.stopSharing()
                    } else {
                        await model.openShare()
                    }
                }
            }
        }
        .padding(8)
        .overlay(alignment: .top) {
            Rectangle().fill(Theme.line).frame(height: 1)
        }
    }

    private func half(_ icon: IconName, _ label: String, on: Bool, enabled: Bool, hint: String, _ action: @escaping @MainActor () async -> Void) -> some View {
        Button {
            Task { await action() }
        } label: {
            HStack(spacing: 6) {
                if icon == .screen, model.shareStarting {
                    ProgressView().controlSize(.small)
                } else {
                    Icon(name: icon, size: 18)
                        .foregroundStyle(on ? Theme.online : Theme.inkSoft)
                }

                Text(label)
                    .font(Theme.button)
                    .foregroundStyle(Theme.ink)
            }
            .frame(maxWidth: .infinity)
            .frame(height: Theme.Size.row)
            .background(Theme.hover, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
            .contentShape(Rectangle())
        }
        .buttonStyle(.pointer)
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.4)
        .help(hint)
    }
}

/// O botão de 32 da barra, sem moldura: `inkSoft` em repouso, `hover` sob o mouse, vermelho
/// quando é o cortado.
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
            Icon(name: icon, size: 20)
                .foregroundStyle(danger ? Theme.danger : ringed ? Theme.online : active || hovering ? Theme.inkStrong : Theme.inkSoft)
                .frame(width: Theme.Size.row, height: Theme.Size.row)
                .background(active || hovering ? Theme.hover : .clear, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous)
                        .strokeBorder(ringed ? Theme.online.opacity(0.6) : .clear, lineWidth: 1)
                )
        }
        .buttonStyle(.pointer)
        .opacity(enabled ? 1 : 0.4)
        .onHover { hovering = $0 && enabled }
        .animation(.easeOut(duration: 0.1), value: hovering)
        .help(hint)
    }
}

/// O sinal da voz: quatro barrinhas que acendem até o nível que o núcleo contou, na cor dele
/// — verde, amarelo, laranja, vermelho. Sem medida ainda (ou reconectando) ficam apagadas.
private struct SignalBars: View {
    var bars: Int?
    /// O balão é desenhado aqui, e não pelo `.help` do sistema: o tooltip nativo demora um
    /// segundo e não dispara sobre um desenho sem área de clique.
    var hint: String

    @State private var hovering = false

    private var tint: Color {
        switch bars {
        case 4: Theme.online
        case 3: Theme.idle
        case 2: Theme.poor
        case 1: Theme.danger
        default: Theme.inkDim
        }
    }

    var body: some View {
        HStack(alignment: .bottom, spacing: 2) {
            ForEach(1 ... 4, id: \.self) { bar in
                RoundedRectangle(cornerRadius: 1, style: .continuous)
                    .fill(bar <= (bars ?? 0) ? tint : Theme.inkDim.opacity(0.35))
                    .frame(width: 3, height: CGFloat(3 + bar * 3))
            }
        }
        .frame(width: 18, height: 16, alignment: .bottom)
        .padding(6)
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .padding(-6)
        .overlay(alignment: .bottomLeading) {
            if hovering {
                Text(hint)
                    .font(Theme.sans(14, .semibold))
                    .foregroundStyle(Theme.ink)
                    .fixedSize()
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .popoverPanel()
                    .offset(x: -4, y: -30)
                    .allowsHitTesting(false)
                    .transition(.opacity)
            }
        }
        .animation(.easeOut(duration: 0.1), value: hovering)
        .animation(.easeOut(duration: 0.2), value: bars)
        .accessibilityLabel("Sinal da voz")
        .accessibilityValue(hint)
    }
}

/// A setinha ao lado do microfone e do fone.
private struct Chevron: View {
    var open: Bool
    var hint: String
    var action: () -> Void

    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            Icon(name: .chevronDown, size: 12)
                .foregroundStyle(open || hovering ? Theme.inkStrong : Theme.inkDim)
                .frame(width: 12, height: Theme.Size.row)
                .background(open || hovering ? Theme.hover : .clear, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
                .contentShape(Rectangle())
        }
        .buttonStyle(.pointer)
        .onHover { hovering = $0 }
        .animation(.easeOut(duration: 0.1), value: hovering)
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
                .padding(.horizontal, 8)
                .padding(.vertical, 6)

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
                    .fill(chosen ? Theme.brand : Theme.inkGhost)
                    .frame(width: 9, height: 9)

                Text(label)
                    .font(Theme.button)
                    .foregroundStyle(hovering ? .white : chosen ? Theme.inkStrong : Theme.inkSoft)
                    .lineLimit(1)

                Spacer(minLength: 0)
            }
            .padding(.horizontal, 8)
            .frame(height: Theme.Size.row)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(hovering ? Theme.brand : .clear, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
        }
        .buttonStyle(.pointer)
        .onHover { hovering = $0 }
    }
}
