//! Crossref REST API, `/works?query.bibliographic=`. Its records are nearly
//! CSL-JSON; type names and a few keys differ.

use paperloom_http::Http;
use paperloom_model::csl;
use serde_json::Value;

use crate::{Error, Hit, Page, Query, Result, Searcher, Source, encode, fill_arxiv_id};

const FIELDS: &str = "DOI,title,subtitle,author,editor,container-title,short-container-title,issued,\
published-print,published-online,volume,issue,page,publisher,publisher-location,ISSN,ISBN,URL,abstract,\
type,is-referenced-by-count";

pub(crate) fn search<H: Http>(searcher: &Searcher<H>, query: &Query) -> Result<Page> {
    let url = format!(
        "https://api.crossref.org/works?query.bibliographic={}&rows={}&offset={}&select={FIELDS}",
        encode(&query.text),
        query.per_page,
        query.offset(),
    );
    let value = searcher.fetch_json(Source::Crossref, &url)?;
    let message = &value["message"];
    let records = message["items"]
        .as_array()
        .ok_or_else(|| Error::Parse { service: "Crossref", detail: "no items".into() })?;
    Ok(Page { hits: records.iter().filter_map(hit).collect(), total: message["total-results"].as_u64() })
}

fn hit(record: &Value) -> Option<Hit> {
    let mut as_csl = record.clone();
    let obj = as_csl.as_object_mut()?;
    let kind = obj.get("type").and_then(Value::as_str).map_or("document", csl_type);
    obj.insert("type".into(), kind.into());
    for (from, to) in
        [("short-container-title", "container-title-short"), ("publisher-location", "publisher-place")]
    {
        if let Some(value) = obj.remove(from) {
            obj.insert(to.into(), value);
        }
    }
    let mut item = csl::item_from_csl(&as_csl)?;
    let subtitle =
        record["subtitle"].as_array().and_then(|s| s.first()).and_then(Value::as_str).map(str::trim);
    if let Some(subtitle) = subtitle.filter(|s| !s.is_empty() && !item.title.contains(s)) {
        item.title = format!("{}: {subtitle}", item.title);
    }
    fill_arxiv_id(&mut item);
    let doi = item.doi.as_ref()?.to_string();
    Some(Hit {
        key: format!("crossref:{doi}"),
        item,
        pdf_url: None,
        cited_by: record["is-referenced-by-count"].as_u64(),
    })
}

/// Crossref's work types as CSL types.
pub(crate) fn csl_type(crossref: &str) -> &'static str {
    match crossref {
        "journal-article" => "article-journal",
        "proceedings-article" => "paper-conference",
        "book-chapter" | "book-section" | "book-part" | "reference-entry" => "chapter",
        "book" | "monograph" | "edited-book" | "reference-book" | "book-set" | "book-series" => "book",
        "posted-content" => "article",
        "dissertation" => "thesis",
        "report" | "report-series" | "report-component" => "report",
        "dataset" => "dataset",
        "standard" => "standard",
        "peer-review" => "review",
        _ => "document",
    }
}
