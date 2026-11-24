import SwiftUI
import UniformTypeIdentifiers
import WebKit

/// What a click in the reading view asks for.
enum NoteLinkAction {
    case note(path: String, anchor: String?)
    case newNote(name: String)
    case tag(String)
    /// An embedded PDF highlight, by annotation ID.
    case annotation(String)
    /// A `[[@paper]]` link, by item ID.
    case item(String)
    /// An embedded drawing or diagram, by vault path.
    case drawing(String)
    /// An embedded recording, by vault path.
    case audio(String)
    case diagram(String)
    case web(URL)
}

/// Serves `paperloom-vault:///<path>` (embedded images) from the vault.
final class VaultFileHandler: NSObject, WKURLSchemeHandler {
    let fileURL: (String) -> URL?

    init(fileURL: @escaping (String) -> URL?) {
        self.fileURL = fileURL
    }

    func webView(_ webView: WKWebView, start task: any WKURLSchemeTask) {
        guard let url = task.request.url,
              let file = fileURL(String(url.path(percentEncoded: false).drop { $0 == "/" })),
              let data = try? Data(contentsOf: file)
        else {
            task.didFailWithError(URLError(.fileDoesNotExist))
            return
        }
        let type = UTType(filenameExtension: file.pathExtension)?.preferredMIMEType ?? "application/octet-stream"
        task.didReceive(URLResponse(url: url, mimeType: type, expectedContentLength: data.count, textEncodingName: nil))
        task.didReceive(data)
        task.didFinish()
    }

    func webView(_ webView: WKWebView, stop task: any WKURLSchemeTask) {}
}

/// Shows the core's HTML for a note. Scripts are off; clicks on note, tag
/// and web links come back as `NoteLinkAction`s.
@MainActor
struct NoteWebView {
    let html: String
    /// A heading `id` to scroll to after loading.
    var anchor: String?
    let fileURL: (String) -> URL?
    let onLink: (NoteLinkAction) -> Void

    @MainActor
    final class Coordinator: NSObject, WKNavigationDelegate {
        var onLink: (NoteLinkAction) -> Void
        var loaded = ""
        var anchor: String?

        init(onLink: @escaping (NoteLinkAction) -> Void) {
            self.onLink = onLink
        }

        func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction) async -> WKNavigationActionPolicy {
            guard action.navigationType == .linkActivated, let url = action.request.url else { return .allow }
            let text = url.absoluteString
            func value(after prefix: String) -> String? {
                guard text.hasPrefix(prefix) else { return nil }
                return String(text.dropFirst(prefix.count)).removingPercentEncoding
            }
            if let rest = value(after: "paperloom:note/") {
                let parts = rest.split(separator: "#", maxSplits: 1).map(String.init)
                onLink(.note(path: parts.first ?? rest, anchor: parts.count > 1 ? parts[1] : nil))
            } else if let name = value(after: "paperloom:new/") {
                onLink(.newNote(name: name))
            } else if let tag = value(after: "paperloom:tag/") {
                onLink(.tag(tag))
            } else if let id = value(after: "paperloom:annotation/") {
                onLink(.annotation(id))
            } else if let id = value(after: "paperloom:item/") {
                onLink(.item(id))
            } else if let path = value(after: "paperloom:drawing/") {
                onLink(.drawing(path))
            } else if let path = value(after: "paperloom:diagram/") {
                onLink(.diagram(path))
            } else if let path = value(after: "paperloom:audio/") {
                onLink(.audio(path))
            } else if url.scheme == "http" || url.scheme == "https" || url.scheme == "mailto" {
                onLink(.web(url))
            }
            return .cancel
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            scroll(webView)
        }

        func scroll(_ webView: WKWebView) {
            guard let anchor, !anchor.isEmpty else { return }
            let id = anchor.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "'", with: "\\'")
            // In the app's own world: the page's JavaScript is off.
            webView.evaluateJavaScript(
                "document.getElementById('\(id)')?.scrollIntoView()", in: nil, in: .defaultClient)
        }
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(onLink: onLink)
    }

    private func makeWebView(_ coordinator: Coordinator) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.defaultWebpagePreferences.allowsContentJavaScript = false
        configuration.setURLSchemeHandler(VaultFileHandler(fileURL: fileURL), forURLScheme: "paperloom-vault")
        let view = WKWebView(frame: .zero, configuration: configuration)
        view.navigationDelegate = coordinator
        return view
    }

    private func update(_ view: WKWebView, _ coordinator: Coordinator) {
        coordinator.onLink = onLink
        coordinator.anchor = anchor
        let page = Self.page(html)
        if coordinator.loaded != page {
            coordinator.loaded = page
            view.loadHTMLString(page, baseURL: nil)
        } else {
            coordinator.scroll(view)
        }
    }

    /// The note in a page styled like the app, light or dark.
    static func page(_ body: String) -> String {
        """
        <!doctype html><html><head><meta charset="utf-8">
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <style>
        :root { color-scheme: light dark; --accent: #4f46e5; --muted: #64748b; --border: rgba(15,23,42,.12); }
        @media (prefers-color-scheme: dark) { :root { --accent: #818cf8; --muted: #94a3b8; --border: rgba(255,255,255,.12); } }
        html { background: Canvas; color: CanvasText; }
        body { font: -apple-system-body; font-family: -apple-system, "PingFang SC", sans-serif; line-height: 1.65;
               max-width: 820px; margin: 0 auto; padding: 20px 24px 60px; }
        a { color: var(--accent); text-decoration: none; }
        a.unresolved { opacity: .6; }
        a.tag { padding: 0 6px; border-radius: 999px; background: color-mix(in srgb, var(--accent) 14%, transparent); }
        .citation { color: var(--accent); }
        .references { margin-top: 2em; padding-top: 0.5em; border-top: 1px solid rgba(127,127,127,0.3); font-size: 0.92em; }
        .references .reference { margin: 0.4em 0; padding-left: 2em; text-indent: -2em; }
        .embed { margin: 12px 0; padding: 2px 16px; border-left: 3px solid var(--accent); }
        img { max-width: 100%; }
        .embed-drawing, .embed-diagram { display: block; }
        .plugin-block { margin: 12px 0; overflow-x: auto; }
        .embed-drawing svg { max-width: 100%; height: auto; border: 1px solid var(--border); border-radius: 8px; }
        blockquote { margin: 0; padding-left: 12px; border-left: 3px solid var(--border); color: var(--muted); }
        code { padding: 1px 5px; border-radius: 5px; background: color-mix(in srgb, var(--accent) 12%, transparent); font-size: .92em; }
        pre code { display: block; padding: 10px; overflow-x: auto; }
        table { border-collapse: collapse; } th, td { padding: 4px 10px; border: 1px solid var(--border); }
        .query-results.empty { color: var(--muted); } .query-error { color: #d70015; }
        .query-board { display: flex; gap: 12px; overflow-x: auto; padding-bottom: 4px; }
        .query-column { flex: 0 0 220px; padding: 8px; border-radius: 10px; background: color-mix(in srgb, var(--accent) 6%, transparent); }
        .query-column h4 { margin: 0 0 8px; font-size: .9em; } .query-column h4 span { color: var(--muted); font-weight: normal; }
        .query-column ul { list-style: none; margin: 0; padding: 0; }
        .query-column li { margin-bottom: 6px; padding: 6px 8px; border: 1px solid var(--border); border-radius: 8px; background: Canvas; line-height: 1.4; }
        .query-column small { display: block; color: var(--muted); }
        .query-calendar table { width: 100%; table-layout: fixed; } .query-calendar td { height: 3.6em; vertical-align: top; font-size: .85em; overflow: hidden; }
        .query-calendar .day { display: block; color: var(--muted); font-size: .85em; } .query-calendar td a { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
        </style></head><body>\(body)</body></html>
        """
    }
}

#if os(macOS)
extension NoteWebView: NSViewRepresentable {
    func makeNSView(context: Context) -> WKWebView { makeWebView(context.coordinator) }
    func updateNSView(_ view: WKWebView, context: Context) { update(view, context.coordinator) }
}
#else
extension NoteWebView: UIViewRepresentable {
    func makeUIView(context: Context) -> WKWebView { makeWebView(context.coordinator) }
    func updateUIView(_ view: WKWebView, context: Context) { update(view, context.coordinator) }
}
#endif
