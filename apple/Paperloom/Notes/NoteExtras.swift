import CoreKit
import SwiftUI
import UniformTypeIdentifiers

/// A note exported to the temporary folder (a Word or LaTeX document), for
/// the file exporter.
struct ExportFile: FileDocument {
    static let word = UTType(filenameExtension: "docx") ?? .data
    /// OpenDocument text, as LibreOffice and WPS write it.
    static let openDocument = UTType(filenameExtension: "odt") ?? .data
    static let latex = UTType(filenameExtension: "tex", conformingTo: .plainText) ?? .plainText
    static let xmind = UTType(filenameExtension: "xmind") ?? .data
    static let opml = UTType(filenameExtension: "opml") ?? .xml
    /// FreeMind's `.mm` (which the system may know as Objective-C++).
    static let freeMind = UTType(filenameExtension: "mm") ?? .xml
    static let drawio = UTType(filenameExtension: "drawio") ?? .xml
    static var readableContentTypes: [UTType] {
        [word, openDocument, latex, .pdf, xmind, opml, freeMind, drawio, .svg, .png]
    }
    let url: URL

    var contentType: UTType {
        switch url.pathExtension.lowercased() {
        case "tex": Self.latex
        case "odt": Self.openDocument
        case "pdf": .pdf
        case "xmind": Self.xmind
        case "opml": Self.opml
        case "mm": Self.freeMind
        case "drawio": Self.drawio
        case "svg": .svg
        case "png": .png
        default: Self.word
        }
    }

    init(url: URL) {
        self.url = url
    }

    init(configuration: ReadConfiguration) throws {
        throw CocoaError(.featureUnsupported)
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        try FileWrapper(url: url)
    }
}

/// Picks a library item to cite as `[@key]`.
struct CitationPicker: View {
    let onPick: (String) -> Void
    @Environment(LibraryModel.self) private var library
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""

    var body: some View {
        NavigationStack {
            List(library.citationCompletions(query), id: \.key) { item in
                Button {
                    onPick(item.key)
                    dismiss()
                } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(item.title).lineLimit(2)
                        Text(verbatim: "\(item.cite) · @\(item.key)").font(.caption).foregroundStyle(.secondary)
                    }
                }
                .buttonStyle(.plain)
            }
            .searchable(text: $query, prompt: Text("Title, authors, year"))
            .navigationTitle("Cite")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 480, minHeight: 420)
        #endif
    }
}

/// The open note's properties (front matter) as fields to edit.
struct NotePropertiesSection: View {
    let properties: [NoteProperty]
    @Environment(VaultModel.self) private var vault
    @State private var rows: [Row] = []

    /// A property being edited; its own identity, so typing a key keeps
    /// the field and removing a row never trips over an index.
    private struct Row: Identifiable, Equatable {
        let id = UUID()
        var key: String
        var value: String

        static func == (a: Row, b: Row) -> Bool { a.key == b.key && a.value == b.value }
    }

    private var edited: [NoteProperty] { rows.map { NoteProperty(key: $0.key, value: $0.value) } }

    var body: some View {
        Section("Properties") {
            ForEach($rows) { $row in
                HStack(spacing: 6) {
                    TextField("Property", text: $row.key)
                        .autocorrectionDisabled()
                        .frame(maxWidth: 110)
                    TextField("Value", text: $row.value)
                    Button("Remove", systemImage: "minus.circle") {
                        rows.removeAll { $0.id == row.id }
                    }
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                }
                .textFieldStyle(.roundedBorder)
                .font(.callout)
            }
            HStack {
                Button("Add Property", systemImage: "plus") {
                    rows.append(Row(key: "", value: ""))
                }
                Spacer()
                if edited != properties {
                    Button("Save") { vault.setProperties(edited) }
                        .buttonStyle(.borderedProminent)
                }
            }
            .buttonStyle(.borderless)
        }
        .onAppear(perform: reset)
        .onChange(of: properties) { reset() }
    }

    private func reset() {
        rows = properties.map { Row(key: $0.key, value: $0.value) }
    }
}

/// The open note's earlier versions: what changed since each, and going
/// back.
struct NoteHistoryView: View {
    @Environment(VaultModel.self) private var vault
    @State private var versions: [NoteVersion] = []
    @State private var chosen: NoteVersion?
    @State private var lines: [DiffLine] = []
    @State private var confirming = false

    var body: some View {
        List {
            if versions.isEmpty {
                Text("No earlier versions yet. Before a note changes, it is kept as it was, at most every five minutes.")
                    .foregroundStyle(.secondary)
            }
            ForEach(versions, id: \.time) { version in
                Button {
                    chosen = version
                    lines = vault.compare(with: version)
                } label: {
                    HStack {
                        Text(Self.date(version.time), format: .dateTime.year().month().day().hour().minute())
                        Spacer()
                        if chosen == version {
                            Image(systemName: "checkmark").foregroundStyle(.tint)
                        }
                    }
                }
                .buttonStyle(.plain)
            }
            if let chosen {
                Section("What changed since then") {
                    ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                        if line.change != "same" {
                            Text(verbatim: (line.change == "added" ? "+ " : "- ") + line.text)
                                .font(.caption.monospaced())
                                .foregroundStyle(line.change == "added" ? Color.green : Color.red)
                        }
                    }
                    Button("Restore This Version") { confirming = true }
                        .confirmationDialog(
                            "Restore This Version", isPresented: $confirming, titleVisibility: .visible
                        ) {
                            Button("Restore This Version") {
                                vault.restore(chosen)
                                reload()
                            }
                        } message: {
                            Text("What the note says now is kept as a version too.")
                        }
                }
            }
        }
        .onAppear(perform: reload)
        .onChange(of: vault.path) { reload() }
    }

    private func reload() {
        versions = vault.versions()
        chosen = nil
        lines = []
    }

    private static func date(_ milliseconds: Int64) -> Date {
        Date(timeIntervalSince1970: TimeInterval(milliseconds) / 1000)
    }
}

/// Notes, the papers they cite and links still to write, as the core laid
/// them out. Drag to move around, pinch to zoom, tap to open.
struct NoteGraphView: View {
    /// A note to draw around; the whole vault without one.
    let center: String?
    var depth: UInt32 = 1
    var compact = false
    @Environment(VaultModel.self) private var vault
    @Environment(TabsModel.self) private var tabs
    @State private var graph: NoteGraph?
    @State private var zoom: CGFloat = 1
    @State private var pan: CGSize = .zero
    @GestureState private var dragging: CGSize = .zero
    @GestureState private var pinching: CGFloat = 1

    var body: some View {
        GeometryReader { geometry in
            let frame = GraphFrame(
                graph: graph, size: geometry.size, zoom: zoom * pinching,
                pan: CGSize(width: pan.width + dragging.width, height: pan.height + dragging.height))
            Canvas { context, _ in
                draw(frame, in: &context)
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture()
                    .updating($dragging) { value, state, _ in state = value.translation }
                    .onEnded { value in
                        pan.width += value.translation.width
                        pan.height += value.translation.height
                    }
            )
            .simultaneousGesture(
                MagnifyGesture()
                    .updating($pinching) { value, state, _ in state = value.magnification }
                    .onEnded { value in zoom = min(max(zoom * value.magnification, 0.2), 8) }
            )
            .onTapGesture(coordinateSpace: .local) { location in
                if let node = frame.node(at: location) { open(node) }
            }
        }
        .overlay(alignment: .bottomTrailing) {
            HStack(spacing: 4) {
                Button("Zoom Out", systemImage: "minus.magnifyingglass") { zoom = max(zoom / 1.4, 0.2) }
                Button("Zoom In", systemImage: "plus.magnifyingglass") { zoom = min(zoom * 1.4, 8) }
                Button("Fit", systemImage: "arrow.up.left.and.down.right.magnifyingglass") {
                    zoom = 1
                    pan = .zero
                }
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .padding(8)
        }
        .overlay {
            if let graph, graph.nodes.isEmpty {
                ContentUnavailableView("Nothing to draw yet.", systemImage: "point.3.connected.trianglepath.dotted")
            }
        }
        .task(id: "\(center ?? "")|\(vault.notes.count)") {
            graph = await vault.graph(around: center, depth: depth)
            zoom = 1
            pan = .zero
        }
        .navigationTitle(compact ? "" : String(localized: "Graph"))
    }

    private func draw(_ frame: GraphFrame, in context: inout GraphicsContext) {
        guard let graph else { return }
        var edges = Path()
        for edge in graph.edges {
            edges.move(to: frame.point(Int(edge.from)))
            edges.addLine(to: frame.point(Int(edge.to)))
        }
        context.stroke(edges, with: .color(.secondary.opacity(0.4)), lineWidth: 1)
        let labelled = compact || frame.scale * zoom > 1.2
        for (index, node) in graph.nodes.enumerated() {
            let point = frame.point(index)
            let radius = frame.radius(node)
            let dot = Path(ellipseIn: CGRect(x: point.x - radius, y: point.y - radius, width: radius * 2, height: radius * 2))
            switch node.kind {
            case "item": context.fill(dot, with: .color(.orange))
            case "missing": context.stroke(dot, with: .color(.secondary), style: StrokeStyle(lineWidth: 1, dash: [2, 2]))
            default: context.fill(dot, with: .color(.accentColor))
            }
            if node.id == center {
                context.stroke(dot, with: .color(.primary), lineWidth: 2)
            }
            if labelled || node.degree >= 3 || node.id == center {
                context.draw(
                    Text(node.title).font(.caption2).foregroundStyle(.primary),
                    at: CGPoint(x: point.x, y: point.y + radius + 8))
            }
        }
    }

    private func open(_ node: GraphNodeInfo) {
        switch node.kind {
        case "note":
            vault.open(node.id)
        case "item":
            if let id = node.itemId { tabs.open(.item(id: id, title: node.title)) }
            return
        default:
            vault.createNote(title: node.title)
        }
        if !compact { tabs.open(.notes) }
    }
}

/// Where the graph's nodes land on screen.
private struct GraphFrame {
    let graph: NoteGraph?
    let size: CGSize
    let zoom: CGFloat
    let pan: CGSize
    /// Screen points per graph unit at zoom 1: the graph fits.
    let scale: CGFloat
    let middle: CGPoint

    init(graph: NoteGraph?, size: CGSize, zoom: CGFloat, pan: CGSize) {
        self.graph = graph
        self.size = size
        self.zoom = zoom
        self.pan = pan
        let nodes = graph?.nodes ?? []
        let xs = nodes.map { CGFloat($0.x) }
        let ys = nodes.map { CGFloat($0.y) }
        let (left, right) = (xs.min() ?? -100, xs.max() ?? 100)
        let (top, bottom) = (ys.min() ?? -100, ys.max() ?? 100)
        let width = max(right - left + 80, 120)
        let height = max(bottom - top + 80, 120)
        scale = min(size.width / width, size.height / height)
        middle = CGPoint(x: (left + right) / 2, y: (top + bottom) / 2)
    }

    func point(_ index: Int) -> CGPoint {
        guard let node = graph?.nodes[index] else { return .zero }
        return CGPoint(
            x: size.width / 2 + (CGFloat(node.x) - middle.x) * scale * zoom + pan.width,
            y: size.height / 2 + (CGFloat(node.y) - middle.y) * scale * zoom + pan.height)
    }

    func radius(_ node: GraphNodeInfo) -> CGFloat {
        4 + CGFloat(Double(node.degree).squareRoot()) * 2
    }

    /// The node under a tap, if any.
    func node(at location: CGPoint) -> GraphNodeInfo? {
        guard let nodes = graph?.nodes else { return nil }
        var best: (GraphNodeInfo, CGFloat)?
        for (index, node) in nodes.enumerated() {
            let p = point(index)
            let distance = hypot(p.x - location.x, p.y - location.y)
            if distance <= radius(node) + 8, distance < (best?.1 ?? .infinity) {
                best = (node, distance)
            }
        }
        return best?.0
    }
}

/// Daily notes and templates in Settings; kept in the vault's `.obsidian`
/// folder, where Obsidian reads the same settings.
struct NotesSettingsSection: View {
    @Environment(VaultModel.self) private var vault
    @Environment(LibraryModel.self) private var library
    @State private var settings: VaultSettings?

    var body: some View {
        Section("Notes") {
            if let binding = Binding($settings) {
                NamedField("Daily notes folder", text: binding.dailyFolder, prompt: Text("The vault's top folder"))
                NamedField("Daily note name", text: binding.dailyFormat, prompt: Text(verbatim: "YYYY-MM-DD"))
                NamedField("Daily note template", text: binding.dailyTemplate, prompt: Text(verbatim: "Templates/Daily"))
                NamedField("Templates folder", text: binding.templatesFolder, prompt: Text(verbatim: "Templates"))
                NamedField("Date format", text: binding.dateFormat, prompt: Text(verbatim: "YYYY-MM-DD"))
                NamedField("Time format", text: binding.timeFormat, prompt: Text(verbatim: "HH:mm"))
                Picker("Citations in notes", selection: binding.citationStyle) {
                    ForEach(library.citationStyles, id: \.id) { style in
                        Text(verbatim: style.name).tag(style.id)
                    }
                }
                Text("Formats are Moment.js patterns, as in Obsidian, which shares these settings with this vault.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .autocorrectionDisabled()
        .onAppear { settings = vault.settings() }
        // Saved when changed, not when first read.
        .onChange(of: settings) { old, new in
            if old != nil, let new { vault.save(settings: new) }
        }
    }
}

/// A view drawn as a one-page PDF its own size: lines and text stay as
/// lines and text.
@MainActor
func pdfData<Content: View>(_ content: Content) -> Data? {
    let renderer = ImageRenderer(content: content)
    let data = NSMutableData()
    renderer.render { size, render in
        var box = CGRect(origin: .zero, size: size)
        guard let consumer = CGDataConsumer(data: data as CFMutableData),
            let context = CGContext(consumer: consumer, mediaBox: &box, nil)
        else { return }
        context.beginPDFPage(nil)
        render(context)
        context.endPDFPage()
        context.closePDF()
    }
    return data.length > 0 ? data as Data : nil
}
