import SwiftUI

/// Os mesmos ícones de `native/apps/desktop/ui/components/common/Icon.tsx`.
///
/// O `d` de cada um foi copiado letra por letra do `.tsx`, e não redesenhado à mão. Um
/// desenho refeito diverge do outro app no primeiro ajuste; uma string copiada se corrige
/// com um `cmd+C`. Todos vivem na mesma caixa de 24×24 e têm traço de 1.8, como lá.
enum IconName: CaseIterable {
    case arrowLeft
    case camera
    case cameraOff
    case chat
    case check
    case chevronDown
    case close
    case copy
    case crown
    case dots
    case edit
    case eye
    case focus
    case fullscreen
    case gear
    case grid
    case headphones
    case headphonesOff
    case home
    case logout
    case logs
    case menu
    case mic
    case micOff
    case phoneOff
    case play
    case plus
    case screen
    case signal
    case sliders
    case speaker
    case speakerOff
    case trash
    case users
}

struct Icon: View {
    let name: IconName
    var size: CGFloat = 16

    /// A barra do "desligado", igual à constante `SLASH` do `.tsx`.
    private static let slash = "M4 4l16 16"

    /// O `strokeWidth="1.8"` do `.tsx` vale dentro do `viewBox` de 24, então ele encolhe
    /// junto com o ícone. Fixar 1.8 em pontos deixaria o ícone de 12 parecendo uma mancha.
    private var lineWidth: CGFloat {
        1.8 * size / 24
    }

    var body: some View {
        let parts = Self.parts(of: name)

        ZStack {
            ForEach(parts.indices, id: \.self) { index in
                let part = parts[index]
                let shape = IconShape(part: part)

                // Forma sem cor explícita pinta com o `foregroundStyle` de quem a contém —
                // que é o `currentColor` do SVG.
                if part.filled {
                    shape
                } else {
                    shape.stroke(style: StrokeStyle(lineWidth: lineWidth, lineCap: .round, lineJoin: .round))
                }
            }
        }
        .frame(width: size, height: size)
        .rotationEffect(.degrees(name == .phoneOff ? 135 : 0))
    }

    enum Part {
        case stroke(String)
        case fill(String)
        case rect(CGFloat, CGFloat, CGFloat, CGFloat, CGFloat, filled: Bool)
        case circle(CGFloat, CGFloat, CGFloat, filled: Bool)

        var filled: Bool {
            switch self {
            case .stroke: false
            case .fill: true
            case let .rect(_, _, _, _, _, filled): filled
            case let .circle(_, _, _, filled): filled
            }
        }
    }

    static func parts(of name: IconName) -> [Part] {
        switch name {
        case .arrowLeft:
            [.stroke("M19 12H5M11 6l-6 6 6 6")]
        case .camera:
            [.rect(3, 6, 13, 12, 2.5, filled: false), .stroke("M16 10.5l5-3v9l-5-3z")]
        case .cameraOff:
            [.rect(3, 6, 13, 12, 2.5, filled: false), .stroke("M16 10.5l5-3v9l-5-3z"), .stroke(slash)]
        case .chat:
            [.stroke("M4 5h16v11H9l-4 4v-4H4z")]
        case .check:
            [.stroke("M5 12l5 5L19 7")]
        case .chevronDown:
            [.stroke("M7 10l5 5 5-5")]
        case .close:
            [.stroke("M6 6l12 12M18 6L6 18")]
        case .copy:
            [.rect(8, 8, 12, 12, 2, filled: false), .stroke("M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3")]
        case .crown:
            [.stroke("M4 18h16M4 8l4 4 4-7 4 7 4-4-2 10H6z")]
        case .dots:
            [.circle(6, 12, 1.3, filled: true), .circle(12, 12, 1.3, filled: true), .circle(18, 12, 1.3, filled: true)]
        case .edit:
            [.stroke("M16.5 3.5l4 4L8 20H4v-4z")]
        case .eye:
            [.stroke("M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z"), .circle(12, 12, 3, filled: false)]
        case .focus:
            [.rect(3, 5, 18, 14, 2, filled: false), .rect(9.5, 9.5, 5, 5, 0, filled: true)]
        case .fullscreen:
            [.stroke("M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5")]
        case .gear:
            [
                .stroke("M10.3 4.3a1 1 0 0 1 1-.8h1.4a1 1 0 0 1 1 .8l.3 1.6a7 7 0 0 1 1.7 1l1.5-.6a1 1 0 0 1 1.2.4l.7 1.2a1 1 0 0 1-.2 1.3l-1.2 1a7 7 0 0 1 0 2l1.2 1a1 1 0 0 1 .2 1.3l-.7 1.2a1 1 0 0 1-1.2.4l-1.5-.6a7 7 0 0 1-1.7 1l-.3 1.6a1 1 0 0 1-1 .8h-1.4a1 1 0 0 1-1-.8l-.3-1.6a7 7 0 0 1-1.7-1l-1.5.6a1 1 0 0 1-1.2-.4l-.7-1.2a1 1 0 0 1 .2-1.3l1.2-1a7 7 0 0 1 0-2l-1.2-1a1 1 0 0 1-.2-1.3l.7-1.2a1 1 0 0 1 1.2.4l1.5-.6a7 7 0 0 1 1.7-1z"),
                .circle(12, 12, 2.5, filled: false),
            ]
        case .grid:
            [
                .rect(4, 4, 7, 7, 1.2, filled: false),
                .rect(13, 4, 7, 7, 1.2, filled: false),
                .rect(4, 13, 7, 7, 1.2, filled: false),
                .rect(13, 13, 7, 7, 1.2, filled: false),
            ]
        case .headphones:
            [.stroke("M4 15v-3a8 8 0 0 1 16 0v3"), .rect(3, 14, 4, 6, 1.5, filled: false), .rect(17, 14, 4, 6, 1.5, filled: false)]
        case .headphonesOff:
            [.stroke("M4 15v-3a8 8 0 0 1 16 0v3"), .rect(3, 14, 4, 6, 1.5, filled: false), .rect(17, 14, 4, 6, 1.5, filled: false), .stroke(slash)]
        case .home:
            [.stroke("M4 11l8-7 8 7M6 10v10h12V10M10 20v-5h4v5")]
        case .logout:
            [.stroke("M15 4h4v16h-4M10 8l-4 4 4 4M6 12h11")]
        case .logs:
            [.stroke("M6 4h9l3 3v13H6zM9 10h6M9 14h6M9 18h4")]
        case .menu:
            [.stroke("M5 7h14M5 12h14M5 17h14")]
        case .mic:
            [.rect(9, 3, 6, 11, 3, filled: false), .stroke("M5 11a7 7 0 0 0 14 0M12 18v3M8.5 21h7")]
        case .micOff:
            [.rect(9, 3, 6, 11, 3, filled: false), .stroke("M5 11a7 7 0 0 0 14 0M12 18v3M8.5 21h7"), .stroke(slash)]
        case .phoneOff:
            [.fill("M6.6 10.8c1.2 2.4 3.2 4.4 5.6 5.6l2-2c.3-.3.7-.4 1-.2 1.1.4 2.3.6 3.5.6.6 0 1 .4 1 1V19c0 .6-.4 1-1 1-8.3 0-15-6.7-15-15 0-.6.4-1 1-1h3.2c.6 0 1 .4 1 1 0 1.2.2 2.4.6 3.5.1.4 0 .8-.3 1l-1.6 1.3z")]
        case .play:
            [.fill("M8 5v14l11-7z")]
        case .plus:
            [.stroke("M12 5v14M5 12h14")]
        case .screen:
            [.rect(3, 4, 18, 12, 2, filled: false), .stroke("M8 20h8M12 16v4")]
        case .signal:
            [.stroke("M5 19v-3M10 19v-7M15 19v-11M20 19V5")]
        case .sliders:
            [
                .stroke("M4 6h16M4 12h16M4 18h16"),
                .circle(9, 6, 2, filled: true),
                .circle(15, 12, 2, filled: true),
                .circle(8, 18, 2, filled: true),
            ]
        case .speaker:
            [.stroke("M4 9h4l5-4v14l-5-4H4z"), .stroke("M17 9a4 4 0 0 1 0 6")]
        case .speakerOff:
            [.stroke("M4 9h4l5-4v14l-5-4H4z"), .stroke("M17 9l5 6M22 9l-5 6")]
        case .trash:
            [.stroke("M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13")]
        case .users:
            [.circle(9, 8, 3.5, filled: false), .stroke("M3 20a6 6 0 0 1 12 0M16 4.5a3.5 3.5 0 0 1 0 7M21 20a6 6 0 0 0-4-5.6")]
        }
    }
}

/// Um `d` de SVG desenhado na caixa que lhe deram. O `viewBox` é quadrado em tudo que o
/// projeto desenha — 24 nos ícones, 48 no "G" do Google.
struct VectorPath: Shape {
    let commands: String
    var viewBox: CGFloat = 24

    func path(in rect: CGRect) -> Path {
        let scale = min(rect.width, rect.height) / viewBox

        return SvgPath.parse(commands).applying(CGAffineTransform(scaleX: scale, y: scale))
    }
}

/// Uma parte do ícone desenhada na caixa que lhe deram, saindo sempre do `viewBox` de 24.
struct IconShape: Shape {
    let part: Icon.Part

    func path(in rect: CGRect) -> Path {
        let scale = min(rect.width, rect.height) / 24

        switch part {
        case let .stroke(commands), let .fill(commands):
            return VectorPath(commands: commands).path(in: rect)
        case let .rect(x, y, width, height, radius, _):
            return Path(
                roundedRect: CGRect(x: x * scale, y: y * scale, width: width * scale, height: height * scale),
                cornerRadius: radius * scale,
                style: .continuous
            )
        case let .circle(x, y, radius, _):
            return Path(ellipseIn: CGRect(
                x: (x - radius) * scale,
                y: (y - radius) * scale,
                width: radius * 2 * scale,
                height: radius * 2 * scale
            ))
        }
    }
}

/// O pedaço de SVG que estes ícones usam, e só ele: `M m L l H h V v C c S s A a Z z`.
///
/// ponytail: não é um parser de SVG. Teto: número em notação científica, arco elíptico
/// (`rx != ry`) e flag de arco colada no número seguinte (`0 011 1`) não são lidos — nada
/// disso aparece no `Icon.tsx`, e o teste `everyIconFitsItsBox` quebra se aparecer. Saída,
/// se um dia precisar: trocar por um parser de verdade, com a mesma assinatura.
enum SvgPath {
    static func parse(_ commands: String) -> Path {
        var path = Path()
        var cursor = CGPoint.zero
        var subpathStart = CGPoint.zero
        var lastControl: CGPoint?
        var letter: Character = "M"
        var tokens = tokenize(commands)[...]

        func number() -> CGFloat {
            guard case let .number(value)? = tokens.first else {
                return 0
            }

            tokens = tokens.dropFirst()

            return value
        }

        func point(relative: Bool) -> CGPoint {
            let x = number()
            let y = number()

            return relative ? CGPoint(x: cursor.x + x, y: cursor.y + y) : CGPoint(x: x, y: y)
        }

        while let token = tokens.first {
            if case let .letter(next) = token {
                letter = next
                tokens = tokens.dropFirst()

                if next == "z" || next == "Z" {
                    path.closeSubpath()
                    cursor = subpathStart
                    lastControl = nil
                }

                continue
            }

            let relative = letter.isLowercase

            switch Character(letter.lowercased()) {
            case "m":
                cursor = point(relative: relative)
                path.move(to: cursor)
                subpathStart = cursor
                lastControl = nil
                // Um par a mais depois de um `M` é uma linha, não outro `move`.
                letter = relative ? "l" : "L"
            case "l":
                cursor = point(relative: relative)
                path.addLine(to: cursor)
                lastControl = nil
            case "h":
                let x = number()

                cursor = CGPoint(x: relative ? cursor.x + x : x, y: cursor.y)
                path.addLine(to: cursor)
                lastControl = nil
            case "v":
                let y = number()

                cursor = CGPoint(x: cursor.x, y: relative ? cursor.y + y : y)
                path.addLine(to: cursor)
                lastControl = nil
            case "c":
                let first = point(relative: relative)
                let second = point(relative: relative)
                let end = point(relative: relative)

                path.addCurve(to: end, control1: first, control2: second)
                cursor = end
                lastControl = second
            case "s":
                let mirrored = lastControl.map { CGPoint(x: cursor.x * 2 - $0.x, y: cursor.y * 2 - $0.y) } ?? cursor
                let second = point(relative: relative)
                let end = point(relative: relative)

                path.addCurve(to: end, control1: mirrored, control2: second)
                cursor = end
                lastControl = second
            case "a":
                let radius = number()

                _ = number()
                _ = number()

                let largeArc = number() != 0
                let sweep = number() != 0
                let end = point(relative: relative)

                addArc(&path, from: cursor, to: end, radius: radius, largeArc: largeArc, sweep: sweep)
                cursor = end
                lastControl = nil
            default:
                tokens = tokens.dropFirst()
            }
        }

        return path
    }

    /// Só arco circular: todo `a` do `Icon.tsx` tem `rx == ry`. A conversão é a do padrão
    /// SVG (centro a partir dos dois extremos), fatiada em pedaços de até 90° porque é aí
    /// que uma Bézier cúbica ainda descreve um arco sem erro visível.
    private static func addArc(_ path: inout Path, from start: CGPoint, to end: CGPoint, radius: CGFloat, largeArc: Bool, sweep: Bool) {
        let halfX = (start.x - end.x) / 2
        let halfY = (start.y - end.y) / 2
        let square = halfX * halfX + halfY * halfY

        guard square > 0 else {
            return
        }

        let grown = max(radius, sqrt(square))
        let factor = sqrt(max(0, grown * grown - square) / square) * (largeArc == sweep ? -1 : 1)
        let center = CGPoint(
            x: factor * halfY + (start.x + end.x) / 2,
            y: -factor * halfX + (start.y + end.y) / 2
        )
        let from = atan2(start.y - center.y, start.x - center.x)
        let to = atan2(end.y - center.y, end.x - center.x)
        var sweepAngle = to - from

        if !sweep, sweepAngle > 0 {
            sweepAngle -= 2 * .pi
        }

        if sweep, sweepAngle < 0 {
            sweepAngle += 2 * .pi
        }

        let steps = max(1, Int(ceil(abs(sweepAngle) / (.pi / 2))))
        let step = sweepAngle / CGFloat(steps)
        let pull = 4.0 / 3.0 * tan(step / 4) * grown

        for index in 0 ..< steps {
            let begin = from + step * CGFloat(index)
            let finish = begin + step
            let head = CGPoint(x: center.x + grown * cos(begin), y: center.y + grown * sin(begin))
            let tail = CGPoint(x: center.x + grown * cos(finish), y: center.y + grown * sin(finish))

            path.addCurve(
                to: tail,
                control1: CGPoint(x: head.x - pull * sin(begin), y: head.y + pull * cos(begin)),
                control2: CGPoint(x: tail.x + pull * sin(finish), y: tail.y - pull * cos(finish))
            )
        }
    }

    private enum Token {
        case letter(Character)
        case number(CGFloat)
    }

    private static func tokenize(_ commands: String) -> [Token] {
        var tokens: [Token] = []
        let characters = Array(commands)
        var index = 0

        while index < characters.count {
            let character = characters[index]

            if character.isLetter {
                tokens.append(.letter(character))
                index += 1

                continue
            }

            guard character == "-" || character == "." || character.isNumber else {
                index += 1

                continue
            }

            var text = ""

            if character == "-" {
                text.append(character)
                index += 1
            }

            // Um segundo ponto começa outro número: em `1.5.5` estão 1.5 e .5.
            while index < characters.count, characters[index].isNumber || (characters[index] == "." && !text.contains(".")) {
                text.append(characters[index])
                index += 1
            }

            if let value = Double(text) {
                tokens.append(.number(CGFloat(value)))
            }
        }

        return tokens
    }
}
