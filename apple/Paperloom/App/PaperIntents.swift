import AppIntents
import CoreKit
import Foundation

struct PaperEntity: AppEntity, Sendable {
    static let typeDisplayRepresentation: TypeDisplayRepresentation = "Paper"
    static let defaultQuery = PaperQuery()

    let id: String
    let title: String
    let subtitle: String

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(title: "\(title)", subtitle: "\(subtitle)")
    }

    init(_ item: ItemSummary) {
        id = item.id
        title = item.title
        subtitle = [item.authors, item.year.map(String.init)].compactMap { $0 }.filter { !$0.isEmpty }
            .joined(separator: " · ")
    }

    init(_ item: ItemDetail) {
        id = item.id
        title = item.title
        subtitle = [item.authors.joined(separator: ", "), item.year.map(String.init)]
            .compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

struct PaperQuery: EntityStringQuery {
    @MainActor
    func entities(for identifiers: [String]) async throws -> [PaperEntity] {
        guard let core = AppleServices.shared.library.core else { throw LibraryUnavailable() }
        do {
            return try await Task.detached {
                let filter = ItemFilter(collectionId: nil, tag: nil, starredOnly: false, readStatus: nil, trashed: false)
                let wanted = Set(identifiers)
                let papers = try core.listItems(filter: filter).filter { wanted.contains($0.id) }.map(PaperEntity.init)
                let byID = Dictionary(uniqueKeysWithValues: papers.map { ($0.id, $0) })
                return identifiers.compactMap { byID[$0] }
            }.value
        } catch {
            throw PaperIntentError(message: CoreKit.message(for: error))
        }
    }

    @MainActor
    func entities(matching string: String) async throws -> [PaperEntity] {
        let text = string.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.isEmpty { return try await suggestedEntities() }
        guard let core = AppleServices.shared.library.core else { throw LibraryUnavailable() }
        do {
            return try await Task.detached {
                let hits = try core.searchEverything(query: text, kinds: ["item"], limit: 200)
                var seen = Set<String>()
                return try hits.compactMap { hit -> PaperEntity? in
                    guard let id = hit.itemId, seen.insert(id).inserted,
                          let item = try core.itemDetail(itemId: id) else { return nil }
                    return PaperEntity(item)
                }
            }.value
        } catch {
            throw PaperIntentError(message: CoreKit.message(for: error))
        }
    }

    @MainActor
    func suggestedEntities() async throws -> [PaperEntity] {
        guard let core = AppleServices.shared.library.core else { throw LibraryUnavailable() }
        do {
            return try await Task.detached {
                let filter = ItemFilter(collectionId: nil, tag: nil, starredOnly: false, readStatus: nil, trashed: false)
                return try core.listItems(filter: filter).prefix(20).map(PaperEntity.init)
            }.value
        } catch {
            throw PaperIntentError(message: CoreKit.message(for: error))
        }
    }
}

struct OpenPaperIntent: AppIntent {
    static let title: LocalizedStringResource = "Open Paper"
    static let supportedModes: IntentModes = .foreground(.immediate)

    @Parameter(title: "Paper") var paper: PaperEntity

    static var parameterSummary: some ParameterSummary {
        Summary { \.$paper }
    }

    @MainActor
    func perform() async throws -> some IntentResult {
        guard let current = try await PaperQuery().entities(for: [paper.id]).first else {
            throw PaperIntentError(message: String(localized: "Item Not Found"))
        }
        AppleServices.shared.navigation.open(.item(current.id), activateWindow: true)
        return .result()
    }
}

struct AddPaperIntent: AppIntent {
    static let title: LocalizedStringResource = "Add Paper"
    static let supportedModes: IntentModes = .background

    @Parameter(title: "DOI, arXiv ID, PMID or ISBN") var identifier: String

    static var parameterSummary: some ParameterSummary {
        Summary { \.$identifier }
    }

    @MainActor
    func perform() async throws -> some IntentResult & ReturnsValue<PaperEntity> & ProvidesDialog {
        do {
            let item = try await AppleServices.shared.library.lookUpAndAdd(identifier, collectionID: nil)
            await AppleServices.shared.finishSpotlightForIntent()
            return .result(value: PaperEntity(item), dialog: "Added “\(item.title)”.")
        } catch {
            throw PaperIntentError(message: CoreKit.message(for: error))
        }
    }
}

struct SearchLibraryIntent: AppIntent {
    static let title: LocalizedStringResource = "Search Library"
    static let supportedModes: IntentModes = .background

    @Parameter(title: "Search") var text: String

    static var parameterSummary: some ParameterSummary {
        Summary { \.$text }
    }

    func perform() async throws -> some IntentResult & ReturnsValue<[PaperEntity]> {
        .result(value: try await PaperQuery().entities(matching: text))
    }
}

struct PaperloomShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: OpenPaperIntent(),
            phrases: ["Open a paper in \(.applicationName)", "Open \(\.$paper) in \(.applicationName)"],
            shortTitle: "Open Paper", systemImageName: "doc.text")
        AppShortcut(
            intent: AddPaperIntent(),
            phrases: ["Add a paper to \(.applicationName)"],
            shortTitle: "Add Paper", systemImageName: "plus")
        AppShortcut(
            intent: SearchLibraryIntent(),
            phrases: ["Search my library in \(.applicationName)"],
            shortTitle: "Search Library", systemImageName: "magnifyingglass")
    }
}

/// Keeps the same localized core error in Siri and the Add Item sheet.
private struct PaperIntentError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}
