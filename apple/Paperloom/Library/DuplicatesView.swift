import CoreKit
import SwiftUI

/// Items that look like the same work, as Zotero's Duplicate Items shows
/// them: each group with the item to keep picked (the oldest, at first) and
/// merged into it with one tap.
struct DuplicatesView: View {
    /// Opens an item.
    var open: (ItemSummary) -> Void
    @Environment(LibraryModel.self) private var library
    @State private var groups: [DuplicateGroup] = []
    /// The item each group keeps, by the group's id.
    @State private var kept: [String: String] = [:]

    /// Sections for a list: the hint, then each group.
    var body: some View {
        Group {
            Section {
                Text(
                    "Items with the same DOI, ISBN, arXiv or PubMed ID, or the same title, close years and an author in common. Merging keeps the item you pick, fills its gaps from the others and brings their files, tags and collections to it; the others go to the trash."
                )
                .font(.callout)
                .foregroundStyle(.secondary)
                if groups.isEmpty {
                    ContentUnavailableView("No Duplicates", systemImage: "square.on.square")
                }
            }
            .onAppear(perform: load)
            .onChange(of: library.revision) { load() }
            ForEach(groups) { group in
                Section {
                    ForEach(group.items) { item in
                        row(item, in: group)
                    }
                    Button("Merge \(String(group.items.count)) Items", systemImage: "arrow.triangle.merge") {
                        let master = keeper(of: group)
                        library.mergeItems(master: master, others: group.items.map(\.id).filter { $0 != master })
                    }
                }
            }
        }
    }

    private func row(_ item: ItemSummary, in group: DuplicateGroup) -> some View {
        let isKept = keeper(of: group) == item.id
        return HStack(alignment: .top, spacing: 10) {
            Button {
                kept[group.id] = item.id
            } label: {
                Image(systemName: isKept ? "largecircle.fill.circle" : "circle")
                    .foregroundStyle(isKept ? Color.accentColor : .secondary)
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Text(item.title))
            .accessibilityAddTraits(isKept ? .isSelected : [])
            VStack(alignment: .leading, spacing: 2) {
                ItemRow(item: item)
                Text("Added \(added(item))")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
            .onTapGesture { open(item) }
        }
    }

    /// The item a group keeps: the one picked, else the oldest.
    private func keeper(of group: DuplicateGroup) -> String {
        if let id = kept[group.id], group.items.contains(where: { $0.id == id }) { return id }
        return group.items.last?.id ?? ""
    }

    private func added(_ item: ItemSummary) -> String {
        Date(timeIntervalSince1970: Double(item.dateAdded) / 1000).formatted(date: .abbreviated, time: .omitted)
    }

    private func load() {
        groups = library.duplicateGroups()
    }
}
