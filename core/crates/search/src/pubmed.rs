//! PubMed through NCBI E-utilities: `esearch` finds the PMIDs, `esummary`
//! gives their records.

use paperloom_http::Http;
use serde_json::Value;

use crate::{Hit, Page, Query, Result, Searcher, Source, encode};

const EUTILS: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils";

pub(crate) fn search<H: Http>(searcher: &Searcher<H>, query: &Query) -> Result<Page> {
    let url = format!(
        "{EUTILS}/esearch.fcgi?db=pubmed&retmode=json&sort=relevance&term={}&retstart={}&retmax={}",
        encode(&query.text),
        query.offset(),
        query.per_page,
    );
    let found = searcher.fetch_json(Source::PubMed, &url)?;
    let result = &found["esearchresult"];
    let total = result["count"].as_str().and_then(|c| c.parse().ok());
    let ids: Vec<&str> =
        result["idlist"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if ids.is_empty() {
        return Ok(Page { hits: Vec::new(), total });
    }
    let url = format!("{EUTILS}/esummary.fcgi?db=pubmed&retmode=json&id={}", ids.join(","));
    let summaries = searcher.fetch_json(Source::PubMed, &url)?;
    let hits = ids
        .iter()
        .filter_map(|id| {
            let item = paperloom_metadata::pubmed_item(&summaries["result"][*id], id)?;
            Some(Hit { key: format!("pubmed:{id}"), item, pdf_url: None, cited_by: None })
        })
        .collect();
    Ok(Page { hits, total })
}
