import CoreKit
import SwiftUI

/// Searches the whole library at once: items, PDF text, annotations and
/// notes. Results open where the words are.
struct FullTextSearchView: View {
    @Environment(LibraryModel.self) private var library
    @Environment(VaultModel.self) private var vault
    @Environment(TabsModel.self) private var tabs
    @Environment(AIAssistant.self) private var ai
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    @State private var query = ""
    @State private var kinds = Set<String>()
    @State private var hits: [FullTextHit]?
    @State private var searching = false
    @State private var openedItem: String?
    @State private var openedPDF: FullTextHit?
    @State private var openedNote = false
    @State private var answer: AiLibraryAnswer?
    @State private var asking = false
    @State private var askProblem: String?

    private static let kindNames: [(id: String, title: LocalizedStringKey, image: String)] = [
        ("item", "Items", "books.vertical"),
        ("pdf", "PDF Text", "doc.richtext"),
        ("annotation", "Annotations", "highlighter"),
        ("note", "Notes", "note.text"),
    ]

    var body: some View {
        content
            // The filters stay at the top, whatever the content's height.
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .navigationTitle("Search Everything")
            .searchable(text: $query, prompt: Text("Words in titles, PDFs, annotations and notes"))
            .onSubmit(of: .search) { Task { await run() } }
            .safeAreaInset(edge: .top, spacing: 0) {
                VStack(spacing: 0) {
                    filters
                    if let askProblem {
                        Text(verbatim: askProblem).font(.caption).foregroundStyle(.red).padding(.horizontal, 16)
                    }
                    if let answer {
                        libraryAnswer(answer)
                    }
                }
            }
            .navigationDestination(item: $openedItem) { id in ItemDetailView(itemID: id) }
            .navigationDestination(item: $openedPDF) { hit in
                if let attachment = library.attachments(itemID: hit.itemId ?? "").first(where: { $0.id == hit.attachmentId }) {
                    PDFReaderView(
                        url: URL(fileURLWithPath: attachment.filePath), attachmentID: attachment.id, title: hit.title,
                        page: hit.pageIndex.map { Int($0) })
                }
            }
            .navigationDestination(isPresented: $openedNote) { NoteView() }
    }

    @ViewBuilder
    private var content: some View {
        if let hits {
            if hits.isEmpty {
                ContentUnavailableView.search(text: query)
            } else {
                List(Array(hits.enumerated()), id: \.offset) { _, hit in
                    Button {
                        open(hit)
                    } label: {
                        HitRow(hit: hit, image: Self.kindNames.first { $0.id == hit.kind }?.image ?? "doc")
                    }
                    .buttonStyle(.plain)
                }
            }
        } else if searching {
            ProgressView()
        } else {
            ContentUnavailableView {
                Label("Search Everything", systemImage: "magnifyingglass")
            } description: {
                Text("Find words anywhere: in titles and abstracts, inside PDFs, in your annotations and notes.")
            }
        }
    }

    private var filters: some View {
        GlassEffectContainer(spacing: 8) {
            FlowLayout(spacing: 8, lineSpacing: 8) {
                ForEach(Self.kindNames, id: \.id) { kind in
                    let on = kinds.contains(kind.id)
                    Button {
                        if on { kinds.remove(kind.id) } else { kinds.insert(kind.id) }
                        Task { await run() }
                    } label: {
                        Label(kind.title, systemImage: kind.image)
                            .font(.callout.weight(on ? .semibold : .regular))
                            .padding(.horizontal, 12)
                            .padding(.vertical, 6)
                    }
                    .buttonStyle(.plain)
                    .glassEffect(
                        on ? .regular.tint(Color.accentColor.opacity(0.3)).interactive() : .regular.interactive(),
                        in: .capsule)
                }
                Button {
                    Task { await ask() }
                } label: {
                    Label(
                        asking ? LocalizedStringKey("Asking…") : LocalizedStringKey("Ask the Library"),
                        systemImage: "sparkles")
                        .font(.callout)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 6)
                }
                .buttonStyle(.plain)
                .glassEffect(.regular.interactive(), in: .capsule)
                .disabled(asking || query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
        }
    }

    /// The AI provider's answer from the whole library, and its sources.
    private func libraryAnswer(_ answer: AiLibraryAnswer) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                AIAnswerView(answer: AiAnswer(text: answer.text, cutShort: answer.cutShort, pages: []))
                Text("Sources").font(.headline)
                ForEach(answer.sources, id: \.number) { source in
                    Button {
                        open(FullTextHit(
                            kind: source.kind, title: source.title, snippetHtml: "", itemId: source.itemId,
                            attachmentId: source.attachmentId, pageIndex: source.pageIndex, annotationId: nil,
                            notePath: source.notePath))
                    } label: {
                        Text(verbatim: "[\(source.number)] \(source.title)")
                            .multilineTextAlignment(.leading)
                    }
                    .buttonStyle(.borderless)
                }
            }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(maxHeight: 360)
        .glassEffect(.regular, in: .rect(cornerRadius: 16))
        .padding(.horizontal, 16)
        .padding(.bottom, 8)
    }

    private func ask() async {
        let question = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !question.isEmpty, let core = library.core else { return }
        asking = true
        askProblem = nil
        let provider = ai.provider
        let language = AIAssistant.language
        do {
            answer = try await Task.detached {
                try core.aiAskLibrary(provider: provider, question: question, language: language)
            }.value
        } catch {
            askProblem = CoreKit.message(for: error)
        }
        asking = false
    }

    private func run() async {
        let words = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !words.isEmpty else { return }
        searching = true
        hits = await library.searchEverything(words, kinds: Array(kinds))
        searching = false
    }

    private var phoneLayout: Bool {
        #if os(iOS)
        sizeClass == .compact
        #else
        false
        #endif
    }

    private func open(_ hit: FullTextHit) {
        switch hit.kind {
        case "note":
            guard let path = hit.notePath else { return }
            vault.open(path)
            if phoneLayout { openedNote = true } else { tabs.open(.notes) }
        case "pdf", "annotation":
            guard let attachmentID = hit.attachmentId, let itemID = hit.itemId,
                  let file = library.attachments(itemID: itemID).first(where: { $0.id == attachmentID })
            else { return }
            if phoneLayout {
                openedPDF = hit
            } else {
                tabs.open(.pdf(
                    attachmentID: attachmentID, path: file.filePath, title: hit.title,
                    page: hit.pageIndex.map { Int($0) }))
            }
        default:
            guard let itemID = hit.itemId else { return }
            if phoneLayout { openedItem = itemID } else { tabs.open(.item(id: itemID, title: hit.title)) }
        }
    }
}

/// A result: its title, where it was found, and the matching words in bold.
private struct HitRow: View {
    let hit: FullTextHit
    let image: String

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: image).foregroundStyle(.secondary).frame(width: 20)
            VStack(alignment: .leading, spacing: 4) {
                Text(hit.title.isEmpty ? String(localized: "Untitled") : hit.title)
                    .font(.headline)
                    .lineLimit(2)
                if let page = hit.pageIndex {
                    Text("Page \(String(page + 1))").font(.caption).foregroundStyle(.secondary)
                } else if let path = hit.notePath {
                    Text(verbatim: path).font(.caption).foregroundStyle(.secondary)
                }
                if !hit.snippetHtml.isEmpty {
                    Text(Self.snippet(hit.snippetHtml))
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .lineLimit(4)
                }
            }
        }
        .padding(.vertical, 4)
    }

    /// The snippet's `<b>` words in bold; other markup dropped.
    static func snippet(_ html: String) -> AttributedString {
        var result = AttributedString()
        var rest = Substring(html)
        var bold = false
        while !rest.isEmpty {
            if rest.hasPrefix("<b>") {
                bold = true
                rest = rest.dropFirst(3)
            } else if rest.hasPrefix("</b>") {
                bold = false
                rest = rest.dropFirst(4)
            } else {
                let next = rest.firstIndex(of: "<").map { $0 == rest.startIndex ? rest.index(after: $0) : $0 } ?? rest.endIndex
                var piece = AttributedString(Self.unescape(String(rest[..<next])))
                if bold { piece.inlinePresentationIntent = .stronglyEmphasized }
                result += piece
                rest = rest[next...]
            }
        }
        return result
    }

    private static func unescape(_ text: String) -> String {
        text.replacingOccurrences(of: "&lt;", with: "<").replacingOccurrences(of: "&gt;", with: ">")
            .replacingOccurrences(of: "&quot;", with: "\"").replacingOccurrences(of: "&#39;", with: "'")
            .replacingOccurrences(of: "&amp;", with: "&")
    }
}
