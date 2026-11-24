import CoreKit
import SwiftUI

/// What a feed announced, as Zotero shows a feed: newest first, unread in
/// bold; an entry opens to its abstract and goes into the library with one
/// tap (its full record looked up by DOI). Sections for a list.
struct FeedEntries: View {
    let feedID: String
    /// Opens a library item, by its ID and title.
    var openItem: (String, String) -> Void
    @Environment(LibraryModel.self) private var library
    @Environment(\.openURL) private var openURL
    @State private var items: [FeedItem] = []
    @State private var expanded: String?
    @State private var working: String?
    @State private var refreshing = false
    @State private var problem: String?

    var body: some View {
        let feed = library.feeds.first { $0.id == feedID }
        Group {
            Section {
                HStack {
                    Button(
                        refreshing ? LocalizedStringKey("Working…") : LocalizedStringKey("Refresh"),
                        systemImage: "arrow.clockwise"
                    ) {
                        refreshing = true
                        Task {
                            await library.refreshFeeds()
                            refreshing = false
                        }
                    }
                    .disabled(refreshing)
                    Button("Mark All as Read", systemImage: "checkmark.circle") { library.markFeedRead(feedID) }
                        .disabled((feed?.unread ?? 0) == 0)
                }
                .buttonStyle(.borderless)
                if let message = feed?.problem ?? problem {
                    Text(message)
                        .font(.callout)
                        .foregroundStyle(.red)
                }
                if items.isEmpty {
                    ContentUnavailableView("Nothing in this feed yet", systemImage: "dot.radiowaves.up.forward")
                }
            } header: {
                if let feed {
                    Text(verbatim: feed.url).textCase(nil)
                }
            }
            .onAppear(perform: load)
            .onChange(of: library.revision) { load() }
            .onChange(of: feedID) { load() }
            Section {
                ForEach(items) { item in
                    row(item)
                }
            }
        }
    }

    private func row(_ item: FeedItem) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                if !item.read {
                    Circle()
                        .fill(Color.accentColor)
                        .frame(width: 7, height: 7)
                        .accessibilityLabel(Text("Unread"))
                }
                Text(verbatim: item.title)
                    .font(.headline.weight(item.read ? .regular : .semibold))
            }
            Text(verbatim: details(item))
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(2)
            if expanded == item.id, let abstract = item.abstractNote {
                Text(verbatim: abstract)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
            HStack(spacing: 16) {
                if let id = item.libraryItemId {
                    Button("Show in Library", systemImage: "books.vertical") { openItem(id, item.title) }
                } else {
                    Button(
                        working == item.id ? LocalizedStringKey("Working…") : LocalizedStringKey("Add to Library"),
                        systemImage: "plus.circle"
                    ) {
                        add(item)
                    }
                    .disabled(working != nil)
                }
                if let link = item.link, let url = URL(string: link) {
                    Button("Open Web Page", systemImage: "safari") { openURL(url) }
                }
            }
            .buttonStyle(.borderless)
            .font(.callout)
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
        .onTapGesture {
            withAnimation(.smooth) { expanded = expanded == item.id ? nil : item.id }
            if !item.read { library.setFeedItemRead(item.id, read: true) }
        }
        .contextMenu {
            if item.read {
                Button("Mark as Unread", systemImage: "circle") { library.setFeedItemRead(item.id, read: false) }
            }
        }
    }

    /// `Curie et al. · Nature Physics · 2025-06-10`
    private func details(_ item: FeedItem) -> String {
        [item.authors, item.containerTitle ?? "", item.date ?? item.year.map { String($0) } ?? ""]
            .filter { !$0.isEmpty }
            .joined(separator: " · ")
    }

    private func add(_ item: FeedItem) {
        working = item.id
        Task {
            problem = await library.addFeedItem(item.id)
            working = nil
        }
    }

    private func load() {
        items = library.feedItems(feedID: feedID)
    }
}

/// Asks for a feed's address and follows it, saying why when it cannot.
struct AddFeedAlert: ViewModifier {
    @Binding var isPresented: Bool
    @Environment(LibraryModel.self) private var library
    @State private var address = ""
    @State private var problem: String?

    func body(content: Content) -> some View {
        content
            .alert("Add Feed", isPresented: $isPresented) {
                TextField("Add Feed", text: $address)
                Button("Add") {
                    let url = address
                    address = ""
                    Task { problem = await library.addFeed(url) }
                }
                Button("Cancel", role: .cancel) { address = "" }
            } message: {
                Text("A journal's RSS or Atom address, or its web page")
            }
            .alert(
                "Add Feed", isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } })
            ) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(problem ?? "")
            }
    }
}
