//! IEEE Xplore, through the Metadata API (`/search/articles`), with the
//! person's own key: journals, conferences, standards and books, with open
//! PDFs where IEEE marks them open.

use paperloom_http::Http;
use paperloom_model::{Creator, Doi, Item, ItemType};
use serde_json::Value;

use crate::{
    Hit, Page, Query, Result, Searcher, Source, encode, fill_arxiv_id, page_range, plain_text, string,
};

pub(crate) fn search<H: Http>(searcher: &Searcher<H>, query: &Query) -> Result<Page> {
    let url = format!(
        "https://ieeexploreapi.ieee.org/api/v1/search/articles?querytext={}&max_records={}&start_record={}&format=json&apikey={}",
        encode(&query.text),
        query.per_page,
        query.offset() + 1,
        encode(query.key()),
    );
    let value = searcher.fetch_keyed_json(Source::Ieee, &url, &[], query.key())?;
    // With nothing found, the answer has no `articles` at all.
    let hits = value["articles"].as_array().into_iter().flatten().filter_map(hit).collect();
    Ok(Page { hits, total: value["total_records"].as_u64() })
}

fn hit(article: &Value) -> Option<Hit> {
    let number = string(&article["article_number"])?;
    let title = string(&article["title"]).map(|t| plain_text(&t)).filter(|t| !t.is_empty())?;
    let mut item = Item::new(item_type(article["content_type"].as_str().unwrap_or("")), title);
    item.creators = article["authors"]["authors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["full_name"].as_str())
        .map(Creator::parse)
        .collect();
    item.container_title = string(&article["publication_title"]);
    item.publisher = string(&article["publisher"]);
    item.year = string(&article["publication_year"]).and_then(|y| y.parse().ok());
    item.volume = string(&article["volume"]);
    item.issue = string(&article["issue"]);
    item.pages = page_range(string(&article["start_page"]), string(&article["end_page"]));
    item.doi = article["doi"].as_str().and_then(|d| Doi::parse(d).ok());
    item.isbn = string(&article["isbn"]);
    item.issn = string(&article["issn"]);
    item.url = item
        .doi
        .as_ref()
        .map(|d| format!("https://doi.org/{d}"))
        .or_else(|| string(&article["html_url"]))
        .or_else(|| Some(format!("https://ieeexplore.ieee.org/document/{number}")));
    item.abstract_note = string(&article["abstract"]).map(|a| plain_text(&a));
    let terms = &article["index_terms"];
    let mut keywords: Vec<String> = Vec::new();
    for list in [&terms["author_terms"]["terms"], &terms["ieee_terms"]["terms"]] {
        for term in list.as_array().into_iter().flatten().filter_map(string) {
            if !keywords.iter().any(|k| k.eq_ignore_ascii_case(&term)) {
                keywords.push(term);
            }
        }
    }
    item.keywords = keywords;
    fill_arxiv_id(&mut item);
    let open = article["access_type"].as_str().is_some_and(|a| a.to_ascii_lowercase().contains("open"));
    let pdf_url = string(&article["pdf_url"]).filter(|_| open);
    let cited_by = article["citing_paper_count"].as_u64();
    Some(Hit { key: format!("ieee:{number}"), item, pdf_url, cited_by })
}

fn item_type(content_type: &str) -> ItemType {
    match content_type {
        "Conferences" => ItemType::ConferencePaper,
        "Standards" => ItemType::Standard,
        "Books" => ItemType::BookChapter,
        "Courses" => ItemType::Other,
        _ => ItemType::JournalArticle,
    }
}
