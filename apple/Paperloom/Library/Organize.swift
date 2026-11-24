import CoreKit
import SwiftUI

extension Color {
    /// `#rrggbb`.
    init?(hex: String) {
        guard let platform = PlatformColor(hex: hex) else { return nil }
        #if os(macOS)
        self.init(nsColor: platform)
        #else
        self.init(uiColor: platform)
        #endif
    }
}

/// A tag's color as a dot; a plain tag shows a tag outline.
struct TagDot: View {
    let color: String?

    var body: some View {
        if let color, let fill = Color(hex: color) {
            Image(systemName: "circle.fill").foregroundStyle(fill)
        } else {
            Image(systemName: "tag").foregroundStyle(.secondary)
        }
    }
}

/// Names and symbols of the reading statuses the core uses.
enum ReadStatusName {
    static let all = ["unread", "reading", "read"]

    static func title(_ status: String) -> LocalizedStringKey {
        switch status {
        case "reading": "In Progress"
        case "read": "Finished"
        default: "Unread"
        }
    }

    static func systemImage(_ status: String) -> String {
        switch status {
        case "reading": "book"
        case "read": "checkmark.circle"
        default: "circle"
        }
    }
}

/// Orders for item lists.
enum ItemSort: String, CaseIterable, Identifiable {
    case dateAdded, title, authors, year, citations

    var id: String { rawValue }

    var title: LocalizedStringKey {
        switch self {
        case .dateAdded: "Date Added"
        case .title: "Title"
        case .authors: "Authors"
        case .year: "Year"
        case .citations: "Cited by"
        }
    }

    func sorted(_ items: [ItemSummary]) -> [ItemSummary] {
        switch self {
        case .dateAdded:
            items
        case .title:
            items.sorted { $0.title.localizedStandardCompare($1.title) == .orderedAscending }
        case .authors:
            items.sorted { $0.authors.localizedStandardCompare($1.authors) == .orderedAscending }
        case .year:
            items.sorted { ($0.year ?? Int32.min) > ($1.year ?? Int32.min) }
        case .citations:
            items.sorted { ($0.citedBy ?? 0) > ($1.citedBy ?? 0) }
        }
    }
}

/// What can be done with an item from a list: star it, set its reading
/// status, move it to the trash, or restore or delete it from the trash.
struct ItemMenu: View {
    let item: ItemSummary
    var pluginMenus: [PluginItemMenu] = []
    /// Called when the item leaves the library (its tab should close).
    var onRemoved: () -> Void = {}
    @Environment(LibraryModel.self) private var library
    @Environment(PluginHost.self) private var plugins

    var body: some View {
        if library.scope == .trash {
            Button("Restore", systemImage: "arrow.uturn.backward") { library.restoreItem(id: item.id) }
            Button("Delete Forever", systemImage: "trash.slash", role: .destructive) {
                library.deleteItemForever(id: item.id)
                onRemoved()
            }
        } else {
            Button(
                item.starred ? LocalizedStringKey("Remove Star") : LocalizedStringKey("Add Star"),
                systemImage: item.starred ? "star.slash" : "star"
            ) {
                library.setStarred(!item.starred, id: item.id)
            }
            Picker(
                "Reading Status",
                selection: Binding(get: { item.readStatus }, set: { library.setReadStatus($0, id: item.id) })
            ) {
                ForEach(ReadStatusName.all, id: \.self) { status in
                    Label(ReadStatusName.title(status), systemImage: ReadStatusName.systemImage(status)).tag(status)
                }
            }
            Button("Move to Trash", systemImage: "trash", role: .destructive) {
                library.trashItem(id: item.id)
                onRemoved()
            }
        }
        if pluginMenus.contains(where: { !$0.entries.isEmpty }) {
            Divider()
            ForEach(pluginMenus, id: \.plugin) { menu in
                ForEach(Array(menu.entries.enumerated()), id: \.offset) { _, entry in
                    if case .submenu(let label, let disabled, let children) = entry {
                        Menu {
                            ForEach(Array(children.enumerated()), id: \.offset) { _, child in
                                pluginCommand(child, menu: menu, submenu: label)
                            }
                        } label: {
                            Text(verbatim: label)
                        }
                        .disabled(disabled)
                    } else {
                        pluginCommand(entry, menu: menu)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func pluginCommand(_ entry: PluginItemMenuEntry, menu: PluginItemMenu, submenu: String? = nil) -> some View {
        switch entry {
        case .command(let command):
            Button {
                runPluginItem(command, menu: menu, path: submenu.map { [$0, command.label] } ?? [command.label])
            } label: {
                Text(verbatim: command.label)
            }
            .disabled(command.disabled)
        case .separator:
            Divider()
        case .submenu:
            EmptyView()
        }
    }

    private func runPluginItem(_ command: ReaderMenuItem, menu: PluginItemMenu, path: [String]) {
        plugins.itemCommand(plugin: menu.plugin, menu: menu.menu, index: command.index, path: path, itemKeys: [item.id])
    }
}

/// An item's tags as chips, with adding (new or existing names) and removing.
struct TagEditor: View {
    let itemID: String
    let tags: [String]
    @Environment(LibraryModel.self) private var library
    @State private var newTag = ""

    var body: some View {
        // Tags, then the ways to add one, wrapping in a narrow pane.
        FlowLayout(spacing: 6, lineSpacing: 6) {
            ForEach(tags, id: \.self) { tag in
                HStack(spacing: 4) {
                    TagDot(color: library.color(ofTag: tag)).font(.caption)
                    Text(tag)
                    Button {
                        library.setTags(tags.filter { $0 != tag }, id: itemID)
                    } label: {
                        Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary)
                            .touchTarget(width: 36, height: 36)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(Text("Remove"))
                    // The target reaches past the chip, which keeps its size.
                    #if os(iOS)
                    .padding(-10)
                    #endif
                }
                .font(.callout)
                .padding(.horizontal, 10)
                .padding(.vertical, 5)
                .glassEffect(.regular, in: .capsule)
            }
            let others = library.tags.filter { !tags.contains($0.name) }
            if !others.isEmpty {
                Menu {
                    ForEach(others, id: \.name) { tag in
                        Button(tag.name) { library.setTags(tags + [tag.name], id: itemID) }
                    }
                } label: {
                    Label("Tags", systemImage: "tag")
                }
                .fixedSize()
            }
            TextField("Add Tag", text: $newTag)
                .textFieldStyle(.roundedBorder)
                .frame(width: 160)
                .onSubmit {
                    let name = newTag.trimmingCharacters(in: .whitespacesAndNewlines)
                    newTag = ""
                    if !name.isEmpty && !tags.contains(name) {
                        library.setTags(tags + [name], id: itemID)
                    }
                }
        }
    }
}

/// Edits every detail of an item.
struct EditItemSheet: View {
    @Environment(LibraryModel.self) private var library
    @Environment(\.dismiss) private var dismiss
    @State private var detail: ItemDetail
    @State private var error: String?
    private let types = itemTypes()

    init(detail: ItemDetail) {
        _detail = State(initialValue: detail)
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Picker("Type", selection: $detail.itemType) {
                        ForEach(types, id: \.self) { id in
                            Text(ItemTypeName.title(for: id)).tag(id)
                        }
                    }
                    TextField("Title", text: $detail.title, axis: .vertical)
                    TextField("Authors", text: lines(\.authors), prompt: Text("One per line"), axis: .vertical)
                    TextField("Editors", text: lines(\.editors), prompt: Text("One per line"), axis: .vertical)
                }
                Section {
                    TextField("Publication", text: text(\.containerTitle))
                    TextField("Short title", text: text(\.containerShortTitle))
                    TextField("Date", text: text(\.date), prompt: Text(verbatim: "2024-05-01"))
                    TextField("Year", text: year)
                    TextField("Volume", text: text(\.volume))
                    TextField("Issue", text: text(\.issue))
                    TextField("Pages", text: text(\.pages))
                    TextField("Publisher", text: text(\.publisher))
                    TextField("Place", text: text(\.place))
                    TextField("Edition", text: text(\.edition))
                    TextField("Number", text: text(\.number))
                    TextField("Language", text: text(\.language), prompt: Text(verbatim: "en, zh-CN…"))
                }
                Section {
                    TextField(text: text(\.doi), prompt: Text(verbatim: "10.1038/…")) { Text(verbatim: "DOI") }
                    TextField(text: text(\.isbn)) { Text(verbatim: "ISBN") }
                    TextField(text: text(\.issn)) { Text(verbatim: "ISSN") }
                    TextField(text: text(\.arxivId)) { Text(verbatim: "arXiv") }
                    TextField(text: text(\.pmid)) { Text(verbatim: "PMID") }
                    TextField(text: text(\.url)) { Text(verbatim: "URL") }
                }
                Section {
                    TextField("Keywords", text: keywords, prompt: Text("Separated by commas"), axis: .vertical)
                    TextField("Abstract", text: text(\.abstractNote), axis: .vertical)
                        .lineLimit(4...12)
                    TextField("Extra", text: text(\.extra), axis: .vertical)
                        .lineLimit(4...12)
                }
                if let error {
                    Section {
                        Label(error, systemImage: "exclamationmark.octagon").foregroundStyle(.red)
                    }
                }
            }
            .formStyle(.grouped)
            .navigationTitle("Edit Item")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        if let problem = library.updateItem(detail) {
                            error = problem
                        } else {
                            dismiss()
                        }
                    }
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 520, minHeight: 600)
        #endif
    }

    private func text(_ field: WritableKeyPath<ItemDetail, String?>) -> Binding<String> {
        Binding(get: { detail[keyPath: field] ?? "" }, set: { detail[keyPath: field] = $0.isEmpty ? nil : $0 })
    }

    private func lines(_ field: WritableKeyPath<ItemDetail, [String]>) -> Binding<String> {
        Binding(
            get: { detail[keyPath: field].joined(separator: "\n") },
            set: { detail[keyPath: field] = $0.components(separatedBy: "\n") })
    }

    private var keywords: Binding<String> {
        Binding(
            get: { detail.keywords.joined(separator: ", ") },
            set: { detail.keywords = $0.components(separatedBy: CharacterSet(charactersIn: ",;，；")) })
    }

    private var year: Binding<String> {
        Binding(
            get: { detail.year.map { String($0) } ?? "" },
            set: { detail.year = Int32($0.trimmingCharacters(in: .whitespaces)) })
    }
}
