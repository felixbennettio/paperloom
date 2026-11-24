#if os(macOS)
import AppKit
#endif
import CoreKit
import Foundation
import Observation

/// What the item list shows.
enum LibraryScope: Hashable {
    case all
    case starred
    case unread
    case trash
    /// Items that look like the same work, to merge.
    case duplicates
    /// What a saved search finds.
    case savedSearch(String)
    /// What a feed announced (not the library's items).
    case feed(String)
    case collection(String)
    case tag(String)
}

/// The library the views show, backed by the Rust core.
@Observable
final class LibraryModel {
    private let library: PaperLibrary?

    private(set) var items: [ItemSummary] = []
    private(set) var collections: [CollectionInfo] = []
    private(set) var tags: [TagInfo] = []
    private(set) var savedSearches: [SavedSearch] = []
    /// The feeds followed, with their unread counts.
    private(set) var feeds: [FeedInfo] = []
    /// The fields saved-search conditions can test.
    let conditionFields = searchFields()
    /// What the item list shows.
    var scope = LibraryScope.all {
        didSet { reloadItems() }
    }

    /// The collection the item list shows, if it shows one; new items go
    /// there too.
    var selectedCollectionID: String? {
        get {
            if case .collection(let id) = scope { return id }
            return nil
        }
        set { scope = newValue.map(LibraryScope.collection) ?? .all }
    }
    var errorMessage: String? = nil
    /// A report to show, such as what adding PDFs did.
    var notice: String? = nil
    private(set) var isImporting = false
    /// Shared files have their own progress sheet and combined report.
    var isImportingSharedFiles = false
    /// Changes whenever items or attachments change, for views that read
    /// them through functions.
    private(set) var revision = 0
    /// Where the database lives, for the About section.
    let location: String
    @ObservationIgnored private var backupsScheduled = false
    #if os(macOS)
    @ObservationIgnored private var connector: ConnectorListener?
    #endif
    /// Built-in and added citation styles, for the style pickers.
    private(set) var citationStyles: [CitationStyle] = offeredCitationStyles()

    /// The core library, for the vault to share its search index.
    var core: PaperLibrary? { library }

    init(library: PaperLibrary?, location: String) {
        self.library = library
        self.location = location
        reload()
    }

    /// The library in Application Support; falls back to an in-memory one
    /// (and says why) when the file cannot be opened.
    static func openDefault() -> LibraryModel {
        if ScreenshotSample.isOn { return ScreenshotSample.library() }
        do {
            let path = try CoreKit.defaultLibraryURL().path(percentEncoded: false)
            // A backup put back last time takes the library's place first.
            _ = try? finishRestore(libraryPath: path)
            return LibraryModel(library: try PaperLibrary(path: path), location: path)
        } catch {
            let model = LibraryModel(library: try? PaperLibrary.inMemory(), location: String(localized: "In memory"))
            model.errorMessage = CoreKit.message(for: error)
            return model
        }
    }

    var selectedCollection: CollectionInfo? {
        collections.first { $0.id == selectedCollectionID }
    }

    func item(id: String) -> ItemSummary? {
        items.first { $0.id == id }
    }

    /// The list's title.
    var scopeTitle: String {
        switch scope {
        case .all: String(localized: "All Items")
        case .starred: String(localized: "Starred")
        case .unread: String(localized: "Unread")
        case .trash: String(localized: "Trash")
        case .duplicates: String(localized: "Duplicate Items")
        case .savedSearch(let id): savedSearches.first { $0.id == id }?.name ?? String(localized: "Saved Searches")
        case .feed(let id): feeds.first { $0.id == id }?.title ?? String(localized: "Feeds")
        case .collection: selectedCollection?.name ?? String(localized: "All Items")
        case .tag(let name): name
        }
    }

    func reload() {
        perform { collections = try $0.collections() }
        perform { savedSearches = try $0.savedSearches() }
        perform { feeds = try $0.feeds() }
        reloadItems()
    }

    private func reloadItems() {
        // Publish after the reload finishes, including a failed filtered read:
        // plugins independently fetch the complete last-good projection.
        defer { revision += 1 }
        let filter: ItemFilter
        switch scope {
        case .all, .duplicates, .feed:
            filter = ItemFilter(collectionId: nil, tag: nil, starredOnly: false, readStatus: nil, trashed: false)
        case .starred: filter = ItemFilter(collectionId: nil, tag: nil, starredOnly: true, readStatus: nil, trashed: false)
        case .unread: filter = ItemFilter(collectionId: nil, tag: nil, starredOnly: false, readStatus: "unread", trashed: false)
        case .trash: filter = ItemFilter(collectionId: nil, tag: nil, starredOnly: false, readStatus: nil, trashed: true)
        case .collection(let id): filter = ItemFilter(collectionId: id, tag: nil, starredOnly: false, readStatus: nil, trashed: false)
        case .tag(let name): filter = ItemFilter(collectionId: nil, tag: name, starredOnly: false, readStatus: nil, trashed: false)
        case .savedSearch(let id):
            let search = savedSearches.first { $0.id == id }
            perform { library in
                items = try search.map { try library.runSearch(search: $0) } ?? []
                tags = try library.tags()
            }
            return
        }
        perform { library in
            items = try library.listItems(filter: filter)
            tags = try library.tags()
        }
    }

    /// Every item with a read status (`unread`, `reading` or `read`),
    /// whatever the list shows.
    func items(readStatus: String) -> [ItemSummary] {
        var found: [ItemSummary] = []
        perform {
            found = try $0.listItems(
                filter: ItemFilter(collectionId: nil, tag: nil, starredOnly: false, readStatus: readStatus, trashed: false))
        }
        return found
    }

    // Duplicates

    /// Items that look like the same work, each group newest first.
    func duplicateGroups() -> [DuplicateGroup] {
        var groups: [DuplicateGroup] = []
        perform { groups = try $0.duplicateGroups() }
        return groups
    }

    /// Merges `others` into `master`, which keeps its details and takes the
    /// others' files, tags and collections; the others go to the trash.
    func mergeItems(master: String, others: [String]) {
        perform { _ = try $0.mergeItems(masterId: master, otherIds: others) }
        reloadItems()
    }

    // Citation counts

    /// Looks up how many works cite some items (all, when none are named),
    /// as OpenAlex counts them; what to tell the reader.
    @MainActor
    func countCitations(_ ids: [String]) async -> String {
        guard let library else { return String(localized: "The library is not available.") }
        do {
            let counted = try await Task.detached { try library.countCitations(itemIds: ids) }.value
            reloadItems()
            return String(localized: "Citations counted for items: \(String(counted)).")
        } catch {
            return CoreKit.message(for: error)
        }
    }

    // Feeds

    /// A feed's entries, the newest reading first.
    func feedItems(feedID: String) -> [FeedItem] {
        guard let library else { return [] }
        return (try? library.feedItems(feedId: feedID)) ?? []
    }

    func setFeedItemRead(_ id: String, read: Bool) {
        perform { try $0.setFeedItemRead(itemId: id, read: read) }
        perform { feeds = try $0.feeds() }
        revision += 1
    }

    func markFeedRead(_ feedID: String) {
        perform { try $0.markFeedRead(feedId: feedID) }
        perform { feeds = try $0.feeds() }
        revision += 1
    }

    func renameFeed(_ id: String, to title: String) {
        perform { try $0.renameFeed(feedId: id, title: title) }
        perform { feeds = try $0.feeds() }
    }

    /// Stops following a feed; its entries go with it.
    func removeFeed(_ id: String) {
        perform { try $0.removeFeed(feedId: id) }
        perform { feeds = try $0.feeds() }
        if scope == .feed(id) { scope = .all }
    }

    /// Follows a feed and reads it, then shows it; the reason when it
    /// cannot be followed.
    @MainActor
    func addFeed(_ url: String) async -> String? {
        guard let library else { return String(localized: "The library is not available.") }
        do {
            let feed = try await Task.detached { try library.addFeed(url: url) }.value
            feeds = try library.feeds()
            scope = .feed(feed.id)
            return nil
        } catch {
            return CoreKit.message(for: error)
        }
    }

    /// Reads every feed again; one that fails says why in its own place.
    @MainActor
    func refreshFeeds() async {
        guard let library else { return }
        let read = try? await Task.detached { try library.refreshFeeds() }.value
        if let read { feeds = read }
        revision += 1
    }

    /// Adds a feed entry to the library (by its DOI's full record when it
    /// has one); the reason when it cannot be added.
    @MainActor
    func addFeedItem(_ id: String) async -> String? {
        guard let library else { return String(localized: "The library is not available.") }
        defer { reloadItems() }
        do {
            _ = try await Task.detached { try library.addFeedItem(itemId: id, collectionId: nil) }.value
            feeds = try library.feeds()
            return nil
        } catch {
            return CoreKit.message(for: error)
        }
    }

    // Related items

    /// The items related to an item, not counting the trash.
    func relatedItems(itemID: String) -> [ItemSummary] {
        guard let library else { return [] }
        return (try? library.relatedItems(itemId: itemID)) ?? []
    }

    /// Relates two items, each to the other.
    func relate(_ itemID: String, _ otherID: String) {
        perform { try $0.relateItems(itemId: itemID, otherId: otherID) }
        revision += 1
    }

    func unrelate(_ itemID: String, _ otherID: String) {
        perform { try $0.unrelateItems(itemId: itemID, otherId: otherID) }
        revision += 1
    }

    /// Every item not in the trash, for pickers.
    func allItems() -> [ItemSummary] {
        guard let library else { return [] }
        let filter = ItemFilter(collectionId: nil, tag: nil, starredOnly: false, readStatus: nil, trashed: false)
        return (try? library.listItems(filter: filter)) ?? []
    }

    // Saved searches

    /// Saves a search and shows what it finds; the reason when it cannot
    /// be saved.
    func saveSearch(_ search: SavedSearch) -> String? {
        guard let library else { return String(localized: "The library is not available.") }
        do {
            let saved = try library.saveSearch(search: search)
            savedSearches = try library.savedSearches()
            if scope == .savedSearch(saved.id) { reloadItems() } else { scope = .savedSearch(saved.id) }
            return nil
        } catch {
            return CoreKit.message(for: error)
        }
    }

    func deleteSavedSearch(id: String) {
        perform { try $0.deleteSavedSearch(searchId: id) }
        perform { savedSearches = try $0.savedSearches() }
        if scope == .savedSearch(id) { scope = .all }
    }

    /// How many items a search finds as it stands, for its editor.
    func matchCount(_ search: SavedSearch) -> Int? {
        guard let library else { return nil }
        return (try? library.runSearch(search: search))?.count
    }

    /// A search not saved yet: one condition on the first field.
    func newSearch() -> SavedSearch {
        let first = conditionFields.first.map { condition(on: $0) }
        return SavedSearch(id: "", name: "", matchAll: true, conditions: first.map { [$0] } ?? [])
    }

    /// A condition on a field: its usual test and a value that fits.
    func condition(on field: SearchField) -> SearchCondition {
        SearchCondition(field: field.id, op: field.ops.first ?? "is", value: defaultValue(for: field))
    }

    /// A value that fits a field: the first choice, or a usual one.
    func defaultValue(for field: SearchField) -> String {
        switch field.kind {
        case "flag": "true"
        case "readStatus": "unread"
        case "itemType": itemTypes().first ?? ""
        case "days": "30"
        case "tag": tags.first?.name ?? ""
        case "collection": collections.first?.id ?? ""
        default: ""
        }
    }

    // Organising

    /// Saves edited details; the reason when they cannot be saved.
    func updateItem(_ detail: ItemDetail) -> String? {
        guard let library else { return String(localized: "The library is not available.") }
        defer { reloadItems() }
        do {
            _ = try library.updateItem(detail: detail)
            return nil
        } catch {
            return CoreKit.message(for: error)
        }
    }

    func setStarred(_ starred: Bool, id: String) {
        perform { try $0.setStarred(itemId: id, starred: starred) }
        reloadItems()
    }

    /// `unread`, `reading` or `read`.
    func setReadStatus(_ status: String, id: String) {
        perform { try $0.setReadStatus(itemId: id, status: status) }
        reloadItems()
    }

    func setTags(_ tags: [String], id: String) {
        perform { try $0.setItemTags(itemId: id, tags: tags) }
        reloadItems()
    }

    func setTagColor(_ color: String?, name: String) {
        perform { try $0.setTagColor(name: name, color: color) }
        reloadItems()
    }

    func renameTag(_ name: String, to newName: String) {
        perform { try $0.renameTag(name: name, newName: newName) }
        if scope == .tag(name) { scope = .tag(newName) } else { reloadItems() }
    }

    func deleteTag(_ name: String) {
        perform { try $0.deleteTag(name: name) }
        if scope == .tag(name) { scope = .all } else { reloadItems() }
    }

    func color(ofTag name: String) -> String? {
        tags.first { $0.name == name }?.color
    }

    func restoreItem(id: String) {
        perform { try $0.restoreItem(itemId: id) }
        reloadItems()
    }

    func deleteItemForever(id: String) {
        perform { try $0.deleteItem(itemId: id) }
        reloadItems()
    }

    func emptyTrash() {
        perform { _ = try $0.emptyTrash() }
        reloadItems()
    }

    /// Adds an item (to the selected collection too, if any). Returns the
    /// reason when it cannot be added.
    func addItem(_ newItem: NewItem) -> String? {
        guard let library else { return String(localized: "The library is not available.") }
        // An item may commit before adding it to its collection fails.
        defer { reloadItems() }
        do {
            let item = try library.addItem(newItem: newItem)
            if let collection = selectedCollectionID {
                try library.addToCollection(collectionId: collection, itemId: item.id)
            }
            return nil
        } catch {
            return CoreKit.message(for: error)
        }
    }

    /// Looks up a DOI, arXiv ID, PubMed ID or ISBN and adds the item with
    /// all its details. Returns the reason when it cannot.
    @MainActor
    func addItem(identifier: String) async -> String? {
        do {
            _ = try await lookUpAndAdd(identifier, collectionID: selectedCollectionID)
            return nil
        } catch {
            return CoreKit.message(for: error)
        }
    }

    /// Shares identifier resolution with Shortcuts, which needs the added title.
    @MainActor
    func lookUpAndAdd(_ identifier: String, collectionID: String?) async throws -> ItemSummary {
        guard let library else { throw LibraryUnavailable() }
        defer { reloadItems() }
        return try await Task.detached {
            try library.addByIdentifier(input: identifier, collectionId: collectionID)
        }.value
    }

    /// Imports reference files into the library (and the selected
    /// collection); returns the report to show.
    func importFiles(_ urls: [URL]) -> String {
        guard let library else { return String(localized: "The library is not available.") }
        var added: UInt32 = 0, duplicates: UInt32 = 0, skipped: UInt32 = 0
        var problems: [String] = []
        for url in urls {
            do {
                let text = try ReferenceFileReader.read(url)
                let report = try library.importReferences(text: text, collectionId: selectedCollectionID)
                added += report.added
                duplicates += report.duplicates
                skipped += report.skipped
            } catch {
                problems.append("\(url.lastPathComponent): \(CoreKit.message(for: error))")
            }
        }
        reloadItems()
        let (a, d, s) = (String(added), String(duplicates), String(skipped))
        var lines = [String(localized: "Added \(a), \(d) already in the library, \(s) skipped.")]
        lines.append(contentsOf: problems)
        return lines.joined(separator: "\n")
    }

    /// Adds PDFs (to the selected collection too, if any): each is
    /// recognised from its first pages and looked up, or joins the paper
    /// already in the library. Shared imports use the returned report;
    /// the in-app picker also leaves it in `notice`.
    @discardableResult
    @MainActor
    func importPDFs(_ urls: [URL], showNotice: Bool = true) async -> String {
        guard let library else {
            let report = String(localized: "The library is not available.")
            if showNotice { notice = report }
            return report
        }
        let collection = selectedCollectionID
        isImporting = true
        let report = await Task.detached { () -> String in
            var added = 0, joined = 0, unrecognised = 0
            var lines: [String] = []
            for url in urls {
                let scoped = url.startAccessingSecurityScopedResource()
                defer { if scoped { url.stopAccessingSecurityScopedResource() } }
                let pages = PDFFirstPages.read(url)
                do {
                    let result = try library.importPdf(
                        path: url.path(percentEncoded: false), firstPagesText: pages.text,
                        pdfTitle: pages.title, collectionId: collection)
                    if result.existing { joined += 1 } else { added += 1 }
                    if result.attachment.isPdf {
                        let pages = PDFFirstPages.allPages(url)
                        try? library.setAttachmentText(attachmentId: result.attachment.id, pages: pages)
                    }
                    if result.matchedBy == "none" { unrecognised += 1 }
                    if let note = result.note {
                        lines.append("\(url.lastPathComponent): \(note)")
                    }
                } catch {
                    lines.append("\(url.lastPathComponent): \(CoreKit.message(for: error))")
                }
            }
            let (a, j) = (String(added), String(joined))
            var report = [String(localized: "PDFs added: \(a); added to papers already in the library: \(j).")]
            if unrecognised > 0 {
                let u = String(unrecognised)
                report.append(String(localized: "Not recognised, so named after their files: \(u). Fill in their details."))
            }
            return (report + lines).joined(separator: "\n")
        }.value
        isImporting = false
        reloadItems()
        if showNotice { notice = report }
        return report
    }

    /// Backs the library (and the vault) up to a `.zip` in the temporary
    /// folder, for the person to save where they like.
    @MainActor
    func backUp(vault: PaperVault?) async throws -> URL {
        guard let library else { throw LibraryUnavailable() }
        let day = Date.now.formatted(.iso8601.year().month().day())
        let url = FileManager.default.temporaryDirectory.appending(path: "Paperloom \(day).zip")
        _ = try await Task.detached {
            try library.backUp(destination: url.path(percentEncoded: false), vault: vault)
        }.value
        return url
    }

    /// Backs the library and the vault up beside the library on the
    /// schedule chosen in Settings: checked a minute after starting, then
    /// every hour, keeping the newest 10. Runs until cancelled.
    @MainActor
    func backUpOnSchedule(vault: VaultModel) async {
        guard !backupsScheduled, let library else { return }
        backupsScheduled = true
        defer { backupsScheduled = false }
        try? await Task.sleep(for: .seconds(60))
        while !Task.isCancelled {
            let choice = AutomaticBackup(rawValue: UserDefaults.standard.string(forKey: AutomaticBackup.key) ?? "")
            if let hours = choice?.hours {
                let core = vault.core
                _ = try? await Task.detached(priority: .background) {
                    try library.backUpIfDue(folder: nil, vault: core, everyHours: hours, keep: 10)
                }.value
            }
            try? await Task.sleep(for: .seconds(3600))
        }
    }

    #if os(macOS)
    /// Starts the connector the browser extension saves through; works it
    /// adds show at once.
    @MainActor
    func startConnector() {
        guard connector == nil, let library else { return }
        let listener = ConnectorListener { [weak self] _ in
            Task { @MainActor in self?.reload() }
        }
        connector = listener
        _ = library.startConnector(port: connectorPort(), events: listener)
    }
    #endif

    /// Styles in the CSL style repository matching every word typed.
    func searchCitationStyles(_ query: String) async throws -> [StyleListing] {
        guard let library else { throw LibraryUnavailable() }
        return try await Task.detached {
            try library.searchCitationStyles(query: query, limit: 30)
        }.value
    }

    /// Downloads a style (and the one it cites like, when needed).
    @MainActor
    func downloadCitationStyle(id: String) async throws {
        guard let library else { throw LibraryUnavailable() }
        _ = try await Task.detached {
            try library.downloadCitationStyle(id: id)
        }.value
        citationStyles = offeredCitationStyles()
    }

    /// Adds the style in a `.csl` file, named after the file.
    @MainActor
    func addCitationStyle(file: URL) async throws {
        guard let library else { throw LibraryUnavailable() }
        _ = try await Task.detached {
            let scoped = file.startAccessingSecurityScopedResource()
            defer { if scoped { file.stopAccessingSecurityScopedResource() } }
            return try library.addCitationStyleFile(path: file.path(percentEncoded: false))
        }.value
        citationStyles = offeredCitationStyles()
    }

    @MainActor
    func removeCitationStyle(id: String) throws {
        guard let library else { throw LibraryUnavailable() }
        try library.removeCitationStyle(id: id)
        citationStyles = offeredCitationStyles()
    }

    /// Puts a backup back: the notes now, the library when the app next
    /// starts.
    @MainActor
    func restore(from backup: URL, vault: PaperVault?) async throws -> RestoreReport {
        guard let library else { throw LibraryUnavailable() }
        return try await Task.detached {
            let scoped = backup.startAccessingSecurityScopedResource()
            defer { if scoped { backup.stopAccessingSecurityScopedResource() } }
            return try library.restoreBackup(backup: backup.path(percentEncoded: false), vault: vault)
        }.value
    }

    /// Moves a Zotero library in from its data folder (usually ~/Zotero):
    /// items, files, annotations, and notes into the vault. Says how it went.
    @MainActor
    func importZotero(from folder: URL, vault: PaperVault?) async -> String {
        guard let library else { return String(localized: "The library is not available.") }
        isImporting = true
        let result = await Task.detached { () -> Result<ZoteroImport, Error> in
            let scoped = folder.startAccessingSecurityScopedResource()
            defer { if scoped { folder.stopAccessingSecurityScopedResource() } }
            return Result { try library.importZotero(dataDir: folder.path(percentEncoded: false), vault: vault) }
        }.value
        isImporting = false
        reload()
        switch result {
        case .success(let report):
            let (a, d, f, n, notes) = (
                String(report.added), String(report.duplicates), String(report.files), String(report.annotations),
                String(report.notes)
            )
            var lines = [
                String(
                    localized:
                        "Items moved in from Zotero: \(a); already here: \(d); files: \(f); annotations: \(n); notes: \(notes)."
                )
            ]
            if report.missingFiles > 0 {
                let missing = String(report.missingFiles)
                lines.append(String(localized: "Attached files not on this computer: \(missing)."))
            }
            if report.fromBackup {
                lines.append(String(localized: "Zotero was open, so its last backup was read."))
            }
            return (lines + report.problems).joined(separator: "\n")
        case .failure(let error):
            return CoreKit.message(for: error)
        }
    }

    /// Writes a copy of a Word (or OpenDocument) document with its
    /// `[@citations]` formatted in a style and the reference list added,
    /// into a temporary folder; the copy, and what became of the citations.
    func citeWordDocument(_ source: URL, style: String) async -> Result<(URL, WordCitations), Error> {
        guard let library else { return .failure(LibraryUnavailable()) }
        let name = source.deletingPathExtension().lastPathComponent
        let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString, directoryHint: .isDirectory)
        // The copy is the same kind of document as the original.
        let suffix = source.pathExtension.lowercased() == "odt" ? ".odt" : ".docx"
        let copy = folder.appending(path: String(localized: "\(name) (cited)") + suffix)
        return await Task.detached { () -> Result<(URL, WordCitations), Error> in
            let scoped = source.startAccessingSecurityScopedResource()
            defer { if scoped { source.stopAccessingSecurityScopedResource() } }
            return Result {
                try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
                let report = try library.citeWordDocument(
                    source: source.path(percentEncoded: false),
                    destination: copy.path(percentEncoded: false),
                    styleId: style
                )
                return (copy, report)
            }
        }.value
    }

    /// Moves an EndNote library in from its XML export, with its PDFs from
    /// the library's `.Data` folder when given. Says how it went.
    @MainActor
    func importEndNote(from export: URL, data: URL?, vault: PaperVault?) async -> String {
        guard let library else { return String(localized: "The library is not available.") }
        isImporting = true
        let result = await Task.detached { () -> Result<EndNoteImport, Error> in
            let scoped = export.startAccessingSecurityScopedResource()
            let dataScoped = data?.startAccessingSecurityScopedResource() ?? false
            defer {
                if scoped { export.stopAccessingSecurityScopedResource() }
                if dataScoped { data?.stopAccessingSecurityScopedResource() }
            }
            return Result {
                try library.importEndnote(
                    xmlPath: export.path(percentEncoded: false), dataFolder: data?.path(percentEncoded: false),
                    vault: vault)
            }
        }.value
        isImporting = false
        reload()
        switch result {
        case .success(let report):
            let (a, d, f, notes) = (
                String(report.added), String(report.duplicates), String(report.files), String(report.notes)
            )
            var lines = [
                String(localized: "Items moved in from EndNote: \(a); already here: \(d); files: \(f); notes: \(notes).")
            ]
            if report.missingFiles > 0 {
                let missing = String(report.missingFiles)
                lines.append(String(localized: "Attached files not on this computer: \(missing)."))
            }
            return (lines + report.problems).joined(separator: "\n")
        case .failure(let error):
            return CoreKit.message(for: error)
        }
    }

    /// Items matching every word typed, for citing in notes. Quiet on
    /// failure: the picker asks as it draws.
    func citationCompletions(_ query: String) -> [CitationCompletion] {
        (try? library?.citationCompletions(query: query, limit: 40)) ?? []
    }

    /// An item's attached files.
    func attachments(itemID: String) -> [AttachmentInfo] {
        guard let library else { return [] }
        return (try? library.attachments(itemId: itemID)) ?? []
    }

    func attachFiles(_ urls: [URL], to itemID: String) {
        for url in urls {
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            perform { _ = try $0.attachFile(itemId: itemID, path: url.path(percentEncoded: false)) }
        }
        revision += 1
    }

    /// Looks for an open-access PDF of an item (arXiv, OpenAlex, the
    /// publisher's page) and attaches it, keeping its text for search. What
    /// to tell the reader when none was attached.
    @MainActor
    func findAvailablePDF(itemID: String) async -> String? {
        guard let library else { return String(localized: "The library is not available.") }
        do {
            let found = try await Task.detached { try library.findAvailablePdf(itemId: itemID) }.value
            guard let found else { return String(localized: "No open-access PDF was found.") }
            revision += 1
            if found.attachment.isPdf {
                indexPDFIfNeeded(
                    attachmentID: found.attachment.id, url: URL(fileURLWithPath: found.attachment.filePath))
            }
            return nil
        } catch {
            return CoreKit.message(for: error)
        }
    }

    /// The marks stored for an attachment.
    func annotations(attachmentID: String) -> [AnnotationInfo] {
        guard let library else { return [] }
        return (try? library.annotations(attachmentId: attachmentID)) ?? []
    }

    func addAnnotation(_ annotation: NewAnnotation) -> AnnotationInfo? {
        guard let library else { return nil }
        do {
            let saved = try library.addAnnotation(annotation: annotation)
            revision += 1
            return saved
        } catch {
            errorMessage = CoreKit.message(for: error)
            return nil
        }
    }

    func updateAnnotation(id: String, color: String, comment: String?) -> AnnotationInfo? {
        guard let library else { return nil }
        do {
            let saved = try library.updateAnnotation(annotationId: id, color: color, comment: comment)
            revision += 1
            return saved
        } catch {
            errorMessage = CoreKit.message(for: error)
            return nil
        }
    }

    /// Markdown that embeds a highlight in a note.
    func annotationEmbed(id: String) -> String? {
        try? library?.annotationEmbed(annotationId: id)
    }

    /// Markdown that links a note to an item.
    func itemLink(id: String) -> String? {
        try? library?.itemLink(itemId: id)
    }

    func annotationLocation(id: String) -> AnnotationLocation? {
        (try? library?.annotationLocation(annotationId: id)) ?? nil
    }

    func resolveItemReference(_ reference: String) -> String? {
        (try? library?.resolveItemReference(reference: reference)) ?? nil
    }

    func attachment(id: String) -> AttachmentInfo? {
        (try? library?.attachment(attachmentId: id)) ?? nil
    }

    func removeAnnotation(id: String) {
        guard let library else { return }
        do {
            try library.removeAnnotation(annotationId: id)
            revision += 1
        } catch {
            errorMessage = CoreKit.message(for: error)
        }
    }

    func removeAttachment(id: String) {
        perform { try $0.removeAttachment(attachmentId: id) }
        revision += 1
    }

    /// Matches in items, PDF text, annotations and connected notes.
    @MainActor
    func searchEverything(_ query: String, kinds: [String]) async -> [FullTextHit] {
        guard let library else { return [] }
        return (try? await Task.detached {
            try library.searchEverything(query: query, kinds: kinds, limit: 200)
        }.value) ?? []
    }

    /// Keeps a PDF's page text for full-text search, unless it already is.
    func indexPDFIfNeeded(attachmentID: String, url: URL) {
        guard let library, (try? library.attachmentHasText(attachmentId: attachmentID)) == false else { return }
        Task.detached(priority: .utility) {
            let pages = PDFFirstPages.allPages(url)
            try? library.setAttachmentText(attachmentId: attachmentID, pages: pages)
        }
    }

    /// Items as a reference file in one of `referenceFormats()`.
    func exportReferences(ids: [String], format: String) -> String? {
        guard let library else { return nil }
        do {
            return try library.exportReferences(itemIds: ids, format: format)
        } catch {
            errorMessage = CoreKit.message(for: error)
            return nil
        }
    }

    /// A page of results from an online database, one of `searchSources()`,
    /// with the person's key for one that needs it. Talks to the network off
    /// the main thread.
    @MainActor
    func searchOnline(sourceID: String, text: String, page: UInt32) async throws -> SearchPage {
        guard let library else { throw LibraryUnavailable() }
        return try await Task.detached {
            let key = DatabaseKeys.key(for: sourceID)
            return try library.searchOnline(sourceId: sourceID, text: text, page: page, apiKey: key)
        }.value
    }

    /// A page of results from a library catalogue the person added. Talks
    /// to the network off the main thread.
    @MainActor
    func searchCatalogue(_ catalogue: SruCatalogue, text: String, page: UInt32) async throws -> SearchPage {
        guard let library else { throw LibraryUnavailable() }
        return try await Task.detached {
            try library.searchCatalogue(catalogue: catalogue, text: text, page: page)
        }.value
    }

    /// Adds results of recent searches (to the selected collection too, if
    /// any).
    func addSearchResults(keys: [String]) -> ImportReport? {
        guard let library else { return nil }
        defer { reloadItems() }
        do {
            let report = try library.addSearchResults(keys: keys, collectionId: selectedCollectionID)
            return report
        } catch {
            errorMessage = CoreKit.message(for: error)
            return nil
        }
    }

    /// References and in-text citation for items in a citation style.
    func formatCitations(ids: [String], style: String) -> CitationOutput? {
        guard let library else { return nil }
        return try? library.formatCitations(itemIds: ids, styleId: style, html: false)
    }

    /// The `[@key]` citation of some items, as notes and Word documents take it.
    func citationMarker(ids: [String]) -> String? {
        try? library?.citationMarker(itemIds: ids)
    }

    /// Every field of an item.
    func detail(id: String) -> ItemDetail? {
        guard let library else { return nil }
        return try? library.itemDetail(itemId: id)
    }

    func createCollection(named name: String, parentID: String? = nil) {
        perform { _ = try $0.createCollection(name: name, parentId: parentID) }
        reload()
    }

    func trashItem(id: String) {
        perform { try $0.trashItem(itemId: id) }
        reloadItems()
    }

    /// The collections as a tree, for the sidebar.
    var collectionTree: [CollectionNode] {
        let children = Dictionary(grouping: collections, by: \.parentId)
        func nodes(under parent: String?) -> [CollectionNode] {
            (children[parent] ?? []).map { info in
                let sub = nodes(under: info.id)
                return CollectionNode(info: info, children: sub.isEmpty ? nil : sub)
            }
        }
        return nodes(under: nil)
    }

    private func perform(_ body: (PaperLibrary) throws -> Void) {
        guard let library else { return }
        do {
            try body(library)
        } catch {
            errorMessage = CoreKit.message(for: error)
        }
    }
}

struct CollectionNode: Identifiable, Hashable {
    let info: CollectionInfo
    let children: [CollectionNode]?

    var id: String { info.id }
}

/// Hears from the connector when the browser extension added works.
final class ConnectorListener: ConnectorEvents, @unchecked Sendable {
    private let onAdded: @Sendable (UInt32) -> Void

    init(onAdded: @escaping @Sendable (UInt32) -> Void) {
        self.onAdded = onAdded
    }

    func added(count: UInt32) {
        onAdded(count)
    }

    /// A writing plugin (Word, WPS) asks to use the library: the person
    /// answers an alert on the main thread while the connector's thread
    /// waits (the connector runs only on the Mac).
    func allowWriter(client: String, origin: String) -> Bool {
        #if os(macOS)
        let answer = WriterAnswer()
        DispatchQueue.main.async {
            MainActor.assumeIsolated {
                answer.allowed = Self.ask(client: client, origin: origin)
            }
            answer.done.signal()
        }
        answer.done.wait()
        return answer.allowed
        #else
        return false
        #endif
    }

    #if os(macOS)
    @MainActor
    private static func ask(client: String, origin: String) -> Bool {
        let alert = NSAlert()
        alert.messageText = String(localized: "Allow \(client) to use your library?")
        var info = String(
            localized: "It can then search your library and format the citations in your documents. It cannot change the library."
        )
        if !origin.isEmpty {
            info += "\n\n" + String(localized: "Asked from \(origin)")
        }
        alert.informativeText = info
        alert.addButton(withTitle: String(localized: "Allow"))
        alert.addButton(withTitle: String(localized: "Don't Allow"))
        NSApp.activate()
        return alert.runModal() == .alertFirstButtonReturn
    }
    #endif
}

/// The answer a pairing question's thread waits for.
private final class WriterAnswer: @unchecked Sendable {
    var allowed = false
    let done = DispatchSemaphore(value: 0)
}

/// The styles the core offers; outside the model, whose property of the
/// same name would hide the core's function.
private func offeredCitationStyles() -> [CitationStyle] {
    citationStyles()
}

/// Thrown when the library database could not be opened at all.
struct LibraryUnavailable: LocalizedError {
    var errorDescription: String? { String(localized: "The library is not available.") }
}
