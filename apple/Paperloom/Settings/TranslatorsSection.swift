import CoreKit
import SwiftUI

/// The optional Zotero translators are kept beside the library.
struct TranslatorsSection: View {
    @Environment(LibraryModel.self) private var library
    @State private var status: TranslatorStatus?
    @State private var working = false
    @State private var message: String?

    var body: some View {
        Section("Zotero Translators") {
            if let status {
                if status.count > 0 {
                    Text("Translators installed: \(String(status.count))")
                } else {
                    Text("No translators installed")
                }
                if let date = status.downloaded {
                    Text("Downloaded \(date)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            HStack {
                Button {
                    Task { await change(remove: false) }
                } label: {
                    if working {
                        Text("Working…")
                    } else if (status?.count ?? 0) > 0 {
                        Text("Update")
                    } else {
                        Text("Download")
                    }
                }
                .disabled(working || library.core == nil || status == nil)
                Button("Remove", role: .destructive) {
                    Task { await change(remove: true) }
                }
                .disabled(working || (status?.count ?? 0) == 0)
                if working { ProgressView().controlSize(.small) }
            }
            Text("These are Zotero's translators, downloaded from GitHub on request and kept apart from the app.")
                .font(.caption)
                .foregroundStyle(.secondary)
            if let message {
                Text(verbatim: message).font(.caption).foregroundStyle(.red)
            }
        }
        .task { await refresh() }
    }

    private func refresh() async {
        guard let core = library.core else { return }
        do {
            status = try await Task.detached { try core.translatorsStatus() }.value
            message = nil
        } catch {
            message = CoreKit.message(for: error)
        }
    }

    private func change(remove: Bool) async {
        guard !working, let core = library.core else { return }
        working = true
        message = nil
        defer { working = false }
        do {
            status = try await Task.detached {
                if remove {
                    try core.removeTranslators()
                } else {
                    _ = try core.downloadTranslators()
                }
                return try core.translatorsStatus()
            }.value
            NotificationCenter.default.post(name: TranslatorHost.changed, object: nil)
        } catch {
            message = CoreKit.message(for: error)
        }
    }
}
