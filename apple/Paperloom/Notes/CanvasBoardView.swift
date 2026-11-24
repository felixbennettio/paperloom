import CoreKit
import SwiftUI

/// Obsidian's six card colours, by number.
private let cardColors: [String: Color] = ["1": .red, "2": .orange, "3": .yellow, "4": .green, "5": .cyan, "6": .purple]

/// A card's colour: one of the six by number, or `#rrggbb`.
private func cardColor(_ value: String?) -> Color? {
    guard let value else { return nil }
    if let named = cardColors[value] { return named }
    guard value.hasPrefix("#"), value.count == 7, let rgb = Int(value.dropFirst(), radix: 16) else { return nil }
    return Color(
        red: Double((rgb >> 16) & 0xff) / 255, green: Double((rgb >> 8) & 0xff) / 255, blue: Double(rgb & 0xff) / 255)
}

/// 16 hex digits, as Obsidian makes card and arrow IDs.
private func newID() -> String {
    (0..<8).map { _ in String(format: "%02x", UInt8.random(in: 0...255)) }.joined()
}

private enum CanvasSide: String {
    case top, right, bottom, left

    var normal: CGVector {
        switch self {
        case .top: CGVector(dx: 0, dy: -1)
        case .right: CGVector(dx: 1, dy: 0)
        case .bottom: CGVector(dx: 0, dy: 1)
        case .left: CGVector(dx: -1, dy: 0)
        }
    }
}

extension CanvasCard {
    fileprivate var rect: CGRect {
        CGRect(x: CGFloat(x), y: CGFloat(y), width: CGFloat(width), height: CGFloat(height))
    }

    fileprivate func anchor(_ side: CanvasSide) -> CGPoint {
        let r = rect
        switch side {
        case .top: return CGPoint(x: r.midX, y: r.minY)
        case .right: return CGPoint(x: r.maxX, y: r.midY)
        case .bottom: return CGPoint(x: r.midX, y: r.maxY)
        case .left: return CGPoint(x: r.minX, y: r.midY)
        }
    }

    /// The side facing a point.
    fileprivate func facing(_ point: CGPoint) -> CanvasSide {
        let r = rect
        let dx = point.x - r.midX
        let dy = point.y - r.midY
        if abs(dx) * r.height >= abs(dy) * r.width { return dx >= 0 ? .right : .left }
        return dy >= 0 ? .bottom : .top
    }
}

/// An arrow's curve, in the board's coordinates.
private struct ArrowCurve: Shape {
    let points: [CGPoint]

    func path(in rect: CGRect) -> Path {
        var path = Path()
        guard points.count == 4 else { return path }
        path.move(to: points[0])
        path.addCurve(to: points[3], control1: points[1], control2: points[2])
        return path
    }
}

/// A triangle at `tip`, pointing away from `from`.
private struct ArrowHead: Shape {
    let from: CGPoint
    let tip: CGPoint

    func path(in rect: CGRect) -> Path {
        let angle = atan2(tip.y - from.y, tip.x - from.x)
        let corner = { (turn: CGFloat) in
            CGPoint(x: tip.x - 10 * cos(angle + turn), y: tip.y - 10 * sin(angle + turn))
        }
        var path = Path()
        path.move(to: tip)
        path.addLine(to: corner(0.45))
        path.addLine(to: corner(-0.45))
        path.closeSubpath()
        return path
    }
}

/// A board of cards (text, notes, web pages, groups) joined by arrows,
/// kept as a JSON Canvas file in the vault. Drag the background to pan,
/// pinch to zoom, double-click (double-tap) to add a card; select a card
/// and choose Connect, then the card to join it to.
struct CanvasBoardView: View {
    let path: String
    @Environment(VaultModel.self) private var vault
    @Environment(TabsModel.self) private var tabs
    @Environment(\.openURL) private var openURL
    @State private var board = CanvasBoard(cards: [], arrows: [])
    @State private var loaded = false
    @State private var offset = CGSize.zero
    @State private var zoom: CGFloat = 1
    @State private var panStart: CGSize?
    @State private var zoomStart: CGFloat?
    @State private var selected: String?
    @State private var editing: String?
    @FocusState private var writing: Bool
    @State private var connecting = false
    /// Where the cards being dragged started.
    @State private var moveStart: [String: CGPoint]?
    @State private var resizeStart: CGSize?
    @State private var previews: [String: String] = [:]
    @State private var choosingNote = false
    @State private var addingLink = false
    @State private var linkDraft = ""
    @State private var saveTask: Task<Void, Never>?
    @State private var size = CGSize.zero

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .topLeading) {
                Color.clear
                    .contentShape(Rectangle())
                    .gesture(pan)
                    .gesture(
                        SpatialTapGesture(count: 2)
                            .onEnded { value in addCard(kind: "text", at: world(value.location), size: CGSize(width: 260, height: 140)) }
                            .exclusively(before: TapGesture().onEnded {
                                selected = nil
                                editing = nil
                                connecting = false
                            }))
                ZStack(alignment: .topLeading) {
                    ForEach(board.arrows, id: \.id) { arrow in
                        arrowView(arrow)
                    }
                    ForEach(groupsFirst, id: \.id) { card in
                        cardView(card)
                    }
                }
                .scaleEffect(zoom, anchor: .topLeading)
                .offset(offset)
                if loaded && board.cards.isEmpty {
                    Text("Double-click anywhere to add a card.")
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .allowsHitTesting(false)
                }
            }
            .coordinateSpace(.named("canvas"))
            .clipped()
            .simultaneousGesture(magnify)
            .onAppear {
                size = geometry.size
                load()
            }
            .onChange(of: geometry.size) { _, new in size = new }
        }
        .background(.background)
        .toolbar { toolbar }
        .sheet(isPresented: $choosingNote) {
            NoteSwitcher(allowCreate: false) { note in
                addCard(kind: "file", at: middle, size: CGSize(width: 400, height: 300)) { $0.file = note.path }
            } onCreate: { _ in }
        }
        .alert("Add Web Page…", isPresented: $addingLink) {
            TextField(text: $linkDraft) { Text(verbatim: "https://") }
            Button("Add") {
                let url = linkDraft.trimmingCharacters(in: .whitespaces)
                if url.hasPrefix("http://") || url.hasPrefix("https://") {
                    addCard(kind: "link", at: middle, size: CGSize(width: 400, height: 120)) { $0.url = url }
                }
            }
            Button("Cancel", role: .cancel) {}
        }
        #if os(macOS)
        .onDeleteCommand { deleteSelected() }
        #endif
        .onDisappear { saveNow() }
    }

    // MARK: Drawing

    private var groupsFirst: [CanvasCard] {
        board.cards.filter { $0.kind == "group" } + board.cards.filter { $0.kind != "group" }
    }

    private func curve(_ arrow: CanvasArrow) -> [CGPoint]? {
        guard let from = board.cards.first(where: { $0.id == arrow.fromNode }),
              let to = board.cards.first(where: { $0.id == arrow.toNode })
        else { return nil }
        let fromSide = arrow.fromSide.flatMap(CanvasSide.init(rawValue:)) ?? from.facing(CGPoint(x: to.rect.midX, y: to.rect.midY))
        let toSide = arrow.toSide.flatMap(CanvasSide.init(rawValue:)) ?? to.facing(CGPoint(x: from.rect.midX, y: from.rect.midY))
        let start = from.anchor(fromSide)
        let end = to.anchor(toSide)
        let reach = max(40, hypot(end.x - start.x, end.y - start.y) / 3)
        return [
            start,
            CGPoint(x: start.x + fromSide.normal.dx * reach, y: start.y + fromSide.normal.dy * reach),
            CGPoint(x: end.x + toSide.normal.dx * reach, y: end.y + toSide.normal.dy * reach),
            end,
        ]
    }

    @ViewBuilder
    private func arrowView(_ arrow: CanvasArrow) -> some View {
        if let points = curve(arrow) {
            let color = cardColor(arrow.color) ?? .secondary
            let width: CGFloat = arrow.id == selected ? 3.5 : 2
            ZStack(alignment: .topLeading) {
                ArrowCurve(points: points)
                    .stroke(color, lineWidth: width)
                    .contentShape(ArrowCurve(points: points).stroke(lineWidth: 16))
                    .onTapGesture { selected = arrow.id }
                if arrow.toEnd != "none" {
                    ArrowHead(from: points[2], tip: points[3]).fill(color)
                }
                if arrow.fromEnd == "arrow" {
                    ArrowHead(from: points[1], tip: points[0]).fill(color)
                }
                if let label = arrow.label, !label.isEmpty {
                    let x = (points[0].x + 3 * points[1].x + 3 * points[2].x + points[3].x) / 8
                    let y = (points[0].y + 3 * points[1].y + 3 * points[2].y + points[3].y) / 8
                    Text(verbatim: label)
                        .font(.caption)
                        .padding(.horizontal, 4)
                        .background(.background, in: .rect(cornerRadius: 4))
                        .position(x: x, y: y)
                }
            }
        }
    }

    private func cardView(_ card: CanvasCard) -> some View {
        let color = cardColor(card.color)
        let isGroup = card.kind == "group"
        let shape = RoundedRectangle(cornerRadius: 10)
        return VStack(alignment: .leading, spacing: 6) {
            cardContent(card)
        }
        .padding(10)
        .frame(width: CGFloat(card.width), height: CGFloat(card.height), alignment: .topLeading)
        .background {
            if isGroup {
                shape.fill((color ?? .secondary).opacity(0.06))
            } else {
                shape.fill(.background)
                shape.fill((color ?? .clear).opacity(0.12))
            }
        }
        .overlay {
            shape.strokeBorder(
                card.id == selected ? Color.accentColor : (color ?? Color.secondary.opacity(0.4)),
                style: StrokeStyle(lineWidth: card.id == selected ? 2.5 : 1.5, dash: isGroup ? [6, 4] : []))
        }
        .overlay(alignment: .topLeading) {
            if isGroup {
                groupLabel(card).offset(y: -24)
            }
        }
        .overlay(alignment: .bottomTrailing) {
            if card.id == selected {
                RoundedRectangle(cornerRadius: 3)
                    .fill(Color.accentColor)
                    .frame(width: 14, height: 14)
                    .offset(x: 7, y: 7)
                    .highPriorityGesture(resize(card))
            }
        }
        .gesture(move(card))
        .gesture(
            TapGesture(count: 2).onEnded { open(card) }
                .exclusively(before: TapGesture().onEnded { tap(card) }))
        .task(id: card.file) {
            if let file = card.file, file.lowercased().hasSuffix(".md"), previews[file] == nil {
                previews[file] = String((vault.noteText(file) ?? "").prefix(600))
            }
        }
        .position(x: card.rect.midX, y: card.rect.midY)
    }

    @ViewBuilder
    private func cardContent(_ card: CanvasCard) -> some View {
        switch card.kind {
        case "text":
            if editing == card.id {
                TextEditor(text: textBinding(card))
                    .scrollContentBackground(.hidden)
                    .font(.callout)
                    // A new or reopened card takes the typing at once.
                    .focused($writing)
                    .onAppear { writing = true }
            } else if let text = card.text, !text.isEmpty {
                Text(verbatim: text).font(.callout)
            } else {
                Text("Double-click to write").font(.callout).foregroundStyle(.secondary)
            }
        case "file":
            let file = card.file ?? ""
            Text(verbatim: ((file as NSString).lastPathComponent as NSString).deletingPathExtension)
                .font(.headline)
            Text(verbatim: previews[file] ?? "")
                .font(.caption)
                .foregroundStyle(.secondary)
        case "link":
            let url = card.url ?? ""
            Text(verbatim: URL(string: url)?.host() ?? url).font(.headline)
            Text(verbatim: url).font(.caption).foregroundStyle(.secondary)
        default:
            EmptyView()
        }
    }

    @ViewBuilder
    private func groupLabel(_ card: CanvasCard) -> some View {
        if editing == card.id {
            TextField("Group", text: labelBinding(card))
                .textFieldStyle(.roundedBorder)
                .frame(width: 200)
                .onSubmit { editing = nil }
        } else {
            Text(verbatim: card.label ?? "")
                .font(.headline)
                .foregroundStyle(.secondary)
        }
    }

    // MARK: Gestures

    private var pan: some Gesture {
        DragGesture(minimumDistance: 4)
            .onChanged { value in
                let start = panStart ?? offset
                panStart = start
                offset = CGSize(width: start.width + value.translation.width, height: start.height + value.translation.height)
            }
            .onEnded { _ in panStart = nil }
    }

    private var magnify: some Gesture {
        MagnifyGesture()
            .onChanged { value in
                let start = zoomStart ?? zoom
                zoomStart = start
                zoom = min(3, max(0.2, start * value.magnification))
            }
            .onEnded { _ in zoomStart = nil }
    }

    private func move(_ card: CanvasCard) -> some Gesture {
        DragGesture(minimumDistance: 2, coordinateSpace: .named("canvas"))
            .onChanged { value in
                guard editing != card.id else { return }
                if moveStart == nil {
                    selected = card.id
                    let ids = moving(card)
                    moveStart = Dictionary(
                        uniqueKeysWithValues: board.cards.filter { ids.contains($0.id) }.map {
                            ($0.id, CGPoint(x: CGFloat($0.x), y: CGFloat($0.y)))
                        })
                }
                guard let starts = moveStart else { return }
                let dx = value.translation.width / zoom
                let dy = value.translation.height / zoom
                for index in board.cards.indices {
                    if let start = starts[board.cards[index].id] {
                        board.cards[index].x = Int64((start.x + dx).rounded())
                        board.cards[index].y = Int64((start.y + dy).rounded())
                    }
                }
            }
            .onEnded { _ in
                if moveStart != nil {
                    moveStart = nil
                    scheduleSave()
                }
            }
    }

    private func resize(_ card: CanvasCard) -> some Gesture {
        DragGesture(minimumDistance: 1, coordinateSpace: .named("canvas"))
            .onChanged { value in
                let start = resizeStart ?? CGSize(width: CGFloat(card.width), height: CGFloat(card.height))
                resizeStart = start
                update(card.id) {
                    $0.width = Int64(max(80, start.width + value.translation.width / zoom).rounded())
                    $0.height = Int64(max(40, start.height + value.translation.height / zoom).rounded())
                }
            }
            .onEnded { _ in
                resizeStart = nil
                scheduleSave()
            }
    }

    /// The cards that move with a card: a group takes what lies inside it.
    private func moving(_ card: CanvasCard) -> Set<String> {
        guard card.kind == "group" else { return [card.id] }
        let outer = card.rect
        return Set(board.cards.filter { $0.id == card.id || outer.contains($0.rect) }.map(\.id))
    }

    private func tap(_ card: CanvasCard) {
        if connecting, let from = selected, from != card.id,
           let source = board.cards.first(where: { $0.id == from })
        {
            let arrow = CanvasArrow(
                id: newID(), fromNode: from,
                fromSide: source.facing(CGPoint(x: card.rect.midX, y: card.rect.midY)).rawValue, fromEnd: nil,
                toNode: card.id, toSide: card.facing(CGPoint(x: source.rect.midX, y: source.rect.midY)).rawValue,
                toEnd: nil, color: nil, label: nil)
            board.arrows.append(arrow)
            connecting = false
            selected = arrow.id
            scheduleSave()
            return
        }
        selected = card.id
        if editing != card.id { editing = nil }
    }

    private func open(_ card: CanvasCard) {
        switch card.kind {
        case "text", "group":
            selected = card.id
            editing = card.id
        case "file":
            if let file = card.file, file.lowercased().hasSuffix(".md") {
                vault.open(file)
                tabs.open(.notes)
            }
        case "link":
            if let url = card.url.flatMap(URL.init(string:)) { openURL(url) }
        default:
            break
        }
    }

    // MARK: Changes

    private func world(_ point: CGPoint) -> CGPoint {
        CGPoint(x: (point.x - offset.width) / zoom, y: (point.y - offset.height) / zoom)
    }

    private var middle: CGPoint {
        world(CGPoint(x: size.width / 2, y: size.height / 2))
    }

    private func update(_ id: String, _ change: (inout CanvasCard) -> Void) {
        guard let index = board.cards.firstIndex(where: { $0.id == id }) else { return }
        change(&board.cards[index])
    }

    private func textBinding(_ card: CanvasCard) -> Binding<String> {
        Binding(
            get: { board.cards.first { $0.id == card.id }?.text ?? "" },
            set: { text in
                update(card.id) { $0.text = text }
                scheduleSave()
            })
    }

    private func labelBinding(_ card: CanvasCard) -> Binding<String> {
        Binding(
            get: { board.cards.first { $0.id == card.id }?.label ?? "" },
            set: { label in
                update(card.id) { $0.label = label }
                scheduleSave()
            })
    }

    private func addCard(kind: String, at point: CGPoint, size: CGSize, fill: (inout CanvasCard) -> Void = { _ in }) {
        var card = CanvasCard(
            id: newID(), kind: kind,
            x: Int64((point.x - size.width / 2).rounded()), y: Int64((point.y - size.height / 2).rounded()),
            width: Int64(size.width), height: Int64(size.height),
            color: nil, text: kind == "text" ? "" : nil, file: nil, subpath: nil, url: nil,
            label: kind == "group" ? String(localized: "Group") : nil)
        fill(&card)
        board.cards.append(card)
        selected = card.id
        editing = kind == "text" ? card.id : nil
        scheduleSave()
    }

    private func deleteSelected() {
        guard let id = selected else { return }
        board.cards.removeAll { $0.id == id }
        board.arrows.removeAll { $0.id == id || $0.fromNode == id || $0.toNode == id }
        selected = nil
        editing = nil
        scheduleSave()
    }

    private func recolor(_ color: String?) {
        guard let id = selected else { return }
        if let index = board.cards.firstIndex(where: { $0.id == id }) {
            board.cards[index].color = color
        } else if let index = board.arrows.firstIndex(where: { $0.id == id }) {
            board.arrows[index].color = color
        }
        scheduleSave()
    }

    private func load() {
        guard !loaded else { return }
        board = vault.readCanvas(path) ?? CanvasBoard(cards: [], arrows: [])
        loaded = true
        guard !board.cards.isEmpty, size.width > 0, size.height > 0 else { return }
        let area = board.cards.map(\.rect).reduce(board.cards[0].rect) { $0.union($1) }
        let fit = min(size.width / (area.width + 80), size.height / (area.height + 80))
        zoom = min(1, max(0.2, fit))
        offset = CGSize(width: size.width / 2 - area.midX * zoom, height: size.height / 2 - area.midY * zoom)
    }

    private func scheduleSave() {
        saveTask?.cancel()
        saveTask = Task {
            try? await Task.sleep(for: .milliseconds(400))
            guard !Task.isCancelled else { return }
            saveNow()
        }
    }

    private func saveNow() {
        saveTask?.cancel()
        saveTask = nil
        guard loaded else { return }
        vault.writeCanvas(path, board)
    }

    // MARK: Toolbar

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItemGroup {
            Button("Add Card", systemImage: "rectangle.badge.plus") {
                addCard(kind: "text", at: middle, size: CGSize(width: 260, height: 140))
            }
            Button("Add Note…", systemImage: "doc.text") { choosingNote = true }
            Button("Add Web Page…", systemImage: "link") {
                linkDraft = ""
                addingLink = true
            }
            Button("Add Group", systemImage: "rectangle.dashed") {
                addCard(kind: "group", at: middle, size: CGSize(width: 600, height: 400))
            }
            Button("Connect", systemImage: "arrow.right") { connecting.toggle() }
                .disabled(selected == nil || !board.cards.contains { $0.id == selected })
            Menu("Colour", systemImage: "paintpalette") {
                Button("No Colour") { recolor(nil) }
                ForEach(["1", "2", "3", "4", "5", "6"], id: \.self) { key in
                    Button {
                        recolor(key)
                    } label: {
                        Label {
                            Text(verbatim: key)
                        } icon: {
                            colorDot(PlatformColor(cardColors[key] ?? .gray))
                        }
                    }
                }
            }
            .disabled(selected == nil)
            Button("Delete", systemImage: "trash", role: .destructive) { deleteSelected() }
                .disabled(selected == nil)
            Button("Zoom Out", systemImage: "minus.magnifyingglass") { zoom = max(0.2, zoom / 1.25) }
            Button("Zoom In", systemImage: "plus.magnifyingglass") { zoom = min(3, zoom * 1.25) }
        }
    }
}
