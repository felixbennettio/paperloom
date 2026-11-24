import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Lays views out in rows and starts a new row when one is full, so a
/// toolbar in a narrow pane wraps instead of losing its ends.
struct FlowLayout: Layout {
    var spacing: CGFloat = 6
    var lineSpacing: CGFloat = 6

    private struct Row {
        var indices: [Int] = []
        var width: CGFloat = 0
        var height: CGFloat = 0
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        // SwiftUI asks for the smallest size with a zero width; answering
        // with one item a row would make the window taller than the screen,
        // so that question gets the one-row answer and a width of zero.
        let offered = proposal.width ?? .infinity
        let rows = arrange(width: offered > 0 ? offered : .infinity, subviews: subviews)
        let height = rows.map(\.height).reduce(0, +) + lineSpacing * CGFloat(max(0, rows.count - 1))
        let widest = rows.map(\.width).max() ?? 0
        return CGSize(width: offered.isFinite ? offered : widest, height: height)
    }

    /// A view's natural size, narrowed to `width` when it is wider than a
    /// whole row (a long label then wraps or truncates).
    private func size(of subview: LayoutSubview, width: CGFloat) -> CGSize {
        let natural = subview.sizeThatFits(.unspecified)
        guard natural.width > width, width.isFinite else { return natural }
        return subview.sizeThatFits(ProposedViewSize(width: width, height: nil))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for row in arrange(width: bounds.width, subviews: subviews) {
            var x = bounds.minX
            for index in row.indices {
                let size = size(of: subviews[index], width: bounds.width)
                subviews[index].place(
                    at: CGPoint(x: x, y: y + (row.height - size.height) / 2), proposal: ProposedViewSize(size))
                x += size.width + spacing
            }
            y += row.height + lineSpacing
        }
    }

    private func arrange(width: CGFloat, subviews: Subviews) -> [Row] {
        var rows: [Row] = []
        var row = Row()
        for index in subviews.indices {
            let size = size(of: subviews[index], width: width)
            let needed = row.indices.isEmpty ? size.width : row.width + spacing + size.width
            if needed > width && !row.indices.isEmpty {
                rows.append(row)
                row = Row(indices: [index], width: size.width, height: size.height)
            } else {
                row.indices.append(index)
                row.width = needed
                row.height = max(row.height, size.height)
            }
        }
        if !row.indices.isEmpty { rows.append(row) }
        return rows
    }
}

/// The divider beside a pane, dragged sideways to make the pane wider or
/// narrower; `width` stays within `range`.
struct PaneResizeHandle: View {
    @Binding var width: Double
    let range: ClosedRange<Double>
    /// The pane is before the handle, so dragging towards the trailing
    /// edge widens it; otherwise dragging towards the leading edge does.
    let paneBefore: Bool
    @State private var start: Double?

    var body: some View {
        Divider()
            // Wider on iOS, where a finger rather than a pointer grabs it.
            #if os(iOS)
            .frame(width: 14)
            #else
            .frame(width: 7)
            #endif
            .contentShape(Rectangle())
            #if os(macOS)
            .pointerStyle(.columnResize)
            #endif
            .gesture(
                DragGesture(minimumDistance: 1, coordinateSpace: .global)
                    .onChanged { value in
                        let base = start ?? width
                        if start == nil { start = width }
                        let delta = Double(value.translation.width) * (paneBefore ? 1 : -1)
                        width = min(range.upperBound, max(range.lowerBound, base + delta))
                    }
                    .onEnded { _ in start = nil })
            .accessibilityLabel(Text("Resize"))
            .accessibilityValue(Text(verbatim: "\(Int(width))"))
            .accessibilityAdjustableAction { direction in
                let step = (range.upperBound - range.lowerBound) / 10
                switch direction {
                case .increment: width = min(range.upperBound, width + step)
                case .decrement: width = max(range.lowerBound, width - step)
                @unknown default: break
                }
            }
    }
}

extension View {
    /// A control big enough to touch: Apple asks for 44 points on iOS. On
    /// the Mac, where the pointer is precise, the control is unchanged.
    func touchTarget(width: CGFloat = 44, height: CGFloat = 44) -> some View {
        #if os(iOS)
        return frame(minWidth: width, minHeight: height).contentShape(Rectangle())
        #else
        return self
        #endif
    }
}

/// A filled circle in a colour. Menus draw symbol images in a single tint,
/// so colour choices show this picture, which keeps its colour.
func colorDot(_ color: PlatformColor, size: CGFloat = 12) -> Image {
    #if os(macOS)
    let image = NSImage(size: NSSize(width: size, height: size), flipped: false) { rect in
        color.setFill()
        NSBezierPath(ovalIn: rect.insetBy(dx: 0.5, dy: 0.5)).fill()
        NSColor.black.withAlphaComponent(0.25).setStroke()
        NSBezierPath(ovalIn: rect.insetBy(dx: 0.5, dy: 0.5)).stroke()
        return true
    }
    image.isTemplate = false
    return Image(nsImage: image)
    #else
    let image = UIGraphicsImageRenderer(size: CGSize(width: size, height: size)).image { _ in
        let oval = UIBezierPath(ovalIn: CGRect(x: 0.5, y: 0.5, width: size - 1, height: size - 1))
        color.setFill()
        oval.fill()
        UIColor.black.withAlphaComponent(0.25).setStroke()
        oval.stroke()
    }
    return Image(uiImage: image.withRenderingMode(.alwaysOriginal))
    #endif
}
