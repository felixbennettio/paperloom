import CoreKit
import SwiftUI
import UniformTypeIdentifiers

/// The Mac and iPad layout: library sidebar, document tabs (horizontal, or
/// vertical on either side), and a notes panel docked left or right.
struct WorkbenchView: View {
    @Environment(LibraryModel.self) private var library
    @Environment(VaultModel.self) private var vault
    @Environment(WorkbenchSettings.self) private var settings
    @Environment(PluginHost.self) private var plugins
    @Environment(\.scenePhase) private var scenePhase
    #if os(macOS)
    @Environment(\.appearsActive) private var appearsActive
    #endif
    @State private var selectionWindowID = UUID()
    @State private var tabs = TabsModel()
    @State private var showingAddItem = false
    @State private var choosingPDFs = false

    var body: some View {
        NavigationSplitView {
            CollectionsSidebar()
                .navigationSplitViewColumnWidth(min: 200, ideal: 240, max: 340)
        } detail: {
            detail
                .toolbar { toolbar }
        }
        .environment(tabs)
        .onAppear {
            AppleServices.shared.navigation.register(selectionWindowID, handler: open)
            plugins.registerSelectionHandler(for: selectionWindowID) { keys in
                guard let key = keys.first else { return }
                guard let item = library.detail(id: key) else { throw PluginItemError.unknownItem }
                tabs.open(.item(id: item.id, title: item.title))
            }
            plugins.registerTabHandler(for: selectionWindowID) { attachmentKey in
                guard let attachmentKey else {
                    tabs.selectedID = WorkbenchTab.library.id
                    return
                }
                guard let tab = tabs.tabs.first(where: {
                    if case .pdf(let id, _, _, _) = $0 { id == attachmentKey } else { false }
                }) else { throw PluginItemError.unknownTab }
                tabs.selectedID = tab.id
            }
            focusSelectionWindow()
            reportReaders()
        }
        .onDisappear {
            plugins.unregisterSelectionHandler(for: selectionWindowID)
            AppleServices.shared.navigation.unregister(selectionWindowID)
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active {
                focusSelectionWindow()
            } else {
                AppleServices.shared.navigation.blur(selectionWindowID)
            }
        }
        #if os(macOS)
        .onChange(of: appearsActive) { _, active in
            if active {
                focusSelectionWindow()
            } else {
                AppleServices.shared.navigation.blur(selectionWindowID)
            }
        }
        #endif
        .onChange(of: [tabs.tabs.map(\.id), [tabs.selectedID]]) { reportReaders() }
        .onChange(of: tabs.selected, initial: true) { _, selected in
            if case .item(let id, _) = selected, plugins.selectedItemKeys.first != id {
                plugins.selectionChanged(itemKeys: [id])
            }
        }
        .sheet(isPresented: $showingAddItem) {
            AddItemSheet()
        }
        .fileImporter(isPresented: $choosingPDFs, allowedContentTypes: [.pdf], allowsMultipleSelection: true) { result in
            guard case .success(let urls) = result, !urls.isEmpty else { return }
            Task { await library.importPDFs(urls) }
        }
        .libraryNotices()
        .alert(
            "Something went wrong",
            isPresented: Binding(
                get: { library.errorMessage != nil },
                set: { if !$0 { library.errorMessage = nil } })
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(library.errorMessage ?? "")
        }
    }

    /// This window's PDF tabs, for Zotero plugins' tab and reader context.
    private func reportReaders() {
        let keys = tabs.tabs.compactMap { tab -> String? in
            if case .pdf(let id, _, _, _) = tab { id } else { nil }
        }
        var selected: String?
        if case .pdf(let id, _, _, _) = tabs.selected { selected = id }
        plugins.readersChanged(in: selectionWindowID, attachmentKeys: keys, selected: selected)
    }

    private func focusSelectionWindow() {
        guard scenePhase == .active else { return }
        #if os(macOS)
        guard appearsActive else { return }
        #endif
        plugins.focusSelectionWindow(selectionWindowID)
        AppleServices.shared.navigation.focus(selectionWindowID)
    }

    private func open(_ destination: AppleDestination) {
        switch destination {
        case .item(let id):
            tabs.open(.item(id: id, title: library.detail(id: id)?.title ?? String(localized: "Item Not Found")))
        case .note(let path):
            guard vault.notes.contains(where: { $0.path == path }) else { return }
            vault.open(path)
            guard vault.path == path else { return }
            tabs.open(.notes)
        }
    }

    /// The notes panel, except while the Notes tab is in front: it would
    /// show the same note twice.
    private var showsNotesPanel: Bool {
        guard settings.showsNotes else { return false }
        if case .notes = tabs.selected { return false }
        return true
    }

    private var detail: some View {
        @Bindable var settings = settings
        return HStack(spacing: 0) {
            if settings.tabStyle == .verticalLeading {
                VerticalTabStrip()
                    .frame(minWidth: 150, maxWidth: settings.tabStripWidth)
                    .layoutPriority(1)
                PaneResizeHandle(width: $settings.tabStripWidth, range: 150...400, paneBefore: true)
            }
            if showsNotesPanel && settings.notesEdge == .leading {
                NotesPanel()
                    .frame(minWidth: 240, maxWidth: settings.notesWidth)
                    .layoutPriority(1)
                PaneResizeHandle(width: $settings.notesWidth, range: 260...900, paneBefore: true)
            }
            VStack(spacing: 0) {
                if settings.tabStyle == .horizontal {
                    HorizontalTabStrip()
                }
                HStack(spacing: 0) {
                    TabContent(tab: tabs.selected)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                    if let side = tabs.side, side.id != tabs.selectedID {
                        PaneResizeHandle(width: $settings.splitWidth, range: 280...1400, paneBefore: false)
                        VStack(spacing: 0) {
                            HStack {
                                Label(side.title, systemImage: side.systemImage)
                                    .font(.callout)
                                    .lineLimit(1)
                                Spacer()
                                Button("Close Split", systemImage: "xmark") { tabs.sideID = nil }
                                    .labelStyle(.iconOnly)
                                    .buttonStyle(.borderless)
                            }
                            .padding(.horizontal, 12)
                            .padding(.vertical, 6)
                            Divider()
                            TabContent(tab: side)
                                .frame(maxWidth: .infinity, maxHeight: .infinity)
                        }
                        .frame(minWidth: 280, maxWidth: settings.splitWidth)
                        .layoutPriority(1)
                    }
                }
            }
            if showsNotesPanel && settings.notesEdge == .trailing {
                PaneResizeHandle(width: $settings.notesWidth, range: 260...900, paneBefore: false)
                NotesPanel()
                    .frame(minWidth: 240, maxWidth: settings.notesWidth)
                    .layoutPriority(1)
            }
            if settings.tabStyle == .verticalTrailing {
                PaneResizeHandle(width: $settings.tabStripWidth, range: 150...400, paneBefore: false)
                VerticalTabStrip()
                    .frame(minWidth: 150, maxWidth: settings.tabStripWidth)
                    .layoutPriority(1)
            }
        }
        .animation(.smooth, value: settings.tabStyle)
        .animation(.smooth, value: settings.notesEdge)
        .animation(.smooth, value: settings.showsNotes)
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            Button {
                showingAddItem = true
            } label: {
                Label("Add Item", systemImage: "plus")
            }
            .keyboardShortcut("n", modifiers: [.command, .shift])

            Button {
                choosingPDFs = true
            } label: {
                Label("Add PDF…", systemImage: "doc.richtext")
            }
            .keyboardShortcut("o", modifiers: [.command, .shift])

            ImportExportMenu(itemIDs: library.items.map(\.id))

            Menu {
                Picker(
                    "Tabs",
                    selection: Binding(get: { settings.tabStyle }, set: { settings.tabStyle = $0 })
                ) {
                    ForEach(TabStripStyle.allCases) { style in
                        Label(style.title, systemImage: style.systemImage).tag(style)
                    }
                }
                Picker(
                    "Notes panel",
                    selection: Binding(get: { settings.notesEdge }, set: { settings.notesEdge = $0 })
                ) {
                    ForEach(PanelEdge.allCases) { edge in
                        Text(edge.title).tag(edge)
                    }
                }
            } label: {
                Label("Layout", systemImage: settings.tabStyle.systemImage)
            }

            Button {
                settings.showsNotes.toggle()
            } label: {
                Label("Notes", systemImage: settings.notesEdge == .leading ? "sidebar.left" : "sidebar.right")
            }
            .keyboardShortcut("\\", modifiers: [.command, .option])

            // The Mac has its Settings window; the iPad opens them in a tab.
            #if os(iOS)
            Button {
                tabs.open(.settings)
            } label: {
                Label("Settings", systemImage: "gearshape")
            }
            .keyboardShortcut(",", modifiers: .command)
            #endif
        }
    }
}

/// A tab's document.
struct TabContent: View {
    let tab: WorkbenchTab

    var body: some View {
        switch tab {
        case .library:
            ItemListView()
        case .item(let id, _):
            ItemDetailView(itemID: id)
        case .pdf(let id, let path, let title, let page):
            PDFReaderView(url: URL(fileURLWithPath: path), attachmentID: id, title: title, page: page)
                .id("\(id):\(page ?? -1)")
        case .find:
            FullTextSearchView()
        case .search:
            OnlineSearchView()
        case .browser:
            BrowserView()
        case .graph:
            NoteGraphView(center: nil, depth: 0)
        case .canvas(let path):
            CanvasBoardView(path: path)
                .id(path)
        case .drawing(let path):
            DrawingBoardView(path: path)
                .id(path)
        case .diagram(let path):
            DiagramBoardView(path: path)
                .id(path)
        case .notes:
            VaultView()
                .navigationTitle("Notes")
        case .settings:
            SettingsView()
        }
    }
}
