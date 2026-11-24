//! Online search in the open databases and those the person has a key for,
//! works read from pages in the in-app browser, and adding the ones a person
//! picks. Results are kept (by key) between the two calls, so adding needs no
//! second trip to the network and loses no detail.

use std::collections::HashMap;

use paperloom_capture as capture;
use paperloom_metadata::{Identifier, Resolver};
use paperloom_model::{CollectionId, Item, ItemId, ItemType};
use paperloom_search::{self as search, Query, Searcher, Source};
use paperloom_store as store;
use serde::{Deserialize, Serialize};

use crate::{CoreError, ImportReport, ItemDetail, PaperLibrary, add_all};

/// How many results to keep for adding; older searches are forgotten.
const KEPT_RESULTS: usize = 2_000;

impl From<search::Error> for CoreError {
    fn from(err: search::Error) -> Self {
        let message = err.to_string();
        match err {
            search::Error::Network(_) | search::Error::RateLimited(_) => CoreError::Network { message },
            search::Error::KeyNeeded(_) | search::Error::KeyRejected(_) => {
                CoreError::InvalidInput { message }
            }
            _ => CoreError::Lookup { message },
        }
    }
}

/// A database the apps can search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SearchSource {
    pub id: String,
    pub name: String,
    pub homepage: String,
    /// Searching it takes the person's own API key.
    pub needs_key: bool,
}

/// The databases `search_online` takes, in the order to offer them.
#[uniffi::export]
pub fn search_sources() -> Vec<SearchSource> {
    Source::ALL
        .iter()
        .map(|s| SearchSource {
            id: s.id().to_string(),
            name: s.name().to_string(),
            homepage: s.homepage().to_string(),
            needs_key: s.needs_key(),
        })
        .collect()
}

/// A library catalogue the person added: any that answers SRU or Z39.50.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SruCatalogue {
    /// Starts the key of each result; the apps make it up.
    pub id: String,
    pub name: String,
    /// The SRU address, e.g. `https://services.dnb.de/sru/dnb?version=1.1`,
    /// or a Z39.50 one, `z3950://host:port/database` (with `user:password@`
    /// and `?charset=gbk` when the catalogue needs them).
    pub url: String,
    /// The CQL index each word is looked for in (`pica.all`); empty for
    /// the server's own choice.
    pub index: String,
    /// The record schema to ask for: `marcxml` (MARC 21, the default when
    /// empty), the server's own name for it, or `dc` (Dublin Core).
    pub schema: String,
}

impl From<search::Catalogue> for SruCatalogue {
    fn from(c: search::Catalogue) -> Self {
        Self { id: c.id, name: c.name, url: c.url, index: c.index, schema: c.schema }
    }
}

impl From<SruCatalogue> for search::Catalogue {
    fn from(c: SruCatalogue) -> Self {
        Self { id: c.id, name: c.name, url: c.url, index: c.index, schema: c.schema }
    }
}

/// National libraries whose catalogues answer SRU, to start from when the
/// person adds one.
#[uniffi::export]
pub fn catalogue_presets() -> Vec<SruCatalogue> {
    search::catalogue_presets().into_iter().map(SruCatalogue::from).collect()
}

/// A site the in-app browser knows how to read, as a place to start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSite {
    pub id: String,
    pub name: String,
    pub home: String,
}

/// CNKI, Wanfang and VIP: the sites whose pages the in-app browser reads
/// beyond their embedded metadata.
#[uniffi::export]
pub fn capture_sites() -> Vec<CaptureSite> {
    capture::sites().into_iter().map(|s| CaptureSite { id: s.id, name: s.name, home: s.home }).collect()
}

/// The script the in-app browser runs in a page to read the works on it.
/// Its value is the JSON `read_captured_page` takes. It only reads.
#[uniffi::export]
pub fn capture_script() -> String {
    capture::script()
}

/// One search result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// What `add_search_results` takes.
    pub key: String,
    /// The work's details; `id` is empty.
    pub detail: ItemDetail,
    /// An open-access PDF, when the database knows one.
    pub pdf_url: Option<String>,
    pub cited_by: Option<u64>,
    /// The library's item with the same DOI, if it has one.
    pub library_item_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub hits: Vec<SearchHit>,
    /// How many results there are in all, when the database says.
    pub total: Option<u64>,
    /// True if asking for the next page may find more.
    pub has_more: bool,
}

/// Results from recent searches, by key.
#[derive(Default)]
pub(crate) struct KeptResults(HashMap<String, Item>, pub(crate) HashMap<String, Vec<String>>);

pub(crate) fn search_online(
    library: &PaperLibrary,
    source_id: &str,
    text: String,
    page: u32,
    api_key: Option<String>,
) -> Result<SearchPage, CoreError> {
    let source = Source::parse_id(source_id).ok_or_else(|| CoreError::InvalidInput {
        message: format!("Unknown search source “{source_id}”."),
    })?;
    let query = Query { page, key: api_key, ..Query::new(text) };
    let found = Searcher::new().search(source, &query)?;
    let has_more = found.has_more(&query);
    keep(library, found.hits, found.total, has_more)
}

pub(crate) fn search_catalogue(
    library: &PaperLibrary,
    catalogue: SruCatalogue,
    text: String,
    page: u32,
) -> Result<SearchPage, CoreError> {
    if catalogue.id.trim().is_empty() || catalogue.url.trim().is_empty() {
        return Err(CoreError::InvalidInput {
            message: "A catalogue needs a name and an address (SRU or Z39.50).".into(),
        });
    }
    let query = Query { page, ..Query::new(text) };
    let found = Searcher::new().search_catalogue(&catalogue.into(), &query)?;
    let has_more = found.has_more(&query);
    keep(library, found.hits, found.total, has_more)
}

/// How many works a page names by DOI alone are looked up.
const MOST_LOOKUPS: usize = 10;

/// The works on a page, from what the capture script answered there.
pub(crate) fn read_captured_page(library: &PaperLibrary, json: &str) -> Result<SearchPage, CoreError> {
    let found = capture::read_json(json, &capture::today())
        .map_err(|e| CoreError::InvalidInput { message: format!("This page could not be read: {e}.") })?;
    keep_captured(library, found)
}

/// Looks up works named by DOI alone and keeps everything the page found.
pub(crate) fn keep_captured(
    library: &PaperLibrary,
    mut found: Vec<capture::Found>,
) -> Result<SearchPage, CoreError> {
    // A page that gives little more than a DOI: the DOI's details.
    let resolver = Resolver::new();
    let bare = found
        .iter_mut()
        .filter(|f| {
            f.item.creators.is_empty() && f.item.doi.is_some() && f.item.item_type != ItemType::Webpage
        })
        .take(MOST_LOOKUPS);
    for work in bare {
        let Some(doi) = work.item.doi.clone() else { continue };
        if let Ok(mut full) = resolver.resolve(&Identifier::Doi(doi)) {
            capture::fill_missing(&mut full, &work.item);
            work.item = full;
        }
    }
    let total = found.len() as u64;
    let hits = found
        .into_iter()
        .map(|f| search::Hit { key: f.key, item: f.item, pdf_url: f.pdf_url, cited_by: f.cited_by })
        .collect();
    keep(library, hits, Some(total), false)
}

/// Keeps results for adding, and says which the library has.
pub(crate) fn keep(
    library: &PaperLibrary,
    found: Vec<search::Hit>,
    total: Option<u64>,
    has_more: bool,
) -> Result<SearchPage, CoreError> {
    let hits = {
        let store = library.lock()?;
        found.iter().map(|hit| summarize(&store, hit)).collect::<Result<Vec<_>, CoreError>>()?
    };
    let mut kept = library.kept_results()?;
    if kept.0.len() + found.len() > KEPT_RESULTS {
        kept.0.clear();
        kept.1.clear();
    }
    // A database's citation count comes along into the library.
    kept.0.extend(found.into_iter().map(|hit| {
        let mut item = hit.item;
        if item.cited_by.is_none() {
            item.cited_by = hit.cited_by.and_then(|count| u32::try_from(count).ok());
        }
        (hit.key, item)
    }));
    Ok(SearchPage { hits, total, has_more })
}

fn summarize(store: &store::Library, hit: &search::Hit) -> Result<SearchHit, CoreError> {
    let library_item_id = match &hit.item.doi {
        Some(doi) => store.find_by_doi(doi)?.map(|item| item.id.to_string()),
        None => None,
    };
    let mut detail = ItemDetail::from(&hit.item);
    detail.id = String::new();
    Ok(SearchHit {
        key: hit.key.clone(),
        detail,
        pdf_url: hit.pdf_url.clone(),
        cited_by: hit.cited_by,
        library_item_id,
    })
}

pub(crate) fn add_search_results(
    library: &PaperLibrary,
    keys: &[String],
    collection_id: Option<String>,
) -> Result<ImportReport, CoreError> {
    let items = {
        let kept = library.kept_results()?;
        keys.iter()
            .map(|key| {
                let mut item = kept.0.get(key).cloned().ok_or_else(|| CoreError::NotFound {
                    message: "This search result is no longer at hand; search again.".into(),
                })?;
                item.id = ItemId::new();
                Ok(item)
            })
            .collect::<Result<Vec<_>, CoreError>>()?
    };
    let collection = collection_id.map(CollectionId::from_string);
    let mut report = ImportReport { format: "search".into(), ..ImportReport::default() };
    {
        let mut store = library.lock()?;
        add_all(&mut store, &items, collection.as_ref(), &mut report)?;
        let kept = library.kept_results()?;
        for (key, item) in keys.iter().zip(&items) {
            let Some(tags) = kept.1.get(key) else { continue };
            let target = if store.item(&item.id)?.is_some() {
                Some(item.id.clone())
            } else if let Some(doi) = &item.doi {
                store.find_by_doi(doi)?.map(|found| found.id)
            } else {
                None
            };
            if let Some(id) = target {
                let mut merged = store.item_tags(&id)?;
                for tag in tags {
                    if !merged.contains(tag) {
                        merged.push(tag.clone());
                    }
                }
                store.set_item_tags(&id, &merged)?;
            }
        }
    }
    let added: Vec<ItemId> = report.added_ids.iter().map(|id| ItemId::from_string(id.clone())).collect();
    library.reindex_items(&added);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_the_open_databases_then_the_keyed_ones() {
        let sources = search_sources();
        let ids: Vec<&str> = sources.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "crossref",
                "openalex",
                "arxiv",
                "pubmed",
                "semantic-scholar",
                "loc",
                "k10plus",
                "wos",
                "scopus",
                "sciencedirect",
                "ieee",
                "springer"
            ]
        );
        let keyed: Vec<&str> = sources.iter().filter(|s| s.needs_key).map(|s| s.id.as_str()).collect();
        assert_eq!(keyed, ["wos", "scopus", "sciencedirect", "ieee", "springer"]);
    }

    #[test]
    fn offers_catalogues_and_checks_them() {
        assert!(catalogue_presets().iter().any(|c| c.url.contains("dnb.de")));
        let lib = PaperLibrary::in_memory().unwrap();
        let blank = SruCatalogue {
            id: "mine".into(),
            name: "Mine".into(),
            url: " ".into(),
            index: String::new(),
            schema: String::new(),
        };
        let err = lib.search_catalogue(blank.clone(), "x".into(), 0).unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput { .. }), "{err}");
        let ftp = SruCatalogue { url: "ftp://example.org/sru".into(), ..blank };
        let err = lib.search_catalogue(ftp, "x".into(), 0).unwrap_err();
        assert!(
            matches!(&err, CoreError::Lookup { message } if message == "Catalogue Mine: not a web address"),
            "{err}"
        );
    }

    #[test]
    fn rejects_unknown_sources() {
        let lib = PaperLibrary::in_memory().unwrap();
        let err = lib.search_online("cnki".into(), "x".into(), 0, None).unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput { .. }));
    }

    #[test]
    fn asks_for_a_key_before_searching() {
        let lib = PaperLibrary::in_memory().unwrap();
        let err = lib.search_online("scopus".into(), "graphene".into(), 0, None).unwrap_err();
        assert!(matches!(&err, CoreError::InvalidInput { message } if message.contains("Scopus")), "{err}");
    }

    #[test]
    fn reads_captured_pages_for_adding() {
        let lib = PaperLibrary::in_memory().unwrap();
        let page = r#"{"url":"https://kns.cnki.net/kns8s/defaultresult/index","site":"cnki","page":"list",
            "records":[{"title":"基于深度学习的文献分类研究","authors":["张三;李四"],"date":"2023-05-01","kind":"硕士",
                        "url":"https://kns.cnki.net/kcms2/article/abstract?v=abc"}]}"#;
        let read = lib.read_captured_page(page.into()).unwrap();
        assert_eq!(read.hits.len(), 1);
        assert_eq!(read.hits[0].detail.item_type, "thesis");
        let report = lib.add_search_results(vec![read.hits[0].key.clone()], None).unwrap();
        assert_eq!(report.added, 1);
        assert!(matches!(lib.read_captured_page("not json".into()), Err(CoreError::InvalidInput { .. })));
        assert_eq!(capture_sites().len(), 3);
        assert!(capture_script().contains("records"));
    }

    #[test]
    fn adds_kept_results_once_per_doi() {
        let lib = PaperLibrary::in_memory().unwrap();
        let mut with_doi = Item::new(paperloom_model::ItemType::JournalArticle, "Kept");
        with_doi.doi = paperloom_model::Doi::parse("10.1000/kept").ok();
        let without_doi = Item::new(paperloom_model::ItemType::Book, "No DOI");
        lib.kept_results()
            .unwrap()
            .0
            .extend([("a:1".to_string(), with_doi), ("a:2".to_string(), without_doi)]);
        let collection = lib.create_collection("Found".into(), None).unwrap();
        let keys = vec!["a:1".to_string(), "a:2".to_string()];
        let report = lib.add_search_results(keys.clone(), Some(collection.id.clone())).unwrap();
        assert_eq!((report.added, report.duplicates), (2, 0));
        assert_eq!(lib.items_in_collection(collection.id).unwrap().len(), 2);
        let again = lib.add_search_results(keys, None).unwrap();
        assert_eq!((again.added, again.duplicates), (1, 1), "the DOI one is already there");
        let gone = lib.add_search_results(vec!["a:3".into()], None).unwrap_err();
        assert!(matches!(gone, CoreError::NotFound { .. }));
    }
}
