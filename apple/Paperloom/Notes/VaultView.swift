import CoreKit
import SwiftUI

/// A folder or note in the vault tree.
struct VaultNode: Identifiable, Hashable {
    /// The note's path, `folder:<path>`, `canvas:<path>` or `drawing:<path>`.
    let id: String
    let name: String
    let isFolder: Bool
    var children: [VaultNode]?

    var isCanvas: Bool { id.hasPrefix("canvas:") }
    var isDrawing: Bool { id.hasPrefix("drawing:") }
    var isDiagram: Bool { id.hasPrefix("diagram:") }

    var systemImage: String {
        if isFolder { return "folder" }
        if isCanvas { return "rectangle.3.group" }
        if isDrawing { return "scribble.variable" }
        return isDiagram ? "flowchart" : "doc.text"
    }

    /// The tree under `parent` (`""` for the top), folders first.
    static func tree(
        folders: [String], notes: [NoteInfo], canvases: [String] = [], drawings: [String] = [],
        diagrams: [String] = [], parent: String = ""
    ) -> [VaultNode] {
        let subfolders = folders.filter { (($0 as NSString).deletingLastPathComponent) == parent }
        let folderNodes = subfolders.map { folder in
            VaultNode(
                id: "folder:\(folder)", name: (folder as NSString).lastPathComponent, isFolder: true,
                children: tree(
                    folders: folders, notes: notes, canvases: canvases, drawings: drawings, diagrams: diagrams,
                    parent: folder))
        }
        let noteNodes = notes.filter { $0.folder == parent }.map {
            VaultNode(id: $0.path, name: $0.title, isFolder: false, children: nil)
        }
        let canvasNodes = canvases.filter { ($0 as NSString).deletingLastPathComponent == parent }.map {
            VaultNode(
                id: "canvas:\($0)", name: (($0 as NSString).lastPathComponent as NSString).deletingPathExtension,
                isFolder: false, children: nil)
        }
        let drawingNodes = drawings.filter { ($0 as NSString).deletingLastPathComponent == parent }.map {
            VaultNode(
                id: "drawing:\($0)", name: (($0 as NSString).lastPathComponent as NSString).deletingPathExtension,
                isFolder: false, children: nil)
        }
        let diagramNodes = diagrams.filter { ($0 as NSString).deletingLastPathComponent == parent }.map {
            VaultNode(id: "diagram:\($0)", name: DrawIO.title($0), isFolder: false, children: nil)
        }
        return folderNodes + noteNodes + canvasNodes + drawingNodes + diagramNodes
    }
}

enum InspectorTab: Hashable {
    case outline, links, search, history, graph
}

/// The Markdown vault on Mac and iPad: the note tree, the open note, and an
/// inspector with its outline, backlinks and search.
struct VaultView: View {
    @Environment(VaultModel.self) private var vault
    @Environment(TabsModel.self) private var tabs
    @State private var showingInspector = true
    @State private var tab = InspectorTab.outline
    @State private var query = ""
    @State private var results: [NoteMention] = []
    @State private var suggestions: [LinkSuggestion] = []
    @State private var creatingFolder = false
    @State private var folderName = ""
    @State private var importingMap = false
    @AppStorage("vault.treeWidth") private var treeWidth = 240.0
    @AppStorage("vault.inspectorWidth") private var inspectorWidth = 260.0

    var body: some View {
        HStack(spacing: 0) {
            tree
                .frame(width: treeWidth)
            PaneResizeHandle(width: $treeWidth, range: 160...480, paneBefore: true)
            NoteView { tag in
                query = "tag:\(tag)"
                tab = .search
                showingInspector = true
                results = vault.search(query)
            }
            // A pane of its own rather than the system inspector, whose
            // toolbar background covers the top of its content here.
            if showingInspector {
                PaneResizeHandle(width: $inspectorWidth, range: 220...480, paneBefore: false)
                inspector
                    .frame(width: inspectorWidth)
            }
        }
        .toolbar {
            ToolbarItem {
                Button {
                    showingInspector.toggle()
                } label: {
                    Label("Outline and Backlinks", systemImage: "sidebar.right")
                }
            }
        }
        .onAppear { vault.refresh() }
        .mindMapImport(isPresented: $importingMap)
        .alert("New Folder", isPresented: $creatingFolder) {
            TextField("Name", text: $folderName)
            Button("Create") { vault.createFolder(folderName) }
            Button("Cancel", role: .cancel) {}
        }
    }

    private var tree: some View {
        List(
            VaultNode.tree(
                folders: vault.folders, notes: vault.notes, canvases: vault.canvases, drawings: vault.drawings,
                diagrams: vault.diagrams),
            children: \.children,
            selection: Binding(
                get: { vault.path },
                set: { id in
                    guard let id, !id.hasPrefix("folder:") else { return }
                    if id.hasPrefix("canvas:") {
                        tabs.open(.canvas(path: String(id.dropFirst("canvas:".count))))
                    } else if id.hasPrefix("drawing:") {
                        tabs.open(.drawing(path: String(id.dropFirst("drawing:".count))))
                    } else if id.hasPrefix("diagram:") {
                        tabs.open(.diagram(path: String(id.dropFirst("diagram:".count))))
                    } else {
                        vault.open(id)
                    }
                })
        ) { node in
            Label(node.name, systemImage: node.systemImage)
                .contextMenu {
                    if node.isFolder {
                        Button("New Note", systemImage: "square.and.pencil") {
                            vault.createNote(in: String(node.id.dropFirst("folder:".count)))
                        }
                    }
                }
        }
        .safeAreaInset(edge: .top, spacing: 0) {
            // The title, then the buttons, wrapping in a narrow column.
            VStack(alignment: .leading, spacing: 6) {
                Text("Notes").font(.headline)
                FlowLayout(spacing: 10, lineSpacing: 6) {
                    Button("New Note", systemImage: "square.and.pencil") { vault.createNote() }
                        .keyboardShortcut("n", modifiers: [.command, .option])
                    Menu("New from Template", systemImage: "doc.badge.plus") {
                        let templates = vault.templates()
                        if templates.isEmpty {
                            Text("No templates yet")
                        }
                        ForEach(templates, id: \.path) { template in
                            Button(template.title) { vault.createNote(from: template) }
                        }
                    }
                    .menuIndicator(.hidden)
                    .fixedSize()
                    Button("Today's Note", systemImage: "calendar") { vault.openToday() }
                        .keyboardShortcut("d", modifiers: [.command, .option])
                    Button("Graph", systemImage: "point.3.connected.trianglepath.dotted") { tabs.open(.graph) }
                    Button("New Canvas", systemImage: "rectangle.3.group") {
                        let folder = vault.path.map { ($0 as NSString).deletingLastPathComponent } ?? ""
                        if let made = vault.createCanvas(in: folder) { tabs.open(.canvas(path: made)) }
                    }
                    Button("New Mind Map", systemImage: "brain") {
                        vault.createMindMap(in: vault.path.map { ($0 as NSString).deletingLastPathComponent } ?? "")
                    }
                    Button("New Drawing", systemImage: "scribble.variable") {
                        let folder = vault.path.map { ($0 as NSString).deletingLastPathComponent } ?? ""
                        if let made = vault.createDrawing(in: folder) { tabs.open(.drawing(path: made)) }
                    }
                    Button("New Diagram", systemImage: "flowchart") {
                        let folder = vault.path.map { ($0 as NSString).deletingLastPathComponent } ?? ""
                        if let made = vault.createDiagram(in: folder) { tabs.open(.diagram(path: made)) }
                    }
                    Button("Import Mind Map…", systemImage: "square.and.arrow.down") { importingMap = true }
                    Button("New Folder", systemImage: "folder.badge.plus") {
                        folderName = ""
                        creatingFolder = true
                    }
                    }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
        }
    }

    private var inspector: some View {
        VStack(spacing: 8) {
            Picker("Show", selection: $tab) {
                Label("Outline", systemImage: "list.bullet.indent").tag(InspectorTab.outline)
                Label("Backlinks", systemImage: "link").tag(InspectorTab.links)
                Label("Search", systemImage: "magnifyingglass").tag(InspectorTab.search)
                Label("History", systemImage: "clock.arrow.circlepath").tag(InspectorTab.history)
                Label("Graph", systemImage: "point.3.connected.trianglepath.dotted").tag(InspectorTab.graph)
            }
            .labelStyle(.iconOnly)
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(.horizontal, 10)
            .padding(.top, 8)
            switch tab {
            case .outline:
                List {
                    if let outline = vault.outline {
                        NotePropertiesSection(properties: outline.properties)
                        if outline.headings.isEmpty {
                            Text("No headings").foregroundStyle(.secondary)
                        }
                        ForEach(outline.headings, id: \.line) { heading in
                            Button(heading.text) {
                                vault.jump = NoteJump(line: Int(heading.line), anchor: heading.anchor)
                            }
                            .buttonStyle(.plain)
                            .padding(.leading, CGFloat(heading.level - 1) * 12)
                        }
                        if !outline.tags.isEmpty {
                            Section("Tags") {
                                ForEach(outline.tags, id: \.self) { tag in
                                    Text(verbatim: "#\(tag)").foregroundStyle(.secondary)
                                }
                            }
                        }
                    }
                }
            case .links:
                List {
                    if let outline = vault.outline {
                        Section("Linked mentions") {
                            mentions(outline.backlinks, empty: "No notes link here yet.")
                        }
                        Section("Unlinked mentions") {
                            mentions(outline.unlinkedMentions, empty: "No other notes mention this title.")
                        }
                        Section("Suggested links") {
                            if suggestions.isEmpty {
                                Text("Nothing related found yet.").foregroundStyle(.secondary)
                            }
                            ForEach(suggestions, id: \.insert) { suggestion in
                                suggestionRow(suggestion)
                            }
                        }
                    }
                }
                .task(id: "\(vault.path ?? "")#\(vault.version)") { suggestions = vault.linkSuggestions() }
            case .history:
                NoteHistoryView()
            case .graph:
                NoteGraphView(center: vault.path, compact: true)
            case .search:
                VStack(spacing: 0) {
                    TextField("Words, tag:x, path:x", text: $query)
                        .textFieldStyle(.roundedBorder)
                        .padding(.horizontal, 10)
                        .onSubmit { results = vault.search(query) }
                    List {
                        mentions(results, empty: query.isEmpty ? "" : "No matches")
                    }
                }
            }
        }
    }

    private func suggestionRow(_ suggestion: LinkSuggestion) -> some View {
        HStack(alignment: .top) {
            Button {
                if let path = suggestion.path { vault.open(path) }
            } label: {
                VStack(alignment: .leading, spacing: 2) {
                    Text(verbatim: suggestion.title).font(.callout.weight(.semibold))
                    Text(verbatim: Self.plain(suggestion.snippetHtml))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(3)
                }
            }
            .buttonStyle(.plain)
            .disabled(suggestion.path == nil)
            Spacer(minLength: 4)
            Button("Link") {
                vault.insertion = suggestion.insert
                suggestions.removeAll { $0.insert == suggestion.insert }
            }
            .controlSize(.small)
            .help(Text(verbatim: suggestion.insert))
        }
    }

    /// A snippet's text without its `<b>` marks and entities.
    private static func plain(_ html: String) -> String {
        [("<b>", ""), ("</b>", ""), ("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""), ("&amp;", "&")]
            .reduce(html) { text, pair in text.replacingOccurrences(of: pair.0, with: pair.1) }
    }

    @ViewBuilder
    private func mentions(_ items: [NoteMention], empty: LocalizedStringKey) -> some View {
        if items.isEmpty {
            Text(empty).foregroundStyle(.secondary)
        }
        ForEach(items, id: \.self) { mention in
            Button {
                vault.open(mention.path)
                vault.jump = NoteJump(line: Int(mention.line), anchor: nil)
            } label: {
                VStack(alignment: .leading, spacing: 2) {
                    Text(mention.title).font(.callout.weight(.semibold))
                    Text(mention.context).font(.caption).foregroundStyle(.secondary).lineLimit(3)
                }
            }
            .buttonStyle(.plain)
        }
    }
}

/// The vault on iPhone: notes by folder; a note opens full screen.
struct PhoneNotesView: View {
    @Binding var path: [String]
    @Environment(VaultModel.self) private var vault
    @State private var openedDrawing: String?
    @State private var openedDiagram: String?
    @State private var switching = false
    @State private var importingMap = false

    var body: some View {
        List {
            if !vault.drawings.isEmpty {
                Section("Drawings") {
                    ForEach(vault.drawings, id: \.self) { drawing in
                        Button {
                            openedDrawing = drawing
                        } label: {
                            Label(
                                ((drawing as NSString).lastPathComponent as NSString).deletingPathExtension,
                                systemImage: "scribble.variable")
                        }
                    }
                }
            }
            if !vault.diagrams.isEmpty {
                Section("Diagrams") {
                    ForEach(vault.diagrams, id: \.self) { diagram in
                        Button {
                            openedDiagram = diagram
                        } label: {
                            Label(DrawIO.title(diagram), systemImage: "flowchart")
                        }
                    }
                }
            }
            let folders = [""] + vault.folders
            ForEach(folders, id: \.self) { folder in
                let notes = vault.notes.filter { $0.folder == folder }
                if !notes.isEmpty {
                    Section(folder.isEmpty ? String(localized: "Notes") : folder) {
                        ForEach(notes, id: \.path) { note in
                            Button {
                                vault.open(note.path)
                                path = [note.path]
                            } label: {
                                Label(note.title, systemImage: "doc.text")
                            }
                        }
                    }
                }
            }
        }
        .overlay {
            if vault.notes.isEmpty {
                ContentUnavailableView {
                    Label("No Notes Yet", systemImage: "note.text")
                } actions: {
                    Button("New Note") {
                        vault.createNote()
                        path = vault.path.map { [$0] } ?? []
                    }
                    .buttonStyle(.glassProminent)
                }
            }
        }
        .navigationTitle("Notes")
        .mindMapImport(isPresented: $importingMap) { path = vault.path.map { [$0] } ?? [] }
        .navigationDestination(for: String.self) { _ in
            NoteView()
                #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
                #endif
        }
        .navigationDestination(item: $openedDiagram) { diagram in
            DiagramBoardView(path: diagram)
                .navigationTitle(DrawIO.title(diagram))
                #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
                #endif
        }
        .navigationDestination(item: $openedDrawing) { drawing in
            DrawingBoardView(path: drawing)
                .navigationTitle(((drawing as NSString).lastPathComponent as NSString).deletingPathExtension)
                #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
                #endif
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button("New Note", systemImage: "square.and.pencil") {
                    vault.createNote()
                    path = vault.path.map { [$0] } ?? []
                }
            }
            ToolbarItem(placement: .secondaryAction) {
                Button("Today's Note", systemImage: "calendar") {
                    vault.openToday()
                    path = vault.path.map { [$0] } ?? []
                }
            }
            ToolbarItem(placement: .secondaryAction) {
                Button("New Mind Map", systemImage: "brain") {
                    vault.createMindMap()
                    path = vault.path.map { [$0] } ?? []
                }
            }
            ToolbarItem(placement: .secondaryAction) {
                Button("Import Mind Map…", systemImage: "square.and.arrow.down") { importingMap = true }
            }
            ToolbarItem(placement: .secondaryAction) {
                Button("New Drawing", systemImage: "scribble.variable") { openedDrawing = vault.createDrawing() }
            }
            ToolbarItem(placement: .secondaryAction) {
                Button("New Diagram", systemImage: "flowchart") { openedDiagram = vault.createDiagram() }
            }
            ToolbarItem(placement: .secondaryAction) {
                Menu("New from Template", systemImage: "doc.badge.plus") {
                    ForEach(vault.templates(), id: \.path) { template in
                        Button(template.title) {
                            vault.createNote(from: template)
                            path = vault.path.map { [$0] } ?? []
                        }
                    }
                }
            }
        }
        .onAppear { vault.refresh() }
    }
}
