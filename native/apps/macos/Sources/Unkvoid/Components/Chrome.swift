import SwiftUI

/// `common/Avatar.tsx`: a foto, e enquanto ela não existe as iniciais. O violeta marca o
/// que é seu na tela — de todo mundo é o cinza chapado (`.avatar-flat`).
struct Avatar: View {
    var name: String?
    var url: String?
    var size: CGFloat = 32
    var mine = false
    var status: Bool?
    var square = false

    var body: some View {
        ZStack(alignment: .bottomTrailing) {
            shape

            if let status {
                Circle()
                    .fill(status ? Theme.online : Theme.offline)
                    .frame(width: max(8, size / 3.2), height: max(8, size / 3.2))
                    .overlay(Circle().strokeBorder(Color(hex: 0x0F0C18), lineWidth: 2))
                    .offset(x: 1, y: 1)
            }
        }
        .frame(width: size, height: size)
    }

    private var radius: CGFloat {
        square ? (size / 3).rounded() : size / 2
    }

    @ViewBuilder
    private var shape: some View {
        let corner = RoundedRectangle(cornerRadius: radius, style: .continuous)

        if let url, let address = URL(string: url) {
            AsyncImage(url: address) { image in
                image.resizable().scaledToFill()
            } placeholder: {
                initialsBox
            }
            .frame(width: size, height: size)
            .clipShape(corner)
        } else {
            initialsBox
        }
    }

    private var initialsBox: some View {
        Text(initials)
            .font(Theme.sans(max(9, (size * 0.33).rounded()), mine ? .bold : .semibold))
            .foregroundStyle(mine ? Theme.inkStrong : Theme.inkIcon)
            .frame(width: size, height: size)
            .background(
                mine ? AnyShapeStyle(Theme.brandGradient) : AnyShapeStyle(Color.white.opacity(0.08)),
                in: RoundedRectangle(cornerRadius: radius, style: .continuous)
            )
    }

    private var initials: String {
        let words = (name ?? "?").split(whereSeparator: \.isWhitespace)

        guard let first = words.first else {
            return "?"
        }

        if words.count > 1, let second = words[1].first, let head = first.first {
            return String([head, second]).uppercased()
        }

        return String(first.prefix(2)).uppercased()
    }
}

/// `common/Popover.tsx`: o painel que abre ao lado de um botão e fecha ao clicar fora.
///
/// Não é o `NSPopover` do sistema de propósito: ele traz a própria seta e o próprio fundo
/// claro, e aqui o painel é o `.popover` do CSS. O que o sistema dá de graça — fechar no
/// Esc e no clique fora — continua sendo dele.
struct PopoverBox<Content: View>: View {
    var width: CGFloat = 224
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            content
        }
        .frame(width: width, alignment: .leading)
        .padding(8)
        .popoverPanel()
    }
}

/// O item de um menu de popover: ícone à esquerda, texto à direita, a linha inteira clica.
struct MenuRow: View {
    var icon: IconName
    var label: String
    var tint: Color = Theme.inkIcon
    var action: () -> Void

    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Icon(name: icon, size: 15)

                Text(label)
                    .font(Theme.sans(12.5))

                Spacer(minLength: 0)
            }
            .foregroundStyle(hovering ? Theme.inkStrong : tint)
            .padding(.vertical, 8)
            .padding(.horizontal, 10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(hovering ? Theme.row : .clear, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
    }
}

/// `common/Modal.tsx`: o cartão que cobre a tela, com título, corpo rolável e rodapé.
struct ModalFrame<Body: View, Footer: View>: View {
    var title: String
    var subtitle: String?
    var width: CGFloat = 480
    var onClose: () -> Void
    @ViewBuilder var content: Body
    @ViewBuilder var footer: Footer

    var body: some View {
        ZStack {
            Color(hex: 0x06050A).opacity(0.74)
                .ignoresSafeArea()
                .onTapGesture(perform: onClose)

            VStack(spacing: 0) {
                HStack(alignment: .top, spacing: 12) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(title)
                            .font(Theme.sans(18, .semibold))
                            .tracking(-0.3)
                            .foregroundStyle(Theme.ink)

                        if let subtitle {
                            Text(subtitle)
                                .font(Theme.sans(12.5))
                                .foregroundStyle(Theme.inkSoft)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)

                    Button(action: onClose) {
                        Icon(name: .close)
                            .foregroundStyle(Theme.inkDim)
                            .padding(4)
                    }
                    .buttonStyle(.plain)
                }
                .padding(.horizontal, 24)
                .padding(.top, 24)

                ScrollView {
                    content
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 24)
                        .padding(.vertical, 20)
                }

                Divider().overlay(Theme.line)

                HStack(spacing: 8) {
                    footer
                }
                .padding(.horizontal, 24)
                .padding(.vertical, 16)
            }
            .frame(maxWidth: width)
            .glassPanel()
            .padding(24)
        }
    }
}
