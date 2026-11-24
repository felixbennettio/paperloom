import CoreKit
import SwiftUI
import UniformTypeIdentifiers
import WebKit

/// A plugin's web view, for its settings and modals.
#if os(macOS)
struct PluginWebView: NSViewRepresentable {
    let webView: WKWebView

    func makeNSView(context: Context) -> WKWebView { webView }
    func updateNSView(_ view: WKWebView, context: Context) {}
}
#else
struct PluginWebView: UIViewRepresentable {
    let webView: WKWebView

    func makeUIView(context: Context) -> WKWebView { webView }
    func updateUIView(_ view: WKWebView, context: Context) {}
}
#endif

/// The vault's Obsidian plugins: turning them on and off (for Obsidian
/// too), their options, and what of them works here.
struct PluginsSection: View {
    @Environment(VaultModel.self) private var vault
    @Environment(PluginHost.self) private var plugins
    @State private var installed: [ObsidianPlugin] = []
    @State private var problem: String?
    @State private var browsing = false

    var body: some View {
        Section("Obsidian Plugins") {
            if installed.isEmpty {
                Text(
                    "No plugins in this vault yet. Plugins installed in Obsidian (in the vault's .obsidian/plugins folder) show here."
                )
                .foregroundStyle(.secondary)
            }
            ForEach(installed, id: \.id) { plugin in
                row(plugin)
            }
            Button("Browse Community Plugins") { browsing = true }
            Text(
                "Each plugin runs apart from the app and asks before it reaches a website. Turning one on or off here does the same in Obsidian."
            )
            .font(.caption)
            .foregroundStyle(.secondary)
            if let problem {
                Text(verbatim: problem).font(.caption).foregroundStyle(.red)
            }
        }
        .onAppear(perform: load)
        .sheet(isPresented: $browsing, onDismiss: load) {
            CommunityPluginsView(installed: Set(installed.map(\.id)))
        }
    }

    private func row(_ plugin: ObsidianPlugin) -> some View {
        let live = plugins.running.first { $0.id == plugin.id }
        return HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: 2) {
                Text(verbatim: plugin.name) + Text(verbatim: "  \(plugin.version)").foregroundStyle(.secondary)
                if !plugin.description.isEmpty {
                    Text(verbatim: plugin.description).font(.caption).foregroundStyle(.secondary)
                }
                if let error = live?.error {
                    Text(verbatim: error).font(.caption).foregroundStyle(.red)
                }
                if let live, !live.partial.isEmpty {
                    Text("Partly works: \(live.partial.joined(separator: ", ")) are not available here yet.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer()
            if live?.hasSettings == true {
                Button("Options") { plugins.showSettings(plugin.id) }
            }
            Toggle("On", isOn: Binding(get: { plugin.enabled }, set: { turn(plugin, on: $0) }))
                .labelsHidden()
        }
    }

    private func load() {
        installed = (try? vault.core?.obsidianPlugins()) ?? []
    }

    private func turn(_ plugin: ObsidianPlugin, on: Bool) {
        do {
            try vault.core?.setObsidianPluginEnabled(id: plugin.id, enabled: on)
            problem = nil
        } catch {
            problem = CoreKit.message(for: error)
        }
        load()
        plugins.reload()
    }
}

/// Obsidian's community plugins, to find and install from their releases.
struct CommunityPluginsView: View {
    let installed: Set<String>
    @Environment(VaultModel.self) private var vault
    @Environment(\.dismiss) private var dismiss
    @State private var catalog: [CommunityPlugin] = []
    @State private var query = ""
    @State private var installing: String?
    @State private var done: Set<String> = []
    @State private var problem: String?

    private var found: [CommunityPlugin] {
        let words = query.lowercased().split(separator: " ")
        return Array(
            catalog.filter { plugin in
                words.allSatisfy { "\(plugin.name) \(plugin.author) \(plugin.description)".lowercased().contains($0) }
            }
            .prefix(60))
    }

    var body: some View {
        NavigationStack {
            List(found, id: \.id) { plugin in
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(verbatim: plugin.name) + Text(verbatim: "  \(plugin.author)").foregroundStyle(.secondary)
                        Text(verbatim: plugin.description).font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button(
                        installing == plugin.id
                            ? LocalizedStringKey("Installing…")
                            : installed.contains(plugin.id) || done.contains(plugin.id)
                                ? LocalizedStringKey("Update") : LocalizedStringKey("Install")
                    ) { install(plugin) }
                    .disabled(installing != nil)
                }
            }
            .searchable(text: $query, prompt: "Search community plugins")
            .overlay {
                if catalog.isEmpty && problem == nil { ProgressView() }
            }
            .safeAreaInset(edge: .bottom) {
                if let problem {
                    Text(verbatim: problem).font(.caption).foregroundStyle(.red).padding()
                }
            }
            .navigationTitle("Browse Community Plugins")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
        .frame(minWidth: 520, minHeight: 480)
        .task {
            do {
                catalog = try await Task.detached { try communityPlugins() }.value
            } catch {
                problem = CoreKit.message(for: error)
            }
        }
    }

    private func install(_ plugin: CommunityPlugin) {
        guard let core = vault.core else { return }
        installing = plugin.id
        let repo = plugin.repo
        Task {
            do {
                _ = try await Task.detached { try core.installObsidianPlugin(repo: repo) }.value
                done.insert(plugin.id)
                problem = nil
            } catch {
                problem = CoreKit.message(for: error)
            }
            installing = nil
        }
    }
}

/// The plugins' commands, to search and run.
struct PluginCommandPalette: View {
    @Environment(PluginHost.self) private var plugins
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""

    private var found: [PluginCommand] {
        let words = query.lowercased().split(separator: " ")
        return plugins.commands.filter { command in words.allSatisfy { command.name.lowercased().contains($0) } }
    }

    var body: some View {
        NavigationStack {
            List(found) { command in
                Button {
                    dismiss()
                    plugins.run(command)
                } label: {
                    Text(verbatim: command.name)
                }
            }
            .searchable(text: $query, prompt: "Run a command…")
            .overlay {
                if found.isEmpty {
                    ContentUnavailableView(
                        plugins.running.isEmpty
                            ? LocalizedStringKey("Turn on Obsidian plugins in Settings to add commands.")
                            : LocalizedStringKey("No matching commands."),
                        systemImage: "command")
                }
            }
            .navigationTitle("Commands")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
        .frame(minWidth: 420, minHeight: 360)
    }
}

/// What the plugins add around the app: their notices, the command palette
/// (⌘P), a plugin's settings or modal, and the question whether a plugin
/// may reach a site.
struct PluginChrome: ViewModifier {
    @Environment(PluginHost.self) private var plugins
    @State private var palette = false

    func body(content: Content) -> some View {
        content
            .overlay(alignment: .bottomTrailing) {
                VStack(alignment: .trailing, spacing: 8) {
                    ForEach(plugins.notices) { notice in
                        Button {
                            plugins.dismiss(notice)
                        } label: {
                            Text(verbatim: notice.text)
                                .multilineTextAlignment(.leading)
                                .padding(.horizontal, 14)
                                .padding(.vertical, 10)
                                .glassEffect(.regular, in: .rect(cornerRadius: 12))
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(16)
                .frame(maxWidth: 380, alignment: .trailing)
            }
            .background {
                Button("Commands") { palette = true }
                    .keyboardShortcut("p", modifiers: .command)
                    .opacity(0)
                    .accessibilityHidden(true)
            }
            .sheet(isPresented: $palette) { PluginCommandPalette() }
            .sheet(isPresented: Binding(get: { plugins.showing != nil }, set: { if !$0 { plugins.closeShown() } })) {
                NavigationStack {
                    Group {
                        if let id = plugins.showing, let runner = plugins.runner(id) {
                            PluginWebView(webView: runner.webView)
                                .navigationTitle(Text(verbatim: runner.plugin.name))
                        }
                    }
                    .toolbar {
                        ToolbarItem(placement: .confirmationAction) {
                            Button("Close") { plugins.closeShown() }
                        }
                    }
                }
                .frame(minWidth: 560, minHeight: 520)
            }
            .alert(
                "Allow a website?",
                isPresented: Binding(
                    get: { plugins.siteRequest != nil },
                    set: { shown in
                        guard !shown, let request = plugins.siteRequest else { return }
                        plugins.siteRequest = nil
                        request.answer(false)
                    }),
                presenting: plugins.siteRequest
            ) { request in
                Button("Allow") {
                    plugins.siteRequest = nil
                    request.answer(true)
                }
                Button("Don't Allow", role: .cancel) {
                    plugins.siteRequest = nil
                    request.answer(false)
                }
            } message: { request in
                Text("\(request.plugin) wants to reach \(request.site). Allow it?")
            }
    }
}

/// A section a Zotero plugin adds to the item pane, as it drew it for this
/// item (its text).
struct PluginSectionBox: View {
    let section: PluginSection
    let itemID: String
    @Environment(PluginHost.self) private var plugins
    @State private var text: String?

    var body: some View {
        // A stack, not a Group: an empty Group has no view to run the task
        // that fetches the text, so the section never appeared.
        VStack(alignment: .leading, spacing: 8) {
            if let text, !text.isEmpty {
                Text(verbatim: section.label).font(.headline)
                Text(verbatim: text).textSelection(.enabled)
            }
        }
        .task(id: "\(section.id):\(itemID)") {
            let html = await plugins.renderSection(section, item: itemID)
            // Another item came in meanwhile: this answer is not for it.
            guard !Task.isCancelled else { return }
            text = html.flatMap(Self.plainText)
        }
    }

    /// What an HTML fragment says, without its markup.
    static func plainText(_ html: String) -> String? {
        guard let data = html.data(using: .utf8),
              let string = try? NSAttributedString(
                  data: data,
                  options: [
                      .documentType: NSAttributedString.DocumentType.html,
                      .characterEncoding: String.Encoding.utf8.rawValue,
                  ],
                  documentAttributes: nil)
        else { return nil }
        return string.string.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

/// Zotero 7 plugins for the library: installing them from their .xpi
/// files, turning them on and off, their preferences, and what of them
/// works here.
struct ZoteroPluginsSection: View {
    @Environment(LibraryModel.self) private var library
    @Environment(PluginHost.self) private var plugins
    @State private var installed: [ZoteroPlugin] = []
    @State private var choosing = false
    @State private var problem: String?

    var body: some View {
        Section("Zotero Plugins") {
            if installed.isEmpty {
                Text("No Zotero plugins yet. Zotero 7 plugins come as .xpi files.").foregroundStyle(.secondary)
            }
            ForEach(installed, id: \.id) { plugin in
                let live = plugins.running.first { $0.id == "zotero:\(plugin.id)" }
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(verbatim: plugin.name) + Text(verbatim: "  \(plugin.version)").foregroundStyle(.secondary)
                        if !plugin.description.isEmpty {
                            Text(verbatim: plugin.description).font(.caption).foregroundStyle(.secondary)
                        }
                        if let error = live?.error {
                            Text(verbatim: error).font(.caption).foregroundStyle(.red)
                        }
                        if let live, !live.partial.isEmpty {
                            Text("Partly works: \(live.partial.joined(separator: ", ")) are not available here yet.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                    Spacer()
                    if live?.hasSettings == true {
                        Button("Options") { plugins.showSettings("zotero:\(plugin.id)") }
                    }
                    Toggle("On", isOn: Binding(get: { plugin.enabled }, set: { turn(plugin, on: $0) }))
                        .labelsHidden()
                }
            }
            Button("Install from File…") { choosing = true }
            Text("Each plugin runs apart from the app and asks before it reaches a website.")
                .font(.caption)
                .foregroundStyle(.secondary)
            if let problem {
                Text(verbatim: problem).font(.caption).foregroundStyle(.red)
            }
        }
        .onAppear(perform: load)
        .background {
            Color.clear.fileImporter(
                isPresented: $choosing, allowedContentTypes: [UTType(filenameExtension: "xpi") ?? .zip]
            ) { result in
                guard case .success(let url) = result else { return }
                install(url)
            }
        }
    }

    private func load() {
        installed = (try? library.core?.zoteroPlugins()) ?? []
    }

    private func turn(_ plugin: ZoteroPlugin, on: Bool) {
        do {
            try library.core?.setZoteroPluginEnabled(id: plugin.id, enabled: on)
            problem = nil
        } catch {
            problem = CoreKit.message(for: error)
        }
        load()
        plugins.reload()
    }

    private func install(_ url: URL) {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        do {
            _ = try library.core?.installZoteroPlugin(xpi: url.path(percentEncoded: false))
            problem = nil
        } catch {
            problem = CoreKit.message(for: error)
        }
        load()
        plugins.reload()
    }
}

/// A plugin's suggestions for the text at the cursor, shown under the note
/// editor (the editor here does not report where the cursor is drawn).
struct PluginSuggestionList: View {
    let suggestion: PluginSuggestion
    let onChoose: (String) -> Void

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(suggestion.items.enumerated()), id: \.element.id) { index, item in
                        row(item, selected: index == suggestion.selected)
                    }
                }
                .padding(4)
            }
            .scrollBounceBehavior(.basedOnSize)
            .frame(height: min(220, CGFloat(suggestion.items.count) * 34 + 8))
            .onChange(of: suggestion.selected) {
                proxy.scrollTo(suggestion.items[suggestion.selected].id)
            }
        }
        .frame(maxWidth: 360)
        .background(.regularMaterial, in: .rect(cornerRadius: 12))
        .shadow(radius: 6, y: 2)
        .padding(8)
    }

    private func row(_ item: PluginSuggestion.Item, selected: Bool) -> some View {
        Button {
            onChoose(item.id)
        } label: {
            VStack(alignment: .leading, spacing: 1) {
                Text(item.label)
                    .lineLimit(1)
                if let secondary = item.secondary, !secondary.isEmpty {
                    Text(secondary)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            .background(selected ? Color.accentColor.opacity(0.18) : Color.clear, in: .rect(cornerRadius: 6))
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .id(item.id)
    }
}

/// The commands plugins add to the note editor's menu, by plugin.
struct PluginEditorMenuList: View {
    let menus: [PluginEditorMenu]
    let onChoose: (PluginEditorMenu, Int) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            if menus.isEmpty {
                Text("No plugin actions here.")
                    .foregroundStyle(.secondary)
                    .padding(8)
            }
            ForEach(Array(menus.enumerated()), id: \.offset) { offset, menu in
                if offset > 0 { Divider() }
                ForEach(Array(menu.entries.enumerated()), id: \.offset) { _, entry in
                    switch entry {
                    case .command(let item):
                        row(item, menu: menu)
                    case .separator:
                        Divider()
                    case .submenu(let label, let disabled, let children):
                        Text(verbatim: label)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 8)
                            .padding(.top, 4)
                        ForEach(Array(children.enumerated()), id: \.offset) { _, child in
                            if case .command(let item) = child {
                                row(item, menu: menu)
                                    .disabled(disabled)
                            }
                        }
                    }
                }
            }
        }
        .padding(6)
        .frame(minWidth: 220, alignment: .leading)
    }

    private func row(_ item: ReaderMenuItem, menu: PluginEditorMenu) -> some View {
        Button {
            onChoose(menu, item.index)
        } label: {
            Text(verbatim: item.label)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .disabled(item.disabled)
    }
}

