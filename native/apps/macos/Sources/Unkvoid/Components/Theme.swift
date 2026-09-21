import AppKit
import SwiftUI

/// A paleta e as medidas são as mesmas de `native/apps/desktop/ui/style.css`, valor por
/// valor. Divergir aqui faz o app do Mac parecer outro programa — e o CSS é a referência
/// porque é ele que está no ar hoje.
enum Theme {
    static let back = Color(hex: 0x06050A)
    static let ink = Color(hex: 0xECE9F3)
    static let inkStrong = Color.white
    static let inkBody = Color(hex: 0xE6E1F2)
    static let inkSoft = Color(hex: 0xA89FC0)
    static let inkDim = Color(hex: 0x8A80A6)
    static let inkIcon = Color(hex: 0xCFC9DE)
    static let inkFaint = Color(hex: 0x6F6889)
    static let inkGhost = Color(hex: 0x3A3350)
    static let brand = Color(hex: 0x8A7CF5)
    static let brandDark = Color(hex: 0x5A3FD6)
    static let lilac = Color(hex: 0x9A9CFF)
    static let lilac2 = Color(hex: 0xAEB0FF)
    static let periwinkle = Color(hex: 0x9AA0E0)
    static let online = Color(hex: 0x34D399)
    static let offline = Color(hex: 0x4A4265)
    static let danger = Color(hex: 0xE2445C)

    /// Nomes curtos que as telas já usavam, apontando para os tokens do CSS.
    static let soft = inkSoft
    static let accent = brand
    static let backdrop = back

    static let line = Color.white.opacity(0.09)
    static let lineStrong = Color.white.opacity(0.12)
    static let lineSoft = Color.white.opacity(0.08)
    static let glassFill = Color.white.opacity(0.04)
    static let fieldFill = Color.white.opacity(0.03)
    static let fieldLine = Color.white.opacity(0.10)
    static let row = Color.white.opacity(0.06)
    static let chrome = Color.white.opacity(0.05)

    /// `.popover` do CSS: opaco de propósito, senão o texto de trás atravessa o menu.
    static let popoverFill = Color(hex: 0x100D1A).opacity(0.96)

    static let brandGradient = LinearGradient(colors: [brand, brandDark], startPoint: .top, endPoint: .bottom)

    /// A mesma cadeia do CSS: `Archivo, Helvetica, Arial`. O projeto não distribui a
    /// Archivo, então hoje o que aparece é a Helvetica — no navegador e aqui. Cair no SF Pro
    /// do sistema, que é o padrão do SwiftUI, faria o app do Mac ter outra tipografia que o
    /// resto do produto.
    static func sans(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        Font.custom(installed(["Archivo", "Helvetica", "Arial"]), size: size).weight(weight)
    }

    /// `"IBM Plex Mono", ui-monospace, monospace`.
    static func mono(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        Font.custom(installed(["IBM Plex Mono", "Menlo", "Monaco"]), size: size).weight(weight)
    }

    /// A cor que o Laravel manda para um cargo, no formato `#rrggbb`. Sem cor, sem cor.
    static func hex(_ value: String?) -> Color? {
        guard let value, value.hasPrefix("#"), let number = UInt32(value.dropFirst(), radix: 16), value.count == 7 else {
            return nil
        }

        return Color(hex: number)
    }

    /// A primeira da lista que existe nesta máquina. `Font.custom` com nome desconhecido
    /// volta para o SF Pro em silêncio, e é justamente isso que se quer evitar.
    private static func installed(_ names: [String]) -> String {
        names.first { NSFont(name: $0, size: 12) != nil } ?? names[names.count - 1]
    }
}

/// `.glass` do CSS: fundo translúcido, desfoque e uma linha clara em volta. No macOS o
/// desfoque é do sistema (`ultraThinMaterial`), e não uma imitação — é o mesmo material das
/// janelas nativas.
struct Glass: ViewModifier {
    var radius: CGFloat = 20
    var shadowed = false

    func body(content: Content) -> some View {
        content
            .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: radius, style: .continuous))
            .background(Theme.glassFill, in: RoundedRectangle(cornerRadius: radius, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .strokeBorder(Theme.line, lineWidth: 1)
            )
            .shadow(color: .black.opacity(shadowed ? 0.9 : 0), radius: 40, x: 0, y: 30)
    }
}

/// `.label-mono`: 10.5px, caixa alta e espaçada. O nome ficou de quando ela era
/// monoespaçada; hoje a fonte é a do resto do app, e a monoespaçada só sobrou onde tem
/// função — o campo do código da sala, onde ela separa 0 de O.
struct LabelMono: ViewModifier {
    var size: CGFloat = 10.5

    func body(content: Content) -> some View {
        content
            .font(Theme.sans(size, .semibold))
            .tracking(size * 0.08)
            .textCase(.uppercase)
            .foregroundStyle(Theme.inkDim)
    }
}

/// `.field`: 12 de raio, 14px, e a borda que acende no foco.
struct Field: ViewModifier {
    var focused: Bool = false
    /// `.field[aria-invalid='true']` do CSS: o vermelho ganha do foco, senão clicar no
    /// campo errado apagaria a única marca de que ele é o errado.
    var invalid: Bool = false

    func body(content: Content) -> some View {
        content
            .textFieldStyle(.plain)
            .font(Theme.sans(14))
            .foregroundStyle(Theme.inkStrong)
            .padding(.vertical, 11)
            .padding(.horizontal, 13)
            .background(Theme.fieldFill, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(border, lineWidth: 1)
            )
            .animation(.easeOut(duration: 0.16), value: focused)
            .animation(.easeOut(duration: 0.16), value: invalid)
    }

    private var border: Color {
        if invalid {
            return Theme.danger
        }

        return focused ? Theme.brand.opacity(0.5) : Theme.fieldLine
    }
}

/// `.btn-primary`: o degradê da marca, de cima para baixo.
struct PrimaryButton: ButtonStyle {
    /// No rodapé de um modal o botão tem o tamanho do texto; num formulário ele ocupa a linha.
    var wide = true
    var font: Font = Theme.sans(14.5, .semibold)

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(font)
            .foregroundStyle(Theme.inkStrong)
            .frame(maxWidth: wide ? .infinity : nil)
            .padding(.vertical, wide ? 12 : 9)
            .padding(.horizontal, 16)
            .background(
                LinearGradient(colors: [Theme.brand, Theme.brandDark], startPoint: .top, endPoint: .bottom),
                in: RoundedRectangle(cornerRadius: 12, style: .continuous)
            )
            .brightness(configuration.isPressed ? -0.05 : 0)
            .scaleEffect(configuration.isPressed ? 0.99 : 1)
            .animation(.easeOut(duration: 0.12), value: configuration.isPressed)
    }
}

/// `.btn-danger`: o vermelho translúcido do que apaga, expulsa ou bane.
struct DangerButton: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Theme.sans(12.5, .medium))
            .foregroundStyle(Theme.danger)
            .padding(.vertical, 8)
            .padding(.horizontal, 12)
            .background(Theme.danger.opacity(configuration.isPressed ? 0.2 : 0.12), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .strokeBorder(Theme.danger.opacity(0.35), lineWidth: 1)
            )
            .scaleEffect(configuration.isPressed ? 0.97 : 1)
    }
}

/// `.btn-ghost`: sem preenchimento, só a linha.
struct GhostButton: ButtonStyle {
    var font: Font = Theme.sans(12.5)
    var padding = EdgeInsets(top: 8, leading: 12, bottom: 8, trailing: 12)

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(font)
            .foregroundStyle(Theme.inkIcon)
            .padding(padding)
            .background(Color.white.opacity(0.05), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .strokeBorder(Theme.lineStrong, lineWidth: 1)
            )
            .opacity(configuration.isPressed ? 0.8 : 1)
    }
}

/// `.btn-icon`: o quadradinho de 34 com o ícone dentro, e os dois estados que o CSS dá a
/// ele — `btn-icon-on` (o degradê da marca) e `btn-icon-off` (o vermelho do desligado).
struct IconButton: ButtonStyle {
    enum Tone {
        case idle
        case on
        case off
    }

    var side: CGFloat = 34
    var radius: CGFloat = 11
    var tone: Tone = .idle

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundStyle(tone == .off ? Theme.danger : tone == .on ? Theme.inkStrong : Theme.inkIcon)
            .frame(width: side, height: side)
            .background(background, in: RoundedRectangle(cornerRadius: radius, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .strokeBorder(border, lineWidth: 1)
            )
            .scaleEffect(configuration.isPressed ? 0.97 : 1)
            .animation(.easeOut(duration: 0.12), value: configuration.isPressed)
    }

    private var background: AnyShapeStyle {
        switch tone {
        case .idle: AnyShapeStyle(Theme.chrome)
        case .on: AnyShapeStyle(Theme.brandGradient)
        case .off: AnyShapeStyle(Theme.danger.opacity(0.12))
        }
    }

    private var border: Color {
        switch tone {
        case .idle: Theme.lineStrong
        case .on: Theme.brand.opacity(0.6)
        case .off: Theme.danger.opacity(0.35)
        }
    }
}

/// `.row-item`: a linha clicável das listas (canal, membro, cargo), com o estado ligado.
struct RowItem: ViewModifier {
    var selected = false

    func body(content: Content) -> some View {
        content
            .padding(.vertical, 9)
            .padding(.horizontal, 11)
            .background(
                selected ? Theme.brand.opacity(0.12) : .clear,
                in: RoundedRectangle(cornerRadius: 12, style: .continuous)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(selected ? Theme.brand.opacity(0.32) : Theme.lineSoft, lineWidth: 1)
            )
    }
}

extension View {
    func rowItem(selected: Bool = false) -> some View {
        modifier(RowItem(selected: selected))
    }

    func glass(radius: CGFloat = 20, shadowed: Bool = false) -> some View {
        modifier(Glass(radius: radius, shadowed: shadowed))
    }

    /// `.glass-panel`: o vidro dos modais, mais claro e com a sombra funda.
    func glassPanel() -> some View {
        background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 22, style: .continuous))
            .background(Theme.chrome, in: RoundedRectangle(cornerRadius: 22, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 22, style: .continuous)
                    .strokeBorder(Color.white.opacity(0.1), lineWidth: 1)
            )
            .shadow(color: .black.opacity(0.95), radius: 45, x: 0, y: 40)
    }

    /// `.popover`: o painel dos menus que abrem ao lado de um botão.
    func popoverPanel() -> some View {
        background(Theme.popoverFill, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(Theme.lineStrong, lineWidth: 1)
            )
            .shadow(color: .black.opacity(0.95), radius: 30, x: 0, y: 24)
    }

    /// `.code-chip`: o código da sala e o do convite, para copiar.
    func codeChip(size: CGFloat = 13) -> some View {
        font(Theme.mono(size))
            .foregroundStyle(Theme.lilac2)
            .padding(.vertical, 3)
            .padding(.horizontal, 8)
            .background(Theme.brand.opacity(0.16), in: RoundedRectangle(cornerRadius: 7, style: .continuous))
    }

    func labelMono(size: CGFloat = 10.5) -> some View {
        modifier(LabelMono(size: size))
    }

    func field(focused: Bool = false, invalid: Bool = false) -> some View {
        modifier(Field(focused: focused, invalid: invalid))
    }

    /// O texto do erro logo abaixo do campo que errou: `mt-1.5 text-[11.5px] text-danger`.
    @ViewBuilder
    func fieldError(_ message: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            self

            if !message.isEmpty {
                Text(message)
                    .font(Theme.sans(11.5))
                    .foregroundStyle(Theme.danger)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

extension Color {
    /// `#rrggbb`, que é como o Laravel guarda a cor de um cargo.
    var hexText: String {
        let color = NSColor(self).usingColorSpace(.sRGB) ?? .white

        return String(format: "#%02x%02x%02x", Int(color.redComponent * 255), Int(color.greenComponent * 255), Int(color.blueComponent * 255))
    }

    init(hex: UInt32) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: 1
        )
    }
}

/// O cartão de vidro com o mesmo respiro do React (`p-8`, `max-w-[420px]`): é a caixa que
/// toda tela usa para o seu conteúdo principal.
struct GlassCard<Content: View>: View {
    var width: CGFloat = 420
    @ViewBuilder var content: Content

    var body: some View {
        VStack(spacing: 0) {
            content
        }
        .padding(32)
        .frame(maxWidth: width)
        .glass(radius: 22, shadowed: true)
    }
}
