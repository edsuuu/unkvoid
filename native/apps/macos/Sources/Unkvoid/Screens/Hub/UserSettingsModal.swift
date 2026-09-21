import SwiftUI

/// `ui/components/hub/modals/UserSettingsModal.tsx`: quem você é, qual microfone, qual
/// saída de áudio.
///
/// Está sem as partes que precisam de uma decisão que a ABI do núcleo ainda não tem: trocar
/// a foto, o modo de abrir o microfone (voz / apertar para falar / sempre aberto), a
/// sensibilidade, a supressão de ruído e as teclas. Todas são preferências do `Voice.ts`, e
/// preferência se guarda no `shared/core` — ver o relatório no `README.md` desta pasta.
struct UserSettingsModal: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        ModalFrame(
            title: model.user?.name ?? "Sua conta",
            subtitle: "Online",
            width: 460,
            onClose: { model.modal = nil }
        ) {
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: 12) {
                    Avatar(name: model.user?.name, url: model.user?.avatar_url, size: 56, mine: true)

                    VStack(alignment: .leading, spacing: 4) {
                        Text(model.user?.name ?? "Sua conta")
                            .font(Theme.sans(14, .semibold))
                            .foregroundStyle(Theme.ink)

                        if let email = model.user?.email {
                            Text(email)
                                .font(Theme.sans(12.5))
                                .foregroundStyle(Theme.inkSoft)
                        }
                    }

                    Spacer(minLength: 0)
                }
                .padding(.bottom, 24)

                Text("Microfone").labelMono()
                    .padding(.bottom, 8)

                Devices(devices: model.microphones, chosen: model.microphone, fallback: "Microfone padrão") {
                    model.microphone = $0
                }

                Text("Saída de áudio").labelMono()
                    .padding(.top, 24)
                    .padding(.bottom, 8)

                Devices(devices: model.speakers, chosen: model.speaker, fallback: "Saída padrão") {
                    model.speaker = $0
                }

                Text("Vale para a voz das pessoas, o áudio das telas e os sons do app.")
                    .font(Theme.sans(11.5))
                    .foregroundStyle(Theme.inkDim)
                    .padding(.top, 6)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        } footer: {
            Button("Sair da conta") {
                model.modal = nil

                Task { await model.signOut() }
            }
            .buttonStyle(QuietButton())

            Spacer(minLength: 0)

            Button("Pronto") {
                model.modal = nil
            }
            .buttonStyle(PrimaryButton())
            .fixedSize()
        }
        .task { model.refreshDevices() }
    }
}

private struct Devices: View {
    var devices: [AudioDevice]
    var chosen: AudioDevice.ID?
    var fallback: String
    var pick: (AudioDevice.ID?) -> Void

    var body: some View {
        VStack(spacing: 6) {
            row(label: fallback, selected: chosen == nil) { pick(nil) }

            ForEach(devices) { device in
                row(label: device.name, selected: chosen == device.id) { pick(device.id) }
            }
        }
    }

    private func row(label: String, selected: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Circle()
                    .fill(selected ? Theme.brand : Color.white.opacity(0.2))
                    .frame(width: 9, height: 9)

                Text(label)
                    .font(Theme.sans(12.5))
                    .foregroundStyle(selected ? Theme.inkStrong : Theme.inkIcon)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .rowItem(selected: selected)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// `.btn-quiet`: a ação que pesa (sair, excluir), com o contorno quieto.
struct QuietButton: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Theme.sans(12.5))
            .foregroundStyle(Theme.periwinkle)
            .padding(.vertical, 9)
            .padding(.horizontal, 13)
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .strokeBorder(Theme.periwinkle.opacity(0.28), lineWidth: 1)
            )
            .opacity(configuration.isPressed ? 0.7 : 1)
    }
}
