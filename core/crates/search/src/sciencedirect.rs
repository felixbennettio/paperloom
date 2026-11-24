//! ScienceDirect, through Elsevier's ScienceDirect Search API (v2, a `PUT`
//! with the query as JSON), with the person's own key: the same Elsevier
//! developer key Scopus takes.

use paperloom_http::Http;
use paperloom_model::{Creator, Doi, Item, ItemType};
use serde_json::{Value, json};

use crate::{
    Hit, Page, Query, Result, Searcher, Source, fill_arxiv_id, page_range, plain_text, string, year_and_date,
};

/// The most the API returns in one page.
const MOST: u32 = 25;

pub(crate) fn search<H: Http>(searcher: &Searcher<H>, query: &Query) -> Result<Page> {
    let body = json!({
        "qs": query.text,
        "display": { "offset": query.offset(), "show": query.per_page.min(MOST), "sortBy": "relevance" },
    });
    let value = searcher.send_keyed_json(
        Source::ScienceDirect,
        "PUT",
        "https://api.elsevier.com/content/search/sciencedirect",
        &[("X-ELS-APIKey", query.key()), ("Content-Type", "application/json")],
        Some(body),
        query.key(),
    )?;
    // With nothing found, there are no `results` at all.
    let hits = value["results"].as_array().into_iter().flatten().filter_map(hit).collect();
    Ok(Page { hits, total: value["resultsFound"].as_u64() })
}

fn hit(result: &Value) -> Option<Hit> {
    let title = string(&result["title"]).map(|t| plain_text(&t)).filter(|t| !t.is_empty())?;
    let doi = result["doi"].as_str().and_then(|d| Doi::parse(d).ok());
    let id = string(&result["pii"]).or_else(|| doi.as_ref().map(|d| d.to_string()))?;
    let mut item = Item::new(ItemType::JournalArticle, title);
    let mut authors: Vec<(u64, &str)> = result["authors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| Some((a["order"].as_u64().unwrap_or(u64::MAX), a["name"].as_str()?)))
        .collect();
    authors.sort_by_key(|(order, _)| *order);
    item.creators = authors.into_iter().map(|(_, name)| Creator::parse(name)).collect();
    item.container_title = string(&result["sourceTitle"]).or_else(|| string(&result["publicationName"]));
    let (year, date) = result["publicationDate"].as_str().map(year_and_date).unwrap_or_default();
    item.year = year;
    item.date = date;
    let (volume, issue) = volume_issue(result["volumeIssue"].as_str().unwrap_or_default());
    item.volume = volume;
    item.issue = issue;
    item.pages = page_range(string(&result["pages"]["first"]), string(&result["pages"]["last"]));
    item.doi = doi;
    item.url = item.doi.as_ref().map(|d| format!("https://doi.org/{d}")).or_else(|| string(&result["uri"]));
    fill_arxiv_id(&mut item);
    Some(Hit { key: format!("sciencedirect:{id}"), item, pdf_url: None, cited_by: None })
}

/// `Volume 500, Issue 7460`, `Volume 12`, `Volumes 3–4`: the volume and issue.
fn volume_issue(text: &str) -> (Option<String>, Option<String>) {
    let mut volume = None;
    let mut issue = None;
    for part in text.split(',') {
        let mut words = part.split_whitespace();
        match (words.next(), words.next()) {
            (Some("Volume" | "Volumes"), Some(value)) => volume = Some(value.to_string()),
            (Some("Issue" | "Issues"), Some(value)) => issue = Some(value.to_string()),
            _ => {}
        }
    }
    (volume, issue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_volume_and_issue() {
        assert_eq!(volume_issue("Volume 500, Issue 7460"), (Some("500".into()), Some("7460".into())));
        assert_eq!(volume_issue("Volume 12"), (Some("12".into()), None));
        assert_eq!(volume_issue(""), (None, None));
    }
}
