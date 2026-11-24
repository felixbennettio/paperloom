//! Web of Science, through the Starter API (`/documents`), with the person's
//! own key: records, Web of Science citation counts and links to the full
//! record.

use paperloom_http::Http;
use paperloom_model::{Creator, Doi, Item, ItemType};
use serde_json::Value;

use crate::{Error, Hit, Page, Query, Result, Searcher, Source, encode, fill_arxiv_id, string};

pub(crate) fn search<H: Http>(searcher: &Searcher<H>, query: &Query) -> Result<Page> {
    let url = format!(
        "https://api.clarivate.com/apis/wos-starter/v1/documents?db=WOS&q={}&limit={}&page={}",
        encode(&query_text(&query.text)),
        query.per_page,
        query.page + 1,
    );
    let value =
        searcher.fetch_keyed_json(Source::WebOfScience, &url, &[("X-ApiKey", query.key())], query.key())?;
    let hits = value["hits"]
        .as_array()
        .ok_or_else(|| Error::Parse { service: "Web of Science", detail: "no hits".into() })?;
    Ok(Page { hits: hits.iter().filter_map(hit).collect(), total: value["metadata"]["total"].as_u64() })
}

/// The person's words as a topic search, or their own field-tagged query
/// (`TI=…`, `AU=… AND PY=2020`) as it is.
fn query_text(text: &str) -> String {
    if field_tagged(text) { text.to_string() } else { format!("TS=({text})") }
}

/// Whether the text has a Web of Science field tag: two or three capitals
/// right before `=`.
fn field_tagged(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().filter(|(_, b)| **b == b'=').any(|(i, _)| {
        let tag = bytes[..i].iter().rev().take_while(|b| b.is_ascii_uppercase()).count();
        (2..=3).contains(&tag) && (i == tag || !bytes[i - tag - 1].is_ascii_alphanumeric())
    })
}

fn hit(record: &Value) -> Option<Hit> {
    let uid = record["uid"].as_str()?;
    let title = string(&record["title"])?;
    let types: Vec<&str> =
        record["types"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let mut item = Item::new(item_type(&types), title);
    item.creators = record["names"]["authors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["displayName"].as_str().or_else(|| a["wosStandard"].as_str()))
        .map(Creator::parse)
        .collect();
    let source = &record["source"];
    item.container_title = string(&source["sourceTitle"]);
    item.year = source["publishYear"]
        .as_i64()
        .map(|y| y as i32)
        .or_else(|| source["publishYear"].as_str().and_then(|y| y.trim().parse().ok()));
    item.volume = string(&source["volume"]);
    item.issue = string(&source["issue"]);
    item.pages = string(&source["pages"]["range"]);
    let ids = &record["identifiers"];
    item.doi = ids["doi"].as_str().and_then(|d| Doi::parse(d).ok());
    item.issn = string(&ids["issn"]).or_else(|| string(&ids["eissn"]));
    item.isbn = string(&ids["isbn"]);
    item.pmid = string(&ids["pmid"]).map(|p| p.rsplit(':').next().unwrap_or_default().to_string());
    item.url = item
        .doi
        .as_ref()
        .map(|d| format!("https://doi.org/{d}"))
        .or_else(|| string(&record["links"]["record"]));
    item.keywords =
        record["keywords"]["authorKeywords"].as_array().into_iter().flatten().filter_map(string).collect();
    fill_arxiv_id(&mut item);
    let citations = record["citations"].as_array();
    let cited_by = citations
        .into_iter()
        .flatten()
        .find(|c| c["db"].as_str() == Some("WOS"))
        .or_else(|| citations.and_then(|c| c.first()))
        .and_then(|c| c["count"].as_u64());
    Some(Hit { key: format!("wos:{uid}"), item, pdf_url: None, cited_by })
}

fn item_type(types: &[&str]) -> ItemType {
    let has = |name: &str| types.iter().any(|t| t.eq_ignore_ascii_case(name));
    if has("Book Chapter") {
        ItemType::BookChapter
    } else if has("Book") {
        ItemType::Book
    } else if has("Preprint") {
        ItemType::Preprint
    } else if has("Proceedings Paper") && !has("Article") {
        ItemType::ConferencePaper
    } else if has("Data Paper") || has("Data Set") {
        ItemType::Dataset
    } else {
        ItemType::JournalArticle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_field_tags_from_words() {
        assert!(field_tagged("TI=graphene AND PY=2020"));
        assert!(field_tagged("(AU=Smith)"));
        assert!(!field_tagged("graphene oxide"));
        assert!(!field_tagged("a = b"));
        assert!(!field_tagged("ABCD=x"), "four capitals is not a tag");
        assert_eq!(query_text("graphene oxide"), "TS=(graphene oxide)");
    }
}
