import Observation
import SwiftUI

/// Where the document tabs sit.
enum TabStripStyle: String, CaseIterable, Identifiable {
    case horizontal
    case verticalLeading
    case verticalTrailing

    var id: Self { self }

    var title: LocalizedStringKey {
        switch self {
        case .horizontal: "Horizontal tabs"
        case .verticalLeading: "Vertical tabs on the left"
        case .verticalTrailing: "Vertical tabs on the right"
        }
    }

    var systemImage: String {
        switch self {
        case .horizontal: "rectangle.topthird.inset.filled"
        case .verticalLeading: "rectangle.leadingthird.inset.filled"
        case .verticalTrailing: "rectangle.trailingthird.inset.filled"
        }
    }
}

/// Which side a panel docks to.
enum PanelEdge: String, CaseIterable, Identifiable {
    case leading
    case trailing

    var id: Self { self }

    var title: LocalizedStringKey {
        switch self {
        case .leading: "Left"
        case .trailing: "Right"
        }
    }
}

/// Layout preferences of the workbench, remembered across launches.
@Observable
final class WorkbenchSettings {
    private let defaults: UserDefaults

    var tabStyle: TabStripStyle {
        didSet { defaults.set(tabStyle.rawValue, forKey: Keys.tabStyle) }
    }

    var notesEdge: PanelEdge {
        didSet { defaults.set(notesEdge.rawValue, forKey: Keys.notesEdge) }
    }

    var showsNotes: Bool {
        didSet { defaults.set(showsNotes, forKey: Keys.showsNotes) }
    }

    var showInSpotlight: Bool {
        didSet { defaults.set(showInSpotlight, forKey: Keys.showInSpotlight) }
    }

    /// Pane widths the person dragged to.
    var notesWidth: Double {
        didSet { defaults.set(notesWidth, forKey: Keys.notesWidth) }
    }

    var tabStripWidth: Double {
        didSet { defaults.set(tabStripWidth, forKey: Keys.tabStripWidth) }
    }

    var splitWidth: Double {
        didSet { defaults.set(splitWidth, forKey: Keys.splitWidth) }
    }

    private enum Keys {
        static let tabStyle = "workbench.tabStyle"
        static let notesEdge = "workbench.notesEdge"
        static let showsNotes = "workbench.showsNotes"
        static let showInSpotlight = "apple.showInSpotlight"
        static let notesWidth = "workbench.notesWidth"
        static let tabStripWidth = "workbench.tabStripWidth"
        static let splitWidth = "workbench.splitWidth"
    }

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        tabStyle = TabStripStyle(rawValue: defaults.string(forKey: Keys.tabStyle) ?? "") ?? .horizontal
        notesEdge = PanelEdge(rawValue: defaults.string(forKey: Keys.notesEdge) ?? "") ?? .trailing
        showsNotes = defaults.object(forKey: Keys.showsNotes) as? Bool ?? true
        showInSpotlight = defaults.object(forKey: Keys.showInSpotlight) as? Bool ?? true
        notesWidth = defaults.object(forKey: Keys.notesWidth) as? Double ?? 360
        tabStripWidth = defaults.object(forKey: Keys.tabStripWidth) as? Double ?? 210
        splitWidth = defaults.object(forKey: Keys.splitWidth) as? Double ?? 480
    }
}
