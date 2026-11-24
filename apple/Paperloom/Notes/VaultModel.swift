import CoreKit
import Foundation
import Observation

/// The Markdown vault the notes views show: the note list, the open note
/// (saved shortly after typing stops), its outline and backlinks.
@MainActor
@Observable
final class VaultModel {
    private var vault: PaperVault?
    private(set) var notes: [NoteInfo] = []
    /// Counts saves and list reloads, so plugins hear that notes changed.
    private(set) var changes = 0
    private(set) var folders: [String] = []
    /// The vault's canvases, by path.
    private(set) var canvases: [String] = []
    /// The vault's drawings, by path.
    private(set) var drawings: [String] = []
    /// The vault's draw.io diagrams, by path.
    private(set) var diagrams: [String] = []
    private(set) var path: String?
    /// The open note's text; edits are saved shortly after they stop.
    var text = "" {
        didSet {
            if path != nil, !loading, text != oldValue {
                edited = true
                scheduleSave()
            }
        }
    }
    /// The open note has edits not yet saved. Only then is it written, so
    /// reopening a note never writes an old copy over a newer file.
    @ObservationIgnored private var edited = false
    private(set) var outline: NoteOutline?
    /// Bumped on every open, so editors reload a note changed elsewhere.
    private(set) var version = 0
    var errorMessage: String?
    private(set) var location = ""
    /// A place in the open note to show (from the outline, backlinks or
    /// search); views react to its `id` changing.
    var jump: NoteJump?
    /// Text for the open note's editor to put at the cursor (a suggested
    /// link); the editor takes it.
    var insertion: String?
    /// The editor's selection as UTF-16 offsets, as plugins count text.
    @ObservationIgnored var selectionUTF16: Range<Int>?
    /// The note editor has the keyboard focus; plugins suggest only then.
    @ObservationIgnored var editorFocused = false
    /// A selection (UTF-16 offsets) a plugin's edit left; the editor shows it.
    var pluginSelection: Range<Int>?
    @ObservationIgnored private var loading = false
    @ObservationIgnored private var saveTask: Task<Void, Never>?

    init() {
        do {
            if ScreenshotSample.isOn {
                let vault = try ScreenshotSample.vault()
                self.vault = vault
                location = try vault.root()
            } else {
                let folder = try CoreKit.defaultLibraryURL().deletingLastPathComponent()
                    .appendingPathComponent("Vault", isDirectory: true)
                let vault = try PaperVault.open(path: folder.path(percentEncoded: false))
                self.vault = vault
                location = (try? vault.root()) ?? folder.path(percentEncoded: false)
            }
        } catch {
            errorMessage = CoreKit.message(for: error)
        }
        reloadList()
        if let first = notes.first { open(first.path) }
    }

    /// The core vault, for syncing its notes.
    var core: PaperVault? { vault }

    /// Reloads the list and the open note after notes sync wrote them.
    func reloadAfterSync() {
        guard saveTask == nil else { return }
        reloadList()
        if let path { open(path) }
    }

    /// Makes the notes findable through the library's full-text search.
    func connect(to library: LibraryModel) {
        guard let vault, let core = library.core else { return }
        Task.detached(priority: .utility) { vault.connectLibrary(library: core) }
    }

    var title: String {
        path.map { (($0 as NSString).lastPathComponent as NSString).deletingPathExtension } ?? ""
    }

    @discardableResult
    private func perform<T>(_ body: (PaperVault) throws -> T) -> T? {
        guard let vault else { return nil }
        do {
            return try body(vault)
        } catch {
            errorMessage = CoreKit.message(for: error)
            return nil
        }
    }

    func reloadList() {
        changes += 1
        notes = perform { try $0.notes() } ?? []
        folders = perform { try $0.folders() } ?? []
        canvases = perform { try $0.canvases() } ?? []
        drawings = perform { try $0.drawings() } ?? []
        diagrams = perform { try $0.diagrams() } ?? []
    }

    func readCanvas(_ path: String) -> CanvasBoard? {
        perform { try $0.readCanvas(path: path) }
    }

    func writeCanvas(_ path: String, _ board: CanvasBoard) {
        perform { try $0.writeCanvas(path: path, board: board) }
    }

    /// Makes an empty canvas in `folder`; its path.
    func createCanvas(in folder: String = "") -> String? {
        let made = perform { try $0.createCanvas(folder: folder, name: String(localized: "Untitled")) }
        reloadList()
        return made
    }

    /// Keeps a drafted mind map as a new note in `folder` and opens it.
    func createMap(titled title: String, text: String, in folder: String = "") {
        saveNow()
        guard let created = perform({ try $0.createNote(folder: folder, title: title) }) else { return }
        reloadList()
        open(created.path)
        self.text = text
    }

    /// Makes an empty drawing in `folder`; its path.
    func createDrawing(in folder: String = "") -> String? {
        let made = perform { try $0.createDrawing(folder: folder, name: String(localized: "Drawing"), paper: "blank") }
        reloadList()
        return made
    }

    /// Keeps a recording (or another file the app made) in `folder`; its
    /// path, for `![[…]]`.
    func addFile(in folder: String, name: String, data: Data) -> String? {
        let made = perform { try $0.addFile(folder: folder, name: name, data: data) }
        reloadList()
        return made
    }

    /// Makes an empty draw.io diagram (`.drawio.svg`) in `folder`; its path.
    func createDiagram(in folder: String = "") -> String? {
        let made = perform { try $0.createDiagram(folder: folder, name: String(localized: "Diagram")) }
        reloadList()
        return made
    }

    /// A note's text, for showing it on a canvas.
    func noteText(_ path: String) -> String? {
        perform { try $0.readNote(path: path) }
    }

    func open(_ next: String) {
        saveNow()
        guard let content = perform({ try $0.readNote(path: next) }) else { return }
        loading = true
        path = next
        text = content
        loading = false
        version += 1
        outline = perform { try $0.outline(path: next) }
    }

    private func scheduleSave() {
        saveTask?.cancel()
        saveTask = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(600))
            guard !Task.isCancelled else { return }
            self?.saveNow()
        }
    }

    func saveNow() {
        saveTask?.cancel()
        saveTask = nil
        guard let path, edited else { return }
        edited = false
        let text = text
        if perform({ try $0.writeNote(path: path, text: text) }) != nil {
            changes += 1
            outline = perform { try $0.outline(path: path) }
        }
    }

    /// The note as HTML for the reading view.
    func renderedHTML() -> String {
        guard let path else { return "" }
        let text = text
        return perform { try $0.render(path: path, text: text) } ?? ""
    }

    func createNote(in folder: String = "", title: String = String(localized: "Untitled")) {
        saveNow()
        guard let created = perform({ try $0.createNote(folder: folder, title: title) }) else { return }
        reloadList()
        open(created.path)
    }

    /// Creates a mind map (a note with a `mindmap` property) and opens it.
    func createMindMap(in folder: String = "") {
        saveNow()
        guard let created = perform({ try $0.createNote(folder: folder, title: String(localized: "Mind Map")) })
        else { return }
        reloadList()
        open(created.path)
        text = newMindmapText(title: title)
    }

    func createFolder(_ name: String) {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        perform { try $0.createFolder(path: name) }
        reloadList()
    }

    /// Renames the open note and updates links to it.
    func rename(to newTitle: String) {
        guard let path else { return }
        let name = newTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name != title else { return }
        saveNow()
        let folder = (path as NSString).deletingLastPathComponent
        let next = folder.isEmpty ? "\(name).md" : "\(folder)/\(name).md"
        guard perform({ try $0.renameNote(path: path, newPath: next) }) != nil else { return }
        reloadList()
        open(next)
    }

    /// Moves the open note to the vault's `.trash`.
    func deleteNote() {
        guard let path else { return }
        saveTask?.cancel()
        perform { try $0.deleteNote(path: path) }
        self.path = nil
        loading = true
        text = ""
        loading = false
        outline = nil
        reloadList()
        if let first = notes.first { open(first.path) }
    }

    /// Makes (once) a literature note for an item and opens it.
    func createLiteratureNote(itemID: String) {
        saveNow()
        guard let note = perform({ try $0.createLiteratureNote(itemId: itemID) }) else { return }
        reloadList()
        open(note.path)
    }

    /// Picks up edits made by other apps.
    func refresh() {
        guard saveTask == nil, perform({ try $0.refresh() }) == true else { return }
        reloadList()
        if let path { open(path) }
    }

    /// Opens the note a link names, or creates it if there is none.
    func openTarget(_ target: String) {
        guard let path else { return }
        let name = target.components(separatedBy: "#").first ?? target
        guard !name.isEmpty else { return }
        if let found = perform({ try $0.resolve(target: name, from: path) }) ?? nil {
            open(found)
        } else {
            createNote(title: name)
        }
    }

    func switcher(_ query: String) -> [NoteInfo] {
        perform { try $0.switcher(query: query, limit: 30) } ?? []
    }

    /// Lines of notes that link to an item or embed its highlights. Quiet
    /// on failure: views ask for these while they draw.
    func mentions(ofItem itemID: String) -> [NoteMention] {
        (try? vault?.itemMentions(itemId: itemID)) ?? []
    }

    func search(_ query: String) -> [NoteMention] {
        perform { try $0.search(query: query, limit: 100) } ?? []
    }

    /// Notes and items the open note is about too but does not link to yet.
    func linkSuggestions() -> [LinkSuggestion] {
        guard let path else { return [] }
        return perform { try $0.linkSuggestions(path: path, limit: 8) } ?? []
    }

    func linkCompletions(_ query: String) -> [LinkCompletion] {
        guard let path else { return [] }
        return perform { try $0.linkCompletions(from: path, query: query, limit: 20) } ?? []
    }

    /// A vault file on disk (embedded images).
    func fileURL(_ vaultPath: String) -> URL? {
        perform { try $0.filePath(path: vaultPath) }.map { URL(fileURLWithPath: $0) }
    }
}

/// Properties, templates and daily notes, earlier versions, the graph.
extension VaultModel {
    /// The date and time here, for templates and daily notes.
    static func localNow(_ date: Date = .now) -> LocalTime {
        let c = Calendar.current.dateComponents([.year, .month, .day, .hour, .minute, .second], from: date)
        return LocalTime(
            year: Int32(c.year ?? 1970), month: UInt32(c.month ?? 1), day: UInt32(c.day ?? 1),
            hour: UInt32(c.hour ?? 0), minute: UInt32(c.minute ?? 0), second: UInt32(c.second ?? 0))
    }

    /// Opens today's daily note, made from the daily template the first time.
    func openToday() {
        saveNow()
        guard let note = perform({ try $0.dailyNote(now: Self.localNow()) }) else { return }
        reloadList()
        open(note.path)
    }

    /// The notes in the templates folder. Quiet on failure: menus ask while
    /// they draw.
    func templates() -> [NoteInfo] {
        (try? vault?.templates()) ?? []
    }

    /// A new note from a template, next to the open one.
    func createNote(from template: NoteInfo) {
        saveNow()
        let folder = path.map { ($0 as NSString).deletingLastPathComponent } ?? ""
        let made = perform {
            try $0.createNoteFromTemplate(
                folder: folder, title: String(localized: "Untitled"), template: template.path, now: Self.localNow())
        }
        guard let made else { return }
        reloadList()
        open(made.path)
    }

    /// A template filled in for the open note, to put into it.
    func templateText(_ template: NoteInfo) -> String? {
        let title = title
        return perform { try $0.templateText(template: template.path, title: title, now: Self.localNow()) }
    }

    /// Writes the open note's properties (front matter) and shows it again.
    func setProperties(_ properties: [NoteProperty]) {
        saveNow()
        guard let path else { return }
        perform { try $0.setProperties(path: path, properties: properties) }
        open(path)
    }

    func settings() -> VaultSettings? {
        perform { try $0.settings() }
    }

    func save(settings: VaultSettings) {
        perform { try $0.setSettings(settings: settings) }
    }

    /// The open note as a Word document (citations formatted, references at
    /// the end), in the temporary folder for the person to save.
    func wordDocument() -> URL? {
        saveNow()
        guard let path else { return nil }
        let url = FileManager.default.temporaryDirectory.appending(path: "\(title).docx")
        let made = perform { try $0.exportDocx(path: path, destination: url.path(percentEncoded: false)) }
        return made == nil ? nil : url
    }

    /// The open note as a LaTeX document (biblatex citations, their BibTeX
    /// inside), in the temporary folder for the person to save.
    func latexDocument() -> URL? {
        saveNow()
        guard let path else { return nil }
        let url = FileManager.default.temporaryDirectory.appending(path: "\(title).tex")
        let made = perform { try $0.exportLatex(path: path, destination: url.path(percentEncoded: false)) }
        return made == nil ? nil : url
    }

    /// The open mind map as another app's file (`xmind`, `opml`, `mm`) or
    /// an SVG picture, in the temporary folder for the person to save.
    func mindMapFile(_ format: String) throws -> URL {
        saveNow()
        let url = FileManager.default.temporaryDirectory.appending(path: "\(title).\(format)")
        try exportMindmap(text: text, title: title, destination: url.path(percentEncoded: false))
        return url
    }

    /// Makes a mind map note from an XMind, OPML or FreeMind file, beside
    /// the open note, and opens it.
    func importMindMap(from url: URL) throws {
        guard let vault else { return }
        saveNow()
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let folder = path.map { ($0 as NSString).deletingLastPathComponent } ?? ""
        let made = try vault.importMindmap(folder: folder, source: url.path(percentEncoded: false))
        reloadList()
        open(made.path)
    }

    /// The open note's earlier versions, newest first.
    func versions() -> [NoteVersion] {
        guard let path else { return [] }
        return perform { try $0.noteVersions(path: path) } ?? []
    }

    /// What changed from a version to the note now.
    func compare(with version: NoteVersion) -> [DiffLine] {
        guard let path else { return [] }
        return perform { try $0.compareVersion(path: path, time: version.time) } ?? []
    }

    /// Puts a version back; what the note says now is kept as a version.
    func restore(_ version: NoteVersion) {
        saveNow()
        guard let path else { return }
        perform { try $0.restoreVersion(path: path, time: version.time) }
        open(path)
    }

    /// The graph of notes, laid out, off the main thread.
    func graph(around center: String?, depth: UInt32) async -> NoteGraph? {
        guard let vault else { return nil }
        do {
            return try await Task.detached { try vault.graph(center: center, depth: depth) }.value
        } catch {
            errorMessage = CoreKit.message(for: error)
            return nil
        }
    }
}

/// A line (and the heading `id` on it, if any) to bring into view.
struct NoteJump: Equatable {
    let id = UUID()
    var line: Int
    var anchor: String?
}
