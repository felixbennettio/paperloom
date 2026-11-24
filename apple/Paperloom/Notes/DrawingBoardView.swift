import CoreKit
import ImageIO
import SwiftUI
import UniformTypeIdentifiers

/// The drawing tools.
private enum DrawTool: String, CaseIterable, Identifiable {
    case pen, highlighter, eraser, rub, lasso, text, line, arrow, rectangle, ellipse, diamond

    var id: String { rawValue }

    var title: LocalizedStringKey {
        switch self {
        case .pen: "Pen"
        case .highlighter: "Highlighter"
        case .eraser: "Eraser"
        case .rub: "Point Eraser"
        case .lasso: "Lasso"
        case .text: "Text"
        case .line: "Line"
        case .arrow: "Arrow"
        case .rectangle: "Rectangle"
        case .ellipse: "Ellipse"
        case .diamond: "Diamond"
        }
    }

    var systemImage: String {
        switch self {
        case .pen: "pencil.tip"
        case .highlighter: "highlighter"
        case .eraser: "eraser"
        case .rub: "eraser.line.dashed"
        case .lasso: "lasso"
        case .text: "textformat"
        case .line: "line.diagonal"
        case .arrow: "arrow.up.right"
        case .rectangle: "rectangle"
        case .ellipse: "circle"
        case .diamond: "diamond"
        }
    }
}

/// How far from its path the point eraser rubs, in points on the screen.
private let rubReach = 9.0

/// A pen kept among the favourites, as OneNote keeps them.
private struct FavouritePen: Codable, Equatable {
    var highlighter: Bool
    var color: String
    /// A pen's stroke width; highlighters have their own.
    var width: Double

    func isSame(as other: FavouritePen) -> Bool {
        highlighter == other.highlighter && color == other.color && (highlighter || width == other.width)
    }
}

/// What a pointer held down is doing.
private enum InkGesture {
    case draw(points: [CGPoint], pressures: [Double], pressure: Bool)
    case erase(Set<String>)
    /// Rubbing out the parts of strokes along a path.
    case rub([CGPoint])
    case lasso([CGPoint])
    case move(start: CGPoint, at: CGPoint)
    /// Growing the selection from its top left corner by its bottom right.
    case scale(origin: CGPoint, start: CGPoint, at: CGPoint)
    /// Turning the selection about its centre; angles from it to the pointer.
    case rotate(pivot: CGPoint, start: Double, at: Double)
    case shape(start: CGPoint, end: CGPoint)
    case pan(last: CGPoint)
    /// Moving a ruler or protractor by its body.
    case guideMove(Guide.Kind, start: CGPoint, from: CGPoint)
    /// Turning one about its middle by its knob: angles from the middle.
    case guideTurn(Guide.Kind, start: Double, from: Double)
}

/// A text being written, and where.
private struct TextEntry {
    var id: String?
    var at: CGPoint
    var value: String
}

private let penColors = ["#1e1e1e", "#1971c2", "#e03131", "#2f9e44", "#f08c00", "#9c36b5"]
private let highlighterColors = ["#ffd43b", "#8ce99a", "#faa2c1", "#a5d8ff"]
private let penWidths: [(LocalizedStringKey, Double)] = [("Thin", 0.35), ("Medium", 0.6), ("Thick", 1.2)]
private let highlighterWidth = 4.0
private let spacing = 32.0

/// The size of a text item, as the system font sets it.
private func measure(_ text: String, size: Double) -> CGSize {
    #if os(macOS)
    let font = NSFont.systemFont(ofSize: size)
    #else
    let font = UIFont.systemFont(ofSize: size)
    #endif
    let lines = text.components(separatedBy: "\n")
    let width = lines.map { NSAttributedString(string: $0, attributes: [.font: font]).size().width }.max() ?? 0
    return CGSize(width: ceil(width), height: ceil(Double(lines.count) * size * 1.25))
}

/// Draws one item in drawing space.
private func drawItem(_ item: DrawingItem, in context: GraphicsContext) {
    var context = context
    context.opacity = max(0, min(1, item.opacity / 100))
    if item.angle != 0 {
        let c = item.centre
        context.translateBy(x: c.x, y: c.y)
        context.rotate(by: .radians(item.angle))
        context.translateBy(x: -c.x, y: -c.y)
    }
    let color = Color(hex: item.strokeColor) ?? .black
    let round = { (width: Double) in StrokeStyle(lineWidth: width, lineCap: .round, lineJoin: .round) }
    switch item.kind {
    case "freedraw":
        let points = item.absolutePoints
        guard let first = points.first else { return }
        if points.count == 1 {
            let r = Ink.diameter(item.strokeWidth, item.pressures.first ?? 0.5) / 2
            context.fill(Path(ellipseIn: CGRect(x: first.x - r, y: first.y - r, width: 2 * r, height: 2 * r)), with: .color(color))
        } else if item.pressures.isEmpty || item.highlighter || item.opacity < 100 {
            // One path, so a see-through stroke is even.
            var path = Path()
            path.move(to: first)
            for i in 1..<(points.count - 1) {
                let mid = CGPoint(x: (points[i].x + points[i + 1].x) / 2, y: (points[i].y + points[i + 1].y) / 2)
                path.addQuadCurve(to: mid, control: points[i])
            }
            path.addLine(to: points[points.count - 1])
            context.stroke(path, with: .color(color), style: round(Ink.diameter(item.strokeWidth, 0.5)))
        } else {
            for i in 1..<points.count {
                let pressure = ((item.pressures[safe: i - 1] ?? 0.5) + (item.pressures[safe: i] ?? 0.5)) / 2
                var segment = Path()
                segment.move(to: points[i - 1])
                segment.addLine(to: points[i])
                context.stroke(segment, with: .color(color), style: round(Ink.diameter(item.strokeWidth, pressure)))
            }
        }
    case "line", "arrow":
        let points = item.absolutePoints
        guard points.count >= 2 else { return }
        var path = Path()
        path.addLines(points)
        if item.arrowhead {
            let a = points[points.count - 2]
            let b = points[points.count - 1]
            let angle = atan2(b.y - a.y, b.x - a.x)
            let size = 8 + 4 * item.strokeWidth
            for turn in [-0.45, 0.45] {
                path.move(to: CGPoint(x: b.x - size * cos(angle + turn), y: b.y - size * sin(angle + turn)))
                path.addLine(to: b)
            }
        }
        context.stroke(path, with: .color(color), style: round(item.strokeWidth))
    case "rectangle", "ellipse", "diamond":
        let box = CGRect(x: item.x, y: item.y, width: item.width, height: item.height).standardized
        let path: Path
        switch item.kind {
        case "rectangle": path = Path(box)
        case "ellipse": path = Path(ellipseIn: box)
        default:
            var diamond = Path()
            diamond.addLines([
                CGPoint(x: box.midX, y: box.minY), CGPoint(x: box.maxX, y: box.midY),
                CGPoint(x: box.midX, y: box.maxY), CGPoint(x: box.minX, y: box.midY),
            ])
            diamond.closeSubpath()
            path = diamond
        }
        if item.backgroundColor != "transparent", let fill = Color(hex: item.backgroundColor) {
            context.fill(path, with: .color(fill))
        }
        context.stroke(path, with: .color(color), style: round(item.strokeWidth))
    case "image":
        let box = CGRect(x: item.x, y: item.y, width: item.width, height: item.height).standardized
        if let image = pictureImage(item.image) {
            context.draw(image, in: box)
        } else {
            context.fill(Path(box), with: .color(Color(white: 0.95)))
        }
    case "text":
        for (i, line) in item.text.components(separatedBy: "\n").enumerated() {
            context.draw(
                Text(verbatim: line).font(.system(size: item.fontSize)).foregroundStyle(color),
                at: CGPoint(x: item.x, y: item.y + Double(i) * item.fontSize * 1.25), anchor: .topLeading)
        }
    default:
        break
    }
}

/// Pictures, decoded once, by their `data:` URL (drawn on the main thread).
private var decodedPictures: [String: Image] = [:]

/// The picture in a `data:` URL, as SwiftUI draws it.
func pictureImage(_ url: String) -> Image? {
    if let known = decodedPictures[url] { return known }
    guard let comma = url.firstIndex(of: ","),
        let data = Data(base64Encoded: String(url[url.index(after: comma)...]))
    else { return nil }
    #if os(macOS)
    guard let platform = NSImage(data: data) else { return nil }
    let image = Image(nsImage: platform)
    #else
    guard let platform = UIImage(data: data) else { return nil }
    let image = Image(uiImage: platform)
    #endif
    decodedPictures[url] = image
    return image
}

/// A picture file as a `data:` URL no bigger than 1600 pixels a side
/// (PNG kept as PNG, anything else as JPEG), and its size.
func pictureURL(_ file: URL) -> (url: String, size: CGSize)? {
    guard let source = CGImageSourceCreateWithURL(file as CFURL, nil) else { return nil }
    let options: [CFString: Any] = [
        kCGImageSourceCreateThumbnailFromImageAlways: true,
        kCGImageSourceThumbnailMaxPixelSize: 1600,
        kCGImageSourceCreateThumbnailWithTransform: true,
    ]
    guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
    let png = (CGImageSourceGetType(source) as String?) == UTType.png.identifier
    let type = png ? UTType.png : UTType.jpeg
    let data = NSMutableData()
    guard let destination = CGImageDestinationCreateWithData(data as CFMutableData, type.identifier as CFString, 1, nil)
    else { return nil }
    CGImageDestinationAddImage(destination, image, [kCGImageDestinationLossyCompressionQuality: 0.9] as CFDictionary)
    guard CGImageDestinationFinalize(destination) else { return nil }
    let mime = png ? "image/png" : "image/jpeg"
    return ("data:\(mime);base64,\((data as Data).base64EncodedString())", CGSize(width: image.width, height: image.height))
}

/// A drawing on white, cropped to what is drawn, for saving as a picture.
private struct DrawingPicture: View {
    let items: [DrawingItem]

    var body: some View {
        let area = (items.bounds ?? CGRect(x: 0, y: 0, width: 320, height: 160)).insetBy(dx: -16, dy: -16)
        Canvas { context, _ in
            context.translateBy(x: -area.minX, y: -area.minY)
            for item in items.filter(\.highlighter) + items.filter({ !$0.highlighter }) {
                drawItem(item, in: context)
            }
        }
        .frame(width: area.width, height: area.height)
        .background(Color.white)
        .environment(\.colorScheme, .light)
    }
}

/// A drawing as PNG data, at twice its size.
@MainActor
private func drawingPNG(_ items: [DrawingItem]) -> Data? {
    let renderer = ImageRenderer(content: DrawingPicture(items: items))
    renderer.scale = 2
    guard let image = renderer.cgImage else { return nil }
    let data = NSMutableData()
    guard
        let destination = CGImageDestinationCreateWithData(
            data as CFMutableData, UTType.png.identifier as CFString, 1, nil)
    else { return nil }
    CGImageDestinationAddImage(destination, image, nil)
    return CGImageDestinationFinalize(destination) ? data as Data : nil
}

/// A hand-drawn page kept as an Excalidraw scene in the vault, as the web
/// app draws it: pens with pressure (Apple Pencil, a tablet), highlighters
/// under the ink, an eraser that takes whole strokes, a lasso to move,
/// recolour or delete, lines, arrows, shapes (or strokes turned into them)
/// and text, on blank, lined, squared or dotted paper. With a Pencil in use
/// a finger pans; two fingers pinch to zoom.
struct DrawingBoardView: View {
    let path: String
    @Environment(VaultModel.self) private var vault
    @State private var page: DrawingPage?
    @State private var problem: String?
    @State private var tool = DrawTool.pen
    @State private var penColor = penColors[0]
    @State private var highlighterColor = highlighterColors[0]
    @State private var width = 0.6
    @State private var shapes = false
    /// Favourite pens, kept on this device as JSON.
    @AppStorage("favouritePens") private var favouritesJSON = "[]"
    @State private var selection: Set<String> = []
    @State private var live: InkGesture?
    @State private var offset = CGPoint(x: 40, y: 40)
    @State private var zoom = 1.0
    @State private var past: [[DrawingItem]] = []
    @State private var future: [[DrawingItem]] = []
    @State private var saveTask: Task<Void, Never>?
    @State private var penSeen = false
    @State private var entry: TextEntry?
    @State private var fitted = false
    @State private var exportFile: ExportFile?
    @State private var choosingPicture = false
    @State private var ruler: Guide?
    @State private var protractor: Guide?
    /// The guide the stroke being drawn keeps to.
    @State private var track: GuideTrack?
    @State private var canvasSize = CGSize.zero

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            Divider()
            GeometryReader { proxy in
                ZStack {
                    Canvas { context, size in render(&context, size: size) }
                    InkInput(callbacks: callbacks)
                }
                .clipped()
                .onAppear {
                    canvasSize = proxy.size
                    fit(in: proxy.size)
                }
                .onChange(of: page == nil) { fit(in: proxy.size) }
                .onChange(of: proxy.size) { canvasSize = proxy.size }
            }
        }
        .task(id: path) { load() }
        .onDisappear { saveNow() }
        .alert(
            "Text",
            isPresented: Binding(get: { entry != nil }, set: { if !$0 { entry = nil } })
        ) {
            TextField("Text", text: Binding(get: { entry?.value ?? "" }, set: { entry?.value = $0 }))
            Button("OK") { commitText() }
            Button("Cancel", role: .cancel) { entry = nil }
        }
        .alert(
            "Drawing",
            isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } })
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(problem ?? "")
        }
    }

    // MARK: Toolbar

    private var colors: [String] { tool == .highlighter ? highlighterColors : penColors }
    private var color: String { tool == .highlighter ? highlighterColor : penColor }

    private var toolbar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                ForEach(DrawTool.allCases) { option in
                    Button {
                        tool = option
                    } label: {
                        Label(option.title, systemImage: option.systemImage)
                            .padding(4)
                            .background(tool == option ? Color.accentColor.opacity(0.18) : .clear, in: .rect(cornerRadius: 6))
                    }
                    .help(Text(option.title))
                }
                Divider().frame(height: 18)
                ForEach(colors, id: \.self) { hex in
                    Button {
                        recolor(hex)
                    } label: {
                        Circle()
                            .fill(Color(hex: hex) ?? .primary)
                            .frame(width: 18, height: 18)
                            .overlay(Circle().strokeBorder(hex == color ? Color.accentColor : .secondary.opacity(0.3), lineWidth: 2))
                    }
                    .accessibilityLabel(Text(verbatim: hex))
                }
                Divider().frame(height: 18)
                ForEach(penWidths.indices, id: \.self) { index in
                    let name = penWidths[index].0
                    let value = penWidths[index].1
                    Button {
                        width = value
                    } label: {
                        Label {
                            Text(name)
                        } icon: {
                            Capsule()
                                .frame(width: 16, height: max(1.5, Ink.diameter(value, 0.5)))
                                .padding(4)
                                .background(width == value ? Color.accentColor.opacity(0.18) : .clear, in: .rect(cornerRadius: 6))
                        }
                    }
                    .help(Text(name))
                }
                Divider().frame(height: 18)
                ForEach(Array(favourites.enumerated()), id: \.offset) { index, pen in
                    Button {
                        use(pen)
                    } label: {
                        Label {
                            Text(pen.highlighter ? LocalizedStringKey("Highlighter") : LocalizedStringKey("Pen"))
                        } icon: {
                            let size = pen.highlighter ? 12 : min(14, max(5, Ink.diameter(pen.width, 0.5) * 2))
                            RoundedRectangle(cornerRadius: pen.highlighter ? 2 : size / 2)
                                .fill((Color(hex: pen.color) ?? .primary).opacity(pen.highlighter ? 0.6 : 1))
                                .frame(width: size, height: pen.highlighter ? 8 : size)
                                .padding(4)
                        }
                    }
                    .help(Text(pen.highlighter ? LocalizedStringKey("Highlighter") : LocalizedStringKey("Pen")))
                    .contextMenu {
                        Button("Remove from Favourites", systemImage: "star.slash", role: .destructive) {
                            var pens = favourites
                            pens.remove(at: index)
                            favourites = pens
                        }
                    }
                }
                if tool == .pen || tool == .highlighter {
                    Button("Add Pen to Favourites", systemImage: "star") {
                        let pen = FavouritePen(
                            highlighter: tool == .highlighter, color: tool == .highlighter ? highlighterColor : penColor,
                            width: tool == .highlighter ? highlighterWidth : width)
                        favourites = Array((favourites.filter { !$0.isSame(as: pen) } + [pen]).suffix(8))
                    }
                    .help("Add Pen to Favourites")
                }
                Divider().frame(height: 18)
                Toggle("Ink to Shape", isOn: $shapes)
                    .toggleStyle(.button)
                    .labelStyle(.titleOnly)
                Button("Insert Picture…", systemImage: "photo") { choosingPicture = true }
                    .help("Insert Picture…")
                Button("Ruler", systemImage: "ruler") { toggleGuide(.ruler) }
                    .padding(4)
                    .background(ruler == nil ? Color.clear : Color.accentColor.opacity(0.18), in: .rect(cornerRadius: 6))
                    .help("Ruler")
                Button("Protractor", systemImage: "angle") { toggleGuide(.protractor) }
                    .padding(4)
                    .background(protractor == nil ? Color.clear : Color.accentColor.opacity(0.18), in: .rect(cornerRadius: 6))
                    .help("Protractor")
                Picker("Paper", selection: Binding(get: { page?.paper ?? "blank" }, set: { setPaper($0) })) {
                    Text("Blank").tag("blank")
                    Text("Lines").tag("lines")
                    Text("Grid").tag("grid")
                    Text("Dots").tag("dots")
                }
                .fixedSize()
                Divider().frame(height: 18)
                Button("Undo", systemImage: "arrow.uturn.backward") { undo() }
                    .disabled(past.isEmpty)
                    .keyboardShortcut("z", modifiers: .command)
                Button("Redo", systemImage: "arrow.uturn.forward") { redo() }
                    .disabled(future.isEmpty)
                    .keyboardShortcut("z", modifiers: [.command, .shift])
                Button("Delete", systemImage: "trash") { removeSelection() }
                    .disabled(selection.isEmpty)
                    .keyboardShortcut(.delete, modifiers: .command)
                Button("Zoom Out", systemImage: "minus.magnifyingglass") { zoom = max(0.1, zoom / 1.25) }
                Text(verbatim: "\(Int((zoom * 100).rounded()))%").monospacedDigit().foregroundStyle(.secondary)
                Button("Zoom In", systemImage: "plus.magnifyingglass") { zoom = min(8, zoom * 1.25) }
                Menu {
                    Button("Picture (SVG)") { exportPicture(png: false) }
                    Button("Picture (PNG)") { exportPicture(png: true) }
                    Button { exportPDF() } label: { Text(verbatim: "PDF") }
                } label: {
                    Label("Export…", systemImage: "square.and.arrow.up")
                }
                .menuIndicator(.hidden)
                .fixedSize()
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

    /// Saves the drawing as an SVG or PNG picture.
    private func exportPicture(png: Bool) {
        saveNow()
        guard let page, let core = vault.core else { return }
        let name = ((path as NSString).lastPathComponent as NSString).deletingPathExtension
        let url = FileManager.default.temporaryDirectory.appending(path: "\(name).\(png ? "png" : "svg")")
        do {
            let data: Data
            if png {
                guard let picture = drawingPNG(page.items) else { throw CocoaError(.fileWriteUnknown) }
                data = picture
            } else {
                data = try Data(core.drawingSvg(path: path).utf8)
            }
            try data.write(to: url)
            exportFile = ExportFile(url: url)
        } catch {
            problem = CoreKit.message(for: error)
        }
    }

    /// Saves the drawing as a one-page PDF, its strokes as lines.
    private func exportPDF() {
        saveNow()
        guard let page else { return }
        let name = ((path as NSString).lastPathComponent as NSString).deletingPathExtension
        let url = FileManager.default.temporaryDirectory.appending(path: "\(name).pdf")
        do {
            guard let data = pdfData(DrawingPicture(items: page.items)) else { throw CocoaError(.fileWriteUnknown) }
            try data.write(to: url)
            exportFile = ExportFile(url: url)
        } catch {
            problem = CoreKit.message(for: error)
        }
    }

    // MARK: Favourite pens

    private var favourites: [FavouritePen] {
        get { (try? JSONDecoder().decode([FavouritePen].self, from: Data(favouritesJSON.utf8))) ?? [] }
        nonmutating set {
            if let data = try? JSONEncoder().encode(newValue) {
                favouritesJSON = String(decoding: data, as: UTF8.self)
            }
        }
    }

    private func use(_ pen: FavouritePen) {
        tool = pen.highlighter ? .highlighter : .pen
        if pen.highlighter {
            highlighterColor = pen.color
        } else {
            penColor = pen.color
            width = pen.width
        }
    }

    // MARK: Drawing

    private func world(_ p: CGPoint) -> CGPoint {
        CGPoint(x: (p.x - offset.x) / zoom, y: (p.y - offset.y) / zoom)
    }

    private func render(_ context: inout GraphicsContext, size: CGSize) {
        context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(.white))
        guard let page else { return }
        drawPaper(&context, paper: page.paper, size: size)
        context.translateBy(x: offset.x, y: offset.y)
        context.scaleBy(x: zoom, y: zoom)
        var hidden: Set<String> = []
        if case .erase(let removed)? = live { hidden = removed }
        let shown = transformed(page.items, by: live)
        let ordered = shown.filter(\.highlighter) + shown.filter { !$0.highlighter }
        for item in ordered where !hidden.contains(item.id) && item.id != entry?.id {
            drawItem(item, in: context)
        }
        switch live {
        case .draw(let points, let pressures, let pressure):
            drawItem(liveStroke(points, pressures: pressures, pressure: pressure), in: context)
        case .shape(let start, let end):
            drawItem(shapeItem(tool, from: start, to: end), in: context)
        case .rub(let points) where !points.isEmpty:
            var path = Path()
            path.addLines(points.count > 1 ? points : [points[0], points[0]])
            context.stroke(
                path, with: .color(Color.accentColor.opacity(0.22)),
                style: StrokeStyle(lineWidth: CGFloat(2 * rubReach / zoom), lineCap: .round, lineJoin: .round))
        case .lasso(let points) where points.count > 1:
            var path = Path()
            path.addLines(points)
            context.stroke(
                path, with: .color(.accentColor),
                style: StrokeStyle(lineWidth: 1 / zoom, dash: [CGFloat(6 / zoom), CGFloat(4 / zoom)]))
        default: break
        }
        if let area = shown.filter({ selection.contains($0.id) }).bounds {
            let box = area.insetBy(dx: -4 / zoom, dy: -4 / zoom)
            context.stroke(
                Path(box), with: .color(.accentColor),
                style: StrokeStyle(lineWidth: 1.5 / zoom, dash: [CGFloat(6 / zoom), CGFloat(4 / zoom)]))
            if tool == .lasso {
                let (corner, knob) = handles(area)
                let size = 9 / zoom
                let square = Path(CGRect(x: corner.x - size / 2, y: corner.y - size / 2, width: size, height: size))
                let circle = Path(ellipseIn: CGRect(x: knob.x - size / 2, y: knob.y - size / 2, width: size, height: size))
                var stem = Path()
                stem.move(to: CGPoint(x: knob.x, y: box.minY))
                stem.addLine(to: knob)
                context.stroke(stem, with: .color(.accentColor), lineWidth: 1.5 / zoom)
                for shape in [square, circle] {
                    context.fill(shape, with: .color(.white))
                    context.stroke(shape, with: .color(.accentColor), lineWidth: 1.5 / zoom)
                }
            }
        }
        for guide in [ruler, protractor].compactMap({ $0 }) {
            drawGuide(guide, in: context)
        }
    }

    /// A ruler marked in millimetres along both edges, or a protractor in
    /// degrees, with its angle and the knob it turns by.
    private func drawGuide(_ guide: Guide, in context: GraphicsContext) {
        var frame = context
        frame.translateBy(x: guide.centre.x, y: guide.centre.y)
        frame.rotate(by: .radians(guide.angle))
        let fill = Color(red: 0.91, green: 0.925, blue: 0.957).opacity(0.85)
        let thin = 1 / zoom
        var ticks = Path()
        let label = { (text: String, size: CGFloat) -> Text in
            Text(verbatim: text).font(.system(size: size)).foregroundStyle(Color.gray)
        }
        switch guide.kind {
        case .ruler:
            let (half, edge) = (guide.size / 2, Guide.rulerWidth / 2)
            let body = Path(CGRect(x: -half, y: -edge, width: guide.size, height: Guide.rulerWidth))
            frame.fill(body, with: .color(fill))
            frame.stroke(body, with: .color(.gray), lineWidth: thin)
            let millimetre = 96 / 25.4
            var mark = 0
            while Double(mark) * millimetre <= guide.size {
                let x = -half + Double(mark) * millimetre
                let tick = mark % 10 == 0 ? 14.0 : mark % 5 == 0 ? 9.0 : 5.0
                ticks.move(to: CGPoint(x: x, y: -edge))
                ticks.addLine(to: CGPoint(x: x, y: -edge + tick))
                ticks.move(to: CGPoint(x: x, y: edge))
                ticks.addLine(to: CGPoint(x: x, y: edge - tick))
                if mark % 10 == 0 {
                    frame.draw(label("\(mark / 10)", 10), at: CGPoint(x: x, y: -edge + 22))
                }
                mark += 1
            }
            frame.stroke(ticks, with: .color(.gray), lineWidth: thin)
            frame.draw(label("\(Guide.degrees(guide.angle))°", 12), at: CGPoint(x: 0, y: 6))
        case .protractor:
            let r = guide.size
            var body = Path()
            body.move(to: CGPoint(x: -r, y: 0))
            body.addArc(center: .zero, radius: r, startAngle: .degrees(180), endAngle: .degrees(360), clockwise: false)
            body.closeSubpath()
            frame.fill(body, with: .color(fill))
            frame.stroke(body, with: .color(.gray), lineWidth: thin)
            for degree in 0...180 {
                let a = Double.pi + Double(degree) * .pi / 180
                let tick = degree % 10 == 0 ? 14.0 : degree % 5 == 0 ? 9.0 : 5.0
                ticks.move(to: CGPoint(x: cos(a) * r, y: sin(a) * r))
                ticks.addLine(to: CGPoint(x: cos(a) * (r - tick), y: sin(a) * (r - tick)))
                if degree % 30 == 0 {
                    frame.draw(label("\(180 - degree)", 10), at: CGPoint(x: cos(a) * (r - 26), y: sin(a) * (r - 26)))
                }
            }
            ticks.move(to: CGPoint(x: -6, y: 0))
            ticks.addLine(to: CGPoint(x: 6, y: 0))
            ticks.move(to: CGPoint(x: 0, y: -6))
            ticks.addLine(to: .zero)
            frame.stroke(ticks, with: .color(.gray), lineWidth: thin)
            frame.draw(label("\(Guide.degrees(guide.angle))°", 12), at: CGPoint(x: 0, y: -r / 2.6))
        }
        let knob = guide.kind == .ruler ? CGPoint(x: guide.size / 2 - 22, y: 0) : CGPoint(x: guide.size - 22, y: -14)
        let circle = Path(ellipseIn: CGRect(x: knob.x - 9, y: knob.y - 9, width: 18, height: 18))
        frame.fill(circle, with: .color(.white))
        frame.stroke(circle, with: .color(.accentColor), lineWidth: 1.5 / zoom)
        var arrow = Path()
        arrow.addArc(center: knob, radius: 4.5, startAngle: .radians(-.pi * 0.9), endAngle: .radians(.pi * 0.4), clockwise: false)
        frame.stroke(arrow, with: .color(.accentColor), lineWidth: 1.5 / zoom)
    }

    /// Lays a ruler or protractor in the middle of the view, or takes it away.
    /// Puts a picture from a file in the middle of the view, to move with
    /// the lasso.
    private func insertPicture(_ file: URL) {
        guard let page, let picture = pictureURL(file) else {
            problem = String(localized: "This file is not a picture.")
            return
        }
        let (url, size) = (picture.url, picture.size)
        let scale = min(1, 480 / max(1, size.width), 360 / max(1, size.height))
        let (width, height) = ((size.width * scale).rounded(), (size.height * scale).rounded())
        let middle = world(CGPoint(x: canvasSize.width / 2, y: canvasSize.height / 2))
        var item = DrawingItem.blank("image")
        item.x = middle.x - width / 2
        item.y = middle.y - height / 2
        item.width = width
        item.height = height
        item.image = url
        setItems(page.items + [item])
        tool = .lasso
    }

    private func toggleGuide(_ kind: Guide.Kind) {
        let middle = world(CGPoint(x: canvasSize.width / 2, y: canvasSize.height / 2))
        switch kind {
        case .ruler:
            ruler = ruler == nil ? Guide(kind: .ruler, centre: middle, angle: 0, size: 600) : nil
        case .protractor:
            protractor = protractor == nil ? Guide(kind: .protractor, centre: middle, angle: 0, size: 160) : nil
        }
    }

    private func updateGuide(_ kind: Guide.Kind, _ change: (inout Guide) -> Void) {
        if kind == .ruler, var guide = ruler {
            change(&guide)
            ruler = guide
        } else if kind == .protractor, var guide = protractor {
            change(&guide)
            protractor = guide
        }
    }

    /// Where a selection's handles are: the corner that scales it and the
    /// knob above it that turns it.
    private func handles(_ area: CGRect) -> (corner: CGPoint, knob: CGPoint) {
        let pad = 4 / zoom
        return (CGPoint(x: area.maxX + pad, y: area.maxY + pad), CGPoint(x: area.midX, y: area.minY - pad - 18 / zoom))
    }

    /// The items with the selection where a move, scale or turn has it so far.
    private func transformed(_ items: [DrawingItem], by gesture: InkGesture?) -> [DrawingItem] {
        switch gesture {
        case .move(let start, let at):
            return items.moving(selection, dx: at.x - start.x, dy: at.y - start.y)
        case .scale(let origin, let start, let at):
            let sx = (at.x - origin.x) / max(1e-6, start.x - origin.x)
            let sy = (at.y - origin.y) / max(1e-6, start.y - origin.y)
            return items.scaling(selection, from: origin, by: max(0.05, (sx + sy) / 2))
        case .rotate(let pivot, let start, let at):
            return items.turning(selection, about: pivot, by: at - start)
        default:
            return items
        }
    }

    private func drawPaper(_ context: inout GraphicsContext, paper: String, size: CGSize) {
        let step = CGFloat(spacing * zoom)
        guard paper != "blank", step >= 6 else { return }
        let startX = (offset.x.truncatingRemainder(dividingBy: step) + step).truncatingRemainder(dividingBy: step) - step
        let startY = (offset.y.truncatingRemainder(dividingBy: step) + step).truncatingRemainder(dividingBy: step) - step
        var path = Path()
        if paper == "dots" {
            for x in stride(from: startX, through: size.width + step, by: step) {
                for y in stride(from: startY, through: size.height + step, by: step) {
                    path.addRect(CGRect(x: x - 1, y: y - 1, width: 2, height: 2))
                }
            }
            context.fill(path, with: .color(Color(red: 0.77, green: 0.78, blue: 0.81)))
            return
        }
        for y in stride(from: startY, through: size.height + step, by: step) {
            path.move(to: CGPoint(x: 0, y: y))
            path.addLine(to: CGPoint(x: size.width, y: y))
        }
        if paper == "grid" {
            for x in stride(from: startX, through: size.width + step, by: step) {
                path.move(to: CGPoint(x: x, y: 0))
                path.addLine(to: CGPoint(x: x, y: size.height))
            }
        }
        context.stroke(path, with: .color(Color(red: 0.89, green: 0.9, blue: 0.92)), lineWidth: 1)
    }

    private func liveStroke(_ points: [CGPoint], pressures: [Double], pressure: Bool) -> DrawingItem {
        let highlighter = tool == .highlighter
        return Ink.stroke(
            points, pressures: pressure && !highlighter ? pressures : [],
            color: highlighter ? highlighterColor : penColor,
            width: highlighter ? highlighterWidth : width, highlighter: highlighter)
    }

    /// A line, arrow or shape from one corner (or end) to the other.
    private func shapeItem(_ kind: DrawTool, from start: CGPoint, to end: CGPoint) -> DrawingItem {
        var item = DrawingItem.blank(kind.rawValue)
        item.strokeColor = penColor
        item.strokeWidth = max(1, (Ink.diameter(width, 0.5) * 2).rounded() / 2)
        item.width = abs(end.x - start.x)
        item.height = abs(end.y - start.y)
        if kind == .line || kind == .arrow {
            item.x = start.x
            item.y = start.y
            item.points = [DrawingPoint(x: 0, y: 0), DrawingPoint(x: end.x - start.x, y: end.y - start.y)]
            item.arrowhead = kind == .arrow
        } else {
            item.x = min(start.x, end.x)
            item.y = min(start.y, end.y)
        }
        return item
    }

    // MARK: Input

    private var callbacks: InkCallbacks {
        InkCallbacks(
            begin: { begin($0, $1) },
            move: { move($0) },
            end: { end() },
            cancel: {
                live = nil
                track = nil
            },
            pan: { delta in offset = CGPoint(x: offset.x + delta.dx, y: offset.y + delta.dy) },
            zoom: { factor, at in zoomBy(factor, at: at) })
    }

    private func zoomBy(_ factor: Double, at point: CGPoint) {
        let next = min(8, max(0.1, zoom * factor))
        offset = CGPoint(
            x: point.x - (point.x - offset.x) / zoom * next, y: point.y - (point.y - offset.y) / zoom * next)
        zoom = next
    }

    private func begin(_ sample: InkSample, _ pointer: InkPointer) {
        guard let page else { return }
        if pointer == .pen { penSeen = true }
        let at = world(sample.location)
        // A ruler or protractor turns by its knob and moves by its body.
        if pointer != .middle, pointer != .eraser {
            let guides = [protractor, ruler].compactMap { $0 }
            if let guide = guides.first(where: { Ink.distance(at, $0.knob) <= 14 / zoom }) {
                let start = atan2(at.y - guide.centre.y, at.x - guide.centre.x)
                live = .guideTurn(guide.kind, start: start, from: guide.angle)
                return
            }
            if let guide = guides.first(where: { $0.holds(at) }) {
                live = .guideMove(guide.kind, start: at, from: guide.centre)
                return
            }
        }
        if pointer == .middle || (pointer == .finger && penSeen) {
            live = .pan(last: sample.location)
            return
        }
        if tool == .rub {
            live = .rub([at])
            return
        }
        if pointer == .eraser || tool == .eraser {
            live = .erase(Set(page.items.filter { $0.hits(at, slop: 6 / zoom) }.map(\.id)))
            return
        }
        switch tool {
        case .pen, .highlighter:
            selection = []
            track = [ruler, protractor].compactMap { $0?.track(from: at, reach: 16 / zoom) }.first
            let first = track?.place(at) ?? at
            live = .draw(points: [first], pressures: [sample.pressure ?? 0.5], pressure: sample.pressure != nil)
        case .lasso:
            let area = page.items.filter({ selection.contains($0.id) }).bounds
            let grips = area.map(handles)
            if let area, let grips, Ink.distance(at, grips.corner) <= 9 / zoom {
                live = .scale(origin: area.origin, start: grips.corner, at: at)
            } else if let area, let grips, Ink.distance(at, grips.knob) <= 9 / zoom {
                let pivot = CGPoint(x: area.midX, y: area.midY)
                let angle = atan2(at.y - pivot.y, at.x - pivot.x)
                live = .rotate(pivot: pivot, start: angle, at: angle)
            } else if let area, area.contains(at) {
                live = .move(start: at, at: at)
            } else {
                selection = []
                live = .lasso([at])
            }
        case .text:
            let existing = page.items.reversed().first { $0.kind == "text" && $0.hits(at, slop: 4 / zoom) }
            entry = TextEntry(
                id: existing?.id, at: existing.map { CGPoint(x: $0.x, y: $0.y) } ?? at, value: existing?.text ?? "")
        default:
            selection = []
            live = .shape(start: at, end: at)
        }
    }

    private func move(_ samples: [InkSample]) {
        guard let page, let gesture = live else { return }
        switch gesture {
        case .pan(let last):
            guard let sample = samples.last else { return }
            offset = CGPoint(x: offset.x + sample.location.x - last.x, y: offset.y + sample.location.y - last.y)
            live = .pan(last: sample.location)
        case .draw(var points, var pressures, let pressure):
            for sample in samples {
                let at = track?.place(world(sample.location)) ?? world(sample.location)
                if let previous = points.last, Ink.distance(at, previous) < 0.6 / zoom { continue }
                points.append(at)
                pressures.append(sample.pressure ?? 0.5)
            }
            live = .draw(points: points, pressures: pressures, pressure: pressure)
        case .erase(var removed):
            for sample in samples {
                let at = world(sample.location)
                for item in page.items where !removed.contains(item.id) && item.hits(at, slop: 6 / zoom) {
                    removed.insert(item.id)
                }
            }
            live = .erase(removed)
        case .rub(var points):
            points.append(contentsOf: samples.map { world($0.location) })
            live = .rub(points)
        case .lasso(var points):
            points.append(contentsOf: samples.map { world($0.location) })
            live = .lasso(points)
        case .move(let start, _):
            guard let sample = samples.last else { return }
            live = .move(start: start, at: world(sample.location))
        case .scale(let origin, let start, _):
            guard let sample = samples.last else { return }
            live = .scale(origin: origin, start: start, at: world(sample.location))
        case .rotate(let pivot, let start, _):
            guard let sample = samples.last else { return }
            let at = world(sample.location)
            live = .rotate(pivot: pivot, start: start, at: atan2(at.y - pivot.y, at.x - pivot.x))
        case .shape(let start, _):
            guard let sample = samples.last else { return }
            live = .shape(start: start, end: world(sample.location))
        case .guideMove(let kind, let start, let from):
            guard let sample = samples.last else { return }
            let at = world(sample.location)
            updateGuide(kind) { $0.centre = CGPoint(x: from.x + at.x - start.x, y: from.y + at.y - start.y) }
        case .guideTurn(let kind, let start, let from):
            guard let sample = samples.last, let guide = kind == .ruler ? ruler : protractor else { return }
            let at = world(sample.location)
            var angle = from + atan2(at.y - guide.centre.y, at.x - guide.centre.x) - start
            // Close to a multiple of 15°, it settles there.
            let step = Double.pi / 12
            let near = (angle / step).rounded() * step
            if abs(angle - near) < .pi / 90 { angle = near }
            updateGuide(kind) { $0.angle = angle }
        }
    }

    private func end() {
        guard let page, let gesture = live else { return }
        live = nil
        let guided = track != nil
        track = nil
        let items = page.items
        switch gesture {
        case .draw(let points, let pressures, let pressure):
            let highlighter = tool == .highlighter
            let shape = shapes && !highlighter && !guided ? Ink.recognize(points) : nil
            switch shape {
            case .line(let from, let to):
                setItems(items + [shapeItem(.line, from: from, to: to)])
            case .rectangle(let box):
                setItems(items + [shapeItem(.rectangle, from: box.origin, to: CGPoint(x: box.maxX, y: box.maxY))])
            case .ellipse(let box):
                setItems(items + [shapeItem(.ellipse, from: box.origin, to: CGPoint(x: box.maxX, y: box.maxY))])
            case nil:
                setItems(items + [liveStroke(points, pressures: pressures, pressure: pressure)])
            }
        case .erase(let removed):
            if !removed.isEmpty { setItems(items.filter { !removed.contains($0.id) }) }
        case .rub(let points):
            let path = points.map { DrawingPoint(x: Double($0.x), y: Double($0.y)) }
            let left = drawingErase(items: items, path: path, radius: rubReach / zoom)
            if left != items { setItems(left) }
        case .lasso(let points):
            let chosen =
                points.count > 3
                ? items.filter({ $0.isLassoed(by: points) })
                : Array(items.reversed().filter({ $0.hits(points[0], slop: 4 / zoom) }).prefix(1))
            selection = Set(chosen.map(\.id))
        case .move, .scale, .rotate:
            let next = transformed(items, by: gesture)
            if next != items { setItems(next) }
        case .shape(let start, let end):
            if Ink.distance(start, end) > 3 / zoom { setItems(items + [shapeItem(tool, from: start, to: end)]) }
        case .pan, .guideMove, .guideTurn:
            break
        }
    }

    // MARK: Changes

    private func load() {
        do {
            guard let core = vault.core else { return }
            page = try core.readDrawing(path: path)
            past = []
            future = []
            fitted = false
        } catch {
            problem = CoreKit.message(for: error)
        }
    }

    /// Shows the whole drawing once it is there.
    private func fit(in size: CGSize) {
        guard let page, !fitted, size.width > 0 else { return }
        fitted = true
        guard let area = page.items.bounds else { return }
        let next = min(1, max(0.1, min(size.width / (area.width + 80), size.height / (area.height + 80))))
        zoom = next
        offset = CGPoint(x: size.width / 2 - area.midX * next, y: size.height / 2 - area.midY * next)
    }

    /// Shows a change now, keeps it for undo, and saves it shortly after.
    private func change(_ next: DrawingPage, undoable: Bool = true) {
        if undoable, let page {
            past = Array((past + [page.items]).suffix(200))
            future = []
        }
        page = next
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
        guard let page, let core = vault.core else { return }
        do {
            try core.writeDrawing(path: path, page: page)
        } catch {
            problem = CoreKit.message(for: error)
        }
    }

    private func setItems(_ items: [DrawingItem]) {
        guard var next = page else { return }
        next.items = items
        change(next)
    }

    private func setPaper(_ paper: String) {
        guard var next = page else { return }
        next.paper = paper
        change(next)
    }

    private func undo() {
        guard var next = page, let previous = past.popLast() else { return }
        future.append(next.items)
        next.items = previous
        selection = []
        change(next, undoable: false)
    }

    private func redo() {
        guard var next = page, let following = future.popLast() else { return }
        past.append(next.items)
        next.items = following
        selection = []
        change(next, undoable: false)
    }

    private func removeSelection() {
        guard let page, !selection.isEmpty else { return }
        setItems(page.items.filter { !selection.contains($0.id) })
        selection = []
    }

    private func recolor(_ hex: String) {
        if tool == .highlighter { highlighterColor = hex } else { penColor = hex }
        guard let page, !selection.isEmpty else { return }
        setItems(page.items.map { item in
            guard selection.contains(item.id) else { return item }
            var colored = item
            colored.strokeColor = hex
            return colored
        })
    }

    private func commitText() {
        guard let entry, let page else { return }
        self.entry = nil
        let text = entry.value.trimmingCharacters(in: .whitespacesAndNewlines)
        let existing = page.items.first { $0.id == entry.id }
        if text.isEmpty {
            if let existing { setItems(page.items.filter { $0.id != existing.id }) }
            return
        }
        var item = existing ?? DrawingItem.blank("text")
        if existing == nil {
            item.x = entry.at.x
            item.y = entry.at.y
            item.strokeColor = penColor
        }
        item.text = text
        let size = measure(text, size: item.fontSize)
        item.width = size.width
        item.height = size.height
        setItems(existing == nil ? page.items + [item] : page.items.map { $0.id == item.id ? item : $0 })
    }
}

extension Array {
    /// The element at an index, if there is one.
    fileprivate subscript(safe index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}
