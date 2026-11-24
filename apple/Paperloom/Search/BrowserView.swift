import CoreKit
import Observation
import SwiftUI
import WebKit
#if os(iOS)
import UIKit
#endif

/// The in-app browser. A person signs in to their library's databases
/// (CNKI, Wanfang, VIP, publishers) here with their own access; Paperloom
/// reads the works a page lists or shows, and adds the PDFs downloaded here.
/// One web view for the whole session, so pages and sign-ins stay as they
/// were between visits.
@MainActor
@Observable
final class BrowserModel {
    let webView: WKWebView
    private(set) var url: URL?
    private(set) var title = ""
    private(set) var canGoBack = false
    private(set) var canGoForward = false
    private(set) var isLoading = false
    private(set) var isReading = false
    private(set) var isSaving = false
    private(set) var translator: DetectedTranslator?
    let translatorHost = TranslatorHost()
    /// The page is a PDF, shown as it is.
    var isShowingPDF = false
    var address = ""
    /// What was read from the page, for picking and adding.
    let found = OnlineSearchModel()
    var showingFound = false
    var message: String?
    /// Where downloaded PDFs go.
    @ObservationIgnored weak var library: LibraryModel?
    @ObservationIgnored private let delegate = BrowserDelegate()
    @ObservationIgnored private var observations: [NSKeyValueObservation] = []
    @ObservationIgnored private var detectionTask: Task<Void, Never>?
    @ObservationIgnored private var pageGeneration = 0

    /// Places worth starting from: the sites the rules read, and more.
    let sites: [CaptureSite] =
        captureSites() + [
            CaptureSite(id: "scholar", name: "Google Scholar", home: "https://scholar.google.com/"),
            CaptureSite(id: "baidu", name: "Baidu Scholar", home: "https://xueshu.baidu.com/"),
        ]

    init() {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .default()
        webView = WKWebView(frame: .zero, configuration: configuration)
        webView.allowsBackForwardNavigationGestures = true
        delegate.model = self
        webView.navigationDelegate = delegate
        webView.uiDelegate = delegate
        // WebKit reports these on the main thread.
        observations = [
            webView.observe(\.url) { [weak self] view, _ in
                MainActor.assumeIsolated {
                    self?.url = view.url
                    self?.address = view.url?.absoluteString ?? ""
                }
            },
            webView.observe(\.title) { [weak self] view, _ in
                MainActor.assumeIsolated { self?.title = view.title ?? "" }
            },
            webView.observe(\.canGoBack) { [weak self] view, _ in
                MainActor.assumeIsolated { self?.canGoBack = view.canGoBack }
            },
            webView.observe(\.canGoForward) { [weak self] view, _ in
                MainActor.assumeIsolated { self?.canGoForward = view.canGoForward }
            },
            webView.observe(\.isLoading) { [weak self] view, _ in
                MainActor.assumeIsolated { self?.isLoading = view.isLoading }
            },
        ]
    }

    /// Goes to an address someone typed: `cnki.net` is `https://cnki.net/`.
    func go(to text: String) {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        let full = text.contains("://") ? text : "https://\(text)"
        guard let url = URL(string: full), url.scheme == "https" || url.scheme == "http" else {
            message = String(localized: "“\(text)” is not a web address.")
            return
        }
        webView.load(URLRequest(url: url))
    }

    /// Reads the works on the page (works named only by DOI are looked up),
    /// or, for a PDF, downloads it into the library.
    func read(in library: LibraryModel) async {
        guard let core = library.core, !isReading, !isSaving else { return }
        if isShowingPDF, let url {
            // Through the web view, with its sign-ins.
            let download = await webView.startDownload(using: URLRequest(url: url))
            download.delegate = delegate
            return
        }
        isReading = true
        defer { isReading = false }
        do {
            let answer = try await webView.evaluateJavaScript(captureScript())
            guard let json = answer as? String else { throw BrowserCouldNotRead() }
            let page = try await Task.detached { try core.readCapturedPage(capture: json) }.value
            found.show(page.hits, from: title)
            showingFound = true
        } catch {
            message = CoreKit.message(for: error)
        }
    }

    /// A page change discards its translator and any unanswered selection.
    func navigationStarted() {
        cancelTranslation()
        isShowingPDF = false
    }

    func cancelTranslation() {
        pageGeneration += 1
        detectionTask?.cancel()
        detectionTask = nil
        translator = nil
        isSaving = false
        translatorHost.reset()
    }

    /// Detection takes a bounded snapshot only when the optional translators exist.
    func detectTranslators() {
        cancelTranslation()
        guard let core = library?.core, let pageURL = webView.url?.absoluteString,
              !webView.isLoading, !isShowingPDF,
              let scheme = webView.url?.scheme, scheme == "http" || scheme == "https"
        else { return }
        let generation = pageGeneration
        detectionTask = Task {
            defer { if pageGeneration == generation { detectionTask = nil } }
            do {
                let translators = try await Task.detached {
                    guard try core.translatorsStatus().count > 0 else { return [WebTranslator]() }
                    return try core.webTranslators()
                }.value
                guard pageGeneration == generation, !Task.isCancelled, !translators.isEmpty else { return }
                let answer = try await webView.evaluateJavaScript("""
                    (() => {
                      const limit = 5 * 1024 * 1024;
                      const text = (document.documentElement?.outerHTML || "").slice(0, limit);
                      const bytes = new TextEncoder().encode(text);
                      const html = new TextDecoder().decode(bytes.subarray(0, limit), { stream: true });
                      return { url: location.href, html };
                    })()
                    """)
                guard pageGeneration == generation, !Task.isCancelled,
                      let snapshot = answer as? [String: Any], let html = snapshot["html"] as? String,
                      snapshot["url"] as? String == pageURL, html.utf8.count <= 5 * 1024 * 1024
                else { return }
                let detected = try await translatorHost.detect(core: core, url: pageURL, html: html, translators: translators)
                guard pageGeneration == generation, !Task.isCancelled else { return }
                translator = detected
            } catch is CancellationError {
            } catch {
                if pageGeneration == generation { message = CoreKit.message(for: error) }
            }
        }
    }

    /// Translated works use the same results and PDF handling as page captures.
    func saveWithTranslator(in library: LibraryModel) async {
        guard let core = library.core, let pageURL = webView.url?.absoluteString,
              translator != nil, !isSaving, !isReading, !isLoading
        else { return }
        let generation = pageGeneration
        let pageTitle = title
        isSaving = true
        message = nil
        defer { if pageGeneration == generation { isSaving = false } }
        do {
            let json = try await translatorHost.save()
            guard pageGeneration == generation else { return }
            let page = try await Task.detached { try core.readTranslatedItems(json: json, pageUrl: pageURL) }.value
            guard pageGeneration == generation else { return }
            found.show(page.hits, from: pageTitle)
            showingFound = true
        } catch is CancellationError {
        } catch {
            if pageGeneration == generation {
                translator = nil
                translatorHost.reset()
                message = CoreKit.message(for: error)
            }
        }
    }

    /// A finished download: a PDF joins the library; anything else goes to
    /// the Downloads folder (on a Mac).
    func downloaded(_ file: URL) {
        if Self.isPDF(file) {
            Task { await library?.importPDFs([file]) }
            return
        }
        #if os(macOS)
        do {
            let folder = try FileManager.default.url(
                for: .downloadsDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            let target = folder.appending(path: file.lastPathComponent)
            try FileManager.default.moveItem(at: file, to: target)
            message = String(localized: "Saved \(target.path(percentEncoded: false)).")
        } catch {
            message = error.localizedDescription
        }
        #else
        message = String(localized: "\(file.lastPathComponent) is not a PDF, so it was not kept.")
        #endif
    }

    func failed(_ error: Error) {
        let error = error as NSError
        // Stopped on purpose, or turned into a download.
        let quiet = (error.domain == NSURLErrorDomain && error.code == NSURLErrorCancelled)
            || (error.domain == "WebKitErrorDomain" && error.code == 102)
        if !quiet { message = error.localizedDescription }
    }

    private static func isPDF(_ file: URL) -> Bool {
        guard let handle = try? FileHandle(forReadingFrom: file) else { return false }
        defer { try? handle.close() }
        return (try? handle.read(upToCount: 5)) == Data("%PDF-".utf8)
    }
}

struct BrowserCouldNotRead: LocalizedError {
    var errorDescription: String? { String(localized: "This page could not be read.") }
}

/// The web view's delegate: downloads, links meant for new windows, and
/// failed loads.
@MainActor
final class BrowserDelegate: NSObject, WKNavigationDelegate, WKUIDelegate, WKDownloadDelegate {
    weak var model: BrowserModel?
    private var destinations: [ObjectIdentifier: URL] = [:]

    func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
        model?.navigationStarted()
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        model?.detectTranslators()
    }

    /// PDFs show as they are; files the web view cannot show come down.
    func webView(
        _ webView: WKWebView, decidePolicyFor navigationResponse: WKNavigationResponse
    ) async -> WKNavigationResponsePolicy {
        if navigationResponse.isForMainFrame {
            model?.isShowingPDF = navigationResponse.response.mimeType == "application/pdf"
        }
        return navigationResponse.canShowMIMEType ? .allow : .download
    }

    func webView(_ webView: WKWebView, navigationResponse: WKNavigationResponse, didBecome download: WKDownload) {
        download.delegate = self
    }

    func webView(_ webView: WKWebView, navigationAction: WKNavigationAction, didBecome download: WKDownload) {
        download.delegate = self
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        model?.failed(error)
    }

    func webView(
        _ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error
    ) {
        model?.failed(error)
    }

    /// A link meant for a new window opens here.
    func webView(
        _ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
        for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures
    ) -> WKWebView? {
        if navigationAction.targetFrame == nil {
            webView.load(navigationAction.request)
        }
        return nil
    }

    func download(
        _ download: WKDownload, decideDestinationUsing response: URLResponse, suggestedFilename: String
    ) async -> URL? {
        let folder = FileManager.default.temporaryDirectory
            .appending(path: "Downloads", directoryHint: .isDirectory)
            .appending(path: UUID().uuidString, directoryHint: .isDirectory)
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let name = suggestedFilename.isEmpty ? "download" : suggestedFilename
        let destination = folder.appending(path: name)
        destinations[ObjectIdentifier(download)] = destination
        return destination
    }

    func downloadDidFinish(_ download: WKDownload) {
        if let file = destinations.removeValue(forKey: ObjectIdentifier(download)) {
            model?.downloaded(file)
        }
    }

    func download(_ download: WKDownload, didFailWithError error: Error, resumeData: Data?) {
        destinations.removeValue(forKey: ObjectIdentifier(download))
        model?.failed(error)
    }
}

/// The web view, in SwiftUI.
#if os(macOS)
struct WebViewBox: NSViewRepresentable {
    let webView: WKWebView
    func makeNSView(context: Context) -> WKWebView { webView }
    func updateNSView(_ view: WKWebView, context: Context) {}
}
#else
struct WebViewBox: UIViewRepresentable {
    let webView: WKWebView
    func makeUIView(context: Context) -> WKWebView { webView }
    func updateUIView(_ view: WKWebView, context: Context) {}
}
#endif

struct BrowserView: View {
    @Environment(BrowserModel.self) private var browser
    @Environment(LibraryModel.self) private var library
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif

    private var isCompact: Bool {
        #if os(iOS)
        sizeClass == .compact || UIDevice.current.userInterfaceIdiom == .phone
        #else
        false
        #endif
    }

    var body: some View {
        @Bindable var browser = browser
        @Bindable var translatorHost = browser.translatorHost
        WebViewBox(webView: browser.webView)
            .overlay {
                if browser.url == nil { startPage }
            }
            .safeAreaInset(edge: .top, spacing: 0) { addressBar }
            .navigationTitle(browser.title.isEmpty ? String(localized: "Browser") : browser.title)
            .onAppear {
                browser.library = library
                browser.detectTranslators()
            }
            .onDisappear { browser.cancelTranslation() }
            .onReceive(NotificationCenter.default.publisher(for: TranslatorHost.changed)) { _ in
                browser.detectTranslators()
            }
            .sheet(item: $translatorHost.selection, onDismiss: { translatorHost.selectionDismissed() }) { selection in
                TranslatorPicker(selection: selection, host: translatorHost)
            }
            .sheet(isPresented: $browser.showingFound) {
                FoundOnPageView(model: browser.found, title: browser.found.searchedText ?? browser.title)
            }
            .alert(
                "Browser",
                isPresented: Binding(get: { browser.message != nil }, set: { if !$0 { browser.message = nil } })
            ) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(verbatim: browser.message ?? "")
            }
    }

    private var addressBar: some View {
        @Bindable var browser = browser
        return HStack(spacing: 8) {
            Button("Back", systemImage: "chevron.backward") { browser.webView.goBack() }
                .disabled(!browser.canGoBack)
            Button("Forward", systemImage: "chevron.forward") { browser.webView.goForward() }
                .disabled(!browser.canGoForward)
            Button("Reload", systemImage: browser.isLoading ? "xmark" : "arrow.clockwise") {
                if browser.isLoading { browser.webView.stopLoading() } else { browser.webView.reload() }
            }
            TextField("Address or site", text: $browser.address)
                .textFieldStyle(.roundedBorder)
                .autocorrectionDisabled()
                #if os(iOS)
                .keyboardType(.URL)
                .textInputAutocapitalization(.never)
                #endif
                .onSubmit { browser.go(to: browser.address) }
            Menu {
                if isCompact, browser.translator != nil {
                    translatorButton
                    Divider()
                }
                ForEach(browser.sites, id: \.id) { site in
                    Button(LocalizedStringKey(site.name)) { browser.go(to: site.home) }
                }
            } label: {
                if isCompact, browser.translator != nil {
                    Label("More", systemImage: "ellipsis")
                } else {
                    Label("Sites", systemImage: "list.bullet")
                }
            }
            if !isCompact, browser.translator != nil {
                translatorButton
                    .labelStyle(.titleAndIcon)
                    .buttonStyle(.glassProminent)
                    .frame(maxWidth: 220)
            }
            Button {
                Task { await browser.read(in: library) }
            } label: {
                if browser.isReading {
                    ProgressView().controlSize(.small)
                } else {
                    Label(
                        browser.isShowingPDF ? "Add This PDF" : "Read This Page",
                        systemImage: browser.isShowingPDF ? "arrow.down.doc" : "text.viewfinder")
                }
            }
            .buttonStyle(.glassProminent)
            .disabled(browser.url == nil || browser.isReading || browser.isSaving)
        }
        .labelStyle(.iconOnly)
        .buttonStyle(.borderless)
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(.bar)
    }

    @ViewBuilder
    private var translatorButton: some View {
        if let translator = browser.translator {
            Button {
                Task { await browser.saveWithTranslator(in: library) }
            } label: {
                Label {
                    if browser.isSaving {
                        Text("Saving…")
                    } else {
                        Text("Save with \(Text(verbatim: translator.label))")
                            .lineLimit(1)
                    }
                } icon: {
                    Image(systemName: "square.and.arrow.down")
                }
            }
            .disabled(browser.isSaving || browser.isReading || browser.isLoading)
        }
    }

    private var startPage: some View {
        ContentUnavailableView {
            Label("Browser", systemImage: "safari")
        } description: {
            Text(
                "Sign in to your library's databases here. On a list of results or a paper's page, choose Read This Page to pick what to add. PDFs you download are added too."
            )
        } actions: {
            ForEach(browser.sites, id: \.id) { site in
                Button(LocalizedStringKey(site.name)) { browser.go(to: site.home) }
                    .buttonStyle(.glass)
            }
        }
        .background(.background)
    }
}

/// The works read or translated from a page are ready to pick and add.
struct FoundOnPageView: View {
    let model: OnlineSearchModel
    let title: String
    @Environment(LibraryModel.self) private var library
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Group {
                if model.hits.isEmpty {
                    ContentUnavailableView(
                        "Nothing Found", systemImage: "doc.text.magnifyingglass",
                        description: Text("Nothing on this page could be read as a work."))
                } else {
                    List(model.hits) { hit in
                        SearchHitRow(hit: hit, model: model)
                    }
                }
            }
            .navigationTitle(title.isEmpty ? String(localized: "This Page") : title)
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add \(String(model.chosen.count)) Selected") {
                        model.add(Array(model.chosen), to: library)
                    }
                    .disabled(model.chosen.isEmpty)
                }
            }
            .alert(
                "Browser",
                isPresented: Binding(get: { model.message != nil }, set: { if !$0 { model.message = nil } })
            ) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(model.message ?? "")
            }
        }
        #if os(macOS)
        .frame(minWidth: 560, minHeight: 520)
        #endif
    }
}
