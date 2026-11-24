import CoreKit
import Foundation
import Observation

struct DetectedTranslator {
    let id: String
    let label: String
    let itemType: String
}

struct TranslatorChoice: Identifiable {
    let id: String
    let title: String
}

struct TranslatorSelection: Identifiable {
    let id = UUID()
    let items: [TranslatorChoice]
}

/// The shared translation runtime uses the plugin frame's message relay.
@MainActor
final class TranslatorRunner: PluginFrameRunner {
    weak var host: TranslatorHost?

    init(host: TranslatorHost) {
        self.host = host
        super.init(page: "translate-frame.html", isolated: true)
    }

    override func receive(_ object: [String: Any]) {
        host?.receive(object, from: self)
    }
}

/// Translator code runs apart from the browser and the app's own pages.
@MainActor
@Observable
final class TranslatorHost {
    static let changed = Notification.Name("paperloom-translators-changed")
    var selection: TranslatorSelection?
    @ObservationIgnored private var runner: TranslatorRunner?
    @ObservationIgnored private var core: PaperLibrary?
    @ObservationIgnored private var ready: CheckedContinuation<Void, Error>?
    @ObservationIgnored private var pending: [Int: (kind: String, continuation: CheckedContinuation<[String: Any], Error>)] = [:]
    @ObservationIgnored private var picker: CheckedContinuation<[String], Never>?
    @ObservationIgnored private var pickerKeys: [String] = []
    @ObservationIgnored private var activeCalls = Set<Int>()
    @ObservationIgnored private var timeout: Task<Void, Never>?
    @ObservationIgnored private var nextID = 1
    @ObservationIgnored private var detected = false
    @ObservationIgnored private var saving = false

    func detect(core: PaperLibrary, url: String, html: String, translators: [WebTranslator]) async throws -> DetectedTranslator? {
        reset()
        guard !translators.isEmpty, html.utf8.count <= 5 * 1024 * 1024 else { return nil }
        self.core = core
        let frame = TranslatorRunner(host: self)
        runner = frame
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            ready = continuation
            armTimeout(for: frame)
        }
        guard runner === frame else { throw CancellationError() }
        let answer = try await ask("translate", values: [
            "url": url, "html": html, "translators": Self.catalog(translators),
        ], from: frame)
        guard runner === frame else { throw CancellationError() }
        guard let id = answer["translator"] as? String,
              let label = answer["label"] as? String,
              let itemType = answer["itemType"] as? String
        else {
            reset()
            return nil
        }
        detected = true
        return DetectedTranslator(id: id, label: label, itemType: itemType)
    }

    /// Completed items return as JSON for the core's existing Add path.
    func save() async throws -> String {
        guard detected, !saving, let frame = runner else { throw CocoaError(.featureUnsupported) }
        saving = true
        defer {
            if runner === frame {
                saving = false
                cancelSelection()
            }
        }
        let answer = try await ask("save", values: [:], from: frame)
        guard runner === frame else { throw CancellationError() }
        guard let items = answer["items"] as? [[String: Any]], JSONSerialization.isValidJSONObject(items) else {
            throw CocoaError(.coderReadCorrupt)
        }
        let data = try JSONSerialization.data(withJSONObject: items)
        guard let json = String(data: data, encoding: .utf8) else { throw CocoaError(.coderReadCorrupt) }
        return json
    }

    /// The picker answers only after its sheet has finished dismissing.
    func choose(_ keys: [String], for id: UUID) {
        guard let selection, selection.id == id else { return }
        let allowed = Set(selection.items.map(\.id))
        pickerKeys = keys.filter { allowed.contains($0) }
        self.selection = nil
    }

    func selectionDismissed() {
        let continuation = picker
        picker = nil
        let keys = pickerKeys
        pickerKeys = []
        selection = nil
        continuation?.resume(returning: keys)
    }

    func cancelSelection() {
        pickerKeys = []
        selectionDismissed()
    }

    /// Navigation and installation changes invalidate every outstanding reply.
    func reset() {
        reset(with: CancellationError())
    }

    private func reset(with error: Error) {
        timeout?.cancel()
        timeout = nil
        runner?.stop()
        runner = nil
        core = nil
        detected = false
        saving = false
        cancelSelection()
        ready?.resume(throwing: error)
        ready = nil
        let requests = pending.values.map { $0.continuation }
        pending.removeAll()
        activeCalls.removeAll()
        for continuation in requests { continuation.resume(throwing: error) }
    }

    private func ask(_ kind: String, values: [String: Any], from frame: TranslatorRunner) async throws -> [String: Any] {
        guard runner === frame else { throw CancellationError() }
        let id = nextID
        nextID += 1
        return try await withCheckedThrowingContinuation { continuation in
            pending[id] = (kind: kind, continuation: continuation)
            var message = values
            message["type"] = kind
            message["id"] = id
            frame.send(message)
            armTimeout(for: frame)
        }
    }

    /// Core requests and the checklist have their own completion paths.
    private func armTimeout(for frame: TranslatorRunner) {
        timeout?.cancel()
        timeout = Task {
            try? await Task.sleep(for: .seconds(5))
            guard !Task.isCancelled, runner === frame, ready != nil || !pending.isEmpty else { return }
            if activeCalls.isEmpty {
                reset(with: URLError(.timedOut))
            } else {
                armTimeout(for: frame)
            }
        }
    }

    func receive(_ message: [String: Any], from frame: TranslatorRunner) {
        guard runner === frame else { return }
        switch message["type"] as? String {
        case "loaded":
            timeout?.cancel()
            timeout = nil
            ready?.resume()
            ready = nil
        case "call":
            guard let id = message["id"] as? Int, let method = message["method"] as? String,
                  let arguments = message["args"] as? [Any], !activeCalls.contains(id)
            else { return }
            activeCalls.insert(id)
            Task {
                guard runner === frame else { return }
                do {
                    let value = try await perform(method, arguments: arguments)
                    guard runner === frame else { return }
                    frame.send(["type": "reply", "id": id, "value": value])
                } catch {
                    guard runner === frame else { return }
                    frame.send(["type": "reply", "id": id, "error": CoreKit.message(for: error)])
                }
                activeCalls.remove(id)
                if !pending.isEmpty { armTimeout(for: frame) }
            }
        case "detected", "translated", "translate-error", "error":
            guard let id = message["id"] as? Int, let request = pending[id] else { return }
            let type = message["type"] as? String
            if type == "detected" && request.kind != "translate" { return }
            if type == "translated" && request.kind != "save" { return }
            pending.removeValue(forKey: id)
            timeout?.cancel()
            timeout = nil
            if type == "translate-error" || type == "error" {
                let error = TranslatorFailure(message: message["message"] as? String ?? String(localized: "This page could not be read."))
                request.continuation.resume(throwing: error)
                reset(with: error)
            } else {
                request.continuation.resume(returning: message)
            }
        default:
            break
        }
    }

    private func perform(_ method: String, arguments: [Any]) async throws -> Any {
        guard let core else { throw CocoaError(.featureUnsupported) }
        switch method {
        case "translatorCode":
            guard let id = arguments.first as? String else { throw CocoaError(.coderReadCorrupt) }
            return try await Task.detached { try core.translatorCode(id: id) }.value
        case "translators":
            let translators = try await Task.detached { try core.translatorCatalog() }.value
            return Self.catalog(translators)
        case "request":
            guard let request = arguments.first as? [String: Any], let url = request["url"] as? String,
                  let method = request["method"] as? String, let headers = request["headers"] as? [String: String],
                  request["body"] == nil || request["body"] is NSNull || request["body"] is String
            else { throw CocoaError(.coderReadCorrupt) }
            let fields = headers.map { PluginHeader(name: $0.key, value: $0.value) }
            let body = request["body"] as? String
            let answer = try await Task.detached {
                try translatorRequest(url: url, method: method, headers: fields, body: body)
            }.value
            return ["status": Int(answer.status), "text": answer.text, "url": answer.url] as [String: Any]
        case "selectItems":
            guard saving, picker == nil, let labels = arguments.first as? [String: Any] else {
                throw CocoaError(.coderReadCorrupt)
            }
            return await withCheckedContinuation { (continuation: CheckedContinuation<[String], Never>) in
                picker = continuation
                pickerKeys = []
                selection = TranslatorSelection(items: labels.keys.sorted().map {
                    TranslatorChoice(id: $0, title: labels[$0] as? String ?? String(describing: labels[$0] ?? ""))
                })
            }
        default:
            throw CocoaError(.featureUnsupported)
        }
    }

    private static func catalog(_ translators: [WebTranslator]) -> [[String: Any]] {
        translators.map { ["id": $0.id, "label": $0.label, "target": $0.target, "priority": Int($0.priority)] }
    }
}

struct TranslatorFailure: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}
