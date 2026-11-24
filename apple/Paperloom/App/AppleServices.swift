import CoreSpotlight
import Foundation
import Observation
import SwiftUI

/// Intents can arrive before a scene exists; they share the scene's models.
@MainActor
final class AppleServices {
    static let shared = AppleServices()

    let library = LibraryModel.openDefault()
    let vault = VaultModel()
    let settings = WorkbenchSettings()
    let navigation = AppleNavigation()
    private let spotlight = SpotlightIndex()

    private init() {
        vault.connect(to: library)
        observeChanges()
        refreshSpotlight()
    }

    private func observeChanges() {
        withObservationTracking {
            _ = library.revision
            _ = vault.changes
            _ = settings.showInSpotlight
        } onChange: { [weak self] in
            Task { @MainActor in
                guard let self else { return }
                self.observeChanges()
                self.refreshSpotlight()
            }
        }
    }

    private func refreshSpotlight() {
        let books = library.core
        let notes = vault.core
        let enabled = settings.showInSpotlight
        Task { await spotlight.update(library: books, vault: notes, enabled: enabled) }
    }

    func finishSpotlightForIntent() async {
        await spotlight.update(library: library.core, vault: vault.core, enabled: settings.showInSpotlight)
        await spotlight.finishUpdates()
    }
}

enum AppleDestination: Hashable, Sendable {
    case item(String)
    case note(String)

    init?(identifier: String) {
        if identifier.hasPrefix("item:"), identifier.count > 5 {
            self = .item(String(identifier.dropFirst(5)))
        } else if identifier.hasPrefix("note:"), identifier.count > 5 {
            self = .note(String(identifier.dropFirst(5)))
        } else {
            return nil
        }
    }
}

/// A cold launch keeps its destination until a window can navigate.
@MainActor
final class AppleNavigation {
    private var handlers: [UUID: (AppleDestination) -> Void] = [:]
    private var focused: UUID?
    private var pending: AppleDestination?
    var openLibraryWindow: (() -> Void)?

    func register(_ id: UUID, handler: @escaping (AppleDestination) -> Void) {
        handlers[id] = handler
    }

    func unregister(_ id: UUID) {
        handlers[id] = nil
        if focused == id { focused = nil }
    }

    func focus(_ id: UUID) {
        focused = id
        deliver()
    }

    func blur(_ id: UUID) {
        if focused == id { focused = nil }
    }

    func open(_ destination: AppleDestination, activateWindow: Bool = false) {
        pending = destination
        if activateWindow, focused == nil { openLibraryWindow?() }
        deliver()
    }

    private func deliver() {
        guard let pending, let focused, let handler = handlers[focused] else { return }
        self.pending = nil
        handler(pending)
    }

    func continueActivity(_ activity: NSUserActivity) {
        if activity.activityType == CSSearchableItemActionType,
           let identifier = activity.userInfo?[CSSearchableItemActivityIdentifier] as? String,
           let destination = AppleDestination(identifier: identifier) {
            open(destination)
        } else if activity.activityType == AppleActivity.item,
                  let id = activity.userInfo?["id"] as? String, !id.isEmpty {
            open(.item(id))
        } else if activity.activityType == AppleActivity.note,
                  let path = activity.userInfo?["path"] as? String, !path.isEmpty {
            open(.note(path))
        }
    }
}

enum AppleActivity {
    static let item = "io.felixbennett.paperloom.item"
    static let note = "io.felixbennett.paperloom.note"

    static func configure(_ activity: NSUserActivity, title: String, key: String, value: String) {
        activity.title = title
        activity.userInfo = [key: value]
        activity.requiredUserInfoKeys = [key]
        activity.isEligibleForHandoff = true
        activity.isEligibleForSearch = false
        activity.isEligibleForPublicIndexing = false
        #if os(iOS)
        activity.isEligibleForPrediction = false
        #endif
    }
}

struct AppleActivityReceiver: ViewModifier {
    #if os(macOS)
    @Environment(\.openWindow) private var openWindow
    #endif

    func body(content: Content) -> some View {
        content
            #if os(macOS)
            .onAppear {
                AppleServices.shared.navigation.openLibraryWindow = { openWindow(id: "library") }
            }
            #endif
            .onContinueUserActivity(CSSearchableItemActionType, perform: receive)
            .onContinueUserActivity(AppleActivity.item, perform: receive)
            .onContinueUserActivity(AppleActivity.note, perform: receive)
    }

    private func receive(_ activity: NSUserActivity) {
        AppleServices.shared.navigation.continueActivity(activity)
    }
}
