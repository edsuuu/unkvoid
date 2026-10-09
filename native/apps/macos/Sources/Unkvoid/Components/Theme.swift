import AppKit
import SwiftUI

/// Os tokens do desenho, no molde do Discord: cada cor tem um papel, e cada papel tem um valor
/// no tema escuro e um no claro (nota "Unkvoid - réplica do Discord", §4.2). O Windows (Slint)
/// e o Linux (GTK) copiam estes nomes e valores, um por um — divergir aqui é o app parecer
/// outro programa em cada sistema.
///
/// A cor segue a aparência do sistema (`NSColor` dinâmico). O app ainda força o tema escuro
/// no `RootView`; o claro já está aqui para a opção "Aparência" ligar depois.
enum Theme {
    // Superfícies: a separação entre as colunas é só a cor, sem vão e sem borda.
    static let surfaceRail = dynamic(0x1E1F22, 0xE3E5E8)
    static let surfaceSide = dynamic(0x2B2D31, 0xF2F3F5)
    static let surfaceChat = dynamic(0x313338, 0xFFFFFF)
    static let surfacePanel = dynamic(0x232428, 0xEBEDEF)
    static let surfaceFloat = dynamic(0x111214, 0xFFFFFF)
    static let surfaceInput = dynamic(0x383A40, 0xEBEDEF)
    static let surfaceCall = Color.black
    static let surfaceTile = Color(hex: 0x1E1F22)
    static let hover = dynamic(0x4E5058, 0.30, 0x747F8D, 0.16)
    static let selected = dynamic(0x4E5058, 0.60, 0x747F8D, 0.24)
    static let line = dynamic(0x4E5058, 0.48, 0x4F545C, 0.16)

    // Tinta, do título ao desabilitado.
    static let inkStrong = dynamic(0xF2F3F5, 0x060607)
    static let ink = dynamic(0xDBDEE1, 0x313338)
    static let inkSoft = dynamic(0xB5BAC1, 0x4E5058)
    static let inkDim = dynamic(0x949BA4, 0x5C5E66)
    static let inkGhost = dynamic(0x4E5058, 0xC4C9CE)

    // O destaque é o violeta do Unkvoid, e não o azul do Discord: a identidade fica.
    static let brand = Color(hex: 0x6A55E0)
    static let brandHover = Color(hex: 0x5A3FD6)
    static let brandText = dynamic(0xA89BFF, 0x5A3FD6)
    static let online = Color(hex: 0x23A55A)
    static let idle = Color(hex: 0xF0B232)
    /// O sinal de voz fraco, entre o amarelo do "ausente" e o vermelho do perigo.
    static let poor = Color(hex: 0xFB923C)
    static let danger = dynamic(0xF23F43, 0xDA373C)
    static let dangerFill = Color(hex: 0xDA373C)
    static let offline = Color(hex: 0x80848E)
    static let live = Color(hex: 0xED4245)

    /// As medidas das colunas e das linhas, em pontos.
    enum Size {
        static let rail: CGFloat = 72
        static let side: CGFloat = 240
        static let members: CGFloat = 240
        static let voiceChat: CGFloat = 360
        static let header: CGFloat = 48
        static let userBar: CGFloat = 52
        static let row: CGFloat = 32
        static let voiceRow: CGFloat = 30
        static let memberRow: CGFloat = 42
        static let serverIcon: CGFloat = 48
        static let control: CGFloat = 56
        static let radius: CGFloat = 4
        static let radiusLarge: CGFloat = 8
    }

    /// A fonte do sistema de cada plataforma (aqui a SF). Papéis, em tamanho e peso.
    static func sans(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        .system(size: size, weight: weight)
    }

    static func mono(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        .system(size: size, weight: weight, design: .monospaced)
    }

    static let meta = sans(12, .medium)
    static let button = sans(14, .medium)
    static let list = sans(16, .medium)
    static let voicePerson = sans(14, .medium)
    static let message = sans(16)
    static let header = sans(16, .semibold)
    static let title = sans(20, .bold)
    static let welcome = sans(32, .heavy)

    /// A cor que o Laravel manda para um cargo, no formato `#rrggbb`. Sem cor, sem cor.
    static func hex(_ value: String?) -> Color? {
        guard let value, value.hasPrefix("#"), let number = UInt32(value.dropFirst(), radix: 16), value.count == 7 else {
            return nil
        }

        return Color(hex: number)
    }

    private static func dynamic(_ dark: UInt32, _ light: UInt32) -> Color {
        dynamic(dark, 1, light, 1)
    }

    private static func dynamic(_ dark: UInt32, _ darkAlpha: CGFloat, _ light: UInt32, _ lightAlpha: CGFloat) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua

            return NSColor(hex: isDark ? dark : light, alpha: isDark ? darkAlpha : lightAlpha)
        })
    }
}

/// A superfície lateral (`surfaceSide`) com o raio dos cartões. Onde o app tinha vidro, agora
/// tem uma cor chapada: a separação é pela superfície, como no Discord.
struct Surface: ViewModifier {
    func body(content: Content) -> some View {
        content.background(Theme.surfaceSide, in: RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous))
    }
}

/// O rótulo de categoria e de grupo: 12, caixa alta, `inkDim`. O nome ficou de quando ele
/// era monoespaçado; é o que o código inteiro usa.
struct LabelMono: ViewModifier {
    var size: CGFloat = 12

    func body(content: Content) -> some View {
        content
            .font(Theme.sans(size, .semibold))
            .tracking(size * 0.02)
            .textCase(.uppercase)
            .foregroundStyle(Theme.inkDim)
    }
}

/// O campo de texto: `surfaceInput`, raio 8, sem borda. A borda só aparece para dizer que o
/// campo errou (vermelho) ou que tem o foco do teclado.
struct Field: ViewModifier {
    var focused: Bool = false
    var invalid: Bool = false

    func body(content: Content) -> some View {
        content
            .textFieldStyle(.plain)
            .font(Theme.sans(14))
            .foregroundStyle(Theme.inkStrong)
            .padding(.vertical, 10)
            .padding(.horizontal, 12)
            .background(Theme.surfaceInput, in: RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous)
                    .strokeBorder(border, lineWidth: 1)
            )
            .animation(.easeOut(duration: 0.1), value: focused)
            .animation(.easeOut(duration: 0.1), value: invalid)
    }

    private var border: Color {
        if invalid {
            return Theme.danger
        }

        return focused ? Theme.brand : .clear
    }
}

/// O botão primário: o violeta chapado, texto branco, raio 4.
struct PrimaryButton: ButtonStyle {
    /// No rodapé de um modal o botão tem o tamanho do texto; num formulário ele ocupa a linha.
    var wide = true
    var font: Font = Theme.button

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(font)
            .foregroundStyle(.white)
            .frame(maxWidth: wide ? .infinity : nil)
            .padding(.vertical, wide ? 12 : 8)
            .padding(.horizontal, 16)
            .background(configuration.isPressed ? Theme.brandHover : Theme.brand, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
            .animation(.easeOut(duration: 0.1), value: configuration.isPressed)
            .pointerCursor()
    }
}

/// O botão do que apaga, expulsa ou bane: vermelho chapado, texto branco.
struct DangerButton: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Theme.button)
            .foregroundStyle(.white)
            .padding(.vertical, 8)
            .padding(.horizontal, 16)
            .background(Theme.dangerFill.opacity(configuration.isPressed ? 0.85 : 1), in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
            .pointerCursor()
    }
}

/// O `.plain` do sistema com a mãozinha do cursor.
struct PointerButton: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .opacity(configuration.isPressed ? 0.75 : 1)
            .pointerCursor()
    }
}

extension ButtonStyle where Self == PointerButton {
    static var pointer: PointerButton { PointerButton() }
}

extension View {
    /// A mãozinha sobre o que clica. O macOS 14 não tem `pointerStyle`: é o `NSCursor` na
    /// entrada e na saída do mouse. `set`, e não `push`/`pop`: o botão que some debaixo do
    /// mouse (um modal que fecha) nunca avisa a saída, e a pilha ficaria com a mão para sempre.
    func pointerCursor() -> some View {
        onHover { inside in
            (inside ? NSCursor.pointingHand : NSCursor.arrow).set()
        }
    }
}

/// O botão secundário: o cinza do `hover`, texto `ink`. "Cancelar" e o que não é a ação principal.
struct GhostButton: ButtonStyle {
    var font: Font = Theme.button
    var padding = EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16)

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(font)
            .foregroundStyle(Theme.ink)
            .padding(padding)
            .background(configuration.isPressed ? Theme.selected : Theme.hover, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
            .pointerCursor()
    }
}

/// O botão só de ícone, de 32, sem moldura: `inkSoft` em repouso, violeta quando ligado
/// (`on`) e vermelho quando é o desligado de algo (`off`).
struct IconButton: ButtonStyle {
    enum Tone {
        case idle
        case on
        case off
    }

    var side: CGFloat = 32
    var radius: CGFloat = Theme.Size.radius
    var tone: Tone = .idle

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundStyle(tone == .off ? Theme.danger : tone == .on ? .white : Theme.inkSoft)
            .frame(width: side, height: side)
            .background(background(pressed: configuration.isPressed), in: RoundedRectangle(cornerRadius: radius, style: .continuous))
            .animation(.easeOut(duration: 0.1), value: configuration.isPressed)
            .pointerCursor()
    }

    private func background(pressed: Bool) -> Color {
        switch tone {
        case .idle: pressed ? Theme.selected : .clear
        case .on: pressed ? Theme.brandHover : Theme.brand
        case .off: pressed ? Theme.selected : .clear
        }
    }
}

/// A linha clicável das listas (canal, membro, cargo): raio 4, o `hover` sob o mouse e o
/// `selected` na que está aberta.
struct RowItem: ViewModifier {
    var selected = false

    @State private var hovering = false

    func body(content: Content) -> some View {
        content
            .padding(.vertical, 6)
            .padding(.horizontal, 8)
            .background(
                selected ? Theme.selected : hovering ? Theme.hover : .clear,
                in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous)
            )
            .onHover { hovering = $0 }
            .animation(.easeOut(duration: 0.1), value: hovering)
    }
}

extension View {
    func rowItem(selected: Bool = false) -> some View {
        modifier(RowItem(selected: selected))
    }

    func surface() -> some View {
        modifier(Surface())
    }

    /// O cartão de um modal: `surfaceChat`, raio 8 e a sombra funda.
    func modalPanel() -> some View {
        background(Theme.surfaceChat, in: RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous))
            .shadow(color: .black.opacity(0.15), radius: 0.5)
            .shadow(color: .black.opacity(0.4), radius: 12, x: 0, y: 8)
    }

    /// O painel que flutua: menu, tooltip, popover. `surfaceFloat`, raio 8, sombra curta.
    func popoverPanel() -> some View {
        background(Theme.surfaceFloat, in: RoundedRectangle(cornerRadius: Theme.Size.radiusLarge, style: .continuous))
            .shadow(color: .black.opacity(0.24), radius: 8, x: 0, y: 8)
    }

    /// O código da sala e o do convite, para copiar.
    func codeChip(size: CGFloat = 13) -> some View {
        font(Theme.mono(size))
            .foregroundStyle(Theme.brandText)
            .padding(.vertical, 3)
            .padding(.horizontal, 8)
            .background(Theme.surfaceInput, in: RoundedRectangle(cornerRadius: Theme.Size.radius, style: .continuous))
    }

    func labelMono(size: CGFloat = 12) -> some View {
        modifier(LabelMono(size: size))
    }

    func field(focused: Bool = false, invalid: Bool = false) -> some View {
        modifier(Field(focused: focused, invalid: invalid))
    }

    /// O texto do erro logo abaixo do campo que errou.
    @ViewBuilder
    func fieldError(_ message: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            self

            if !message.isEmpty {
                Text(message)
                    .font(Theme.meta)
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

        return String(format: "#%02x%02x%02x", Int(round(color.redComponent * 255)), Int(round(color.greenComponent * 255)), Int(round(color.blueComponent * 255)))
    }

    init(hex: UInt32) {
        self.init(nsColor: NSColor(hex: hex, alpha: 1))
    }
}

extension NSColor {
    convenience init(hex: UInt32, alpha: CGFloat) {
        self.init(
            srgbRed: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            alpha: alpha
        )
    }
}

/// O cartão de uma tela inteira (entrada, sem conexão): `surfaceSide`, 32 de respiro, 420 de largura.
struct SurfaceCard<Content: View>: View {
    var width: CGFloat = 420
    @ViewBuilder var content: Content

    var body: some View {
        VStack(spacing: 0) {
            content
        }
        .padding(32)
        .frame(maxWidth: width)
        .surface()
    }
}
