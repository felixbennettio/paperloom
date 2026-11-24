import CoreKit
import Foundation
import Observation
import SwiftUI

/// The AI provider: Anthropic's API or an OpenAI-style one, its address,
/// the person's key (kept in the keychain) and the model, picked from the
/// provider's own list.
@MainActor
@Observable
final class AIAssistant {
    static let anthropicAddress = "https://api.anthropic.com"
    /// OpenAI-style providers people use, by name.
    static let presets: [(name: String, url: String)] = [
        ("OpenAI", "https://api.openai.com/v1"),
        ("DeepSeek", "https://api.deepseek.com/v1"),
        ("Kimi", "https://api.moonshot.cn/v1"),
        ("Qwen", "https://dashscope.aliyuncs.com/compatible-mode/v1"),
        ("GLM", "https://open.bigmodel.cn/api/paas/v4"),
        ("Ollama", "http://localhost:11434/v1"),
    ]

    /// `anthropic` or `openai`.
    private(set) var api: String {
        didSet { UserDefaults.standard.set(api, forKey: "aiAPI") }
    }
    var baseURL: String {
        didSet { UserDefaults.standard.set(baseURL, forKey: "aiBaseURL") }
    }
    var model: String {
        didSet { UserDefaults.standard.set(model, forKey: "aiModel") }
    }
    /// An OpenAI-style embedding model for semantic search; empty for none.
    var embeddingModel: String {
        didSet { UserDefaults.standard.set(embeddingModel, forKey: "aiEmbeddingModel") }
    }
    var key: String {
        didSet { SecretKeychain.save(key, SecretKeychain.ai) }
    }
    private(set) var models: [AiModel] = []
    private(set) var isLoadingModels = false
    /// How much of the library the semantic index covers, for this model.
    private(set) var semantic: AiSemanticStatus?
    private(set) var isIndexing = false
    var errorMessage: String?

    init() {
        let defaults = UserDefaults.standard
        api = defaults.string(forKey: "aiAPI") ?? "anthropic"
        baseURL = defaults.string(forKey: "aiBaseURL") ?? Self.anthropicAddress
        model = defaults.string(forKey: "aiModel") ?? ""
        embeddingModel = defaults.string(forKey: "aiEmbeddingModel") ?? ""
        key = SecretKeychain.load(SecretKeychain.ai) ?? ""
    }

    var provider: AiProvider {
        AiProvider(
            api: api, baseUrl: baseURL, apiKey: key, model: model,
            embeddingModel: api == "openai" ? embeddingModel.trimmingCharacters(in: .whitespaces) : "")
    }

    func choose(api: String) {
        guard api != self.api else { return }
        self.api = api
        baseURL = api == "anthropic" ? Self.anthropicAddress : ""
        model = ""
        models = []
    }

    func loadModels() async {
        isLoadingModels = true
        errorMessage = nil
        let provider = provider
        do {
            models = try await Task.detached { try aiModels(provider: provider) }.value
            if let first = models.first, !models.contains(where: { $0.id == model }) {
                model = first.id
            }
        } catch {
            errorMessage = CoreKit.message(for: error)
        }
        isLoadingModels = false
    }

    func refreshSemantic(library: PaperLibrary?) async {
        semantic = nil
        let provider = provider
        guard let library, !provider.embeddingModel.isEmpty else { return }
        semantic = try? await Task.detached { try library.aiSemanticStatus(provider: provider) }.value
    }

    /// Embeds the documents not yet indexed, a batch at a time, so the
    /// count goes up as it works.
    func buildSemanticIndex(library: PaperLibrary?) async {
        guard let library, !isIndexing else { return }
        isIndexing = true
        errorMessage = nil
        let provider = provider
        do {
            while true {
                let status = try await Task.detached { try library.aiSemanticIndex(provider: provider, limit: 64) }.value
                semantic = status
                if status.indexed >= status.total { break }
            }
        } catch {
            errorMessage = CoreKit.message(for: error)
        }
        isIndexing = false
    }

    /// The language the app shows, for the model to answer in.
    static var language: String {
        Bundle.main.preferredLocalizations.first ?? "en"
    }
}

/// An AI answer: its text, the pages it read, and copying it.
struct AIAnswerView: View {
    let answer: AiAnswer

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(Self.markdown(answer.text))
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
            if answer.cutShort {
                Text("The answer stops early: it reached the length limit.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            if !answer.pages.isEmpty {
                Text("Read pages \(Self.ranges(answer.pages))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Button("Copy", systemImage: "doc.on.doc") { Clipboard.copy(answer.text) }
                .buttonStyle(.borderless)
        }
    }

    static func markdown(_ text: String) -> AttributedString {
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        return (try? AttributedString(markdown: text, options: options)) ?? AttributedString(text)
    }

    /// `1–5, 8`.
    static func ranges(_ pages: [UInt32]) -> String {
        var parts: [String] = []
        var index = 0
        while index < pages.count {
            var end = index
            while end + 1 < pages.count, pages[end + 1] == pages[end] + 1 {
                end += 1
            }
            parts.append(end == index ? String(pages[index]) : "\(pages[index])–\(pages[end])")
            index = end + 1
        }
        return parts.joined(separator: ", ")
    }
}
