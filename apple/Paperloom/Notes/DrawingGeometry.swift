import CoreGraphics
import CoreKit
import Foundation

/// Geometry for drawings, as the web app's: how wide strokes are, what an
/// item covers, what a pointer or a lasso touches, and strokes recognised
/// as shapes.
enum Ink {
    /// Excalidraw draws a stroke this many times its `strokeWidth` wide.
    static let scale = 4.25

    /// How wide a stroke is at a pressure, as Excalidraw draws it.
    static func diameter(_ strokeWidth: Double, _ pressure: Double) -> Double {
        strokeWidth * scale * (0.4 + 1.2 * min(1, max(0, pressure)))
    }

    /// A fresh id, as Excalidraw's are random.
    static func newID() -> String {
        let letters = Array("abcdefghijklmnopqrstuvwxyz0123456789")
        return String((0..<20).map { _ in letters[Int.random(in: 0..<letters.count)] })
    }

    static func distance(_ a: CGPoint, _ b: CGPoint) -> Double {
        hypot(a.x - b.x, a.y - b.y)
    }

    /// How far a point is from the segment `a`–`b`.
    static func distance(_ p: CGPoint, toSegment a: CGPoint, _ b: CGPoint) -> Double {
        let dx = b.x - a.x
        let dy = b.y - a.y
        let length = dx * dx + dy * dy
        let t = length == 0 ? 0 : max(0, min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / length))
        return hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy))
    }

    /// Whether a point lies inside a polygon (its last point joins its first).
    static func inside(_ p: CGPoint, _ polygon: [CGPoint]) -> Bool {
        var inside = false
        var j = polygon.count - 1
        for i in polygon.indices {
            let a = polygon[i]
            let b = polygon[j]
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                inside.toggle()
            }
            j = i
        }
        return inside
    }

    static func length(_ points: [CGPoint]) -> Double {
        zip(points, points.dropFirst()).reduce(0.0) { $0 + distance($1.0, $1.1) }
    }

    /// A shape a hand-drawn stroke was meant to be.
    enum Shape: Equatable {
        case line(CGPoint, CGPoint)
        case rectangle(CGRect)
        case ellipse(CGRect)
    }

    /// A stroke as a straight line, a rectangle or an ellipse; nil when it
    /// is none of them.
    static func recognize(_ points: [CGPoint]) -> Shape? {
        guard points.count >= 3, let first = points.first, let last = points.last else { return nil }
        let total = length(points)
        let chord = distance(first, last)
        guard total >= 24 else { return nil }
        if chord / total > 0.95 { return .line(first, last) }
        guard chord <= 0.2 * total, total >= 60 else { return nil }
        let xs = points.map(\.x)
        let ys = points.map(\.y)
        guard let left = xs.min(), let right = xs.max(), let top = ys.min(), let bottom = ys.max() else { return nil }
        let w = right - left
        let h = bottom - top
        guard w >= 12, h >= 12 else { return nil }
        let cx = (left + right) / 2
        let cy = (top + bottom) / 2
        var ellipse = 0.0
        var rectangle = 0.0
        for p in points {
            ellipse += abs(hypot((p.x - cx) / (w / 2), (p.y - cy) / (h / 2)) - 1)
            rectangle += min(p.x - left, right - p.x, p.y - top, bottom - p.y) / max(w, h)
        }
        ellipse /= Double(points.count)
        rectangle /= Double(points.count)
        let box = CGRect(x: left, y: top, width: w, height: h)
        if rectangle < 0.05 && rectangle * 2 < ellipse { return .rectangle(box) }
        if ellipse < 0.1 { return .ellipse(box) }
        return nil
    }

    /// A stroke with its points made relative to its corner.
    static func stroke(
        _ points: [CGPoint], pressures: [Double], color: String, width: Double, highlighter: Bool
    ) -> DrawingItem {
        let left = points.map(\.x).min() ?? 0
        let top = points.map(\.y).min() ?? 0
        let right = points.map(\.x).max() ?? 0
        let bottom = points.map(\.y).max() ?? 0
        var item = DrawingItem.blank("freedraw")
        item.x = left
        item.y = top
        item.width = right - left
        item.height = bottom - top
        item.points = points.map { DrawingPoint(x: $0.x - left, y: $0.y - top) }
        item.pressures = pressures
        item.strokeColor = color
        item.strokeWidth = width
        item.opacity = highlighter ? 40 : 100
        item.highlighter = highlighter
        return item
    }
}

extension DrawingItem {
    static func blank(_ kind: String) -> DrawingItem {
        DrawingItem(
            id: Ink.newID(), kind: kind, x: 0, y: 0, width: 0, height: 0, angle: 0, strokeColor: "#1e1e1e",
            backgroundColor: "transparent", strokeWidth: 2, opacity: 100, points: [], pressures: [], text: "",
            fontSize: 20, highlighter: false, arrowhead: false, image: "")
    }

    var centre: CGPoint { CGPoint(x: x + width / 2, y: y + height / 2) }

    /// A line, stroke or arrow's points in drawing space.
    var absolutePoints: [CGPoint] { points.map { CGPoint(x: x + $0.x, y: y + $0.y) } }

    /// An item's points in drawing space: a stroke's or line's points, or a
    /// shape's outline (closed), turned by its angle.
    var outline: [CGPoint] {
        let corners: [CGPoint]
        switch kind {
        case "freedraw", "line", "arrow":
            corners = absolutePoints
        case "ellipse":
            corners = (0...32).map { i in
                let a = Double(i) / 32 * 2 * .pi
                return CGPoint(x: x + width / 2 + width / 2 * cos(a), y: y + height / 2 + height / 2 * sin(a))
            }
        case "diamond":
            corners = [
                CGPoint(x: x + width / 2, y: y), CGPoint(x: x + width, y: y + height / 2),
                CGPoint(x: x + width / 2, y: y + height), CGPoint(x: x, y: y + height / 2),
                CGPoint(x: x + width / 2, y: y),
            ]
        default:
            corners = [
                CGPoint(x: x, y: y), CGPoint(x: x + width, y: y), CGPoint(x: x + width, y: y + height),
                CGPoint(x: x, y: y + height), CGPoint(x: x, y: y),
            ]
        }
        guard angle != 0 else { return corners }
        let c = centre
        let (cosA, sinA) = (cos(angle), sin(angle))
        return corners.map { p in
            let dx = p.x - c.x
            let dy = p.y - c.y
            return CGPoint(x: c.x + dx * cosA - dy * sinA, y: c.y + dx * sinA + dy * cosA)
        }
    }

    /// The area an item covers, its stroke included.
    var bounds: CGRect {
        let points = outline
        guard !points.isEmpty else { return CGRect(x: x, y: y, width: 0, height: 0) }
        let reach = kind == "freedraw" ? Ink.diameter(strokeWidth, 1) / 2 : strokeWidth / 2 + 1
        let xs = points.map(\.x)
        let ys = points.map(\.y)
        let left = (xs.min() ?? 0) - reach
        let top = (ys.min() ?? 0) - reach
        return CGRect(x: left, y: top, width: (xs.max() ?? 0) + reach - left, height: (ys.max() ?? 0) + reach - top)
    }

    private var filled: Bool {
        kind == "text" || kind == "image"
            || (backgroundColor != "transparent" && ["rectangle", "ellipse", "diamond"].contains(kind))
    }

    /// Whether a point touches the item, within `slop` drawing units.
    func hits(_ p: CGPoint, slop: Double) -> Bool {
        let points = outline
        let reach = slop + (kind == "freedraw" ? Ink.diameter(strokeWidth, 0.5) / 2 : strokeWidth / 2)
        if points.count == 1 { return Ink.distance(points[0], p) <= reach }
        for (a, b) in zip(points, points.dropFirst()) where Ink.distance(p, toSegment: a, b) <= reach {
            return true
        }
        return filled && Ink.inside(p, points)
    }

    /// Whether most of the item lies inside a lasso.
    func isLassoed(by lasso: [CGPoint]) -> Bool {
        let points = outline
        guard !points.isEmpty, lasso.count >= 3 else { return false }
        return Double(points.filter { Ink.inside($0, lasso) }.count) / Double(points.count) >= 0.8
    }
}

extension Array where Element == DrawingItem {
    /// The area the items cover; nil for none.
    var bounds: CGRect? {
        guard let first else { return nil }
        return dropFirst().reduce(first.bounds) { $0.union($1.bounds) }
    }
}

extension Array where Element == DrawingItem {
    /// The items in `ids` moved by `dx`, `dy`.
    func moving(_ ids: Set<String>, dx: Double, dy: Double) -> [DrawingItem] {
        map { item in
            guard ids.contains(item.id) else { return item }
            var moved = item
            moved.x += dx
            moved.y += dy
            return moved
        }
    }

    /// The items in `ids` grown or shrunk by `factor` from `origin`, their
    /// strokes and text with them.
    func scaling(_ ids: Set<String>, from origin: CGPoint, by factor: Double) -> [DrawingItem] {
        map { item in
            guard ids.contains(item.id) else { return item }
            var scaled = item
            scaled.x = origin.x + (item.x - origin.x) * factor
            scaled.y = origin.y + (item.y - origin.y) * factor
            scaled.width = item.width * factor
            scaled.height = item.height * factor
            scaled.points = item.points.map { DrawingPoint(x: $0.x * factor, y: $0.y * factor) }
            scaled.strokeWidth = item.strokeWidth * factor
            scaled.fontSize = item.fontSize * factor
            return scaled
        }
    }

    /// The items in `ids` turned by `angle` (radians, clockwise) about `pivot`.
    func turning(_ ids: Set<String>, about pivot: CGPoint, by angle: Double) -> [DrawingItem] {
        let (cosA, sinA) = (cos(angle), sin(angle))
        return map { item in
            guard ids.contains(item.id) else { return item }
            let c = item.centre
            let dx = c.x - pivot.x
            let dy = c.y - pivot.y
            var turned = item
            turned.x = pivot.x + dx * cosA - dy * sinA - item.width / 2
            turned.y = pivot.y + dx * sinA + dy * cosA - item.height / 2
            turned.angle = item.angle + angle
            return turned
        }
    }
}

/// A ruler or protractor laid on the page, as the web app's: strokes that
/// start by its edge (or arc) keep to it.
struct Guide: Equatable {
    enum Kind {
        case ruler, protractor
    }

    var kind: Kind
    /// The ruler's middle, or the middle of the protractor's straight side.
    var centre: CGPoint
    var angle: Double
    /// The ruler's length, or the protractor's radius.
    var size: Double

    static let rulerWidth = 64.0

    /// A point in its own frame: along it, and across it (negative is
    /// above it when it is level).
    func local(_ p: CGPoint) -> (along: Double, across: Double) {
        let (dx, dy) = (p.x - centre.x, p.y - centre.y)
        return (dx * cos(angle) + dy * sin(angle), -dx * sin(angle) + dy * cos(angle))
    }

    func point(along: Double, across: Double) -> CGPoint {
        CGPoint(
            x: centre.x + along * cos(angle) - across * sin(angle),
            y: centre.y + along * sin(angle) + across * cos(angle))
    }

    /// Whether a point is on its body, where it is picked up to move.
    func holds(_ p: CGPoint) -> Bool {
        let (along, across) = local(p)
        switch kind {
        case .ruler: return abs(along) <= size / 2 && abs(across) <= Self.rulerWidth / 2
        case .protractor: return across <= 0 && hypot(along, across) <= size
        }
    }

    /// The knob it turns by.
    var knob: CGPoint {
        kind == .ruler ? point(along: size / 2 - 22, across: 0) : point(along: size - 22, across: -14)
    }

    /// How a stroke that starts at `p` keeps to it; nil when `p` is not
    /// within `reach` of the ruler's edges or the protractor's arc.
    func track(from p: CGPoint, reach: Double) -> GuideTrack? {
        let (along, across) = local(p)
        switch kind {
        case .ruler:
            guard abs(along) <= size / 2, abs(across) - Self.rulerWidth / 2 <= reach else { return nil }
            return GuideTrack(guide: self, side: across < 0 ? -1 : 1)
        case .protractor:
            guard across <= reach, abs(hypot(along, across) - size) <= reach else { return nil }
            return GuideTrack(guide: self, side: 1)
        }
    }

    /// An angle as whole degrees from 0 to 359.
    static func degrees(_ angle: Double) -> Int {
        ((Int((angle * 180 / .pi).rounded()) % 360) + 360) % 360
    }
}

/// A stroke kept to a guide: to one edge of a ruler, or to a protractor's
/// arc.
struct GuideTrack {
    let guide: Guide
    let side: Double

    func place(_ p: CGPoint) -> CGPoint {
        let (along, across) = guide.local(p)
        switch guide.kind {
        case .ruler:
            return guide.point(along: along, across: side * Guide.rulerWidth / 2)
        case .protractor:
            let length = max(hypot(along, across), 1e-6)
            return guide.point(along: along / length * guide.size, across: across / length * guide.size)
        }
    }
}
