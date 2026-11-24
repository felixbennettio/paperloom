//! Semantic Scholar Graph API, `/paper/search`. Without a key it shares a
//! rate limit with everyone, so busy answers (429) are common.

use paperloom_http::Http;
use paperloom_model::{Creator, Doi, Item, ItemType};
use serde_json::Value;

use crate::{Hit, Page, Query, Result, Searcher, Source, encode, fill_arxiv_id, string, year_and_date};

const FIELDS: &str = "title,authors,year,venue,externalIds,abstract,publicationTypes,journal,\
publicationDate,openAccessPdf,citationCount,url";

pub(crate) fn search<H: Http>(searcher: &Searcher<H>, query: &Query) -> Result<Page> {
    let url = format!(
        "https://api.semanticscholar.org/graph/v1/paper/search?query={}&offset={}&limit={}&fields={FIELDS}",
        encode(&query.text),
        query.offset(),
        query.per_page,
    );
    let value = searcher.fetch_json(Source::SemanticScholar, &url)?;
    let hits = value["data"].as_array().into_iter().flatten().filter_map(hit).collect();
    Ok(Page { hits, total: value["total"].as_u64() })
}

fn hit(paper: &Value) -> Option<Hit> {
    let paper_id = paper["paperId"].as_str()?;
    let ids = &paper["externalIds"];
    let journal = &paper["journal"];
    let types: Vec<&str> =
        paper["publicationTypes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let arxiv = string(&ids["ArXiv"]);
    let container = string(&journal["name"]).or_else(|| string(&paper["venue"]));
    let item_type = if types.contains(&"Conference") {
        ItemType::ConferencePaper
    } else if types.contains(&"BookSection") {
        ItemType::BookChapter
    } else if types.contains(&"Book") {
        ItemType::Book
    } else if types.contains(&"Dataset") {
        ItemType::Dataset
    } else if !types.is_empty() {
        ItemType::JournalArticle
    } else if arxiv.is_some() {
        ItemType::Preprint
    } else if container.is_some() {
        ItemType::JournalArticle
    } else {
        ItemType::Other
    };
    let mut item = Item::new(item_type, string(&paper["title"])?);
    item.creators = paper["authors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["name"].as_str())
        .map(Creator::parse)
        .collect();
    let (year, date) = paper["publicationDate"].as_str().map(year_and_date).unwrap_or_default();
    item.year = year.or_else(|| paper["year"].as_i64().map(|y| y as i32));
    item.date = date;
    item.container_title = container;
    item.volume = string(&journal["volume"]);
    item.pages = string(&journal["pages"]);
    item.abstract_note = string(&paper["abstract"]);
    item.pmid = string(&ids["PubMed"]);
    item.doi = string(&ids["DOI"])
        .or_else(|| arxiv.as_ref().map(|a| format!("10.48550/arXiv.{a}")))
        .and_then(|d| Doi::parse(&d).ok());
    item.arxiv_id = arxiv;
    fill_arxiv_id(&mut item);
    item.url = string(&paper["url"]);
    Some(Hit {
        key: format!("semantic-scholar:{paper_id}"),
        item,
        pdf_url: string(&paper["openAccessPdf"]["url"]),
        cited_by: paper["citationCount"].as_u64(),
    })
}
