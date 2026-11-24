import CoreFoundation
import CoreKit
import Observation
import SwiftUI
import UniformTypeIdentifiers
import WebKit

/// A plugin to run: an Obsidian one, keyed by its id, or a Zotero one,
/// keyed by `zotero:` and its id.
struct PluginRef: Hashable {
    let key: String
    let id: String
    let name: String
    let version: String
    let zotero: Bool
    var obsidian: ObsidianPlugin?
}

/// A section a Zotero plugin adds to the item pane.
struct PluginSection: Identifiable, Hashable {
    let plugin: String
    let paneID: String
    let label: String
    var id: String { "\(plugin):\(paneID)" }
}

/// A column a Zotero plugin adds to the item list.
struct PluginColumn: Identifiable, Hashable {
    let plugin: String
    let key: String
    let label: String
    var id: String { "\(plugin):\(key)" }
}

struct ReaderRender: Sendable {
    let plugin: String
    let html: String
}

struct ReaderMenuItem: Identifiable, Hashable, Sendable {
    let index: Int
    let label: String
    let disabled: Bool
    var id: Int { index }
}

struct ReaderMenu: Sendable {
    let plugin: String
    let menu: Int
    let items: [ReaderMenuItem]
}

enum PluginItemMenuEntry: Sendable {
    case command(ReaderMenuItem)
    case submenu(label: String, disabled: Bool, children: [PluginItemMenuEntry])
    case separator
}

struct PluginItemMenu: Sendable {
    let plugin: String
    let menu: Int
    let entries: [PluginItemMenuEntry]
}

/// What a plugin adds to the note editor's menu (Obsidian's `editor-menu`),
/// for the editor state it was asked about.
struct PluginEditorMenu: Sendable {
    let plugin: String
    let request: Int
    let identity: EditorIdentity
    let entries: [PluginItemMenuEntry]
}

/// A command a plugin added.
struct PluginCommand: Identifiable, Hashable {
    let id: String
    let name: String
    let plugin: String
}

struct PluginNotice: Identifiable, Hashable {
    let id: Int
    let text: String
}

/// Which state of the open note something refers to: its editor session,
/// the note opened in it, the text and the selection.
struct EditorIdentity: Equatable, Sendable {
    let session: String
    let generation: Int
    let revision: Int
    let contextRevision: Int

    init(session: String, generation: Int, revision: Int, contextRevision: Int) {
        self.session = session
        self.generation = generation
        self.revision = revision
        self.contextRevision = contextRevision
    }

    init?(_ message: [String: Any]) {
        guard let session = message["session"] as? String, let generation = message["generation"] as? Int,
              let revision = message["revision"] as? Int, let contextRevision = message["contextRevision"] as? Int
        else { return nil }
        self.init(session: session, generation: generation, revision: revision, contextRevision: contextRevision)
    }
}

/// Suggestions a plugin offers while typing (an Obsidian `EditorSuggest`).
struct PluginSuggestion: Equatable {
    struct Item: Identifiable, Equatable {
        let id: String
        let label: String
        let secondary: String?
    }

    let plugin: String
    let request: Int
    let identity: EditorIdentity
    let priority: Double
    let items: [Item]
    var selected: Int
}

/// A plugin that is running, and how it is doing.
struct RunningPlugin: Identifiable, Hashable {
    let id: String
    let name: String
    var ready = false
    var hasSettings = false
    /// What it uses that Paperloom does not offer yet.
    var partial: [String] = []
    var error: String?
}

/// A plugin asking to reach a site, waiting for the person.
struct SiteRequest: Identifiable {
    let id = UUID()
    let plugin: String
    let site: String
    let answer: (Bool) -> Void
}

/// Serves `plugin://localhost/…`: the runtime from the app's bundle (shared
/// with the Tauri app) and the page that frames it.
final class PluginFileHandler: NSObject, WKURLSchemeHandler {
    /// The plugin in a sandboxed frame (an opaque origin, apart from the app);
    /// its messages go to the app, the app's to it.
    static func page(_ runtime: String) -> String {
        """
        <!doctype html><html><head><meta charset="utf-8">
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <style>html, body, iframe { margin: 0; width: 100%; height: 100%; border: 0; }</style>
        </head><body><iframe id="plugin" sandbox="allow-scripts" src="\(runtime)"></iframe>
        <script>
        const frame = document.getElementById("plugin");
        window.addEventListener("message", (event) => {
          if (event.source === frame.contentWindow) {
            window.webkit.messageHandlers.plugin.postMessage(JSON.stringify(event.data));
          }
        });
        window.paperloomSend = (message) => frame.contentWindow.postMessage(message, "*");
        </script></body></html>
        """
    }

    func webView(_ webView: WKWebView, start task: any WKURLSchemeTask) {
        guard let url = task.request.url else { return }
        let name = String(url.path(percentEncoded: false).drop { $0 == "/" })
        let data: Data?
        switch name {
        case "frame.html":
            data = Data(Self.page("host.html").utf8)
        case "zotero-frame.html":
            data = Data(Self.page("zotero.html").utf8)
        case "translate-frame.html":
            data = Data(Self.page("translate.html").utf8)
        case "host.html", "cm6.js", "obsidian.js", "obsidian.css", "zotero.html", "zotero.js", "translate.html", "translate.js":
            data = Bundle.main.url(forResource: name, withExtension: nil, subdirectory: "plugin-runtime")
                .flatMap { try? Data(contentsOf: $0) }
        default:
            data = nil
        }
        guard let data else {
            task.didFailWithError(URLError(.fileDoesNotExist))
            return
        }
        let type = UTType(filenameExtension: (name as NSString).pathExtension)?.preferredMIMEType ?? "text/plain"
        task.didReceive(URLResponse(url: url, mimeType: type, expectedContentLength: data.count, textEncodingName: "utf-8"))
        task.didReceive(data)
        task.didFinish()
    }

    func webView(_ webView: WKWebView, stop task: any WKURLSchemeTask) {}
}

/// This runner manages a sandboxed runtime's web view and its message relay.
@MainActor
class PluginFrameRunner: NSObject, WKScriptMessageHandler {
    let webView: WKWebView

    init(page: String, isolated: Bool = false) {
        let configuration = WKWebViewConfiguration()
        if isolated { configuration.websiteDataStore = .nonPersistent() }
        configuration.setURLSchemeHandler(PluginFileHandler(), forURLScheme: "plugin")
        webView = WKWebView(frame: .zero, configuration: configuration)
        super.init()
        configuration.userContentController.add(self, name: "plugin")
        if let url = URL(string: "plugin://localhost/\(page)") {
            webView.load(URLRequest(url: url))
        }
    }

    func send(_ object: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: object),
              let json = String(data: data, encoding: .utf8)
        else { return }
        webView.evaluateJavaScript("paperloomSend(\(json))")
    }

    func stop() {
        webView.stopLoading()
        webView.configuration.userContentController.removeScriptMessageHandler(forName: "plugin")
    }

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        guard message.frameInfo.isMainFrame, let text = message.body as? String,
              let object = try? JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any]
        else { return }
        receive(object)
    }

    func receive(_ object: [String: Any]) {}
}

/// This runner relays messages between one plugin and its host.
@MainActor
final class PluginRunner: PluginFrameRunner {
    let plugin: PluginRef
    weak var host: PluginHost?
    var starting = false
    var started = false
    var ready = false
    var failed = false
    var startupTimer: Task<Void, Never>?
    var pendingItems: PluginItemSnapshot?
    var pendingEvents: [PluginItemEvent] = []
    var pendingEventKeys = 0
    var eventsOverflowed = false
    /// The latest tabs state and the file opens that wait for ready.
    var pendingTabs: [String: Any]?
    var pendingOpens: [[String: Any]] = []

    init(plugin: PluginRef, host: PluginHost) {
        self.plugin = plugin
        self.host = host
        super.init(page: plugin.zotero ? "zotero-frame.html" : "frame.html")
    }

    override func receive(_ object: [String: Any]) {
        host?.receive(object, from: self)
    }

    override func stop() {
        failed = true
        ready = false
        startupTimer?.cancel()
        startupTimer = nil
        clearPendingContext()
        send(["type": "stop"])
        super.stop()
    }

    func clearPendingContext() {
        pendingItems = nil
        pendingEvents = []
        pendingEventKeys = 0
        eventsOverflowed = false
        pendingTabs = nil
        pendingOpens = []
    }
}

/// Runs the vault's turned-on Obsidian plugins, each in its own web view,
/// and does what they ask of the vault, the open note and the web, or
/// refuses. A plugin reaches only the sites the person allowed for it.
@MainActor
@Observable
final class PluginHost {
    private(set) var running: [RunningPlugin] = []
    private(set) var commands: [PluginCommand] = []
    private(set) var notices: [PluginNotice] = []
    private(set) var status: [String: String] = [:]
    private(set) var sections: [PluginSection] = []
    private(set) var columns: [PluginColumn] = []
    private(set) var readerEvents: [String: [String]] = [:]
    private(set) var selectedItemKeys: [String] = []
    /// Live layouts register separately; focus order chooses the navigation target.
    @ObservationIgnored private var selectionHandlers: [UUID: @MainActor ([String]) throws -> Void] = [:]
    @ObservationIgnored private var selectionFocus: [UUID] = []
    @ObservationIgnored private var tabHandlers: [UUID: @MainActor (String) throws -> Void] = [:]
    /// PDF tabs per window, as attachment keys and the one in front.
    @ObservationIgnored private var windowReaders: [UUID: (keys: [String], selected: String?)] = [:]
    /// Reader views of the focused window, with ids that last while the tab is open.
    @ObservationIgnored private var readers: [PluginReaderView] = []
    @ObservationIgnored private var selectedViewID = "zotero-pane"
    @ObservationIgnored private var openedViews: Set<String> = []
    @ObservationIgnored private var openedInstances: Set<ObjectIdentifier> = []
    @ObservationIgnored private var lastTabs = ""
    @ObservationIgnored private var tabsTimer: Task<Void, Never>?
    /// The counter changes when plugins register note renderers or stop.
    private(set) var drawing = 0
    private static let readerTypes = [
        "renderTextSelectionPopup", "renderSidebarAnnotationHeader", "renderToolbar",
        "createAnnotationContextMenu", "createViewContextMenu",
    ]
    /// The plugin whose settings or modal is on show.
    var showing: String?
    var siteRequest: SiteRequest?
    @ObservationIgnored private var runners: [String: PluginRunner] = [:]
    @ObservationIgnored private weak var vault: VaultModel?
    @ObservationIgnored private weak var library: LibraryModel?
    @ObservationIgnored private var noticeID = 1
    /// The code block languages plugins draw, and which plugin draws each.
    @ObservationIgnored private var codeLanguages: [String: String] = [:]
    /// Plugins that change notes as they are shown (Markdown post
    /// processors), in the order they asked to.
    @ObservationIgnored private var postProcessors: [String] = []
    @ObservationIgnored private var renders: [Int: CheckedContinuation<String?, Never>] = [:]
    /// The library revision the Zotero plugins last heard about.
    @ObservationIgnored private var itemsRevision: Int?
    @ObservationIgnored private var itemBaseline: PluginItemSnapshot?
    @ObservationIgnored private var contextSeq = 0
    @ObservationIgnored private var itemTimer: Task<Void, Never>?
    @ObservationIgnored private var fetchingItems = false
    @ObservationIgnored private var itemsDirty = false
    @ObservationIgnored private var itemWaiters: [CheckedContinuation<Void, Never>] = []
    @ObservationIgnored private var itemOrigins: [String: Set<String>] = [:]
    @ObservationIgnored private var itemWrites: Task<Void, Error>?
    /// A synchronous plugin commit racing an asynchronous read invalidates that read.
    @ObservationIgnored private var itemCommitRevision = 0
    @ObservationIgnored private var writeHistory: [String: (at: Date, plugins: [String])] = [:]
    private static let writeHistoryLimit = 2_000
    @ObservationIgnored private var selectionTimer: Task<Void, Never>?
    @ObservationIgnored private var columnRequests: [Int: (plugin: String, continuation: CheckedContinuation<[String: String], Never>)] = [:]
    @ObservationIgnored private var readerRenders: [Int: (plugin: String, continuation: CheckedContinuation<String, Never>)] = [:]
    @ObservationIgnored private var readerMenus: [Int: (plugin: String, continuation: CheckedContinuation<[ReaderMenuItem], Never>)] = [:]
    @ObservationIgnored private var itemMenus: [Int: (plugin: String, continuation: CheckedContinuation<[PluginItemMenuEntry], Never>)] = [:]
    @ObservationIgnored private var renderID = 1
    /// The open note as plugins' editor sees it: one session for the app's
    /// run, a generation per note opened, a revision per text change and a
    /// context revision per text or selection change. Plugin edits name the
    /// state they were made for and are refused when it has moved on.
    @ObservationIgnored private let editorSession = "apple-editor-\(UUID().uuidString)"
    @ObservationIgnored private var editorGeneration = 0
    @ObservationIgnored private var editorRevision = 0
    @ObservationIgnored private var editorContextRevision = 0
    @ObservationIgnored private var editorVersion: Int?
    @ObservationIgnored private var editorPath: String?
    @ObservationIgnored private var editorText = ""
    @ObservationIgnored private var editorRange: Range<Int>?
    @ObservationIgnored private var editorOpen = false
    @ObservationIgnored private var editorFlush: Task<Void, Never>?
    @ObservationIgnored private var editorRequest = 0
    @ObservationIgnored private var suggestPlugins: Set<String> = []
    @ObservationIgnored private var suggestQuery: (request: Int, identity: EditorIdentity, length: Int, pending: Set<String>,
                                                   results: [String: PluginSuggestion], timer: Task<Void, Never>)?
    /// The suggestions on show under the editor, if any.
    private(set) var suggestion: PluginSuggestion?
    /// Some plugin adds to the editor's menu.
    private(set) var hasEditorMenu = false
    @ObservationIgnored private var editorMenuPlugins: Set<String> = []
    @ObservationIgnored private var editorMenuRequests: [Int: (plugin: String, identity: EditorIdentity,
                                                               continuation: CheckedContinuation<[PluginItemMenuEntry], Never>)] = [:]

    func connect(to vault: VaultModel, library: LibraryModel) {
        self.vault = vault
        self.library = library
    }

    func runner(_ id: String) -> PluginRunner? { runners[id] }

    /// Two projections (JSON arrays) as one.
    nonisolated static func joinProjections(_ items: String, _ more: String) -> String {
        let rest = more.trimmingCharacters(in: .whitespacesAndNewlines)
        let first = items.trimmingCharacters(in: .whitespacesAndNewlines)
        guard rest.count > 2 else { return first }
        guard first.count > 2 else { return rest }
        return String(first.dropLast()) + "," + String(rest.dropFirst())
    }

    func registerSelectionHandler(for window: UUID, handler: @escaping @MainActor ([String]) throws -> Void) {
        selectionHandlers[window] = handler
    }

    func focusSelectionWindow(_ window: UUID) {
        guard selectionHandlers[window] != nil else { return }
        selectionFocus.removeAll { $0 == window }
        selectionFocus.append(window)
        tabsChanged()
    }

    func unregisterSelectionHandler(for window: UUID) {
        selectionHandlers[window] = nil
        tabHandlers[window] = nil
        windowReaders[window] = nil
        selectionFocus.removeAll { $0 == window }
        tabsChanged()
    }

    /// How a window brings a reader tab (by attachment key) or the library
    /// ("zotero-pane", passed as nil) to the front for a plugin.
    func registerTabHandler(for window: UUID, handler: @escaping @MainActor (String?) throws -> Void) {
        tabHandlers[window] = { viewID in
            try handler(viewID == "zotero-pane" ? nil : self.readers.first { $0.viewID == viewID }?.attachmentKey)
        }
    }

    /// A window's PDF tabs changed (attachment keys, and the one in front, if any).
    func readersChanged(in window: UUID, attachmentKeys: [String], selected: String?) {
        windowReaders[window] = (attachmentKeys, selected)
        tabsChanged()
    }

    private func tabsChanged() {
        resolveReaders()
        guard tabsTimer == nil else { return }
        tabsTimer = Task { @MainActor in
            do { try await Task.sleep(for: .milliseconds(16)) } catch { return }
            sendTabs()
        }
    }

    /// The focused window's PDF tabs whose attachment the plugins know.
    private func resolveReaders() {
        let window = selectionFocus.last.flatMap { windowReaders[$0] } ?? ([], nil)
        let old = readers
        readers = window.keys.compactMap { key in
            guard let parent = parentItemKey(ofAttachment: key) else { return nil }
            let viewID = old.first { $0.attachmentKey == key }?.viewID ?? UUID().uuidString
            return PluginReaderView(viewID: viewID, attachmentKey: key, parentItemKey: parent)
        }
        openedViews = openedViews.filter { id in readers.contains { $0.viewID == id } }
        selectedViewID = readers.first { $0.attachmentKey == window.selected }?.viewID ?? "zotero-pane"
    }

    private func parentItemKey(ofAttachment key: String) -> String? {
        itemBaseline?.byKey.first { _, item in
            (item["attachments"] as? [[String: Any]])?.contains { $0["key"] as? String == key } ?? false
        }?.key
    }

    private var tabContext: [String: Any] {
        ["readers": readers.map(\.message), "selectedViewID": selectedViewID]
    }

    private func sendTabs() {
        tabsTimer?.cancel()
        tabsTimer = nil
        let state = readers.map { "\($0.viewID)|\($0.attachmentKey)|\($0.parentItemKey)" }.joined(separator: ",") + "#" + selectedViewID
        guard state != lastTabs else { return }
        lastTabs = state
        contextSeq += 1
        var message = tabContext
        message["type"] = "tabs"
        message["seq"] = contextSeq
        for runner in runners.values where runner.plugin.zotero && !runner.failed {
            if runner.ready { runner.send(message) } else { runner.pendingTabs = message }
        }
    }

    /// A PDF reader showed its document. Each reader instance counts once;
    /// a tab keeps one open instance across redraws and page changes.
    func readerOpened(attachmentKey: String, instance: ObjectIdentifier) async {
        if parentItemKey(ofAttachment: attachmentKey) == nil {
            itemsChanged()
            await itemsSettled()
            resolveReaders()
        }
        guard parentItemKey(ofAttachment: attachmentKey) != nil else { return }
        if let reader = readers.first(where: { $0.attachmentKey == attachmentKey }) {
            guard openedViews.insert(reader.viewID).inserted else { return }
            sendTabs()
        } else {
            // A reader outside the tabs (the phone's pushed reader).
            guard openedInstances.insert(instance).inserted else { return }
            if openedInstances.count > 200 { openedInstances = [instance] }
        }
        sendFileOpen(attachmentKey)
    }

    /// A file shown outside the reader (Quick Look); each successful open counts.
    func fileOpened(attachmentKey: String) async {
        if parentItemKey(ofAttachment: attachmentKey) == nil {
            itemsChanged()
            await itemsSettled()
        }
        guard parentItemKey(ofAttachment: attachmentKey) != nil else { return }
        sendFileOpen(attachmentKey)
    }

    private func sendFileOpen(_ attachmentKey: String) {
        contextSeq += 1
        let message: [String: Any] = [
            "type": "notifier", "seq": contextSeq,
            "events": [["event": "open", "objectType": "file", "itemKeys": [attachmentKey]] as [String: Any]],
        ]
        for runner in runners.values where runner.plugin.zotero && !runner.failed {
            if runner.ready {
                runner.send(message)
            } else if runner.pendingOpens.count < 50 {
                runner.pendingOpens.append(message)
            }
        }
    }

    private func selectItems(_ keys: [String]) throws {
        guard let window = selectionFocus.last, let handler = selectionHandlers[window] else {
            throw PluginItemError.selectionUnavailable
        }
        try handler(keys)
    }

    /// Starts the turned-on plugins and stops the others.
    func reload() {
        #if os(iOS)
        let mobile = true
        #else
        let mobile = false
        #endif
        let obsidian = ((try? vault?.core?.obsidianPlugins()) ?? [])
            .filter { $0.enabled && !($0.isDesktopOnly && mobile) }
            .map { PluginRef(key: $0.id, id: $0.id, name: $0.name, version: $0.version, zotero: false, obsidian: $0) }
        let zotero = ((try? library?.core?.zoteroPlugins()) ?? [])
            .filter(\.enabled)
            .map { PluginRef(key: "zotero:\($0.id)", id: $0.id, name: $0.name, version: $0.version, zotero: true) }
        let wanted = obsidian + zotero
        let keys = Set(wanted.map(\.key))
        for key in runners.keys where !keys.contains(key) {
            stop(key)
        }
        for plugin in wanted where runners[plugin.key] == nil {
            let runner = PluginRunner(plugin: plugin, host: self)
            if plugin.zotero { runner.pendingItems = itemBaseline }
            runners[plugin.key] = runner
            running.append(RunningPlugin(id: plugin.key, name: plugin.name))
            runner.startupTimer = Task { @MainActor [weak self, weak runner] in
                do { try await Task.sleep(for: .seconds(30)) } catch { return }
                guard let self, let runner else { return }
                self.failStartup(runner, message: "Plugin startup timed out after 30 seconds.")
            }
        }
    }

    /// A Zotero plugin's item pane section for an item (HTML), or nil.
    func renderSection(_ section: PluginSection, item: String) async -> String? {
        // A plugin registers its sections while it starts, before it has the
        // library's items; drawn then, a section would come back empty. Wait
        // for it to be ready or to fail (startup gives up after 30 seconds).
        var waited = 0
        while let starting = runners[section.plugin], !starting.ready, !starting.failed, waited < 310 {
            do { try await Task.sleep(for: .milliseconds(100)) } catch { return nil }
            waited += 1
        }
        await itemsSettled()
        guard let runner = runners[section.plugin], runner.ready else { return nil }
        let request = renderID
        renderID += 1
        return await withCheckedContinuation { continuation in
            renders[request] = continuation
            runner.send(["type": "render-section", "id": request, "paneID": section.paneID, "itemKey": item])
            Task {
                try? await Task.sleep(for: .seconds(5))
                renders.removeValue(forKey: request)?.resume(returning: nil)
            }
        }
    }

    /// A Zotero plugin's column values for a batch of items, as plain text.
    func columnValues(_ column: PluginColumn, items: [String]) async -> [String: String] {
        guard let runner = runners[column.plugin] else { return [:] }
        let request = renderID
        renderID += 1
        return await withCheckedContinuation { continuation in
            columnRequests[request] = (plugin: column.plugin, continuation: continuation)
            runner.send(["type": "column-values", "id": request, "key": column.key, "itemKeys": items])
            Task {
                try? await Task.sleep(for: .seconds(5))
                columnRequests.removeValue(forKey: request)?.continuation.resume(returning: [:])
            }
        }
    }

    /// Asks the listening Zotero plugins for HTML to convert to plain text.
    func readerRender(_ event: String, attachmentKey: String, itemKey: String?, params: [String: Any]) async -> [ReaderRender] {
        let tasks = readerPlugins(event).map { runner in
            Task { @MainActor in
                let plugin = runner.plugin.key
                guard runners[plugin] === runner else { return ReaderRender(plugin: plugin, html: "") }
                let request = renderID
                renderID += 1
                let html: String = await withCheckedContinuation { continuation in
                    readerRenders[request] = (plugin: plugin, continuation: continuation)
                    runner.send([
                        "type": "reader-render", "id": request, "event": event, "attachmentKey": attachmentKey,
                        "itemKey": itemKey.map { $0 as Any } ?? NSNull(), "params": params,
                    ])
                    Task {
                        try? await Task.sleep(for: .seconds(5))
                        readerRenders.removeValue(forKey: request)?.continuation.resume(returning: "")
                    }
                }
                return ReaderRender(plugin: plugin, html: html)
            }
        }
        var results: [ReaderRender] = []
        for task in tasks { results.append(await task.value) }
        return results
    }

    /// Asks the listening Zotero plugins for menu labels and command indexes.
    func readerMenu(_ event: String, attachmentKey: String, itemKey: String?, params: [String: Any]) async -> [ReaderMenu] {
        let tasks = readerPlugins(event).map { runner in
            Task { @MainActor in
                let plugin = runner.plugin.key
                guard runners[plugin] === runner else { return ReaderMenu(plugin: plugin, menu: 0, items: []) }
                let request = renderID
                renderID += 1
                let items: [ReaderMenuItem] = await withCheckedContinuation { continuation in
                    readerMenus[request] = (plugin: plugin, continuation: continuation)
                    runner.send([
                        "type": "reader-menu", "id": request, "event": event, "attachmentKey": attachmentKey,
                        "itemKey": itemKey.map { $0 as Any } ?? NSNull(), "params": params,
                    ])
                    Task {
                        try? await Task.sleep(for: .seconds(5))
                        readerMenus.removeValue(forKey: request)?.continuation.resume(returning: [])
                    }
                }
                return ReaderMenu(plugin: plugin, menu: request, items: items)
            }
        }
        var results: [ReaderMenu] = []
        for task in tasks { results.append(await task.value) }
        return results
    }

    func readerCommand(plugin: String, menu: Int, index: Int) {
        guard let runner = runners[plugin], runner.plugin.zotero else { return }
        runner.send(["type": "reader-command", "menu": menu, "index": index])
    }

    /// Asks every running Zotero plugin for its item menu.
    func itemMenu(itemKeys: [String]) async -> [PluginItemMenu] {
        let tasks: [Task<PluginItemMenu, Never>] = running.compactMap { runners[$0.id] }.filter(\.plugin.zotero).map { runner in
            Task { @MainActor in
                let plugin = runner.plugin.key
                guard runners[plugin] === runner else { return PluginItemMenu(plugin: plugin, menu: 0, entries: []) }
                let request = renderID
                renderID += 1
                let entries: [PluginItemMenuEntry] = await withCheckedContinuation { continuation in
                    itemMenus[request] = (plugin: plugin, continuation: continuation)
                    runner.send(["type": "item-menu", "id": request, "itemKeys": itemKeys])
                    Task {
                        try? await Task.sleep(for: .seconds(2))
                        itemMenus.removeValue(forKey: request)?.continuation.resume(returning: [])
                    }
                }
                return PluginItemMenu(plugin: plugin, menu: request, entries: entries)
            }
        }
        var results: [PluginItemMenu] = []
        for task in tasks { results.append(await task.value) }
        return results
    }

    func itemCommand(plugin: String, menu: Int, index: Int, path: [String], itemKeys: [String]) {
        guard let runner = runners[plugin], runner.plugin.zotero else { return }
        runner.send(["type": "item-command", "menu": menu, "index": index, "path": path, "itemKeys": itemKeys])
    }

    private static func menuInteger(_ value: Any?) -> Int? {
        guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID() else { return nil }
        let value = number.doubleValue
        guard value.isFinite, value.rounded(.towardZero) == value, abs(value) <= 9_007_199_254_740_991 else { return nil }
        return Int(value)
    }

    private static func menuBoolean(_ value: Any?) -> Bool? {
        guard let number = value as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() else { return nil }
        return number.boolValue
    }

    private static func itemMenuEntries(_ value: Any?) -> [PluginItemMenuEntry] {
        var count = 0
        var indexes = Set<Int>()
        let whitespace = CharacterSet(charactersIn:
            "\t\n\u{000B}\u{000C}\r \u{00A0}\u{1680}"
                + "\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200A}"
                + "\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}")
        func read(_ value: Any?, nested: Bool = false) -> [PluginItemMenuEntry]? {
            guard let items = value as? [Any], items.count <= 50 else { return nil }
            var entries: [PluginItemMenuEntry] = []
            for value in items {
                count += 1
                guard count <= 50, let item = value as? [String: Any] else { return nil }
                if menuBoolean(item["separator"]) == true {
                    guard item["label"] == nil, item["index"] == nil, item["children"] == nil else { return nil }
                    entries.append(.separator)
                    continue
                }
                guard let label = item["label"] as? String, !label.trimmingCharacters(in: whitespace).isEmpty,
                      let disabled = menuBoolean(item["disabled"])
                else { return nil }
                let title = String(decoding: label.utf16.prefix(500), as: UTF16.self)
                if let value = item["children"] {
                    guard !nested, item["index"] == nil, let children = read(value, nested: true) else { return nil }
                    if !children.isEmpty { entries.append(.submenu(label: title, disabled: disabled, children: children)) }
                } else {
                    guard let index = menuInteger(item["index"]), index >= 0, indexes.insert(index).inserted else { return nil }
                    entries.append(.command(ReaderMenuItem(index: index, label: title, disabled: disabled)))
                }
            }
            return entries
        }
        return read(value) ?? []
    }

    private func readerPlugins(_ event: String) -> [PluginRunner] {
        readerEvents.keys.sorted().compactMap { plugin in
            guard readerEvents[plugin]?.contains(event) == true, let runner = runners[plugin], runner.plugin.zotero
            else { return nil }
            return runner
        }
    }

    /// Tells the Zotero plugins about the library as of this revision, once.
    func itemsChanged(revision: Int) {
        guard revision != itemsRevision else { return }
        itemsRevision = revision
        itemsChanged()
    }

    /// Invalidation only: one complete-projection read after a 50 ms burst.
    func itemsChanged() {
        itemsDirty = true
        guard !fetchingItems, itemTimer == nil else { return }
        itemTimer = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(50))
            itemTimer = nil
            await refreshItems()
        }
    }

    /// Menus, columns, startup and saves wait for the same coordinator.
    func itemsSettled() async {
        guard itemsDirty || fetchingItems || itemTimer != nil else { return }
        await withCheckedContinuation { itemWaiters.append($0) }
    }

    private func refreshItems() async {
        fetchingItems = true
        while itemsDirty {
            itemsDirty = false
            do {
                guard let books = library?.core else { throw PluginItemError.unavailable }
                let revision = itemCommitRevision
                // Only the Sendable JSON string crosses back to the main actor.
                let notes = vault?.core
                let json = try await Task.detached {
                    // Notes with a lasting identity are Zotero note items too.
                    try Self.joinProjections(books.zoteroItems(), notes?.zoteroNotes() ?? "[]")
                }.value
                guard revision == itemCommitRevision else {
                    itemsDirty = true
                    continue
                }
                let snapshot = try PluginItemSnapshot(json: json)
                let events = snapshot.events(since: itemBaseline)
                let origins = itemOrigins
                itemOrigins = [:]
                itemBaseline = snapshot
                pruneWriteHistory()
                tabsChanged()
                contextSeq += 1
                for runner in runners.values where runner.plugin.zotero && !runner.failed {
                    let delivered = events.compactMap { event -> PluginItemEvent? in
                        let keys = event.itemKeys.filter {
                            event.event != "modify" || !(origins[$0]?.contains(runner.plugin.key) ?? false)
                        }
                        return keys.isEmpty ? nil : event.restricted(to: keys)
                    }
                    if runner.ready {
                        runner.send(["type": "items", "items": snapshot.items, "seq": contextSeq, "events": delivered.map(\.message)])
                    } else {
                        runner.pendingItems = snapshot
                        guard !runner.eventsOverflowed else { continue }
                        let count = delivered.reduce(0) { $0 + $1.itemKeys.count }
                        if runner.pendingEventKeys + count > 2_000 {
                            // A partial event backlog is misleading. Keep only the latest snapshot.
                            runner.pendingEvents = []
                            runner.pendingEventKeys = 0
                            runner.eventsOverflowed = true
                        } else {
                            runner.pendingEvents.append(contentsOf: delivered)
                            runner.pendingEventKeys += count
                        }
                    }
                }
            } catch {
                // Keep both the last-good baseline and origins; never synthesize deletes.
                NSLog("Refreshing Zotero plugin items: %@", CoreKit.message(for: error))
            }
        }
        fetchingItems = false
        let waiters = itemWaiters
        itemWaiters = []
        for waiter in waiters { waiter.resume() }
    }

    func selectionChanged(itemKeys: [String]) {
        var seen = Set<String>()
        let keys = itemKeys.filter { seen.insert($0).inserted }
        guard keys != selectedItemKeys else { return }
        selectedItemKeys = keys
        guard selectionTimer == nil else { return }
        selectionTimer = Task { @MainActor in
            do { try await Task.sleep(for: .milliseconds(16)) } catch { return }
            sendSelection()
        }
    }

    private func sendSelection() {
        selectionTimer?.cancel()
        selectionTimer = nil
        contextSeq += 1
        let message: [String: Any] = ["type": "selection", "seq": contextSeq, "itemKeys": selectedItemKeys]
        // Startup may await pane.selectItems, so selection must never wait for ready.
        for runner in runners.values where runner.plugin.zotero && runner.started && !runner.failed {
            runner.send(message)
        }
    }

    /// A code block as the plugin for its language draws it, or nil.
    func renderCodeBlock(language: String, source: String, path: String) async -> String? {
        guard let plugin = codeLanguages[language], let runner = runners[plugin] else { return nil }
        let request = renderID
        renderID += 1
        return await withCheckedContinuation { continuation in
            renders[request] = continuation
            runner.send(["type": "render-code", "id": request, "language": language, "source": source, "path": path])
            Task {
                try? await Task.sleep(for: .seconds(5))
                renders.removeValue(forKey: request)?.resume(returning: nil)
            }
        }
    }

    /// A rendered note as the plugins' post processors change it, one
    /// plugin after another.
    func postProcess(_ html: String, path: String) async -> String {
        var html = html
        for plugin in postProcessors {
            guard let runner = runners[plugin] else { continue }
            let request = renderID
            renderID += 1
            let source = html
            let changed: String? = await withCheckedContinuation { continuation in
                renders[request] = continuation
                runner.send(["type": "post-process", "id": request, "html": source, "path": path])
                Task {
                    try? await Task.sleep(for: .seconds(5))
                    renders.removeValue(forKey: request)?.resume(returning: nil)
                }
            }
            if let changed { html = changed }
        }
        return html
    }

    /// A note's HTML with the code blocks plugins draw put in as they drew
    /// them.
    func drawCodeBlocks(in html: String, path: String) async -> String {
        guard !codeLanguages.isEmpty,
              let pattern = try? NSRegularExpression(
                  pattern: #"<pre><code class="language-([^"]+)">([\s\S]*?)</code></pre>"#)
        else { return html }
        var result = html
        let matches = pattern.matches(in: html, range: NSRange(html.startIndex..., in: html)).reversed()
        for match in matches {
            guard let whole = Range(match.range, in: html), let languageRange = Range(match.range(at: 1), in: html),
                  let sourceRange = Range(match.range(at: 2), in: html)
            else { continue }
            let language = String(html[languageRange])
            guard codeLanguages[language] != nil else { continue }
            let source = String(html[sourceRange])
                .replacingOccurrences(of: "&lt;", with: "<").replacingOccurrences(of: "&gt;", with: ">")
                .replacingOccurrences(of: "&quot;", with: "\"").replacingOccurrences(of: "&#39;", with: "'")
                .replacingOccurrences(of: "&amp;", with: "&")
            if let drawn = await renderCodeBlock(language: language, source: source, path: path),
               let target = Range(match.range, in: result), result[target] == html[whole]
            {
                result.replaceSubrange(target, with: "<div class=\"plugin-block\">\(drawn)</div>")
            }
        }
        return result
    }

    private func stop(_ id: String) {
        runners[id]?.stop()
        suggestPlugins.remove(id)
        if suggestion?.plugin == id { closeSuggestions() }
        editorMenuPlugins.remove(id)
        hasEditorMenu = !editorMenuPlugins.isEmpty
        for (request, pending) in editorMenuRequests where pending.plugin == id {
            editorMenuRequests.removeValue(forKey: request)?.continuation.resume(returning: [])
        }
        removeWriteHistory(for: id)
        codeLanguages = codeLanguages.filter { $0.value != id }
        postProcessors.removeAll { $0 == id }
        runners[id] = nil
        for (request, pending) in columnRequests where pending.plugin == id {
            columnRequests.removeValue(forKey: request)?.continuation.resume(returning: [:])
        }
        for (request, pending) in readerRenders where pending.plugin == id {
            readerRenders.removeValue(forKey: request)?.continuation.resume(returning: "")
        }
        for (request, pending) in readerMenus where pending.plugin == id {
            readerMenus.removeValue(forKey: request)?.continuation.resume(returning: [])
        }
        for (request, pending) in itemMenus where pending.plugin == id {
            itemMenus.removeValue(forKey: request)?.continuation.resume(returning: [])
        }
        readerEvents[id] = nil
        running.removeAll { $0.id == id }
        commands.removeAll { $0.plugin == id }
        sections.removeAll { $0.plugin == id }
        columns.removeAll { $0.plugin == id }
        status[id] = nil
        drawing += 1
        if showing == id { showing = nil }
    }

    func run(_ command: PluginCommand) {
        if command.id.hasPrefix("ribbon:"), let index = Int(command.id.dropFirst("ribbon:".count)) {
            runners[command.plugin]?.send(["type": "ribbon", "index": index])
        } else {
            flushEditor()
            runners[command.plugin]?.send(["type": "command", "id": command.id, "editor": editorState()])
        }
    }

    func showSettings(_ id: String) {
        runners[id]?.send(["type": "settings"])
    }

    /// Closes the settings or modal on show.
    func closeShown() {
        if let showing { runners[showing]?.send(["type": "settings-close"]) }
        showing = nil
    }

    func dismiss(_ notice: PluginNotice) {
        notices.removeAll { $0.id == notice.id }
    }

    private func notify(_ text: String) {
        let notice = PluginNotice(id: noticeID, text: text)
        noticeID += 1
        notices = Array((notices + [notice]).suffix(5))
        Task {
            try? await Task.sleep(for: .seconds(5))
            dismiss(notice)
        }
    }

    private func update(_ id: String, _ change: (inout RunningPlugin) -> Void) {
        guard let index = running.firstIndex(where: { $0.id == id }) else { return }
        change(&running[index])
    }

    private func failStartup(_ runner: PluginRunner, message: String) {
        guard runners[runner.plugin.key] === runner, !runner.ready, !runner.failed else { return }
        runner.stop()
        removeWriteHistory(for: runner.plugin.key)
        update(runner.plugin.key) { $0.ready = false; $0.error = message }
    }

    /// Expiry, permanent deletion and a hard entry limit keep this cache bounded.
    private func pruneWriteHistory() {
        let now = Date()
        writeHistory = writeHistory.filter {
            now.timeIntervalSince($0.value.at) < 5 && itemBaseline?.byKey[$0.key] != nil
        }
        let excess = writeHistory.count - Self.writeHistoryLimit
        guard excess > 0 else { return }
        let oldest = writeHistory.sorted {
            if $0.value.at == $1.value.at { return $0.key < $1.key }
            return $0.value.at < $1.value.at
        }
        for entry in oldest.prefix(excess) { writeHistory[entry.key] = nil }
    }

    private func removeWriteHistory(for plugin: String) {
        for key in Array(writeHistory.keys) {
            guard let history = writeHistory[key] else { continue }
            let plugins = history.plugins.filter { $0 != plugin }
            if plugins.isEmpty { writeHistory[key] = nil }
            else { writeHistory[key] = (at: history.at, plugins: plugins) }
        }
    }

    /// The open note as a plugin's editor sees it (editor protocol 1): its
    /// text and the selection (UTF-16 offsets), else the cursor at its end.
    private func editorState() -> Any {
        editorSnapshot() ?? NSNull()
    }

    private func editorSnapshot() -> [String: Any]? {
        guard editorOpen, let vault, let path = vault.path else { return nil }
        let range = currentRange(in: vault)
        return [
            "protocol": 1, "path": path, "session": editorSession, "generation": editorGeneration,
            "revision": editorRevision, "contextRevision": editorContextRevision, "value": vault.text,
            "ranges": [["anchor": range.lowerBound, "head": range.upperBound]], "mainIndex": 0,
            // The editor here shows the Markdown source; the preview is beside it.
            "mode": "source", "focused": vault.editorFocused, "composing": false,
        ]
    }

    private var editorIdentity: EditorIdentity {
        EditorIdentity(session: editorSession, generation: editorGeneration, revision: editorRevision,
                       contextRevision: editorContextRevision)
    }

    private func currentRange(in vault: VaultModel) -> Range<Int> {
        let count = vault.text.utf16.count
        return vault.selectionUTF16.map { min($0.lowerBound, count)..<min($0.upperBound, count) } ?? count..<count
    }

    private func broadcastEditor(_ message: [String: Any]) {
        for runner in runners.values where !runner.plugin.zotero { runner.send(message) }
    }

    /// The note editor's text, selection, focus or note changed; plugins hear
    /// about it once the change settles (typing changes text and selection
    /// one after the other).
    func editorChanged() {
        guard editorFlush == nil else { return }
        editorFlush = Task { @MainActor [weak self] in
            await Task.yield()
            self?.flushEditor()
        }
    }

    /// The note editor went away.
    func editorClosed() {
        editorFlush?.cancel()
        editorFlush = nil
        guard editorOpen else { return }
        closeSuggestions()
        editorOpen = false
        broadcastEditor(["type": "editor/close", "session": editorSession, "generation": editorGeneration])
        broadcastEditor(["type": "active", "editor": NSNull()])
    }

    private func flushEditor() {
        editorFlush?.cancel()
        editorFlush = nil
        guard let vault, vault.path != nil else {
            editorClosed()
            return
        }
        let range = currentRange(in: vault)
        if !editorOpen || vault.path != editorPath || vault.version != editorVersion {
            closeSuggestions()
            if editorOpen {
                broadcastEditor(["type": "editor/close", "session": editorSession, "generation": editorGeneration])
            }
            editorOpen = true
            editorPath = vault.path
            editorVersion = vault.version
            editorGeneration += 1
            editorRevision = 0
            editorContextRevision = 0
            editorText = vault.text
            editorRange = range
            let editor = editorState()
            broadcastEditor(["type": "active", "editor": editor])
            broadcastEditor(["type": "editor/open", "editor": editor])
            return
        }
        let textChanged = vault.text != editorText
        guard textChanged || range != editorRange else { return }
        let base = editorIdentity
        if textChanged { editorRevision += 1 }
        editorContextRevision += 1
        editorText = vault.text
        editorRange = range
        let editor = editorState()
        broadcastEditor([
            "type": "editor/update", "editor": editor, "baseRevision": base.revision,
            "baseContextRevision": base.contextRevision, "changes": [Any](), "origin": "user",
        ])
        if textChanged { broadcastEditor(["type": "event", "name": "editor-change", "editor": editor]) }
        // As in Obsidian, typing opens suggestions; moving the cursor only
        // updates or closes a list already open.
        if textChanged || suggestion != nil || suggestQuery != nil { querySuggestions() }
    }

    /// Applies a plugin's edit to the open note if it was made for the note
    /// as it is now: all edits and the new selection, or nothing.
    private func applyProposal(_ message: [String: Any]) -> Bool {
        flushEditor()
        guard editorOpen, let vault, vault.path != nil,
              let operation = message["operation"] as? String, operation.count <= 200,
              message["session"] as? String == editorSession, message["generation"] as? Int == editorGeneration,
              message["baseRevision"] as? Int == editorRevision,
              message["baseContextRevision"] as? Int == editorContextRevision,
              let edits = message["edits"] as? [[String: Any]], edits.count <= 1000,
              let ranges = message["ranges"] as? [[String: Any]], !ranges.isEmpty, ranges.count <= 1000,
              let main = message["mainIndex"] as? Int, ranges.indices.contains(main)
        else { return false }
        var units = Array(vault.text.utf16)
        // An offset between the two halves of a surrogate pair would split a character.
        func boundary(_ offset: Int) -> Bool {
            offset == 0 || offset == units.count || !UTF16.isTrailSurrogate(units[offset])
        }
        var payload = 0
        for edit in edits {
            guard let from = edit["from"] as? Int, let to = edit["to"] as? Int, let insert = edit["insert"] as? String,
                  from >= 0, from <= to, to <= units.count, boundary(from), boundary(to)
            else { return false }
            payload += insert.utf16.count
            guard payload <= 10_000_000 else { return false }
            units.replaceSubrange(from..<to, with: Array(insert.utf16))
        }
        guard let anchor = ranges[main]["anchor"] as? Int, let head = ranges[main]["head"] as? Int,
              anchor >= 0, head >= 0, anchor <= units.count, head <= units.count, boundary(anchor), boundary(head)
        else { return false }
        // The editor here keeps one selection: the main one.
        let selection = min(anchor, head)..<max(anchor, head)
        vault.text = String(decoding: units, as: UTF16.self)
        vault.selectionUTF16 = selection
        vault.pluginSelection = selection
        flushEditor()
        return true
    }

    /// Asks the plugins with editor suggestions what to offer at the cursor.
    private func querySuggestions() {
        let old = suggestQuery
        old?.timer.cancel()
        suggestQuery = nil
        suggestion = nil
        guard let vault, let editor = editorSnapshot(), vault.editorFocused, currentRange(in: vault).isEmpty else {
            // An ineligible context also ends work the runtimes still do.
            if let old { sendSuggestClose(old.request) }
            return
        }
        let plugins = runners.filter { suggestPlugins.contains($0.key) && $0.value.ready && !$0.value.failed }
        guard !plugins.isEmpty else { return }
        editorRequest += 1
        let request = editorRequest
        let timer = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(1))
            guard !Task.isCancelled else { return }
            self?.finishSuggestions(request)
        }
        suggestQuery = (request, editorIdentity, vault.text.utf16.count, Set(plugins.keys), [:], timer)
        for runner in plugins.values { runner.send(["type": "suggest/query", "request": request, "editor": editor]) }
    }

    private func sendSuggestClose(_ request: Int) {
        for plugin in suggestPlugins { runners[plugin]?.send(["type": "suggest/close", "request": request]) }
    }

    private func finishSuggestions(_ request: Int) {
        guard let query = suggestQuery, query.request == request, query.identity == editorIdentity else { return }
        query.timer.cancel()
        suggestQuery?.pending = []
        // Highest priority first; ties go to the plugin that started first.
        let order = running.map(\.id)
        let ranked = order.compactMap { query.results[$0] }.enumerated()
        let best = ranked.max { a, b in
            a.element.priority == b.element.priority ? a.offset > b.offset : a.element.priority < b.element.priority
        }
        let winner = best?.element
        suggestion = winner
        for plugin in suggestPlugins where plugin != winner?.plugin {
            runners[plugin]?.send(["type": "suggest/close", "request": request])
        }
    }

    private func receiveSuggestions(_ message: [String: Any], from id: String) {
        guard let query = suggestQuery, message["request"] as? Int == query.request,
              EditorIdentity(message) == query.identity, query.identity == editorIdentity, suggestPlugins.contains(id),
              query.pending.contains(id) || suggestion?.plugin == id
        else { return }
        suggestQuery?.pending.remove(id)
        let listed = message["items"] as? [[String: Any]] ?? []
        let items: [PluginSuggestion.Item] = listed.prefix(100).compactMap { item in
            guard let key = item["id"] as? String, key.count <= 200, let label = item["label"] as? String else { return nil }
            return PluginSuggestion.Item(id: key, label: String(label.prefix(500)),
                                         secondary: (item["secondary"] as? String).map { String($0.prefix(500)) })
        }
        if !items.isEmpty, items.count == listed.count, Set(items.map(\.id)).count == items.count,
           let priority = message["priority"] as? Double, priority.isFinite,
           let start = message["start"] as? Int, let end = message["end"] as? Int,
           start >= 0, end >= start, end <= query.length {
            let selected = message["selected"] as? Int ?? 0
            suggestQuery?.results[id] = PluginSuggestion(
                plugin: id, request: query.request, identity: query.identity, priority: priority, items: items,
                selected: items.indices.contains(selected) ? selected : 0)
        } else {
            suggestQuery?.results[id] = nil
        }
        if suggestQuery?.pending.isEmpty == true { finishSuggestions(query.request) }
    }

    func closeSuggestions() {
        if let query = suggestQuery {
            query.timer.cancel()
            sendSuggestClose(query.request)
        }
        suggestQuery = nil
        suggestion = nil
    }

    /// What the plugins add to the editor's menu for the note as it is now.
    func editorMenus() async -> [PluginEditorMenu] {
        flushEditor()
        closeSuggestions()
        guard let editor = editorSnapshot() else { return [] }
        let identity = editorIdentity
        let asked = running.compactMap { runners[$0.id] }.filter { editorMenuPlugins.contains($0.plugin.key) && $0.ready }
        var menus: [PluginEditorMenu] = []
        for runner in asked {
            let plugin = runner.plugin.key
            editorRequest += 1
            let request = editorRequest
            let entries: [PluginItemMenuEntry] = await withCheckedContinuation { continuation in
                editorMenuRequests[request] = (plugin: plugin, identity: identity, continuation: continuation)
                runner.send(["type": "menu/request", "request": request, "editor": editor])
                Task {
                    try? await Task.sleep(for: .milliseconds(750))
                    if editorMenuRequests[request] != nil {
                        runner.send(["type": "menu/close", "request": request])
                        editorMenuRequests.removeValue(forKey: request)?.continuation.resume(returning: [])
                    }
                }
            }
            if !entries.isEmpty { menus.append(PluginEditorMenu(plugin: plugin, request: request, identity: identity, entries: entries)) }
        }
        // A menu for a note that has changed since would act on the wrong text.
        return identity == editorIdentity ? menus : []
    }

    /// Runs a plugin's editor menu command, if the note has not changed since.
    func chooseEditorMenu(_ menu: PluginEditorMenu, index: Int) {
        flushEditor()
        guard menu.identity == editorIdentity, let runner = runners[menu.plugin] else { return }
        runner.send([
            "type": "menu/choose", "request": menu.request, "session": menu.identity.session,
            "generation": menu.identity.generation, "revision": menu.identity.revision,
            "contextRevision": menu.identity.contextRevision, "index": index,
        ])
    }

    /// Moves the highlight in the suggestion list.
    func moveSuggestion(by step: Int) {
        guard var shown = suggestion, !shown.items.isEmpty else { return }
        shown.selected = (shown.selected + step + shown.items.count) % shown.items.count
        suggestion = shown
    }

    /// Takes a suggestion, as the key pressed (Enter, Tab) or a click would.
    func chooseSuggestion(_ id: String? = nil, key: String = "Enter", shift: Bool = false) {
        flushEditor()
        guard let shown = suggestion, shown.identity == editorIdentity else { return }
        let chosen = id ?? shown.items[shown.selected].id
        guard shown.items.contains(where: { $0.id == chosen }) else { return }
        runners[shown.plugin]?.send([
            "type": "suggest/choose", "request": shown.request, "session": shown.identity.session,
            "generation": shown.identity.generation, "revision": shown.identity.revision,
            "contextRevision": shown.identity.contextRevision, "id": chosen, "key": key, "shiftKey": shift,
        ])
    }

    /// Every note's metadata, as Obsidian gives it to plugins.
    private func metadata() -> Any {
        guard let text = try? vault?.core?.obsidianMetadata(),
              let object = try? JSONSerialization.jsonObject(with: Data(text.utf8))
        else { return [String: Any]() }
        return object
    }

    /// Tells the plugins the vault's notes changed.
    func filesChanged() {
        guard !runners.isEmpty else { return }
        let list = files()
        let data = metadata()
        for runner in runners.values {
            runner.send(["type": "files", "files": list, "metadata": data])
        }
    }

    private func files() -> [[String: Any]] {
        (vault?.notes ?? []).map { note in
            ["path": note.path, "stat": ["ctime": note.modified, "mtime": note.modified, "size": note.size] as [String: Any]]
        }
    }

    func receive(_ message: [String: Any], from runner: PluginRunner) {
        let id = runner.plugin.key
        guard runners[id] === runner, !runner.failed else { return }
        switch message["type"] as? String {
        case "loaded":
            start(runner)
        case "ready":
            guard runner.started, !runner.ready else { return }
            runner.ready = true
            runner.startupTimer?.cancel()
            runner.startupTimer = nil
            let pendingTabs = runner.pendingTabs
            let pendingOpens = runner.pendingOpens
            if let snapshot = runner.pendingItems {
                // Use a fresh sequence even when start already carried this snapshot.
                contextSeq += 1
                runner.send([
                    "type": "items", "items": snapshot.items, "seq": contextSeq,
                    "events": runner.pendingEvents.map(\.message),
                ])
            }
            runner.clearPendingContext()
            // Tabs and file opens follow the items they refer to.
            if var tabs = pendingTabs {
                contextSeq += 1
                tabs["seq"] = contextSeq
                runner.send(tabs)
            }
            for open in pendingOpens { runner.send(open) }
            update(id) { $0.ready = true }
            if !runner.plugin.zotero {
                flushEditor()
                runner.send(["type": "editor/snapshot", "editor": editorState()])
                querySuggestions()
            }
        case "call":
            guard let call = message["id"] as? Int, let method = message["method"] as? String else { return }
            let arguments = message["args"] as? [Any] ?? []
            Task {
                do {
                    let value = try await perform(method, arguments, for: runner)
                    runner.send(["type": "reply", "id": call, "ok": true, "value": value])
                } catch {
                    runner.send(["type": "reply", "id": call, "ok": false, "error": CoreKit.message(for: error)])
                }
            }
        case "commands":
            let listed = message["commands"] as? [[String: Any]] ?? []
            commands.removeAll { $0.plugin == id }
            commands += listed.compactMap { command in
                guard let key = command["id"] as? String, let name = command["name"] as? String else { return nil }
                return PluginCommand(id: key, name: "\(runner.plugin.name): \(name)", plugin: id)
            }
        case "ribbon":
            // Ribbon icons are in the command list, as commands.
            if let title = message["title"] as? String, let index = message["index"] as? Int {
                commands.append(PluginCommand(id: "ribbon:\(index)", name: "\(runner.plugin.name): \(title)", plugin: id))
            }
        case "status":
            status[id] = message["text"] as? String
        case "notice":
            notify(message["text"] as? String ?? "")
        case "section":
            guard let pane = message["paneID"] as? String else { return }
            sections.removeAll { $0.plugin == id && $0.paneID == pane }
            sections.append(PluginSection(plugin: id, paneID: pane, label: message["label"] as? String ?? pane))
        case "column":
            guard let key = message["key"] as? String, !key.isEmpty,
                  let label = message["label"] as? String, !label.isEmpty
            else { return }
            columns.removeAll { $0.plugin == id && $0.key == key }
            columns.append(PluginColumn(plugin: id, key: key, label: label))
        case "column-removed":
            guard let key = message["key"] as? String else { return }
            columns.removeAll { $0.plugin == id && $0.key == key }
        case "column-values":
            guard let request = message["id"] as? Int, let pending = columnRequests[request], pending.plugin == id
            else { return }
            columnRequests.removeValue(forKey: request)
            var values: [String: String] = [:]
            for (key, value) in message["values"] as? [String: Any] ?? [:] {
                if let text = value as? String { values[key] = text }
            }
            pending.continuation.resume(returning: values)
        case "reader-events":
            guard runner.plugin.zotero, let types = message["types"] as? [String] else { return }
            let supported = Self.readerTypes.filter { types.contains($0) }
            readerEvents[id] = supported.isEmpty ? nil : supported
        case "reader-rendered":
            guard let request = message["id"] as? Int, let pending = readerRenders[request], pending.plugin == id
            else { return }
            readerRenders.removeValue(forKey: request)
            pending.continuation.resume(returning: message["html"] as? String ?? "")
        case "reader-menu":
            guard let request = message["id"] as? Int, let pending = readerMenus[request], pending.plugin == id
            else { return }
            readerMenus.removeValue(forKey: request)
            let items: [ReaderMenuItem] = (message["items"] as? [[String: Any]] ?? []).compactMap { item in
                guard let index = item["index"] as? Int, index >= 0, let label = item["label"] as? String
                else { return nil }
                return ReaderMenuItem(index: index, label: label, disabled: item["disabled"] as? Bool ?? false)
            }
            pending.continuation.resume(returning: items)
        case "item-menu":
            guard let request = Self.menuInteger(message["id"]), let pending = itemMenus[request], pending.plugin == id
            else { return }
            itemMenus.removeValue(forKey: request)
            pending.continuation.resume(returning: Self.itemMenuEntries(message["items"]))
        case "codeblock":
            if let language = message["language"] as? String { codeLanguages[language] = id }
            drawing += 1
        case "postprocessor":
            if !postProcessors.contains(id) { postProcessors.append(id) }
            drawing += 1
        case "rendered":
            if let request = message["id"] as? Int, let waiting = renders.removeValue(forKey: request) {
                waiting.resume(returning: message["html"] as? String)
            }
        case "suggest/register":
            if !runner.plugin.zotero { suggestPlugins.insert(id) }
        case "suggest/trigger":
            if suggestPlugins.contains(id) {
                flushEditor()
                querySuggestions()
            }
        case "suggest/close":
            if suggestion?.plugin == id, message["request"] as? Int == suggestion?.request { closeSuggestions() }
        case "suggest/result":
            receiveSuggestions(message, from: id)
        case "menu/register":
            if !runner.plugin.zotero {
                editorMenuPlugins.insert(id)
                hasEditorMenu = true
            }
        case "menu/result":
            guard let request = message["request"] as? Int, let pending = editorMenuRequests[request], pending.plugin == id,
                  EditorIdentity(message) == pending.identity
            else { return }
            editorMenuRequests.removeValue(forKey: request)
            pending.continuation.resume(returning: Self.itemMenuEntries(message["items"]))
        case "editor/propose":
            guard !runner.plugin.zotero else { return }
            let accepted = applyProposal(message)
            var reply: [String: Any] = [
                "type": accepted ? "editor/ack" : "editor/reject", "operation": message["operation"] ?? "",
                "editor": editorState(),
            ]
            if !accepted { reply["reason"] = "stale-or-invalid" }
            runner.send(reply)
        case "settings-available":
            update(id) { $0.hasSettings = true }
        case "show":
            showing = id
        case "hide":
            if showing == id { showing = nil }
        case "partial":
            update(id) { $0.partial.append(message["what"] as? String ?? "") }
        case "error":
            let text = (message["message"] as? String ?? "").components(separatedBy: "\n").first ?? ""
            if !runner.ready { failStartup(runner, message: text) }
            else { update(id) { $0.error = text } }
            notify("\(runner.plugin.name): \(text)")
        default:
            break
        }
    }

    private func start(_ runner: PluginRunner) {
        guard !runner.failed, !runner.started, !runner.starting else { return }
        if runner.plugin.zotero {
            Task { await startZotero(runner) }
            return
        }
        guard let core = vault?.core, let plugin = runner.plugin.obsidian else {
            failStartup(runner, message: "Could not read the vault for plugin startup.")
            return
        }
        do {
            let code = try core.obsidianPluginCode(id: runner.plugin.id)
            let manifest: [String: Any] = [
                "id": plugin.id, "name": plugin.name, "version": plugin.version, "minAppVersion": plugin.minAppVersion,
                "description": plugin.description, "author": plugin.author, "authorUrl": plugin.authorUrl,
                "isDesktopOnly": plugin.isDesktopOnly, "dir": ".obsidian/plugins/\(plugin.id)",
            ]
            var codeObject: [String: Any] = ["main": code.main]
            codeObject["styles"] = code.styles.map { $0 as Any } ?? NSNull()
            codeObject["data"] = code.data.map { $0 as Any } ?? NSNull()
            #if os(iOS)
            let platform: [String: Any] = ["isMobile": true, "isMobileApp": true, "isIosApp": true]
            #else
            let platform: [String: Any] = ["isDesktop": true, "isMacOS": true]
            #endif
            runner.started = true
            runner.send([
                "type": "start", "manifest": manifest, "code": codeObject, "files": files(), "metadata": metadata(),
                "platform": platform,
            ])
        } catch {
            failStartup(runner, message: CoreKit.message(for: error))
        }
    }

    private func startZotero(_ runner: PluginRunner) async {
        guard runners[runner.plugin.key] === runner, !runner.failed, !runner.starting, !runner.started else { return }
        guard let core = library?.core else {
            failStartup(runner, message: CoreKit.message(for: PluginItemError.unavailable))
            return
        }
        runner.starting = true
        defer { runner.starting = false }
        do {
            let code = try core.zoteroPluginCode(id: runner.plugin.id)
            itemsChanged()
            await itemsSettled()
            guard runners[runner.plugin.key] === runner, !runner.failed else { return }
            guard itemBaseline != nil, let initial = runner.pendingItems else { throw PluginItemError.unavailable }
            var codeObject: [String: Any] = ["files": code.files.map { ["path": $0.path, "text": $0.text] }]
            codeObject["prefs"] = code.prefs.map { $0 as Any } ?? NSNull()
            let manifest: [String: Any] = [
                "id": runner.plugin.id, "name": runner.plugin.name, "version": runner.plugin.version,
            ]
            runner.started = true
            runner.send([
                "type": "start", "manifest": manifest, "code": codeObject, "items": initial.items, "seq": contextSeq,
                "selection": ["seq": contextSeq, "itemKeys": selectedItemKeys] as [String: Any],
                "tabs": tabContext.merging(["seq": contextSeq]) { current, _ in current },
            ])
        } catch {
            failStartup(runner, message: CoreKit.message(for: error))
        }
    }

    private func perform(_ method: String, _ arguments: [Any], for runner: PluginRunner) async throws -> Any {
        let text = { (i: Int) in arguments.indices.contains(i) ? (arguments[i] as? String ?? "") : "" }
        // Zotero RPCs belong to the library and also work before a vault opens.
        switch method {
        case "prefs.save":
            guard runner.plugin.zotero, let books = library?.core else { throw CocoaError(.featureUnsupported) }
            try books.saveZoteroPluginPrefs(id: runner.plugin.id, prefs: text(0))
            return NSNull()
        case "items.update":
            guard runner.plugin.zotero, let books = library?.core else { throw CocoaError(.featureUnsupported) }
            let key = text(0)
            let fields = text(1)
            let rawTags = arguments.count > 2 ? arguments[2] : NSNull()
            let annotation = itemBaseline?.byKey[key]?["itemType"] as? String == "annotation"
            guard !annotation || rawTags is NSNull || rawTags is [String] else {
                throw NSError(domain: "Paperloom.Plugin", code: 1, userInfo: [NSLocalizedDescriptionKey: "Invalid field tags."])
            }
            let tags = (rawTags as? [Any])?.map { "\($0)" }
            let previous = itemWrites
            // Serialize through the committed refresh, so another plugin cannot
            // overtake this save's origin bookkeeping or its observer event.
            let write: Task<Void, Error> = Task { @MainActor in
                _ = try? await previous?.value
                guard runners[runner.plugin.key] === runner, !runner.failed else { throw CancellationError() }
                pruneWriteHistory()
                let history = writeHistory[key]
                let recent = history.map { Date().timeIntervalSince($0.at) < 5 ? $0.plugins : [] } ?? []
                if recent.count >= 20 && recent.last != runner.plugin.key { throw PluginItemError.alternatingWrites }
                let before = PluginItemSnapshot.normalized(itemBaseline?.byKey[key])
                itemCommitRevision += 1
                do {
                    try books.zoteroUpdateItem(key: key, fields: fields, tags: tags)
                } catch {
                    itemCommitRevision += 1
                    throw error
                }
                itemCommitRevision += 1
                itemOrigins[key, default: []].insert(runner.plugin.key)
                itemsChanged()
                await itemsSettled()
                if runners[runner.plugin.key] === runner, !runner.failed, itemBaseline?.byKey[key] != nil,
                   before != PluginItemSnapshot.normalized(itemBaseline?.byKey[key]) {
                    let plugins = (recent.last == runner.plugin.key ? recent : recent + [runner.plugin.key])
                        .filter { runners[$0] != nil && runners[$0]?.failed != true }
                    writeHistory[key] = (Date(), plugins)
                    pruneWriteHistory()
                }
                library?.reload()
            }
            itemWrites = write
            try await write.value
            return NSNull()
        case "notes.create", "notes.update":
            // Plugins make notes in the vault and change only the ones they made.
            guard runner.plugin.zotero, let notes = vault?.core else { throw CocoaError(.featureUnsupported) }
            let html = text(1)
            let parent = arguments.first as? String
            let requested = text(0)
            let previous = itemWrites
            let write: Task<String?, Error> = Task { @MainActor in
                _ = try? await previous?.value
                guard runners[runner.plugin.key] === runner, !runner.failed else { throw CancellationError() }
                var key = requested
                itemCommitRevision += 1
                do {
                    if method == "notes.create" {
                        key = try notes.zoteroCreateNote(parentItem: parent, html: html)
                    } else {
                        try notes.zoteroUpdateNote(key: key, html: html)
                    }
                } catch {
                    itemCommitRevision += 1
                    throw error
                }
                itemCommitRevision += 1
                if method == "notes.update" { itemOrigins[key, default: []].insert(runner.plugin.key) }
                vault?.reloadList()
                itemsChanged()
                await itemsSettled()
                return method == "notes.create" ? key : nil
            }
            itemWrites = Task { _ = try await write.value }
            return try await write.value ?? NSNull()
        case "tabs.select":
            guard runner.plugin.zotero else { throw CocoaError(.featureUnsupported) }
            let viewID = text(0)
            guard viewID == "zotero-pane" || readers.contains(where: { $0.viewID == viewID }) else {
                throw PluginItemError.unknownTab
            }
            guard let window = selectionFocus.last, let handler = tabHandlers[window] else {
                throw PluginItemError.selectionUnavailable
            }
            try handler(viewID)
            return true
        case "pane.selectItems":
            guard runner.plugin.zotero else { throw CocoaError(.featureUnsupported) }
            guard let requested = arguments.first as? [String] else { throw PluginItemError.invalidSelection }
            var seen = Set<String>()
            let keys = requested.filter { seen.insert($0).inserted }
            guard keys.allSatisfy({ itemBaseline?.byKey[$0] != nil }) else { throw PluginItemError.unknownItem }
            try selectItems(keys)
            selectionChanged(itemKeys: keys)
            sendSelection()
            return true
        default:
            break
        }
        guard let vault, let core = vault.core else { throw CocoaError(.featureUnsupported) }
        switch method {
        case "vault.read":
            if text(0) == vault.path { return vault.text }
            return try core.readNote(path: text(0))
        case "vault.write", "vault.create":
            guard text(0).lowercased().hasSuffix(".md") else { throw CocoaError(.fileWriteNoPermission) }
            if text(0) == vault.path { vault.text = text(1) } else { _ = try core.writeNote(path: text(0), text: text(1)) }
            vault.reloadList()
            filesChanged()
            return NSNull()
        case "vault.createFolder":
            vault.createFolder(text(0))
            return NSNull()
        case "vault.delete":
            try core.deleteNote(path: text(0))
            vault.reloadList()
            return NSNull()
        case "vault.rename":
            _ = try core.renameNote(path: text(0), newPath: text(1))
            vault.reloadList()
            return NSNull()
        case "saveData":
            try core.saveObsidianPluginData(id: runner.plugin.id, data: text(0))
            return NSNull()
        case "editor.setValue", "editor.replaceSelection":
            throw NSError(domain: "Paperloom.Plugin", code: 2, userInfo: [
                NSLocalizedDescriptionKey: "Editor writes require a revisioned editor/propose message.",
            ])
        case "workspace.open":
            vault.openTarget(text(0))
            return NSNull()
        case "markdown.render":
            return try core.render(path: text(1).isEmpty ? (vault.path ?? "") : text(1), text: text(0))
        case "requestUrl":
            return try await fetch(arguments.first as? [String: Any] ?? [:], for: runner.plugin)
        default:
            throw CocoaError(.featureUnsupported)
        }
    }

    /// The sites each plugin may reach, as the person allowed them.
    private static let sitesKey = "pluginSites"

    private func fetch(_ request: [String: Any], for plugin: PluginRef) async throws -> Any {
        let address = request["url"] as? String ?? ""
        guard let url = URL(string: address), ["http", "https"].contains(url.scheme?.lowercased() ?? ""),
              let host = url.host()
        else { throw URLError(.unsupportedURL) }
        let site = url.port.map { "\(host):\($0)" } ?? host
        var sites = UserDefaults.standard.dictionary(forKey: Self.sitesKey) as? [String: [String]] ?? [:]
        if !(sites[plugin.key] ?? []).contains(site) {
            let allowed = await withCheckedContinuation { continuation in
                siteRequest = SiteRequest(plugin: plugin.name, site: site) { continuation.resume(returning: $0) }
            }
            guard allowed else { throw URLError(.userCancelledAuthentication) }
            sites[plugin.key, default: []].append(site)
            UserDefaults.standard.set(sites, forKey: Self.sitesKey)
        }
        let headers = (request["headers"] as? [String: Any] ?? [:]).map { PluginHeader(name: $0.key, value: "\($0.value)") }
        let method = request["method"] as? String ?? "GET"
        let body = request["body"] as? String
        let answer = try await Task.detached {
            try pluginFetch(url: address, method: method, headers: headers, body: body)
        }.value
        return ["status": Int(answer.status), "text": answer.text] as [String: Any]
    }
}
