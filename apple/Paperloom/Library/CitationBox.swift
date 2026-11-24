import CoreKit
import SwiftUI

#if os(iOS)
import UIKit
#else
import AppKit
#endif

/// The formatted reference and in-text citation for some items, in the
/// style last chosen, with buttons to copy them.
struct CitationBox: View {
    let itemIDs: [String]
    @Environment(LibraryModel.self) private var library
    @AppStorage("citation.style") private var styleID = "gb-7714-2015-numeric"
    @State private var copied: String?

    var body: some View {
        let output = library.formatCitations(ids: itemIDs, style: styleID)
        VStack(alignment: .leading, spacing: 12) {
            // On a phone the style's long name goes under the heading.
            ViewThatFits(in: .horizontal) {
                HStack {
                    heading
                    Spacer()
                    stylePicker
                }
                VStack(alignment: .leading, spacing: 8) {
                    heading
                    stylePicker
                }
            }
            if let output, !output.entries.isEmpty {
                ForEach(Array(output.entries.enumerated()), id: \.offset) { _, entry in
                    Text(entry)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                if !output.citation.isEmpty {
                    Text(output.citation)
                        .font(.callout.monospaced())
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
                GlassEffectContainer(spacing: 8) {
                    // Side by side when both fit on one line each.
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 8) { copyButtons(output) }
                        VStack(alignment: .leading, spacing: 8) { copyButtons(output) }
                    }
                    .buttonStyle(.glass)
                }
            }
        }
        .padding(16)
        .glassEffect(.regular, in: .rect(cornerRadius: 18))
    }

    private var heading: some View {
        Label("Cite", systemImage: "quote.bubble")
            .font(.headline)
    }

    private var stylePicker: some View {
        Picker("Style", selection: $styleID) {
            ForEach(library.citationStyles, id: \.id) { style in
                Text(style.name).tag(style.id)
            }
        }
        .labelsHidden()
        .fixedSize()
    }

    @ViewBuilder private func copyButtons(_ output: CitationOutput) -> some View {
        Button {
            copy(output.entries.joined(separator: "\n"), label: "reference")
        } label: {
            Label(copied == "reference" ? LocalizedStringKey("Copied") : LocalizedStringKey("Copy Reference"), systemImage: "doc.on.doc")
        }
        Button {
            copy(output.citation, label: "citation")
        } label: {
            Label(
                copied == "citation" ? LocalizedStringKey("Copied") : LocalizedStringKey("Copy In-Text Citation"),
                systemImage: "quote.opening")
        }
        .disabled(output.citation.isEmpty)
        // `[@key]`, for notes and for Word documents cited later.
        Button {
            if let marker = library.citationMarker(ids: itemIDs) { copy(marker, label: "key") }
        } label: {
            Label(copied == "key" ? LocalizedStringKey("Copied") : LocalizedStringKey("Copy Citation Key"), systemImage: "at")
        }
    }

    private func copy(_ text: String, label: String) {
        #if os(iOS)
        UIPasteboard.general.string = text
        #else
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        #endif
        copied = label
        Task {
            try? await Task.sleep(for: .seconds(2))
            if copied == label { copied = nil }
        }
    }
}
