import SwiftUI

/// A tela de entrada, medida por medida igual a
/// `native/apps/desktop/ui/components/entry/EntryScreen.tsx` e `AuthCard.tsx`.
///
/// São duas colunas de 420 lado a lado, até 880 no total: criar ou entrar numa sala, e a
/// conta. Quem já entrou não vê a segunda.
struct EntryScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        ScrollView {
            // `items-start ... pt-[10vh]`: o conteúdo começa a um décimo da altura, e não
            // centralizado — com a janela alta, centralizar joga tudo para baixo demais.
            HStack(alignment: .top, spacing: 20) {
                RoomCard()

                if !model.signedIn {
                    AuthCard()
                }
            }
            .frame(maxWidth: 880)
            .fixedSize(horizontal: false, vertical: true)
            .padding(24)
            .padding(.top, 40)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.back)
    }
}

private struct RoomCard: View {
    @EnvironmentObject private var model: AppModel
    @FocusState private var focus: Focus?

    private enum Focus {
        case name
        case code
    }

    var body: some View {
        VStack(spacing: 0) {
            Text("Criar uma sala")
                .font(Theme.sans(18, .semibold))
                .tracking(-0.3)
                .foregroundStyle(Theme.ink)

            Text("Compartilhe sua tela com quem você quiser.")
                .font(Theme.sans(13))
                .foregroundStyle(Theme.inkSoft)
                .padding(.top, 6)
                .padding(.bottom, 24)

            VStack(alignment: .leading, spacing: 8) {
                Text("Seu nome").labelMono()

                TextField("Como aparecer para os outros", text: $model.name)
                    .field(focused: focus == .name, invalid: !model.nameError.isEmpty)
                    .focused($focus, equals: .name)
                    .onSubmit(create)
                    .fieldError(model.nameError)
            }
            .padding(.bottom, 14)

            Button(action: create) {
                Text(model.signedIn ? "Criar uma sala" : "Criar uma sala sem login")
            }
            .buttonStyle(PrimaryButton())
            .disabled(model.busy != nil)

            DividerOr()
                .padding(.vertical, 16)

            HStack(alignment: .top, spacing: 8) {
                TextField("Código da sala", text: $model.code)
                    .font(Theme.mono(14))
                    .field(focused: focus == .code, invalid: !model.codeError.isEmpty)
                    .focused($focus, equals: .code)
                    .onSubmit(join)
                    .fieldError(model.codeError)

                Button("Entrar", action: join)
                    .buttonStyle(GhostButton())
                    .disabled(model.busy != nil)
            }

            // `min-h-5`: o espaço do erro existe sempre, senão a tela pula quando ele
            // aparece.
            Text(model.entryError)
                .font(Theme.sans(12.5))
                .foregroundStyle(Theme.danger)
                .multilineTextAlignment(.center)
                .frame(maxWidth: .infinity, minHeight: 20)
                .padding(.top, 12)

            Spacer(minLength: 0)
        }
        .padding(32)
        .frame(maxWidth: 420, maxHeight: .infinity, alignment: .top)
        .glass(radius: 22, shadowed: true)
    }

    private func create() {
        Task { await model.createRoom() }
    }

    private func join() {
        Task { await model.joinRoom() }
    }
}

private struct AuthCard: View {
    @EnvironmentObject private var model: AppModel
    @FocusState private var focus: Focus?
    @State private var registering = false

    private enum Focus {
        case email
        case password
    }

    var body: some View {
        VStack(spacing: 0) {
            Text(registering ? "Criar conta" : "Entrar")
                .font(Theme.sans(16, .semibold))
                .foregroundStyle(Theme.ink)

            Text(registering ? "Para ter servidores, voz e chat." : "Sem conta dá para compartilhar a tela. Servidores, voz e chat pedem login.")
                .font(Theme.sans(12.5))
                .foregroundStyle(Theme.inkSoft)
                .multilineTextAlignment(.center)
                .padding(.top, 6)
                .padding(.bottom, 20)

            Button(action: { Task { await model.googleLogin() } }) {
                HStack(spacing: 10) {
                    if model.googleWaiting {
                        ProgressView()
                            .controlSize(.small)
                            .frame(width: 18, height: 18)
                    } else {
                        GoogleMark()
                    }

                    Text(googleLabel)
                }
                .font(Theme.sans(14, .semibold))
                .foregroundStyle(Color(hex: 0x1F1F1F))
                .frame(maxWidth: .infinity)
                .padding(12)
                .background(.white, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
            .buttonStyle(.plain)
            .disabled(model.googleWaiting)

            DividerOr()
                .padding(.vertical, 16)

            VStack(alignment: .leading, spacing: 8) {
                Text("E-mail").labelMono()

                TextField("voce@email.com", text: $model.email)
                    .field(focused: focus == .email, invalid: !model.emailError.isEmpty)
                    .focused($focus, equals: .email)
                    .fieldError(model.emailError)
            }

            VStack(alignment: .leading, spacing: 8) {
                Text("Senha").labelMono()

                SecureField(registering ? "8 ou mais" : "••••••••", text: $model.password)
                    .field(focused: focus == .password, invalid: !model.passwordError.isEmpty)
                    .focused($focus, equals: .password)
                    .onSubmit(submit)
                    .fieldError(model.passwordError)
            }
            .padding(.top, 14)

            Button(registering ? "Criar conta" : "Entrar", action: submit)
                .buttonStyle(PrimaryButton())
                .padding(.top, 16)

            Text(model.loginError)
                .font(Theme.sans(12.5))
                .foregroundStyle(Theme.danger)
                .multilineTextAlignment(.center)
                .frame(maxWidth: .infinity, minHeight: 20)
                .padding(.top, 12)

            HStack(spacing: 4) {
                Text(registering ? "Já tem conta?" : "Não tem conta?")
                    .foregroundStyle(Theme.inkDim)

                Button(registering ? "Entrar" : "Criar conta") {
                    registering.toggle()
                }
                .buttonStyle(.plain)
                .foregroundStyle(Color(hex: 0x9A9CFF))
                .underline()
            }
            .font(Theme.sans(12.5))
            .padding(.top, 16)
        }
        .padding(32)
        .frame(maxWidth: 420, maxHeight: .infinity, alignment: .top)
        .glass(radius: 22, shadowed: true)
    }

    private var googleLabel: String {
        if model.googleWaiting {
            return "Aguardando o navegador…"
        }

        return registering ? "Criar conta com Google" : "Entrar com Google"
    }

    private func submit() {
        Task { await model.signIn(registering: registering) }
    }
}

/// `.divider-or`: uma linha de cada lado da palavra.
private struct DividerOr: View {
    var body: some View {
        HStack(spacing: 12) {
            line
            Text("ou").labelMono()
            line
        }
    }

    private var line: some View {
        Rectangle()
            .fill(Theme.line)
            .frame(height: 1)
    }
}

/// O "G" do Google, com os quatro caminhos do `AuthCard.tsx` — os mesmos que o Google
/// publica. Desenhá-lo no olho dá um anel colorido com um risco, que não é a marca de
/// ninguém e não pode ir num botão que diz "Entrar com Google".
private struct GoogleMark: View {
    private static let quarters: [(UInt32, String)] = [
        (0xFFC107, "M43.6 20.5H42V20H24v8h11.3C33.7 32.7 29.2 36 24 36c-6.6 0-12-5.4-12-12s5.4-12 12-12c3 0 5.8 1.1 7.9 3l5.7-5.7C34 6.1 29.3 4 24 4 12.9 4 4 12.9 4 24s8.9 20 20 20 20-8.9 20-20c0-1.3-.1-2.4-.4-3.5z"),
        (0xFF3D00, "M6.3 14.7l6.6 4.8C14.7 15.1 19 12 24 12c3 0 5.8 1.1 7.9 3l5.7-5.7C34 6.1 29.3 4 24 4 16.3 4 9.7 8.3 6.3 14.7z"),
        (0x4CAF50, "M24 44c5.2 0 9.9-2 13.4-5.2l-6.2-5.2C29.2 35.1 26.7 36 24 36c-5.2 0-9.6-3.3-11.3-8l-6.5 5C9.5 39.6 16.2 44 24 44z"),
        (0x1976D2, "M43.6 20.5H42V20H24v8h11.3c-.8 2.2-2.2 4.2-4.1 5.6l6.2 5.2C37 38.2 44 33 44 24c0-1.3-.1-2.4-.4-3.5z"),
    ]

    var body: some View {
        ZStack {
            ForEach(Self.quarters.indices, id: \.self) { index in
                VectorPath(commands: Self.quarters[index].1, viewBox: 48)
                    .fill(Color(hex: Self.quarters[index].0))
            }
        }
        .frame(width: 18, height: 18)
    }
}
