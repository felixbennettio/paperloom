import CoreKit
import SwiftUI
import UniformTypeIdentifiers

/// The reference file types Paperloom reads and writes.
enum ReferenceFileTypes {
    // Keep these identifiers in step with Support/Info.plist.
    static let ris = UTType(importedAs: "org.refman.ris", conformingTo: .plainText)
    static let endNote = UTType(importedAs: "com.endnote.enw", conformingTo: .plainText)
    static let bibTeX = UTType(importedAs: "org.tug.tex.bibtex", conformingTo: .plainText)
    static let pubMed = UTType(importedAs: "gov.nih.nlm.nbib", conformingTo: .plainText)

    static let importable: [UTType] =
        [ris, endNote, bibTeX, pubMed]
        + [.json, .plainText, .commaSeparatedText, .xml]

    /// One type per export format, so the exporter accepts each of them.
    static let exportable: [UTType] = referenceFormats().map { type(forExtension: $0.fileExtension) } + [.plainText]

    static func type(forExtension ext: String) -> UTType {
        switch ext.lowercased() {
        case "ris": ris
        case "enw": endNote
        case "bib": bibTeX
        case "nbib": pubMed
        default: UTType(filenameExtension: ext) ?? .plainText
        }
    }
}

/// Text read from a picked file: UTF-8, or GB18030 (older CNKI exports).
enum ReferenceFileReader {
    static func read(_ url: URL) throws -> String {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let data = try Data(contentsOf: url)
        if let text = String(data: data, encoding: .utf8) {
            return text
        }
        let gb18030 = String.Encoding(
            rawValue: CFStringConvertEncodingToNSStringEncoding(CFStringEncoding(CFStringEncodings.GB_18030_2000.rawValue)))
        if let text = String(data: data, encoding: gb18030) {
            return text
        }
        return String(decoding: data, as: UTF8.self)
    }
}

/// A plain-text document for the file exporter.
struct ReferenceDocument: FileDocument {
    static var readableContentTypes: [UTType] { [.plainText] }
    static var writableContentTypes: [UTType] { ReferenceFileTypes.exportable }
    var text: String

    init(text: String) {
        self.text = text
    }

    init(configuration: ReadConfiguration) throws {
        text = String(decoding: configuration.file.regularFileContents ?? Data(), as: UTF8.self)
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        FileWrapper(regularFileWithContents: Data(text.utf8))
    }
}

/// Import and export buttons for a list of items, with their pickers and
/// the import report.
struct ImportExportMenu: View {
    let itemIDs: [String]
    @Environment(LibraryModel.self) private var library
    @Environment(VaultModel.self) private var vault
    @State private var importing = false
    @State private var choosingZotero = false
    @State private var choosingEndNote = false
    /// The EndNote export picked; its `.Data` folder is asked for next.
    @State private var endNoteExport: URL?
    @State private var choosingEndNoteData = false
    @State private var exportDocument: ReferenceDocument?
    @State private var exportType: UTType = .plainText
    @State private var report: String?
    @State private var citingWord = false
    private let formats = referenceFormats()

    var body: some View {
        Menu {
            Button("Import…", systemImage: "square.and.arrow.down") { importing = true }
            #if os(macOS)
            Button("From Zotero…", systemImage: "books.vertical") { choosingZotero = true }
            Button("From EndNote…", systemImage: "books.vertical.circle") { choosingEndNote = true }
            #endif
            Menu {
                ForEach(formats, id: \.id) { format in
                    Button(format.name) { export(format) }
                }
            } label: {
                Label("Export…", systemImage: "square.and.arrow.up")
            }
            .disabled(itemIDs.isEmpty)
            Button("Cite in Word Document…", systemImage: "doc.text") { citingWord = true }
        } label: {
            Label("Import and Export", systemImage: "arrow.up.arrow.down.square")
        }
        .fileImporter(isPresented: $importing, allowedContentTypes: ReferenceFileTypes.importable, allowsMultipleSelection: true) { result in
            guard case .success(let urls) = result else { return }
            report = library.importFiles(urls)
        }
        // A second picker needs a view of its own.
        .background {
            Color.clear.fileImporter(isPresented: $choosingZotero, allowedContentTypes: [.folder]) { result in
                guard case .success(let folder) = result else { return }
                Task {
                    report = await library.importZotero(from: folder, vault: vault.core)
                    vault.reloadAfterSync()
                }
            }
        }
        .background {
            Color.clear.fileImporter(isPresented: $choosingEndNote, allowedContentTypes: [.xml]) { result in
                guard case .success(let file) = result else { return }
                endNoteExport = file
                // The second picker opens once the first has gone.
                Task {
                    try? await Task.sleep(for: .milliseconds(400))
                    choosingEndNoteData = true
                }
            }
        }
        // Then the library's .Data folder, for its PDFs (cancel to bring in
        // the items alone).
        .background {
            Color.clear.fileImporter(isPresented: $choosingEndNoteData, allowedContentTypes: [.folder]) { result in
                guard let export = endNoteExport else { return }
                let data = try? result.get()
                endNoteExport = nil
                Task {
                    report = await library.importEndNote(from: export, data: data, vault: vault.core)
                    vault.reloadAfterSync()
                }
            }
        }
        .fileExporter(
            isPresented: Binding(get: { exportDocument != nil }, set: { if !$0 { exportDocument = nil } }),
            document: exportDocument,
            contentType: exportType,
            defaultFilename: "Paperloom"
        ) { _ in
            exportDocument = nil
        }
        .sheet(isPresented: $citingWord) { WordCiteSheet() }
        .alert("Import", isPresented: Binding(get: { report != nil }, set: { if !$0 { report = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(report ?? "")
        }
    }

    private func export(_ format: ReferenceFormat) {
        guard let text = library.exportReferences(ids: itemIDs, format: format.id) else { return }
        exportType = ReferenceFileTypes.type(forExtension: format.fileExtension)
        exportDocument = ReferenceDocument(text: text)
    }
}
