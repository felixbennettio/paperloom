import SwiftUI

/// The open-source software in the app and its licences, written into the
/// app's resources when Xcode Cloud builds it (scripts/licenses.py).
struct NoticesView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var paragraphs: [String]?

    var body: some View {
        NavigationStack {
            Group {
                if let paragraphs, !paragraphs.isEmpty {
                    ScrollView {
                        // About a megabyte of text: drawn as it scrolls into view.
                        LazyVStack(alignment: .leading, spacing: 8) {
                            ForEach(paragraphs.indices, id: \.self) { index in
                                Text(paragraphs[index])
                                    .font(.caption.monospaced())
                                    .textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                        }
                        .padding()
                    }
                } else if paragraphs != nil {
                    ContentUnavailableView("This build has no list of third-party notices.", systemImage: "doc.text")
                } else {
                    ProgressView()
                }
            }
            .navigationTitle("Third-Party Notices")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 560, minHeight: 480)
        #endif
        .task {
            paragraphs = await Self.load()
        }
    }

    private static func load() async -> [String] {
        await Task.detached(priority: .userInitiated) {
            guard let url = Bundle.main.url(forResource: "THIRD_PARTY_NOTICES", withExtension: "txt"),
                  let text = try? String(contentsOf: url, encoding: .utf8)
            else { return [] }
            return text.components(separatedBy: "\n\n")
        }.value
    }
}
