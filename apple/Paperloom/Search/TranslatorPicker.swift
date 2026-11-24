import SwiftUI

/// This sheet shows a translator's checklist of works to save from a results page.
struct TranslatorPicker: View {
    let selection: TranslatorSelection
    let host: TranslatorHost
    @State private var chosen: Set<String>

    init(selection: TranslatorSelection, host: TranslatorHost) {
        self.selection = selection
        self.host = host
        _chosen = State(initialValue: Set(selection.items.map(\.id)))
    }

    var body: some View {
        NavigationStack {
            List(selection.items) { item in
                Button {
                    if !chosen.insert(item.id).inserted { chosen.remove(item.id) }
                } label: {
                    HStack(alignment: .top, spacing: 12) {
                        Image(systemName: chosen.contains(item.id) ? "checkmark.square.fill" : "square")
                            .foregroundStyle(chosen.contains(item.id) ? Color.accentColor : Color.secondary)
                        Text(verbatim: item.title)
                            .foregroundStyle(.primary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(chosen.contains(item.id) ? .isSelected : [])
            }
            .navigationTitle("Choose items to save")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", role: .cancel) { host.choose([], for: selection.id) }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        host.choose(selection.items.map(\.id).filter { chosen.contains($0) }, for: selection.id)
                    }
                    .disabled(chosen.isEmpty)
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 560, minHeight: 420)
        #endif
    }
}
