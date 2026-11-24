import Foundation
import Observation
import SwiftUI

/// File-open events arrive one URL at a time. Keep one queue and report
/// across windows, including files delivered while PDFs are being read.
@MainActor
@Observable
final class SharedFileImport {
    private struct File {
        let url: URL
        let scoped: Bool
    }

    private(set) var isReading = false
    private(set) var report = ""
    private(set) var windowID: UUID?
    private var pending: [File] = []
    private var received: Set<URL> = []

    func receive(_ url: URL, in window: UUID, library: LibraryModel) {
        guard url.isFileURL else { return }
        if windowID == nil {
            report = ""
            received = []
        }
        guard received.insert(url).inserted else { return }
        windowID = window
        // Retain access while this file waits in the queue. The readers
        // also scope their own reads, as they do for the in-app pickers.
        pending.append(File(url: url, scoped: url.startAccessingSecurityScopedResource()))
        guard !isReading else { return }
        isReading = true
        Task { await readPending(library: library) }
    }

    func done() {
        guard !isReading else { return }
        windowID = nil
        received = []
    }

    private func readPending(library: LibraryModel) async {
        // Let an in-app PDF import finish before starting another one.
        while library.isImporting {
            try? await Task.sleep(for: .milliseconds(100))
        }
        library.isImportingSharedFiles = true
        defer {
            library.isImportingSharedFiles = false
            isReading = false
        }
        while !pending.isEmpty {
            // Coalesce the individual callbacks from a multi-file open,
            // and give the progress sheet a chance to appear.
            try? await Task.sleep(for: .milliseconds(200))
            let files = pending
            pending = []
            await read(files, library: library)
        }
    }

    private func read(_ files: [File], library: LibraryModel) async {
        defer {
            for file in files {
                if file.scoped { file.url.stopAccessingSecurityScopedResource() }
                removeInboxCopy(file.url)
                received.remove(file.url)
            }
        }
        var pdfs: [URL] = []
        var references: [URL] = []
        var lines: [String] = []
        for file in files {
            switch file.url.pathExtension.lowercased() {
            case "pdf": pdfs.append(file.url)
            case "ris", "enw", "bib", "nbib", "xml", "json", "txt", "csv":
                references.append(file.url)
            default:
                let name = file.url.lastPathComponent
                lines.append(String(localized: "\(name) cannot be imported."))
            }
        }
        if !references.isEmpty { lines.append(library.importFiles(references)) }
        if !pdfs.isEmpty { lines.append(await library.importPDFs(pdfs, showNotice: false)) }
        let text = lines.joined(separator: "\n\n")
        report = report.isEmpty ? text : report + "\n\n" + text
    }

    private func removeInboxCopy(_ url: URL) {
        #if os(iOS)
        guard let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first else { return }
        let inbox = documents.appendingPathComponent("Inbox", isDirectory: true).resolvingSymlinksInPath()
        let file = url.resolvingSymlinksInPath()
        // Only remove copies in this app's Inbox, never a provider's
        // original or another file elsewhere in the sandbox.
        guard file.deletingLastPathComponent() == inbox else { return }
        try? FileManager.default.removeItem(at: file)
        #endif
    }
}

/// Keep file handling off the app's already long root modifier chain.
struct SharedFileReceiver: ViewModifier {
    let imports: SharedFileImport
    @Environment(LibraryModel.self) private var library
    @State private var windowID = UUID()

    func body(content: Content) -> some View {
        content
            .onOpenURL { url in imports.receive(url, in: windowID, library: library) }
            #if os(macOS)
            // Reuse an open library window for Finder and Dock file opens.
            // Explicit New Window actions keep their usual behaviour.
            .handlesExternalEvents(preferring: ["*"], allowing: ["*"])
            #endif
            .sheet(isPresented: Binding(
                get: { imports.windowID == windowID },
                set: { if !$0 && imports.windowID == windowID { imports.done() } })
            ) {
                SharedFileReport(imports: imports)
            }
    }
}

private struct SharedFileReport: View {
    let imports: SharedFileImport

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    if imports.isReading {
                        ProgressView("Reading what was shared…")
                    }
                    if !imports.report.isEmpty {
                        Text(imports.report)
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .padding()
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .navigationTitle("Shared to Paperloom")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { imports.done() }
                        .disabled(imports.isReading)
                }
            }
        }
        .interactiveDismissDisabled(imports.isReading)
        #if os(iOS)
        .presentationDetents([.medium, .large])
        #else
        .frame(minWidth: 420, idealWidth: 480, minHeight: 240, idealHeight: 320)
        #endif
    }
}
