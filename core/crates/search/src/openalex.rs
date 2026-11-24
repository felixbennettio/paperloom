//! OpenAlex, `/works?search=`: abstracts (as an inverted index), open-access
//! PDF links and citation counts.

use paperloom_http::Http;
use paperloom_model::{Creator, Doi, Item, ItemType};
use serde_json::Value;

use crate::{
    Error, Hit, Page, Query, Result, Searcher, Source, encode, fill_arxiv_id, page_range, string,
    year_and_date,
};

pub(crate) fn search<H: Http>(searcher: &Searcher<H>, query: &Query) -> Result<Page> {
    let url = format!(
        "https://api.openalex.org/works?search={}&per-page={}&page={}",
        encode(&query.text),
        query.per_page,
        query.page + 1,
    );
    let value = searcher.fetch_json(Source::OpenAlex, &url)?;
    let results = value["results"]
        .as_array()
        .ok_or_else(|| Error::Parse { service: "OpenAlex", detail: "no results".into() })?;
    Ok(Page { hits: results.iter().filter_map(hit).collect(), total: value["meta"]["count"].as_u64() })
}

fn hit(work: &Value) -> Option<Hit> {
    let id = work["id"].as_str()?.rsplit('/').next()?.to_string();
    let title = string(&work["title"]).or_else(|| string(&work["display_name"]))?;
    let source = &work["primary_location"]["source"];
    let mut item = Item::new(item_type(work["type"].as_str().unwrap_or(""), source["type"].as_str()), title);
    item.creators = work["authorships"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["author"]["display_name"].as_str())
        .map(Creator::parse)
        .collect();
    let (year, date) = work["publication_date"].as_str().map(year_and_date).unwrap_or_default();
    item.year = year.or_else(|| work["publication_year"].as_i64().map(|y| y as i32));
    item.date = date;
    item.container_title = string(&source["display_name"]);
    item.issn = string(&source["issn_l"]);
    item.publisher = string(&source["host_organization_name"]);
    let biblio = &work["biblio"];
    item.volume = string(&biblio["volume"]);
    item.issue = string(&biblio["issue"]);
    item.pages = page_range(string(&biblio["first_page"]), string(&biblio["last_page"]));
    item.doi = work["doi"].as_str().and_then(|d| Doi::parse(d).ok());
    item.pmid = work["ids"]["pmid"]
        .as_str()
        .and_then(|p| p.trim_end_matches('/').rsplit('/').next())
        .map(String::from);
    item.url = item
        .doi
        .as_ref()
        .map(|d| format!("https://doi.org/{d}"))
        .or_else(|| string(&work["primary_location"]["landing_page_url"]));
    item.abstract_note = abstract_text(&work["abstract_inverted_index"]);
    item.keywords = work["keywords"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|k| string(&k["display_name"]))
        .collect();
    item.language = string(&work["language"]);
    fill_arxiv_id(&mut item);
    let pdf_url =
        string(&work["best_oa_location"]["pdf_url"]).or_else(|| string(&work["primary_location"]["pdf_url"]));
    Some(Hit { key: format!("openalex:{id}"), item, pdf_url, cited_by: work["cited_by_count"].as_u64() })
}

fn item_type(work_type: &str, source_type: Option<&str>) -> ItemType {
    match work_type {
        "article" | "review" | "letter" | "editorial" | "erratum" | "retraction" => match source_type {
            Some("conference") => ItemType::ConferencePaper,
            Some("repository") => ItemType::Preprint,
            _ => ItemType::JournalArticle,
        },
        "preprint" => ItemType::Preprint,
        "book-chapter" | "reference-entry" => ItemType::BookChapter,
        "book" => ItemType::Book,
        "dissertation" => ItemType::Thesis,
        "report" => ItemType::Report,
        "dataset" => ItemType::Dataset,
        "standard" => ItemType::Standard,
        _ => ItemType::Other,
    }
}

/// OpenAlex keeps abstracts as `{word: [positions]}`; this puts the words
/// back in order.
fn abstract_text(index: &Value) -> Option<String> {
    let mut words: Vec<(u64, &str)> = Vec::new();
    for (word, positions) in index.as_object()? {
        for position in positions.as_array()? {
            words.push((position.as_u64()?, word));
        }
    }
    words.sort_unstable_by_key(|(position, _)| *position);
    let text = words.iter().map(|(_, word)| *word).collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}
