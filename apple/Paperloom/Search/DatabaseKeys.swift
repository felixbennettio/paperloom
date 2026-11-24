import CoreKit
import Foundation
import Observation
import SwiftUI

/// The person's keys for the databases that need one, each kept in the
/// keychain on this device.
@MainActor
@Observable
final class DatabaseKeys {
    /// The databases that need a key, in the order to offer them.
    let sources = searchSources().filter(\.needsKey)
    /// The databases a key is kept for.
    private(set) var known: Set<String>

    /// Where to ask each database for a key.
    static let keyPages: [String: URL] = [
        "wos": URL(string: "https://developer.clarivate.com/apis/wos-starter")!,
        "scopus": URL(string: "https://dev.elsevier.com/")!,
        "sciencedirect": URL(string: "https://dev.elsevier.com/")!,
        "ieee": URL(string: "https://developer.ieee.org/")!,
        "springer": URL(string: "https://dev.springernature.com/")!,
    ]

    init() {
        known = Set(sources.map(\.id).filter { Self.key(for: $0) != nil })
    }

    nonisolated static func key(for sourceID: String) -> String? {
        SecretKeychain.load(service(sourceID))
    }

    /// Keeps a key; an empty one is forgotten.
    func keep(_ key: String, for sourceID: String) {
        let key = key.trimmingCharacters(in: .whitespacesAndNewlines)
        SecretKeychain.save(key, Self.service(sourceID))
        if key.isEmpty {
            known.remove(sourceID)
        } else {
            known.insert(sourceID)
        }
    }

    private nonisolated static func service(_ sourceID: String) -> String {
        "io.felixbennett.paperloom.database.\(sourceID)"
    }
}

/// One database's key: typed in and kept, or forgotten.
struct DatabaseKeyField: View {
    let source: SearchSource
    @Environment(DatabaseKeys.self) private var keys
    @Environment(\.openURL) private var openURL
    @State private var draft = ""

    var body: some View {
        let kept = keys.known.contains(source.id)
        LabeledContent {
            HStack {
                SecureField(source.name, text: $draft, prompt: kept ? Text(verbatim: "••••••••") : Text("API key"))
                    .labelsHidden()
                    .onSubmit(keep)
                if !draft.isEmpty {
                    Button("Save", action: keep)
                } else if kept {
                    Button("Forget", role: .destructive) { keys.keep("", for: source.id) }
                } else if let page = DatabaseKeys.keyPages[source.id] {
                    Button("Get a Key") { openURL(page) }
                }
            }
        } label: {
            Text(verbatim: source.name)
        }
    }

    private func keep() {
        guard !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        keys.keep(draft, for: source.id)
        draft = ""
    }
}
