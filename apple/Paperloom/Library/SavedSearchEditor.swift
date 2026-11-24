import CoreKit
import SwiftUI

/// Edits a saved search, as Zotero's search window does: a name, whether
/// items must pass all conditions or any, and the conditions, with how many
/// items they find as they stand.
struct SavedSearchEditor: View {
    @State var search: SavedSearch
    @Environment(LibraryModel.self) private var library
    @Environment(\.dismiss) private var dismiss
    @State private var error: String?
    @State private var count: Int?
    @State private var confirmingDelete = false

    private var fields: [SearchField] { library.conditionFields }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $search.name)
                    Picker("Match All Conditions", selection: $search.matchAll) {
                        Text("Match All Conditions").tag(true)
                        Text("Match Any Condition").tag(false)
                    }
                    .labelsHidden()
                }
                Section {
                    ForEach(search.conditions.indices, id: \.self) { index in
                        conditionRow(index)
                    }
                    Button("Add Condition", systemImage: "plus.circle") {
                        if let first = fields.first {
                            search.conditions.append(library.condition(on: first))
                        }
                    }
                } footer: {
                    if let count {
                        Text("Matching items: \(String(count))")
                    }
                }
                if let error {
                    Section {
                        Text(error).foregroundStyle(.red)
                    }
                }
                if !search.id.isEmpty {
                    Section {
                        Button("Delete Saved Search", role: .destructive) { confirmingDelete = true }
                    }
                }
            }
            .formStyle(.grouped)
            .navigationTitle(
                search.id.isEmpty ? LocalizedStringKey("New Saved Search") : LocalizedStringKey("Edit Saved Search"))
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        if let message = library.saveSearch(search) {
                            error = message
                        } else {
                            dismiss()
                        }
                    }
                    .disabled(search.name.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .confirmationDialog("Delete Saved Search", isPresented: $confirmingDelete) {
                Button("Delete Saved Search", role: .destructive) {
                    library.deleteSavedSearch(id: search.id)
                    dismiss()
                }
            }
            .task(id: search) {
                // How many items the conditions find, a moment after they change.
                try? await Task.sleep(for: .milliseconds(250))
                guard !Task.isCancelled else { return }
                count = library.matchCount(search)
            }
        }
        #if os(macOS)
        .frame(minWidth: 560, minHeight: 420)
        #endif
    }

    @ViewBuilder
    private func conditionRow(_ index: Int) -> some View {
        let condition = search.conditions[index]
        let field = fields.first { $0.id == condition.field }
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Picker("Field", selection: fieldBinding(index)) {
                    ForEach(fields) { field in
                        Text(SearchNames.field(field.id)).tag(field.id)
                    }
                }
                .labelsHidden()
                Picker("Comparison", selection: binding(index, \.op)) {
                    ForEach(field?.ops ?? [condition.op], id: \.self) { op in
                        Text(SearchNames.op(op)).tag(op)
                    }
                }
                .labelsHidden()
                Spacer(minLength: 0)
                Button("Remove Condition", systemImage: "minus.circle") {
                    search.conditions.remove(at: index)
                }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
            }
            valueEditor(index, kind: field?.kind ?? "text")
        }
    }

    @ViewBuilder
    private func valueEditor(_ index: Int, kind: String) -> some View {
        let value = binding(index, \.value)
        switch kind {
        case "itemType":
            Picker("Value", selection: value) {
                ForEach(itemTypes(), id: \.self) { id in
                    Text(ItemTypeName.title(for: id)).tag(id)
                }
            }
            .labelsHidden()
        case "readStatus":
            Picker("Value", selection: value) {
                ForEach(ReadStatusName.all, id: \.self) { status in
                    Text(ReadStatusName.title(status)).tag(status)
                }
            }
            .labelsHidden()
        case "collection":
            Picker("Value", selection: value) {
                ForEach(library.collections) { collection in
                    Text(collection.name).tag(collection.id)
                }
            }
            .labelsHidden()
        case "tag":
            Picker("Value", selection: value) {
                ForEach(library.tags, id: \.name) { tag in
                    Text(tag.name).tag(tag.name)
                }
            }
            .labelsHidden()
        case "flag":
            Picker("Value", selection: value) {
                Text("Yes").tag("true")
                Text("No").tag("false")
            }
            .pickerStyle(.segmented)
            .labelsHidden()
        case "days":
            HStack {
                TextField("Value", text: value)
                    #if os(iOS)
                    .keyboardType(.numberPad)
                    #endif
                Text("days").foregroundStyle(.secondary)
            }
        default:
            TextField("Value", text: value)
                #if os(iOS)
                .keyboardType(kind == "year" ? .numberPad : .default)
                #endif
        }
    }

    private func binding(_ index: Int, _ key: WritableKeyPath<SearchCondition, String>) -> Binding<String> {
        Binding(
            get: { search.conditions.indices.contains(index) ? search.conditions[index][keyPath: key] : "" },
            set: { if search.conditions.indices.contains(index) { search.conditions[index][keyPath: key] = $0 } })
    }

    /// Changing the field keeps the test, and the value, where the new
    /// field takes them.
    private func fieldBinding(_ index: Int) -> Binding<String> {
        Binding(
            get: { search.conditions.indices.contains(index) ? search.conditions[index].field : "" },
            set: { id in
                guard search.conditions.indices.contains(index), let next = fields.first(where: { $0.id == id })
                else { return }
                let old = search.conditions[index]
                let oldKind = fields.first { $0.id == old.field }?.kind
                search.conditions[index] = SearchCondition(
                    field: next.id,
                    op: next.ops.contains(old.op) ? old.op : (next.ops.first ?? "is"),
                    value: oldKind == next.kind ? old.value : library.defaultValue(for: next))
            })
    }
}

/// Display names of the fields and tests saved searches use.
enum SearchNames {
    static func field(_ id: String) -> LocalizedStringKey {
        switch id {
        case "anyField": "Any Field"
        case "title": "Title"
        case "creator": "Creator"
        case "publication": "Publication"
        case "year": "Year"
        case "tag": "Tag"
        case "collection": "Collection"
        case "itemType": "Item Type"
        case "keyword": "Keyword"
        case "abstract": "Abstract"
        case "doi": "DOI"
        case "publisher": "Publisher"
        case "readStatus": "Reading Status"
        case "starred": "Starred"
        case "hasPdf": "Has PDF"
        case "dateAdded": "Date Added"
        default: LocalizedStringKey(id)
        }
    }

    static func op(_ id: String) -> LocalizedStringKey {
        switch id {
        case "contains": "contains"
        case "doesNotContain": "does not contain"
        case "is": "is"
        case "isNot": "is not"
        case "isBefore": "is before"
        case "isAfter": "is after"
        case "inTheLast": "is in the last"
        case "notInTheLast": "is not in the last"
        default: LocalizedStringKey(id)
        }
    }
}
