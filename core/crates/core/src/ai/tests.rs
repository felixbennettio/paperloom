use std::sync::Mutex;

use paperloom_http::{BytesResponse, NetworkError, Request, Response};
use serde_json::{Value, json};

use super::*;
use crate::NewItem;

/// Answers every request with one reply (search keywords with their own,
/// when given) and keeps the request bodies.
struct Model {
    reply: String,
    keywords: Option<String>,
    embedding_failure_after: Option<usize>,
    asked: Mutex<Vec<Value>>,
}

impl Model {
    fn new(reply: &str) -> Self {
        Self {
            reply: reply.into(),
            keywords: None,
            embedding_failure_after: None,
            asked: Mutex::new(Vec::new()),
        }
    }

    fn with_keywords(keywords: &str, reply: &str) -> Self {
        Self { keywords: Some(keywords.into()), ..Self::new(reply) }
    }

    fn last_prompt(&self) -> String {
        self.asked.lock().unwrap().last().unwrap()["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn embeddings(&self) -> Vec<Value> {
        self.asked.lock().unwrap().iter().filter(|r| r.get("input").is_some()).cloned().collect()
    }
}

impl Http for Model {
    fn get(&self, _url: &str, _accept: &str) -> Result<Response, NetworkError> {
        Err(NetworkError("not used".into()))
    }

    fn post_form(&self, _url: &str, _form: &[(&str, &str)]) -> Result<Response, NetworkError> {
        Err(NetworkError("not used".into()))
    }

    fn send(&self, request: Request) -> Result<BytesResponse, NetworkError> {
        let asked: Value = serde_json::from_slice(request.body.as_deref().unwrap()).unwrap();
        let wants_keywords = asked["system"]
            .as_str()
            .or_else(|| asked["messages"][0]["content"].as_str())
            .is_some_and(|s| s.contains("search keywords"));
        let mut seen = self.asked.lock().unwrap();
        let embeddings = seen.iter().filter(|r| r.get("input").is_some()).count();
        seen.push(asked.clone());
        drop(seen);
        if request.url.ends_with("/embeddings") {
            if self.embedding_failure_after.is_some_and(|n| embeddings >= n) {
                return Err(NetworkError("offline".into()));
            }
            let data: Vec<Value> = asked["input"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, text)| {
                    let text = text.as_str().unwrap().to_lowercase();
                    let thermal = ["cell", "temperature", "nanodiamond", "thermometry"]
                        .iter()
                        .any(|term| text.contains(term));
                    let vector = if thermal { [3.0, 0.0] } else { [0.0, 2.0] };
                    json!({ "index": i, "embedding": vector })
                })
                .collect();
            return Ok(BytesResponse { status: 200, body: json!({ "data": data }).to_string().into_bytes() });
        }
        let text = match &self.keywords {
            Some(keywords) if wants_keywords => keywords,
            _ => &self.reply,
        };
        let body = if request.url.ends_with("/chat/completions") {
            json!({ "choices": [{ "message": { "content": text }, "finish_reason": "stop" }] })
        } else {
            json!({ "content": [{ "type": "text", "text": text }], "stop_reason": "end_turn" })
        };
        Ok(BytesResponse { status: 200, body: body.to_string().into_bytes() })
    }
}

fn anthropic() -> Provider {
    Provider {
        protocol: Protocol::Anthropic,
        base_url: "https://api.anthropic.com".into(),
        api_key: "key".into(),
        model: "a-model".into(),
    }
}

/// A library with one paper whose PDF has these pages.
struct Paper {
    _dir: tempfile::TempDir,
    library: std::sync::Arc<PaperLibrary>,
    item: String,
    file: String,
}

fn paper_with_pages(pages: Vec<String>) -> Paper {
    let dir = tempfile::tempdir().unwrap();
    let library = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
    let item = library
        .add_item(NewItem {
            item_type: String::new(),
            title: "Nanometre-scale thermometry in a living cell".into(),
            authors: vec!["Georg Kucsko".into()],
            container_title: Some("Nature".into()),
            year: Some(2013),
            doi: None,
        })
        .unwrap();
    let pdf = dir.path().join("paper.pdf");
    std::fs::write(&pdf, b"%PDF-1.7").unwrap();
    let file = library.attach_file(item.id.clone(), pdf.display().to_string()).unwrap();
    library.set_attachment_text(file.id.clone(), pages).unwrap();
    Paper { _dir: dir, library, item: item.id, file: file.id }
}

#[test]
fn summarizes_from_the_details_and_the_text() {
    let paper = paper_with_pages(vec!["Introduction".into(), "Results".into()]);
    let model = Model::new("## The question");
    let answer = paper.library.summarize_with(&model, &anthropic(), &paper.item, "zh-Hans").unwrap();
    assert_eq!(answer.pages, [1, 2]);
    let prompt = model.last_prompt();
    assert!(
        prompt.contains("Title: Nanometre-scale thermometry") && prompt.contains("Year: 2013"),
        "{prompt}"
    );
    assert!(prompt.contains("(all 2 pages)") && prompt.contains("[p. 2]\nResults"), "{prompt}");
    let system = model.asked.lock().unwrap()[0]["system"].as_str().unwrap().to_string();
    assert!(system.contains("Simplified Chinese"), "{system}");
}

#[test]
fn asks_with_the_pages_that_match_the_question() {
    // Three long pages: only one fits, the one about the question.
    let pages = vec![
        "alpha ".repeat(10_000),
        "beta ".repeat(12_000),
        format!("{}thermometry", "gamma ".repeat(10_000)),
    ];
    let paper = paper_with_pages(pages);
    let model = Model::new("On page 3 (p. 3).");
    let answer = paper
        .library
        .ask_with(&model, &anthropic(), &paper.file, "How precise is the thermometry?", "en")
        .unwrap();
    assert_eq!(answer.pages, [3]);
    assert!(model.last_prompt().contains("(pages 3 of 3)"));
    assert!(model.last_prompt().ends_with("Question: How precise is the thermometry?"));
    assert!(paper.library.ask_with(&model, &anthropic(), &paper.file, "  ", "en").is_err());
}

#[test]
fn helps_with_a_passage() {
    let model = Model::new("Translated.");
    let answer = transform_with(&model, &anthropic(), "translate", "Bonjour", "ja").unwrap();
    assert_eq!((answer.text.as_str(), answer.pages.len()), ("Translated.", 0));
    assert!(model.asked.lock().unwrap()[0]["system"].as_str().unwrap().contains("Japanese"));
    assert!(transform_with(&model, &anthropic(), "sing", "x", "en").is_err());
    assert!(transform_with(&model, &anthropic(), "polish", " ", "en").is_err());
    assert_eq!(
        terms("纳米温度计 and THE quantum"),
        ["and", "quantum", "the", "温度", "度计", "米温", "纳米"].map(String::from).into()
    );
}

#[test]
fn drafts_mind_maps() {
    let model = Model::new(
        "Here is the map:\n\n```markdown\n# Thermometry\n\n- Methods\n  - NV centres\n- Results\n```\n\nHope this helps!",
    );
    let note = mindmap_with(&model, &anthropic(), "Ignored", "Title: x\n\ntext".into(), "en").unwrap();
    assert!(note.starts_with("---\nmindmap: logic\n---\n# Thermometry\n"), "{note}");
    assert!(note.ends_with("\n- Methods\n  - NV centres\n- Results\n"), "{note}");
    let system = model.asked.lock().unwrap()[0]["system"].as_str().unwrap().to_string();
    assert!(system.contains("Markdown outline") && system.contains("English"), "{system}");
    // Without a fence, from the first heading.
    assert!(mindmap_note("T", "Sure!\n# Centre\n- A").ends_with("# Centre\n\n- A\n"));
    assert!(mindmap_note("Fallback", "- A\n- B").contains("# Fallback\n"));

    let paper = paper_with_pages(vec!["Introduction".into()]);
    let drafted = paper.library.mindmap_item_with(&model, &anthropic(), &paper.item, "fr").unwrap();
    assert_eq!(drafted.title, "Nanometre-scale thermometry in a living cell");
    assert!(model.last_prompt().contains("[p. 1]\nIntroduction"), "{}", model.last_prompt());
}

#[test]
fn answers_from_the_whole_library_with_sources() {
    let paper = paper_with_pages(vec![
        "We measured temperature inside living cells with nitrogen-vacancy centres.".into(),
        "Acknowledgements".into(),
    ]);
    let model = Model::with_keywords("", "Nanodiamonds report temperature [1].");
    let answer = paper
        .library
        .ask_library_with(&model, &anthropic(), "", "How is temperature measured in living cells?", "en")
        .unwrap();
    assert_eq!(answer.text, "Nanodiamonds report temperature [1].");
    assert!(!answer.sources.is_empty());
    assert_eq!(answer.sources[0].number, 1);
    let prompt = model.last_prompt();
    assert!(prompt.starts_with("Question: How is temperature measured"), "{prompt}");
    assert!(prompt.contains("[1] ") && prompt.contains("nitrogen-vacancy"), "{prompt}");
    assert!(paper.library.ask_library_with(&model, &anthropic(), "", "  ", "en").is_err());
    assert!(
        paper.library.ask_library_with(&model, &anthropic(), "", "zebrafish xylophone quasar", "en").is_err()
    );
}

#[test]
fn the_model_s_keywords_find_papers_in_another_language() {
    let paper = paper_with_pages(vec![
        "We measured temperature inside living cells with nitrogen-vacancy centres.".into(),
    ]);
    // A Chinese question shares no words with the English paper.
    let question = "怎样测量活细胞内的温度？";
    let silent = Model::with_keywords("", "unused");
    assert!(paper.library.ask_library_with(&silent, &anthropic(), "", question, "zh-Hans").is_err());
    let model = Model::with_keywords(
        "温度 测量 temperature thermometry living cells\nmore",
        "用纳米金刚石测温 [1]。",
    );
    let answer = paper.library.ask_library_with(&model, &anthropic(), "", question, "zh-Hans").unwrap();
    assert!(!answer.sources.is_empty());
    let asked = model.asked.lock().unwrap();
    assert_eq!(asked.len(), 2, "the keywords first, then the answer");
    assert_eq!(asked[0]["messages"][0]["content"], question);
}

fn semantic_provider() -> AiProvider {
    AiProvider {
        api: "openai".into(),
        base_url: "https://example.org/v1".into(),
        api_key: String::new(),
        model: "chat".into(),
        embedding_model: "vectors".into(),
    }
}

fn note(key: &str, title: &str, body: &str) -> paperloom_fulltext::Document {
    paperloom_fulltext::Document {
        key: key.into(),
        kind: paperloom_fulltext::Kind::Note,
        title: title.into(),
        body: body.into(),
        item: None,
        attachment: None,
        page: None,
    }
}

fn semantic_library(count: usize) -> std::sync::Arc<PaperLibrary> {
    let library = PaperLibrary::in_memory().unwrap();
    let documents: Vec<_> = (0..count)
        .map(|i| note(&format!("{i:03}.md"), "Crystals", "Lattice geometry and diffraction"))
        .collect();
    library.fulltext.put(&documents).unwrap();
    library
}

#[test]
fn semantic_index_counts_limits_and_resumes_in_batches() {
    let library = semantic_library(70);
    let model = Model::new("");
    let settings = semantic_provider();
    assert_eq!(
        library.ai_semantic_status(settings.clone()).unwrap(),
        crate::AiSemanticStatus { model: "vectors".into(), indexed: 0, total: 70 }
    );
    assert!(model.embeddings().is_empty());
    assert_eq!(library.semantic_index_with(&model, settings.clone(), 0).unwrap().indexed, 0);
    assert_eq!(library.semantic_index_with(&model, settings.clone(), 10).unwrap().indexed, 10);
    assert_eq!(library.semantic_index_with(&model, settings.clone(), 40).unwrap().indexed, 50);
    assert_eq!(library.semantic_index_with(&model, settings.clone(), 100).unwrap().indexed, 70);
    assert_eq!(library.semantic_index_with(&model, settings, 100).unwrap().indexed, 70);
    let batches: Vec<usize> =
        model.embeddings().iter().map(|r| r["input"].as_array().unwrap().len()).collect();
    assert_eq!(batches, [10, 32, 8, 20]);
    let vectors = library.vectors.lock().unwrap();
    let (dimensions, bytes): (usize, Vec<u8>) = vectors
        .as_ref()
        .unwrap()
        .query_row("SELECT dimensions, vector FROM vectors LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(dimensions, 2);
    assert_eq!(bytes, [0.0_f32, 1.0].into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>());
}

#[test]
fn semantic_index_keeps_completed_batches_after_a_failure() {
    let library = semantic_library(65);
    let failing = Model { embedding_failure_after: Some(1), ..Model::new("") };
    let settings = semantic_provider();
    assert!(library.semantic_index_with(&failing, settings.clone(), 65).is_err());
    assert_eq!(library.ai_semantic_status(settings.clone()).unwrap().indexed, 32);
    let model = Model::new("");
    assert_eq!(library.semantic_index_with(&model, settings, 65).unwrap().indexed, 65);
    let batches: Vec<usize> =
        model.embeddings().iter().map(|r| r["input"].as_array().unwrap().len()).collect();
    assert_eq!(batches, [32, 1]);
}

#[test]
fn semantic_vectors_become_stale_and_deleted_documents_are_removed() {
    let library = semantic_library(3);
    let model = Model::new("");
    let settings = semantic_provider();
    library.semantic_index_with(&model, settings.clone(), 3).unwrap();
    library.fulltext.put(&[note("000.md", "Changed title", "Changed body")]).unwrap();
    assert_eq!(library.ai_semantic_status(settings.clone()).unwrap().indexed, 2);
    assert_eq!(library.semantic_index_with(&model, settings.clone(), 1).unwrap().indexed, 3);
    let changed = AiProvider { embedding_model: "other".into(), ..settings.clone() };
    assert_eq!(library.ai_semantic_status(changed.clone()).unwrap().indexed, 0);
    assert_eq!(library.semantic_index_with(&model, changed, 1).unwrap().indexed, 1);
    assert_eq!(library.ai_semantic_status(settings.clone()).unwrap().indexed, 2);
    library.fulltext.remove_keys(&["002.md".into()]).unwrap();
    let status = library.semantic_index_with(&model, settings, 0).unwrap();
    assert_eq!((status.indexed, status.total), (1, 2));
    let vectors = library.vectors.lock().unwrap();
    let keys: String = vectors
        .as_ref()
        .unwrap()
        .query_row("SELECT group_concat(key, ',') FROM vectors", [], |r| r.get(0))
        .unwrap();
    assert!(!keys.contains("002.md"), "{keys}");
}

#[test]
fn semantic_index_caps_text_by_characters_and_digests_the_embedded_text() {
    let library = semantic_library(0);
    let body = "界".repeat(2_500);
    library.fulltext.put(&[note("long.md", "Title", &body)]).unwrap();
    let model = Model::new("");
    let settings = semantic_provider();
    library.semantic_index_with(&model, settings.clone(), 1).unwrap();
    let requests = model.embeddings();
    let text = requests[0]["input"][0].as_str().unwrap();
    assert!(text.starts_with("Title\n"));
    assert_eq!(text.chars().count(), 2_000);
    library.fulltext.put(&[note("long.md", "Title", &format!("{body} new ending"))]).unwrap();
    assert_eq!(library.ai_semantic_status(settings.clone()).unwrap().indexed, 1);
    library.fulltext.put(&[note("long.md", "Changed title", &body)]).unwrap();
    assert_eq!(library.ai_semantic_status(settings).unwrap().indexed, 0);
}

#[test]
fn vectors_find_passages_without_shared_words() {
    let library = semantic_library(30);
    library
        .fulltext
        .put(&[note("thermal.md", "Nanodiamond", "Thermometry with nitrogen vacancies")])
        .unwrap();
    let settings = semantic_provider();
    let provider = provider(settings.clone()).unwrap();
    let model = Model::with_keywords("", "Found [1].");
    let question = "cell temperature";
    assert!(library.fulltext.related(question, &[], &[], 24).unwrap().is_empty());
    library.semantic_index_with(&model, settings.clone(), 100).unwrap();
    let answer =
        library.ask_library_with(&model, &provider, &settings.embedding_model, question, "en").unwrap();
    assert_eq!(answer.sources[0].note_path.as_deref(), Some("thermal.md"));
    assert_eq!(answer.sources[0].title, "Nanodiamond");
    assert!(model.last_prompt().contains("Thermometry with nitrogen vacancies"));
    assert_eq!(model.embeddings().last().unwrap()["input"], json!([question]));
}

#[test]
fn embedding_failures_fall_back_and_unset_embeddings_make_no_requests() {
    let library = semantic_library(1);
    let settings = semantic_provider();
    let provider = provider(settings.clone()).unwrap();
    let model = Model::with_keywords("", "Found [1].");
    // Having a setting alone does not index anything or embed the question.
    library.ask_library_with(&model, &provider, "vectors", "lattice geometry", "en").unwrap();
    assert!(model.embeddings().is_empty());
    assert_eq!(model.asked.lock().unwrap().len(), 2);
    library.semantic_index_with(&model, settings, 1).unwrap();
    let failing = Model { embedding_failure_after: Some(0), ..Model::with_keywords("", "Found [1].") };
    let answer = library.ask_library_with(&failing, &provider, "vectors", "lattice geometry", "en").unwrap();
    assert_eq!(answer.sources.len(), 1);
    assert_eq!(failing.embeddings().len(), 1);
    let off = Model::with_keywords("", "Found [1].");
    library.ask_library_with(&off, &provider, "", "lattice geometry", "en").unwrap();
    assert!(off.embeddings().is_empty());
    assert_eq!(off.asked.lock().unwrap().len(), 2);
    let fresh = semantic_library(1);
    fresh.ask_library_with(&off, &provider, "", "lattice geometry", "en").unwrap();
    assert!(fresh.vectors.lock().unwrap().is_none());
}

#[test]
fn semantic_setup_is_required_and_old_settings_deserialize() {
    let library = semantic_library(1);
    let model = Model::new("");
    let old: AiProvider = serde_json::from_value(json!({
        "api": "openai", "baseUrl": "https://example.org/v1", "apiKey": "", "model": "chat"
    }))
    .unwrap();
    assert!(old.embedding_model.is_empty());
    assert!(library.ai_semantic_status(old.clone()).is_err());
    assert!(library.semantic_index_with(&model, old, 1).is_err());
    let unsupported = AiProvider { api: "anthropic".into(), ..semantic_provider() };
    assert!(library.ai_semantic_status(unsupported.clone()).is_err());
    assert!(library.semantic_index_with(&model, unsupported, 1).is_err());
    assert!(library.vectors.lock().unwrap().is_none());
    assert!(model.asked.lock().unwrap().is_empty());
}

#[test]
fn semantic_index_is_lazy_and_survives_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.sqlite").display().to_string();
    let library = PaperLibrary::new(path.clone()).unwrap();
    assert!(!dir.path().join("vectors.sqlite").exists());
    library.fulltext.put(&[note("saved.md", "Crystals", "Lattice geometry")]).unwrap();
    library.semantic_index_with(&Model::new(""), semantic_provider(), 1).unwrap();
    assert!(dir.path().join("vectors.sqlite").exists());
    drop(library);
    let reopened = PaperLibrary::new(path).unwrap();
    let status = reopened.ai_semantic_status(semantic_provider()).unwrap();
    assert_eq!((status.indexed, status.total), (1, 1));
}

#[test]
fn reciprocal_rank_fusion_rewards_documents_in_both_lists() {
    let library = semantic_library(3);
    let hit = |key| library.fulltext.hit(key).unwrap().unwrap();
    let hits = crate::semantic::fuse(vec![hit("000.md"), hit("001.md")], vec![hit("001.md"), hit("002.md")]);
    assert_eq!(hits.iter().map(|h| h.key.as_str()).collect::<Vec<_>>(), ["001.md", "000.md", "002.md"]);
}
