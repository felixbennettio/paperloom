import CoreKit
import SwiftUI

/// The online search's state: database, words, results and the ones picked.
@MainActor
@Observable
final class OnlineSearchModel {
    let sources = searchSources()
    var sourceID = "crossref"
    var text = ""
    private(set) var hits: [SearchHit] = []
    private(set) var total: UInt64?
    private(set) var hasMore = false
    private(set) var isSearching = false
    /// The words the results are for; nil before the first search.
    private(set) var searchedText: String?
    private var page: UInt32 = 0
    private(set) var chosen = Set<String>()
    var expanded: String?
    var message: String?
    /// The catalogues the person added, searched in their own way.
    @ObservationIgnored var catalogues: Catalogues?

    func search(in library: LibraryModel) async {
        let words = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !words.isEmpty else { return }
        await run(in: library, text: words, page: 0)
    }

    /// Searches the same words again, in the database just picked.
    func repeatSearch(in library: LibraryModel) async {
        guard let searchedText else { return }
        await run(in: library, text: searchedText, page: 0)
    }

    func loadMore(in library: LibraryModel) async {
        guard let searchedText, hasMore else { return }
        await run(in: library, text: searchedText, page: page + 1)
    }

    private func run(in library: LibraryModel, text: String, page: UInt32) async {
        guard !isSearching else { return }
        isSearching = true
        defer { isSearching = false }
        do {
            let result: SearchPage
            if let catalogue = catalogues?.catalogue(id: sourceID) {
                result = try await library.searchCatalogue(catalogue, text: text, page: page)
            } else {
                result = try await library.searchOnline(sourceID: sourceID, text: text, page: page)
            }
            if page == 0 {
                hits = result.hits
                chosen = []
                expanded = nil
            } else {
                let shown = Set(hits.map(\.key))
                hits += result.hits.filter { !shown.contains($0.key) }
            }
            total = result.total
            hasMore = result.hasMore
            self.page = page
            searchedText = text
        } catch {
            message = CoreKit.message(for: error)
        }
    }

    /// Shows works found elsewhere (read from a page in the browser), for
    /// picking and adding like results.
    func show(_ found: [SearchHit], from place: String) {
        hits = found
        total = UInt64(found.count)
        hasMore = false
        searchedText = place
        page = 0
        chosen = []
        expanded = nil
    }

    func toggle(_ key: String) {
        if chosen.contains(key) {
            chosen.remove(key)
        } else {
            chosen.insert(key)
        }
    }

    func add(_ keys: [String], to library: LibraryModel) {
        guard let report = library.addSearchResults(keys: keys) else { return }
        // Results that could be added each got the next new ID; with
        // duplicates among them the IDs cannot be matched up, so the results
        // are only marked.
        let ids = report.addedIds.count == keys.count ? report.addedIds : keys.map { _ in "" }
        for (key, id) in zip(keys, ids) {
            if let index = hits.firstIndex(where: { $0.key == key }), hits[index].libraryItemId == nil {
                hits[index].libraryItemId = id
            }
        }
        chosen.subtract(keys)
        if report.duplicates > 0 {
            let (a, d, s) = (String(report.added), String(report.duplicates), String(report.skipped))
            message = String(localized: "Added \(a), \(d) already in the library, \(s) skipped.")
        }
    }
}

/// Searches the open scholarly databases and adds the results a person
/// picks to the library.
struct OnlineSearchView: View {
    @Environment(LibraryModel.self) private var library
    @Environment(DatabaseKeys.self) private var keys
    @Environment(Catalogues.self) private var catalogues
    @State private var model = OnlineSearchModel()

    /// The open databases, those a key is kept for, and the catalogues the
    /// person added.
    private var sources: [SearchSource] {
        model.sources.filter { !$0.needsKey || keys.known.contains($0.id) } + catalogues.sources
    }

    /// The names of the databases still waiting for a key.
    private var missingKeys: String {
        model.sources.filter { $0.needsKey && !keys.known.contains($0.id) }.map(\.name).joined(separator: ", ")
    }

    var body: some View {
        @Bindable var model = model
        content
            // The databases stay at the top, whatever the content's height.
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .navigationTitle("Online Search")
            .searchable(text: $model.text, prompt: Text("Title, authors, keywords"))
            .onSubmit(of: .search) {
                Task { await model.search(in: library) }
            }
            .onChange(of: model.sourceID) {
                Task { await model.repeatSearch(in: library) }
            }
            .onAppear { model.catalogues = catalogues }
            .safeAreaInset(edge: .top, spacing: 0) { sourceBar }
            .safeAreaInset(edge: .bottom) { addBar }
            .toolbar {
                if model.isSearching {
                    ToolbarItem {
                        ProgressView().controlSize(.small)
                    }
                }
            }
            .alert(
                "Online Search",
                isPresented: Binding(get: { model.message != nil }, set: { if !$0 { model.message = nil } })
            ) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(model.message ?? "")
            }
    }

    @ViewBuilder
    private var content: some View {
        if let searched = model.searchedText {
            if model.hits.isEmpty {
                ContentUnavailableView.search(text: searched)
            } else {
                List {
                    Section {
                        ForEach(model.hits) { hit in
                            SearchHitRow(hit: hit, model: model)
                        }
                        if model.hasMore {
                            Button("Load More") {
                                Task { await model.loadMore(in: library) }
                            }
                            .disabled(model.isSearching)
                            .frame(maxWidth: .infinity)
                        }
                    } header: {
                        if let total = model.total {
                            Text("Results: \(total.formatted())")
                        }
                    }
                }
            }
        } else if model.isSearching {
            ProgressView()
        } else {
            ContentUnavailableView {
                Label("Online Search", systemImage: "magnifyingglass")
            } description: {
                Text("Search Crossref, OpenAlex, arXiv, PubMed and Semantic Scholar.")
            } actions: {
                if !missingKeys.isEmpty {
                    Text("Add an API key in Settings to search \(missingKeys) too.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    /// The databases, as Liquid Glass chips, in as many rows as they need.
    private var sourceBar: some View {
        GlassEffectContainer(spacing: 8) {
            FlowLayout(spacing: 8, lineSpacing: 8) {
                ForEach(sources) { source in
                    let selected = source.id == model.sourceID
                    Button {
                        model.sourceID = source.id
                    } label: {
                        Text(source.name)
                            .font(.callout.weight(selected ? .semibold : .regular))
                            .padding(.horizontal, 12)
                            .padding(.vertical, 6)
                    }
                    .buttonStyle(.plain)
                    .glassEffect(
                        selected ? .regular.tint(Color.accentColor.opacity(0.3)).interactive() : .regular.interactive(),
                        in: .capsule)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
        }
    }

    @ViewBuilder
    private var addBar: some View {
        if !model.chosen.isEmpty {
            Button {
                model.add(Array(model.chosen), to: library)
            } label: {
                Label("Add \(String(model.chosen.count)) Selected", systemImage: "plus")
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
            }
            .buttonStyle(.glassProminent)
            .padding(.bottom, 8)
        }
    }
}

/// One search result: pick it, add it, or read its abstract.
struct SearchHitRow: View {
    let hit: SearchHit
    let model: OnlineSearchModel
    @Environment(LibraryModel.self) private var library
    @Environment(TabsModel.self) private var tabs
    @Environment(\.openURL) private var openURL

    private var inLibrary: Bool { hit.libraryItemId != nil }
    private var isChosen: Bool { model.chosen.contains(hit.key) }
    private var isExpanded: Bool { model.expanded == hit.key }

    var body: some View {
        let detail = hit.detail
        HStack(alignment: .top, spacing: 12) {
            Button {
                model.toggle(hit.key)
            } label: {
                Image(systemName: inLibrary || isChosen ? "checkmark.circle.fill" : "circle")
                    .font(.title3)
                    .foregroundStyle(inLibrary ? Color.green : isChosen ? Color.accentColor : Color.secondary)
            }
            .buttonStyle(.plain)
            .disabled(inLibrary)

            VStack(alignment: .leading, spacing: 4) {
                Text(detail.title)
                    .font(.headline)
                    .lineLimit(isExpanded ? nil : 3)
                if !detail.authors.isEmpty {
                    Text(authorLine(detail.authors))
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(isExpanded ? nil : 1)
                }
                Text(facts(detail))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                HStack(spacing: 10) {
                    if inLibrary {
                        Label("In Library", systemImage: "checkmark.seal.fill")
                            .foregroundStyle(.green)
                    }
                    if let cited = hit.citedBy {
                        Text("Cited by \(String(cited))")
                    }
                    if hit.pdfUrl != nil {
                        Label {
                            Text(verbatim: "PDF")
                        } icon: {
                            Image(systemName: "doc.richtext")
                        }
                    }
                }
                .font(.caption)
                .foregroundStyle(.secondary)
                if isExpanded, let abstract = detail.abstractNote {
                    Text(abstract)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                        .padding(.top, 4)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
            .onTapGesture {
                withAnimation(.smooth) { model.expanded = isExpanded ? nil : hit.key }
            }

            if !inLibrary {
                Button {
                    model.add([hit.key], to: library)
                } label: {
                    Label("Add", systemImage: "plus")
                        .labelStyle(.iconOnly)
                }
                .buttonStyle(.glass)
            }
        }
        .padding(.vertical, 4)
        .contextMenu {
            if let pdf = hit.pdfUrl, let url = URL(string: pdf) {
                Button("Open PDF", systemImage: "doc.richtext") { openURL(url) }
            }
            if let page = detail.url, let url = URL(string: page) {
                Button("Open Web Page", systemImage: "safari") { openURL(url) }
            }
            #if os(macOS)
            if let id = hit.libraryItemId, !id.isEmpty {
                Button("Show in Library", systemImage: "books.vertical") {
                    tabs.open(.item(id: id, title: detail.title))
                }
            }
            #endif
        }
    }

    /// `Vaswani, Shazeer, Parmar et al.`
    private func authorLine(_ authors: [String]) -> String {
        authors.count > 3 ? authors.prefix(3).joined(separator: ", ") + " et al." : authors.joined(separator: ", ")
    }

    private func facts(_ detail: ItemDetail) -> String {
        [detail.containerTitle, detail.year.map { String($0) }, String(localized: ItemTypeName.title(for: detail.itemType))]
            .compactMap { $0 }
            .filter { !$0.isEmpty }
            .joined(separator: " · ")
    }
}
