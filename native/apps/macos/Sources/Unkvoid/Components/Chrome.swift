import SwiftUI

/// A foto, e enquanto ela não existe as iniciais. O violeta marca o que é seu na tela — de
/// todo mundo é o cinza da superfície de cima.
struct Avatar: View {
    var name: String?
    var url: String?
    var size: CGFloat = 32
    var mine = false
    var status: Bool?
    var square = false
    /// O raio, quando não é o do círculo nem o do quadrado: o ícone do servidor no trilho
    /// anima de 50% para 16 sob o mouse.
    var corner: CGFloat?
    /// A cor do anel em volta da bolinha de status: a da superfície onde o avatar está.
    var ring: Color = Theme.surfaceSide

    var body: some View {
        ZStack(alignment: .bottomTrailing) {
            shape

            if let status {
                Circle()
                    .fill(status ? Theme.online : .clear)
                    .overlay(Circle().strokeBorder(status ? .clear : Theme.offline, lineWidth: 2.5).padding(3))
                    .frame(width: dot, height: dot)
                    .overlay(Circle().strokeBorder(ring, lineWidth: 3))
                    .offset(x: 2, y: 2)
            }
        }
        .frame(width: size, height: size)
    }

    private var dot: CGFloat {
        max(10, (size / 3.2).rounded()) + 6
    }

    private var radius: CGFloat {
        corner ?? (square ? (size / 3).rounded() : size / 2)
    }

    @ViewBuilder
    private var shape: some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)

        if let url, let address = URL(string: url) {
            AsyncImage(url: address) { image in
                image.resizable().scaledToFill()
            } placeholder: {
                initialsBox
            }
            .frame(width: size, height: size)
            .clipShape(shape)
        } else {
            initialsBox
        }
    }

    private var initialsBox: some View {
        Text(initials)
            .font(Theme.sans(max(9, (size * 0.33).rounded()), .semibold))
            .foregroundStyle(Theme.inkStrong)
            .frame(width: size, height: size)
            .background(mine ? Theme.brand : Theme.surfaceChat, in: RoundedRectangle(cornerRadius: radius, style: .continuous))
            .animation(.easeOut(duration: 0.15), value: radius)
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

/// O painel que abre ao lado de um botão: `surfaceFloat`, raio 8, 188 no mínimo.
///
/// Não é o `NSPopover` do sistema de propósito: ele traz a própria seta e o próprio fundo
/// claro. O que o sistema dá de graça — fechar no Esc e no clique fora — continua sendo dele.
struct PopoverBox<Content: View>: View {
    var width: CGFloat = 220
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            content
        }
        .frame(width: width, alignment: .leading)
        .padding(.vertical, 6)
        .padding(.horizontal, 8)
        .popoverPanel()
    }
}

/// O clique fora fecha o que é desenhado à mão (o `.popover` do sistema e o `ModalFrame` já
/// fecham sozinhos). A conta é por geometria, não por `onHover`: o painel aberto sai dos
/// limites de quem o abriu, e o clique no próprio botão tem de continuar sendo do botão —
/// senão ele fecharia aqui e reabriria no `action`.
struct ClosesOnOutsideClick: ViewModifier {
    var active: Bool
    /// Onde está o painel aberto, em coordenadas globais: ele sai dos limites de quem o abriu.
    var panel: CGRect
    var close: () -> Void

    @State private var frame = CGRect.zero
    @State private var monitor: Any?

    func body(content: Content) -> some View {
        content
            .reportsFrame(to: $frame)
            .onChange(of: active, initial: true) { _, watching in
                watching ? watch() : unwatch()
            }
            .onDisappear(perform: unwatch)
    }

    private func watch() {
        guard monitor == nil else {
            return
        }

        monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { event in
            if let height = event.window?.contentView?.bounds.height {
                let point = CGPoint(x: event.locationInWindow.x, y: height - event.locationInWindow.y)

                if !frame.contains(point), !panel.contains(point) {
                    close()
                }
            }

            return event
        }
    }

    private func unwatch() {
        monitor.map(NSEvent.removeMonitor)
        monitor = nil
    }
}

extension View {
    func closesOnOutsideClick(active: Bool, panel: CGRect, close: @escaping () -> Void) -> some View {
        modifier(ClosesOnOutsideClick(active: active, panel: panel, close: close))
    }

    /// Conta a quem desenha onde esta view foi parar, em coordenadas globais.
    func reportsFrame(to frame: Binding<CGRect>) -> some View {
        background(GeometryReader { measured in
            Color.clear
                .onAppear { frame.wrappedValue = measured.frame(in: .global) }
                .onChange(of: measured.frame(in: .global)) { _, moved in frame.wrappedValue = moved }
        })
    }
}

/// O item de um menu: 32 de altura, 14/500; sob o mouse o fundo vira o violeta e o texto,
/// branco. O item de perigo é vermelho, e sob o mouse o fundo é o vermelho.
struct MenuRow: View {
    var icon: IconName?
    var label: String
    var danger = false
    var action: () -> Void

    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                if let icon {
                    Icon(name: icon, size: 16)
                }

                Text(label)
                    .font(Theme.button)
                    .lineLimit(1)

                Spacer(minLength: 0)
            }
            .foregroundStyle(hovering ? .white : danger ? Theme.danger : Theme.inkSoft)
            .padding(.horizontal, 8)
            .frame(height: Theme.Size.row)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(hovering ? (danger ? Theme.dangerFill : Theme.brand) : .clear, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
            .contentShape(Rectangle())
        }
        .buttonStyle(.pointer)
        .onHover { hovering = $0 }
    }
}

/// A divisória de um menu: 1 de altura, 4 de margem.
struct MenuDivider: View {
    var body: some View {
        Rectangle()
            .fill(Theme.line)
            .frame(height: 1)
            .padding(.vertical, 4)
            .padding(.horizontal, 4)
    }
}

/// O cartão que cobre a tela, com título, corpo rolável e rodapé. O fundo de trás é o preto a
/// 70%, sem desfoque; o rodapé é a superfície lateral.
struct ModalFrame<Body: View, Footer: View>: View {
    var title: String
    var subtitle: String?
    var width: CGFloat = 440
    /// O modal que pede uma decisão (escolher o apelido) não tem como ser dispensado.
    var dismissable = true
    var onClose: () -> Void
    @ViewBuilder var content: Body
    @ViewBuilder var footer: Footer

    /// A altura do corpo, medida: o cartão abraça o conteúdo e só rola quando ele não cabe.
    /// Um `ScrollView` solto ocuparia a janela inteira, e um "tem certeza?" viraria um painel.
    @State private var bodyHeight: CGFloat = 0

    private static var tallest: CGFloat { 520 }

    var body: some View {
        ZStack {
            Color.black.opacity(0.7)
                .ignoresSafeArea()
                .onTapGesture(perform: onClose)

            VStack(spacing: 0) {
                HStack(alignment: .top, spacing: 12) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(title)
                            .font(Theme.title)
                            .foregroundStyle(Theme.inkStrong)

                        if let subtitle {
                            Text(subtitle)
                                .font(Theme.sans(14))
                                .foregroundStyle(Theme.inkDim)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)

                    if dismissable {
                        Button(action: onClose) {
                            Icon(name: .close, size: 24)
                                .foregroundStyle(Theme.inkDim)
                        }
                        .buttonStyle(.pointer)
                        .help("Fechar")
                    }
                }
                .padding(.horizontal, 16)
                .padding(.top, 16)

                ScrollView {
                    content
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(16)
                        .background(GeometryReader { measured in
                            Color.clear.preference(key: BodyHeight.self, value: measured.size.height)
                        })
                }
                .frame(height: min(max(bodyHeight, 1), Self.tallest))
                .onPreferenceChange(BodyHeight.self) { bodyHeight = $0 }

                HStack(spacing: 8) {
                    footer
                }
                .padding(16)
                .background(Theme.surfaceSide)
            }
            .frame(maxWidth: width)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous))
            .modalPanel()
            .padding(24)
        }
    }
}

private struct BodyHeight: PreferenceKey {
    static let defaultValue: CGFloat = 0

    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = max(value, nextValue())
    }
}
