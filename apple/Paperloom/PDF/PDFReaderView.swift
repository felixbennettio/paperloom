import CoreKit
import CoreText
import PDFKit
import SwiftUI
import UniformTypeIdentifiers

#if os(macOS)
typealias PlatformColor = NSColor
typealias PlatformBezierPath = NSBezierPath
typealias PlatformFont = NSFont

extension NSBezierPath {
    fileprivate func addLine(to point: CGPoint) { line(to: point) }
}
#else
typealias PlatformColor = UIColor
typealias PlatformBezierPath = UIBezierPath
typealias PlatformFont = UIFont
#endif

extension PlatformColor {
    /// `#rrggbb`.
    convenience init?(hex: String) {
        guard hex.count == 7, hex.hasPrefix("#"), let value = UInt32(hex.dropFirst(), radix: 16) else { return nil }
        self.init(
            red: CGFloat((value >> 16) & 0xff) / 255, green: CGFloat((value >> 8) & 0xff) / 255,
            blue: CGFloat(value & 0xff) / 255, alpha: 1)
    }
}

/// Colors for marking up text; the same five on every platform.
enum MarkColor: String, CaseIterable, Identifiable {
    case yellow, red, green, blue, purple

    var id: String { rawValue }

    var title: LocalizedStringKey {
        switch self {
        case .yellow: "Yellow"
        case .red: "Red"
        case .green: "Green"
        case .blue: "Blue"
        case .purple: "Purple"
        }
    }

    var hex: String {
        switch self {
        case .yellow: "#ffd400"
        case .red: "#ff6666"
        case .green: "#5fb236"
        case .blue: "#2ea8e5"
        case .purple: "#a28ae5"
        }
    }

    var color: Color {
        let platform = PlatformColor(hex: hex) ?? .systemYellow
        #if os(macOS)
        return Color(nsColor: platform)
        #else
        return Color(uiColor: platform)
        #endif
    }
}

/// What the pointer does on the pages: select text, or draw a kind of mark.
/// The raw values are the core's annotation kinds.
enum PDFTool: String, CaseIterable, Identifiable {
    case select, ink, text, square, circle, arrow, stamp, sign

    var id: String { rawValue }

    var title: LocalizedStringKey {
        switch self {
        case .select: "Select Text"
        case .ink: "Draw"
        case .text: "Text Box"
        case .square: "Rectangle"
        case .circle: "Ellipse"
        case .arrow: "Arrow"
        case .stamp: "Stamp"
        case .sign: "Sign"
        }
    }

    var symbol: String {
        switch self {
        case .select: "character.cursor.ibeam"
        case .ink: "pencil.tip"
        case .text: "textformat"
        case .square: "rectangle"
        case .circle: "circle"
        case .arrow: "arrow.up.right"
        case .stamp: "seal"
        case .sign: "signature"
        }
    }
}

/// The words stamps take, as Acrobat's standard stamps.
let stampWords = ["Approved", "Not Approved", "Draft", "Final", "Confidential", "For Comment", "Reviewed"]

/// A stamp drawn by Paperloom: its word in capitals, framed, in its colour.
final class StampAnnotation: PDFAnnotation {
    private let word: String
    private let tint: PlatformColor

    init(bounds: CGRect, word: String, tint: PlatformColor) {
        self.word = word.uppercased()
        self.tint = tint
        super.init(bounds: bounds, forType: .stamp, withProperties: nil)
    }

    required init?(coder: NSCoder) {
        return nil
    }

    override func draw(with box: PDFDisplayBox, in context: CGContext) {
        context.saveGState()
        let frame = bounds.insetBy(dx: 1, dy: 1)
        context.setStrokeColor(tint.cgColor)
        context.setLineWidth(2)
        context.addPath(CGPath(roundedRect: frame, cornerWidth: 4, cornerHeight: 4, transform: nil))
        context.strokePath()
        let size = max(4, min(bounds.height * 0.55, (bounds.width - 12) / (0.72 * CGFloat(max(1, word.count)))))
        let text = NSAttributedString(
            string: word,
            attributes: [
                .font: PlatformFont.boldSystemFont(ofSize: size),
                NSAttributedString.Key(kCTForegroundColorAttributeName as String): tint.cgColor,
            ])
        let line = CTLineCreateWithAttributedString(text)
        let width = CGFloat(CTLineGetTypographicBounds(line, nil, nil, nil))
        context.textMatrix = .identity
        context.textPosition = CGPoint(x: bounds.midX - width / 2, y: bounds.midY - size * 0.35)
        CTLineDraw(line, context)
        context.restoreGState()
    }
}

/// A handwritten signature kept on this device: its strokes in the points
/// of the pad it was written on (y down), cropped to them.
struct SavedSignature: Codable {
    var width: Double
    var height: Double
    var strokes: [[[Double]]]

    private static let key = "pdfSignature"

    static func load() -> SavedSignature? {
        guard let data = UserDefaults.standard.data(forKey: key) else { return nil }
        return try? JSONDecoder().decode(SavedSignature.self, from: data)
    }

    func save() {
        if let data = try? JSONEncoder().encode(self) { UserDefaults.standard.set(data, forKey: Self.key) }
    }

    /// The strokes on a PDF page, `width` points wide and centred on a
    /// point (PDF y grows upwards).
    func placed(x: Double, y: Double, width placedWidth: Double) -> [[AnnotationPoint]] {
        let scale = placedWidth / width
        let placedHeight = height * scale
        return strokes.map { stroke in
            stroke.compactMap { p in
                guard p.count == 2 else { return nil }
                return AnnotationPoint(x: x - placedWidth / 2 + p[0] * scale, y: y + placedHeight / 2 - p[1] * scale)
            }
        }
    }
}

/// An open PDF: the PDFKit view, finding text, and the marks on it. Marks
/// are kept in the library (in PDF page coordinates), not in the file, so
/// every device and app shows the same ones.
@MainActor
@Observable
final class PDFReaderModel {
    let url: URL
    let attachmentID: String
    let document: PDFDocument?
    /// The colour for marks, kept for the next reading session.
    var markColor = MarkColor(rawValue: UserDefaults.standard.string(forKey: "pdf.markColor") ?? "") ?? .yellow {
        didSet { UserDefaults.standard.set(markColor.rawValue, forKey: "pdf.markColor") }
    }
    var searchText = ""
    private(set) var pageLabel = ""
    private(set) var selectionRevision = 0
    /// The stored marks, for the annotation list.
    private(set) var marks: [AnnotationInfo] = []
    /// The PDF's table of contents, flattened.
    let outline: [OutlineEntry]
    /// Two pages side by side.
    var twoUp = false {
        didSet {
            view?.displayMode = twoUp ? .twoUpContinuous : .singlePageContinuous
            view?.autoScales = true
        }
    }
    /// Light text on dark pages.
    var night = false
    /// Selecting text, or drawing marks with the pen or a shape.
    var tool = PDFTool.select
    /// Where a text box is being placed: its page (from 0) and top-left
    /// corner on it.
    var placingText: (page: Int, corner: CGPoint)?
    /// What a stamp says.
    var stampWord = stampWords[0]
    /// The signature kept on this device, if one was written.
    var signature = SavedSignature.load()
    /// The Sign tool waits for a signature to be written.
    var askingSignature = false
    /// The PDF's form, as the core reads it; PDFKit lets its boxes be
    /// filled in, and Save Form writes them into the file.
    var formFields: [PdfFormField] = []
    @ObservationIgnored private var library: LibraryModel?
    @ObservationIgnored private weak var view: PDFView?
    @ObservationIgnored private var pageObserver: NSObjectProtocol?
    @ObservationIgnored private var selectionObserver: NSObjectProtocol?
    /// The PDFKit annotations drawn for each stored mark, by its ID.
    @ObservationIgnored private var drawn: [String: [PDFAnnotation]] = [:]
    @ObservationIgnored private var matches: [PDFSelection] = []
    @ObservationIgnored private var matchIndex = 0
    @ObservationIgnored private var matchedText = ""

    /// A page to show once the view is ready (from 0).
    @ObservationIgnored var startPage: Int?

    init(url: URL, attachmentID: String, page: Int? = nil) {
        self.url = url
        self.attachmentID = attachmentID
        startPage = page
        let document = PDFDocument(url: url)
        self.document = document
        outline = document.map(OutlineEntry.read) ?? []
    }

    func makeView() -> PDFView {
        let view = PDFView()
        view.document = document
        view.autoScales = true
        view.displayMode = .singlePageContinuous
        view.displayDirection = .vertical
        self.view = view
        pageObserver = NotificationCenter.default.addObserver(
            forName: .PDFViewPageChanged, object: view, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.updatePageLabel() }
        }
        selectionObserver = NotificationCenter.default.addObserver(
            forName: .PDFViewSelectionChanged, object: view, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.selectionRevision += 1 }
        }
        updatePageLabel()
        return view
    }

    /// Draws the marks stored for this PDF. Once.
    func load(from library: LibraryModel) {
        guard self.library == nil else { return }
        self.library = library
        guard let document else { return }
        marks = library.annotations(attachmentID: attachmentID)
        for mark in marks {
            draw(mark, in: document)
        }
        formFields = (try? library.core?.pdfFormFields(attachmentId: attachmentID)) ?? []
        library.indexPDFIfNeeded(attachmentID: attachmentID, url: url)
        if let startPage, let page = document.page(at: startPage) {
            view?.go(to: page)
        }
    }

    /// Reconcile committed library edits without reopening the PDF or changing drafts.
    func reloadMarks() {
        guard let library, let document else { return }
        let saved = library.annotations(attachmentID: attachmentID)
        for id in Array(drawn.keys) { undraw(id) }
        marks = saved
        for mark in marks { draw(mark, in: document) }
    }

    /// The page in view, from 0.
    var currentPageIndex: Int {
        guard let document, let page = view?.currentPage else { return 0 }
        return document.index(for: page)
    }

    var pageCount: Int { document?.pageCount ?? 0 }

    private func updatePageLabel() {
        guard let document, let page = view?.currentPage else { return }
        let index = document.index(for: page) + 1
        pageLabel = "\(index) / \(document.pageCount)"
    }

    private func subtype(for kind: String) -> PDFAnnotationSubtype? {
        switch kind {
        case "highlight": .highlight
        case "underline": .underline
        case "strikeout": .strikeOut
        case "note": .text
        default: nil
        }
    }

    /// An ink mark as one PDFKit ink annotation, its paths relative to its
    /// bounds.
    private func drawInk(_ mark: AnnotationInfo, on page: PDFPage) {
        let points = mark.strokes.flatMap { $0 }
        guard let first = points.first else { return }
        var box = CGRect(x: first.x, y: first.y, width: 0, height: 0)
        for p in points {
            box = box.union(CGRect(x: p.x, y: p.y, width: 0, height: 0))
        }
        let bounds = box.insetBy(dx: -3, dy: -3)
        let annotation = PDFAnnotation(bounds: bounds, forType: .ink, withProperties: nil)
        annotation.color = PlatformColor(hex: mark.color) ?? .systemRed
        let border = PDFBorder()
        border.lineWidth = 3
        annotation.border = border
        for stroke in mark.strokes {
            let path = PlatformBezierPath()
            for (index, p) in stroke.enumerated() {
                let point = CGPoint(x: p.x - bounds.minX, y: p.y - bounds.minY)
                if index == 0 {
                    path.move(to: point)
                    if stroke.count == 1 { path.addLine(to: point) }
                } else {
                    path.addLine(to: point)
                }
            }
            annotation.add(path)
        }
        if let comment = mark.comment {
            annotation.contents = comment
        }
        page.addAnnotation(annotation)
        drawn[mark.id] = [annotation]
    }

    /// Keeps a stroke drawn over the view (in its top-left coordinates) as
    /// an ink mark on the page under where it began.
    func finishStroke(_ points: [CGPoint]) {
        guard let view, let document, let library, let first = points.first else { return }
        func inView(_ point: CGPoint) -> CGPoint {
            #if os(macOS)
            return view.isFlipped ? point : CGPoint(x: point.x, y: view.bounds.height - point.y)
            #else
            return point
            #endif
        }
        guard let page = view.page(for: inView(first), nearest: true) else { return }
        let stroke = points.map { point in
            let onPage = view.convert(inView(point), to: page)
            return AnnotationPoint(x: Double(onPage.x), y: Double(onPage.y))
        }
        let new = NewAnnotation(
            attachmentId: attachmentID, kind: "ink", color: markColor.hex, pageIndex: UInt32(document.index(for: page)),
            rects: [], text: nil, comment: nil, strokes: [stroke])
        if let saved = library.addAnnotation(new) {
            draw(saved, in: document)
            marks.append(saved)
        }
    }

    /// A text box, rectangle, ellipse or arrow as one PDFKit annotation.
    private func drawShape(_ mark: AnnotationInfo, on page: PDFPage) {
        let color = PlatformColor(hex: mark.color) ?? .systemRed
        let border = PDFBorder()
        border.lineWidth = 2
        let annotation: PDFAnnotation
        switch mark.kind {
        case "arrow":
            guard let line = mark.strokes.first, let tail = line.first, let head = line.last else { return }
            let from = CGPoint(x: tail.x, y: tail.y)
            let to = CGPoint(x: head.x, y: head.y)
            let bounds = CGRect(
                x: min(from.x, to.x), y: min(from.y, to.y), width: abs(to.x - from.x), height: abs(to.y - from.y)
            ).insetBy(dx: -10, dy: -10)
            annotation = PDFAnnotation(bounds: bounds, forType: .line, withProperties: nil)
            annotation.startPoint = CGPoint(x: from.x - bounds.minX, y: from.y - bounds.minY)
            annotation.endPoint = CGPoint(x: to.x - bounds.minX, y: to.y - bounds.minY)
            annotation.startLineStyle = .none
            annotation.endLineStyle = .openArrow
            annotation.color = color
            annotation.border = border
        case "stamp":
            guard let rect = mark.rects.first else { return }
            let bounds = CGRect(x: rect.x, y: rect.y, width: rect.width, height: rect.height)
            annotation = StampAnnotation(bounds: bounds, word: mark.comment ?? "", tint: color)
        case "text":
            guard let rect = mark.rects.first else { return }
            let bounds = CGRect(x: rect.x, y: rect.y, width: rect.width, height: rect.height)
            annotation = PDFAnnotation(bounds: bounds, forType: .freeText, withProperties: nil)
            annotation.font = PlatformFont.systemFont(ofSize: CGFloat(mark.fontSize ?? 14))
            annotation.fontColor = color
            annotation.color = .clear
            let none = PDFBorder()
            none.lineWidth = 0
            annotation.border = none
        default:
            guard let rect = mark.rects.first else { return }
            let bounds = CGRect(x: rect.x, y: rect.y, width: rect.width, height: rect.height)
            annotation = PDFAnnotation(bounds: bounds, forType: mark.kind == "circle" ? .circle : .square, withProperties: nil)
            annotation.color = color
            annotation.border = border
        }
        if let comment = mark.comment {
            annotation.contents = comment
        }
        page.addAnnotation(annotation)
        drawn[mark.id] = [annotation]
    }

    /// A point of the view (in its top-left coordinates) in PDFKit's view
    /// coordinates.
    private func inView(_ point: CGPoint) -> CGPoint {
        #if os(macOS)
        guard let view, !view.isFlipped else { return point }
        return CGPoint(x: point.x, y: view.bounds.height - point.y)
        #else
        return point
        #endif
    }

    /// Keeps a rectangle, ellipse or arrow dragged over the view (in its
    /// top-left coordinates) on the page under where it began.
    func finishShape(from start: CGPoint, to end: CGPoint) {
        guard let view, let document, let library, [.square, .circle, .arrow].contains(tool),
            let page = view.page(for: inView(start), nearest: true)
        else { return }
        let a = view.convert(inView(start), to: page)
        let b = view.convert(inView(end), to: page)
        guard hypot(b.x - a.x, b.y - a.y) >= 4 else { return }
        let point = { (p: CGPoint) in AnnotationPoint(x: Double(p.x), y: Double(p.y)) }
        let box = AnnotationRect(
            x: Double(min(a.x, b.x)), y: Double(min(a.y, b.y)), width: Double(abs(b.x - a.x)),
            height: Double(abs(b.y - a.y)))
        let new = NewAnnotation(
            attachmentId: attachmentID, kind: tool.rawValue, color: markColor.hex,
            pageIndex: UInt32(document.index(for: page)), rects: tool == .arrow ? [] : [box], text: nil, comment: nil,
            strokes: tool == .arrow ? [[point(a), point(b)]] : [])
        if let saved = library.addAnnotation(new) {
            draw(saved, in: document)
            marks.append(saved)
        }
    }

    /// A tap with the text box, stamp or signature tool (in the view's
    /// top-left coordinates).
    func place(at point: CGPoint) {
        switch tool {
        case .text: placeText(at: point)
        case .stamp: addMark(at: point, kind: "stamp")
        case .sign:
            if signature != nil { addMark(at: point, kind: "ink") } else { askingSignature = true }
        default: break
        }
    }

    /// Keeps a signature written on the pad and takes up the Sign tool.
    func keepSignature(_ written: SavedSignature) {
        written.save()
        signature = written
        tool = .sign
    }

    /// A stamp, or the signature (as ink), centred where the view was tapped.
    private func addMark(at point: CGPoint, kind: String) {
        guard let view, let document, let library, let page = view.page(for: inView(point), nearest: true) else { return }
        let at = view.convert(inView(point), to: page)
        var new = NewAnnotation(
            attachmentId: attachmentID, kind: kind, color: markColor.hex, pageIndex: UInt32(document.index(for: page)),
            rects: [], text: nil, comment: nil, strokes: [])
        if kind == "stamp" {
            let word = stampWord
            let width = max(80, Double(word.count) * 0.72 * 20 + 24)
            new.rects = [AnnotationRect(x: Double(at.x) - width / 2, y: Double(at.y) - 18, width: width, height: 36)]
            new.comment = word
        } else if let signature {
            new.strokes = signature.placed(x: Double(at.x), y: Double(at.y), width: 160)
        }
        if let saved = library.addAnnotation(new) {
            draw(saved, in: document)
            marks.append(saved)
        }
    }

    /// Starts a text box where the view was tapped (in its top-left
    /// coordinates).
    func placeText(at point: CGPoint) {
        guard let view, let document, let page = view.page(for: inView(point), nearest: true) else { return }
        placingText = (document.index(for: page), view.convert(inView(point), to: page))
    }

    /// Writes the text box being placed, sized to its text.
    func addText(_ text: String) {
        let value = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let placing = placingText, let document, let library, !value.isEmpty else {
            placingText = nil
            return
        }
        placingText = nil
        let font = PlatformFont.systemFont(ofSize: 14)
        let size = (value as NSString).boundingRect(
            with: CGSize(width: 10_000, height: 10_000), options: [.usesLineFragmentOrigin],
            attributes: [.font: font], context: nil
        ).size
        let corner = placing.corner
        let box = AnnotationRect(
            x: Double(corner.x), y: Double(corner.y - size.height - 4), width: Double(size.width + 8),
            height: Double(size.height + 4))
        let new = NewAnnotation(
            attachmentId: attachmentID, kind: "text", color: markColor.hex, pageIndex: UInt32(placing.page),
            rects: [box], text: nil, comment: value, strokes: [], fontSize: 14)
        if let saved = library.addAnnotation(new) {
            draw(saved, in: document)
            marks.append(saved)
        }
    }

    private func draw(_ mark: AnnotationInfo, in document: PDFDocument) {
        if mark.kind == "ink" {
            if let page = document.page(at: Int(mark.pageIndex)) { drawInk(mark, on: page) }
            return
        }
        if ["text", "square", "circle", "arrow", "stamp"].contains(mark.kind) {
            if let page = document.page(at: Int(mark.pageIndex)) { drawShape(mark, on: page) }
            return
        }
        guard let page = document.page(at: Int(mark.pageIndex)), let kind = subtype(for: mark.kind) else { return }
        let color = (PlatformColor(hex: mark.color) ?? .systemYellow)
            .withAlphaComponent(mark.kind == "highlight" ? 0.4 : 0.9)
        drawn[mark.id] = mark.rects.map { rect in
            let bounds = CGRect(x: rect.x, y: rect.y, width: rect.width, height: rect.height)
            let annotation = PDFAnnotation(bounds: bounds, forType: kind, withProperties: nil)
            annotation.color = color
            if let comment = mark.comment {
                annotation.contents = comment
            }
            page.addAnnotation(annotation)
            return annotation
        }
    }

    /// Highlights, underlines or strikes through the selected text: one
    /// stored mark per page, one rectangle per line.
    func mark(_ kind: String) {
        guard let view, let document, let library, let selection = view.currentSelection else { return }
        var rects: [Int: [CGRect]] = [:]
        var texts: [Int: [String]] = [:]
        for line in selection.selectionsByLine() {
            for page in line.pages {
                let index = document.index(for: page)
                rects[index, default: []].append(line.bounds(for: page))
                if let text = line.string {
                    texts[index, default: []].append(text)
                }
            }
        }
        for (index, lines) in rects.sorted(by: { $0.key < $1.key }) {
            let new = NewAnnotation(
                attachmentId: attachmentID, kind: kind, color: markColor.hex, pageIndex: UInt32(index),
                rects: lines.map {
                    AnnotationRect(
                        x: Double($0.origin.x), y: Double($0.origin.y), width: Double($0.width),
                        height: Double($0.height))
                },
                text: texts[index]?.joined(separator: " "), comment: nil, strokes: [])
            if let saved = library.addAnnotation(new) {
                draw(saved, in: document)
                marks.append(saved)
            }
        }
        view.clearSelection()
    }

    /// Removes the marks under the selected text.
    func eraseMarks() {
        guard let view, let library, let selection = view.currentSelection else { return }
        var hit = Set<String>()
        for page in selection.pages {
            let bounds = selection.bounds(for: page)
            for (id, annotations) in drawn
            where annotations.contains(where: { $0.page == page && $0.bounds.intersects(bounds) }) {
                hit.insert(id)
            }
        }
        for id in hit {
            library.removeAnnotation(id: id)
            undraw(id)
        }
        view.clearSelection()
    }

    private func undraw(_ id: String) {
        for annotation in drawn.removeValue(forKey: id) ?? [] {
            annotation.page?.removeAnnotation(annotation)
        }
        marks.removeAll { $0.id == id }
    }

    /// Removes one mark (from the annotation list).
    func remove(_ mark: AnnotationInfo) {
        library?.removeAnnotation(id: mark.id)
        undraw(mark.id)
    }

    /// Saves a mark's comment; empty removes it.
    func setComment(_ comment: String, on mark: AnnotationInfo) {
        let text = comment.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let current = library?.annotations(attachmentID: attachmentID).first(where: { $0.id == mark.id }),
              text != (current.comment ?? ""),
              let saved = library?.updateAnnotation(id: mark.id, color: current.color, comment: text.isEmpty ? nil : text)
        else { return }
        if let index = marks.firstIndex(where: { $0.id == saved.id }) {
            marks[index] = saved
        }
        for annotation in drawn[saved.id] ?? [] {
            annotation.contents = saved.comment
        }
    }

    /// The text selected in the PDF.
    var selectedText: String {
        view?.currentSelection?.string?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    }

    var selectionAnnotation: [String: Any] {
        var annotation: [String: Any] = ["text": selectedText, "type": "highlight"]
        if let document, let page = view?.currentSelection?.pages.first {
            annotation["pageIndex"] = document.index(for: page)
        }
        return annotation
    }

    func goTo(page index: Int) {
        guard let page = document?.page(at: index) else { return }
        view?.go(to: page)
    }

    func zoomIn() { view?.zoomIn(nil) }
    func zoomOut() { view?.zoomOut(nil) }
    func zoomToFit() { view?.autoScales = true }

    /// Selects the next place the search text appears, from the start again
    /// after the last one.
    func findNext() {
        guard let document, let view else { return }
        let text = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        if text != matchedText {
            matches = document.findString(text, withOptions: [.caseInsensitive, .diacriticInsensitive])
            matchIndex = 0
            matchedText = text
        } else if !matches.isEmpty {
            matchIndex = (matchIndex + 1) % matches.count
        }
        guard matches.indices.contains(matchIndex) else { return }
        let match = matches[matchIndex]
        view.setCurrentSelection(match, animate: true)
        view.go(to: match)
    }

    /// What the form's boxes hold now, for the fields that changed: PDFKit
    /// fills them in as they are typed into or clicked. A box is found by
    /// its page and where it lies.
    func formValues() -> [PdfFieldValue] {
        guard let document else { return [] }
        var values: [PdfFieldValue] = []
        for field in formFields where !field.readOnly {
            var value: String?
            for widget in field.widgets {
                guard let page = document.page(at: Int(widget.pageIndex)) else { continue }
                let place = CGRect(x: widget.x, y: widget.y, width: widget.width, height: widget.height)
                guard let box = page.annotations.first(where: {
                    ($0.type ?? "").hasSuffix("Widget") && abs($0.bounds.minX - place.minX) < 1.5
                        && abs($0.bounds.minY - place.minY) < 1.5 && abs($0.bounds.width - place.width) < 1.5
                        && abs($0.bounds.height - place.height) < 1.5
                }) else { continue }
                switch field.kind {
                case "text", "choice":
                    value = box.widgetStringValue ?? ""
                case "checkbox", "radio":
                    if box.buttonWidgetState == .onState {
                        value = widget.onValue
                    } else if value == nil {
                        value = "Off"
                    }
                default:
                    break
                }
                if value != nil && field.kind != "checkbox" && field.kind != "radio" { break }
            }
            if let value, value != field.value {
                values.append(PdfFieldValue(name: field.name, value: value))
            }
        }
        return values
    }

    func close() {
        if let pageObserver {
            NotificationCenter.default.removeObserver(pageObserver)
        }
        pageObserver = nil
        if let selectionObserver {
            NotificationCenter.default.removeObserver(selectionObserver)
        }
        selectionObserver = nil
    }
}

/// One entry of a PDF's table of contents.
struct OutlineEntry: Identifiable {
    let id: Int
    let title: String
    let level: Int
    /// From 0; nil when the entry points nowhere in the document.
    let page: Int?

    static func read(_ document: PDFDocument) -> [OutlineEntry] {
        var entries: [OutlineEntry] = []
        func walk(_ node: PDFOutline, level: Int) {
            for index in 0..<node.numberOfChildren {
                guard let child = node.child(at: index) else { continue }
                let page = child.destination?.page.map { document.index(for: $0) }
                entries.append(OutlineEntry(
                    id: entries.count, title: child.label ?? "", level: level, page: page))
                walk(child, level: level + 1)
            }
        }
        if let root = document.outlineRoot {
            walk(root, level: 0)
        }
        return entries
    }
}

/// Draws over the pages while a drawing tool is on: a pen stroke, or a
/// shape from where the drag began to where it is, kept when the finger,
/// pencil or mouse lifts; a tap with the text box tool places a box.
private struct DrawOverlay: View {
    let tool: PDFTool
    let color: Color
    let onStroke: ([CGPoint]) -> Void
    let onShape: (CGPoint, CGPoint) -> Void
    let onTap: (CGPoint) -> Void
    @State private var points: [CGPoint] = []

    var body: some View {
        preview
            .stroke(color, style: StrokeStyle(lineWidth: 3, lineCap: .round, lineJoin: .round))
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { value in
                        if tool == .ink || points.isEmpty {
                            points.append(value.location)
                        } else {
                            points = [points[0], value.location]
                        }
                    }
                    .onEnded { value in
                        switch tool {
                        case .ink: onStroke(points)
                        case .text, .stamp, .sign: onTap(value.startLocation)
                        case .square, .circle, .arrow: onShape(value.startLocation, value.location)
                        case .select: break
                        }
                        points = []
                    })
    }

    private var preview: Path {
        Path { path in
            guard let first = points.first, let last = points.last else { return }
            let box = CGRect(
                x: min(first.x, last.x), y: min(first.y, last.y), width: abs(last.x - first.x),
                height: abs(last.y - first.y))
            switch tool {
            case .ink:
                path.move(to: first)
                for point in points.dropFirst() {
                    path.addLine(to: point)
                }
            case .square:
                path.addRect(box)
            case .circle:
                path.addEllipse(in: box)
            case .arrow:
                path.move(to: first)
                path.addLine(to: last)
                let angle = atan2(last.y - first.y, last.x - first.x)
                for side in [CGFloat.pi - 0.44, CGFloat.pi + 0.44] {
                    path.move(to: last)
                    path.addLine(to: CGPoint(x: last.x + 10 * cos(angle + side), y: last.y + 10 * sin(angle + side)))
                }
            case .text, .stamp, .sign, .select:
                break
            }
        }
    }
}

#if os(macOS)
private struct PDFKitView: NSViewRepresentable {
    let model: PDFReaderModel
    func makeNSView(context: Context) -> PDFView { model.makeView() }
    func updateNSView(_ view: PDFView, context: Context) {}
}
#else
private struct PDFKitView: UIViewRepresentable {
    let model: PDFReaderModel
    func makeUIView(context: Context) -> PDFView { model.makeView() }
    func updateUIView(_ view: PDFView, context: Context) {}
}
#endif

/// Reads a PDF and marks it up: highlight, underline or strike through the
/// selected text in a choice of colors.
struct PDFReaderView: View {
    let title: String
    @Environment(LibraryModel.self) private var library
    @Environment(VaultModel.self) private var vault
    @Environment(TabsModel.self) private var tabs
    @Environment(AIAssistant.self) private var ai
    @Environment(PluginHost.self) private var plugins
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    @State private var model: PDFReaderModel
    @State private var showingPanel = false
    @State private var panel = Panel.marks
    @State private var notice: String?
    /// Reading scanned pages for a text layer: a label, not an alert.
    @State private var recognizing = false
    @State private var question = ""
    @State private var answer: AiAnswer?
    @State private var asking = false
    @State private var aiError: String?
    @State private var confirmingDelete = false
    @State private var exportFile: ExportFile?
    @State private var boxText = ""
    @State private var selectionPluginText: String?
    @State private var viewPluginMenus: [ReaderMenu] = []

    private enum Panel { case marks, contents, ai }

    init(url: URL, attachmentID: String, title: String, page: Int? = nil) {
        self.title = title
        _model = State(initialValue: PDFReaderModel(url: url, attachmentID: attachmentID, page: page))
    }

    var body: some View {
        @Bindable var model = model
        Group {
            if model.document != nil {
                PDFKitView(model: model)
                    // A file read again after its pages changed gets a new view.
                    .id(ObjectIdentifier(model))
                    .contextMenu {
                        ForEach(viewPluginMenus, id: \.plugin) { menu in
                            ForEach(menu.items) { item in
                                Button {
                                    runViewPluginItem(item, plugin: menu.plugin)
                                } label: {
                                    Text(verbatim: item.label)
                                }
                                .disabled(item.disabled)
                            }
                        }
                    }
                    .overlay {
                        if model.tool != .select {
                            DrawOverlay(
                                tool: model.tool, color: model.markColor.color,
                                onStroke: { model.finishStroke($0) },
                                onShape: { model.finishShape(from: $0, to: $1) },
                                onTap: { model.place(at: $0) })
                        }
                    }
                    .overlay {
                        // Difference with white turns the pages negative:
                        // light text on dark paper.
                        if model.night {
                            Rectangle().fill(.white).blendMode(.difference).allowsHitTesting(false)
                        }
                    }
            } else {
                ContentUnavailableView(
                    "Cannot Open PDF", systemImage: "doc.questionmark",
                    description: Text(model.url.lastPathComponent))
            }
        }
        .navigationTitle(title)
        .searchable(text: $model.searchText, prompt: Text("Find in PDF"))
        .onSubmit(of: .search) { model.findNext() }
        .toolbar { tools }
        #if os(iOS)
        // The iPhone's tab bar would cover the annotation tools below.
        .toolbar(.hidden, for: .tabBar)
        #endif
        .overlay(alignment: .bottomLeading) {
            if let selectionPluginText, !selectionPluginText.isEmpty {
                HStack(alignment: .top, spacing: 8) {
                    Text(verbatim: selectionPluginText).font(.callout).textSelection(.enabled)
                    Button("Close", systemImage: "xmark") { self.selectionPluginText = nil }
                        .labelStyle(.iconOnly)
                        .buttonStyle(.borderless)
                }
                .padding(12)
                .frame(maxWidth: 360, alignment: .leading)
                .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 8))
                .padding(12)
            }
        }
        .inspector(isPresented: $showingPanel) {
            VStack(spacing: 0) {
                Picker("Show", selection: $panel) {
                    Text("Annotations").tag(Panel.marks)
                    if !model.outline.isEmpty {
                        Text("Contents").tag(Panel.contents)
                    }
                    Text("Ask AI").tag(Panel.ai)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .padding(8)
                switch panel {
                case .contents where !model.outline.isEmpty:
                    OutlineList(model: model)
                case .ai:
                    askPanel
                default:
                    MarksList(model: model) { notice = $0 }
                }
            }
            .inspectorColumnWidth(min: 240, ideal: 300, max: 420)
            // On the iPhone the panel is a sheet: show that it drags away.
            .presentationDragIndicator(.visible)
        }
        .confirmationDialog(
            String(localized: "Delete page \(String(model.currentPageIndex + 1))? Its marks go with it."),
            isPresented: $confirmingDelete
        ) {
            Button("Delete Page…", role: .destructive) {
                editPages(.delete(pages: [UInt32(model.currentPageIndex)]))
            }
        }
        .fileExporter(
            isPresented: Binding(get: { exportFile != nil }, set: { if !$0 { exportFile = nil } }),
            document: exportFile,
            contentType: .pdf,
            defaultFilename: exportFile?.url.lastPathComponent
        ) { _ in
            if let url = exportFile?.url { try? FileManager.default.removeItem(at: url) }
            exportFile = nil
        }
        .sheet(isPresented: Binding(get: { model.askingSignature }, set: { model.askingSignature = $0 })) {
            SignaturePad { model.keepSignature($0) }
        }
        .alert(
            "Text Box",
            isPresented: Binding(
                get: { model.placingText != nil },
                set: { shown in
                    // After the button's own action, which reads the place.
                    if !shown { Task { @MainActor in model.placingText = nil } }
                })
        ) {
            TextField("Text", text: $boxText)
            Button("Add") {
                model.addText(boxText)
                boxText = ""
            }
            Button("Cancel", role: .cancel) { boxText = "" }
        }
        .overlay(alignment: .bottom) {
            if recognizing {
                Label {
                    Text("Recognizing text…")
                } icon: {
                    ProgressView()
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .glassEffect(.regular, in: .capsule)
                .padding(.bottom, 96)
            }
        }
        .alert("Annotations", isPresented: Binding(get: { notice != nil }, set: { if !$0 { notice = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(notice ?? "")
        }
        .task {
            model.load(from: library)
            // A document that opened counts as opening the file, for plugins.
            if model.document != nil {
                await plugins.readerOpened(attachmentKey: model.attachmentID, instance: ObjectIdentifier(model))
            }
        }
        .onChange(of: library.revision) { _, _ in model.reloadMarks() }
        .task(id: "\(ObjectIdentifier(model)):\(readerEventsID)") {
            viewPluginMenus = []
            let menus = await plugins.readerMenu(
                "createViewContextMenu", attachmentKey: model.attachmentID,
                itemKey: library.attachment(id: model.attachmentID)?.itemId, params: [:])
            guard !Task.isCancelled else { return }
            viewPluginMenus = menus
        }
        .task(id: selectionRequestID) {
            selectionPluginText = nil
            guard !model.selectedText.isEmpty,
                  plugins.readerEvents.values.contains(where: { $0.contains("renderTextSelectionPopup") })
            else { return }
            do { try await Task.sleep(for: .milliseconds(150)) } catch { return }
            let rendered = await plugins.readerRender(
                "renderTextSelectionPopup", attachmentKey: model.attachmentID,
                itemKey: library.attachment(id: model.attachmentID)?.itemId,
                params: ["annotation": model.selectionAnnotation])
            guard !Task.isCancelled else { return }
            selectionPluginText = rendered.compactMap { PluginSectionBox.plainText($0.html) }
                .filter { !$0.isEmpty }.joined(separator: "\n")
        }
        .onDisappear { model.close() }
    }

    private var selectionRequestID: String {
        "\(ObjectIdentifier(model)):\(model.selectionRevision):\(readerEventsID)"
    }

    private var readerEventsID: String {
        plugins.readerEvents.keys.sorted().map { "\($0):\(plugins.readerEvents[$0] ?? [])" }.joined()
    }

    private func runViewPluginItem(_ item: ReaderMenuItem, plugin: String) {
        Task {
            let menus = await plugins.readerMenu(
                "createViewContextMenu", attachmentKey: model.attachmentID,
                itemKey: library.attachment(id: model.attachmentID)?.itemId, params: [:])
            guard let menu = menus.first(where: { $0.plugin == plugin }),
                  let current = menu.items.first(where: { $0.index == item.index && $0.label == item.label }), !current.disabled
            else { return }
            plugins.readerCommand(plugin: plugin, menu: menu.menu, index: current.index)
        }
    }

    private var placement: ToolbarItemPlacement {
        #if os(iOS)
        .bottomBar
        #else
        .primaryAction
        #endif
    }

    @ToolbarContentBuilder
    private var tools: some ToolbarContent {
        ToolbarItemGroup(placement: placement) {
            // The colour for highlights, pen strokes and shapes; the
            // button shows the one chosen.
            Menu {
                ForEach(MarkColor.allCases) { color in
                    Toggle(isOn: Binding(get: { model.markColor == color }, set: { _ in model.markColor = color })) {
                        Label {
                            Text(color.title)
                        } icon: {
                            colorDot(PlatformColor(hex: color.hex) ?? .systemYellow)
                        }
                    }
                }
            } label: {
                Label {
                    Text("Color")
                } icon: {
                    colorDot(PlatformColor(hex: model.markColor.hex) ?? .systemYellow, size: 16)
                }
            }
            .menuIndicator(.hidden)
            Button("Highlight", systemImage: "highlighter") { model.mark("highlight") }
            Button("Underline", systemImage: "underline") { model.mark("underline") }
            Button("Strikethrough", systemImage: "strikethrough") { model.mark("strikeout") }
            Menu {
                Picker(
                    "Tool",
                    selection: Binding(
                        get: { model.tool },
                        set: {
                            model.tool = $0
                            if $0 == .sign && model.signature == nil { model.askingSignature = true }
                        })
                ) {
                    ForEach(PDFTool.allCases) { tool in
                        Label(tool.title, systemImage: tool.symbol).tag(tool)
                    }
                }
                .pickerStyle(.inline)
                if model.tool == .stamp {
                    Picker("Stamp", selection: Binding(get: { model.stampWord }, set: { model.stampWord = $0 })) {
                        ForEach(stampWords, id: \.self) { word in
                            Text(LocalizedStringKey(word)).tag(word)
                        }
                    }
                }
                if model.tool == .sign && model.signature != nil {
                    Button("Change Signature…", systemImage: "signature") { model.askingSignature = true }
                }
            } label: {
                Label("Tool", systemImage: model.tool.symbol)
            }
            Button("Erase Marks", systemImage: "eraser") { model.eraseMarks() }
        }
        ToolbarItemGroup(placement: .primaryAction) {
            Menu {
                Toggle("Two Pages", systemImage: "book", isOn: Binding(get: { model.twoUp }, set: { model.twoUp = $0 }))
                Toggle("Night", systemImage: "moon", isOn: Binding(get: { model.night }, set: { model.night = $0 }))
                if !model.outline.isEmpty {
                    Button("Contents", systemImage: "list.bullet.indent") {
                        panel = .contents
                        showingPanel = true
                    }
                }
            } label: {
                Label("View", systemImage: "eye")
            }
            Button {
                if showingPanel && panel == .marks {
                    showingPanel = false
                } else {
                    panel = .marks
                    showingPanel = true
                }
            } label: {
                Label("Annotations", systemImage: "list.bullet.rectangle")
            }
            Menu {
                Button("Translate", systemImage: "character.book.closed") { helpWith("translate") }
                Button("Explain", systemImage: "questionmark.bubble") { helpWith("explain") }
                Button("Ask AI", systemImage: "bubble.left.and.text.bubble.right") {
                    panel = .ai
                    showingPanel = true
                }
            } label: {
                Label("AI", systemImage: "sparkles")
            }
            Button("Literature Note", systemImage: "note.text.badge.plus") { literatureNote() }
            if !model.formFields.isEmpty {
                Button("Save Form", systemImage: "square.and.arrow.down.on.square") { saveForm() }
            }
            Menu {
                Button("Turn Page Left", systemImage: "rotate.left") {
                    editPages(.rotate(pages: [UInt32(model.currentPageIndex)], degrees: -90))
                }
                Button("Turn Page Right", systemImage: "rotate.right") {
                    editPages(.rotate(pages: [UInt32(model.currentPageIndex)], degrees: 90))
                }
                Button("Move Page Up", systemImage: "arrow.up") {
                    let index = model.currentPageIndex
                    if index > 0 { editPages(.move(from: UInt32(index), to: UInt32(index - 1))) }
                }
                Button("Move Page Down", systemImage: "arrow.down") {
                    let index = model.currentPageIndex
                    if index + 1 < model.pageCount { editPages(.move(from: UInt32(index), to: UInt32(index + 1))) }
                }
                Button("Add Blank Page After", systemImage: "doc.badge.plus") {
                    editPages(.insertBlank(at: UInt32(model.currentPageIndex + 1)))
                }
                Button("Delete Page…", systemImage: "trash", role: .destructive) { confirmingDelete = true }
                Divider()
                Button("Recognize Text", systemImage: "text.viewfinder") { recognizeText() }
                    .disabled(recognizing)
                Button("Save a Copy with Annotations…", systemImage: "square.and.arrow.down") { exportAnnotated() }
            } label: {
                Label("Edit Pages", systemImage: "doc.on.doc")
            }
        }
        ToolbarItemGroup(placement: placement) {
            Button("Zoom Out", systemImage: "minus.magnifyingglass") { model.zoomOut() }
            Button("Zoom to Fit", systemImage: "arrow.up.left.and.down.right.magnifyingglass") { model.zoomToFit() }
            Button("Zoom In", systemImage: "plus.magnifyingglass") { model.zoomIn() }
            if !model.pageLabel.isEmpty {
                Text(verbatim: model.pageLabel)
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
            }
        }
    }
}

extension PDFReaderView {
    /// Changes the pages (the file is rewritten), then reads it again at the
    /// same place; the marks follow their pages.
    fileprivate func editPages(_ edit: PageEdit) {
        guard let core = library.core else { return }
        let (id, url, index) = (model.attachmentID, model.url, model.currentPageIndex)
        Task {
            do {
                _ = try await Task.detached { try core.editPdfPages(attachmentId: id, edit: edit) }.value
                model.close()
                let reopened = PDFReaderModel(url: url, attachmentID: id, page: index)
                model = reopened
                reopened.load(from: library)
            } catch {
                notice = CoreKit.message(for: error)
            }
        }
    }

    /// Reads the scanned pages' pictures and writes their text into the PDF,
    /// invisible, so it can be selected and searched; then reads it again at
    /// the same place.
    fileprivate func recognizeText() {
        guard let core = library.core else { return }
        let (id, url, index) = (model.attachmentID, model.url, model.currentPageIndex)
        recognizing = true
        Task {
            defer { recognizing = false }
            do {
                let (pages, scanned) = await Task.detached { PDFTextRecognition.scannedPages(of: url) }.value
                guard !pages.isEmpty else {
                    notice = scanned == 0
                        ? String(localized: "No scanned pages: every page has text already.")
                        : String(localized: "No words were found on the scanned pages.")
                    return
                }
                _ = try await Task.detached { try core.addTextLayer(attachmentId: id, pages: pages) }.value
                model.close()
                let reopened = PDFReaderModel(url: url, attachmentID: id, page: index)
                model = reopened
                reopened.load(from: library)
                let count = String(pages.count)
                notice = String(localized: "Pages with recognized text: \(count). The text can be selected and searched now.")
            } catch {
                notice = CoreKit.message(for: error)
            }
        }
    }

    /// Writes what was filled into the form's boxes into the file, then
    /// reads it again at the same place.
    fileprivate func saveForm() {
        guard let core = library.core else { return }
        let values = model.formValues()
        guard !values.isEmpty else {
            notice = String(localized: "Nothing in the form has changed.")
            return
        }
        let (id, url, index) = (model.attachmentID, model.url, model.currentPageIndex)
        Task {
            do {
                _ = try await Task.detached { try core.fillPdfForm(attachmentId: id, values: values) }.value
                model.close()
                let reopened = PDFReaderModel(url: url, attachmentID: id, page: index)
                model = reopened
                reopened.load(from: library)
                notice = String(localized: "The form is saved in the PDF.")
            } catch {
                notice = CoreKit.message(for: error)
            }
        }
    }

    /// A copy of the PDF with the marks written in, for the file exporter.
    fileprivate func exportAnnotated() {
        guard let core = library.core else { return }
        let name = (title as NSString).deletingPathExtension
        let url = FileManager.default.temporaryDirectory.appending(path: "\(name) (annotated).pdf")
        do {
            _ = try core.exportAnnotatedPdf(attachmentId: model.attachmentID, destination: url.path(percentEncoded: false))
            exportFile = ExportFile(url: url)
        } catch {
            notice = CoreKit.message(for: error)
        }
    }

    /// The Ask AI panel: a question about the paper, and the answer (or a
    /// translation or explanation of the selection).
    fileprivate var askPanel: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 10) {
                TextField("A question about this paper", text: $question, axis: .vertical)
                    .lineLimit(2...6)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { ask() }
                Button {
                    ask()
                } label: {
                    if asking {
                        ProgressView().controlSize(.small)
                    } else {
                        Text("Ask")
                    }
                }
                .disabled(asking || question.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                if let aiError {
                    Text(aiError).font(.caption).foregroundStyle(.red)
                }
                if let answer {
                    AIAnswerView(answer: answer)
                }
            }
            .padding(12)
        }
    }

    private func runAI(_ request: @escaping @Sendable () throws -> AiAnswer) {
        panel = .ai
        showingPanel = true
        asking = true
        aiError = nil
        Task {
            do {
                answer = try await Task.detached { try request() }.value
            } catch {
                aiError = CoreKit.message(for: error)
            }
            asking = false
        }
    }

    fileprivate func ask() {
        guard let core = library.core, !asking else { return }
        let provider = ai.provider
        let language = AIAssistant.language
        let attachment = model.attachmentID
        let question = question
        runAI { try core.aiAsk(provider: provider, attachmentId: attachment, question: question, language: language) }
    }

    /// Translates or explains the selected text.
    fileprivate func helpWith(_ task: String) {
        let text = model.selectedText
        guard !text.isEmpty else {
            notice = String(localized: "Select some text first.")
            return
        }
        let provider = ai.provider
        let language = AIAssistant.language
        runAI { try aiTransform(provider: provider, task: task, text: text, language: language) }
    }

    /// Makes the item's literature note; shows it in the Notes tab on a
    /// Mac or iPad, or says where it is on a phone.
    fileprivate func literatureNote() {
        guard let itemID = library.attachment(id: model.attachmentID)?.itemId else { return }
        vault.createLiteratureNote(itemID: itemID)
        #if os(iOS)
        if sizeClass == .compact {
            notice = String(localized: "Saved in Notes as “\(vault.title)”.")
            return
        }
        #endif
        tabs.open(.notes)
    }
}

/// The PDF's table of contents; picking an entry turns to its page.
private struct OutlineList: View {
    let model: PDFReaderModel

    var body: some View {
        List(model.outline) { entry in
            Button {
                if let page = entry.page { model.goTo(page: page) }
            } label: {
                HStack {
                    Text(entry.title).lineLimit(2)
                    Spacer()
                    if let page = entry.page {
                        Text(verbatim: String(page + 1)).monospacedDigit().foregroundStyle(.secondary)
                    }
                }
                .padding(.leading, CGFloat(entry.level) * 12)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(entry.page == nil)
        }
    }
}

/// The marks on a PDF, page by page: jump to one, comment on it, copy it
/// into a note, or remove it.
private struct MarksList: View {
    let model: PDFReaderModel
    let onNotice: (String) -> Void
    @Environment(LibraryModel.self) private var library
    @Environment(PluginHost.self) private var plugins
    @State private var drafts: [String: String] = [:]
    @State private var pluginHeaders: [String: String] = [:]
    @State private var pluginMenus: [String: [ReaderMenu]] = [:]

    var body: some View {
        let ordered = model.marks.sorted {
            ($0.pageIndex, -($0.rects.first?.y ?? 0)) < ($1.pageIndex, -($1.rects.first?.y ?? 0))
        }
        List {
            if ordered.isEmpty {
                Text("Select text and highlight it to see it here.").foregroundStyle(.secondary)
            }
            ForEach(ordered, id: \.id) { mark in
                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Circle().fill(Color(hex: mark.color) ?? .yellow).frame(width: 10, height: 10)
                        Button("Page \(String(mark.pageIndex + 1))") { model.goTo(page: Int(mark.pageIndex)) }
                            .buttonStyle(.borderless)
                        Spacer()
                        Menu {
                            Button("Copy for Notes", systemImage: "doc.on.doc") {
                                if let embed = library.annotationEmbed(id: mark.id) {
                                    Clipboard.copy(embed)
                                    onNotice(String(localized: "Copied. Paste it into a note to embed this highlight."))
                                }
                            }
                            Button("Remove", systemImage: "trash", role: .destructive) { model.remove(mark) }
                        } label: {
                            Label("More", systemImage: "ellipsis.circle").labelStyle(.iconOnly)
                        }
                        .menuIndicator(.hidden)
                        .fixedSize()
                    }
                    if let text = pluginHeaders[mark.id], !text.isEmpty {
                        Text(verbatim: text).font(.caption).textSelection(.enabled)
                    }
                    if let text = mark.text {
                        Text(text).font(.callout).lineLimit(6).textSelection(.enabled)
                    }
                    if !mark.tags.isEmpty {
                        Text(verbatim: mark.tags.joined(separator: " · ")).font(.caption)
                    }
                    TextField(
                        "Add a comment",
                        text: Binding(
                            get: { drafts[mark.id] ?? mark.comment ?? "" },
                            set: { drafts[mark.id] = $0 }),
                        axis: .vertical
                    )
                    .font(.caption)
                    .onSubmit {
                        if let draft = drafts.removeValue(forKey: mark.id) {
                            model.setComment(draft, on: mark)
                        }
                    }
                }
                .padding(.vertical, 4)
                .contextMenu {
                    ForEach(pluginMenus[mark.id] ?? [], id: \.plugin) { menu in
                        ForEach(menu.items) { item in
                            Button {
                                runPluginItem(item, plugin: menu.plugin, mark: mark)
                            } label: {
                                Text(verbatim: item.label)
                            }
                            .disabled(item.disabled)
                        }
                    }
                }
                .task(id: pluginRequestID(mark)) {
                    await loadPluginExtensions(mark)
                }
                .onDisappear {
                    if let draft = drafts[mark.id] { model.setComment(draft, on: mark) }
                }
            }
        }
    }

    private func pluginRequestID(_ mark: AnnotationInfo) -> String {
        let events = plugins.readerEvents.keys.sorted().map { "\($0):\(plugins.readerEvents[$0] ?? [])" }.joined()
        return "\(ObjectIdentifier(model)):\(mark.id):\(mark.text ?? ""):\(mark.comment ?? ""):\(mark.color):\(events)"
    }

    private func loadPluginExtensions(_ mark: AnnotationInfo) async {
        pluginHeaders[mark.id] = nil
        pluginMenus[mark.id] = nil
        let itemKey = library.attachment(id: model.attachmentID)?.itemId
        let annotation: [String: Any] = [
            "id": mark.id, "text": mark.text ?? "", "comment": mark.comment ?? "",
            "type": mark.kind, "color": mark.color, "pageIndex": mark.pageIndex,
        ]
        let headers = await plugins.readerRender(
            "renderSidebarAnnotationHeader", attachmentKey: model.attachmentID, itemKey: itemKey,
            params: ["annotation": annotation])
        guard !Task.isCancelled else { return }
        pluginHeaders[mark.id] = headers.compactMap { PluginSectionBox.plainText($0.html) }
            .filter { !$0.isEmpty }.joined(separator: "\n")
        let menus = await plugins.readerMenu(
            "createAnnotationContextMenu", attachmentKey: model.attachmentID, itemKey: itemKey,
            params: ["ids": [mark.id], "currentID": mark.id])
        guard !Task.isCancelled else { return }
        pluginMenus[mark.id] = menus
    }

    private func runPluginItem(_ item: ReaderMenuItem, plugin: String, mark: AnnotationInfo) {
        Task {
            // The runtime retains twenty menus, so refresh this row before dispatching its command.
            let menus = await plugins.readerMenu(
                "createAnnotationContextMenu", attachmentKey: model.attachmentID,
                itemKey: library.attachment(id: model.attachmentID)?.itemId,
                params: ["ids": [mark.id], "currentID": mark.id])
            guard let menu = menus.first(where: { $0.plugin == plugin }),
                  let current = menu.items.first(where: { $0.index == item.index && $0.label == item.label }), !current.disabled
            else { return }
            plugins.readerCommand(plugin: plugin, menu: menu.menu, index: current.index)
        }
    }
}

/// What the core needs to recognise a PDF: the text of its first pages and
/// the title stored in it.
enum PDFFirstPages {
    nonisolated static func read(_ url: URL, pages: Int = 2) -> (text: String, title: String?) {
        guard let document = PDFDocument(url: url) else { return ("", nil) }
        // A scanned first page is read from its picture, so it can still be
        // recognised by its DOI or title.
        let text = PDFTextRecognition.pages(of: document, count: pages).joined(separator: "\n")
        let title = document.documentAttributes?[PDFDocumentAttribute.titleAttribute] as? String
        return (String(text.prefix(20_000)), title)
    }

    /// Every page's text, for full-text search; scanned pages' read from
    /// their pictures.
    nonisolated static func allPages(_ url: URL) -> [String] {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        guard let document = PDFDocument(url: url) else { return [] }
        return PDFTextRecognition.pages(of: document)
    }
}

/// A pad to write a signature on once; it is kept on this device and put
/// on pages with the Sign tool.
private struct SignaturePad: View {
    let onSave: (SavedSignature) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var strokes: [[CGPoint]] = []

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 12) {
                Text("Write your signature; it is kept on this device and put on pages with Sign.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                Canvas { context, size in
                    var guide = Path()
                    guide.move(to: CGPoint(x: 24, y: size.height - 32))
                    guide.addLine(to: CGPoint(x: size.width - 24, y: size.height - 32))
                    context.stroke(guide, with: .color(.secondary.opacity(0.5)), style: StrokeStyle(lineWidth: 1, dash: [4, 4]))
                    for stroke in strokes {
                        var path = Path()
                        path.addLines(stroke.count > 1 ? stroke : stroke + stroke)
                        context.stroke(
                            path, with: .color(.black), style: StrokeStyle(lineWidth: 2.5, lineCap: .round, lineJoin: .round))
                    }
                }
                .frame(minWidth: 320, minHeight: 160)
                .background(.white, in: .rect(cornerRadius: 8))
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.separator))
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { value in
                            if value.translation == .zero || strokes.isEmpty {
                                strokes.append([value.location])
                            } else {
                                strokes[strokes.count - 1].append(value.location)
                            }
                        }
                        .onEnded { _ in strokes.append([]) })
                Button("Clear", role: .destructive) { strokes = [] }
                    .disabled(strokes.allSatisfy(\.isEmpty))
            }
            .padding()
            .navigationTitle("Signature")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        if let written = cropped() {
                            onSave(written)
                            dismiss()
                        }
                    }
                    .disabled(strokes.allSatisfy(\.isEmpty))
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 480, minHeight: 320)
        #endif
    }

    /// The strokes moved to the corner of the box around them.
    private func cropped() -> SavedSignature? {
        let drawn = strokes.filter { !$0.isEmpty }
        let all = drawn.flatMap { $0 }
        guard let left = all.map(\.x).min(), let top = all.map(\.y).min(), let right = all.map(\.x).max(),
            let bottom = all.map(\.y).max()
        else { return nil }
        return SavedSignature(
            width: Double(max(1, right - left)), height: Double(max(1, bottom - top)),
            strokes: drawn.map { stroke in stroke.map { [Double($0.x - left), Double($0.y - top)] } })
    }
}
