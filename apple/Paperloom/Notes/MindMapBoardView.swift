import CoreKit
import ImageIO
import SwiftUI
import UniformTypeIdentifiers

/// A map's theme in SwiftUI colours. The classic theme follows the app's
/// own look: its accent colour, light or dark.
struct MapPalette {
    /// The paper; none for the classic theme.
    let paper: Color?
    let centre: Color
    let centreText: Color
    /// Main topics filled with their branch's colour.
    let filled: Bool
    /// Outlined topics' fill; none for the view's own background.
    let topic: Color?
    let text: Color
    let line: Color
    let branches: [Color]

    init(_ theme: MindMapTheme) {
        guard theme.id != "classic" else {
            paper = nil
            centre = .accentColor
            centreText = .white
            filled = false
            topic = nil
            text = .primary
            line = .secondary
            branches = [.indigo, .red, .green, .orange, .cyan, .purple]
            return
        }
        let color = { (hex: String) in Color(hex: hex) ?? .gray }
        paper = color(theme.background)
        centre = color(theme.centre)
        centreText = color(theme.centreText)
        filled = theme.filled
        topic = color(theme.topic)
        text = color(theme.text)
        line = color(theme.line)
        branches = theme.branches.map(color)
    }

    /// The colour of the branch a topic (`0.2.1`) is on.
    func branch(_ id: String) -> Color {
        let parts = id.split(separator: ".")
        let index = parts.count > 1 ? Int(parts[1]) ?? 0 : 0
        return branches.isEmpty ? .accentColor : branches[index % branches.count]
    }
}

/// SVG path data split into its command letters and numbers.
func svgTokens(_ data: String) -> [Substring] {
    var spaced = ""
    for character in data {
        if character.isLetter {
            spaced += " \(character) "
        } else {
            spaced.append(character == "," ? " " : character)
        }
    }
    return spaced.split(separator: " ")
}

/// A line or outline from the SVG path data the core lays out (`M`, `L`,
/// `H`, `V`, `Q`, `C` and `Z` commands).
struct EdgeLine: Shape {
    let data: String

    func path(in rect: CGRect) -> Path {
        var path = Path()
        var command = "M"
        var values: [Double] = []
        var current = CGPoint.zero
        for token in svgTokens(data) {
            if let letter = token.first, letter.isLetter {
                command = String(letter)
                values = []
                if command == "Z" { path.closeSubpath() }
                continue
            }
            guard let value = Double(token) else { continue }
            values.append(value)
            let numbers = values
            let point = { (i: Int) in CGPoint(x: numbers[i], y: numbers[i + 1]) }
            switch (command, numbers.count) {
            case ("M", 2):
                current = point(0)
                path.move(to: current)
            case ("L", 2):
                current = point(0)
                path.addLine(to: current)
            case ("H", 1):
                current = CGPoint(x: numbers[0], y: current.y)
                path.addLine(to: current)
            case ("V", 1):
                current = CGPoint(x: current.x, y: numbers[0])
                path.addLine(to: current)
            case ("Q", 4):
                current = point(2)
                path.addQuadCurve(to: current, control: point(0))
            case ("C", 6):
                current = point(4)
                path.addCurve(to: current, control1: point(0), control2: point(2))
            default:
                continue
            }
            values = []
        }
        return path
    }
}

/// The head of a relation's arrow, where its curve ends.
private struct ArrowHead: Shape {
    let data: String

    func path(in rect: CGRect) -> Path {
        let numbers = svgTokens(data).compactMap { Double($0) }
        guard numbers.count >= 4 else { return Path() }
        let n = numbers.count
        let (cx, cy, ex, ey) = (numbers[n - 4], numbers[n - 3], numbers[n - 2], numbers[n - 1])
        let angle = atan2(ey - cy, ex - cx)
        let (size, spread) = (9.0, 0.45)
        var path = Path()
        path.move(to: CGPoint(x: ex, y: ey))
        path.addLine(to: CGPoint(x: ex - size * cos(angle - spread), y: ey - size * sin(angle - spread)))
        path.addLine(to: CGPoint(x: ex - size * cos(angle + spread), y: ey - size * sin(angle + spread)))
        path.closeSubpath()
        return path
    }
}

/// Relations, summaries and boundaries, drawn over the branches.
private struct ExtraShapes: View {
    let extras: [MindExtra]
    let palette: MapPalette
    var picked: UInt32?
    var onPick: ((MindExtra) -> Void)?

    var body: some View {
        ForEach(extras, id: \.index) { extra in
            let isPicked = extra.index == picked
            let color = isPicked ? Color.accentColor : palette.line
            let width: CGFloat = isPicked ? 2.5 : 1.5
            Group {
                switch extra.kind {
                case "boundary":
                    EdgeLine(data: extra.path)
                        .fill(palette.centre.opacity(0.06))
                        .overlay {
                            EdgeLine(data: extra.path).stroke(
                                isPicked ? Color.accentColor : palette.centre.opacity(0.55),
                                style: StrokeStyle(lineWidth: width, dash: [6, 4]))
                        }
                case "relation":
                    ZStack {
                        EdgeLine(data: extra.path)
                            .stroke(color, style: StrokeStyle(lineWidth: width, lineCap: .round, dash: [5, 4]))
                        ArrowHead(data: extra.path).fill(color)
                    }
                default:
                    EdgeLine(data: extra.path)
                        .stroke(color, style: StrokeStyle(lineWidth: width, lineCap: .round, lineJoin: .round))
                }
            }
            .contentShape(EdgeLine(data: extra.path).path(in: .zero).strokedPath(StrokeStyle(lineWidth: 12)))
            .onTapGesture { onPick?(extra) }
            .allowsHitTesting(onPick != nil)
        }
    }
}

/// The text of relations, summaries and boundaries, over the topics.
private struct ExtraLabels: View {
    let extras: [MindExtra]
    let palette: MapPalette
    var picked: UInt32?
    var onPick: ((MindExtra) -> Void)?

    var body: some View {
        ForEach(extras.filter { $0.width > 0 }, id: \.index) { extra in
            let isSummary = extra.kind == "summary"
            let isBoundary = extra.kind == "boundary"
            Text(verbatim: extra.text)
                .font(.system(size: isSummary ? 14 : 12))
                .multilineTextAlignment(.center)
                .foregroundStyle(isBoundary ? palette.centreText : palette.text)
                .padding(.horizontal, isSummary ? 10 : 6)
                .frame(width: extra.width, height: extra.height)
                .background {
                    let shape = RoundedRectangle(cornerRadius: isSummary ? 8 : 4)
                    if isBoundary {
                        shape.fill(palette.centre)
                    } else {
                        if let fill = palette.topic {
                            shape.fill(fill)
                        } else {
                            shape.fill(.background)
                        }
                        if isSummary { shape.strokeBorder(palette.line, lineWidth: 1.5) }
                    }
                }
                .overlay {
                    if extra.index == picked {
                        RoundedRectangle(cornerRadius: 6).inset(by: -3).strokeBorder(Color.accentColor, lineWidth: 2)
                    }
                }
                .contentShape(Rectangle())
                .onTapGesture { onPick?(extra) }
                .allowsHitTesting(onPick != nil)
                .position(x: extra.x + extra.width / 2, y: extra.y + extra.height / 2)
        }
    }
}

/// Whether two topics are under the same parent (the centre is under none).
private func siblings(_ a: String, _ b: String) -> Bool {
    guard let p = a.lastIndex(of: "."), let q = b.lastIndex(of: ".") else { return false }
    return a[..<p] == b[..<q]
}

/// Whether a topic is among the siblings from `first` to `last`.
private func inRun(_ id: String, _ first: String, _ last: String) -> Bool {
    guard siblings(id, first), siblings(first, last) else { return false }
    let index = { (path: String) in Int(path.split(separator: ".").last ?? "") ?? 0 }
    let (a, b) = (index(first), index(last))
    return (min(a, b)...max(a, b)).contains(index(id))
}

/// A topic's labels in a row under its text.
private struct TopicLabels: View {
    let labels: [String]

    var body: some View {
        HStack(spacing: 4) {
            ForEach(labels, id: \.self) { label in
                Text(verbatim: label)
                    .font(.system(size: 11))
                    .lineLimit(1)
                    .padding(.horizontal, 5)
                    .background(.primary.opacity(0.12), in: .rect(cornerRadius: 4))
            }
        }
    }
}

/// A topic's box and its text (or the field renaming it).
private struct TopicBox<Content: View>: View {
    let topic: MindTopic
    let palette: MapPalette
    @ViewBuilder let content: Content

    var body: some View {
        let isCentre = topic.depth == 0
        let isFilled = topic.depth == 1 && palette.filled
        let shape = RoundedRectangle(cornerRadius: isCentre ? 14 : 10)
        content
            .font(.system(size: topic.fontSize, weight: isCentre ? .semibold : .regular))
            .foregroundStyle(isCentre || isFilled ? palette.centreText : palette.text)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .frame(width: topic.width, height: topic.height)
            .background {
                if isCentre {
                    shape.fill(palette.centre)
                } else if isFilled {
                    shape.fill(palette.branch(topic.id))
                } else if topic.depth == 1 {
                    if let fill = palette.topic {
                        shape.fill(fill)
                    } else {
                        shape.fill(.background)
                    }
                    shape.strokeBorder(palette.branch(topic.id), lineWidth: 2)
                } else {
                    Rectangle().fill(palette.branch(topic.id)).frame(height: 2)
                        .frame(maxHeight: .infinity, alignment: .bottom)
                }
            }
    }
}

/// Whether a topic shows on a presentation's step: the centre's step
/// shows them all, a branch's step the centre and that branch.
private func onStep(_ id: String, _ step: String) -> Bool {
    step == "0" || id == "0" || id == step || id.hasPrefix(step + ".")
}

/// The whole map drawn on its paper, for saving as a picture or
/// presenting; with `shown`, the topics off that step fade.
private struct MindMapPicture: View {
    let layout: MindMapLayout
    var shown: String?

    var body: some View {
        let palette = MapPalette(layout.theme)
        let fade = { (id: String) in shown.map { onStep(id, $0) } ?? true ? 1.0 : 0.12 }
        ZStack(alignment: .topLeading) {
            palette.paper ?? Color.white
            ForEach(layout.topics.filter { !$0.edge.isEmpty }, id: \.id) { topic in
                EdgeLine(data: topic.edge)
                    .stroke(palette.branch(topic.id), style: StrokeStyle(lineWidth: topic.depth == 1 ? 3 : 2, lineCap: .round))
                    .opacity(fade(topic.id))
            }
            ExtraShapes(extras: layout.extras, palette: palette)
                .opacity(shown == nil || shown == "0" ? 1 : 0.3)
            ForEach(layout.topics, id: \.id) { topic in
                TopicBox(topic: topic, palette: palette) {
                    VStack(spacing: 6) {
                        HStack(spacing: 3) {
                            Text(verbatim: topic.text).multilineTextAlignment(.center)
                            if !topic.link.isEmpty {
                                Image(systemName: "arrow.up.right").imageScale(.small)
                            }
                        }
                        if !topic.labels.isEmpty {
                            TopicLabels(labels: topic.labels)
                        }
                    }
                }
                .position(x: topic.x + topic.width / 2, y: topic.y + topic.height / 2)
                .opacity(fade(topic.id))
            }
            ExtraLabels(extras: layout.extras, palette: palette)
                .opacity(shown == nil || shown == "0" ? 1 : 0.3)
        }
        .frame(width: layout.width, height: layout.height)
        .environment(\.colorScheme, .light)
    }
}

/// A map presented as XMind presents it: the whole map, then each main
/// branch in turn, zoomed to fill the view while the rest fades. A tap or
/// the arrow keys move on; Escape ends it.
private struct MindMapPresentation: View {
    let layout: MindMapLayout
    @Binding var step: Int?
    @FocusState private var focused: Bool

    private var steps: [String] { ["0"] + layout.topics.filter { $0.depth == 1 }.map(\.id) }

    var body: some View {
        let index = min(step ?? 0, steps.count - 1)
        let shown = steps[index]
        GeometryReader { proxy in
            let box = bounds(of: shown)
            let zoom = max(
                0.05, min(2, (proxy.size.width - 96) / max(1, box.width), (proxy.size.height - 96) / max(1, box.height)))
            MindMapPicture(layout: layout, shown: shown)
                .frame(width: layout.width, height: layout.height, alignment: .topLeading)
                .scaleEffect(zoom, anchor: .topLeading)
                .offset(
                    x: (proxy.size.width - box.width * zoom) / 2 - box.minX * zoom,
                    y: (proxy.size.height - box.height * zoom) / 2 - box.minY * zoom)
                .animation(.smooth(duration: 0.6), value: index)
        }
        .background(MapPalette(layout.theme).paper ?? Color.white)
        .clipped()
        .contentShape(Rectangle())
        .onTapGesture { move(1) }
        .overlay(alignment: .bottom) {
            HStack(spacing: 12) {
                Button("Previous", systemImage: "chevron.left") { move(-1) }
                    .disabled(index == 0)
                Text(verbatim: "\(index + 1) / \(steps.count)").monospacedDigit()
                Button("Next", systemImage: "chevron.right") { move(1) }
                Button("End Presentation", systemImage: "xmark") { step = nil }
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            .glassEffect(.regular, in: .capsule)
            .padding(.bottom, 20)
        }
        .focusable()
        .focused($focused)
        .focusEffectDisabled()
        .onKeyPress(phases: .down) { press in
            switch press.key {
            case .rightArrow, .downArrow, .space, .return: move(1)
            case .leftArrow, .upArrow: move(-1)
            case .escape: step = nil
            default: return .ignored
            }
            return .handled
        }
        .onAppear { focused = true }
    }

    /// The box a step shows: the whole map, or a branch and what is under it.
    private func bounds(of shown: String) -> CGRect {
        guard shown != "0" else { return CGRect(x: 0, y: 0, width: layout.width, height: layout.height) }
        let boxes = layout.topics.filter { $0.id != "0" && onStep($0.id, shown) }.map {
            CGRect(x: $0.x, y: $0.y, width: $0.width, height: $0.height)
        }
        return boxes.dropFirst().reduce(boxes.first ?? .zero) { $0.union($1) }
    }

    /// The next step, or the one before; past the last, the end.
    private func move(_ by: Int) {
        guard let current = step else { return }
        let next = current + by
        step = next >= steps.count ? nil : max(0, next)
    }
}

/// A map as a PNG picture at twice its size.
@MainActor
private func pngData(_ layout: MindMapLayout) -> Data? {
    let renderer = ImageRenderer(content: MindMapPicture(layout: layout))
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

/// A mind map: the open note's outline as topics around a centre. Tab
/// adds a topic under the selected one, Return one beside it, Delete
/// removes it, Space renames it, the arrow keys move between topics, and a
/// topic dragged onto another moves under it. Shift with a click or an
/// arrow key takes in a run of siblings for a summary or boundary; a
/// relation joins the selected topic to the next one tapped.
struct MindMapBoardView: View {
    @Environment(VaultModel.self) private var vault
    @Environment(\.openURL) private var openURL
    @State private var focus = "0"
    @State private var editing: String?
    @State private var draft = ""
    @State private var folded: [String] = []
    @State private var zoom: CGFloat = 1
    @State private var zoomStart: CGFloat?
    @State private var dragging: String?
    @State private var problem: String?
    @State private var exportFile: ExportFile?
    /// The last of a run of siblings that starts at the selected topic.
    @State private var runEnd: String?
    /// A relation waits for its other topic.
    @State private var relating: String?
    /// The relation, summary or boundary picked, by its index.
    @State private var extra: UInt32?
    @State private var extraDraft = ""
    /// The selected topic's link is being set: what is typed so far.
    @State private var linkDraft: String?
    /// The selected topic's labels are being set, separated by commas.
    @State private var labelDraft: String?
    /// The step of a presentation shown, if one is under way.
    @State private var presenting: Int?
    @FocusState private var keyboard: Bool
    @FocusState private var naming: Bool
    @State private var scroll = ScrollPosition()
    @State private var viewport = CGSize.zero
    @State private var offset = CGPoint.zero

    var body: some View {
        let layout = mindmapLayout(text: vault.text, title: vault.title, style: "", folded: folded)
        let byID = Dictionary(uniqueKeysWithValues: layout.topics.map { ($0.id, $0) })
        let palette = MapPalette(layout.theme)
        VStack(spacing: 0) {
            toolbar(layout: layout, byID: byID)
            if linkDraft != nil, let selected = byID[focus] {
                linkBar(selected)
            } else if labelDraft != nil, let selected = byID[focus] {
                labelBar(selected)
            } else if let picked = layout.extras.first(where: { $0.index == extra }) {
                extraBar(picked)
            } else if relating != nil {
                Text("Click the topic to relate it to. Esc cancels.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.bottom, 6)
            }
            Divider()
            ScrollView([.horizontal, .vertical]) {
                ZStack(alignment: .topLeading) {
                    Color.clear
                        .contentShape(Rectangle())
                        .onTapGesture {
                            commit()
                            keyboard = true
                            extra = nil
                            runEnd = nil
                            relating = nil
                        }
                    ForEach(layout.topics.filter { !$0.edge.isEmpty }, id: \.id) { topic in
                        EdgeLine(data: topic.edge)
                            .stroke(
                                palette.branch(topic.id),
                                style: StrokeStyle(lineWidth: topic.depth == 1 ? 3 : 2, lineCap: .round))
                    }
                    ExtraShapes(extras: layout.extras, palette: palette, picked: extra) { pick($0) }
                    ForEach(layout.topics, id: \.id) { topic in
                        topicView(topic, layout: layout, palette: palette)
                    }
                    ExtraLabels(extras: layout.extras, palette: palette, picked: extra) { pick($0) }
                }
                .frame(width: layout.width, height: layout.height, alignment: .topLeading)
                .scaleEffect(zoom, anchor: .topLeading)
                .frame(width: CGFloat(layout.width) * zoom, height: CGFloat(layout.height) * zoom, alignment: .topLeading)
                .coordinateSpace(.named("map"))
            }
            .scrollPosition($scroll)
            .onScrollGeometryChange(for: CGPoint.self) { $0.contentOffset } action: { _, now in offset = now }
            // A pane resized (the notes panel dragged narrower, say) shows
            // the topic in focus again, in the middle.
            .onGeometryChange(for: CGSize.self) { $0.size } action: { size in
                viewport = size
                reveal(byID[focus], centre: true)
            }
            .onChange(of: focus) { reveal(byID[focus], centre: false) }
            .onChange(of: layout.topics.count) { reveal(byID[focus], centre: false) }
            .background(palette.paper ?? Color.clear)
            .gesture(
                MagnifyGesture()
                    .onChanged { value in
                        let start = zoomStart ?? zoom
                        zoomStart = start
                        zoom = min(3, max(0.25, start * value.magnification))
                    }
                    .onEnded { _ in zoomStart = nil })
            .focusable()
            .focused($keyboard)
            .focusEffectDisabled()
            .onKeyPress(phases: .down) { press in handle(press, layout: layout, byID: byID) }
        }
        .overlay {
            if presenting != nil {
                MindMapPresentation(layout: layout, step: $presenting)
            }
        }
        .onChange(of: presenting) { _, now in
            if now == nil { keyboard = true }
        }
        .onChange(of: focus) {
            linkDraft = nil
            labelDraft = nil
        }
        .onChange(of: vault.version) {
            linkDraft = nil
            labelDraft = nil
            focus = "0"
            folded = []
            editing = nil
            runEnd = nil
            relating = nil
            extra = nil
        }
        .alert(
            "Mind Map", isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } })
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(problem ?? "")
        }
    }

    /// Scrolls so a topic shows: to the middle, or only as far as needed.
    private func reveal(_ topic: MindTopic?, centre: Bool) {
        guard let topic, viewport.width > 0 else { return }
        let box = CGRect(
            x: topic.x * zoom, y: topic.y * zoom, width: topic.width * zoom, height: topic.height * zoom)
        let visible = CGRect(origin: offset, size: viewport).insetBy(dx: 20, dy: 20)
        guard centre || !visible.contains(box) else { return }
        scroll.scrollTo(point: CGPoint(x: max(0, box.midX - viewport.width / 2), y: max(0, box.midY - viewport.height / 2)))
    }

    // MARK: Parts

    private func toolbar(layout: MindMapLayout, byID: [String: MindTopic]) -> some View {
        let selected = byID[focus]
        // Wraps in a narrow notes panel rather than losing its ends.
        return FlowLayout(spacing: 8, lineSpacing: 4) {
            Button("Add Subtopic", systemImage: "arrow.turn.down.right") {
                apply(.addChild(id: focus, text: String(localized: "Topic")), thenEdit: true)
            }
            Button("Add Topic", systemImage: "plus") {
                apply(.addSibling(id: focus, text: String(localized: "Topic")), thenEdit: true)
            }
            .disabled(focus == "0")
            Button("Delete", systemImage: "trash") { apply(.delete(id: focus)) }
                .disabled(focus == "0")
            let fold: LocalizedStringKey = selected?.folded == true ? "Unfold" : "Fold"
            Button(fold, systemImage: "chevron.right.2") { toggleFold(focus) }
                .disabled((selected?.children ?? 0) == 0)
            Button("Relation", systemImage: "arrow.turn.up.right") {
                extra = nil
                relating = relating == nil ? focus : nil
            }
            .help("Click the topic to relate it to. Esc cancels.")
            .background(relating == nil ? Color.clear : Color.accentColor.opacity(0.18), in: .rect(cornerRadius: 5))
            Button("Summary", systemImage: "curlybraces") {
                apply(.summarize(first: focus, last: runEnd ?? focus, text: String(localized: "Summary")))
            }
            .help("Shift-click a sibling to take in several topics.")
            .disabled(focus == "0")
            Button("Boundary", systemImage: "rectangle.dashed") {
                apply(.enclose(first: focus, last: runEnd ?? focus, text: ""))
            }
            .help("Shift-click a sibling to take in several topics.")
            .disabled(focus == "0")
            Button("Link", systemImage: "link") {
                commit()
                extra = nil
                labelDraft = nil
                linkDraft = linkDraft == nil ? (selected?.link ?? "") : nil
            }
            .keyboardShortcut("k", modifiers: .command)
            .help("Link the topic to a web page or a note (⌘K or Ctrl+K).")
            .background(linkDraft == nil ? Color.clear : Color.accentColor.opacity(0.18), in: .rect(cornerRadius: 5))
            Button("Labels", systemImage: "tag") {
                commit()
                extra = nil
                linkDraft = nil
                labelDraft = labelDraft == nil ? (selected?.labels ?? []).joined(separator: ", ") : nil
            }
            .help("Labels show under the topic; the note keeps them as tags.")
            .background(labelDraft == nil ? Color.clear : Color.accentColor.opacity(0.18), in: .rect(cornerRadius: 5))
            Menu {
                Button("Both Sides") { setStyle("logic") }
                Button("To the Right") { setStyle("right") }
                Button("Top Down") { setStyle("org") }
                Button("Tree") { setStyle("tree") }
                Button("Timeline") { setStyle("timeline") }
                Button("Fishbone") { setStyle("fishbone") }
            } label: {
                Label("Layout", systemImage: "rectangle.3.group")
            }
            .menuIndicator(.hidden)
            .fixedSize()
            Button("Present", systemImage: "play.rectangle") {
                commit()
                presenting = 0
            }
            Menu {
                ForEach(mindmapMarkers(), id: \.id) { marker in
                    Button {
                        apply(.toggleMarker(id: focus, marker: marker.id))
                    } label: {
                        Text(verbatim: marker.emoji)
                    }
                }
            } label: {
                Label("Marker", systemImage: "flag")
            }
            .menuIndicator(.hidden)
            .fixedSize()
            Menu {
                ForEach(mindmapThemes(), id: \.id) { theme in
                    Toggle(isOn: Binding(get: { theme.id == layout.theme.id }, set: { _ in setTheme(theme.id) })) {
                        Label {
                            Text(MapThemeName.title(theme.id))
                        } icon: {
                            colorDot(PlatformColor(hex: theme.centre) ?? .systemBlue)
                        }
                    }
                }
            } label: {
                Label("Theme", systemImage: "paintpalette")
            }
            .menuIndicator(.hidden)
            .fixedSize()
            Menu {
                Button { export("xmind") } label: { Text(verbatim: "XMind") }
                Button { export("opml") } label: { Text(verbatim: "OPML") }
                Button { export("mm") } label: { Text(verbatim: "FreeMind") }
                Button("Picture (SVG)") { export("svg") }
                Button("Picture (PNG)") { exportPicture(layout) }
                Button { exportPDF(layout) } label: { Text(verbatim: "PDF") }
            } label: {
                Label("Export…", systemImage: "square.and.arrow.up")
            }
            .menuIndicator(.hidden)
            .fixedSize()
            Button("Zoom Out", systemImage: "minus.magnifyingglass") { zoom = max(0.25, zoom / 1.25) }
            Text(verbatim: "\(Int((zoom * 100).rounded()))%").monospacedDigit().foregroundStyle(.secondary)
            Button("Zoom In", systemImage: "plus.magnifyingglass") { zoom = min(3, zoom * 1.25) }
        }
        .labelStyle(.iconOnly)
        .buttonStyle(.borderless)
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
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

    /// The selected topic's link, as it is typed: a web address or a
    /// note's name.
    private func linkBar(_ topic: MindTopic) -> some View {
        HStack {
            Text("Link").foregroundStyle(.secondary)
            TextField("A web address or a note's name", text: Binding(get: { linkDraft ?? "" }, set: { linkDraft = $0 }))
                .textFieldStyle(.roundedBorder)
                .frame(maxWidth: 320)
                .autocorrectionDisabled()
                .onSubmit { setLink(topic, to: linkDraft ?? "") }
                .onKeyPress(.escape) {
                    linkDraft = nil
                    keyboard = true
                    return .handled
                }
            Button("OK") { setLink(topic, to: linkDraft ?? "") }
            if !topic.link.isEmpty {
                Button("Remove Link", role: .destructive) { setLink(topic, to: "") }
            }
            Spacer()
        }
        .padding(.horizontal, 12)
        .padding(.bottom, 6)
    }

    /// The selected topic's labels, as they are typed.
    private func labelBar(_ topic: MindTopic) -> some View {
        HStack {
            Text("Labels").foregroundStyle(.secondary)
            TextField("Separated by commas", text: Binding(get: { labelDraft ?? "" }, set: { labelDraft = $0 }))
                .textFieldStyle(.roundedBorder)
                .frame(maxWidth: 320)
                .autocorrectionDisabled()
                .onSubmit { setLabels(topic, from: labelDraft ?? "") }
                .onKeyPress(.escape) {
                    labelDraft = nil
                    keyboard = true
                    return .handled
                }
            Button("OK") { setLabels(topic, from: labelDraft ?? "") }
            Spacer()
        }
        .padding(.horizontal, 12)
        .padding(.bottom, 6)
    }

    private func setLabels(_ topic: MindTopic, from typed: String) {
        labelDraft = nil
        keyboard = true
        let labels = typed.split(whereSeparator: { ",，;；".contains($0) })
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
        if labels != topic.labels { apply(.setLabels(id: topic.id, labels: labels)) }
    }

    private func setLink(_ topic: MindTopic, to link: String) {
        linkDraft = nil
        keyboard = true
        let link = link.trimmingCharacters(in: .whitespacesAndNewlines)
        if link != topic.link { apply(.setLink(id: topic.id, link: link)) }
    }

    /// Follows a topic's link: a web page in the browser, a note here.
    private func follow(_ topic: MindTopic) {
        if topic.webLink {
            if let url = URL(string: topic.link) { openURL(url) }
        } else {
            vault.openTarget(topic.link)
        }
    }

    /// The picked relation, summary or boundary: its text, and a way to
    /// remove it.
    private func extraBar(_ picked: MindExtra) -> some View {
        HStack {
            let kind =
                picked.kind == "relation" ? "Relation" : picked.kind == "summary" ? "Summary" : "Boundary"
            Text(LocalizedStringKey(kind)).foregroundStyle(.secondary)
            TextField("Text", text: $extraDraft)
                .textFieldStyle(.roundedBorder)
                .frame(maxWidth: 320)
                .onSubmit {
                    let text = extraDraft.trimmingCharacters(in: .whitespacesAndNewlines)
                    if text != picked.text { apply(.retextExtra(index: picked.index, text: text)) }
                }
            Button("Remove", role: .destructive) { apply(.removeExtra(index: picked.index)) }
            Spacer()
        }
        .padding(.horizontal, 12)
        .padding(.bottom, 6)
    }

    private func pick(_ picked: MindExtra) {
        commit()
        extra = picked.index
        extraDraft = picked.text
        relating = nil
        keyboard = true
    }

    /// A topic tapped: it completes a relation, joins the run (with Shift,
    /// on a Mac) or is selected.
    private func tapped(_ topic: MindTopic) {
        commit()
        keyboard = true
        extra = nil
        if let from = relating {
            relating = nil
            if from != topic.id { apply(.relate(from: from, to: topic.id, text: "")) }
            return
        }
        #if os(macOS)
        if NSEvent.modifierFlags.contains(.shift), siblings(topic.id, focus) {
            runEnd = topic.id
            return
        }
        #endif
        runEnd = nil
        focus = topic.id
    }

    /// Shift and an arrow along the siblings takes in the next one.
    private func extendRun(_ key: KeyEquivalent, layout: MindMapLayout) {
        let org = layout.style == "org"
        let back: KeyEquivalent = org ? .leftArrow : .upArrow
        let forth: KeyEquivalent = org ? .rightArrow : .downArrow
        let end = runEnd ?? focus
        guard key == back || key == forth, let dot = end.lastIndex(of: "."),
              let index = Int(end[end.index(after: dot)...])
        else { return }
        let next = "\(end[..<dot]).\(key == back ? index - 1 : index + 1)"
        if layout.topics.contains(where: { $0.id == next }) { runEnd = next }
    }

    /// Saves the map as another app's file, or as SVG.
    private func export(_ format: String) {
        do {
            exportFile = ExportFile(url: try vault.mindMapFile(format))
        } catch {
            problem = CoreKit.message(for: error)
        }
    }

    /// Saves the map as a one-page PDF, drawn as it is shown.
    private func exportPDF(_ layout: MindMapLayout) {
        let url = FileManager.default.temporaryDirectory.appending(path: "\(vault.title).pdf")
        do {
            guard let data = pdfData(MindMapPicture(layout: layout)) else { throw CocoaError(.fileWriteUnknown) }
            try data.write(to: url)
            exportFile = ExportFile(url: url)
        } catch {
            problem = error.localizedDescription
        }
    }

    /// Saves the map as it is shown as a PNG picture.
    private func exportPicture(_ layout: MindMapLayout) {
        let url = FileManager.default.temporaryDirectory.appending(path: "\(vault.title).png")
        do {
            guard let data = pngData(layout) else { throw CocoaError(.fileWriteUnknown) }
            try data.write(to: url)
            exportFile = ExportFile(url: url)
        } catch {
            problem = error.localizedDescription
        }
    }

    @ViewBuilder
    private func topicView(_ topic: MindTopic, layout: MindMapLayout, palette: MapPalette) -> some View {
        let isCentre = topic.depth == 0
        let shape = RoundedRectangle(cornerRadius: isCentre ? 14 : 10)
        let org = layout.style == "org"
        let foldAlignment: Alignment = topic.side < 0 ? .leading : org ? .bottom : .trailing
        TopicBox(topic: topic, palette: palette) {
            if editing == topic.id {
                TextField("Topic", text: $draft)
                    .textFieldStyle(.plain)
                    .multilineTextAlignment(.center)
                    .focused($naming)
                    .onSubmit { commit() }
                    .onKeyPress(.escape) {
                        editing = nil
                        keyboard = true
                        return .handled
                    }
                    .onChange(of: naming) { _, now in
                        if !now && editing == topic.id { commit() }
                    }
                    .onAppear { naming = true }
            } else {
                VStack(spacing: 6) {
                    HStack(spacing: 3) {
                        Text(verbatim: topic.text)
                            .multilineTextAlignment(.center)
                        if !topic.link.isEmpty {
                            Button {
                                follow(topic)
                            } label: {
                                Image(systemName: "arrow.up.right").imageScale(.small)
                            }
                            .buttonStyle(.plain)
                            .help(Text(verbatim: topic.link))
                            .accessibilityLabel("Open Link")
                        }
                    }
                    if !topic.labels.isEmpty {
                        TopicLabels(labels: topic.labels)
                    }
                }
            }
        }
        .overlay {
            if focus == topic.id {
                shape.inset(by: -4).strokeBorder(Color.accentColor, lineWidth: 2)
            } else if let runEnd, inRun(topic.id, focus, runEnd) {
                shape.inset(by: -4).strokeBorder(Color.accentColor, style: StrokeStyle(lineWidth: 2, dash: [5, 3]))
            }
        }
        .overlay(alignment: foldAlignment) {
            if topic.children > 0 && !isCentre {
                Button {
                    toggleFold(topic.id)
                } label: {
                    Text(verbatim: topic.folded ? "\(topic.children)" : "−")
                        .font(.caption2)
                        .frame(minWidth: 16, minHeight: 16)
                        .background(.background, in: .capsule)
                        .overlay(Capsule().strokeBorder(.secondary.opacity(0.5)))
                }
                .buttonStyle(.plain)
                .offset(x: org ? 0 : topic.side < 0 ? -10 : 10, y: org ? 10 : 0)
            }
        }
        .contentShape(shape)
        .help(Text(verbatim: topic.note))
        .gesture(
            TapGesture(count: 2).onEnded { startEditing(topic) }
                .exclusively(before: TapGesture().onEnded { tapped(topic) }),
            including: editing == topic.id ? .subviews : .all)
        .simultaneousGesture(
            DragGesture(minimumDistance: 6, coordinateSpace: .named("map"))
                .onChanged { _ in if topic.depth > 0 { dragging = topic.id } }
                .onEnded { value in drop(topic, at: value.location, layout: layout) },
            including: editing == topic.id ? .subviews : .all)
        .opacity(dragging == topic.id ? 0.5 : 1)
        .position(x: topic.x + topic.width / 2, y: topic.y + topic.height / 2)
    }

    // MARK: Changes

    private func apply(_ edit: MindMapEdit, thenEdit: Bool = false) {
        do {
            let change = try mindmapEdit(text: vault.text, title: vault.title, edit: edit)
            vault.text = change.text
            if !change.focus.isEmpty { focus = change.focus }
            runEnd = nil
            extra = nil
            if thenEdit {
                editing = change.focus
                draft = ""
            }
        } catch {
            problem = CoreKit.message(for: error)
        }
    }

    private func startEditing(_ topic: MindTopic) {
        commit()
        focus = topic.id
        editing = topic.id
        draft = topic.text
    }

    private func commit() {
        guard let id = editing else { return }
        editing = nil
        keyboard = true
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        apply(.rename(id: id, text: text.isEmpty ? String(localized: "Topic") : text))
    }

    private func toggleFold(_ id: String) {
        if let index = folded.firstIndex(of: id) {
            folded.remove(at: index)
        } else {
            folded.append(id)
            if focus.hasPrefix(id + ".") { focus = id }
        }
    }

    /// Moves a dragged topic under the topic it was dropped on.
    private func drop(_ topic: MindTopic, at location: CGPoint, layout: MindMapLayout) {
        dragging = nil
        let point = CGPoint(x: location.x / zoom, y: location.y / zoom)
        guard topic.depth > 0,
              let target = layout.topics.first(where: {
                  $0.id != topic.id && !$0.id.hasPrefix(topic.id + ".")
                      && CGRect(x: $0.x, y: $0.y, width: $0.width, height: $0.height).contains(point)
              })
        else { return }
        apply(.move(id: topic.id, parent: target.id, index: target.children))
    }

    /// Writes the theme into the note's `mindmap-theme` property, beside
    /// its `mindmap` one.
    private func setTheme(_ theme: String) {
        var lines = vault.text.components(separatedBy: "\n")
        let key = { (line: String) in line.lowercased().replacingOccurrences(of: " ", with: "") }
        if let index = lines.firstIndex(where: { key($0).hasPrefix("mindmap-theme:") }) {
            lines[index] = "mindmap-theme: \(theme)"
        } else if let index = lines.firstIndex(where: { key($0).hasPrefix("mindmap:") }) {
            lines.insert("mindmap-theme: \(theme)", at: index + 1)
        } else {
            return
        }
        vault.text = lines.joined(separator: "\n")
    }

    /// Writes the layout into the note's `mindmap` property.
    private func setStyle(_ style: String) {
        var lines = vault.text.components(separatedBy: "\n")
        guard let index = lines.firstIndex(where: {
            $0.lowercased().replacingOccurrences(of: " ", with: "").hasPrefix("mindmap:")
        }) else { return }
        lines[index] = "mindmap: \(style)"
        vault.text = lines.joined(separator: "\n")
    }

    /// The topic next to the selected one in a direction.
    private func neighbour(_ key: KeyEquivalent, layout: MindMapLayout, byID: [String: MindTopic]) -> MindTopic? {
        guard let selected = byID[focus] else { return nil }
        let org = layout.style == "org"
        let children = layout.topics.filter { $0.parent == selected.id }
        let siblings = layout.topics.filter { $0.parent == selected.parent && $0.side == selected.side }
        let index = siblings.firstIndex { $0.id == selected.id } ?? 0
        let inward: KeyEquivalent = org ? .upArrow : selected.side < 0 ? .rightArrow : .leftArrow
        let outward: KeyEquivalent = org ? .downArrow : selected.side < 0 ? .leftArrow : .rightArrow
        if key == inward { return selected.parent.flatMap { byID[$0] } }
        if key == outward {
            let side: Int32 = key == .leftArrow ? -1 : 1
            return selected.depth == 0 ? children.first { org || $0.side == side } : children.first
        }
        guard selected.depth > 0 else { return nil }
        let back: KeyEquivalent = org ? .leftArrow : .upArrow
        let next = key == back ? index - 1 : index + 1
        return siblings.indices.contains(next) ? siblings[next] : nil
    }

    private func handle(_ press: KeyPress, layout: MindMapLayout, byID: [String: MindTopic]) -> KeyPress.Result {
        guard editing == nil else { return .ignored }
        switch press.key {
        case .escape:
            guard relating != nil || runEnd != nil || extra != nil else { return .ignored }
            relating = nil
            runEnd = nil
            extra = nil
        case .tab:
            if byID[focus]?.folded == true { toggleFold(focus) }
            apply(.addChild(id: focus, text: String(localized: "Topic")), thenEdit: true)
        case .return:
            apply(
                focus == "0"
                    ? .addChild(id: "0", text: String(localized: "Topic"))
                    : .addSibling(id: focus, text: String(localized: "Topic")),
                thenEdit: true)
        case .delete, .deleteForward:
            if let picked = extra {
                apply(.removeExtra(index: picked))
                return .handled
            }
            guard focus != "0" else { return .ignored }
            apply(.delete(id: focus))
        case .space:
            guard let topic = byID[focus] else { return .ignored }
            startEditing(topic)
        case .upArrow, .downArrow, .leftArrow, .rightArrow:
            if press.modifiers.contains(.shift) {
                extendRun(press.key, layout: layout)
            } else if let next = neighbour(press.key, layout: layout, byID: byID) {
                focus = next.id
                runEnd = nil
            }
        default:
            return .ignored
        }
        return .handled
    }
}

/// Picks an XMind, OPML or FreeMind file and makes a mind map note of it.
private struct MindMapImport: ViewModifier {
    @Binding var isPresented: Bool
    let onImported: () -> Void
    @Environment(VaultModel.self) private var vault
    @State private var problem: String?

    func body(content: Content) -> some View {
        content
            .background {
                Color.clear.fileImporter(
                    isPresented: $isPresented,
                    allowedContentTypes: [ExportFile.xmind, ExportFile.opml, ExportFile.freeMind]
                ) { result in
                    do {
                        try vault.importMindMap(from: try result.get())
                        onImported()
                    } catch {
                        problem = CoreKit.message(for: error)
                    }
                }
            }
            .alert(
                "Mind Map", isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } })
            ) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(problem ?? "")
            }
    }
}

extension View {
    /// Asks for an XMind, OPML or FreeMind file while `isPresented`, and
    /// opens the mind map note made of it.
    func mindMapImport(isPresented: Binding<Bool>, onImported: @escaping () -> Void = {}) -> some View {
        modifier(MindMapImport(isPresented: isPresented, onImported: onImported))
    }
}

/// Display names of the map themes.
enum MapThemeName {
    static func title(_ id: String) -> LocalizedStringKey {
        switch id {
        case "ocean": "Ocean"
        case "forest": "Forest"
        case "sunset": "Sunset"
        case "mono": "Monochrome"
        case "dark": "Dark"
        default: "Classic"
        }
    }
}
