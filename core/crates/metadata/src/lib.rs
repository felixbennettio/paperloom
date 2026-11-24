//! Looks up an item's details from an identifier.
//!
//! - DOI: doi.org content negotiation for CSL-JSON, which every major
//!   registration agency answers (Crossref, DataCite, mEDRA, JaLC, …).
//! - arXiv: through its DataCite DOI, `10.48550/arXiv.<id>`.
//! - PubMed: NCBI E-utilities `esummary` (JSON).
//! - ISBN: Open Library's books API (JSON).

mod identifier;

use paperloom_http::{Http, NetworkError, UreqHttp};
use paperloom_model::{Creator, Doi, Item, ItemType, csl};
use serde_json::Value;

pub use identifier::Identifier;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(String),
    #[error("{0} was not found")]
    NotFound(String),
    #[error("the lookup service answered {status} for {what}")]
    Http { what: String, status: u16 },
    #[error("unexpected reply for {what}: {detail}")]
    Parse { what: String, detail: String },
}

impl From<NetworkError> for Error {
    fn from(err: NetworkError) -> Self {
        Error::Network(err.0)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

const CSL_JSON: &str = "application/vnd.citationstyles.csl+json";

pub struct Resolver<H: Http = UreqHttp> {
    http: H,
}

impl Resolver<UreqHttp> {
    pub fn new() -> Self {
        Self { http: UreqHttp::new() }
    }
}

impl Default for Resolver<UreqHttp> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H: Http> Resolver<H> {
    pub fn with_http(http: H) -> Self {
        Self { http }
    }

    /// The details of the work an identifier names.
    pub fn resolve(&self, id: &Identifier) -> Result<Item> {
        match id {
            Identifier::Doi(doi) => self.doi(doi),
            Identifier::Arxiv(arxiv) => self.arxiv(arxiv),
            Identifier::Pmid(pmid) => self.pubmed(pmid),
            Identifier::Isbn(isbn) => self.isbn(isbn),
        }
    }

    fn fetch_json(&self, what: &str, url: &str, accept: &str) -> Result<Value> {
        let response = self.http.get(url, accept)?;
        match response.status {
            200..=299 => serde_json::from_str(&response.body)
                .map_err(|e| Error::Parse { what: what.to_string(), detail: e.to_string() }),
            404 | 410 => Err(Error::NotFound(what.to_string())),
            status => Err(Error::Http { what: what.to_string(), status }),
        }
    }

    fn doi(&self, doi: &Doi) -> Result<Item> {
        let what = format!("DOI {doi}");
        let url = format!("https://doi.org/{}", encode_path(doi.as_str()));
        let value = self.fetch_json(&what, &url, CSL_JSON)?;
        let mut item = csl::item_from_csl(&value)
            .ok_or_else(|| Error::Parse { what: what.clone(), detail: "no title".into() })?;
        item.doi = Some(doi.clone());
        Ok(item)
    }

    fn arxiv(&self, id: &str) -> Result<Item> {
        let doi = Doi::parse(&format!("10.48550/arXiv.{id}"))
            .map_err(|e| Error::Parse { what: format!("arXiv:{id}"), detail: e.to_string() })?;
        let mut item = self.doi(&doi).map_err(|e| match e {
            Error::NotFound(_) => Error::NotFound(format!("arXiv:{id}")),
            other => other,
        })?;
        item.item_type = ItemType::Preprint;
        item.arxiv_id = Some(id.to_string());
        item.container_title.get_or_insert_with(|| "arXiv".to_string());
        item.number.get_or_insert_with(|| format!("arXiv:{id}"));
        item.url = Some(format!("https://arxiv.org/abs/{id}"));
        Ok(item)
    }

    fn pubmed(&self, pmid: &str) -> Result<Item> {
        let what = format!("PMID {pmid}");
        let url = format!(
            "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&retmode=json&id={}",
            encode_path(pmid)
        );
        let value = self.fetch_json(&what, &url, "application/json")?;
        pubmed_item(&value["result"][pmid], pmid).ok_or(Error::NotFound(what))
    }

    fn isbn(&self, isbn: &str) -> Result<Item> {
        let what = format!("ISBN {isbn}");
        let key = format!("ISBN:{isbn}");
        let url = format!("https://openlibrary.org/api/books?format=json&jscmd=data&bibkeys={key}");
        let value = self.fetch_json(&what, &url, "application/json")?;
        let book = value.get(&key).filter(|b| b.is_object()).ok_or_else(|| Error::NotFound(what.clone()))?;
        let title = string(&book["title"]).ok_or_else(|| Error::NotFound(what.clone()))?;
        let full_title = match string(&book["subtitle"]) {
            Some(subtitle) => format!("{title}: {subtitle}"),
            None => title,
        };
        let mut item = Item::new(ItemType::Book, full_title);
        let names = |field: &str| -> Vec<String> {
            book[field].as_array().into_iter().flatten().filter_map(|v| string(&v["name"])).collect()
        };
        item.creators = names("authors").iter().map(|n| Creator::parse(n)).collect();
        item.publisher = names("publishers").into_iter().next();
        item.place = names("publish_places").into_iter().next();
        if let Some(date) = string(&book["publish_date"]) {
            item.year = date
                .split(|c: char| !c.is_ascii_digit())
                .find(|part| part.len() == 4)
                .and_then(|y| y.parse().ok());
        }
        item.isbn = Some(isbn.to_string());
        item.url = string(&book["url"]);
        Ok(item)
    }
}

/// Reads one record of an E-utilities `esummary` reply (`result.<pmid>`);
/// `None` for an unknown PMID.
pub fn pubmed_item(record: &Value, pmid: &str) -> Option<Item> {
    let title = record["title"].as_str().map(str::trim).filter(|t| !t.is_empty());
    let (Some(title), None) = (title, record.get("error")) else {
        return None;
    };
    let mut item = Item::new(ItemType::JournalArticle, title.trim_end_matches('.'));
    item.creators = record["authors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["authtype"].as_str().is_none_or(|t| t == "Author"))
        .filter_map(|a| a["name"].as_str())
        .map(pubmed_name)
        .collect();
    item.container_title = string(&record["fulljournalname"]);
    item.container_short_title = string(&record["source"]);
    item.volume = string(&record["volume"]);
    item.issue = string(&record["issue"]);
    item.pages = string(&record["pages"]);
    item.pmid = Some(pmid.to_string());
    item.url = Some(format!("https://pubmed.ncbi.nlm.nih.gov/{pmid}/"));
    if let Some(date) = record["sortpubdate"].as_str().or(record["pubdate"].as_str()) {
        item.year = date.get(..4).and_then(|y| y.parse().ok());
    }
    for id in record["articleids"].as_array().into_iter().flatten() {
        if id["idtype"] == "doi" {
            item.doi = id["value"].as_str().and_then(|d| Doi::parse(d).ok());
        }
    }
    item.language = record["lang"].as_array().and_then(|l| l.first()).and_then(Value::as_str).map(|l| {
        match l {
            "eng" => "en",
            "chi" => "zh",
            "jpn" => "ja",
            "fre" => "fr",
            "rus" => "ru",
            "ger" => "de",
            other => other,
        }
        .to_string()
    });
    Some(item)
}

fn string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// PubMed names are `Family Initials`: `Kucsko G`.
fn pubmed_name(name: &str) -> Creator {
    match name.rsplit_once(' ') {
        Some((family, initials)) if initials.chars().all(|c| c.is_ascii_uppercase()) => {
            let given = initials.chars().map(|c| format!("{c}.")).collect::<Vec<_>>().join(" ");
            Creator::person(family, given)
        }
        _ => Creator::parse(name),
    }
}

/// Percent-encodes the characters a DOI may contain that a URL path may not.
fn encode_path(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes())
        .collect::<String>()
        .replace("%2F", "/")
        .replace('+', "%20")
}

#[cfg(test)]
mod tests;
