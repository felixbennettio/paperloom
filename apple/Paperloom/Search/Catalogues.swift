import CoreKit
import Foundation
import Observation
import SwiftUI

/// The library catalogues the person added (any that answers SRU), kept on
/// this device and offered in Online Search after the databases.
@MainActor
@Observable
final class Catalogues {
    private(set) var all: [SruCatalogue]
    /// National libraries' catalogues to start from.
    let presets = cataloguePresets()

    private static let defaultsKey = "catalogues"

    init() {
        let saved = UserDefaults.standard.array(forKey: Self.defaultsKey) as? [[String: String]] ?? []
        all = saved.compactMap { entry in
            guard let id = entry["id"], let name = entry["name"], let url = entry["url"],
                !id.isEmpty, !name.isEmpty, Self.isCatalogueAddress(url)
            else { return nil }
            return SruCatalogue(id: id, name: name, url: url, index: entry["index"] ?? "", schema: entry["schema"] ?? "")
        }
    }

    /// As databases to pick in Online Search.
    var sources: [SearchSource] {
        all.map { SearchSource(id: $0.id, name: $0.name, homepage: $0.url, needsKey: false) }
    }

    func catalogue(id: String) -> SruCatalogue? {
        all.first { $0.id == id }
    }

    func add(_ catalogue: SruCatalogue) {
        all.append(catalogue)
        save()
    }

    func remove(id: String) {
        all.removeAll { $0.id == id }
        save()
    }

    /// A catalogue to fill in: a preset's details, or none, with an ID of
    /// its own.
    static func fresh(from preset: SruCatalogue?) -> SruCatalogue {
        SruCatalogue(
            id: "sru-" + UUID().uuidString.lowercased(),
            name: preset?.name ?? "",
            url: preset?.url ?? "",
            index: preset?.index ?? "",
            schema: preset?.schema ?? "")
    }

    /// An SRU (web) or Z39.50 (`z3950://host:port/database`) address.
    static func isCatalogueAddress(_ text: String) -> Bool {
        guard let url = URL(string: text.trimmingCharacters(in: .whitespaces)),
            let scheme = url.scheme?.lowercased()
        else { return false }
        return ["http", "https", "z3950", "z39.50", "tcp"].contains(scheme) && url.host() != nil
    }

    private func save() {
        let entries = all.map {
            ["id": $0.id, "name": $0.name, "url": $0.url, "index": $0.index, "schema": $0.schema]
        }
        UserDefaults.standard.set(entries, forKey: Self.defaultsKey)
    }
}

/// Library catalogues searched from Online Search: any that answers SRU,
/// from a national library's preset or typed in.
struct CataloguesSection: View {
    @Environment(Catalogues.self) private var catalogues
    @State private var draft: SruCatalogue?

    var body: some View {
        Section("Library Catalogues") {
            ForEach(catalogues.all) { catalogue in
                LabeledContent {
                    Button("Remove", role: .destructive) { catalogues.remove(id: catalogue.id) }
                } label: {
                    Text(verbatim: catalogue.name)
                    Text(verbatim: catalogue.url)
                }
            }
            Menu("Add a Catalogue…") {
                ForEach(catalogues.presets) { preset in
                    Button {
                        draft = Catalogues.fresh(from: preset)
                    } label: {
                        Text(verbatim: preset.name)
                    }
                }
                Divider()
                Button("Another Catalogue…") { draft = Catalogues.fresh(from: nil) }
            }
            Text("Any library catalogue that answers SRU or Z39.50 can be searched from Online Search. A Z39.50 address is z3950://host:port/database. Records come as MARC 21, UNIMARC (CNMARC) or Dublin Core (dc).")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .sheet(item: $draft) { catalogue in
            CatalogueForm(catalogue: catalogue) { catalogues.add($0) }
        }
    }
}

/// A catalogue's name, SRU address, search index and record format, before
/// it is added.
struct CatalogueForm: View {
    @State var catalogue: SruCatalogue
    let add: (SruCatalogue) -> Void
    @Environment(\.dismiss) private var dismiss

    private var ready: Bool {
        !catalogue.name.trimmingCharacters(in: .whitespaces).isEmpty && Catalogues.isCatalogueAddress(catalogue.url)
    }

    var body: some View {
        NavigationStack {
            Form {
                TextField("Name", text: $catalogue.name)
                TextField("Address (SRU or Z39.50)", text: $catalogue.url, prompt: Text(verbatim: "https://… / z3950://…"))
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .textInputAutocapitalization(.never)
                    .keyboardType(.URL)
                    #endif
                TextField("Search Index", text: $catalogue.index, prompt: Text("The server's choice"))
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .textInputAutocapitalization(.never)
                    #endif
                TextField("Record Format", text: $catalogue.schema, prompt: Text(verbatim: "marcxml"))
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .textInputAutocapitalization(.never)
                    #endif
            }
            .formStyle(.grouped)
            .navigationTitle("Library Catalogues")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") {
                        let trim = { (text: String) in text.trimmingCharacters(in: .whitespacesAndNewlines) }
                        add(
                            SruCatalogue(
                                id: catalogue.id,
                                name: trim(catalogue.name),
                                url: trim(catalogue.url),
                                index: trim(catalogue.index),
                                schema: trim(catalogue.schema)))
                        dismiss()
                    }
                    .disabled(!ready)
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 440, minHeight: 280)
        #endif
    }
}
