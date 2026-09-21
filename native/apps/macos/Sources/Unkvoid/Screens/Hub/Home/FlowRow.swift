import SwiftUI

/// As fichas das salas recentes quebram linha quando não cabem — é o `flex-wrap` do React.
struct FlowRow: Layout {
    var spacing: CGFloat = 8

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        let rows = lines(subviews, within: width)
        let height = rows.reduce(0) { $0 + $1.height + spacing } - (rows.isEmpty ? 0 : spacing)

        return CGSize(width: width == .infinity ? rows.map(\.width).max() ?? 0 : width, height: max(0, height))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY

        for row in lines(subviews, within: bounds.width) {
            var x = bounds.minX

            for index in row.items {
                let size = subviews[index].sizeThatFits(.unspecified)

                subviews[index].place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
                x += size.width + spacing
            }

            y += row.height + spacing
        }
    }

    private struct Line {
        var items: [Int] = []
        var width: CGFloat = 0
        var height: CGFloat = 0
    }

    private func lines(_ subviews: Subviews, within width: CGFloat) -> [Line] {
        var rows: [Line] = []
        var current = Line()

        for index in subviews.indices {
            let size = subviews[index].sizeThatFits(.unspecified)

            if !current.items.isEmpty, current.width + spacing + size.width > width {
                rows.append(current)
                current = Line()
            }

            current.width += (current.items.isEmpty ? 0 : spacing) + size.width
            current.height = max(current.height, size.height)
            current.items.append(index)
        }

        return current.items.isEmpty ? rows : rows + [current]
    }
}
