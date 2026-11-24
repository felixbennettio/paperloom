import CoreKit
import CoreText
import SwiftUI
#if os(macOS)
import AppKit
#endif

/// Sample papers and notes for the UI tests that photograph the screens
/// (apple/ScreensUITests). Launched with `-PaperloomScreenshots`, the app
/// keeps its library and notes in a temporary folder, made afresh at each
/// launch, so it never opens the person's own library or vault.
enum ScreenshotSample {
    static let isOn = ProcessInfo.processInfo.arguments.contains("-PaperloomScreenshots")
    /// With `-PaperloomStore` as well: the App Store's pictures. No sample
    /// plugins, scanned page or overlong title, which are there to be
    /// tested; a paper with a readable PDF and highlights instead.
    static let isStore = isOn && ProcessInfo.processInfo.arguments.contains("-PaperloomStore")

    /// `-PaperloomAppearance dark` (or `light`) overrides the system's
    /// appearance, which UI tests run by xcodebuild cannot set reliably.
    static var colorScheme: ColorScheme? {
        guard isOn else { return nil }
        switch UserDefaults.standard.string(forKey: "PaperloomAppearance") {
        case "dark": return .dark
        case "light": return .light
        default: return nil
        }
    }

    #if os(macOS)
    /// The App Store's Mac pictures are 16:10 (1280 × 800 at most): the
    /// library window takes that shape, title bar included, as large as the
    /// screen allows (a small screen's pictures are scaled up afterwards).
    @MainActor static func sizeWindowForStore() {
        guard isStore, let window = NSApp.windows.first(where: { $0.isVisible && $0.canBecomeMain }),
              let screen = window.screen ?? NSScreen.main
        else { return }
        let space = screen.visibleFrame
        let height = min(800, space.height, space.width / 1.6).rounded(.down)
        let width = (height * 1.6).rounded(.down)
        window.setFrame(
            NSRect(x: space.midX - width / 2, y: space.midY - height / 2, width: width, height: height),
            display: true)
    }
    #endif

    private static let root = FileManager.default.temporaryDirectory
        .appendingPathComponent("PaperloomScreenshots", isDirectory: true)

    /// A fresh library on disk (so it keeps files, as a real one does),
    /// with the sample papers; in memory if the folder cannot be made.
    static func library() -> LibraryModel {
        let folder = root.appendingPathComponent("Library", isDirectory: true)
        try? FileManager.default.removeItem(at: folder)
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            if !isStore { try writeZoteroPlugin(in: folder) }
            let path = folder.appendingPathComponent("library.sqlite").path(percentEncoded: false)
            let library = try PaperLibrary(path: path)
            fillNoting(library)
            return LibraryModel(library: library, location: path)
        } catch {
            let library = try? PaperLibrary.inMemory()
            if let library { fillNoting(library) }
            return LibraryModel(library: library, location: String(localized: "In memory"))
        }
    }

    /// The sample papers, then the scanned one; a step that fails leaves
    /// an item saying why, which the screenshots show at the top.
    private static func fillNoting(_ library: PaperLibrary) {
        for step in isStore ? [fill, addStorePDF] : [fill, addScan] {
            do {
                try step(library)
            } catch {
                _ = try? library.addItem(
                    newItem: NewItem(
                        itemType: "journal-article", title: "Sample step failed: \(CoreKit.message(for: error))",
                        authors: [], containerTitle: nil, year: nil, doi: nil))
            }
        }
    }

    /// A fresh vault in the temporary folder, with a few linked notes.
    static func vault() throws -> PaperVault {
        let folder = root.appendingPathComponent("Vault", isDirectory: true)
        try? FileManager.default.removeItem(at: folder)
        if isStore {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        } else {
            try writePlugin(in: folder)
        }
        let vault = try PaperVault.open(path: folder.path(percentEncoded: false))
        for (title, text) in notes {
            let note = try vault.createNote(folder: "", title: title)
            _ = try vault.writeNote(path: note.path, text: text)
        }
        return vault
    }

    /// A small Zotero 7 plugin, turned on, kept where the library keeps
    /// plugins: a "Decade" column under each item and a section in the
    /// item's page.
    private static func writeZoteroPlugin(in library: URL) throws {
        let zotero = library.appendingPathComponent("plugins/zotero", isDirectory: true)
        let plugin = zotero.appendingPathComponent("sample@paperloom.app", isDirectory: true)
        try FileManager.default.createDirectory(at: plugin, withIntermediateDirectories: true)
        let manifest = #"""
            {"manifest_version": 2, "name": "Paperloom Sample for Zotero", "version": "1.0.0",
             "description": "A decade column and an item section.", "author": "Paperloom",
             "applications": {"zotero": {"id": "sample@paperloom.app", "strict_min_version": "6.999", "strict_max_version": "7.*"}}}
            """#
        let bootstrap = #"""
            var columnKey;
            function install() {}
            function startup({ id }) {
              columnKey = Zotero.ItemTreeManager.registerColumn({
                dataKey: "decade", label: "Decade", pluginID: id,
                dataProvider: (item) => {
                  const year = parseInt(String(item.getField("date") || ""), 10);
                  return year ? `${Math.floor(year / 10) * 10}s` : "";
                },
              });
              Zotero.ItemPaneManager.registerSection({
                paneID: "paperloom-sample", pluginID: id,
                header: { label: "Sample Section" }, sidenav: { label: "Sample" },
                onRender: ({ body, item }) => {
                  body.textContent = `${item.getField("title")}: ${item.getCreators().length} creators`;
                },
              });
            }
            function shutdown() {
              if (columnKey) Zotero.ItemTreeManager.unregisterColumn(columnKey);
              Zotero.ItemPaneManager.unregisterSection("paperloom-sample");
            }
            function uninstall() {}
            """#
        try manifest.write(to: plugin.appendingPathComponent("manifest.json"), atomically: true, encoding: .utf8)
        try bootstrap.write(to: plugin.appendingPathComponent("bootstrap.js"), atomically: true, encoding: .utf8)
        try #"["sample@paperloom.app"]"#.write(
            to: zotero.appendingPathComponent("enabled.json"), atomically: true, encoding: .utf8)
    }

    /// A small Obsidian plugin, turned on, so the screens show the plugin
    /// runtime at work: it suggests words after "@" in a note.
    private static func writePlugin(in vault: URL) throws {
        let obsidian = vault.appendingPathComponent(".obsidian", isDirectory: true)
        let plugin = obsidian.appendingPathComponent("plugins/paperloom-sample", isDirectory: true)
        try FileManager.default.createDirectory(at: plugin, withIntermediateDirectories: true)
        let manifest = #"""
            {"id": "paperloom-sample", "name": "Paperloom Sample", "version": "1.0.0", "minAppVersion": "1.0.0",
             "description": "Suggests words after @.", "author": "Paperloom", "isDesktopOnly": false}
            """#
        let main = #"""
            const { Plugin, EditorSuggest, Notice } = require("obsidian");
            class Words extends EditorSuggest {
              onTrigger(cursor, editor) {
                const match = /@(\w*)$/.exec(editor.getLine(cursor.line).slice(0, cursor.ch));
                if (!match) return null;
                return { start: { line: cursor.line, ch: cursor.ch - match[0].length }, end: cursor, query: match[1] };
              }
              getSuggestions(context) {
                return ["Paperloom", "Papers", "Pattern"].filter((w) => w.toLowerCase().startsWith(context.query.toLowerCase()));
              }
              renderSuggestion(word, el) { el.setText(word); }
              selectSuggestion(word) { this.context.editor.replaceRange(word, this.context.start, this.context.end); }
            }
            module.exports = class Sample extends Plugin {
              onload() {
                this.registerEditorSuggest(new Words(this.app));
                this.addCommand({ id: "hello", name: "Say hello", callback: () => new Notice("Hello from a plugin") });
              }
            };
            """#
        try manifest.write(to: plugin.appendingPathComponent("manifest.json"), atomically: true, encoding: .utf8)
        try main.write(to: plugin.appendingPathComponent("main.js"), atomically: true, encoding: .utf8)
        try #"["paperloom-sample"]"#.write(
            to: obsidian.appendingPathComponent("community-plugins.json"), atomically: true, encoding: .utf8)
    }

    private struct Paper {
        var type = "journal-article"
        var title: String
        var authors: [String]
        var container: String?
        var year: Int32?
        var doi: String?
        var tags: [String] = []
        var starred = false
        var status = "unread"
        var collection: String?
        /// Only in the screens the tests walk.
        var testOnly = false
    }

    private static let papers = [
        Paper(
            type: "conference-paper", title: "Attention Is All You Need",
            authors: ["Vaswani, Ashish", "Shazeer, Noam", "Parmar, Niki", "Uszkoreit, Jakob"],
            container: "Advances in Neural Information Processing Systems", year: 2017,
            doi: "10.48550/arXiv.1706.03762", tags: ["transformers", "to-read"], starred: true,
            status: "reading", collection: "Thesis"),
        Paper(
            type: "conference-paper", title: "Deep Residual Learning for Image Recognition",
            authors: ["He, Kaiming", "Zhang, Xiangyu", "Ren, Shaoqing", "Sun, Jian"],
            container: "IEEE Conference on Computer Vision and Pattern Recognition", year: 2016,
            doi: "10.1109/CVPR.2016.90", tags: ["vision"], status: "read", collection: "Thesis"),
        Paper(
            title: "Highly accurate protein structure prediction with AlphaFold",
            authors: ["Jumper, John", "Evans, Richard", "Pritzel, Alexander", "Green, Tim"],
            container: "Nature", year: 2021, doi: "10.1038/s41586-021-03819-2",
            tags: ["biology"], starred: true),
        Paper(
            title:
                "Long-term exposure to ambient air pollution and cognitive function in older adults: a systematic review and meta-analysis of prospective cohort studies",
            authors: ["Sample, Alex", "Example, Jordan", "Placeholder, Casey"],
            container: "Journal of Sample Data", year: 2024, tags: ["to-read"], testOnly: true),
        Paper(
            title: "面向学术文献管理的知识图谱构建方法", authors: ["张三", "李四"],
            container: "示例学报", year: 2023, tags: ["中文"]),
        Paper(
            type: "book", title: "The Structure of Scientific Revolutions", authors: ["Kuhn, Thomas S."],
            container: "University of Chicago Press", year: 1962, status: "read"),
    ]

    private static let notes = [
        (
            "Reading list",
            """
            # Reading list

            - [[Transformers]]: start with *Attention Is All You Need*.
            - Residual networks, then [[Protein folding]].

            #to-read
            """
        ),
        (
            "Transformers",
            """
            # Transformers

            Self-attention replaces recurrence; see [[Reading list]].

            > Attention weights every token against every other one.

            | Model | Year |
            | --- | --- |
            | Transformer | 2017 |
            | ResNet | 2016 |
            """
        ),
        (
            "Protein folding",
            """
            # Protein folding

            AlphaFold predicts structures close to experimental accuracy.

            - [ ] Compare with the 2018 results
            - [x] Read the methods section
            """
        ),
    ]

    private static func fill(_ library: PaperLibrary) throws {
        var collections: [String: String] = [:]
        // Newest first in the list: the store's pictures start with the
        // paper they open, so it is added last.
        for paper in isStore ? Array(papers.reversed()) : papers where !(isStore && paper.testOnly) {
            let item = try library.addItem(
                newItem: NewItem(
                    itemType: paper.type, title: paper.title, authors: paper.authors,
                    containerTitle: paper.container, year: paper.year, doi: paper.doi))
            if !paper.tags.isEmpty { try library.setItemTags(itemId: item.id, tags: paper.tags) }
            if paper.starred { try library.setStarred(itemId: item.id, starred: true) }
            if paper.status != "unread" { try library.setReadStatus(itemId: item.id, status: paper.status) }
            if let name = paper.collection {
                let id = try collections[name] ?? library.createCollection(name: name, parentId: nil).id
                collections[name] = id
                try library.addToCollection(collectionId: id, itemId: item.id)
            }
        }
    }

    /// A scanned paper: its PDF holds a picture of the page and no text, so
    /// search finds its words ("sensing", 金刚石) only through recognition.
    private static func addScan(_ library: PaperLibrary) throws {
        #if os(iOS)
        let page = CGRect(x: 0, y: 0, width: 612, height: 792)
        let picture = UIGraphicsImageRenderer(bounds: page).image { _ in
            UIColor.white.setFill()
            UIRectFill(page)
            let lines = ["Thermometry with nanodiamonds", "Quantum sensing inside living cells", "纳米金刚石量子测温"]
            for (index, line) in lines.enumerated() {
                (line as NSString).draw(
                    at: CGPoint(x: 60, y: 90 + 60 * index),
                    withAttributes: [.font: UIFont.systemFont(ofSize: 28), .foregroundColor: UIColor.black])
            }
        }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let url = root.appendingPathComponent("Scanned Thermometry.pdf")
        try UIGraphicsPDFRenderer(bounds: page).writePDF(to: url) { context in
            context.beginPage()
            picture.draw(in: page)
        }
        let item = try library.addItem(
            newItem: NewItem(
                itemType: "journal-article", title: "Scanned Thermometry Notes", authors: ["Sample Scanner"],
                containerTitle: nil, year: 2015, doi: nil))
        let file = try library.attachFile(itemId: item.id, path: url.path(percentEncoded: false))
        try library.setAttachmentText(attachmentId: file.id, pages: PDFFirstPages.allPages(url))
        #endif
    }

    /// A page of "Attention Is All You Need" written for these pictures
    /// (not the paper's own text), attached to it with two highlights and
    /// a comment, so the reader has something to show.
    private static func addStorePDF(_ library: PaperLibrary) throws {
        guard let item = try library.items().first(where: { $0.title == "Attention Is All You Need" }) else { return }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let url = root.appendingPathComponent("Attention Is All You Need.pdf")
        let lines = try SamplePage.write(to: url)
        let file = try library.attachFile(itemId: item.id, path: url.path(percentEncoded: false))
        try library.setAttachmentText(attachmentId: file.id, pages: PDFFirstPages.allPages(url))
        let marks: [(ClosedRange<Int>, String, String?)] = [
            (SamplePage.firstHighlight, "#ffd400", nil),
            (SamplePage.secondHighlight, "#5fb236", "Compare with the residual connections in ResNet."),
        ]
        for (range, color, comment) in marks {
            let marked = lines.filter { range.contains($0.index) }
            if marked.isEmpty { continue }
            _ = try library.addAnnotation(
                annotation: NewAnnotation(
                    attachmentId: file.id, kind: "highlight", color: color, pageIndex: 0,
                    rects: marked.map(\.rect), text: marked.map(\.text).joined(separator: " "), comment: comment,
                    strokes: []))
        }
    }
}

/// One page of sample text drawn with Core Text, on iOS and macOS alike,
/// with each line's box kept for the highlights.
private enum SamplePage {
    struct Line {
        var index: Int
        var text: String
        var rect: AnnotationRect
    }

    /// Body lines (counted from the first body line) under each highlight.
    static let firstHighlight = 1...2
    static let secondHighlight = 4...5

    private static let size = CGSize(width: 612, height: 792)
    private static let margin: CGFloat = 72

    private struct Block {
        var text: String
        var size: CGFloat
        var bold = false
        /// Body text, whose lines can be highlighted.
        var body = false
    }

    private static let blocks: [Block] = [
        Block(text: "Attention Is All You Need", size: 22, bold: true),
        Block(text: "Vaswani, Shazeer, Parmar, Uszkoreit, Jones, Gomez, Kaiser, Polosukhin", size: 11),
        Block(text: "Abstract", size: 13, bold: true),
        Block(
            text: """
            Models of sequences have long been built on recurrent or convolutional networks. \
            This paper replaces both with attention alone: every position of a sequence looks at \
            every other position in one step, so a whole sentence is processed at once rather than \
            word by word, and training runs in parallel.
            """, size: 11, body: true),
        Block(text: "1  Introduction", size: 13, bold: true),
        Block(
            text: """
            Recurrent networks read a sequence one step at a time, and the distance between two \
            related words decides how hard the link between them is to learn. Self-attention \
            makes that distance constant. Stacked layers of attention and small feed-forward \
            networks, each wrapped in a residual connection, form the encoder and the decoder.
            """, size: 11, body: true),
        Block(text: "Sample page drawn for Paperloom's screenshots; the text is a summary, not the paper.", size: 9),
    ]

    /// Writes the page; returns the body text's lines.
    static func write(to url: URL) throws -> [Line] {
        var box = CGRect(origin: .zero, size: size)
        guard let context = CGContext(url as CFURL, mediaBox: &box, nil) else {
            throw CocoaError(.fileWriteUnknown)
        }
        context.beginPDFPage(nil)
        var lines: [Line] = []
        var bodyLine = 0
        // PDF space: from the bottom left.
        var y = size.height - margin
        for block in blocks {
            let fontSize = block.size
            let font = CTFontCreateWithName((block.bold ? "Helvetica-Bold" : "Times-Roman") as CFString, fontSize, nil)
            let attributes: [NSAttributedString.Key: Any] = [
                NSAttributedString.Key(kCTFontAttributeName as String): font,
                NSAttributedString.Key(kCTForegroundColorAttributeName as String): CGColor(gray: 0.1, alpha: 1),
            ]
            let string = NSAttributedString(string: block.text, attributes: attributes)
            let typesetter = CTTypesetterCreateWithAttributedString(string)
            let width = Double(size.width - 2 * margin)
            let leading = fontSize * 1.45
            var start = 0
            while start < string.length {
                let count = CTTypesetterSuggestLineBreak(typesetter, start, width)
                let line = CTTypesetterCreateLine(typesetter, CFRange(location: start, length: count))
                y -= leading
                context.textPosition = CGPoint(x: margin, y: y)
                CTLineDraw(line, context)
                if block.body {
                    var ascent: CGFloat = 0
                    var descent: CGFloat = 0
                    let lineWidth = CTLineGetTypographicBounds(line, &ascent, &descent, nil)
                    let piece = (string.string as NSString).substring(with: NSRange(location: start, length: count))
                    lines.append(
                        Line(
                            index: bodyLine, text: piece.trimmingCharacters(in: .whitespaces),
                            rect: AnnotationRect(
                                x: Double(margin), y: Double(y - descent), width: lineWidth,
                                height: Double(ascent + descent))))
                    bodyLine += 1
                }
                start += count
            }
            y -= fontSize * 0.9
        }
        context.endPDFPage()
        context.closePDF()
        return lines
    }
}
