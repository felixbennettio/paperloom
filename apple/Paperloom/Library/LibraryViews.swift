import CoreKit
import SwiftUI

/// The workbench sidebar: the library's views, the collection tree, and
/// tags.
struct CollectionsSidebar: View {
    @Environment(LibraryModel.self) private var library
    @Environment(TabsModel.self) private var tabs
    @State private var creatingCollection = false
    @State private var newCollectionName = ""
    @State private var renamingTag: String?
    @State private var newTagName = ""
    @State private var editingSearch: SavedSearch?
    @State private var addingFeed = false
    @State private var renamingFeed: String?
    @State private var feedName = ""

    private static let allItems = "all-items"
    private static let starred = "starred"
    private static let unread = "unread"
    private static let trash = "trash"
    private static let duplicates = "duplicates"
    private static let onlineSearch = "online-search"
    private static let searchEverything = "search-everything"
    private static let browser = "browser"
    private static let notes = "notes"
    private static let tagPrefix = "tag:"
    private static let searchPrefix = "search:"
    private static let feedPrefix = "feed:"

    var body: some View {
        List(selection: selection) {
            Section("Library") {
                Label("All Items", systemImage: "books.vertical").tag(Self.allItems)
                Label("Starred", systemImage: "star").tag(Self.starred)
                Label("Unread", systemImage: "circle").tag(Self.unread)
                Label("Search Everything", systemImage: "magnifyingglass").tag(Self.searchEverything)
                Label("Online Search", systemImage: "globe").tag(Self.onlineSearch)
                Label("Browser", systemImage: "safari").tag(Self.browser)
                Label("Notes", systemImage: "note.text").tag(Self.notes)
                Label("Duplicate Items", systemImage: "square.on.square").tag(Self.duplicates)
                Label("Trash", systemImage: "trash").tag(Self.trash)
            }
            Section("Collections") {
                OutlineGroup(library.collectionTree, children: \.children) { node in
                    Label(node.info.name, systemImage: "folder")
                        .tag(node.id)
                }
            }
            Section {
                ForEach(library.savedSearches) { search in
                    Label(search.name, systemImage: "folder.badge.gearshape")
                        .tag(Self.searchPrefix + search.id)
                        .contextMenu {
                            Button("Edit Saved Search", systemImage: "slider.horizontal.3") { editingSearch = search }
                            Button("Delete Saved Search", systemImage: "trash", role: .destructive) {
                                library.deleteSavedSearch(id: search.id)
                            }
                        }
                }
            } header: {
                HStack {
                    Text("Saved Searches")
                    Spacer()
                    Button {
                        editingSearch = library.newSearch()
                    } label: {
                        Image(systemName: "plus")
                    }
                    .buttonStyle(.borderless)
                    .help("New Saved Search")
                    .accessibilityLabel(Text("New Saved Search"))
                }
            }
            Section {
                ForEach(library.feeds) { feed in
                    Label(feed.title, systemImage: "dot.radiowaves.up.forward")
                        .badge(Int(feed.unread))
                        .tag(Self.feedPrefix + feed.id)
                        .help(feed.problem ?? feed.url)
                        .contextMenu {
                            Button("Rename…", systemImage: "pencil") {
                                feedName = feed.title
                                renamingFeed = feed.id
                            }
                            Button("Unsubscribe", systemImage: "minus.circle", role: .destructive) {
                                library.removeFeed(feed.id)
                            }
                        }
                }
            } header: {
                HStack {
                    Text("Feeds")
                    Spacer()
                    Button {
                        addingFeed = true
                    } label: {
                        Image(systemName: "plus")
                    }
                    .buttonStyle(.borderless)
                    .help("Add Feed")
                    .accessibilityLabel(Text("Add Feed"))
                }
            }
            if !library.tags.isEmpty {
                Section("Tags") {
                    ForEach(library.tags, id: \.name) { tag in
                        Label {
                            Text(tag.name)
                        } icon: {
                            TagDot(color: tag.color)
                        }
                        .badge(Int(tag.count))
                        .tag(Self.tagPrefix + tag.name)
                        .contextMenu { tagMenu(tag) }
                    }
                }
            }
        }
        .navigationTitle("Paperloom")
        .toolbar {
            ToolbarItem {
                Button {
                    newCollectionName = ""
                    creatingCollection = true
                } label: {
                    Label("New Collection", systemImage: "folder.badge.plus")
                }
            }
        }
        .sheet(item: $editingSearch) { search in
            SavedSearchEditor(search: search)
        }
        .modifier(AddFeedAlert(isPresented: $addingFeed))
        .alert(
            "Rename", isPresented: Binding(get: { renamingFeed != nil }, set: { if !$0 { renamingFeed = nil } })
        ) {
            TextField("Name", text: $feedName)
            Button("Rename") {
                if let id = renamingFeed { library.renameFeed(id, to: feedName) }
            }
            Button("Cancel", role: .cancel) {}
        }
        .task { await library.refreshFeeds() }
        .alert("New Collection", isPresented: $creatingCollection) {
            TextField("Name", text: $newCollectionName)
            Button("Create") {
                library.createCollection(named: newCollectionName, parentID: library.selectedCollectionID)
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            if let parent = library.selectedCollection {
                Text("Inside “\(parent.name)”.")
            } else {
                Text("At the top level.")
            }
        }
        .alert(
            "Rename Tag",
            isPresented: Binding(get: { renamingTag != nil }, set: { if !$0 { renamingTag = nil } })
        ) {
            TextField("Name", text: $newTagName)
            Button("Rename") {
                if let tag = renamingTag { library.renameTag(tag, to: newTagName) }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Renaming onto an existing tag merges the two.")
        }
    }

    @ViewBuilder
    private func tagMenu(_ tag: TagInfo) -> some View {
        Menu("Color") {
            Button("None") { library.setTagColor(nil, name: tag.name) }
            ForEach(MarkColor.allCases) { color in
                Button {
                    library.setTagColor(color.hex, name: tag.name)
                } label: {
                    Label {
                        Text(color.title)
                    } icon: {
                        colorDot(PlatformColor(hex: color.hex) ?? .systemYellow)
                    }
                }
            }
        }
        Button("Rename…") {
            newTagName = tag.name
            renamingTag = tag.name
        }
        Button("Delete Tag", role: .destructive) { library.deleteTag(tag.name) }
    }

    private var selection: Binding<String?> {
        Binding(
            get: {
                if case .search = tabs.selected { return Self.onlineSearch }
                if case .find = tabs.selected { return Self.searchEverything }
                if case .browser = tabs.selected { return Self.browser }
                if case .notes = tabs.selected { return Self.notes }
                switch library.scope {
                case .all: return Self.allItems
                case .starred: return Self.starred
                case .unread: return Self.unread
                case .trash: return Self.trash
                case .duplicates: return Self.duplicates
                case .savedSearch(let id): return Self.searchPrefix + id
                case .feed(let id): return Self.feedPrefix + id
                case .collection(let id): return id
                case .tag(let name): return Self.tagPrefix + name
                }
            },
            set: { value in
                guard let value else { return }
                switch value {
                case Self.onlineSearch:
                    tabs.open(.search)
                    return
                case Self.searchEverything:
                    tabs.open(.find)
                    return
                case Self.browser:
                    tabs.open(.browser)
                    return
                case Self.notes:
                    tabs.open(.notes)
                    return
                case Self.allItems: library.scope = .all
                case Self.starred: library.scope = .starred
                case Self.unread: library.scope = .unread
                case Self.trash: library.scope = .trash
                case Self.duplicates: library.scope = .duplicates
                case _ where value.hasPrefix(Self.searchPrefix):
                    library.scope = .savedSearch(String(value.dropFirst(Self.searchPrefix.count)))
                case _ where value.hasPrefix(Self.feedPrefix):
                    library.scope = .feed(String(value.dropFirst(Self.feedPrefix.count)))
                default:
                    library.scope =
                        value.hasPrefix(Self.tagPrefix)
                        ? .tag(String(value.dropFirst(Self.tagPrefix.count))) : .collection(value)
                }
                tabs.open(.library)
            })
    }
}

/// One line in an item list.
struct ItemRow: View {
    let item: ItemSummary
    var columns: [PluginColumn] = []
    var columnValues: [String: [String: String]] = [:]
    @Environment(LibraryModel.self) private var library

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(item.title)
                    .font(.headline)
                    .lineLimit(2)
                Spacer(minLength: 4)
                if item.starred {
                    Image(systemName: "star.fill").foregroundStyle(.yellow).font(.caption)
                }
                if item.hasPdf {
                    Image(systemName: "doc.richtext").foregroundStyle(.secondary).font(.caption)
                }
            }
            if !item.tags.isEmpty {
                HStack(spacing: 8) {
                    ForEach(item.tags.prefix(5), id: \.self) { tag in
                        HStack(spacing: 3) {
                            TagDot(color: library.color(ofTag: tag))
                            Text(tag)
                        }
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    }
                }
            }
            Text(details)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(1)
            if let doi = item.doi {
                Text(doi)
                    .font(.caption.monospaced())
                    .foregroundStyle(.tertiary)
            }
            if let cited = item.citedBy {
                Text("Cited by \(String(cited))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            ForEach(columns) { column in
                if let value = columnValues[column.id]?[item.id], !value.isEmpty {
                    Text(verbatim: "\(column.label): \(value)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }
        }
        .padding(.vertical, 4)
    }

    private var details: String {
        [item.authors, item.year.map { String($0) }, item.containerTitle]
            .compactMap { $0 }
            .filter { !$0.isEmpty }
            .joined(separator: " · ")
    }
}

/// Prefetches a row's plugin menu before its context menu opens.
@MainActor
struct ItemContextMenu: ViewModifier {
    let item: ItemSummary
    var onRemoved: () -> Void = {}
    @Environment(LibraryModel.self) private var library
    @Environment(PluginHost.self) private var plugins
    @State private var pluginMenus: [PluginItemMenu] = []

    func body(content: Content) -> some View {
        content
            .contextMenu {
                ItemMenu(item: item, pluginMenus: pluginMenus, onRemoved: onRemoved)
                    .onAppear { plugins.selectionChanged(itemKeys: [item.id]) }
            }
            .task(id: [[item.id], [String(library.revision)], zoteroPlugins]) {
                pluginMenus = []
                guard !zoteroPlugins.isEmpty else { return }
                plugins.itemsChanged(revision: library.revision)
                await plugins.itemsSettled()
                let menus = await plugins.itemMenu(itemKeys: [item.id])
                guard !Task.isCancelled else { return }
                pluginMenus = menus
            }
    }

    private var zoteroPlugins: [String] {
        plugins.running.filter { $0.ready && plugins.runner($0.id)?.plugin.zotero == true }.map(\.id).sorted()
    }
}

/// The items of the selected view; opens an item in its own tab.
struct ItemListView: View {
    @Environment(LibraryModel.self) private var library
    @Environment(TabsModel.self) private var tabs
    @Environment(PluginHost.self) private var plugins
    @State private var search = ""
    @State private var confirmingEmptyTrash = false
    @State private var editingSearch: SavedSearch?
    @State private var countNotice: String?
    @State private var columnValues: [String: [String: String]] = [:]
    @AppStorage("itemSort") private var sort = ItemSort.dateAdded

    var body: some View {
        Group {
            if case .feed(let id) = library.scope {
                List {
                    FeedEntries(feedID: id) { itemID, title in
                        plugins.selectionChanged(itemKeys: [itemID])
                        tabs.open(.item(id: itemID, title: title))
                    }
                }
            } else if library.scope == .duplicates {
                List {
                    DuplicatesView { item in
                        plugins.selectionChanged(itemKeys: [item.id])
                        tabs.open(.item(id: item.id, title: item.title))
                    }
                }
            } else if library.items.isEmpty {
                if library.scope == .trash {
                    ContentUnavailableView("Trash Is Empty", systemImage: "trash")
                } else if case .savedSearch = library.scope {
                    ContentUnavailableView("No matches", systemImage: "magnifyingglass")
                } else {
                    ContentUnavailableView(
                        "No Items Yet", systemImage: "books.vertical",
                        description: Text("Add a paper with the + button: paste its DOI or fill in the details."))
                }
            } else if filtered.isEmpty {
                ContentUnavailableView.search(text: search)
            } else {
                List(filtered) { item in
                    Button {
                        plugins.selectionChanged(itemKeys: [item.id])
                        tabs.open(.item(id: item.id, title: item.title))
                    } label: {
                        ItemRow(item: item, columns: plugins.columns, columnValues: columnValues)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .modifier(ItemContextMenu(item: item) { tabs.close(.item(id: item.id, title: item.title)) })
                }
                .task(id: [filtered.map(\.id), plugins.columns.flatMap { [$0.id, $0.label] }, [String(library.revision)]]) {
                    columnValues = [:]
                    let items = filtered.map(\.id)
                    let columns = plugins.columns
                    guard !items.isEmpty, !columns.isEmpty else { return }
                    plugins.itemsChanged(revision: library.revision)
                    await plugins.itemsSettled()
                    var values: [String: [String: String]] = [:]
                    for column in columns {
                        let result = await plugins.columnValues(column, items: items)
                        guard !Task.isCancelled else { return }
                        values[column.id] = result
                    }
                    columnValues = values
                }
            }
        }
        .searchable(text: $search, prompt: Text("Titles, authors, DOIs"))
        .navigationTitle(library.scopeTitle)
        .toolbar {
            ToolbarItem {
                Menu {
                    Picker("Sort By", selection: $sort) {
                        ForEach(ItemSort.allCases) { order in
                            Text(order.title).tag(order)
                        }
                    }
                    Divider()
                    Button("Count Citations", systemImage: "quote.bubble") {
                        let ids = library.items.map(\.id)
                        Task { countNotice = await library.countCitations(ids) }
                    }
                    .disabled(library.items.isEmpty)
                } label: {
                    Label("Sort By", systemImage: "arrow.up.arrow.down")
                }
            }
            if library.scope == .trash && !library.items.isEmpty {
                ToolbarItem {
                    Button("Empty Trash", systemImage: "trash.slash", role: .destructive) {
                        confirmingEmptyTrash = true
                    }
                }
            }
            if let current = currentSearch {
                ToolbarItem {
                    Button("Edit Saved Search", systemImage: "slider.horizontal.3") { editingSearch = current }
                }
            }
        }
        .sheet(item: $editingSearch) { search in
            SavedSearchEditor(search: search)
        }
        .alert(
            "Count Citations", isPresented: Binding(get: { countNotice != nil }, set: { if !$0 { countNotice = nil } })
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(countNotice ?? "")
        }
        .confirmationDialog("Empty the trash?", isPresented: $confirmingEmptyTrash) {
            Button("Empty Trash", role: .destructive) { library.emptyTrash() }
        } message: {
            Text("Items in the trash, with their files and annotations, are deleted for good.")
        }
        .dropDestination(for: URL.self) { urls, _ in
            let pdfs = urls.filter { $0.pathExtension.lowercased() == "pdf" }
            guard !pdfs.isEmpty else { return false }
            Task { await library.importPDFs(pdfs) }
            return true
        }
    }

    /// The saved search the list shows, if it shows one.
    private var currentSearch: SavedSearch? {
        guard case .savedSearch(let id) = library.scope else { return nil }
        return library.savedSearches.first { $0.id == id }
    }

    private var filtered: [ItemSummary] {
        let query = search.trimmingCharacters(in: .whitespaces)
        let items = sort.sorted(library.items)
        guard !query.isEmpty else { return items }
        return items.filter { item in
            item.title.localizedCaseInsensitiveContains(query)
                || item.authors.localizedCaseInsensitiveContains(query)
                || (item.doi?.localizedCaseInsensitiveContains(query) ?? false)
                || item.tags.contains { $0.localizedCaseInsensitiveContains(query) }
        }
    }
}

/// An item's details, tags, files and citation.
struct ItemDetailView: View {
    let itemID: String
    @Environment(LibraryModel.self) private var library
    @Environment(VaultModel.self) private var vault
    @Environment(TabsModel.self) private var tabs
    @Environment(\.openURL) private var openURL
    @Environment(AIAssistant.self) private var ai
    @Environment(PluginHost.self) private var plugins
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    @State private var editing: ItemDetail?
    @State private var showingNote = false
    @State private var summary: AiAnswer?
    @State private var summarizing = false
    @State private var drafting = false
    @State private var summaryError: String?

    var body: some View {
        let _ = library.revision
        if let item = library.detail(id: itemID) {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    Text(ItemTypeName.title(for: item.itemType))
                        .font(.caption.weight(.semibold))
                        .textCase(.uppercase)
                        .foregroundStyle(.secondary)
                    Text(item.title)
                        .font(.largeTitle.bold())
                        .textSelection(.enabled)
                    if !item.authors.isEmpty {
                        Text(item.authors.joined(separator: ", "))
                            .font(.title3)
                            .foregroundStyle(.secondary)
                            .textSelection(.enabled)
                    }
                    // Wraps in a narrow pane instead of cutting the buttons short.
                    FlowLayout(spacing: 10, lineSpacing: 8) {
                        Button {
                            library.setStarred(!item.starred, id: itemID)
                        } label: {
                            Image(systemName: item.starred ? "star.fill" : "star")
                                .foregroundStyle(item.starred ? Color.yellow : Color.secondary)
                        }
                        .buttonStyle(.glass)
                        .accessibilityLabel(item.starred ? Text("Remove Star") : Text("Add Star"))
                        Picker(
                            "Reading Status",
                            selection: Binding(get: { item.readStatus }, set: { library.setReadStatus($0, id: itemID) })
                        ) {
                            ForEach(ReadStatusName.all, id: \.self) { status in
                                Label(ReadStatusName.title(status), systemImage: ReadStatusName.systemImage(status))
                                    .tag(status)
                            }
                        }
                        .pickerStyle(.menu)
                        .labelsHidden()
                        .fixedSize()
                        Button("Edit", systemImage: "pencil") { editing = item }
                            .buttonStyle(.glass)
                        Button("Literature Note", systemImage: "note.text.badge.plus") {
                            vault.createLiteratureNote(itemID: itemID)
                            showNote()
                        }
                        .buttonStyle(.glass)
                        Button("Copy for Notes", systemImage: "link") {
                            if let link = library.itemLink(id: itemID) { Clipboard.copy(link) }
                        }
                        .buttonStyle(.glass)
                        .labelStyle(.iconOnly)
                    }
                    TagEditor(itemID: itemID, tags: item.tags)
                    GlassEffectContainer(spacing: 10) {
                        FlowLayout(spacing: 10, lineSpacing: 8) {
                            if let year = item.year {
                                fact(String(year), systemImage: "calendar")
                            }
                            if let journal = item.containerTitle {
                                fact(journal, systemImage: "book.closed")
                            }
                            if let doi = item.doi, let url = URL(string: "https://doi.org/\(doi)") {
                                Button {
                                    openURL(url)
                                } label: {
                                    Label(doi, systemImage: "link")
                                        .font(.callout)
                                        .lineLimit(1)
                                }
                                .buttonStyle(.glass)
                                .accessibilityLabel(Text("DOI"))
                                .accessibilityValue(Text(verbatim: doi))
                            }
                        }
                    }
                    if let abstract = item.abstractNote {
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Abstract").font(.headline)
                            Text(abstract)
                                .textSelection(.enabled)
                                .foregroundStyle(.secondary)
                        }
                    }
                    Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 16, verticalSpacing: 8) {
                        row("Volume", item.volume)
                        row("Issue", item.issue)
                        row("Pages", item.pages)
                        row("Publisher", item.publisher)
                        row("Place", item.place)
                        row("ISBN", item.isbn)
                        row("ISSN", item.issn)
                        row("arXiv", item.arxivId)
                        row("PMID", item.pmid)
                        row("Keywords", item.keywords.isEmpty ? nil : item.keywords.joined(separator: "; "))
                        row("Cited by", item.citedBy.map { $0.formatted() })
                    }
                    FilesSection(itemID: itemID, itemTitle: item.title)
                    mentions(of: itemID)
                    RelatedSection(itemID: itemID)
                    aiSummary
                    ForEach(plugins.sections) { section in
                        PluginSectionBox(section: section, itemID: itemID)
                    }
                    CitationBox(itemIDs: [itemID])
                }
                .padding(32)
                .frame(maxWidth: 860, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .sheet(item: $editing) { detail in
                EditItemSheet(detail: detail)
            }
            .userActivity(AppleActivity.item, element: itemID) { id, activity in
                AppleActivity.configure(activity, title: item.title, key: "id", value: id)
            }
            .navigationDestination(isPresented: $showingNote) { NoteView() }
            .onAppear {
                #if os(iOS)
                // Compact search/related-item navigation also bypasses TabsModel.
                if sizeClass == .compact, plugins.selectedItemKeys.first != itemID {
                    plugins.selectionChanged(itemKeys: [itemID])
                }
                #endif
            }
            .simultaneousGesture(TapGesture().onEnded {
                plugins.selectionChanged(itemKeys: [itemID])
            })
        } else {
            ContentUnavailableView("Item Not Found", systemImage: "questionmark.folder")
        }
    }

    /// Shows the open note: pushed on a phone, in the Notes tab otherwise.
    private func showNote() {
        #if os(iOS)
        if sizeClass == .compact {
            showingNote = true
            return
        }
        #endif
        tabs.open(.notes)
    }

    /// A summary of the paper by the AI provider set up in Settings.
    private var aiSummary: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("AI Summary").font(.headline)
                Spacer()
                Button {
                    Task { await summarize() }
                } label: {
                    if summarizing {
                        ProgressView().controlSize(.small)
                    } else {
                        Text("Summarize")
                    }
                }
                .buttonStyle(.glass)
                .disabled(summarizing)
                Button {
                    Task { await draftMap() }
                } label: {
                    if drafting {
                        ProgressView().controlSize(.small)
                    } else {
                        Text("Mind Map")
                    }
                }
                .buttonStyle(.glass)
                .disabled(drafting)
            }
            if let summaryError {
                Text(summaryError).font(.caption).foregroundStyle(.red)
            }
            if let summary {
                AIAnswerView(answer: summary)
            }
        }
        .onChange(of: itemID) {
            summary = nil
            summaryError = nil
        }
    }

    private func summarize() async {
        guard let core = library.core else { return }
        summarizing = true
        summaryError = nil
        let provider = ai.provider
        let language = AIAssistant.language
        let id = itemID
        do {
            summary = try await Task.detached {
                try core.aiSummarize(provider: provider, itemId: id, language: language)
            }.value
        } catch {
            summaryError = CoreKit.message(for: error)
        }
        summarizing = false
    }

    /// A mind map of the paper, drafted by the AI provider, as a note in
    /// the vault, which then shows.
    private func draftMap() async {
        guard let core = library.core else { return }
        drafting = true
        summaryError = nil
        let provider = ai.provider
        let language = AIAssistant.language
        let id = itemID
        do {
            let drafted = try await Task.detached {
                try core.aiMindmapItem(provider: provider, itemId: id, language: language)
            }.value
            vault.createMap(titled: "\(drafted.title) – \(String(localized: "Mind Map"))", text: drafted.text)
            showNote()
        } catch {
            summaryError = CoreKit.message(for: error)
        }
        drafting = false
    }

    /// The notes that link to the item or quote its highlights.
    private func mentions(of itemID: String) -> some View {
        let found = vault.mentions(ofItem: itemID)
        return VStack(alignment: .leading, spacing: 10) {
            Text("Notes").font(.headline)
            if found.isEmpty {
                Text("No notes mention this paper yet.").foregroundStyle(.secondary)
            }
            ForEach(found, id: \.self) { mention in
                Button {
                    vault.open(mention.path)
                    vault.jump = NoteJump(line: Int(mention.line), anchor: nil)
                    showNote()
                } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(mention.title).font(.callout.weight(.medium))
                        Text(mention.context).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
    }

    private func fact(_ text: String, systemImage: String) -> some View {
        Label(text, systemImage: systemImage)
            .font(.callout)
            .lineLimit(1)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .glassEffect(.regular, in: .capsule)
    }

    @ViewBuilder
    private func row(_ label: LocalizedStringKey, _ value: String?) -> some View {
        if let value, !value.isEmpty {
            GridRow {
                Text(label).foregroundStyle(.secondary)
                Text(value).textSelection(.enabled)
            }
        }
    }
}

/// Adds an item: by identifier, with every detail looked up, or by hand.
struct AddItemSheet: View {
    @Environment(LibraryModel.self) private var library
    @Environment(\.dismiss) private var dismiss
    @State private var identifier = ""
    @State private var lookingUp = false
    @State private var itemType = "journal-article"
    @State private var title = ""
    @State private var authors = ""
    @State private var container = ""
    @State private var year = ""
    @State private var doi = ""
    @State private var error: String?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("DOI, arXiv ID, PMID or ISBN", text: $identifier)
                        .autocorrectionDisabled()
                        .onSubmit(lookUp)
                    if !trimmed(identifier).isEmpty {
                        if let kind = recognizeIdentifier(input: identifier) {
                            Label("Recognized as \(kindName(kind))", systemImage: "checkmark.seal")
                                .foregroundStyle(.green)
                        } else {
                            Label("Not a DOI, arXiv ID, PMID or ISBN", systemImage: "exclamationmark.triangle")
                                .foregroundStyle(.orange)
                        }
                    }
                    Button(action: lookUp) {
                        HStack {
                            Label(
                                lookingUp ? LocalizedStringKey("Looking up…") : LocalizedStringKey("Look Up and Add"),
                                systemImage: "magnifyingglass")
                            if lookingUp {
                                Spacer()
                                ProgressView().controlSize(.small)
                            }
                        }
                    }
                    .disabled(lookingUp || recognizeIdentifier(input: identifier) == nil)
                } header: {
                    Text("Add by identifier")
                }
                Section("Add by hand") {
                    Picker("Type", selection: $itemType) {
                        ForEach(itemTypes(), id: \.self) { id in
                            Text(ItemTypeName.title(for: id)).tag(id)
                        }
                    }
                    NamedField("Title", text: $title, axis: .vertical)
                    NamedField("Authors", text: $authors, prompt: Text("One per line, or separated by ;"), axis: .vertical)
                    NamedField("Journal or book", text: $container)
                    NamedField("Year", text: $year)
                    NamedField("DOI", text: $doi, prompt: Text("10.1038/… or https://doi.org/…"))
                        .autocorrectionDisabled()
                }
                if let error {
                    Section {
                        Label(error, systemImage: "exclamationmark.octagon")
                            .foregroundStyle(.red)
                    }
                }
            }
            .formStyle(.grouped)
            .navigationTitle("Add Item")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add", action: save)
                        .disabled(trimmed(title).isEmpty)
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 480, minHeight: 560)
        #endif
    }

    private func trimmed(_ text: String) -> String {
        text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func kindName(_ kind: String) -> String {
        switch kind {
        case "doi": "DOI"
        case "arxiv": "arXiv"
        case "pmid": "PubMed"
        case "isbn": "ISBN"
        default: kind
        }
    }

    private func lookUp() {
        guard !lookingUp, recognizeIdentifier(input: identifier) != nil else { return }
        lookingUp = true
        error = nil
        Task {
            let message = await library.addItem(identifier: identifier)
            lookingUp = false
            if let message { error = message } else { dismiss() }
        }
    }

    private func save() {
        let names = authors
            .split(whereSeparator: { $0 == "\n" || $0 == ";" || $0 == "；" })
            .map { trimmed(String($0)) }
            .filter { !$0.isEmpty }
        let newItem = NewItem(
            itemType: itemType,
            title: trimmed(title),
            authors: names,
            containerTitle: trimmed(container).isEmpty ? nil : trimmed(container),
            year: Int32(trimmed(year)),
            doi: trimmed(doi).isEmpty ? nil : trimmed(doi))
        if let message = library.addItem(newItem) {
            error = message
        } else {
            dismiss()
        }
    }
}

/// Display names of the core's item type identifiers.
enum ItemTypeName {
    static func title(for id: String) -> LocalizedStringResource {
        switch id {
        case "journal-article": "Journal Article"
        case "conference-paper": "Conference Paper"
        case "book": "Book"
        case "book-chapter": "Book Chapter"
        case "thesis": "Thesis"
        case "report": "Report"
        case "preprint": "Preprint"
        case "patent": "Patent"
        case "standard": "Standard"
        case "dataset": "Dataset"
        case "webpage": "Web Page"
        default: "Other"
        }
    }
}
