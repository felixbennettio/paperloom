import CoreKit
import SwiftUI

/// The items related to this one, as Zotero relates them: open one, add
/// one found by title, author or DOI, or take one off.
struct RelatedSection: View {
    let itemID: String
    @Environment(LibraryModel.self) private var library
    @Environment(TabsModel.self) private var tabs
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    @State private var picking = false

    var body: some View {
        let _ = library.revision
        let related = library.relatedItems(itemID: itemID)
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("Related").font(.headline)
                Spacer()
                Button("Add Related Item…", systemImage: "link.badge.plus") { picking = true }
                    .buttonStyle(.glass)
            }
            if related.isEmpty {
                Text("No related items yet.")
                    .foregroundStyle(.secondary)
            }
            ForEach(related) { item in
                HStack(spacing: 12) {
                    Image(systemName: "link")
                        .foregroundStyle(.secondary)
                    openLink(item)
                    Button("Remove Relation", systemImage: "xmark.circle") {
                        library.unrelate(itemID, item.id)
                    }
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                }
                .padding(12)
                .glassEffect(.regular, in: .rect(cornerRadius: 14))
            }
        }
        .sheet(isPresented: $picking) {
            RelatedItemPicker(itemID: itemID, excluded: Set(related.map(\.id) + [itemID]))
        }
    }

    @ViewBuilder
    private func openLink(_ item: ItemSummary) -> some View {
        #if os(iOS)
        if sizeClass == .compact {
            NavigationLink(value: item.id) {
                ItemRow(item: item)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .buttonStyle(.plain)
        } else {
            openInTab(item)
        }
        #else
        openInTab(item)
        #endif
    }

    private func openInTab(_ item: ItemSummary) -> some View {
        Button {
            tabs.open(.item(id: item.id, title: item.title))
        } label: {
            ItemRow(item: item)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// Picks an item to relate to another, found by title, author or DOI.
struct RelatedItemPicker: View {
    let itemID: String
    /// The item itself and those already related.
    let excluded: Set<String>
    @Environment(LibraryModel.self) private var library
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @State private var items: [ItemSummary] = []

    var body: some View {
        NavigationStack {
            List(matches) { item in
                Button {
                    library.relate(itemID, item.id)
                    dismiss()
                } label: {
                    ItemRow(item: item)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            .overlay {
                if matches.isEmpty && !query.isEmpty {
                    ContentUnavailableView.search(text: query)
                }
            }
            .searchable(text: $query, prompt: Text("Titles, authors, DOIs"))
            .navigationTitle("Related")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
            .onAppear { items = library.allItems() }
        }
        #if os(macOS)
        .frame(minWidth: 520, minHeight: 480)
        #endif
    }

    private var matches: [ItemSummary] {
        let words = query.trimmingCharacters(in: .whitespaces)
        let candidates = items.filter { !excluded.contains($0.id) }
        guard !words.isEmpty else { return candidates }
        return candidates.filter { item in
            item.title.localizedCaseInsensitiveContains(words)
                || item.authors.localizedCaseInsensitiveContains(words)
                || (item.doi?.localizedCaseInsensitiveContains(words) ?? false)
        }
    }
}
