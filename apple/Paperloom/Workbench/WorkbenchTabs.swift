import Observation
import SwiftUI

/// A document open in the workbench.
enum WorkbenchTab: Hashable, Identifiable {
    case library
    case item(id: String, title: String)
    /// A PDF attachment open for reading.
    case pdf(attachmentID: String, path: String, title: String, page: Int? = nil)
    case search
    /// The in-app browser.
    case browser
    /// The whole vault's graph.
    case graph
    /// A canvas in the vault.
    case canvas(path: String)
    /// A drawing (an Excalidraw scene) in the vault.
    case drawing(path: String)
    /// A draw.io diagram in the vault.
    case diagram(path: String)
    case find
    case notes
    case settings

    var id: String {
        switch self {
        case .library: "library"
        case .item(let id, _): "item:\(id)"
        case .pdf(let id, _, _, _): "pdf:\(id)"
        case .search: "search"
        case .browser: "browser"
        case .graph: "graph"
        case .canvas(let path): "canvas:\(path)"
        case .drawing(let path): "drawing:\(path)"
        case .diagram(let path): "diagram:\(path)"
        case .find: "find"
        case .notes: "notes"
        case .settings: "settings"
        }
    }

    var title: String {
        switch self {
        case .library: String(localized: "Library")
        case .item(_, let title): title
        case .pdf(_, _, let title, _): title
        case .search: String(localized: "Online Search")
        case .browser: String(localized: "Browser")
        case .graph: String(localized: "Graph")
        case .canvas(let path), .drawing(let path):
            ((path as NSString).lastPathComponent as NSString).deletingPathExtension
        case .diagram(let path): DrawIO.title(path)
        case .find: String(localized: "Search Everything")
        case .notes: String(localized: "Notes")
        case .settings: String(localized: "Settings")
        }
    }

    var systemImage: String {
        switch self {
        case .library: "books.vertical"
        case .item: "doc.text"
        case .pdf: "doc.richtext"
        case .search: "globe"
        case .browser: "safari"
        case .graph: "point.3.connected.trianglepath.dotted"
        case .canvas: "rectangle.3.group"
        case .drawing: "scribble.variable"
        case .diagram: "flowchart"
        case .find: "magnifyingglass"
        case .notes: "note.text"
        case .settings: "gearshape"
        }
    }

    /// The library tab is always there.
    var isClosable: Bool { self != .library }
}

/// The open tabs and which one is in front.
@Observable
final class TabsModel {
    private(set) var tabs: [WorkbenchTab] = [.library]
    var selectedID: WorkbenchTab.ID = WorkbenchTab.library.id
    /// The tab shown beside the one in front, if the documents are split.
    var sideID: WorkbenchTab.ID?

    var selected: WorkbenchTab {
        tabs.first { $0.id == selectedID } ?? .library
    }

    var side: WorkbenchTab? {
        tabs.first { $0.id == sideID }
    }

    /// Shows a tab beside the one in front.
    func openToTheSide(_ tab: WorkbenchTab) {
        if !tabs.contains(where: { $0.id == tab.id }) { tabs.append(tab) }
        sideID = tab.id
        if selectedID == tab.id, let other = tabs.first(where: { $0.id != tab.id }) {
            selectedID = other.id
        }
    }

    /// Brings a tab to the front, opening it if needed.
    /// Brings a tab to the front, opening it if needed; an open one takes
    /// the new details (such as the page to show).
    func open(_ tab: WorkbenchTab) {
        if let index = tabs.firstIndex(where: { $0.id == tab.id }) {
            tabs[index] = tab
        } else {
            tabs.append(tab)
        }
        selectedID = tab.id
    }

    func close(_ tab: WorkbenchTab) {
        guard tab.isClosable, let index = tabs.firstIndex(where: { $0.id == tab.id }) else { return }
        tabs.remove(at: index)
        if sideID == tab.id { sideID = nil }
        if selectedID == tab.id {
            selectedID = tabs[min(index, tabs.count - 1)].id
        }
    }
}
