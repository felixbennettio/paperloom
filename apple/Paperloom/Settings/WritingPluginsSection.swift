#if os(macOS)
import AppKit
import CoreKit
import SwiftUI

/// Word and WPS task panes (docs/writing-plugins.md): the ones the person
/// let use the library, and the Word add-in's manifest put where Word for
/// Mac loads add-ins from. Only the Mac runs the connector they talk to.
struct WritingPluginsSection: View {
    @Environment(LibraryModel.self) private var library
    @State private var writers: [ConnectedWriter] = []
    @State private var installed = false
    @State private var message: String?

    var body: some View {
        Section("Writing Plugins") {
            if let version = WordAddin.version {
                HStack {
                    Text("Word add-in")
                    Spacer()
                    Button { install() } label: { installed ? Text("Reinstall") : Text("Install") }
                    if installed {
                        Button("Remove", role: .destructive) { remove() }
                    }
                }
                Group {
                    if installed {
                        Text("Version \(version) installed")
                    } else {
                        Text("Not installed")
                    }
                    Text("Restart Word after installing. Paperloom's Cite button is then on Word's Home tab.")
                }
                .font(.caption)
                .foregroundStyle(.secondary)
            }
            ForEach(writers, id: \.self) { writer in
                LabeledContent(writer.client) {
                    let origin = writer.origin.isEmpty ? String(localized: "this computer") : writer.origin
                    let added = Date(timeIntervalSince1970: TimeInterval(writer.added))
                    Text(verbatim: "\(origin) · \(added.formatted(date: .abbreviated, time: .omitted))")
                }
            }
            if writers.isEmpty {
                Text("No writing plugin is connected. The Word and WPS add-ins ask here the first time they connect.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                HStack {
                    Text("Connected writing plugins can search the library and format citations.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button("Disconnect All") { disconnect() }
                }
            }
            if let message {
                Text(verbatim: message).font(.caption).foregroundStyle(.red)
            }
        }
        .task { refresh() }
    }

    private func refresh() {
        installed = WordAddin.installed()
        guard let core = library.core else { return }
        do {
            writers = try core.connectedWriters()
        } catch {
            message = CoreKit.message(for: error)
        }
    }

    private func install() {
        do {
            try WordAddin.install()
            message = nil
        } catch WordAddin.Failure.cancelled {
        } catch {
            message = error.localizedDescription
        }
        installed = WordAddin.installed()
    }

    private func remove() {
        do {
            try WordAddin.remove()
            message = nil
        } catch {
            message = error.localizedDescription
        }
        installed = WordAddin.installed()
    }

    private func disconnect() {
        guard let core = library.core else { return }
        do {
            try core.forgetConnectedWriters()
            writers = []
            message = nil
        } catch {
            message = CoreKit.message(for: error)
        }
    }
}

/// The Word add-in's manifest in Word for Mac's add-in folder: Word loads
/// every manifest in `~/Library/Containers/com.microsoft.Word/Data/Documents/wef`
/// when it starts. The app carries the manifest once the add-in has a
/// hosting address (scripts/word-manifest.py, ci_post_clone.sh). The
/// Developer ID app writes there itself; the App Store app, in its sandbox,
/// asks the person to choose the folder and keeps a bookmark to it.
@MainActor
enum WordAddin {
    enum Failure: Error {
        case cancelled
    }

    private static let fileName = "Paperloom.xml"
    private static let bookmarkKey = "WordAddinFolder"

    private static var manifest: URL? {
        Bundle.main.url(forResource: "WordAddinManifest", withExtension: "xml")
    }

    /// The version of the manifest this app carries, if it carries one.
    static var version: String? {
        guard let manifest, let text = try? String(contentsOf: manifest, encoding: .utf8),
            let start = text.range(of: "<Version>"), let end = text.range(of: "</Version>", range: start.upperBound..<text.endIndex)
        else { return nil }
        return String(text[start.upperBound..<end.lowerBound])
    }

    /// Word's add-in folder, under the person's home (not the sandbox's).
    private static var folder: URL {
        let home = getpwuid(getuid())?.pointee.pw_dir.map { String(cString: $0) } ?? NSHomeDirectory()
        return URL(fileURLWithPath: home, isDirectory: true)
            .appending(path: "Library/Containers/com.microsoft.Word/Data/Documents/wef", directoryHint: .isDirectory)
    }

    /// The folder chosen in the sandbox, from its bookmark.
    private static var chosenFolder: URL? {
        guard let data = UserDefaults.standard.data(forKey: bookmarkKey) else { return nil }
        var stale = false
        return try? URL(resolvingBookmarkData: data, options: .withSecurityScope, bookmarkDataIsStale: &stale)
    }

    static func installed() -> Bool {
        if FileManager.default.fileExists(atPath: folder.appending(path: fileName).path) { return true }
        guard let chosen = chosenFolder else { return false }
        let scoped = chosen.startAccessingSecurityScopedResource()
        defer { if scoped { chosen.stopAccessingSecurityScopedResource() } }
        return FileManager.default.fileExists(atPath: chosen.appending(path: fileName).path)
    }

    static func install() throws {
        guard let manifest else { return }
        let data = try Data(contentsOf: manifest)
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            try data.write(to: folder.appending(path: fileName), options: .atomic)
            return
        } catch {
            // In the sandbox: the person chooses the folder.
        }
        let chosen = try choose()
        let scoped = chosen.startAccessingSecurityScopedResource()
        defer { if scoped { chosen.stopAccessingSecurityScopedResource() } }
        // Word's Documents folder, or the "wef" folder in it.
        let target = chosen.lastPathComponent == "wef" ? chosen : chosen.appending(path: "wef", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: target, withIntermediateDirectories: true)
        try data.write(to: target.appending(path: fileName), options: .atomic)
        // Later checks look in the folder that holds the manifest.
        UserDefaults.standard.set(try target.bookmarkData(options: .withSecurityScope), forKey: bookmarkKey)
    }

    static func remove() throws {
        let direct = folder.appending(path: fileName)
        if FileManager.default.fileExists(atPath: direct.path) {
            try FileManager.default.removeItem(at: direct)
        }
        if let chosen = chosenFolder {
            let scoped = chosen.startAccessingSecurityScopedResource()
            defer { if scoped { chosen.stopAccessingSecurityScopedResource() } }
            let file = chosen.appending(path: fileName)
            if FileManager.default.fileExists(atPath: file.path) {
                try FileManager.default.removeItem(at: file)
            }
            UserDefaults.standard.removeObject(forKey: bookmarkKey)
        }
    }

    private static func choose() throws -> URL {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        let documents = folder.deletingLastPathComponent()
        panel.directoryURL = FileManager.default.fileExists(atPath: folder.path) ? folder : documents
        panel.message = String(
            localized: "Paperloom puts the add-in in the “wef” folder of Word's Documents folder. Choose that folder, or Word's Documents folder, to install it."
        )
        panel.prompt = String(localized: "Install Here")
        guard panel.runModal() == .OK, let url = panel.url else { throw Failure.cancelled }
        return url
    }
}
#endif
