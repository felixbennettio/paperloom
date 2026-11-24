import CoreKit
import CoreSpotlight
import Foundation
import OSLog
import UniformTypeIdentifiers

/// Keeps Spotlight's copy of the library and the notes, off the UI actor and
/// regardless of the library's current filter. After the first update since
/// launch (which rebuilds both domains, as the index may hold anything), only
/// entries whose summary changed are read in full and written again, and
/// gone ones are deleted: rebuilding everything on each edit was costly for
/// large libraries and kept Add Paper waiting.
actor SpotlightIndex {
    private var pending: Task<Void, Never>?
    private var revision = 0
    /// What each indexed entry was built from, by its identifier.
    private var indexed: [String: String] = [:]
    private var rebuilt = false
    private let log = Logger(subsystem: "io.felixbennett.paperloom", category: "Spotlight")

    func update(library: PaperLibrary?, vault: PaperVault?, enabled: Bool) {
        revision += 1
        let previous = pending
        previous?.cancel()
        pending = Task {
            // A cancelled request may still be inside a Spotlight write. Finish
            // it before deleting or replacing its index, especially on opt-out.
            await previous?.value
            do {
                try Task.checkCancellation()
                let index = CSSearchableIndex.default()
                guard enabled else {
                    try await index.deleteSearchableItems(withDomainIdentifiers: ["items", "notes"])
                    indexed = [:]
                    rebuilt = false
                    return
                }
                try await Task.sleep(for: .milliseconds(750))
                try Task.checkCancellation()
                guard let library, let vault else { return }
                try await refresh(index, library: library, vault: vault)
            } catch is CancellationError {
                return
            } catch {
                log.error("Spotlight update failed: \(error.localizedDescription)")
            }
        }
    }

    /// A background intent keeps running until its index writes have finished.
    func finishUpdates() async {
        while true {
            let current = revision
            await pending?.value
            if current == revision { return }
        }
    }

    private func refresh(_ index: CSSearchableIndex, library: PaperLibrary, vault: PaperVault) async throws {
        // Summaries are cheap; an entry is read in full only when its summary changed.
        let filter = ItemFilter(collectionId: nil, tag: nil, starredOnly: false, readStatus: nil, trashed: false)
        var current: [String: String] = [:]
        var items: [String: ItemSummary] = [:]
        for item in try library.listItems(filter: filter) {
            let id = "item:\(item.id)"
            current[id] = [item.title, item.authors, item.year.map(String.init) ?? "", item.containerTitle ?? "",
                           item.doi ?? ""].joined(separator: "\u{1F}")
            items[id] = item
        }
        var notes: [String: NoteInfo] = [:]
        for note in try vault.notes() {
            let id = "note:\(note.path)"
            current[id] = "\(note.title)\u{1F}\(note.modified)\u{1F}\(note.size)"
            notes[id] = note
        }
        let changed = current.filter { !rebuilt || indexed[$0.key] != $0.value }.map(\.key)
        let gone = rebuilt ? indexed.keys.filter { current[$0] == nil } : []

        var entries: [CSSearchableItem] = []
        for id in changed {
            try Task.checkCancellation()
            if let item = items[id], let detail = try library.itemDetail(itemId: item.id) {
                entries.append(itemEntry(id, detail))
            } else if let note = notes[id] {
                entries.append(noteEntry(id, note, text: try vault.readNote(path: note.path)))
            }
        }
        try Task.checkCancellation()
        // Built first: a failed library or vault read must not erase a good index.
        if !rebuilt {
            try await index.deleteSearchableItems(withDomainIdentifiers: ["items", "notes"])
        } else if !gone.isEmpty {
            try await index.deleteSearchableItems(withIdentifiers: gone)
        }
        for start in stride(from: 0, to: entries.count, by: 200) {
            try Task.checkCancellation()
            try await index.indexSearchableItems(Array(entries[start..<min(start + 200, entries.count)]))
        }
        indexed = current
        rebuilt = true
    }

    private func itemEntry(_ id: String, _ detail: ItemDetail) -> CSSearchableItem {
        let attributes = CSSearchableItemAttributeSet(contentType: .text)
        attributes.title = detail.title
        attributes.authorNames = detail.authors
        let metadata = [detail.authors.joined(separator: ", "), detail.year.map(String.init),
                        detail.containerTitle, detail.doi].compactMap { $0 }.filter { !$0.isEmpty }
        attributes.contentDescription = (metadata + [String((detail.abstractNote ?? "").prefix(1000))])
            .joined(separator: "\n")
        attributes.keywords = detail.authors + [detail.year.map(String.init), detail.containerTitle, detail.doi]
            .compactMap { $0 }
        return searchable(id, domain: "items", attributes: attributes)
    }

    private func noteEntry(_ id: String, _ note: NoteInfo, text: String) -> CSSearchableItem {
        let attributes = CSSearchableItemAttributeSet(contentType: .text)
        attributes.title = note.title
        attributes.contentDescription = String(text.prefix(1000))
        return searchable(id, domain: "notes", attributes: attributes)
    }

    private func searchable(_ id: String, domain: String, attributes: CSSearchableItemAttributeSet) -> CSSearchableItem {
        let item = CSSearchableItem(uniqueIdentifier: id, domainIdentifier: domain, attributeSet: attributes)
        item.expirationDate = .distantFuture
        return item
    }
}
