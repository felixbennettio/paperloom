import AppIntents
import CoreKit
import SwiftUI

@main
struct PaperloomApp: App {
    @State private var library = AppleServices.shared.library
    @State private var settings = AppleServices.shared.settings
    @State private var account = AccountModel()
    @State private var vault = AppleServices.shared.vault
    @State private var sync = SyncModel()
    @State private var ai = AIAssistant()
    @State private var plugins = PluginHost()
    @State private var databaseKeys = DatabaseKeys()
    @State private var catalogues = Catalogues()
    @State private var browser = BrowserModel()
    @State private var sharedFiles = SharedFileImport()

    init() {
        PaperloomShortcuts.updateAppShortcutParameters()
    }

    var body: some Scene {
        WindowGroup(id: "library") {
            RootView(sharedFiles: sharedFiles)
                .modifier(PluginChrome())
                .task {
                    plugins.connect(to: vault, library: library)
                    plugins.reload()
                }
                // Revision is an invalidation hint, including connector/import/
                // sync writes and scope changes. Observe outside list/column views.
                .onChange(of: library.revision, initial: true) { _, revision in
                    plugins.itemsChanged(revision: revision)
                }
                // Notes with a lasting identity are Zotero note items too.
                .onChange(of: vault.changes) {
                    plugins.filesChanged()
                    plugins.itemsChanged()
                }
                .task { await library.backUpOnSchedule(vault: vault) }
                #if os(macOS)
                .task { library.startConnector() }
                #endif
                .environment(library)
                .environment(settings)
                .environment(account)
                .environment(vault)
                .environment(sync)
                .environment(ai)
                .environment(databaseKeys)
                .environment(catalogues)
                .environment(browser)
                .environment(plugins)
        }
        #if os(macOS)
        .defaultSize(width: 1280, height: 820)
        #endif

        #if os(macOS)
        Settings {
            SettingsView()
                .environment(library)
                .environment(settings)
                .environment(account)
                .environment(vault)
                .environment(sync)
                .environment(ai)
                .environment(databaseKeys)
                .environment(catalogues)
                .environment(plugins)
                // Resizable, so long sections and fields need not be squeezed.
                .frame(minWidth: 520, idealWidth: 640, minHeight: 480, idealHeight: 700)
        }
        .defaultSize(width: 640, height: 700)
        .windowResizability(.contentMinSize)
        #endif
    }
}

/// Phones get a tab bar; iPads and Macs get the workbench with sidebars and
/// document tabs.
struct RootView: View {
    let sharedFiles: SharedFileImport
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif

    var body: some View {
        content.modifier(SharedFileReceiver(imports: sharedFiles))
            .modifier(AppleActivityReceiver())
            .preferredColorScheme(ScreenshotSample.colorScheme)
    }

    @ViewBuilder private var content: some View {
        #if os(iOS)
        if sizeClass == .compact {
            PhoneRootView()
        } else {
            WorkbenchView()
        }
        #else
        WorkbenchView()
            .onAppear {
                // Once the window is on screen.
                DispatchQueue.main.async { ScreenshotSample.sizeWindowForStore() }
            }
        #endif
    }
}
