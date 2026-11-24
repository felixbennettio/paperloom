//! EndNote-style online search. First the open scholarly databases, which
//! need no account or key:
//!
//! - Crossref: every DOI Crossref registered (`/works?query.bibliographic=`).
//! - OpenAlex: works with abstracts, open-access links and citation counts.
//! - arXiv: preprints (Atom feed of `export.arxiv.org/api/query`).
//! - PubMed: biomedicine (E-utilities `esearch`, then `esummary`).
//! - Semantic Scholar: works across fields (Graph API paper search).
//! - Library catalogues over SRU, for books above all: the Library of
//!   Congress and K10plus (German libraries), and any catalogue the person
//!   adds ([`Catalogue`]).
//!
//! Then the subscription databases, with the person's own API key:
//!
//! - Web of Science (Starter API, `TS=` topic search).
//! - Scopus (Search API, `TITLE-ABS-KEY`).
//! - ScienceDirect (Search API v2).
//! - IEEE Xplore (Metadata API).
//! - Springer Nature (Meta API).
//!
//! Each answer becomes [`Item`]s, ready to add to the library.

mod arxiv;
mod crossref;
mod feed;
mod ieee;
mod marc;
mod openalex;
mod pubmed;
mod sciencedirect;
mod scopus;
mod semantic_scholar;
mod springer;
mod sru;
mod wos;
mod xml;
mod z3950;

use paperloom_http::{Http, NetworkError, Request, UreqHttp};
use paperloom_model::Item;
use serde_json::Value;

pub use feed::{Feed, FeedEntry, read_feed};
pub use sru::{Catalogue, catalogue_presets};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(String),
    #[error("{0} is busy; try again in a minute")]
    RateLimited(&'static str),
    #[error("{service} answered {status}")]
    Http { service: &'static str, status: u16 },
    #[error("unexpected reply from {service}: {detail}")]
    Parse { service: &'static str, detail: String },
    #[error("Add your {0} API key in Settings to search it.")]
    KeyNeeded(&'static str),
    #[error("{0} did not accept the API key.")]
    KeyRejected(&'static str),
    /// A library catalogue's own answer: an SRU diagnostic or an HTTP status.
    #[error("Catalogue {name}: {detail}")]
    Catalogue { name: String, detail: String },
}

impl From<NetworkError> for Error {
    fn from(err: NetworkError) -> Self {
        Error::Network(err.0)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// A database to search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Crossref,
    OpenAlex,
    Arxiv,
    PubMed,
    SemanticScholar,
    LibraryOfCongress,
    K10plus,
    WebOfScience,
    Scopus,
    ScienceDirect,
    Ieee,
    Springer,
}

impl Source {
    /// The open databases first, then those that need a key.
    pub const ALL: [Source; 12] = [
        Source::Crossref,
        Source::OpenAlex,
        Source::Arxiv,
        Source::PubMed,
        Source::SemanticScholar,
        Source::LibraryOfCongress,
        Source::K10plus,
        Source::WebOfScience,
        Source::Scopus,
        Source::ScienceDirect,
        Source::Ieee,
        Source::Springer,
    ];

    /// Searching it takes the person's own API key.
    pub fn needs_key(self) -> bool {
        matches!(
            self,
            Source::WebOfScience | Source::Scopus | Source::ScienceDirect | Source::Ieee | Source::Springer
        )
    }

    pub fn id(self) -> &'static str {
        match self {
            Source::Crossref => "crossref",
            Source::OpenAlex => "openalex",
            Source::Arxiv => "arxiv",
            Source::PubMed => "pubmed",
            Source::SemanticScholar => "semantic-scholar",
            Source::LibraryOfCongress => "loc",
            Source::K10plus => "k10plus",
            Source::WebOfScience => "wos",
            Source::Scopus => "scopus",
            Source::ScienceDirect => "sciencedirect",
            Source::Ieee => "ieee",
            Source::Springer => "springer",
        }
    }

    pub fn parse_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Source::Crossref => "Crossref",
            Source::OpenAlex => "OpenAlex",
            Source::Arxiv => "arXiv",
            Source::PubMed => "PubMed",
            Source::SemanticScholar => "Semantic Scholar",
            Source::LibraryOfCongress => "Library of Congress",
            Source::K10plus => "K10plus",
            Source::WebOfScience => "Web of Science",
            Source::Scopus => "Scopus",
            Source::ScienceDirect => "ScienceDirect",
            Source::Ieee => "IEEE Xplore",
            Source::Springer => "Springer Nature",
        }
    }

    pub fn homepage(self) -> &'static str {
        match self {
            Source::Crossref => "https://search.crossref.org/",
            Source::OpenAlex => "https://openalex.org/",
            Source::Arxiv => "https://arxiv.org/",
            Source::PubMed => "https://pubmed.ncbi.nlm.nih.gov/",
            Source::SemanticScholar => "https://www.semanticscholar.org/",
            Source::LibraryOfCongress => "https://catalog.loc.gov/",
            Source::K10plus => "https://kxp.k10plus.de/",
            Source::WebOfScience => "https://www.webofscience.com/",
            Source::Scopus => "https://www.scopus.com/",
            Source::ScienceDirect => "https://www.sciencedirect.com/",
            Source::Ieee => "https://ieeexplore.ieee.org/",
            Source::Springer => "https://link.springer.com/",
        }
    }
}

/// Words to search for, and which page of results.
#[derive(Clone, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    /// From 0.
    pub page: u32,
    pub per_page: u32,
    /// The person's API key, for a source that needs one.
    pub key: Option<String>,
}

impl Query {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), page: 0, per_page: 20, key: None }
    }

    /// The key, for a source that has checked there is one.
    fn key(&self) -> &str {
        self.key.as_deref().unwrap_or_default()
    }

    fn offset(&self) -> u32 {
        self.page.saturating_mul(self.per_page)
    }
}

/// Leaves the key out.
impl std::fmt::Debug for Query {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Query")
            .field("text", &self.text)
            .field("page", &self.page)
            .field("per_page", &self.per_page)
            .field("key", &self.key.as_ref().map(|_| "…"))
            .finish()
    }
}

/// One result.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    /// `<source>:<the source's own ID>`, the same each time the work is found.
    pub key: String,
    pub item: Item,
    /// An open-access PDF, when the source knows one.
    pub pdf_url: Option<String>,
    pub cited_by: Option<u64>,
}

/// A page of results.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Page {
    pub hits: Vec<Hit>,
    /// How many results there are in all, when the source says.
    pub total: Option<u64>,
}

impl Page {
    /// True if a later page may have more results.
    pub fn has_more(&self, query: &Query) -> bool {
        let seen = u64::from(query.offset()) + self.hits.len() as u64;
        !self.hits.is_empty() && self.total.is_none_or(|total| seen < total)
    }
}

pub struct Searcher<H: Http = UreqHttp> {
    http: H,
}

impl Searcher<UreqHttp> {
    pub fn new() -> Self {
        Self { http: UreqHttp::new() }
    }
}

impl Default for Searcher<UreqHttp> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H: Http> Searcher<H> {
    pub fn with_http(http: H) -> Self {
        Self { http }
    }

    pub fn search(&self, source: Source, query: &Query) -> Result<Page> {
        let Some(query) = tidy(query) else { return Ok(Page::default()) };
        if source.needs_key() && query.key.as_deref().is_none_or(|k| k.trim().is_empty()) {
            return Err(Error::KeyNeeded(source.name()));
        }
        match source {
            Source::Crossref => crossref::search(self, &query),
            Source::OpenAlex => openalex::search(self, &query),
            Source::Arxiv => arxiv::search(self, &query),
            Source::PubMed => pubmed::search(self, &query),
            Source::SemanticScholar => semantic_scholar::search(self, &query),
            Source::LibraryOfCongress | Source::K10plus => sru::search(self, &Catalogue::of(source), &query),
            Source::WebOfScience => wos::search(self, &query),
            Source::Scopus => scopus::search(self, &query),
            Source::ScienceDirect => sciencedirect::search(self, &query),
            Source::Ieee => ieee::search(self, &query),
            Source::Springer => springer::search(self, &query),
        }
    }

    /// Searches a library catalogue over SRU, or over Z39.50 when its
    /// address is `z3950://host:port/database`.
    pub fn search_catalogue(&self, catalogue: &Catalogue, query: &Query) -> Result<Page> {
        match tidy(query) {
            Some(query) if z3950::is_z3950(&catalogue.url) => z3950::search(catalogue, &query),
            Some(query) => sru::search(self, catalogue, &query),
            None => Ok(Page::default()),
        }
    }

    /// GETs JSON with extra headers, for a source that takes the person's
    /// key: tells a refused key from other failures, and keeps the key out
    /// of any error.
    fn fetch_keyed_json(
        &self,
        source: Source,
        url: &str,
        headers: &[(&str, &str)],
        key: &str,
    ) -> Result<Value> {
        self.send_keyed_json(source, "GET", url, headers, None, key)
    }

    /// Like `fetch_keyed_json`, for any method, with a JSON body.
    fn send_keyed_json(
        &self,
        source: Source,
        method: &'static str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<Value>,
        key: &str,
    ) -> Result<Value> {
        let mut all = vec![("Accept".to_string(), "application/json".to_string())];
        all.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        let body = body.map(|b| b.to_string().into_bytes());
        let request = Request { method, url: url.to_string(), headers: all, body };
        let response = self
            .http
            .send(request)
            .map_err(|e| Error::Network(if key.is_empty() { e.0 } else { e.0.replace(key, "…") }))?;
        match response.status {
            200..=299 => serde_json::from_slice(&response.body)
                .map_err(|e| Error::Parse { service: source.name(), detail: e.to_string() }),
            401 | 403 => Err(Error::KeyRejected(source.name())),
            429 | 503 => Err(Error::RateLimited(source.name())),
            status => Err(Error::Http { service: source.name(), status }),
        }
    }

    fn fetch(&self, source: Source, url: &str, accept: &str) -> Result<String> {
        let response = self.http.get(url, accept)?;
        match response.status {
            200..=299 => Ok(response.body),
            429 | 503 => Err(Error::RateLimited(source.name())),
            status => Err(Error::Http { service: source.name(), status }),
        }
    }

    fn fetch_json(&self, source: Source, url: &str) -> Result<Value> {
        let body = self.fetch(source, url, "application/json")?;
        serde_json::from_str(&body)
            .map_err(|e| Error::Parse { service: source.name(), detail: e.to_string() })
    }
}

/// The query with its words single-spaced and a page size the databases
/// take; `None` when there is nothing to search for.
fn tidy(query: &Query) -> Option<Query> {
    let text = query.text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
    }
    Some(Query { text, per_page: query.per_page.clamp(1, 50), ..query.clone() })
}

/// Percent-encodes a query parameter value.
fn encode(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

/// A JSON string or number as trimmed text; `None` when empty.
fn string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Text with HTML tags taken out and common entities decoded (titles and
/// abstracts some databases mark up).
fn plain_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = text
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// An item whose DOI is an arXiv DOI (`10.48550/arxiv.<id>`) gets the arXiv
/// ID too.
fn fill_arxiv_id(item: &mut Item) {
    if item.arxiv_id.is_none()
        && let Some(doi) = &item.doi
        && let Some(id) = doi.as_str().strip_prefix("10.48550/arxiv.")
    {
        item.arxiv_id = Some(id.to_string());
    }
}

/// `54` and `58` → `54-58`.
fn page_range(first: Option<String>, last: Option<String>) -> Option<String> {
    match (first, last) {
        (Some(first), Some(last)) if first != last => Some(format!("{first}-{last}")),
        (Some(first), _) => Some(first),
        (None, last) => last,
    }
}

/// `2013-07-31` and the like: the year and, when longer, the date itself.
fn year_and_date(date: &str) -> (Option<i32>, Option<String>) {
    let date = date.trim();
    let year = date.get(..4).and_then(|y| y.parse().ok());
    let full = (date.len() > 4 && year.is_some()).then(|| date.chars().take(10).collect());
    (year, full)
}

#[cfg(test)]
mod tests;
