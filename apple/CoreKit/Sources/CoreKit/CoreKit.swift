import Foundation

/// Helpers around the generated bindings (`PaperLibrary`, `ItemSummary`,
/// `normalizeDoi(input:)`…), which live in Generated/paperloom_core.swift.
public enum CoreKit {
    /// The library database in Application Support (inside the app's
    /// container when sandboxed), with its folder created on first use.
    public static func defaultLibraryURL() throws -> URL {
        let support = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        let folder = support.appendingPathComponent("Paperloom", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder.appendingPathComponent("library.sqlite")
    }

    /// A sentence to show for an error thrown by the core, in the app's
    /// language. Every `CoreError` case carries a `message` written for
    /// people, in English; Core's message catalog translates it.
    public static func message(for error: Error) -> String {
        guard error is CoreError else { return error.localizedDescription }
        return localizeMessage(message: englishMessage(for: error), language: language)
    }

    /// The language the app shows, which Core's messages follow.
    public static var language: String {
        Bundle.main.preferredLocalizations.first ?? "en"
    }

    private static func englishMessage(for error: Error) -> String {
        guard let payload = Mirror(reflecting: error).children.first?.value else {
            return String(describing: error)
        }
        if let message = payload as? String {
            return message
        }
        let field = Mirror(reflecting: payload).children.first { $0.label == "message" }
        return field?.value as? String ?? String(describing: error)
    }
}

extension ItemSummary: Identifiable {}
extension CollectionInfo: Identifiable {}
extension SearchSource: Identifiable {}
extension SruCatalogue: Identifiable {}
extension ItemDetail: Identifiable {}
extension FullTextHit: Identifiable {
    public var id: String {
        [kind, itemId, attachmentId, annotationId, notePath, pageIndex.map { String($0) }]
            .map { $0 ?? "" }.joined(separator: "|")
    }
}
extension NoteInfo: Identifiable {
    public var id: String { path }
}
extension SearchHit: Identifiable {
    public var id: String { key }
}
extension SavedSearch: Identifiable {}
extension FeedInfo: Identifiable {}
extension FeedItem: Identifiable {}
extension SearchField: Identifiable {}
extension DuplicateGroup: Identifiable {
    /// A group is known by its newest item.
    public var id: String { items.first?.id ?? "" }
}
