//! Full-text search over the library: items, annotations, PDF pages and
//! notes, in one Tantivy index. Chinese is split into words with jieba.
//!
//! The library database stays the source of truth; this index is derived
//! from it and rebuilt whenever its format changes.

mod tokenizer;

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

use tantivy::collector::{Count, TopDocs};
use tantivy::directory::MmapDirectory;
use tantivy::query::{BooleanQuery, Occur, Query, QueryParser, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::snippet::SnippetGenerator;
use tantivy::tokenizer::TextAnalyzer;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

pub use tokenizer::tokens;

const TOKENIZER: &str = "paperloom_words";
/// Bump when the schema or the tokenizer changes; an older index is rebuilt.
const FORMAT: &str = "1";
const FORMAT_FILE: &str = "paperloom-index-format";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("search index: {0}")]
    Index(#[from] tantivy::TantivyError),
    #[error("search index: {0}")]
    Io(#[from] std::io::Error),
    #[error("search index: {0}")]
    Directory(#[from] tantivy::directory::error::OpenDirectoryError),
    #[error("search index: {0}")]
    Lock(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A snippet on one line, as lists show it: an item's text has a field
/// per line (authors, journal, abstract…), set apart with " · "; other
/// line breaks become spaces.
fn one_line(snippet: &str, kind: Kind) -> String {
    let separator = if kind == Kind::Item { " · " } else { " " };
    snippet.split('\n').map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join(separator)
}

/// What a document is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Item,
    Annotation,
    /// One page of a PDF's text.
    Pdf,
    Note,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Item, Kind::Annotation, Kind::Pdf, Kind::Note];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Item => "item",
            Kind::Annotation => "annotation",
            Kind::Pdf => "pdf",
            Kind::Note => "note",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == value)
    }
}

/// Something to make findable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub kind: Kind,
    /// Unique among all documents: the item or annotation ID,
    /// `<attachment id>/<page>`, or the note's path.
    pub key: String,
    /// The item it belongs to.
    pub item: Option<String>,
    /// The attachment it belongs to.
    pub attachment: Option<String>,
    /// From 0.
    pub page: Option<u32>,
    pub title: String,
    pub body: String,
}

/// A match.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub kind: Kind,
    pub key: String,
    pub item: Option<String>,
    pub attachment: Option<String>,
    pub page: Option<u32>,
    pub title: String,
    /// The best passage, as HTML with the matched words in `<b>`.
    pub snippet: String,
    pub score: f32,
}

#[derive(Clone, Copy)]
struct Fields {
    kind: Field,
    key: Field,
    item: Field,
    attachment: Field,
    page: Field,
    title: Field,
    body: Field,
}

pub struct FullText {
    index: Index,
    reader: IndexReader,
    writer: Mutex<IndexWriter>,
    fields: Fields,
}

fn schema() -> (Schema, Fields) {
    let mut builder = Schema::builder();
    let text = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(TOKENIZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    let fields = Fields {
        kind: builder.add_text_field("kind", STRING | STORED),
        key: builder.add_text_field("key", STRING | STORED),
        item: builder.add_text_field("item", STRING | STORED),
        attachment: builder.add_text_field("attachment", STRING | STORED),
        page: builder.add_u64_field("page", STORED),
        title: builder.add_text_field("title", text.clone()),
        body: builder.add_text_field("body", text),
    };
    (builder.build(), fields)
}

impl FullText {
    /// Opens the index in a folder. Returns true as well when the index is
    /// new or was rebuilt (an older format, or unreadable), so the caller
    /// fills it.
    pub fn open(folder: &Path) -> Result<(Self, bool)> {
        fs::create_dir_all(folder)?;
        let marker = folder.join(FORMAT_FILE);
        let current = fs::read_to_string(&marker).is_ok_and(|f| f.trim() == FORMAT);
        if current {
            let (schema, fields) = schema();
            if let Ok(index) = MmapDirectory::open(folder)
                .map_err(Error::from)
                .and_then(|dir| Index::open_or_create(dir, schema).map_err(Error::from))
            {
                return Ok((Self::with_index(index, fields)?, false));
            }
        }
        // Start over.
        fs::remove_dir_all(folder)?;
        fs::create_dir_all(folder)?;
        let (schema, fields) = schema();
        let index = Index::create_in_dir(folder, schema)?;
        fs::write(&marker, FORMAT)?;
        Ok((Self::with_index(index, fields)?, true))
    }

    /// An index kept only in memory.
    pub fn in_memory() -> Result<Self> {
        let (schema, fields) = schema();
        Self::with_index(Index::create_in_ram(schema), fields)
    }

    fn with_index(index: Index, fields: Fields) -> Result<Self> {
        index.tokenizers().register(TOKENIZER, TextAnalyzer::builder(tokenizer::WordTokenizer).build());
        let writer = index.writer_with_num_threads(1, 20_000_000)?;
        let reader = index.reader_builder().reload_policy(ReloadPolicy::Manual).try_into()?;
        Ok(Self { index, reader, writer: Mutex::new(writer), fields })
    }

    fn write(&self, change: impl FnOnce(&mut IndexWriter, &Fields) -> Result<()>) -> Result<()> {
        let mut writer = self.writer.lock().map_err(|e| Error::Lock(e.to_string()))?;
        change(&mut writer, &self.fields)?;
        writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    /// Adds documents, replacing any with the same keys.
    pub fn put(&self, documents: &[Document]) -> Result<()> {
        if documents.is_empty() {
            return Ok(());
        }
        self.write(|writer, f| {
            for document in documents {
                writer.delete_term(Term::from_field_text(f.key, &document.key));
                let mut doc = TantivyDocument::default();
                doc.add_text(f.kind, document.kind.as_str());
                doc.add_text(f.key, &document.key);
                if let Some(item) = &document.item {
                    doc.add_text(f.item, item);
                }
                if let Some(attachment) = &document.attachment {
                    doc.add_text(f.attachment, attachment);
                }
                if let Some(page) = document.page {
                    doc.add_u64(f.page, u64::from(page));
                }
                doc.add_text(f.title, &document.title);
                doc.add_text(f.body, &document.body);
                writer.add_document(doc)?;
            }
            Ok(())
        })
    }

    pub fn remove_keys(&self, keys: &[String]) -> Result<()> {
        self.write(|writer, f| {
            for key in keys {
                writer.delete_term(Term::from_field_text(f.key, key));
            }
            Ok(())
        })
    }

    /// Removes an item and everything belonging to it.
    pub fn remove_item(&self, item: &str) -> Result<()> {
        self.write(|writer, f| {
            writer.delete_term(Term::from_field_text(f.key, item));
            writer.delete_term(Term::from_field_text(f.item, item));
            Ok(())
        })
    }

    /// Removes an attachment's pages and annotations.
    pub fn remove_attachment(&self, attachment: &str) -> Result<()> {
        self.write(|writer, f| {
            writer.delete_term(Term::from_field_text(f.attachment, attachment));
            Ok(())
        })
    }

    /// Removes every document of a kind (before re-adding all notes).
    pub fn remove_kind(&self, kind: Kind) -> Result<()> {
        self.write(|writer, f| {
            writer.delete_term(Term::from_field_text(f.kind, kind.as_str()));
            Ok(())
        })
    }

    pub fn clear(&self) -> Result<()> {
        self.write(|writer, _| {
            writer.delete_all_documents()?;
            Ok(())
        })
    }

    pub fn len(&self) -> u64 {
        self.reader.searcher().num_docs()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every live document, in key order, including its stored text.
    pub fn documents(&self) -> Result<Vec<Document>> {
        let searcher = self.reader.searcher();
        let f = self.fields;
        let mut documents = Vec::new();
        for (ordinal, segment) in searcher.segment_readers().iter().enumerate() {
            for id in segment.doc_ids_alive() {
                let doc: TantivyDocument = searcher.doc(tantivy::DocAddress::new(ordinal as u32, id))?;
                let text = |field| doc.get_first(field).and_then(|v| v.as_str()).map(str::to_string);
                let Some(kind) = text(f.kind).as_deref().and_then(Kind::parse) else { continue };
                documents.push(Document {
                    kind,
                    key: text(f.key).unwrap_or_default(),
                    item: text(f.item),
                    attachment: text(f.attachment),
                    page: doc.get_first(f.page).and_then(|v| v.as_u64()).map(|p| p as u32),
                    title: text(f.title).unwrap_or_default(),
                    body: text(f.body).unwrap_or_default(),
                });
            }
        }
        documents.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(documents)
    }

    /// A document's source metadata, by its key.
    pub fn hit(&self, key: &str) -> Result<Option<Hit>> {
        Ok(self.hits(&*term_query(self.fields.key, key), 1)?.into_iter().next())
    }

    /// Whether any PDF page of an attachment is indexed.
    pub fn has_pages(&self, attachment: &str) -> Result<bool> {
        let searcher = self.reader.searcher();
        let query = BooleanQuery::new(vec![
            (Occur::Must, term_query(self.fields.attachment, attachment)),
            (Occur::Must, term_query(self.fields.kind, Kind::Pdf.as_str())),
        ]);
        Ok(searcher.search(&query, &Count)? > 0)
    }

    /// The best matches for words (all of them must appear; `"a phrase"`,
    /// `-word` and `OR` work too), optionally only of some kinds.
    pub fn search(&self, text: &str, kinds: &[Kind], limit: usize) -> Result<Vec<Hit>> {
        let f = self.fields;
        if text.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let mut parser = QueryParser::for_index(&self.index, vec![f.title, f.body]);
        parser.set_conjunction_by_default();
        parser.set_field_boost(f.title, 2.0);
        let (words, _) = parser.parse_query_lenient(text);
        let query: Box<dyn Query> = if kinds.is_empty() {
            words
        } else {
            let any_kind: Vec<(Occur, Box<dyn Query>)> =
                kinds.iter().map(|k| (Occur::Should, term_query(f.kind, k.as_str()))).collect();
            Box::new(BooleanQuery::new(vec![
                (Occur::Must, words),
                (Occur::Must, Box::new(BooleanQuery::new(any_kind))),
            ]))
        };
        self.hits(&*query, limit)
    }

    /// Documents about what a text is about: its most telling words
    /// (frequent in it, rare in the index), any of them, best match first;
    /// documents with a `skip` key left out.
    pub fn related(&self, text: &str, kinds: &[Kind], skip: &[String], limit: usize) -> Result<Vec<Hit>> {
        let f = self.fields;
        let mut counts: HashMap<String, u32> = HashMap::new();
        for token in tokens(text) {
            let word = token.text;
            let shortest = if word.is_ascii() { 3 } else { 2 };
            if word.chars().count() < shortest
                || word.chars().all(|c| c.is_ascii_digit())
                || STOP_WORDS.contains(&word.as_str())
            {
                continue;
            }
            *counts.entry(word).or_default() += 1;
        }
        let searcher = self.reader.searcher();
        let documents = searcher.num_docs().max(1) as f64;
        let mut weighted: Vec<(f64, String)> = Vec::new();
        for (word, count) in counts {
            let found_in = searcher.doc_freq(&Term::from_field_text(f.body, &word))?;
            if found_in > 0 {
                weighted.push((f64::from(count).sqrt() * (documents / found_in as f64).ln_1p(), word));
            }
        }
        weighted.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        weighted.truncate(RELATED_WORDS);
        if weighted.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let words: Vec<(Occur, Box<dyn Query>)> = weighted
            .iter()
            .flat_map(|(_, word)| [f.title, f.body].map(|field| (Occur::Should, word_query(field, word))))
            .collect();
        let any_word: Box<dyn Query> = Box::new(BooleanQuery::new(words));
        let mut clauses = vec![(Occur::Must, any_word)];
        if !kinds.is_empty() {
            let any_kind = kinds.iter().map(|k| (Occur::Should, term_query(f.kind, k.as_str()))).collect();
            clauses.push((Occur::Must, Box::new(BooleanQuery::new(any_kind))));
        }
        clauses.extend(skip.iter().map(|key| (Occur::MustNot, term_query(f.key, key))));
        self.hits(&BooleanQuery::new(clauses), limit)
    }

    /// The text a document was indexed with (its body), by its key.
    pub fn body(&self, key: &str) -> Result<Option<String>> {
        let f = self.fields;
        let searcher = self.reader.searcher();
        let query = term_query(f.key, key);
        let top = searcher.search(&*query, &TopDocs::with_limit(1))?;
        let Some((_, address)) = top.into_iter().next() else { return Ok(None) };
        let doc: TantivyDocument = searcher.doc(address)?;
        Ok(doc.get_first(f.body).and_then(|v| v.as_str()).map(str::to_string))
    }

    /// The best `limit` documents for a query, with snippets.
    fn hits(&self, query: &dyn Query, limit: usize) -> Result<Vec<Hit>> {
        let f = self.fields;
        let searcher = self.reader.searcher();
        let top = searcher.search(query, &TopDocs::with_limit(limit))?;
        let mut snippets = SnippetGenerator::create(&searcher, query, f.body)?;
        snippets.set_max_num_chars(180);
        let mut hits = Vec::with_capacity(top.len());
        for (score, address) in top {
            let doc: TantivyDocument = searcher.doc(address)?;
            let text_of = |field: Field| doc.get_first(field).and_then(|v| v.as_str()).map(str::to_string);
            let Some(kind) = text_of(f.kind).as_deref().and_then(Kind::parse) else { continue };
            let mut snippet = snippets.snippet_from_doc(&doc).to_html();
            if snippet.is_empty() {
                let body = text_of(f.body).unwrap_or_default();
                snippet = escape(&body.chars().take(180).collect::<String>());
            }
            let snippet = one_line(&snippet, kind);
            hits.push(Hit {
                kind,
                key: text_of(f.key).unwrap_or_default(),
                item: text_of(f.item),
                attachment: text_of(f.attachment),
                page: doc.get_first(f.page).and_then(|v| v.as_u64()).map(|p| p as u32),
                title: text_of(f.title).unwrap_or_default(),
                snippet,
                score,
            });
        }
        Ok(hits)
    }
}

/// How many of a text's words look for related documents.
const RELATED_WORDS: usize = 24;

/// Words too common to tell texts apart.
const STOP_WORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "are", "was", "were", "from", "have", "has", "had", "not",
    "but", "you", "all", "can", "our", "which", "their", "its", "into", "also", "been", "more", "than",
    "these", "they", "will", "about", "when", "there", "would", "what", "such", "only", "other", "may",
    "some", "one", "two", "each", "how", "out", "use", "used", "using", "then", "them", "who", "your",
    "http", "https", "www", "com", "org", "我们", "一个", "可以", "这个", "进行", "以及", "因为", "所以",
    "但是", "如果", "没有", "已经", "通过", "其中", "对于", "这些", "那些", "就是", "还是", "或者",
];

/// A word in a text field, scored by how often it appears.
fn word_query(field: Field, word: &str) -> Box<dyn Query> {
    Box::new(TermQuery::new(Term::from_field_text(field, word), IndexRecordOption::WithFreqs))
}

fn term_query(field: Field, value: &str) -> Box<dyn Query> {
    Box::new(TermQuery::new(Term::from_field_text(field, value), IndexRecordOption::Basic))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests;
