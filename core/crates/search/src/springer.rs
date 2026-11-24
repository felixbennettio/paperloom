//! Springer Nature, through the Meta API (`/meta/v2/json`), with the
//! person's own key: articles, chapters and conference papers with
//! abstracts, and PDFs of the open-access ones.

use paperloom_http::Http;
use paperloom_model::{Creator, Doi, Item, ItemType};
use serde_json::Value;

use crate::{
    Error, Hit, Page, Query, Result, Searcher, Source, encode, fill_arxiv_id, page_range, plain_text, string,
    year_and_date,
};

/// The most the API returns in one page.
const MOST: u32 = 25;

pub(crate) fn search<H: Http>(searcher: &Searcher<H>, query: &Query) -> Result<Page> {
    let url = format!(
        "https://api.springernature.com/meta/v2/json?q={}&s={}&p={}&api_key={}",
        encode(&query.text),
        query.offset() + 1,
        query.per_page.min(MOST),
        encode(query.key()),
    );
    let value = searcher.fetch_keyed_json(Source::Springer, &url, &[], query.key())?;
    let records = value["records"]
        .as_array()
        .ok_or_else(|| Error::Parse { service: "Springer Nature", detail: "no records".into() })?;
    let total = value["result"].as_array().and_then(|r| r.first()).and_then(|r| string(&r["total"]));
    Ok(Page { hits: records.iter().filter_map(hit).collect(), total: total.and_then(|t| t.parse().ok()) })
}

fn hit(record: &Value) -> Option<Hit> {
    let title = string(&record["title"]).map(|t| plain_text(&t)).filter(|t| !t.is_empty())?;
    let doi = record["doi"].as_str().and_then(|d| Doi::parse(d).ok());
    let id = doi.as_ref().map(|d| d.to_string()).or_else(|| string(&record["identifier"]))?;
    let mut item = Item::new(item_type(record), title);
    item.creators = record["creators"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["creator"].as_str())
        .map(Creator::parse)
        .collect();
    item.container_title = string(&record["publicationName"]);
    item.publisher = string(&record["publisher"]);
    let (year, date) = record["publicationDate"].as_str().map(year_and_date).unwrap_or_default();
    item.year = year;
    item.date = date;
    item.volume = string(&record["volume"]);
    item.issue = string(&record["number"]);
    item.pages = page_range(string(&record["startingPage"]), string(&record["endingPage"]));
    item.doi = doi;
    item.issn = string(&record["issn"]).or_else(|| string(&record["eIssn"]));
    item.isbn = string(&record["printIsbn"])
        .or_else(|| string(&record["electronicIsbn"]))
        .or_else(|| string(&record["isbn"]));
    item.language = string(&record["language"]);
    let links = record["url"].as_array();
    item.url = item.doi.as_ref().map(|d| format!("https://doi.org/{d}")).or_else(|| {
        links
            .into_iter()
            .flatten()
            .find(|u| u["format"].as_str() != Some("pdf"))
            .and_then(|u| string(&u["value"]))
    });
    item.abstract_note = abstract_text(&record["abstract"]);
    item.keywords = record["keyword"].as_array().into_iter().flatten().filter_map(string).collect();
    fill_arxiv_id(&mut item);
    let open = record["openaccess"].as_str() == Some("true") || record["openaccess"].as_bool() == Some(true);
    let pdf_url = links
        .into_iter()
        .flatten()
        .find(|u| u["format"].as_str() == Some("pdf"))
        .and_then(|u| string(&u["value"]))
        .filter(|_| open);
    Some(Hit { key: format!("springer:{id}"), item, pdf_url, cited_by: None })
}

/// A plain abstract, or one given as `{"h1": "Abstract", "p": …}`.
fn abstract_text(value: &Value) -> Option<String> {
    let text = match value {
        Value::Object(parts) => match parts.get("p")? {
            Value::Array(paragraphs) => paragraphs.iter().filter_map(string).collect::<Vec<_>>().join("\n\n"),
            other => string(other)?,
        },
        other => string(other)?,
    };
    let text = plain_text(&text);
    (!text.is_empty()).then_some(text)
}

fn item_type(record: &Value) -> ItemType {
    let content = record["contentType"].as_str().unwrap_or("");
    let genre = |name: &str| match &record["genre"] {
        Value::Array(all) => all.iter().any(|g| g.as_str() == Some(name)),
        other => other.as_str() == Some(name),
    };
    if content.contains("Conference") || genre("ConferencePaper") {
        ItemType::ConferencePaper
    } else if content == "Book" {
        ItemType::Book
    } else if content.contains("Chapter") || content.contains("ReferenceWorkEntry") {
        ItemType::BookChapter
    } else {
        ItemType::JournalArticle
    }
}
