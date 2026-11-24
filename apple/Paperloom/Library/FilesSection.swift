import CoreKit
import QuickLook
import SwiftUI
import UniformTypeIdentifiers

/// An item's attached files: read the PDF, attach more, remove one.
struct FilesSection: View {
    let itemID: String
    let itemTitle: String
    @Environment(LibraryModel.self) private var library
    @Environment(TabsModel.self) private var tabs
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    @State private var attaching = false
    @State private var previewURL: URL?
    @Environment(PluginHost.self) private var plugins
    @State private var finding = false
    /// Why looking for a PDF attached none.
    @State private var findMessage: String?

    var body: some View {
        let _ = library.revision
        let files = library.attachments(itemID: itemID)
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("Files").font(.headline)
                Spacer()
                Button("Attach File…", systemImage: "paperclip") { attaching = true }
                    .buttonStyle(.glass)
            }
            if files.isEmpty {
                Text("Attach the paper's PDF to read and mark it up here.")
                    .foregroundStyle(.secondary)
            }
            if !files.contains(where: \.isPdf) {
                Button(
                    finding ? LocalizedStringKey("Finding a PDF…") : LocalizedStringKey("Find Available PDF"),
                    systemImage: "doc.text.magnifyingglass"
                ) {
                    finding = true
                    findMessage = nil
                    Task {
                        findMessage = await library.findAvailablePDF(itemID: itemID)
                        finding = false
                    }
                }
                .buttonStyle(.glass)
                .disabled(finding)
                if let findMessage {
                    Text(findMessage)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            }
            ForEach(files, id: \.id) { file in
                HStack(spacing: 12) {
                    Image(systemName: file.isPdf ? "doc.richtext" : "doc")
                        .font(.title2)
                        .foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(file.title).lineLimit(1)
                        Text(ByteCountFormatter.string(fromByteCount: Int64(file.size), countStyle: .file))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    openButton(file)
                }
                .padding(12)
                .glassEffect(.regular, in: .rect(cornerRadius: 14))
                .contextMenu {
                    Button("Remove", systemImage: "trash", role: .destructive) {
                        library.removeAttachment(id: file.id)
                    }
                }
            }
        }
        .fileImporter(isPresented: $attaching, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
            guard case .success(let urls) = result else { return }
            library.attachFiles(urls, to: itemID)
        }
        .quickLookPreview($previewURL)
        .onChange(of: itemID) { findMessage = nil }
    }

    @ViewBuilder
    private func openButton(_ file: AttachmentInfo) -> some View {
        if file.isPdf {
            #if os(iOS)
            if sizeClass == .compact {
                NavigationLink {
                    PDFReaderView(url: URL(fileURLWithPath: file.filePath), attachmentID: file.id, title: itemTitle)
                } label: {
                    Label("Read", systemImage: "book")
                }
                .buttonStyle(.glassProminent)
            } else {
                readInTab(file)
            }
            #else
            readInTab(file)
            #endif
        } else {
            Button("Open", systemImage: "eye") {
                previewURL = URL(fileURLWithPath: file.filePath)
                // Quick Look shows files that exist; that counts as opening it, for plugins.
                if FileManager.default.fileExists(atPath: file.filePath) {
                    Task { await plugins.fileOpened(attachmentKey: file.id) }
                }
            }
                .buttonStyle(.glass)
        }
    }

    private func readInTab(_ file: AttachmentInfo) -> some View {
        Button("Read", systemImage: "book") {
            tabs.open(.pdf(attachmentID: file.id, path: file.filePath, title: itemTitle))
        }
        .buttonStyle(.glassProminent)
    }
}

/// Shows `LibraryModel.notice` (what adding PDFs did) in an alert, and a
/// Liquid Glass badge while PDFs are being read.
struct LibraryNotices: ViewModifier {
    @Environment(LibraryModel.self) private var library

    func body(content: Content) -> some View {
        content
            .overlay(alignment: .bottom) {
                if library.isImporting && !library.isImportingSharedFiles {
                    Label("Reading PDFs…", systemImage: "doc.text.magnifyingglass")
                        .padding(.horizontal, 16)
                        .padding(.vertical, 10)
                        .glassEffect(.regular, in: .capsule)
                        .padding(.bottom, 28)
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                }
            }
            .animation(.smooth, value: library.isImporting)
            .alert(
                "Add PDF",
                isPresented: Binding(get: { library.notice != nil }, set: { if !$0 { library.notice = nil } })
            ) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(library.notice ?? "")
            }
    }
}

extension View {
    func libraryNotices() -> some View {
        modifier(LibraryNotices())
    }
}
