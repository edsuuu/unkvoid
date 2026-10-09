import SwiftUI

/// "Convidar pessoas": o código do servidor aberto, para copiar, e o caminho para gerar outro.
struct InvitePeopleModal: View {
    @EnvironmentObject private var model: AppModel

    @State private var copied = false

    var body: some View {
        ModalFrame(
            title: "Convide amigos para \(model.tree?.name ?? "o servidor")",
            subtitle: "Quem tiver o código entra pelo \"+\" do trilho, em \"Entrar com um convite\".",
            width: 440,
            onClose: { model.modal = nil }
        ) {
            VStack(alignment: .leading, spacing: 8) {
                Text("Código do convite").labelMono()

                HStack(spacing: 8) {
                    Text(model.tree?.invite_code ?? "—")
                        .font(Theme.mono(16))
                        .foregroundStyle(Theme.inkStrong)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 12)
                        .frame(height: 40)
                        .background(Theme.surfaceInput, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))

                    Button(copied ? "Copiado" : "Copiar") {
                        model.copyInvite()
                        copied = true
                    }
                    .buttonStyle(copied ? AnyButtonStyle(OnlineButton()) : AnyButtonStyle(PrimaryButton(wide: false)))
                    .disabled(model.tree?.invite_code == nil)
                }

                if model.abilities.allows("manageServer") {
                    HStack(spacing: 4) {
                        Text("O convite não expira.")
                            .foregroundStyle(Theme.inkDim)

                        Button("Gerar outro") {
                            copied = false

                            Task { await model.renewInvite() }
                        }
                        .buttonStyle(.pointer)
                        .foregroundStyle(Theme.brandText)
                    }
                    .font(Theme.meta)
                    .padding(.top, 4)
                }
            }
        } footer: {
            Spacer(minLength: 0)

            Button("Fechar") { model.modal = nil }
                .buttonStyle(GhostButton())
        }
    }
}

/// O botão que ficou verde porque deu certo ("Copiado").
private struct OnlineButton: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Theme.button)
            .foregroundStyle(.white)
            .padding(.vertical, 8)
            .padding(.horizontal, 16)
            .background(Theme.online, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
    }
}

/// Dois estilos num mesmo botão: o SwiftUI pede um tipo só.
private struct AnyButtonStyle: ButtonStyle {
    private let make: (Configuration) -> AnyView

    init<Style: ButtonStyle>(_ style: Style) {
        make = { AnyView(style.makeBody(configuration: $0)) }
    }

    func makeBody(configuration: Configuration) -> some View {
        make(configuration)
    }
}
