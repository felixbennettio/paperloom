import CoreKit
import SwiftUI

/// Formats the `[@citations]` in a Word, WPS or LibreOffice document: a
/// style, then the document, then where its copy goes. The document itself
/// is left as it is, so it can be cited again after editing.
struct WordCiteSheet: View {
    @Environment(LibraryModel.self) private var library
    @Environment(\.dismiss) private var dismiss
    /// Shared with the Cite box.
    @AppStorage("citation.style") private var styleID = "gb-7714-2015-numeric"
    @State private var choosing = false
    @State private var working = false
    /// The copy written, waiting to be saved where the person says.
    @State private var copy: ExportFile?
    @State private var copyName = ""
    @State private var result: WordCitations?
    @State private var saved = false
    @State private var problem: String?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    hint
                    Picker("Style", selection: $styleID) {
                        ForEach(library.citationStyles, id: \.id) { style in
                            Text(style.name).tag(style.id)
                        }
                    }
                }
                if saved, let result {
                    Section {
                        Text(
                            "Citations formatted: \(String(result.citations)); references: \(String(result.references)). Saved as \(copyName)."
                        )
                        if !result.missing.isEmpty {
                            Text("Not in the library: \(result.missing.joined(separator: ", "))")
                                .foregroundStyle(.red)
                        }
                    }
                }
                if let problem {
                    Section {
                        Text(problem).foregroundStyle(.red)
                    }
                }
            }
            .formStyle(.grouped)
            .navigationTitle("Cite in Word Document")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(saved ? LocalizedStringKey("Done") : LocalizedStringKey("Cancel")) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(working ? LocalizedStringKey("Working…") : LocalizedStringKey("Choose Document…")) {
                        choosing = true
                    }
                    .disabled(working)
                }
            }
        }
        .fileImporter(isPresented: $choosing, allowedContentTypes: [ExportFile.word, ExportFile.openDocument]) { picked in
            guard case .success(let url) = picked else { return }
            Task { await cite(url) }
        }
        // A second picker needs a view of its own.
        .background {
            Color.clear.fileExporter(
                isPresented: Binding(get: { copy != nil }, set: { if !$0 { copy = nil } }),
                document: copy,
                contentType: copy?.contentType ?? ExportFile.word,
                defaultFilename: copyName
            ) { outcome in
                if case .success = outcome { saved = true }
                copy = nil
            }
        }
        #if os(macOS)
        .frame(minWidth: 460, minHeight: 340)
        #endif
    }

    private var hint: some View {
        // Written as is, not filled in: the marker for the reference list.
        let marker = "{bibliography}"
        return Text(
            "Write citations in Word, WPS or LibreOffice as in notes: [@citation key], [@DOI, p. 3] or [@key1; @key2]. Put \(marker) where the reference list goes, or it goes at the end. Paperloom saves a copy with the citations formatted; your document stays as it is, so run this again after editing."
        )
        .font(.callout)
        .foregroundStyle(.secondary)
    }

    private func cite(_ source: URL) async {
        working = true
        problem = nil
        saved = false
        switch await library.citeWordDocument(source, style: styleID) {
        case .success(let (file, report)):
            result = report
            copyName = file.deletingPathExtension().lastPathComponent
            // The importer has to be gone before the exporter opens.
            try? await Task.sleep(for: .milliseconds(400))
            copy = ExportFile(url: file)
        case .failure(let error):
            problem = CoreKit.message(for: error)
        }
        working = false
    }
}
