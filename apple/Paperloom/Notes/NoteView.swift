import CoreKit
import SwiftUI
import UniformTypeIdentifiers

enum NoteMode: String, CaseIterable, Identifiable {
    case live, source, reading

    var id: String { rawValue }

    var title: LocalizedStringKey {
        switch self {
        case .live: "Live Preview"
        case .source: "Source"
        case .reading: "Reading View"
        }
    }

    var systemImage: String {
        switch self {
        case .live: "rectangle.split.2x1"
        case .source: "chevron.left.forwardslash.chevron.right"
        case .reading: "book"
        }
    }
}

/// The open note: its title, a mode (live preview beside the source, the
/// source alone, or reading), and a formatting toolbar with colors.
struct NoteView: View {
    var compact = false
    /// A `#tag` was clicked in the reading view.
    var onTag: (String) -> Void = { _ in }
    @Environment(VaultModel.self) private var vault
    @Environment(LibraryModel.self) private var library
    @Environment(TabsModel.self) private var tabs
    @Environment(AIAssistant.self) private var ai
    @Environment(PluginHost.self) private var plugins
    @Environment(\.openURL) private var openURL
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    @AppStorage("noteMode") private var mode = NoteMode.live
    /// Mind map notes show as a map rather than in `mode`.
    @AppStorage("mindMapShown") private var showMap = true
    @State private var selection: TextSelection?
    @FocusState private var editorFocus: Bool
    @State private var editorMenus: [PluginEditorMenu] = []
    @State private var showingEditorMenus = false
    @State private var titleDraft = ""
    @State private var html = ""
    @State private var anchor: String?
    @State private var switching = false
    @State private var linking = false
    @State private var confirmingDelete = false
    @State private var renderTask: Task<Void, Never>?
    @State private var openedPDF: AnnotationLocation?
    @State private var openedItem: String?
    @State private var openedDrawing: String?
    @State private var openedDiagram: String?
    @State private var linkProblem: String?
    @State private var exportFile: ExportFile?
    @State private var citing = false
    @State private var recorder = AudioRecorder()
    @State private var player = AudioPlayer()
    @State private var recordProblem: String?
    @State private var drafting = false

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            if vault.path == nil {
                ContentUnavailableView {
                    Label("No Notes Yet", systemImage: "note.text")
                } actions: {
                    Button("New Note") { vault.createNote() }
                        .buttonStyle(.glassProminent)
                }
                .frame(maxHeight: .infinity)
            } else if showingMap {
                MindMapBoardView()
            } else {
                switch mode {
                case .source:
                    editor
                case .reading:
                    preview
                case .live:
                    if stacked {
                        VStack(spacing: 0) {
                            editor
                            Divider()
                            preview
                        }
                    } else {
                        HStack(spacing: 0) {
                            editor
                            Divider()
                            preview
                        }
                    }
                }
            }
        }
        .onAppear(perform: noteChanged)
        .userActivity(AppleActivity.note, element: vault.path) { path, activity in
            AppleActivity.configure(activity, title: vault.title, key: "path", value: path)
        }
        .onChange(of: vault.version) { noteChanged() }
        .onChange(of: vault.text) { scheduleRender() }
        .onChange(of: mode) { render() }
        .onChange(of: showMap) { render() }
        .onChange(of: plugins.drawing) { render() }
        .onChange(of: vault.jump) { jump() }
        .onChange(of: vault.insertion) { insertPending() }
        .sheet(isPresented: $switching) {
            NoteSwitcher(allowCreate: true) { note in vault.open(note.path) } onCreate: { name in vault.createNote(title: name) }
        }
        .sheet(isPresented: $citing) {
            CitationPicker { key in
                apply { text, range in MarkdownEditing.insert(text, range, "[@\(key)]") }
            }
        }
        .sheet(isPresented: $linking) {
            NoteSwitcher(allowCreate: false) { note in
                let target = vault.linkCompletions(note.title).first { $0.path == note.path }?.insert ?? note.title
                apply { text, range in MarkdownEditing.insert(text, range, "[[\(target)]]") }
            } onCreate: { _ in }
        }
        .fileExporter(
            isPresented: Binding(get: { exportFile != nil }, set: { if !$0 { exportFile = nil } }),
            document: exportFile,
            contentType: exportFile?.contentType ?? ExportFile.word,
            defaultFilename: exportFile?.url.lastPathComponent
        ) { _ in
            if let url = exportFile?.url { try? FileManager.default.removeItem(at: url) }
            exportFile = nil
        }
        .confirmationDialog("Move this note to the vault’s trash?", isPresented: $confirmingDelete) {
            Button("Move to Trash", role: .destructive) { vault.deleteNote() }
        }
        .navigationDestination(item: $openedPDF) { place in
            PDFReaderView(
                url: URL(fileURLWithPath: place.filePath), attachmentID: place.attachmentId, title: place.itemTitle,
                page: Int(place.pageIndex))
        }
        .navigationDestination(item: $openedItem) { id in ItemDetailView(itemID: id) }
        .navigationDestination(item: $openedDrawing) { path in DrawingBoardView(path: path) }
        .navigationDestination(item: $openedDiagram) { path in DiagramBoardView(path: path) }
        .alert(
            "Record Audio", isPresented: Binding(get: { recordProblem != nil }, set: { if !$0 { recordProblem = nil } })
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(recordProblem ?? "")
        }
        .overlay(alignment: .bottom) { AudioPlayerBar(player: player) }
        .alert("Notes", isPresented: Binding(get: { linkProblem != nil }, set: { if !$0 { linkProblem = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(linkProblem ?? "")
        }
    }

    private var stacked: Bool {
        #if os(iOS)
        sizeClass == .compact || compact
        #else
        compact
        #endif
    }

    private var modePicker: some View {
        Picker("Mode", selection: $mode) {
            ForEach(NoteMode.allCases) { mode in
                Label(mode.title, systemImage: mode.systemImage).tag(mode)
            }
        }
        .labelsHidden()
        .fixedSize()
    }

    private var header: some View {
        HStack(spacing: 8) {
            Button {
                switching = true
            } label: {
                Label("Quick Switcher", systemImage: "magnifyingglass")
                    .labelStyle(.iconOnly)
                    .touchTarget()
            }
            .keyboardShortcut(compact ? nil : KeyboardShortcut("o", modifiers: .command))
            .buttonStyle(.borderless)
            TextField("Title", text: $titleDraft)
                .textFieldStyle(.plain)
                .font(compact ? .headline : .title2.weight(.semibold))
                .disabled(vault.path == nil)
                .onSubmit { vault.rename(to: titleDraft) }
            if isMap {
                Toggle(isOn: $showMap) {
                    Label("Mind Map", systemImage: "brain")
                }
                .toggleStyle(.button)
                .labelStyle(.iconOnly)
                .help("Mind Map")
            }
            if !showingMap {
                // The notes panel and the iPhone are narrow: the mode is a
                // menu there, leaving room for the title.
                if stacked {
                    modePicker.pickerStyle(.menu).labelStyle(.iconOnly)
                } else {
                    modePicker.pickerStyle(.segmented)
                }
            }
            Menu {
                Button("New Note", systemImage: "square.and.pencil") { vault.createNote(in: folder) }
                Button("New Mind Map", systemImage: "brain") { vault.createMindMap(in: folder) }
                Button(
                    drafting ? LocalizedStringKey("Drafting…") : LocalizedStringKey("Mind Map with AI"),
                    systemImage: "sparkles"
                ) { draftMap() }
                    .disabled(vault.path == nil || drafting || isMap)
                Button("Export as Word…", systemImage: "doc.richtext") {
                    exportFile = vault.wordDocument().map(ExportFile.init(url:))
                }
                .disabled(vault.path == nil)
                Button("Export as LaTeX…", systemImage: "sum") {
                    exportFile = vault.latexDocument().map(ExportFile.init(url:))
                }
                .disabled(vault.path == nil)
                Button("Move to Trash", systemImage: "trash", role: .destructive) { confirmingDelete = true }
                    .disabled(vault.path == nil)
            } label: {
                Label("More", systemImage: "ellipsis.circle")
                    .labelStyle(.iconOnly)
                    .touchTarget()
            }
            .menuIndicator(.hidden)
            .fixedSize()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, Self.barPadding)
    }

    /// Space above and below the header and the toolbar. iOS controls are
    /// already tall enough to touch, so they need none.
    #if os(iOS)
    private static let barPadding: CGFloat = 0
    #else
    private static let barPadding: CGFloat = 8
    #endif

    /// A mind map of the open note, drafted by the AI provider, as a new
    /// note beside it.
    private func draftMap() {
        let provider = ai.provider
        let language = AIAssistant.language
        let (title, text, folder) = (vault.title, vault.text, self.folder)
        drafting = true
        Task {
            do {
                let drafted = try await Task.detached {
                    try aiMindmap(provider: provider, title: title, text: text, language: language)
                }.value
                vault.createMap(titled: "\(title) – \(String(localized: "Mind Map"))", text: drafted, in: folder)
            } catch {
                linkProblem = CoreKit.message(for: error)
            }
            drafting = false
        }
    }

    /// The open note is a mind map (it has a `mindmap` property).
    private var isMap: Bool {
        vault.path != nil && isMindmap(text: vault.text)
    }

    private var showingMap: Bool { isMap && showMap }

    private var folder: String {
        vault.path.map { ($0 as NSString).deletingLastPathComponent } ?? ""
    }

    private var editor: some View {
        @Bindable var vault = vault
        return VStack(spacing: 0) {
            toolbar
            TextEditor(text: $vault.text, selection: $selection)
                .onChange(of: selection) {
                    vault.selectionUTF16 = utf16Offsets(in: vault.text)
                    plugins.editorChanged()
                }
                .focused($editorFocus)
                .onKeyPress(phases: .down) { press in suggestionKey(press) }
                .font(.body.monospaced())
                .scrollContentBackground(.hidden)
                .padding(.horizontal, 8)
                .id(vault.version)
                .overlay(alignment: .bottomLeading) { suggestions }
        }
        .onChange(of: vault.text) { plugins.editorChanged() }
        .onChange(of: vault.pluginSelection) { showPluginSelection() }
        .onChange(of: editorFocus) { focusChanged() }
        .onDisappear { plugins.editorClosed() }
    }

    /// What plugins add to the editor's menu, asked for when opened.
    private var pluginActions: some View {
        Button {
            Task {
                editorMenus = await plugins.editorMenus()
                showingEditorMenus = true
            }
        } label: {
            Label("Plugin Actions", systemImage: "puzzlepiece.extension").labelStyle(.iconOnly).touchTarget(width: 40)
        }
        .help(Text("Plugin Actions"))
        .popover(isPresented: $showingEditorMenus) {
            PluginEditorMenuList(menus: editorMenus) { menu, index in
                showingEditorMenus = false
                plugins.chooseEditorMenu(menu, index: index)
            }
        }
    }

    /// What a plugin suggests for the text at the cursor.
    @ViewBuilder private var suggestions: some View {
        if let suggestion = plugins.suggestion {
            PluginSuggestionList(suggestion: suggestion) { id in plugins.chooseSuggestion(id) }
        }
    }

    /// Keys for an open suggestion list: move, take (Enter, Tab), dismiss.
    private func suggestionKey(_ press: KeyPress) -> KeyPress.Result {
        guard plugins.suggestion != nil else { return .ignored }
        let shift = press.modifiers.contains(.shift)
        switch press.key {
        case .upArrow: plugins.moveSuggestion(by: -1)
        case .downArrow: plugins.moveSuggestion(by: 1)
        case .return: plugins.chooseSuggestion(key: "Enter", shift: shift)
        case .tab: plugins.chooseSuggestion(key: "Tab", shift: shift)
        case .escape: plugins.closeSuggestions()
        default: return .ignored
        }
        return .handled
    }

    /// Shows the selection a plugin's edit left.
    private func showPluginSelection() {
        guard let range = vault.pluginSelection else { return }
        vault.pluginSelection = nil
        let text = vault.text
        guard range.upperBound <= text.utf16.count else { return }
        let lower = String.Index(utf16Offset: range.lowerBound, in: text)
        let upper = String.Index(utf16Offset: range.upperBound, in: text)
        selection = range.isEmpty ? TextSelection(insertionPoint: lower) : TextSelection(range: lower..<upper)
    }

    private func focusChanged() {
        vault.editorFocused = editorFocus
        guard !editorFocus else {
            plugins.editorChanged()
            return
        }
        // A click on a suggestion can take the focus first; the list closes
        // only when the focus stays away.
        Task {
            try? await Task.sleep(for: .milliseconds(500))
            if !editorFocus { plugins.closeSuggestions() }
        }
    }

    private var preview: some View {
        NoteWebView(html: html, anchor: anchor, fileURL: { vault.fileURL($0) }) { action in
            switch action {
            case .note(let path, let anchor):
                vault.open(path)
                self.anchor = anchor
            case .newNote(let name):
                if name.hasPrefix("@") {
                    let parts = name.split(separator: "#", maxSplits: 1).map(String.init)
                    if parts.count > 1, parts[1].hasPrefix("hl-") {
                        openHighlight(String(parts[1].dropFirst(3)))
                    } else if let id = library.resolveItemReference(parts[0]) {
                        openItem(id)
                    } else {
                        linkProblem = String(localized: "No item in the library matches \(parts[0]).")
                    }
                } else {
                    vault.openTarget(name)
                }
            case .tag(let tag):
                onTag(tag)
            case .annotation(let id):
                openHighlight(id)
            case .item(let id):
                openItem(id)
            case .drawing(let path):
                if pushes { openedDrawing = path } else { tabs.open(.drawing(path: path)) }
            case .diagram(let path):
                if pushes { openedDiagram = path } else { tabs.open(.diagram(path: path)) }
            case .audio(let path):
                if let url = vault.fileURL(path) { player.play(url) }
            case .web(let url):
                openURL(url)
            }
        }
    }

    /// On a phone screens are pushed; on a Mac or iPad they open as tabs.
    private var pushes: Bool {
        #if os(iOS)
        sizeClass == .compact
        #else
        false
        #endif
    }

    private func openHighlight(_ id: String) {
        guard let place = library.annotationLocation(id: id) else {
            linkProblem = String(localized: "This highlight is no longer in the library.")
            return
        }
        if pushes {
            openedPDF = place
        } else {
            tabs.open(.pdf(
                attachmentID: place.attachmentId, path: place.filePath, title: place.itemTitle,
                page: Int(place.pageIndex)))
        }
    }

    private func openItem(_ id: String) {
        if pushes {
            openedItem = id
        } else {
            tabs.open(.item(id: id, title: library.detail(id: id)?.title ?? ""))
        }
    }

    private var toolbar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 2) {
                tool("Bold", "bold") { MarkdownEditing.toggleWrap($0, $1, before: "**") }
                tool("Italic", "italic") { MarkdownEditing.toggleWrap($0, $1, before: "*") }
                tool("Underline", "underline") { MarkdownEditing.toggleWrap($0, $1, before: "<u>", after: "</u>") }
                tool("Strikethrough", "strikethrough") { MarkdownEditing.toggleWrap($0, $1, before: "~~") }
                Divider().frame(height: 18)
                tool("Heading 1", "textformat.size.larger") { MarkdownEditing.toggleLinePrefix($0, $1, prefix: "# ") }
                tool("Heading 2", "textformat.size") { MarkdownEditing.toggleLinePrefix($0, $1, prefix: "## ") }
                tool("Bulleted List", "list.bullet") { MarkdownEditing.toggleLinePrefix($0, $1, prefix: "- ") }
                tool("Numbered List", "list.number") { MarkdownEditing.toggleLinePrefix($0, $1, prefix: "1. ") }
                tool("Task", "checklist") { MarkdownEditing.toggleLinePrefix($0, $1, prefix: "- [ ] ") }
                tool("Quote", "text.quote") { MarkdownEditing.toggleLinePrefix($0, $1, prefix: "> ") }
                tool("Code", "chevron.left.forwardslash.chevron.right") { MarkdownEditing.toggleWrap($0, $1, before: "`") }
                tool("Table", "tablecells") { MarkdownEditing.insert($0, $1, MarkdownEditing.table, cursor: 2) }
                Button {
                    linking = true
                } label: {
                    Label("Link to Note", systemImage: "link").labelStyle(.iconOnly).touchTarget(width: 40)
                }
                Button {
                    citing = true
                } label: {
                    Label("Cite", systemImage: "quote.opening").labelStyle(.iconOnly).touchTarget(width: 40)
                }
                .help(Text("Cite"))
                Button {
                    insertDrawing(diagram: false)
                } label: {
                    Label("Insert Drawing", systemImage: "scribble.variable").labelStyle(.iconOnly).touchTarget(width: 40)
                }
                .help(Text("Insert Drawing"))
                Button {
                    insertDrawing(diagram: true)
                } label: {
                    Label("Insert Diagram", systemImage: "flowchart").labelStyle(.iconOnly).touchTarget(width: 40)
                }
                .help(Text("Insert Diagram"))
                Button {
                    Task { await toggleRecording() }
                } label: {
                    Label(
                        recorder.isRecording ? LocalizedStringKey("Stop Recording") : LocalizedStringKey("Record Audio"),
                        systemImage: recorder.isRecording ? "stop.circle.fill" : "mic"
                    )
                    .labelStyle(.iconOnly)
                    .foregroundStyle(recorder.isRecording ? Color.red : Color.primary)
                    .touchTarget(width: 40)
                }
                .help(Text(recorder.isRecording ? LocalizedStringKey("Stop Recording") : LocalizedStringKey("Record Audio")))
                if plugins.hasEditorMenu { pluginActions }
                Divider().frame(height: 18)
                Menu {
                    ForEach(MarkdownEditing.textColors, id: \.self) { hex in
                        Button {
                            apply { MarkdownEditing.color($0, $1, hex: hex, background: false) }
                        } label: {
                            Label { Text(verbatim: hex) } icon: { colorDot(PlatformColor(hex: hex) ?? .gray) }
                        }
                    }
                } label: {
                    Label("Text Color", systemImage: "paintbrush").labelStyle(.iconOnly).touchTarget(width: 40)
                }
                .menuIndicator(.hidden)
                .fixedSize()
                Menu {
                    ForEach(MarkdownEditing.backgroundColors, id: \.self) { hex in
                        Button {
                            apply { MarkdownEditing.color($0, $1, hex: hex, background: true) }
                        } label: {
                            Label { Text(verbatim: hex) } icon: { colorDot(PlatformColor(hex: hex) ?? .systemYellow) }
                        }
                    }
                } label: {
                    Label("Highlight Color", systemImage: "highlighter").labelStyle(.iconOnly).touchTarget(width: 40)
                }
                .menuIndicator(.hidden)
                .fixedSize()
                Divider().frame(height: 18)
                Menu {
                    let templates = vault.templates()
                    if templates.isEmpty {
                        Text("No templates yet")
                    }
                    ForEach(templates, id: \.path) { template in
                        Button(template.title) {
                            if let filled = vault.templateText(template) {
                                apply { MarkdownEditing.insert($0, $1, filled) }
                            }
                        }
                    }
                } label: {
                    Label("Insert Template", systemImage: "doc.badge.plus").labelStyle(.iconOnly).touchTarget(width: 40)
                }
                .menuIndicator(.hidden)
                .fixedSize()
            }
            .buttonStyle(.borderless)
            .padding(.horizontal, 10)
            .padding(.vertical, Self.barPadding / 2)
        }
    }

    /// Starts recording, or stops and embeds the recording (kept beside the
    /// note) where the cursor is.
    private func toggleRecording() async {
        guard recorder.isRecording else {
            recordProblem = await recorder.start()
            return
        }
        guard let data = recorder.stop() else { return }
        let stamp = Date().formatted(.verbatim(
            "\(year: .defaultDigits)-\(month: .twoDigits)-\(day: .twoDigits) \(hour: .twoDigits(clock: .twentyFourHour, hourCycle: .zeroBased)).\(minute: .twoDigits).\(second: .twoDigits)",
            timeZone: .current, calendar: .current))
        if let made = vault.addFile(in: folder, name: "Recording \(stamp).m4a", data: data) {
            apply { text, range in MarkdownEditing.insert(text, range, "![[\(made)]]") }
        }
    }

    /// A new drawing (or draw.io diagram) beside the note, embedded where
    /// the cursor is, and opened.
    private func insertDrawing(diagram: Bool) {
        guard let made = diagram ? vault.createDiagram(in: folder) : vault.createDrawing(in: folder) else { return }
        apply { text, range in MarkdownEditing.insert(text, range, "![[\(made)]]") }
        if diagram {
            if pushes { openedDiagram = made } else { tabs.open(.diagram(path: made)) }
        } else {
            if pushes { openedDrawing = made } else { tabs.open(.drawing(path: made)) }
        }
    }

    private func tool(
        _ title: LocalizedStringKey, _ systemImage: String,
        _ command: @escaping (String, Range<Int>) -> MarkdownEditing.Edit
    ) -> some View {
        Button {
            apply(command)
        } label: {
            Label(title, systemImage: systemImage).labelStyle(.iconOnly).touchTarget(width: 40)
        }
        .help(Text(title))
    }

    /// Runs an editing command on the text and the selection.
    private func apply(_ command: (String, Range<Int>) -> MarkdownEditing.Edit) {
        let text = vault.text
        let edit = command(text, offsets(in: text) ?? (text.count..<text.count))
        vault.text = edit.text
        let new = edit.text
        let lower = new.index(new.startIndex, offsetBy: min(edit.selection.lowerBound, new.count))
        let upper = new.index(new.startIndex, offsetBy: min(edit.selection.upperBound, new.count))
        selection = TextSelection(range: lower..<upper)
    }

    private func offsets(in text: String) -> Range<Int>? {
        guard let selection, case .selection(let range) = selection.indices,
              range.lowerBound >= text.startIndex, range.upperBound <= text.endIndex
        else { return nil }
        return text.distance(from: text.startIndex, to: range.lowerBound)..<text.distance(from: text.startIndex, to: range.upperBound)
    }

    /// The selection as UTF-16 offsets, for plugins.
    private func utf16Offsets(in text: String) -> Range<Int>? {
        guard let selection, case .selection(let range) = selection.indices,
              range.lowerBound >= text.startIndex, range.upperBound <= text.endIndex
        else { return nil }
        let utf16 = text.utf16
        return utf16.distance(from: utf16.startIndex, to: range.lowerBound)..<utf16.distance(from: utf16.startIndex, to: range.upperBound)
    }

    private func noteChanged() {
        titleDraft = vault.title
        selection = nil
        plugins.editorChanged()
        render()
    }

    private func render() {
        renderTask?.cancel()
        guard mode != .source, !showingMap else { return }
        html = vault.renderedHTML()
        drawPluginBlocks()
    }

    /// Puts in what plugins draw: the notes as their post processors change
    /// them, and the code blocks they draw (a Dataview query, say).
    private func drawPluginBlocks() {
        guard let path = vault.path else { return }
        let rendered = html
        Task {
            let processed = await plugins.postProcess(rendered, path: path)
            let drawn = await plugins.drawCodeBlocks(in: processed, path: path)
            if drawn != rendered && html == rendered { html = drawn }
        }
    }

    /// Re-renders shortly after typing stops.
    private func scheduleRender() {
        guard mode == .live, !showingMap else { return }
        renderTask?.cancel()
        renderTask = Task {
            try? await Task.sleep(for: .milliseconds(300))
            guard !Task.isCancelled else { return }
            html = vault.renderedHTML()
            drawPluginBlocks()
        }
    }

    private func insertPending() {
        guard let text = vault.insertion else { return }
        vault.insertion = nil
        apply { MarkdownEditing.insert($0, $1, text) }
    }

    private func jump() {
        guard let jump = vault.jump else { return }
        anchor = jump.anchor
        if mode == .source || (mode == .live && jump.anchor == nil) {
            let text = vault.text
            var offset = 0
            for (n, line) in text.components(separatedBy: "\n").enumerated() {
                if n == jump.line { break }
                offset += line.count + 1
            }
            let at = text.index(text.startIndex, offsetBy: min(offset, text.count))
            selection = TextSelection(insertionPoint: at)
        }
    }
}

/// Finds a note by name; picks it, or creates a note with the typed name.
struct NoteSwitcher: View {
    let allowCreate: Bool
    let onPick: (NoteInfo) -> Void
    let onCreate: (String) -> Void
    @Environment(VaultModel.self) private var vault
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""

    var body: some View {
        NavigationStack {
            let found = vault.switcher(query)
            List {
                ForEach(found, id: \.path) { note in
                    Button {
                        onPick(note)
                        dismiss()
                    } label: {
                        VStack(alignment: .leading) {
                            Text(note.title)
                            if !note.folder.isEmpty {
                                Text(note.folder).font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    }
                }
                let name = query.trimmingCharacters(in: .whitespacesAndNewlines)
                if allowCreate && !name.isEmpty && !found.contains(where: { $0.title.caseInsensitiveCompare(name) == .orderedSame }) {
                    Button("Create “\(name)”", systemImage: "plus") {
                        onCreate(name)
                        dismiss()
                    }
                }
            }
            .searchable(text: $query, prompt: Text("Find or create a note…"))
            .navigationTitle("Quick Switcher")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 420, minHeight: 420)
        #endif
    }
}
