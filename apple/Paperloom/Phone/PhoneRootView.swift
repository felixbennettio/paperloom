#if os(iOS)
import CoreKit
import SwiftUI
import UniformTypeIdentifiers

/// The iPhone layout: a Liquid Glass tab bar with the library, search,
/// reading, notes and settings.
struct PhoneRootView: View {
    private enum Place: Hashable { case library, reading, notes, settings, search }
    @Environment(LibraryModel.self) private var library
    @Environment(VaultModel.self) private var vault
    @Environment(PluginHost.self) private var plugins
    @Environment(\.scenePhase) private var scenePhase
    @State private var selectionWindowID = UUID()
    @State private var tabs = TabsModel()
    @State private var place = Place.library
    @State private var libraryPath: [String] = []
    @State private var notesPath: [String] = []

    var body: some View {
        TabView(selection: $place) {
            Tab("Library", systemImage: "books.vertical", value: Place.library) {
                NavigationStack(path: $libraryPath) {
                    PhoneLibraryView(path: $libraryPath)
                }
            }
            Tab("Reading", systemImage: "book", value: Place.reading) {
                NavigationStack {
                    PhoneReadingView()
                }
            }
            Tab("Notes", systemImage: "note.text", value: Place.notes) {
                NavigationStack(path: $notesPath) {
                    PhoneNotesView(path: $notesPath)
                }
            }
            Tab("Settings", systemImage: "gearshape", value: Place.settings) {
                NavigationStack {
                    SettingsView()
                }
            }
            Tab(value: Place.search, role: .search) {
                NavigationStack {
                    PhoneSearchView()
                }
            }
        }
        .tabBarMinimizeBehavior(.onScrollDown)
        .environment(tabs)
        .onAppear {
            AppleServices.shared.navigation.register(selectionWindowID, handler: open)
            plugins.registerSelectionHandler(for: selectionWindowID) { keys in
                guard let key = keys.first else { return }
                guard library.detail(id: key) != nil else { throw PluginItemError.unknownItem }
                place = .library
                libraryPath = [key]
            }
            focusSelectionWindow()
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
        .onChange(of: libraryPath) { _, path in
            if let key = path.last, plugins.selectedItemKeys.first != key {
                plugins.selectionChanged(itemKeys: [key])
            }
        }
        .onChange(of: place) { _, place in
            if place == .library, let key = libraryPath.last, plugins.selectedItemKeys.first != key {
                plugins.selectionChanged(itemKeys: [key])
            }
        }
        .libraryNotices()
    }

    private func focusSelectionWindow() {
        guard scenePhase == .active else { return }
        plugins.focusSelectionWindow(selectionWindowID)
        AppleServices.shared.navigation.focus(selectionWindowID)
    }

    private func open(_ destination: AppleDestination) {
        switch destination {
        case .item(let id):
            place = .library
            libraryPath = [id]
        case .note(let path):
            guard vault.notes.contains(where: { $0.path == path }) else { return }
            vault.open(path)
            guard vault.path == path else { return }
            place = .notes
            notesPath = [path]
        }
    }
}

/// The iPhone's Reading tab: the papers marked In Progress.
struct PhoneReadingView: View {
    @Environment(LibraryModel.self) private var library
    @State private var items: [ItemSummary] = []

    var body: some View {
        List(items) { item in
            NavigationLink(value: item.id) {
                ItemRow(item: item)
            }
        }
        .overlay {
            if items.isEmpty {
                ContentUnavailableView(
                    "Nothing Open", systemImage: "book",
                    description: Text("Papers you are reading will appear here."))
            }
        }
        .navigationDestination(for: String.self) { ItemDetailView(itemID: $0) }
        .navigationTitle("Reading")
        .task(id: library.revision) { items = library.items(readStatus: "reading") }
    }
}

/// The iPhone search tab: this library (full text), online databases, or
/// the web with the person's own access.
struct PhoneSearchView: View {
    enum Place: Hashable {
        case library, online, web
    }

    @State private var place = Place.library

    var body: some View {
        Group {
            switch place {
            case .library: FullTextSearchView()
            case .online: OnlineSearchView()
            case .web: BrowserView()
            }
        }
        // Related-item links from a search result also use value navigation.
        .navigationDestination(for: String.self) { ItemDetailView(itemID: $0) }
        .toolbar {
            ToolbarItem(placement: .principal) {
                Picker("Search", selection: $place) {
                    Text("This Library").tag(Place.library)
                    Text("Online").tag(Place.online)
                    Text("Web").tag(Place.web)
                }
                .pickerStyle(.segmented)
                .fixedSize()
            }
        }
    }
}

struct PhoneLibraryView: View {
    @Binding var path: [String]
    @Environment(LibraryModel.self) private var library
    @Environment(PluginHost.self) private var plugins
    @State private var showingAddItem = false
    @State private var choosingPDFs = false
    @State private var creatingCollection = false
    @State private var newCollectionName = ""
    @State private var editingSearch: SavedSearch?
    @State private var addingFeed = false
    @State private var columnValues: [String: [String: String]] = [:]

    var body: some View {
        List {
            if case .feed(let id) = library.scope {
                FeedEntries(feedID: id) { itemID, _ in
                    plugins.selectionChanged(itemKeys: [itemID])
                    path = [itemID]
                }
            } else if library.scope == .duplicates {
                DuplicatesView {
                    plugins.selectionChanged(itemKeys: [$0.id])
                    path = [$0.id]
                }
            } else {
                items
            }
        }
        .navigationDestination(for: String.self) { ItemDetailView(itemID: $0) }
        .navigationTitle(library.scopeTitle)
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                ImportExportMenu(itemIDs: library.items.map(\.id))
            }
            ToolbarItem(placement: .topBarLeading) {
                Menu {
                    Picker("Show", selection: Binding(get: { library.scope }, set: { library.scope = $0 })) {
                        Label("All Items", systemImage: "books.vertical").tag(LibraryScope.all)
                        Label("Starred", systemImage: "star").tag(LibraryScope.starred)
                        Label("Unread", systemImage: "circle").tag(LibraryScope.unread)
                        Label("Duplicate Items", systemImage: "square.on.square").tag(LibraryScope.duplicates)
                        Label("Trash", systemImage: "trash").tag(LibraryScope.trash)
                    }
                    // A submenu: a menu shows no heading over an inline
                    // picker, and a library can have many tags.
                    if !library.tags.isEmpty {
                        Picker(selection: Binding(get: { library.scope }, set: { library.scope = $0 })) {
                            ForEach(library.tags, id: \.name) { tag in
                                Text(tag.name).tag(LibraryScope.tag(tag.name))
                            }
                        } label: {
                            Label("Tags", systemImage: "tag")
                        }
                        .pickerStyle(.menu)
                    }
                    Section("Saved Searches") {
                        Picker("Saved Searches", selection: Binding(get: { library.scope }, set: { library.scope = $0 })) {
                            ForEach(library.savedSearches) { search in
                                Label(search.name, systemImage: "folder.badge.gearshape")
                                    .tag(LibraryScope.savedSearch(search.id))
                            }
                        }
                        if case .savedSearch(let id) = library.scope,
                            let current = library.savedSearches.first(where: { $0.id == id })
                        {
                            Button("Edit Saved Search", systemImage: "slider.horizontal.3") { editingSearch = current }
                        }
                        Button("New Saved Search", systemImage: "plus") { editingSearch = library.newSearch() }
                    }
                    Section("Feeds") {
                        Picker("Feeds", selection: Binding(get: { library.scope }, set: { library.scope = $0 })) {
                            ForEach(library.feeds) { feed in
                                Label(feed.title, systemImage: "dot.radiowaves.up.forward")
                                    .badge(Int(feed.unread))
                                    .tag(LibraryScope.feed(feed.id))
                            }
                        }
                        if case .feed(let id) = library.scope {
                            Button("Unsubscribe", systemImage: "minus.circle", role: .destructive) {
                                library.removeFeed(id)
                            }
                        }
                        Button("Add Feed", systemImage: "plus") { addingFeed = true }
                    }
                    if library.scope == .trash && !library.items.isEmpty {
                        Button("Empty Trash", systemImage: "trash.slash", role: .destructive) {
                            library.emptyTrash()
                        }
                    }
                } label: {
                    Label("Show", systemImage: "line.3.horizontal.decrease.circle")
                }
            }
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Button("Add Item", systemImage: "doc.badge.plus") { showingAddItem = true }
                    Button("Add PDF…", systemImage: "doc.richtext") { choosingPDFs = true }
                    Button("New Collection", systemImage: "folder.badge.plus") {
                        newCollectionName = ""
                        creatingCollection = true
                    }
                } label: {
                    Label("Add", systemImage: "plus")
                }
            }
        }
        .sheet(isPresented: $showingAddItem) {
            AddItemSheet()
        }
        .sheet(item: $editingSearch) { search in
            SavedSearchEditor(search: search)
        }
        .modifier(AddFeedAlert(isPresented: $addingFeed))
        .task { await library.refreshFeeds() }
        .fileImporter(isPresented: $choosingPDFs, allowedContentTypes: [.pdf], allowsMultipleSelection: true) { result in
            guard case .success(let urls) = result, !urls.isEmpty else { return }
            Task { await library.importPDFs(urls) }
        }
        .alert("New Collection", isPresented: $creatingCollection) {
            TextField("Name", text: $newCollectionName)
            Button("Create") { library.createCollection(named: newCollectionName) }
            Button("Cancel", role: .cancel) {}
        }
    }

    /// The collection chips and the items of the scope.
    @ViewBuilder private var items: some View {
        if !library.collections.isEmpty {
            Section("Collections") {
                ScrollView(.horizontal, showsIndicators: false) {
                    GlassEffectContainer(spacing: 8) {
                        HStack(spacing: 8) {
                            chip(title: String(localized: "All Items"), id: nil)
                            ForEach(library.collections) { collection in
                                chip(title: collection.name, id: collection.id)
                            }
                        }
                        .padding(.vertical, 4)
                    }
                }
                .listRowInsets(EdgeInsets(top: 4, leading: 12, bottom: 4, trailing: 12))
            }
        }
        Section {
            if library.items.isEmpty {
                if case .savedSearch = library.scope {
                    ContentUnavailableView("No matches", systemImage: "magnifyingglass")
                } else {
                    ContentUnavailableView(
                        "No Items Yet", systemImage: "books.vertical",
                        description: Text("Add a paper with the + button: paste its DOI or fill in the details."))
                }
            }
            ForEach(library.items) { item in
                NavigationLink(value: item.id) {
                    ItemRow(item: item, columns: plugins.columns, columnValues: columnValues)
                }
                .modifier(ItemContextMenu(item: item))
                .swipeActions(edge: .leading) {
                    if library.scope != .trash {
                        Button {
                            library.setStarred(!item.starred, id: item.id)
                        } label: {
                            Label(
                                item.starred ? LocalizedStringKey("Remove Star") : LocalizedStringKey("Add Star"),
                                systemImage: item.starred ? "star.slash" : "star")
                        }
                        .tint(.yellow)
                    }
                }
                .swipeActions {
                    if library.scope == .trash {
                        Button("Delete Forever", systemImage: "trash.slash", role: .destructive) {
                            library.deleteItemForever(id: item.id)
                        }
                        Button("Restore", systemImage: "arrow.uturn.backward") {
                            library.restoreItem(id: item.id)
                        }
                    } else {
                        Button("Move to Trash", systemImage: "trash", role: .destructive) {
                            library.trashItem(id: item.id)
                        }
                    }
                }
            }
        }
        .task(id: [library.items.map(\.id), plugins.columns.flatMap { [$0.id, $0.label] }, [String(library.revision)]]) {
            columnValues = [:]
            let items = library.items.map(\.id)
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

    private func chip(title: String, id: String?) -> some View {
        let selected = library.selectedCollectionID == id
        return Button {
            library.selectedCollectionID = id
        } label: {
            Text(title)
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
#endif
