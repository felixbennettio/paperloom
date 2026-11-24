import CoreKit
import ImageIO
import SwiftUI
import UniformTypeIdentifiers

/// Names for diagram files.
enum DrawIO {
    /// A diagram's name, without `.drawio` or `.drawio.svg`.
    static func title(_ path: String) -> String {
        var name = (path as NSString).lastPathComponent
        for suffix in [".svg", ".drawio"] where name.lowercased().hasSuffix(suffix) {
            name = String(name.dropLast(suffix.count))
        }
        return name
    }
}

/// Geometry for the diagram editor, as the web app's: what a point is on,
/// resize handles, connection points, snapping.
enum DiagramGeometry {
    static func box(_ item: DiagramDrawn) -> CGRect {
        CGRect(x: item.x, y: item.y, width: item.width, height: item.height)
    }

    static func distance(_ p: CGPoint, toLine points: [DiagramPoint]) -> Double {
        let (px, py) = (Double(p.x), Double(p.y))
        guard let first = points.first else { return .infinity }
        if points.count == 1 { return hypot(px - first.x, py - first.y) }
        var best = Double.infinity
        for i in 1..<points.count {
            let (a, b) = (points[i - 1], points[i])
            let (dx, dy) = (b.x - a.x, b.y - a.y)
            let length = dx * dx + dy * dy
            let t: Double = length > 0 ? max(0, min(1, ((px - a.x) * dx + (py - a.y) * dy) / length)) : 0
            best = min(best, hypot(px - (a.x + t * dx), py - (a.y + t * dy)))
        }
        return best
    }

    /// The topmost shape or edge at a point; a container (not a group) is
    /// picked by its rim or header, so what is in it can be.
    static func hit(_ items: [DiagramDrawn], at p: CGPoint, slop: Double) -> DiagramDrawn? {
        for item in items.reversed() {
            if item.edge {
                if !item.points.isEmpty, distance(p, toLine: item.points) <= slop { return item }
                continue
            }
            let inBox = box(item).contains(p)
            let label = CGRect(x: item.textX, y: item.textY, width: item.textWidth, height: item.textHeight)
            let onLabel = !item.text.isEmpty && label.contains(p)
            if inBox, item.container, !item.style.hasPrefix("group") {
                let inner = CGRect(x: item.x + slop, y: item.y + 26, width: item.width - 2 * slop, height: item.height - 26 - slop)
                if inner.contains(p) { continue }
            }
            if inBox || onLabel { return item }
        }
        return nil
    }

    /// The resize handles: the corners and the middles of the sides,
    /// clockwise from the top left.
    static func handles(_ r: CGRect) -> [CGPoint] {
        [
            CGPoint(x: r.minX, y: r.minY), CGPoint(x: r.midX, y: r.minY), CGPoint(x: r.maxX, y: r.minY),
            CGPoint(x: r.maxX, y: r.midY), CGPoint(x: r.maxX, y: r.maxY), CGPoint(x: r.midX, y: r.maxY),
            CGPoint(x: r.minX, y: r.maxY), CGPoint(x: r.minX, y: r.midY),
        ]
    }

    /// A box dragged by one of its handles.
    static func resized(_ r: CGRect, handle: Int, dx: Double, dy: Double, least: Double = 10) -> CGRect {
        var (x, y, width, height) = (Double(r.minX), Double(r.minY), Double(r.width), Double(r.height))
        if [0, 6, 7].contains(handle) {
            let w = max(least, width - dx)
            x += width - w
            width = w
        }
        if [2, 3, 4].contains(handle) { width = max(least, width + dx) }
        if [0, 1, 2].contains(handle) {
            let h = max(least, height - dy)
            y += height - h
            height = h
        }
        if [4, 5, 6].contains(handle) { height = max(least, height + dy) }
        return CGRect(x: x, y: y, width: width, height: height)
    }

    /// The middles of a box's sides, where new edges start.
    static func connectionPoints(_ r: CGRect) -> [CGPoint] {
        [CGPoint(x: r.midX, y: r.minY), CGPoint(x: r.maxX, y: r.midY), CGPoint(x: r.midX, y: r.maxY), CGPoint(x: r.minX, y: r.midY)]
    }

    static func snap(_ value: Double, _ grid: Double) -> Double {
        grid > 0 ? (value / grid).rounded() * grid : value
    }

    /// A free position within four lattice rings, preferring right and down.
    static func freeSpot(_ boxes: [CGRect], wanted: CGRect, gap: CGFloat = 20) -> CGPoint {
        let free = { (p: CGPoint) in
            !boxes.contains { b in
                p.x < b.maxX + gap && p.x + wanted.width > b.minX - gap
                    && p.y < b.maxY + gap && p.y + wanted.height > b.minY - gap
            }
        }
        let origin = wanted.origin
        if free(origin) { return origin }
        let forward = { (p: CGPoint) in p.x >= 0 && p.y >= 0 ? 0 : 1 }
        let angle = { (p: CGPoint) in
            let value = atan2(Double(p.y), Double(p.x))
            return value < 0 ? value + 2 * .pi : value
        }
        for ring in 1...4 {
            var offsets: [CGPoint] = []
            for x in -ring...ring { for y in -ring...ring where max(abs(x), abs(y)) == ring {
                offsets.append(CGPoint(x: x, y: y))
            } }
            offsets.sort { a, b in
                if forward(a) != forward(b) { return forward(a) < forward(b) }
                let da = a.x * a.x + a.y * a.y, db = b.x * b.x + b.y * b.y
                return da != db ? da < db : angle(a) < angle(b)
            }
            for offset in offsets {
                let p = CGPoint(x: origin.x + offset.x * (wanted.width + gap), y: origin.y + offset.y * (wanted.height + gap))
                if free(p) { return p }
            }
        }
        return origin
    }

    /// How far to nudge a moving box so an edge or middle lines up with
    /// another box's; and where the guides go (x for vertical lines).
    static func aligned(_ moving: CGRect, with others: [CGRect], tolerance: Double)
        -> (dx: Double, dy: Double, xs: [Double], ys: [Double])
    {
        let xs = { (r: CGRect) in [Double(r.minX), Double(r.midX), Double(r.maxX)] }
        let ys = { (r: CGRect) in [Double(r.minY), Double(r.midY), Double(r.maxY)] }
        var (nearestX, nearestY) = (Double.infinity, Double.infinity)
        for other in others {
            for a in xs(moving) { for b in xs(other) where abs(b - a) < abs(nearestX) { nearestX = b - a } }
            for a in ys(moving) { for b in ys(other) where abs(b - a) < abs(nearestY) { nearestY = b - a } }
        }
        let dx = abs(nearestX) <= tolerance ? nearestX : 0
        let dy = abs(nearestY) <= tolerance ? nearestY : 0
        let placed = moving.offsetBy(dx: dx, dy: dy)
        var lines: (xs: [Double], ys: [Double]) = ([], [])
        for other in others {
            if abs(nearestX) <= tolerance {
                for a in xs(placed) where xs(other).contains(where: { abs($0 - a) < 0.01 }) && !lines.xs.contains(a) {
                    lines.xs.append(a)
                }
            }
            if abs(nearestY) <= tolerance {
                for a in ys(placed) where ys(other).contains(where: { abs($0 - a) < 0.01 }) && !lines.ys.contains(a) {
                    lines.ys.append(a)
                }
            }
        }
        return (dx, dy, lines.xs, lines.ys)
    }

    /// A cell's style: `key=value` pairs, a bare word as itself.
    static func style(_ text: String) -> [String: String] {
        var map: [String: String] = [:]
        for part in text.split(separator: ";") {
            let entry = part.trimmingCharacters(in: .whitespaces)
            guard !entry.isEmpty else { continue }
            if let at = entry.firstIndex(of: "=") {
                map[String(entry[..<at])] = String(entry[entry.index(after: at)...])
            } else {
                map[entry] = entry
            }
        }
        return map
    }
}

/// Draws a shape or an edge as the core laid it out.
private func drawItem(_ item: DiagramDrawn, in context: GraphicsContext) {
    var ctx = context
    if item.rotation != 0 {
        let (cx, cy) = (item.x + item.width / 2, item.y + item.height / 2)
        ctx.translateBy(x: cx, y: cy)
        ctx.rotate(by: .degrees(item.rotation))
        ctx.translateBy(x: -cx, y: -cy)
    }
    ctx.opacity = item.opacity
    if !item.image.isEmpty {
        let box = CGRect(x: item.x, y: item.y, width: item.width, height: item.height)
        if let picture = pictureImage(item.image) {
            let resolved = ctx.resolve(picture)
            var frame = box
            if item.imageAspect, resolved.size.width > 0, resolved.size.height > 0 {
                let scale = min(box.width / resolved.size.width, box.height / resolved.size.height)
                let size = CGSize(width: resolved.size.width * scale, height: resolved.size.height * scale)
                frame = CGRect(x: box.midX - size.width / 2, y: box.midY - size.height / 2, width: size.width, height: size.height)
            }
            ctx.draw(resolved, in: frame)
        } else {
            ctx.fill(Path(box), with: .color(Color(white: 0.92)))
        }
    }
    let stroke = Color(hex: item.stroke)
    let style = StrokeStyle(lineWidth: item.strokeWidth, lineCap: .round, lineJoin: .round, dash: item.dashed ? [6, 4] : [])
    if !item.path.isEmpty {
        let outline = EdgeLine(data: item.path).path(in: .zero)
        if !item.edge, let fill = Color(hex: item.fill) { ctx.fill(outline, with: .color(fill)) }
        if let stroke { ctx.stroke(outline, with: .color(stroke), style: style) }
    }
    if let stroke {
        if !item.inner.isEmpty {
            ctx.stroke(EdgeLine(data: item.inner).path(in: .zero), with: .color(stroke), lineWidth: item.strokeWidth)
        }
        if !item.arrows.isEmpty {
            let arrows = EdgeLine(data: item.arrows).path(in: .zero)
            ctx.fill(arrows, with: .color(stroke))
            ctx.stroke(arrows, with: .color(stroke), lineWidth: item.strokeWidth)
        }
        if !item.openArrows.isEmpty {
            ctx.stroke(EdgeLine(data: item.openArrows).path(in: .zero), with: .color(stroke), lineWidth: item.strokeWidth)
        }
    }
    if !item.text.isEmpty { drawLabel(item, in: ctx) }
}

/// A label in its box, wrapped when it wraps, placed as it is aligned.
private func drawLabel(_ item: DiagramDrawn, in context: GraphicsContext) {
    var text = Text(verbatim: item.text).font(.system(size: item.fontSize, weight: item.bold ? .bold : .regular))
    if item.italic { text = text.italic() }
    if item.underline { text = text.underline() }
    text = text.foregroundStyle(Color(hex: item.fontColor) ?? .black)
    let resolved = context.resolve(text)
    let room = CGSize(width: item.wrap ? max(item.textWidth, 1) : .greatestFiniteMagnitude, height: .greatestFiniteMagnitude)
    let size = resolved.measure(in: room)
    let x: Double =
        switch item.align {
        case "left": item.textX
        case "right": item.textX + item.textWidth - size.width
        default: item.textX + (item.textWidth - size.width) / 2
        }
    let y: Double =
        switch item.verticalAlign {
        case "top": item.textY
        case "bottom": item.textY + item.textHeight - size.height
        default: item.textY + (item.textHeight - size.height) / 2
        }
    let rect = CGRect(x: x, y: y, width: size.width, height: size.height)
    if let background = Color(hex: item.labelBackground) {
        context.fill(Path(rect.insetBy(dx: -2, dy: 0)), with: .color(background))
    }
    context.draw(resolved, in: rect)
}

/// A page drawn on white, for saving as a picture.
private struct DiagramPicture: View {
    let layout: DiagramView

    var body: some View {
        Canvas { context, size in
            context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(.white))
            var ctx = context
            ctx.translateBy(x: 10 - layout.x, y: 10 - layout.y)
            for item in layout.items { drawItem(item, in: ctx) }
        }
        .frame(width: max(layout.width + 20, 1), height: max(layout.height + 20, 1))
        .environment(\.colorScheme, .light)
    }
}

/// A shape of the palette, drawn small.
private struct DiagramShapeIcon: View {
    let shape: DiagramShape

    var body: some View {
        Canvas { context, size in
            guard shape.width > 0, shape.height > 0 else { return }
            let (w, h) = (Double(size.width), Double(size.height))
            let scale = min((w - 4) / shape.width, (h - 4) / shape.height)
            var ctx = context
            ctx.translateBy(x: (w - shape.width * scale) / 2, y: (h - shape.height * scale) / 2)
            ctx.scaleBy(x: scale, y: scale)
            if shape.path.isEmpty {
                ctx.draw(Text(verbatim: "T").font(.system(size: 14 / scale)), at: CGPoint(x: shape.width / 2, y: shape.height / 2))
            } else {
                let outline = EdgeLine(data: shape.path).path(in: .zero)
                ctx.fill(outline, with: .color(Color.gray.opacity(0.15)))
                ctx.stroke(outline, with: .color(.primary), lineWidth: 1.2 / scale)
            }
        }
        .frame(width: 34, height: 30)
    }
}

/// What a pointer held down is doing to a diagram.
private enum DiagramDrag {
    case move(start: CGPoint, dx: Double, dy: Double, xs: [Double], ys: [Double])
    case resize(id: String, handle: Int, start: CGPoint, box: CGRect, now: CGRect)
    case marquee(start: CGPoint, now: CGPoint)
    case connect(source: String, from: CGPoint, now: CGPoint)
    case end(id: String, source: Bool, now: CGPoint)
    case bend(id: String, index: Int, points: [CGPoint])
    /// Turning a shape about its middle: its rotation before, and the
    /// pointer's angles.
    case rotate(id: String, centre: CGPoint, base: Double, start: Double, now: Double)
}

/// Where a shape's turning knob is: above the middle of its top.
private func rotateKnob(_ box: CGRect, zoom: Double) -> CGPoint {
    CGPoint(x: box.midX, y: box.minY - 24 / zoom)
}

/// The pointer's angle about a centre, in degrees, 0 straight up.
private func pointerAngle(_ centre: CGPoint, _ p: CGPoint) -> Double {
    Double(atan2(p.y - centre.y, p.x - centre.x)) * 180 / .pi + 90
}

/// A rotation turned by the pointer, in whole degrees from 0 to 359; with
/// Shift, in steps of 15°.
private func turned(_ base: Double, _ start: Double, _ now: Double, steps: Bool) -> Double {
    let raw = base + now - start
    let settled = steps ? (raw / 15).rounded() * 15 : raw.rounded()
    return (settled.truncatingRemainder(dividingBy: 360) + 360).truncatingRemainder(dividingBy: 360)
}

/// A diagram drawn and edited here, kept in the vault as a `.drawio.svg`
/// (a picture every app shows, the diagram inside) or a `.drawio` file in
/// draw.io's format. Shapes come from the palette (tapped or dragged); the
/// blue points on a shape start an edge; a double tap writes a label.
struct DiagramBoardView: View {
    let path: String
    @Environment(VaultModel.self) private var vault
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    @State private var xml: String?
    @State private var page: UInt32 = 0
    @State private var layout: DiagramView?
    @State private var shapes: [DiagramShape] = []
    @State private var selection: [String] = []
    @State private var offset = CGPoint(x: 40, y: 40)
    @State private var zoom = 1.0
    @State private var drag: DiagramDrag?
    @State private var hover: String?
    @State private var editingID: String?
    @State private var editingText = ""
    @State private var snapping = true
    @State private var past: [String] = []
    @State private var future: [String] = []
    @State private var saveTask: Task<Void, Never>?
    @State private var problem: String?
    @State private var lastTap: (at: CGPoint, time: Date)?
    @State private var fitted = false
    @State private var canvasSize = CGSize.zero
    @State private var exportFile: ExportFile?
    @State private var renaming: String?
    @State private var choosingPicture = false
    @FocusState private var keyboard: Bool
    @FocusState private var writing: Bool

    private var items: [DiagramDrawn] { layout?.items ?? [] }
    private var selected: [DiagramDrawn] { selection.compactMap { id in items.first { $0.id == id } } }
    private var selectedShapes: [DiagramDrawn] { selected.filter { !$0.edge } }
    private var single: DiagramDrawn? { selected.count == 1 ? selected.first : nil }
    private var grid: Double {
        let spacing = layout?.grid ?? 10
        return snapping ? (spacing > 0 ? spacing : 10) : 0
    }
    private var reach: Double { 8 / zoom }
    private var showsPalette: Bool {
        #if os(iOS)
        sizeClass != .compact
        #else
        true
        #endif
    }

    /// The selection and what it holds, which move with it.
    private var moving: Set<String> {
        var ids = Set(selection)
        var grew = true
        while grew {
            grew = false
            for item in items where !ids.contains(item.id) && !item.parent.isEmpty && ids.contains(item.parent) {
                ids.insert(item.id)
                grew = true
            }
        }
        return ids
    }

    @AppStorage("diagram.paletteWidth") private var paletteWidth = 176.0

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            if !selection.isEmpty { formatBar }
            Divider()
            HStack(spacing: 0) {
                if showsPalette {
                    palette
                    PaneResizeHandle(width: $paletteWidth, range: 120...360, paneBefore: true)
                }
                stage
            }
        }
        .task(id: path) { load() }
        .onDisappear { saveNow() }
        .alert("Diagram", isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(problem ?? "")
        }
        .alert("Rename Page", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            TextField("Rename Page", text: Binding(get: { renaming ?? "" }, set: { renaming = $0 }))
            Button("OK") {
                if let name = renaming, !name.trimmingCharacters(in: .whitespaces).isEmpty { apply(.renamePage(name: name)) }
                renaming = nil
            }
            Button("Cancel", role: .cancel) { renaming = nil }
        }
    }

    // MARK: Parts

    private var toolbar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                Button("Undo", systemImage: "arrow.uturn.backward") { undo() }
                    .disabled(past.isEmpty)
                    .keyboardShortcut("z", modifiers: .command)
                Button("Redo", systemImage: "arrow.uturn.forward") { redo() }
                    .disabled(future.isEmpty)
                    .keyboardShortcut("z", modifiers: [.command, .shift])
                Button("Delete", systemImage: "trash") { apply(.delete(ids: selection)) }
                    .disabled(selection.isEmpty)
                    .keyboardShortcut(.delete, modifiers: .command)
                Button("Duplicate", systemImage: "plus.square.on.square") { apply(.duplicate(ids: selection, dx: 20, dy: 20)) }
                    .disabled(selection.isEmpty)
                    .keyboardShortcut("d", modifiers: .command)
                Button("To Front", systemImage: "square.3.layers.3d.top.filled") { apply(.toFront(ids: selection)) }
                    .disabled(selection.isEmpty)
                Button("To Back", systemImage: "square.3.layers.3d.bottom.filled") { apply(.toBack(ids: selection)) }
                    .disabled(selection.isEmpty)
                Button("Group", systemImage: "square.on.square.dashed") { apply(.group(ids: selectedShapes.map(\.id))) }
                    .disabled(selectedShapes.count < 2)
                    .keyboardShortcut("g", modifiers: .command)
                Button("Ungroup", systemImage: "square.on.square") {
                    if let single { apply(.ungroup(id: single.id)) }
                }
                .disabled(single?.container != true)
                .keyboardShortcut("g", modifiers: [.command, .shift])
                Button("Insert Picture…", systemImage: "photo") { choosingPicture = true }
                    .help("Insert Picture…")
                Menu {
                    Button("Left") { apply(.align(ids: selection, how: "left")) }
                    Button("Center") { apply(.align(ids: selection, how: "center")) }
                    Button("Right") { apply(.align(ids: selection, how: "right")) }
                    Button("Top") { apply(.align(ids: selection, how: "top")) }
                    Button("Middle") { apply(.align(ids: selection, how: "middle")) }
                    Button("Bottom") { apply(.align(ids: selection, how: "bottom")) }
                    Divider()
                    Button("Distribute Horizontally") { apply(.distribute(ids: selection, horizontal: true)) }
                    Button("Distribute Vertically") { apply(.distribute(ids: selection, horizontal: false)) }
                } label: {
                    Label("Align", systemImage: "align.horizontal.left")
                }
                .menuIndicator(.hidden)
                .fixedSize()
                .disabled(selectedShapes.count < 2)
                if !showsPalette {
                    Menu {
                        ForEach(Self.groups, id: \.self) { group in
                            Section(LocalizedStringKey(group)) {
                                ForEach(shapes.filter { $0.group == group }, id: \.id) { shape in
                                    Button(LocalizedStringKey(shape.name)) { add(shape, at: freeMiddle(shape.width, shape.height)) }
                                }
                            }
                        }
                    } label: {
                        Label("Shapes", systemImage: "square.on.circle")
                    }
                    .menuIndicator(.hidden)
                    .fixedSize()
                }
                Toggle(isOn: $snapping) { Label("Snap to Grid", systemImage: "grid") }
                    .toggleStyle(.button)
                Menu {
                    Button("Picture (SVG)") { export("svg") }
                    Button("Picture (PNG)") { export("png") }
                    Button { export("pdf") } label: { Text(verbatim: "PDF") }
                    Button { export("drawio") } label: { Text(verbatim: "draw.io") }
                } label: {
                    Label("Export…", systemImage: "square.and.arrow.up")
                }
                .menuIndicator(.hidden)
                .fixedSize()
                Spacer(minLength: 12)
                Button("Zoom Out", systemImage: "minus.magnifyingglass") { zoomBy(1 / 1.25, at: centre) }
                Button {
                    fit()
                } label: {
                    Text(verbatim: "\(Int((zoom * 100).rounded()))%").monospacedDigit()
                }
                Button("Zoom In", systemImage: "plus.magnifyingglass") { zoomBy(1.25, at: centre) }
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
        }
        .fileImporter(isPresented: $choosingPicture, allowedContentTypes: [.image]) { result in
            guard case .success(let file) = result else { return }
            let scoped = file.startAccessingSecurityScopedResource()
            defer { if scoped { file.stopAccessingSecurityScopedResource() } }
            insertPicture(file)
        }
        .background {
            Color.clear.fileExporter(
                isPresented: Binding(get: { exportFile != nil }, set: { if !$0 { exportFile = nil } }),
                document: exportFile,
                contentType: exportFile?.contentType ?? .data,
                defaultFilename: exportFile?.url.lastPathComponent
            ) { _ in
                if let url = exportFile?.url { try? FileManager.default.removeItem(at: url) }
                exportFile = nil
            }
        }
    }

    private static let fills = ["", "#ffffff", "#dae8fc", "#d5e8d4", "#fff2cc", "#ffe6cc", "#f8cecc", "#e1d5e7", "#f5f5f5"]
    private static let strokes = ["#000000", "#6c8ebf", "#82b366", "#d6b656", "#d79b00", "#b85450", "#9673a6", ""]
    private static let arrows: [(String, LocalizedStringKey)] = [
        ("classic", "Classic"), ("block", "Block"), ("open", "Open Arrowhead"), ("oval", "Oval"), ("diamond", "Diamond"),
        ("none", "None"),
    ]

    private var formatBar: some View {
        let style = DiagramGeometry.style(single?.style ?? "")
        let fontStyle = Int(style["fontStyle"] ?? "0") ?? 0
        let edges = selected.contains { $0.edge }
        let line = style["curved"] == "1" ? "curved" : style["edgeStyle"] != nil ? "orthogonal" : "straight"
        // Wraps in a narrow pane rather than hiding its end.
        return FlowLayout(spacing: 10, lineSpacing: 4) {
            if !selectedShapes.isEmpty {
                Menu {
                    ForEach(Self.fills, id: \.self) { hex in
                        Button { setStyle("fillColor", hex.isEmpty ? "none" : hex) } label: {
                            if hex.isEmpty {
                                Text("None")
                            } else {
                                Label { Text(verbatim: hex) } icon: { colorDot(PlatformColor(hex: hex) ?? .gray) }
                            }
                        }
                    }
                } label: {
                    Label("Fill", systemImage: "paintbrush.fill")
                }
                .fixedSize()
            }
            Menu {
                ForEach(Self.strokes, id: \.self) { hex in
                    Button { setStyle("strokeColor", hex.isEmpty ? "none" : hex) } label: {
                        if hex.isEmpty {
                            Text("None")
                        } else {
                            Label { Text(verbatim: hex) } icon: { colorDot(PlatformColor(hex: hex) ?? .gray) }
                        }
                    }
                }
            } label: {
                Label("Line Color", systemImage: "pencil.line")
            }
            .fixedSize()
            Picker("Line Width", selection: Binding(get: { style["strokeWidth"] ?? "1" }, set: { setStyle("strokeWidth", $0) })) {
                ForEach(["1", "2", "3", "4", "6"], id: \.self) { Text(verbatim: "\($0) pt").tag($0) }
            }
            .fixedSize()
            Toggle(
                "Dashed", isOn: Binding(get: { style["dashed"] == "1" }, set: { setStyle("dashed", $0 ? "1" : "") })
            )
            .toggleStyle(.button)
            Picker("Font Size", selection: Binding(get: { style["fontSize"] ?? "12" }, set: { setStyle("fontSize", $0) })) {
                ForEach(["9", "10", "11", "12", "14", "16", "18", "20", "24", "28", "36"], id: \.self) {
                    Text(verbatim: $0).tag($0)
                }
            }
            .fixedSize()
            Button("Bold", systemImage: "bold") { setStyle("fontStyle", String(fontStyle ^ 1)) }
                .foregroundStyle(fontStyle & 1 != 0 ? Color.accentColor : Color.primary)
            Button("Italic", systemImage: "italic") { setStyle("fontStyle", String(fontStyle ^ 2)) }
                .foregroundStyle(fontStyle & 2 != 0 ? Color.accentColor : Color.primary)
            Picker("Text Alignment", selection: Binding(get: { style["align"] ?? "center" }, set: { setStyle("align", $0) })) {
                Text("Left").tag("left")
                Text("Center").tag("center")
                Text("Right").tag("right")
            }
            .fixedSize()
            if edges {
                Picker("Connector", selection: Binding(get: { line }, set: { setLine($0) })) {
                    Text("Straight").tag("straight")
                    Text("Orthogonal").tag("orthogonal")
                    Text("Curved").tag("curved")
                }
                .fixedSize()
                Picker("Start Arrow", selection: Binding(get: { style["startArrow"] ?? "none" }, set: { setStyle("startArrow", $0) })) {
                    ForEach(Self.arrows, id: \.0) { arrow in Text(arrow.1).tag(arrow.0) }
                }
                .fixedSize()
                Picker("End Arrow", selection: Binding(get: { style["endArrow"] ?? "classic" }, set: { setStyle("endArrow", $0) })) {
                    ForEach(Self.arrows, id: \.0) { arrow in Text(arrow.1).tag(arrow.0) }
                }
                .fixedSize()
            }
        }
        .labelStyle(.titleAndIcon)
        .buttonStyle(.borderless)
        .padding(.horizontal, 12)
        .padding(.vertical, 4)
    }

    /// The palette's groups, in order.
    static let groups = ["General", "Arrows", "Flowchart", "UML", "Entity Relation", "BPMN"]

    private var palette: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(Self.groups, id: \.self) { group in
                    Text(LocalizedStringKey(group)).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 38, maximum: 38), spacing: 2)], alignment: .leading, spacing: 2) {
                        ForEach(shapes.filter { $0.group == group }, id: \.id) { shape in
                            Button {
                                add(shape, at: freeMiddle(shape.width, shape.height))
                            } label: {
                                DiagramShapeIcon(shape: shape)
                            }
                            .buttonStyle(.plain)
                            .help(Text(LocalizedStringKey(shape.name)))
                            .draggable(shape.id)
                        }
                    }
                }
            }
            .padding(8)
        }
        .frame(width: paletteWidth)
    }

    private var stage: some View {
        GeometryReader { proxy in
            ZStack(alignment: .topLeading) {
                Canvas { context, size in render(context, size: size) }
                InkInput(callbacks: callbacks)
                    .onContinuousHover { phase in
                        switch phase {
                        case .active(let location):
                            guard drag == nil else { return }
                            let p = world(location)
                            if let near = hover.flatMap({ id in items.first { $0.id == id } }),
                               DiagramGeometry.connectionPoints(DiagramGeometry.box(near)).contains(where: { hypot($0.x - p.x, $0.y - p.y) <= reach * 1.5 })
                            {
                                return
                            }
                            let hit = DiagramGeometry.hit(items, at: p, slop: 6 / zoom)
                            hover = hit.flatMap { $0.edge ? nil : $0.id }
                        case .ended:
                            hover = nil
                        }
                    }
                if let id = editingID, let item = items.first(where: { $0.id == id }) {
                    editor(for: item)
                }
                pagesBar
                    .padding(8)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
            }
            .clipped()
            .background(Color.white)
            .dropDestination(for: String.self) { ids, location in
                guard let id = ids.first, let shape = shapes.first(where: { $0.id == id }) else { return false }
                add(shape, at: world(location))
                return true
            }
            .onAppear { canvasSize = proxy.size }
            .onChange(of: proxy.size) { canvasSize = proxy.size }
        }
        .focusable()
        .focused($keyboard)
        .focusEffectDisabled()
        .onKeyPress(phases: .down) { press in handle(press) }
    }

    private func editor(for item: DiagramDrawn) -> some View {
        let box = item.text.isEmpty && item.edge
            ? DiagramGeometry.box(item)
            : CGRect(x: item.textX, y: item.textY, width: item.textWidth, height: item.textHeight)
        let width = max(box.width, 80) * zoom
        return TextField("Text", text: $editingText, axis: .vertical)
            .textFieldStyle(.plain)
            .multilineTextAlignment(.center)
            .font(.system(size: item.fontSize * zoom))
            .foregroundStyle(Color.black)
            .padding(2)
            .frame(width: width)
            .background(Color.white)
            .overlay(Rectangle().strokeBorder(Color.accentColor))
            .focused($writing)
            .onSubmit { commitText() }
            .onKeyPress(.escape) {
                editingID = nil
                keyboard = true
                return .handled
            }
            .onAppear { writing = true }
            .position(x: offset.x + box.midX * zoom, y: offset.y + box.midY * zoom)
    }

    private var pagesBar: some View {
        HStack(spacing: 2) {
            ForEach(Array((layout?.pages ?? []).enumerated()), id: \.offset) { index, title in
                Button {
                    selection = []
                    page = UInt32(index)
                    refresh()
                } label: {
                    Text(verbatim: title)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 3)
                        .foregroundStyle(UInt32(index) == page ? Color.white : Color.primary)
                        .background(UInt32(index) == page ? Color.accentColor : Color.clear, in: .rect(cornerRadius: 4))
                }
                .buttonStyle(.plain)
                .contextMenu {
                    Button("Rename Page") { renaming = title }
                }
            }
            Button("Add Page", systemImage: "plus") { apply(.addPage(name: "")) }
            if (layout?.pages.count ?? 0) > 1 {
                Button("Remove Page", systemImage: "minus") { apply(.removePage) }
            }
        }
        .labelStyle(.iconOnly)
        .buttonStyle(.borderless)
        .font(.caption)
        .padding(3)
        .background(.regularMaterial, in: .rect(cornerRadius: 6))
    }

    // MARK: Drawing

    private func world(_ p: CGPoint) -> CGPoint {
        CGPoint(x: (p.x - offset.x) / zoom, y: (p.y - offset.y) / zoom)
    }

    private var centre: CGPoint { CGPoint(x: canvasSize.width / 2, y: canvasSize.height / 2) }
    private var middle: CGPoint { world(centre) }

    /// A free centre near the middle of the view.
    private func freeMiddle(_ width: Double, _ height: Double) -> CGPoint {
        let p = middle
        let spot = DiagramGeometry.freeSpot(
            items.filter { !$0.edge }.map(DiagramGeometry.box),
            wanted: CGRect(x: p.x - width / 2, y: p.y - height / 2, width: width, height: height))
        return CGPoint(x: spot.x + width / 2, y: spot.y + height / 2)
    }

    private func render(_ context: GraphicsContext, size: CGSize) {
        var ctx = context
        ctx.translateBy(x: offset.x, y: offset.y)
        ctx.scaleBy(x: zoom, y: zoom)
        let thin = 1 / zoom
        if grid > 0, grid * zoom >= 6 {
            let left = -Double(offset.x) / zoom
            let top = -Double(offset.y) / zoom
            let (right, bottom) = (left + Double(size.width) / zoom, top + Double(size.height) / zoom)
            var lines = Path()
            var x = (left / grid).rounded(.down) * grid
            while x <= right {
                lines.move(to: CGPoint(x: x, y: top))
                lines.addLine(to: CGPoint(x: x, y: bottom))
                x += grid
            }
            var y = (top / grid).rounded(.down) * grid
            while y <= bottom {
                lines.move(to: CGPoint(x: left, y: y))
                lines.addLine(to: CGPoint(x: right, y: y))
                y += grid
            }
            ctx.stroke(lines, with: .color(Color(white: 0.91)), lineWidth: thin * 0.6)
        }
        var shift = CGSize.zero
        if case .move(_, let dx, let dy, _, _)? = drag { shift = CGSize(width: dx, height: dy) }
        let moved = moving
        for item in items {
            if moved.contains(item.id), shift != .zero {
                var shifted = ctx
                shifted.translateBy(x: shift.width, y: shift.height)
                drawItem(item, in: shifted)
            } else {
                drawItem(item, in: ctx)
            }
        }
        let accent = GraphicsContext.Shading.color(.accentColor)
        for item in selected where item.edge {
            ctx.stroke(EdgeLine(data: item.path).path(in: .zero), with: .color(Color.accentColor.opacity(0.25)), lineWidth: item.strokeWidth + 6 * thin)
            for p in [item.points.first, item.points.last].compactMap({ $0 }) {
                ctx.fill(Path(ellipseIn: CGRect(x: p.x - 5 * thin, y: p.y - 5 * thin, width: 10 * thin, height: 10 * thin)), with: accent)
            }
            var bends = item.bends.map { CGPoint(x: $0.x, y: $0.y) }
            if case .bend(let id, _, let points)? = drag, id == item.id { bends = points }
            for p in bends {
                let square = Path(CGRect(x: p.x - 4 * thin, y: p.y - 4 * thin, width: 8 * thin, height: 8 * thin))
                ctx.fill(square, with: .color(.white))
                ctx.stroke(square, with: accent, lineWidth: thin)
            }
        }
        let shapeBoxes = selectedShapes.map(DiagramGeometry.box)
        if var area = shapeBoxes.first {
            for box in shapeBoxes.dropFirst() { area = area.union(box) }
            if case .resize? = drag {} else {
                ctx.stroke(Path(area.offsetBy(dx: shift.width, dy: shift.height)), with: accent, style: StrokeStyle(lineWidth: thin, dash: [4 * thin, 3 * thin]))
            }
        }
        if let single, !single.edge, drag == nil {
            for p in DiagramGeometry.handles(DiagramGeometry.box(single)) {
                let square = Path(CGRect(x: p.x - 4 * thin, y: p.y - 4 * thin, width: 8 * thin, height: 8 * thin))
                ctx.fill(square, with: .color(.white))
                ctx.stroke(square, with: accent, lineWidth: thin)
            }
            let box = DiagramGeometry.box(single)
            let knob = rotateKnob(box, zoom: zoom)
            var stem = Path()
            stem.move(to: CGPoint(x: box.midX, y: box.minY))
            stem.addLine(to: knob)
            ctx.stroke(stem, with: accent, lineWidth: thin)
            let dot = Path(ellipseIn: CGRect(x: knob.x - 5 * thin, y: knob.y - 5 * thin, width: 10 * thin, height: 10 * thin))
            ctx.fill(dot, with: .color(.white))
            ctx.stroke(dot, with: accent, lineWidth: thin)
        }
        for id in Set([hover, single?.edge == false ? single?.id : nil].compactMap({ $0 })) where drag == nil {
            guard let item = items.first(where: { $0.id == id }) else { continue }
            for p in DiagramGeometry.connectionPoints(DiagramGeometry.box(item)) {
                let dot = Path(ellipseIn: CGRect(x: p.x - 4.5 * thin, y: p.y - 4.5 * thin, width: 9 * thin, height: 9 * thin))
                ctx.fill(dot, with: .color(.white))
                ctx.stroke(dot, with: .color(.blue), lineWidth: thin)
            }
        }
        switch drag {
        case .resize(_, _, _, _, let now)?:
            ctx.stroke(Path(now), with: accent, style: StrokeStyle(lineWidth: thin, dash: [4 * thin, 3 * thin]))
        case .rotate(let id, let centre, let base, let start, let now)?:
            if let item = items.first(where: { $0.id == id }) {
                let angle = turned(base, start, now, steps: false) * .pi / 180
                let turn = CGAffineTransform(translationX: centre.x, y: centre.y)
                    .rotated(by: angle)
                    .translatedBy(x: -centre.x, y: -centre.y)
                ctx.stroke(
                    Path(DiagramGeometry.box(item)).applying(turn), with: accent,
                    style: StrokeStyle(lineWidth: thin, dash: [4 * thin, 3 * thin]))
            }
        case .marquee(let start, let now)?:
            let area = CGRect(x: min(start.x, now.x), y: min(start.y, now.y), width: abs(now.x - start.x), height: abs(now.y - start.y))
            ctx.fill(Path(area), with: .color(Color.accentColor.opacity(0.08)))
            ctx.stroke(Path(area), with: accent, lineWidth: thin)
        case .connect(_, let from, let now)?:
            var rubber = Path()
            rubber.move(to: from)
            rubber.addLine(to: now)
            ctx.stroke(rubber, with: .color(.blue), style: StrokeStyle(lineWidth: 1.5 * thin, dash: [5 * thin, 4 * thin]))
        case .end(let id, let source, let now)?:
            let points = items.first { $0.id == id }?.points ?? []
            if let fixed = source ? points.last : points.first {
                var rubber = Path()
                rubber.move(to: CGPoint(x: fixed.x, y: fixed.y))
                rubber.addLine(to: now)
                ctx.stroke(rubber, with: .color(.blue), style: StrokeStyle(lineWidth: 1.5 * thin, dash: [5 * thin, 4 * thin]))
            }
        case .move(_, _, _, let xs, let ys)?:
            var guides = Path()
            for x in xs {
                guides.move(to: CGPoint(x: x, y: -20000))
                guides.addLine(to: CGPoint(x: x, y: 20000))
            }
            for y in ys {
                guides.move(to: CGPoint(x: -20000, y: y))
                guides.addLine(to: CGPoint(x: 20000, y: y))
            }
            ctx.stroke(guides, with: .color(.red), lineWidth: thin)
        default:
            break
        }
    }

    // MARK: Input

    private var callbacks: InkCallbacks {
        InkCallbacks(
            begin: { begin($0, $1) },
            move: { move($0) },
            end: { end() },
            cancel: { drag = nil },
            pan: { delta in offset = CGPoint(x: offset.x + delta.dx, y: offset.y + delta.dy) },
            zoom: { factor, at in zoomBy(factor, at: at) })
    }

    private func zoomBy(_ factor: Double, at point: CGPoint) {
        let next = min(4, max(0.1, zoom * factor))
        offset = CGPoint(x: point.x - (point.x - offset.x) / zoom * next, y: point.y - (point.y - offset.y) / zoom * next)
        zoom = next
    }

    /// Whether a point of an edge is within reach of `p`.
    private func near(_ point: DiagramPoint, _ p: CGPoint) -> Bool {
        hypot(point.x - Double(p.x), point.y - Double(p.y)) <= reach
    }

    private var shiftHeld: Bool {
        #if os(macOS)
        NSEvent.modifierFlags.contains(.shift)
        #else
        false
        #endif
    }

    private func begin(_ sample: InkSample, _ pointer: InkPointer) {
        guard pointer != .middle, layout != nil else { return }
        keyboard = true
        let p = world(sample.location)
        // A second tap in the same place writes the label.
        if let last = lastTap, Date().timeIntervalSince(last.time) < 0.35, hypot(last.at.x - sample.location.x, last.at.y - sample.location.y) < 6 {
            lastTap = nil
            drag = nil
            if let hit = DiagramGeometry.hit(items, at: p, slop: 6 / zoom) {
                selection = [hit.id]
                editingID = hit.id
                editingText = hit.text
            }
            return
        }
        lastTap = (sample.location, Date())
        commitText()
        // Connection points win over the selected shape's handles beside them.
        for id in [hover, single?.edge == false ? single?.id : nil].compactMap({ $0 }) {
            guard let item = items.first(where: { $0.id == id }) else { continue }
            if let from = DiagramGeometry.connectionPoints(DiagramGeometry.box(item)).first(where: { hypot($0.x - p.x, $0.y - p.y) <= reach }) {
                drag = .connect(source: item.id, from: from, now: p)
                return
            }
        }
        if let single, !single.edge {
            let box = DiagramGeometry.box(single)
            let knob = rotateKnob(box, zoom: zoom)
            if hypot(knob.x - p.x, knob.y - p.y) <= reach {
                let centre = CGPoint(x: box.midX, y: box.midY)
                let angle = pointerAngle(centre, p)
                drag = .rotate(id: single.id, centre: centre, base: single.rotation, start: angle, now: angle)
                return
            }
        }
        if let single, !single.edge,
           let handle = DiagramGeometry.handles(DiagramGeometry.box(single)).firstIndex(where: { hypot($0.x - p.x, $0.y - p.y) <= reach })
        {
            let box = DiagramGeometry.box(single)
            drag = .resize(id: single.id, handle: handle, start: p, box: box, now: box)
            return
        }
        if let single, single.edge, let first = single.points.first, let last = single.points.last {
            if near(first, p) {
                drag = .end(id: single.id, source: true, now: p)
                return
            }
            if near(last, p) {
                drag = .end(id: single.id, source: false, now: p)
                return
            }
            if let index = single.bends.firstIndex(where: { near($0, p) }) {
                drag = .bend(id: single.id, index: index, points: single.bends.map { CGPoint(x: $0.x, y: $0.y) })
                return
            }
        }
        guard let hit = DiagramGeometry.hit(items, at: p, slop: 6 / zoom) else {
            if !shiftHeld { selection = [] }
            drag = .marquee(start: p, now: p)
            return
        }
        if shiftHeld {
            if let at = selection.firstIndex(of: hit.id) { selection.remove(at: at) } else { selection.append(hit.id) }
            return
        }
        let wasSelected = selection.contains(hit.id)
        if !wasSelected { selection = [hit.id] }
        if !hit.edge || wasSelected { drag = .move(start: p, dx: 0, dy: 0, xs: [], ys: []) }
    }

    private func move(_ samples: [InkSample]) {
        guard let sample = samples.last, let current = drag else { return }
        let p = world(sample.location)
        switch current {
        case .move(let start, _, _, _, _):
            let boxes = items.filter { moving.contains($0.id) && !$0.edge }.map(DiagramGeometry.box)
            var dx = Double(p.x - start.x)
            var dy = Double(p.y - start.y)
            var (xs, ys): ([Double], [Double]) = ([], [])
            if var area = boxes.first {
                for box in boxes.dropFirst() { area = area.union(box) }
                let others = items.filter { !$0.edge && !moving.contains($0.id) }.map(DiagramGeometry.box)
                let snapped = DiagramGeometry.aligned(area.offsetBy(dx: dx, dy: dy), with: others, tolerance: 6 / zoom)
                (xs, ys) = (snapped.xs, snapped.ys)
                let (left, top) = (Double(area.minX), Double(area.minY))
                if snapped.dx != 0 {
                    dx += snapped.dx
                } else if grid > 0 {
                    dx = DiagramGeometry.snap(left + dx, grid) - left
                }
                if snapped.dy != 0 {
                    dy += snapped.dy
                } else if grid > 0 {
                    dy = DiagramGeometry.snap(top + dy, grid) - top
                }
            }
            drag = .move(start: start, dx: dx, dy: dy, xs: xs, ys: ys)
        case .resize(let id, let handle, let start, let box, _):
            let raw = DiagramGeometry.resized(box, handle: handle, dx: p.x - start.x, dy: p.y - start.y)
            let now = grid > 0
                ? CGRect(
                    x: DiagramGeometry.snap(raw.minX, grid), y: DiagramGeometry.snap(raw.minY, grid),
                    width: max(grid, DiagramGeometry.snap(raw.width, grid)), height: max(grid, DiagramGeometry.snap(raw.height, grid)))
                : raw
            drag = .resize(id: id, handle: handle, start: start, box: box, now: now)
        case .marquee(let start, _):
            drag = .marquee(start: start, now: p)
        case .connect(let source, let from, _):
            drag = .connect(source: source, from: from, now: p)
        case .end(let id, let source, _):
            drag = .end(id: id, source: source, now: p)
        case .bend(let id, let index, var points):
            if points.indices.contains(index) {
                points[index] = CGPoint(x: DiagramGeometry.snap(p.x, grid), y: DiagramGeometry.snap(p.y, grid))
            }
            drag = .bend(id: id, index: index, points: points)
        case .rotate(let id, let centre, let base, let start, _):
            drag = .rotate(id: id, centre: centre, base: base, start: start, now: pointerAngle(centre, p))
        }
    }

    private func end() {
        guard let done = drag else { return }
        drag = nil
        /// The shape a dropped end lands on, if any.
        func target(at p: CGPoint, except: String) -> String {
            guard let hit = DiagramGeometry.hit(items, at: p, slop: 6 / zoom), !hit.edge, hit.id != except else { return "" }
            return hit.id
        }
        switch done {
        case .move(_, let dx, let dy, _, _):
            if dx != 0 || dy != 0 { apply(.move(ids: selection, dx: dx, dy: dy)) }
        case .resize(let id, _, _, let box, let now):
            if now != box {
                apply(.resize(id: id, x: now.minX, y: now.minY, width: now.width, height: now.height))
            }
        case .marquee(let start, let now):
            let area = CGRect(x: min(start.x, now.x), y: min(start.y, now.y), width: abs(now.x - start.x), height: abs(now.y - start.y))
            guard area.width >= 2 || area.height >= 2 else { return }
            let inside = items.filter { item in
                item.edge
                    ? item.points.allSatisfy { area.contains(CGPoint(x: $0.x, y: $0.y)) }
                    : area.contains(DiagramGeometry.box(item))
            }
            selection = Array(Set((shiftHeld ? selection : []) + inside.map(\.id)))
        case .connect(let source, let from, let now):
            guard hypot(now.x - from.x, now.y - from.y) > reach else { return }
            apply(
                .connect(
                    source: source, target: target(at: now, except: source), from: DiagramPoint(x: from.x, y: from.y),
                    to: DiagramPoint(x: now.x, y: now.y)))
        case .end(let id, let source, let now):
            let edge = items.first { $0.id == id }
            let other = (source ? edge?.target : edge?.source) ?? ""
            apply(.reconnect(id: id, source: source, cell: target(at: now, except: other), at: DiagramPoint(x: now.x, y: now.y)))
        case .bend(let id, _, let points):
            apply(.setBends(id: id, points: points.map { DiagramPoint(x: $0.x, y: $0.y) }))
        case .rotate(let id, _, let base, let start, let now):
            let angle = turned(base, start, now, steps: shiftHeld)
            if angle != base {
                apply(.setStyle(ids: [id], key: "rotation", value: angle == 0 ? "" : String(Int(angle))))
            }
        }
    }

    private func handle(_ press: KeyPress) -> KeyPress.Result {
        guard editingID == nil else { return .ignored }
        switch press.key {
        case .delete, .deleteForward:
            guard !selection.isEmpty else { return .ignored }
            apply(.delete(ids: selection))
        case .escape:
            selection = []
        case .upArrow, .downArrow, .leftArrow, .rightArrow:
            guard !selection.isEmpty else { return .ignored }
            let step = press.modifiers.contains(.shift) ? 10.0 : 1.0
            let dx = press.key == .leftArrow ? -step : press.key == .rightArrow ? step : 0
            let dy = press.key == .upArrow ? -step : press.key == .downArrow ? step : 0
            apply(.move(ids: selection, dx: dx, dy: dy))
        default:
            return .ignored
        }
        return .handled
    }

    // MARK: Changes

    private func load() {
        guard let core = vault.core else { return }
        do {
            xml = try core.readDiagram(path: path)
            page = 0
            past = []
            future = []
            selection = []
            fitted = false
            shapes = diagramShapes()
            refresh()
        } catch {
            problem = CoreKit.message(for: error)
        }
    }

    private func refresh() {
        guard let xml else { return }
        do {
            let laid = try diagramView(xml: xml, page: page)
            layout = laid
            page = laid.page
            if !fitted, canvasSize.width > 0 {
                fitted = true
                fit()
            }
        } catch {
            problem = CoreKit.message(for: error)
        }
    }

    private func fit() {
        guard let layout, !layout.items.isEmpty, canvasSize.width > 0 else { return }
        let next = min(1, max(0.1, min(canvasSize.width / (layout.width + 80), canvasSize.height / (layout.height + 80))))
        zoom = next
        offset = CGPoint(
            x: canvasSize.width / 2 - (layout.x + layout.width / 2) * next,
            y: canvasSize.height / 2 - (layout.y + layout.height / 2) * next)
    }

    /// Makes changes one after another, kept as one step.
    private func apply(_ changes: DiagramChange...) {
        guard let before = xml else { return }
        var (text, shown, picked) = (before, page, selection)
        do {
            for change in changes {
                let result = try diagramChange(xml: text, page: shown, change: change)
                (text, shown) = (result.xml, result.page)
                if !result.selection.isEmpty { picked = result.selection }
            }
        } catch {
            problem = CoreKit.message(for: error)
            return
        }
        past = Array((past + [before]).suffix(100))
        future = []
        xml = text
        page = shown
        selection = picked
        refresh()
        scheduleSave()
    }

    private func setStyle(_ key: String, _ value: String) {
        apply(.setStyle(ids: selection, key: key, value: value))
    }

    private func setLine(_ kind: String) {
        apply(
            .setStyle(ids: selection, key: "edgeStyle", value: kind == "orthogonal" ? "orthogonalEdgeStyle" : ""),
            .setStyle(ids: selection, key: "curved", value: kind == "curved" ? "1" : ""))
    }

    /// A picture from a file in the middle of the view, no bigger than fits.
    private func insertPicture(_ file: URL) {
        guard let picture = pictureURL(file) else {
            problem = String(localized: "This file is not a picture.")
            return
        }
        let scale = min(1, 320 / max(1, picture.size.width), 240 / max(1, picture.size.height))
        let (width, height) = ((picture.size.width * scale).rounded(), (picture.size.height * scale).rounded())
        let p = freeMiddle(width, height)
        apply(
            .addImage(
                image: picture.url, x: DiagramGeometry.snap(p.x - width / 2, grid),
                y: DiagramGeometry.snap(p.y - height / 2, grid), width: width, height: height))
    }

    private func add(_ shape: DiagramShape, at p: CGPoint) {
        apply(
            .addShape(
                shape: shape.id, x: DiagramGeometry.snap(p.x - shape.width / 2, grid),
                y: DiagramGeometry.snap(p.y - shape.height / 2, grid)))
    }

    private func commitText() {
        guard let id = editingID else { return }
        editingID = nil
        keyboard = true
        let was = items.first { $0.id == id }?.text ?? ""
        if editingText != was { apply(.setText(id: id, text: editingText)) }
    }

    private func undo() {
        guard let previous = past.popLast(), let current = xml else { return }
        future.append(current)
        xml = previous
        selection = []
        refresh()
        scheduleSave()
    }

    private func redo() {
        guard let next = future.popLast(), let current = xml else { return }
        past.append(current)
        xml = next
        selection = []
        refresh()
        scheduleSave()
    }

    private func scheduleSave() {
        saveTask?.cancel()
        saveTask = Task {
            try? await Task.sleep(for: .milliseconds(600))
            guard !Task.isCancelled else { return }
            saveNow()
        }
    }

    private func saveNow() {
        saveTask?.cancel()
        saveTask = nil
        guard let xml, let core = vault.core else { return }
        do {
            try core.writeDiagram(path: path, xml: xml, svg: nil)
        } catch {
            problem = CoreKit.message(for: error)
        }
    }

    /// Saves the page as a picture, or the diagram as a `.drawio` file.
    private func export(_ format: String) {
        guard let xml else { return }
        let url = FileManager.default.temporaryDirectory.appending(path: "\(DrawIO.title(path)).\(format)")
        do {
            let data: Data
            switch format {
            case "svg":
                data = try Data(diagramPageSvg(xml: xml, page: page).utf8)
            case "png":
                guard let layout, let picture = diagramPNG(layout) else { throw CocoaError(.fileWriteUnknown) }
                data = picture
            case "pdf":
                guard let layout, let pdf = pdfData(DiagramPicture(layout: layout)) else {
                    throw CocoaError(.fileWriteUnknown)
                }
                data = pdf
            default:
                data = Data(xml.utf8)
            }
            try data.write(to: url)
            exportFile = ExportFile(url: url)
        } catch {
            problem = CoreKit.message(for: error)
        }
    }
}

/// A page as a PNG picture at twice its size.
@MainActor
private func diagramPNG(_ layout: DiagramView) -> Data? {
    let renderer = ImageRenderer(content: DiagramPicture(layout: layout))
    renderer.scale = 2
    guard let image = renderer.cgImage else { return nil }
    let data = NSMutableData()
    guard let destination = CGImageDestinationCreateWithData(data as CFMutableData, UTType.png.identifier as CFString, 1, nil)
    else { return nil }
    CGImageDestinationAddImage(destination, image, nil)
    return CGImageDestinationFinalize(destination) ? data as Data : nil
}
