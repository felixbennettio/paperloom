import AuthenticationServices
import CoreKit
import SwiftUI
import UniformTypeIdentifiers

struct SettingsView: View {
    @Environment(WorkbenchSettings.self) private var settings
    @Environment(LibraryModel.self) private var library
    @Environment(AccountModel.self) private var account
    @Environment(SyncModel.self) private var sync
    @Environment(VaultModel.self) private var vault
    @Environment(AIAssistant.self) private var ai
    @Environment(DatabaseKeys.self) private var databaseKeys
    @Environment(\.webAuthenticationSession) private var webAuthenticationSession
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    @State private var choosingFolder = false
    @State private var passphrase = ""
    @State private var showsNotices = false

    var body: some View {
        @Bindable var settings = settings
        Form {
            accountSection
            syncSection
            Section("Search") {
                Toggle("Show in Spotlight", isOn: $settings.showInSpotlight)
            }
            aiSection
            NotesSettingsSection()
            CitationStylesSection()
            #if os(macOS)
            WritingPluginsSection()
            #endif
            TranslatorsSection()
            PluginsSection()
            ZoteroPluginsSection()
            Section("Databases") {
                ForEach(databaseKeys.sources) { source in
                    DatabaseKeyField(source: source)
                }
                Text("Keys stay on this device and go only to their database. What you find depends on your subscription.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            CataloguesSection()
            // The iPhone has no workbench: tabs and the notes panel are
            // for the iPad and the Mac.
            if hasWorkbench {
                Section("Workbench") {
                    Picker("Tabs", selection: $settings.tabStyle) {
                        ForEach(TabStripStyle.allCases) { style in
                            Text(style.title).tag(style)
                        }
                    }
                    Picker("Notes panel", selection: $settings.notesEdge) {
                        ForEach(PanelEdge.allCases) { edge in
                            Text(edge.title).tag(edge)
                        }
                    }
                    Toggle("Show notes panel", isOn: $settings.showsNotes)
                }
            }
            BackupSection()
            Section("About") {
                LabeledContent("Version", value: Self.appVersion)
                LabeledContent("Core", value: coreVersion())
                LabeledContent("Library") {
                    Text(library.location)
                        .textSelection(.enabled)
                        .lineLimit(2)
                        .truncationMode(.middle)
                }
                Button("Third-Party Notices") { showsNotices = true }
            }
        }
        .formStyle(.grouped)
        .sheet(isPresented: $showsNotices) { NoticesView() }
        .navigationTitle("Settings")
    }

    private var hasWorkbench: Bool {
        #if os(iOS)
        sizeClass != .compact
        #else
        true
        #endif
    }

    /// Semantic search: the embedding model, and the index of the library.
    @ViewBuilder
    private var semanticRows: some View {
        @Bindable var ai = ai
        NamedField("Embedding model", text: $ai.embeddingModel, prompt: Text(verbatim: "text-embedding-3-small"))
            .autocorrectionDisabled()
        if !ai.provider.embeddingModel.isEmpty {
            LabeledContent("Semantic search") {
                HStack {
                    if let status = ai.semantic {
                        Text("Indexed \(String(status.indexed)) of \(String(status.total))")
                            .foregroundStyle(.secondary)
                    }
                    Button {
                        Task { await ai.buildSemanticIndex(library: library.core) }
                    } label: {
                        if ai.isIndexing {
                            ProgressView().controlSize(.small)
                        } else {
                            Text("Build Semantic Index")
                        }
                    }
                    .disabled(ai.isIndexing)
                }
            }
            .task(id: "\(ai.baseURL) \(ai.provider.embeddingModel)") { await ai.refreshSemantic(library: library.core) }
        }
        Text(
            "With an embedding model, Ask the Library also finds passages by meaning. Building the index sends the text of your papers and notes to the provider."
        )
        .font(.caption)
        .foregroundStyle(.secondary)
    }

    @ViewBuilder
    private var accountSection: some View {
        Section {
            if let user = account.user {
                LabeledContent {
                    Button("Sign Out", role: .destructive) { account.signOut() }
                } label: {
                    Label {
                        VStack(alignment: .leading) {
                            Text(user.displayName)
                            if let email = user.email {
                                Text(email).font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    } icon: {
                        Image(systemName: "person.crop.circle.fill")
                    }
                }
            } else {
                Button {
                    Task { await account.signIn(with: webAuthenticationSession) }
                } label: {
                    HStack {
                        Label("Sign In", systemImage: "person.crop.circle.badge.plus")
                        if account.isWorking {
                            Spacer()
                            ProgressView().controlSize(.small)
                        }
                    }
                }
                .disabled(!account.isAvailable || account.isWorking)
            }
            if let message = account.errorMessage {
                Text(message).font(.caption).foregroundStyle(.red)
            }
        } header: {
            Text("Account")
        } footer: {
            if !account.isAvailable {
                Text("Signing in is not set up in this build yet.")
            } else if account.user == nil {
                Text("Sign in to sync your library across devices with Paperloom Cloud.")
            }
        }
    }

    private var syncSection: some View {
        @Bindable var sync = sync
        return Section("Sync") {
            Picker("Sync with", selection: $sync.target) {
                ForEach(SyncTarget.allCases) { target in
                    Text(target.title).tag(target)
                }
            }
            switch sync.target {
            case .cloud:
                if account.user == nil {
                    Text("Sign in under Account first to sync with Paperloom Cloud.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else {
                    NamedField("Sync service", text: $sync.service)
                        .autocorrectionDisabled()
                }
            case .folder:
                LabeledContent("Folder") {
                    Button {
                        choosingFolder = true
                    } label: {
                        Text(sync.folderPath.map { ($0 as NSString).lastPathComponent } ?? String(localized: "Choose…"))
                    }
                }
                Text("iCloud Drive, a synced Nextcloud, Seafile or Syncthing folder, or a NAS share. Each library needs a folder of its own.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            case .webdav:
                NamedField("WebDAV address", text: $sync.webdavAddress, prompt: Text(verbatim: "https://dav.jianguoyun.com/dav/Paperloom"))
                    .autocorrectionDisabled()
                NamedField("User name", text: $sync.webdavUser)
                    .autocorrectionDisabled()
                NamedField("Password", text: $sync.webdavPassword, secure: true)
                Text("For 坚果云 and Nextcloud, use an app password.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            case .seafile:
                NamedField("Seafile address", text: $sync.seafileServer, prompt: Text(verbatim: "https://cloud.example.com"))
                    .autocorrectionDisabled()
                NamedField("Seafile account", text: $sync.seafileAccount)
                    .autocorrectionDisabled()
                NamedField("Seafile library", text: $sync.seafileLibrary, prompt: Text(verbatim: "Paperloom"))
                    .autocorrectionDisabled()
                NamedField("Folder in the library", text: $sync.seafileFolder)
                    .autocorrectionDisabled()
                NamedField("Password or API token", text: $sync.seafilePassword, secure: true)
                Text("The library is created if missing. Leave the folder empty for its top. A credential of exactly 40 hexadecimal characters is used as an account API token; otherwise it is a password.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            case .s3:
                NamedField("S3 address", text: $sync.s3Endpoint, prompt: Text(verbatim: "https://s3.us-east-1.amazonaws.com"))
                    .autocorrectionDisabled()
                NamedField("Region", text: $sync.s3Region, prompt: Text(verbatim: "us-east-1"))
                    .autocorrectionDisabled()
                NamedField("Bucket", text: $sync.s3Bucket)
                    .autocorrectionDisabled()
                NamedField("Folder in the bucket", text: $sync.s3Prefix, prompt: Text(verbatim: "paperloom"))
                    .autocorrectionDisabled()
                NamedField("Access key ID", text: $sync.s3AccessKeyID)
                    .autocorrectionDisabled()
                NamedField("Secret key", text: $sync.s3Secret, secure: true)
                Text("AWS S3, Cloudflare R2 (region auto), MinIO, 阿里云 OSS or 腾讯云 COS. The bucket must exist; give the keys access to it alone.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            if sync.sealed {
                LabeledContent("End-to-end encryption") {
                    Button("Turn Off") { Task { await sync.setPassphrase("", library: library) } }
                        .disabled(sync.isSealing)
                }
            } else {
                LabeledContent("End-to-end encryption") {
                    HStack {
                        SecureField("Passphrase", text: $passphrase, prompt: Text("Passphrase"))
                            .labelsHidden()
                            .textFieldStyle(.roundedBorder)
                            .frame(minWidth: 160)
                            .onSubmit { turnOnSealing() }
                        Button("Turn On", action: turnOnSealing)
                            .disabled(passphrase.isEmpty || sync.isSealing)
                    }
                }
            }
            Text("Changes and files are sealed on this device before they sync. Every device needs the same passphrase, and nobody can recover it.")
                .font(.caption)
                .foregroundStyle(.secondary)
            LabeledContent {
                Button {
                    Task { await sync.syncNow(library: library, vault: vault, account: account) }
                } label: {
                    if sync.isSyncing {
                        ProgressView().controlSize(.small)
                    } else {
                        Text("Sync Now")
                    }
                }
                .disabled(sync.isSyncing || !sync.isReady(signedIn: account.user != nil))
            } label: {
                Text(sync.summary).font(.caption).foregroundStyle(.secondary)
            }
            if let report = sync.lastReport {
                Text(
                    "Changes sent: \(String(report.sent)); received: \(String(report.received)). Files downloaded: \(String(report.downloaded))."
                )
                .font(.caption)
                .foregroundStyle(.secondary)
            }
            if let message = sync.errorMessage {
                Text(message).font(.caption).foregroundStyle(.red)
            }
        }
        .fileImporter(isPresented: $choosingFolder, allowedContentTypes: [.folder]) { result in
            if case .success(let url) = result {
                sync.chooseFolder(url)
            }
        }
        .task { sync.refresh(library: library) }
    }

    private func turnOnSealing() {
        guard !passphrase.isEmpty else { return }
        let chosen = passphrase
        passphrase = ""
        Task { await sync.setPassphrase(chosen, library: library) }
    }

    private var aiSection: some View {
        @Bindable var ai = ai
        return Section("AI") {
            Picker("Provider", selection: Binding(get: { ai.api }, set: { ai.choose(api: $0) })) {
                Text(verbatim: "Anthropic").tag("anthropic")
                Text("OpenAI-compatible").tag("openai")
            }
            if ai.api == "openai" {
                Menu("Preset") {
                    ForEach(AIAssistant.presets, id: \.url) { preset in
                        Button(preset.name) {
                            ai.baseURL = preset.url
                            ai.model = ""
                        }
                    }
                }
            }
            NamedField("Address", text: $ai.baseURL)
                .autocorrectionDisabled()
            NamedField("API key", text: $ai.key, secure: true)
            if ai.models.isEmpty {
                NamedField("Model", text: $ai.model)
                    .autocorrectionDisabled()
            } else {
                Picker("Model", selection: $ai.model) {
                    ForEach(ai.models, id: \.id) { model in
                        Text(verbatim: model.name).tag(model.id)
                    }
                }
            }
            Button {
                Task { await ai.loadModels() }
            } label: {
                if ai.isLoadingModels {
                    ProgressView().controlSize(.small)
                } else {
                    Text("Load Models")
                }
            }
            .disabled(ai.isLoadingModels || ai.baseURL.isEmpty)
            if ai.api == "openai" {
                semanticRows
            }
            Text("The key stays on this device. Papers are sent to the provider only when you ask.")
                .font(.caption)
                .foregroundStyle(.secondary)
            if let message = ai.errorMessage {
                Text(message).font(.caption).foregroundStyle(.red)
            }
        }
    }

    private static var appVersion: String {
        let info = Bundle.main.infoDictionary
        let version = info?["CFBundleShortVersionString"] as? String ?? "?"
        let build = info?["CFBundleVersion"] as? String ?? "?"
        return "\(version) (\(build))"
    }
}
