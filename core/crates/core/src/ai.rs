//! Reading with AI, through a language model the person brings a key for
//! (`paperloom-ai`): an item's summary, questions about a PDF, help with a
//! passage (translate, explain, summarize, polish), and mind maps drafted
//! from a note or a paper.
//!
//! Papers can be longer than a model reads at once. The text given is
//! capped; for a question, the pages that share the most words with it are
//! chosen. Every answer says which pages the model read.

use std::collections::BTreeSet;
use std::time::Duration;

use paperloom_ai::{self as ai, AiError, Prompt, Protocol, Provider};
use paperloom_http::{Http, UreqHttp};
use paperloom_model::{AttachmentId, ItemId};
use serde::{Deserialize, Serialize};

use crate::{CoreError, PaperLibrary};

/// How much page text a request carries, in characters: about 30 000 to
/// 60 000 tokens, within what the providers people use read at once.
const TEXT_BUDGET: usize = 100_000;
/// The longest passage to transform.
const PASSAGE_LIMIT: usize = 60_000;
const MAX_TOKENS: u32 = 8_000;

impl From<AiError> for CoreError {
    fn from(err: AiError) -> Self {
        let message = err.to_string();
        match err {
            AiError::Network(_) => CoreError::Network { message },
            _ => CoreError::Ai { message },
        }
    }
}

/// A language model provider, with the person's key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct AiProvider {
    /// The provider's API: `anthropic`, or `openai` for the OpenAI-style
    /// chat API (OpenAI, DeepSeek, Kimi, Qwen, GLM, Ollama…).
    pub api: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// Empty disables semantic retrieval.
    #[serde(default)]
    pub embedding_model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct AiModel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct AiAnswer {
    /// Markdown.
    pub text: String,
    /// The answer reached the length limit and stops early.
    pub cut_short: bool,
    /// The PDF's pages the model read (from 1); empty when it read none.
    pub pages: Vec<u32>,
}

pub(crate) fn provider(provider: AiProvider) -> Result<Provider, CoreError> {
    let protocol = Protocol::parse(&provider.api)
        .ok_or_else(|| CoreError::InvalidInput { message: format!("unknown AI API {}", provider.api) })?;
    Ok(Provider { protocol, base_url: provider.base_url, api_key: provider.api_key, model: provider.model })
}

/// Models write for minutes; lookups give up sooner.
pub(crate) fn http() -> UreqHttp {
    UreqHttp::with_timeout(Duration::from_secs(300))
}

/// The language to write in, from the app's language code.
fn language_name(code: &str) -> &str {
    match code {
        "zh-Hans" | "zh-CN" | "zh" => "Simplified Chinese",
        "zh-Hant" | "zh-TW" | "zh-HK" => "Traditional Chinese",
        "fr" => "French",
        "ja" => "Japanese",
        "ru" => "Russian",
        "en" | "" => "English",
        other => other,
    }
}

/// The models a provider offers this key.
#[uniffi::export]
pub fn ai_models(provider: AiProvider) -> Result<Vec<AiModel>, CoreError> {
    let provider = self::provider(provider)?;
    Ok(ai::models(&http(), &provider)?.into_iter().map(|m| AiModel { id: m.id, name: m.name }).collect())
}

/// A mind map drafted from a note or a passage under `title`: the text of
/// a new map note.
#[uniffi::export]
pub fn ai_mindmap(
    provider: AiProvider,
    title: String,
    text: String,
    language: String,
) -> Result<String, CoreError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(CoreError::InvalidInput { message: "There is nothing to map yet.".into() });
    }
    let user = format!("Title: {title}\n\n{}", text.chars().take(TEXT_BUDGET).collect::<String>());
    mindmap_with(&http(), &self::provider(provider)?, &title, user, &language)
}

/// What a mind map is asked to be.
fn mindmap_system(language: &str) -> String {
    format!(
        "You turn a researcher's material into a mind map. Write in {}. Reply with a Markdown outline and \
         nothing else: a first line `# ` with the central topic, then nested `- ` list items, two spaces per \
         level, at most four levels and about six main branches. Each topic is a short phrase, not a sentence; \
         keep the methods, data and numbers the material names. Say nothing the material does not support.",
        language_name(language)
    )
}

/// The model's outline as a map note: what is in a fenced block if it
/// wrote one, from its first heading, read the way the apps read maps.
fn mindmap_note(title: &str, answer: &str) -> String {
    let mut body = answer.trim();
    if let Some((_, fenced)) = body.split_once("```") {
        let fenced = fenced.split_once('\n').map_or("", |(_, rest)| rest);
        body = fenced.split("```").next().unwrap_or(fenced);
    }
    if let Some(at) = body.find("\n# ").map(|i| i + 1).or_else(|| body.starts_with("# ").then_some(0)) {
        body = &body[at..];
    }
    let map = paperloom_vault::read_map(body, title);
    paperloom_vault::write_map(&paperloom_vault::new_map(&map.text, paperloom_vault::MapStyle::Logic), &map)
}

fn mindmap_with(
    http: &dyn Http,
    provider: &Provider,
    title: &str,
    user: String,
    language: &str,
) -> Result<String, CoreError> {
    let answer = ai::complete(
        http,
        provider,
        &Prompt { system: mindmap_system(language), user, max_tokens: MAX_TOKENS },
    )?;
    Ok(mindmap_note(title, &answer.text))
}

/// Something an answer about the library drew on, by its number in the
/// answer (`[1]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct AiSource {
    pub number: u32,
    /// `item`, `annotation`, `pdf` (a page) or `note`.
    pub kind: String,
    pub title: String,
    pub item_id: Option<String>,
    pub attachment_id: Option<String>,
    /// From 0.
    pub page_index: Option<u32>,
    pub note_path: Option<String>,
}

/// An answer from the whole library, citing its sources by number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct AiLibraryAnswer {
    /// Markdown, with `[1]`-style citations.
    pub text: String,
    pub cut_short: bool,
    pub sources: Vec<AiSource>,
}

/// How many passages an answer about the library reads, and how much of
/// each.
const LIBRARY_SOURCES: usize = 12;
const SOURCE_CHARS: usize = 6_000;
/// Words the model may add to a library question's search.
const SEARCH_WORDS_CHARS: usize = 400;

/// Keywords for searching the library for a question, from the model: the
/// question's terms in its language and in English, with synonyms and
/// technical terms. Empty when the model cannot be asked; the question's
/// own words are searched anyway.
fn search_words(http: &dyn Http, provider: &Provider, question: &str) -> String {
    let system = "You turn a researcher's question into search keywords for a full-text search of their \
                  library. Answer with one line of 5 to 15 keywords or short phrases separated by spaces, in \
                  the question's language and in English, with synonyms and technical terms. No explanations."
        .to_string();
    let Ok(answer) =
        ai::complete(http, provider, &Prompt { system, user: question.to_string(), max_tokens: 200 })
    else {
        return String::new();
    };
    let line = answer.text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    line.chars().take(SEARCH_WORDS_CHARS).collect()
}

/// A mind map drafted from a paper, to keep as a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct AiMindMap {
    /// The paper's title, for the note's.
    pub title: String,
    /// The note's text.
    pub text: String,
}

/// Help with a passage: `translate` (into `language`), `explain`,
/// `summarize` or `polish` (better academic writing, same language).
#[uniffi::export]
pub fn ai_transform(
    provider: AiProvider,
    task: String,
    text: String,
    language: String,
) -> Result<AiAnswer, CoreError> {
    transform_with(&http(), &self::provider(provider)?, &task, &text, &language)
}

fn transform_with(
    http: &dyn Http,
    provider: &Provider,
    task: &str,
    text: &str,
    language: &str,
) -> Result<AiAnswer, CoreError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(CoreError::InvalidInput { message: "Select some text first.".into() });
    }
    if text.chars().count() > PASSAGE_LIMIT {
        return Err(CoreError::InvalidInput {
            message: "Select a shorter passage (up to 60,000 characters).".into(),
        });
    }
    let language = language_name(language);
    let system = match task {
        "translate" => format!(
            "Translate the text into {language}. Keep technical terms accurate and keep the formatting \
             (Markdown, formulas, citations). Reply with the translation only."
        ),
        "explain" => format!(
            "Explain the passage, in {language}, to a researcher from a neighbouring field: what it says, \
             the terms it relies on, and why it matters. Keep it short."
        ),
        "summarize" => format!("Summarize the text in {language} in a few sentences."),
        "polish" => {
            "Improve the academic writing of the text: clarity, grammar and flow. Keep its language, \
                     meaning, citations and Markdown. Reply with the improved text only."
                .to_string()
        }
        other => return Err(CoreError::InvalidInput { message: format!("unknown task {other}") }),
    };
    let answer =
        ai::complete(http, provider, &Prompt { system, user: text.to_string(), max_tokens: MAX_TOKENS })?;
    Ok(AiAnswer { text: answer.text, cut_short: answer.cut_short, pages: Vec::new() })
}

/// Words to match pages by: Latin words of three letters or more, and pairs
/// of CJK characters (which have no spaces between words).
fn terms(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let lower = text.to_lowercase();
    for word in lower.split(|c: char| !c.is_alphanumeric()) {
        let chars: Vec<char> = word.chars().collect();
        if chars.iter().any(|c| is_cjk(*c)) {
            for pair in chars.windows(2) {
                found.insert(pair.iter().collect());
            }
        } else if chars.len() >= 3 {
            found.insert(word.to_string());
        }
    }
    found
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xac00..=0xd7af | 0xf900..=0xfaff)
}

/// Pages that fit the budget: from the start, or (with a question) the
/// pages that share the most words with it, in page order.
fn choose_pages(pages: &[(u32, String)], question: Option<&str>) -> Vec<usize> {
    let mut order: Vec<usize> = (0..pages.len()).collect();
    if let Some(question) = question {
        let wanted = terms(question);
        let score = |text: &str| -> usize {
            let lower = text.to_lowercase();
            wanted.iter().map(|term| lower.matches(term.as_str()).count()).sum()
        };
        let scores: Vec<usize> = pages.iter().map(|(_, text)| score(text)).collect();
        order.sort_by_key(|&i| (std::cmp::Reverse(scores[i]), i));
    }
    let mut chosen = Vec::new();
    let mut used = 0;
    for i in order {
        let length = pages[i].1.chars().count();
        if used + length > TEXT_BUDGET && !chosen.is_empty() {
            continue;
        }
        used += length;
        chosen.push(i);
    }
    chosen.sort_unstable();
    chosen
}

fn page_text(pages: &[(u32, String)], chosen: &[usize]) -> (String, Vec<u32>) {
    let mut text = String::new();
    let mut numbers = Vec::new();
    for &i in chosen {
        let (page, body) = &pages[i];
        // A single page longer than the budget is cut at it.
        let body: String = body.chars().take(TEXT_BUDGET).collect();
        text.push_str(&format!("[p. {}]\n{}\n\n", page + 1, body.trim()));
        numbers.push(page + 1);
    }
    (text, numbers)
}

fn read_through(numbers: &[u32], total: usize) -> String {
    if numbers.len() == total {
        format!("all {total} pages")
    } else {
        let list: Vec<String> = numbers.iter().map(u32::to_string).collect();
        format!("pages {} of {total}", list.join(", "))
    }
}

#[uniffi::export]
impl PaperLibrary {
    /// A summary of an item for reading it: its question, approach,
    /// findings and limits, from its details and its PDF's text.
    pub fn ai_summarize(
        &self,
        provider: AiProvider,
        item_id: String,
        language: String,
    ) -> Result<AiAnswer, CoreError> {
        self.summarize_with(&http(), &self::provider(provider)?, &item_id, &language)
    }

    /// A mind map of a paper, from its details and its PDF's text.
    pub fn ai_mindmap_item(
        &self,
        provider: AiProvider,
        item_id: String,
        language: String,
    ) -> Result<AiMindMap, CoreError> {
        self.mindmap_item_with(&http(), &self::provider(provider)?, &item_id, &language)
    }

    /// Answers a question from the whole library: the PDF pages, notes,
    /// highlights and items matching its words or indexed meaning, cited
    /// by number.
    pub fn ai_ask_library(
        &self,
        provider: AiProvider,
        question: String,
        language: String,
    ) -> Result<AiLibraryAnswer, CoreError> {
        let embedding_model = provider.embedding_model.trim().to_string();
        self.ask_library_with(&http(), &self::provider(provider)?, &embedding_model, &question, &language)
    }

    /// Answers a question about a PDF from its text, citing pages.
    pub fn ai_ask(
        &self,
        provider: AiProvider,
        attachment_id: String,
        question: String,
        language: String,
    ) -> Result<AiAnswer, CoreError> {
        self.ask_with(&http(), &self::provider(provider)?, &attachment_id, &question, &language)
    }
}

impl PaperLibrary {
    /// What a model reads of a paper: its title, its details and as much of
    /// its PDF's text as fits, and the pages that is.
    fn item_context(&self, item_id: &str) -> Result<(String, String, Vec<u32>), CoreError> {
        let (item, pages) = {
            let store = self.lock()?;
            let item = store
                .item(&ItemId::from_string(item_id))?
                .ok_or_else(|| CoreError::not_found(format!("item {item_id}")))?;
            let mut pages = Vec::new();
            for attachment in store.attachments(&item.id)? {
                pages = store.attachment_pages(&attachment.id)?;
                if !pages.is_empty() {
                    break;
                }
            }
            (item, pages)
        };
        let mut user = format!("Title: {}\n", item.title);
        let authors: Vec<String> = item.creators.iter().map(|c| c.display_name()).collect();
        if !authors.is_empty() {
            user.push_str(&format!("Authors: {}\n", authors.join("; ")));
        }
        if let Some(year) = item.year {
            user.push_str(&format!("Year: {year}\n"));
        }
        if let Some(container) = &item.container_title {
            user.push_str(&format!("Published in: {container}\n"));
        }
        if let Some(summary) = &item.abstract_note {
            user.push_str(&format!("\nAbstract:\n{summary}\n"));
        }
        let chosen = choose_pages(&pages, None);
        let (text, numbers) = page_text(&pages, &chosen);
        if !numbers.is_empty() {
            user.push_str(&format!(
                "\nText of the paper ({}):\n\n{text}",
                read_through(&numbers, pages.len())
            ));
        } else if item.abstract_note.is_none() {
            return Err(CoreError::InvalidInput {
                message: "There is nothing to read yet: add the abstract or open the PDF once.".into(),
            });
        }
        Ok((item.title, user, numbers))
    }

    pub(crate) fn ask_library_with(
        &self,
        http: &dyn Http,
        provider: &Provider,
        embedding_model: &str,
        question: &str,
        language: &str,
    ) -> Result<AiLibraryAnswer, CoreError> {
        let question = question.trim();
        if question.is_empty() {
            return Err(CoreError::InvalidInput { message: "Ask a question first.".into() });
        }
        // The model's search words find papers written in another language
        // than the question, and under other terms.
        let words = search_words(http, provider, question);
        let mut hits =
            self.fulltext.related(&format!("{question} {words}"), &[], &[], LIBRARY_SOURCES * 2)?;
        if !embedding_model.trim().is_empty()
            && let Ok(semantic) = self.semantic_hits(http, provider, embedding_model.trim(), question)
            && !semantic.is_empty()
        {
            hits = crate::semantic::fuse(hits, semantic);
        }
        let mut sources = Vec::new();
        let mut text = String::new();
        {
            let store = self.lock()?;
            for hit in hits {
                // Leave out what is in the trash.
                if let Some(item) = &hit.item
                    && store.is_trashed(&ItemId::from_string(item.clone()))? != Some(false)
                {
                    continue;
                }
                let body = self.fulltext.body(&hit.key)?.unwrap_or_default();
                let body: String = body.chars().take(SOURCE_CHARS).collect();
                if body.trim().is_empty() || text.chars().count() > TEXT_BUDGET {
                    continue;
                }
                let number = sources.len() as u32 + 1;
                let place = hit.page.map(|p| format!(", p. {}", p + 1)).unwrap_or_default();
                text.push_str(&format!(
                    "[{number}] {} ({}{place})\n{}\n\n",
                    hit.title,
                    hit.kind.as_str(),
                    body.trim()
                ));
                sources.push(AiSource {
                    number,
                    kind: hit.kind.as_str().to_string(),
                    title: hit.title,
                    item_id: hit.item,
                    attachment_id: hit.attachment,
                    page_index: hit.page,
                    note_path: (hit.kind == paperloom_fulltext::Kind::Note).then(|| hit.key.clone()),
                });
                if sources.len() >= LIBRARY_SOURCES {
                    break;
                }
            }
        }
        if sources.is_empty() {
            return Err(CoreError::InvalidInput {
                message: "Nothing in the library speaks to this question yet.".into(),
            });
        }
        let system = format!(
            "You answer a researcher's question from their own library. Use only the numbered sources below \
             and cite them inline as [1], [2], where you use them. If they do not answer the question, say so \
             plainly. Write in {}, in Markdown.",
            language_name(language)
        );
        let user = format!("Question: {question}\n\nSources:\n\n{text}");
        let answer = ai::complete(http, provider, &Prompt { system, user, max_tokens: MAX_TOKENS })?;
        Ok(AiLibraryAnswer { text: answer.text, cut_short: answer.cut_short, sources })
    }

    pub(crate) fn mindmap_item_with(
        &self,
        http: &dyn Http,
        provider: &Provider,
        item_id: &str,
        language: &str,
    ) -> Result<AiMindMap, CoreError> {
        let (title, user, _) = self.item_context(item_id)?;
        let text = mindmap_with(http, provider, &title, user, language)?;
        Ok(AiMindMap { title, text })
    }

    pub(crate) fn summarize_with(
        &self,
        http: &dyn Http,
        provider: &Provider,
        item_id: &str,
        language: &str,
    ) -> Result<AiAnswer, CoreError> {
        let (_, user, numbers) = self.item_context(item_id)?;
        let system = format!(
            "You help a researcher read a paper. Write in {}, in Markdown, under these headings: the question, \
             the approach, the findings, the limits, and how it relates to other work. Be concrete: name the \
             methods, data and numbers the text gives. Say nothing the text does not support.",
            language_name(language)
        );
        let answer = ai::complete(http, provider, &Prompt { system, user, max_tokens: MAX_TOKENS })?;
        Ok(AiAnswer { text: answer.text, cut_short: answer.cut_short, pages: numbers })
    }

    pub(crate) fn ask_with(
        &self,
        http: &dyn Http,
        provider: &Provider,
        attachment_id: &str,
        question: &str,
        language: &str,
    ) -> Result<AiAnswer, CoreError> {
        let question = question.trim();
        if question.is_empty() {
            return Err(CoreError::InvalidInput { message: "Ask a question first.".into() });
        }
        let pages = self.lock()?.attachment_pages(&AttachmentId::from_string(attachment_id))?;
        if pages.is_empty() {
            return Err(CoreError::InvalidInput {
                message: "This PDF's text is not read yet: open it once, or it has no text layer.".into(),
            });
        }
        let chosen = choose_pages(&pages, Some(question));
        let (text, numbers) = page_text(&pages, &chosen);
        let system = format!(
            "You answer questions about a paper from its text, given below. Write in {}. Cite the pages you rely \
             on as (p. N). If the text does not answer the question, say so plainly rather than guessing.",
            language_name(language)
        );
        let user = format!(
            "Text of the paper ({}):\n\n{text}Question: {question}",
            read_through(&numbers, pages.len())
        );
        let answer = ai::complete(http, provider, &Prompt { system, user, max_tokens: MAX_TOKENS })?;
        Ok(AiAnswer { text: answer.text, cut_short: answer.cut_short, pages: numbers })
    }
}

#[cfg(test)]
mod tests;
