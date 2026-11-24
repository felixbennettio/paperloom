import CoreKit
import SwiftUI
import UniformTypeIdentifiers

/// A backup file made in the temporary folder, for the file exporter.
struct BackupFile: FileDocument {
    static var readableContentTypes: [UTType] { [.zip] }
    let url: URL

    init(url: URL) {
        self.url = url
    }

    init(configuration: ReadConfiguration) throws {
        throw CocoaError(.featureUnsupported)
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        try FileWrapper(url: url)
    }
}

/// How often the library backs itself up.
enum AutomaticBackup: String, CaseIterable, Identifiable {
    case off, daily, weekly

    static let key = "backup.automatic"

    var id: String { rawValue }

    var hours: UInt32? {
        switch self {
        case .off: nil
        case .daily: 24
        case .weekly: 24 * 7
        }
    }

    var title: LocalizedStringKey {
        switch self {
        case .off: "Off"
        case .daily: "Every day"
        case .weekly: "Every week"
        }
    }
}

/// Backing the library up to one `.zip`, and putting a backup back.
struct BackupSection: View {
    @Environment(LibraryModel.self) private var library
    @Environment(VaultModel.self) private var vault
    @AppStorage(AutomaticBackup.key) private var automatic = AutomaticBackup.off
    @State private var working = false
    @State private var made: BackupFile?
    @State private var choosing = false
    @State private var message: String?

    var body: some View {
        Section("Backup") {
            LabeledContent("The library, its files and the vault, in one .zip.") {
                Button {
                    Task { await backUp() }
                } label: {
                    if working {
                        ProgressView().controlSize(.small)
                    } else {
                        Text("Back Up Now…")
                    }
                }
                .disabled(working)
            }
            LabeledContent("Put a backup back") {
                Button("Restore from Backup…") { choosing = true }
                    .disabled(working)
            }
            Picker("Back up automatically", selection: $automatic) {
                ForEach(AutomaticBackup.allCases) { choice in
                    Text(choice.title).tag(choice)
                }
            }
            if automatic != .off {
                LabeledContent("Backup folder") {
                    Text("Beside the library")
                }
                Text("The newest 10 automatic backups are kept.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            if let message {
                Text(message).font(.caption).foregroundStyle(.secondary)
            }
        }
        .fileExporter(
            isPresented: Binding(get: { made != nil }, set: { if !$0 { made = nil } }),
            document: made,
            contentType: .zip,
            defaultFilename: made?.url.lastPathComponent
        ) { _ in
            if let url = made?.url { try? FileManager.default.removeItem(at: url) }
            made = nil
        }
        .background {
            Color.clear.fileImporter(isPresented: $choosing, allowedContentTypes: [.zip]) { result in
                guard case .success(let url) = result else { return }
                Task { await restore(url) }
            }
        }
    }

    private func backUp() async {
        working = true
        defer { working = false }
        do {
            made = BackupFile(url: try await library.backUp(vault: vault.core))
            message = nil
        } catch {
            message = CoreKit.message(for: error)
        }
    }

    private func restore(_ url: URL) async {
        working = true
        defer { working = false }
        do {
            let report = try await library.restore(from: url, vault: vault.core)
            vault.reloadAfterSync()
            let (notes, files) = (String(report.notes), String(report.attachments))
            message = String(
                localized: "Notes put back: \(notes). Restart Paperloom to bring back the library and its files (\(files)).")
        } catch {
            message = CoreKit.message(for: error)
        }
    }
}
