import CoreKit
import SwiftUI
import UniformTypeIdentifiers

/// Styles beyond the built-in ones: from the CSL style repository or a
/// `.csl` file.
struct CitationStylesSection: View {
    @Environment(LibraryModel.self) private var library
    @State private var query = ""
    @State private var found: [StyleListing]?
    @State private var working: String?
    @State private var choosing = false
    @State private var message: String?

    private static let styleFile = UTType(filenameExtension: "csl", conformingTo: .xml) ?? .xml

    var body: some View {
        Section("Citation Styles") {
            ForEach(library.citationStyles.filter(\.added), id: \.id) { style in
                LabeledContent {
                    Button("Remove") { remove(style.id) }
                        .disabled(working != nil)
                } label: {
                    Text(verbatim: style.name)
                }
            }
            HStack {
                TextField("Search the style repository", text: $query)
                    .onSubmit { Task { await search() } }
                Button {
                    Task { await search() }
                } label: {
                    if working == "search" {
                        ProgressView().controlSize(.small)
                    } else {
                        Text("Search")
                    }
                }
                .disabled(working != nil || query.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            if let found {
                if found.isEmpty {
                    Text("No style matches.").foregroundStyle(.secondary)
                }
                ForEach(found, id: \.id) { style in
                    LabeledContent {
                        Button(style.available ? LocalizedStringKey("Added") : LocalizedStringKey("Add")) {
                            Task { await download(style.id) }
                        }
                        .disabled(style.available || working != nil)
                    } label: {
                        Text(verbatim: style.name)
                        formatLabel(style.format)
                    }
                }
            }
            Button("Add from File…") { choosing = true }
                .disabled(working != nil)
            if let message {
                Text(verbatim: message).font(.caption).foregroundStyle(.red)
            }
            Text("Styles come from the CSL style repository, the one Zotero uses, and are kept beside the library on this device.")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .autocorrectionDisabled()
        .background {
            Color.clear.fileImporter(isPresented: $choosing, allowedContentTypes: [Self.styleFile]) { result in
                guard case .success(let url) = result else { return }
                Task { await add(url) }
            }
        }
    }

    private func formatLabel(_ format: String) -> Text {
        switch format {
        case "numeric": Text("Numeric")
        case "author-date": Text("Author-date")
        case "note": Text("Footnotes")
        default: Text(verbatim: format)
        }
    }

    private func search() async {
        let text = query.trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty, working == nil else { return }
        working = "search"
        defer { working = nil }
        do {
            found = try await library.searchCitationStyles(text)
            message = nil
        } catch {
            message = CoreKit.message(for: error)
        }
    }

    private func download(_ id: String) async {
        working = id
        defer { working = nil }
        do {
            try await library.downloadCitationStyle(id: id)
            found = found?.map { style in
                var style = style
                if style.id == id { style.available = true }
                return style
            }
            message = nil
        } catch {
            message = CoreKit.message(for: error)
        }
    }

    private func add(_ url: URL) async {
        working = "file"
        defer { working = nil }
        do {
            try await library.addCitationStyle(file: url)
            message = nil
        } catch {
            message = CoreKit.message(for: error)
        }
    }

    private func remove(_ id: String) {
        do {
            try library.removeCitationStyle(id: id)
            found = found?.map { style in
                var style = style
                if style.id == id { style.available = false }
                return style
            }
            message = nil
        } catch {
            message = CoreKit.message(for: error)
        }
    }
}
