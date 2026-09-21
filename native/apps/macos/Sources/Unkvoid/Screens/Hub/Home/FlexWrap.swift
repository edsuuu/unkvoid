import SwiftUI

/// O `flex flex-wrap` do CSS, com o que a Home do React usa dele: cada cartão tem uma base,
/// um mínimo e um peso para crescer; a linha quebra quando as bases não cabem, o que sobra
/// na linha é repartido pelo peso, e todos os cartões de uma linha ficam da mesma altura.
struct FlexWrap: Layout {
    var spacing: CGFloat = 12

    struct Item: LayoutValueKey {
        static let defaultValue = Flex(basis: 0, grow: 1, minimum: 0)
    }

    struct Flex: Equatable {
        var basis: CGFloat
        var grow: CGFloat
        var minimum: CGFloat

        var hypothetical: CGFloat {
            max(basis, minimum)
        }
    }

    private struct Line {
        var items: [Int] = []
        var widths: [CGFloat] = []
        var height: CGFloat = 0
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? 1000
        let lines = lines(subviews, within: width)

        return CGSize(width: width, height: lines.reduce(0) { $0 + $1.height } + spacing * CGFloat(max(0, lines.count - 1)))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY

        for line in lines(subviews, within: bounds.width) {
            var x = bounds.minX

            for (index, width) in zip(line.items, line.widths) {
                subviews[index].place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(width: width, height: line.height))
                x += width + spacing
            }

            y += line.height + spacing
        }
    }

    private func lines(_ subviews: Subviews, within width: CGFloat) -> [Line] {
        var lines: [Line] = []
        var current: [Int] = []
        var used: CGFloat = 0

        for index in subviews.indices {
            let wanted = subviews[index][Item.self].hypothetical

            if !current.isEmpty, used + spacing + wanted > width {
                lines.append(settle(current, subviews, within: width))
                current = []
                used = 0
            }

            used += (current.isEmpty ? 0 : spacing) + wanted
            current.append(index)
        }

        return current.isEmpty ? lines : lines + [settle(current, subviews, within: width)]
    }

    /// Reparte o que sobra da linha pelo peso de cada cartão, e mede a altura com a largura final.
    private func settle(_ items: [Int], _ subviews: Subviews, within width: CGFloat) -> Line {
        let flexes = items.map { subviews[$0][Item.self] }
        let free = width - flexes.reduce(0) { $0 + $1.hypothetical } - spacing * CGFloat(items.count - 1)
        let weight = flexes.reduce(0) { $0 + $1.grow }

        let widths = flexes.map { flex in
            let grown = free > 0 && weight > 0 ? flex.hypothetical + free * flex.grow / weight : flex.hypothetical

            // Um cartão sozinho numa linha estreita encolhe até o mínimo dele, como o `flex-shrink`.
            return items.count == 1 ? min(grown, max(flex.minimum, width)) : grown
        }

        let height = zip(items, widths).map { subviews[$0].sizeThatFits(ProposedViewSize(width: $1, height: nil)).height }.max() ?? 0

        return Line(items: items, widths: widths, height: height)
    }
}

extension View {
    /// `flex: <grow> 1 <basis>` com `min-width`.
    func flex(basis: CGFloat, grow: CGFloat, minimum: CGFloat) -> some View {
        layoutValue(key: FlexWrap.Item.self, value: FlexWrap.Flex(basis: basis, grow: grow, minimum: minimum))
    }
}
