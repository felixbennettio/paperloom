//! Scopus, through Elsevier's Search API, with the person's own key. The
//! standard view gives the first author only; a key with fuller access also
//! brings every author, the abstract and keywords.

use paperloom_http::Http;
use paperloom_model::{Creator, Doi, Item, ItemType};
use serde_json::Value;

use crate::{
    Error, Hit, Page, Query, Result, Searcher, Source, encode, fill_arxiv_id, plain_text, string,
    year_and_date,
};

/// The most the API returns in one page.
const MOST: u32 = 25;

pub(crate) fn search<H: Http>(searcher: &Searcher<H>, query: &Query) -> Result<Page> {
    let url = format!(
        "https://api.elsevier.com/content/search/scopus?query={}&start={}&count={}",
        encode(&query_text(&query.text)),
        query.offset(),
        query.per_page.min(MOST),
    );
    let value =
        searcher.fetch_keyed_json(Source::Scopus, &url, &[("X-ELS-APIKey", query.key())], query.key())?;
    let results = &value["search-results"];
    let entries = results["entry"]
        .as_array()
        .ok_or_else(|| Error::Parse { service: "Scopus", detail: "no entries".into() })?;
    let total = string(&results["opensearch:totalResults"]).and_then(|t| t.parse().ok());
    Ok(Page { hits: entries.iter().filter_map(hit).collect(), total })
}

/// The person's words searched in titles, abstracts and keywords, or their
/// own Scopus query (`AUTHOR-NAME(…)`, `DOI(…)`) as it is.
fn query_text(text: &str) -> String {
    if field_coded(text) { text.to_string() } else { format!("TITLE-ABS-KEY({text})") }
}

/// Whether the text has a Scopus field code: three or more capitals (or
/// hyphens) right before `(`.
fn field_coded(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().filter(|(_, b)| **b == b'(').any(|(i, _)| {
        bytes[..i].iter().rev().take_while(|b| b.is_ascii_uppercase() || **b == b'-').count() >= 3
    })
}

fn hit(entry: &Value) -> Option<Hit> {
    if entry.get("error").is_some() {
        return None;
    }
    let eid = string(&entry["eid"]).or_else(|| string(&entry["dc:identifier"]))?;
    let title = string(&entry["dc:title"]).map(|t| plain_text(&t)).filter(|t| !t.is_empty())?;
    let mut item = Item::new(
        item_type(entry["subtype"].as_str().unwrap_or(""), entry["prism:aggregationType"].as_str()),
        title,
    );
    let authors: Vec<Creator> = entry["author"].as_array().into_iter().flatten().filter_map(author).collect();
    item.creators = if authors.is_empty() {
        entry["dc:creator"].as_str().map(scopus_name).into_iter().collect()
    } else {
        authors
    };
    item.container_title = string(&entry["prism:publicationName"]);
    let (year, date) = entry["prism:coverDate"].as_str().map(year_and_date).unwrap_or_default();
    item.year = year;
    item.date = date;
    item.volume = string(&entry["prism:volume"]);
    item.issue = string(&entry["prism:issueIdentifier"]);
    item.pages = string(&entry["prism:pageRange"]);
    item.doi = entry["prism:doi"].as_str().and_then(|d| Doi::parse(d).ok());
    item.issn = string(&entry["prism:issn"]).or_else(|| string(&entry["prism:eIssn"])).map(|i| issn(&i));
    item.isbn = match &entry["prism:isbn"] {
        Value::Array(all) => all.iter().find_map(|i| string(&i["$"])),
        other => string(other),
    };
    item.pmid = string(&entry["pubmed-id"]);
    let page = entry["link"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|l| l["@ref"].as_str() == Some("scopus"))
        .and_then(|l| string(&l["@href"]));
    item.url = item.doi.as_ref().map(|d| format!("https://doi.org/{d}")).or(page);
    item.abstract_note = string(&entry["dc:description"]).map(|a| plain_text(&a));
    item.keywords = entry["authkeywords"]
        .as_str()
        .map(|k| k.split('|').map(str::trim).filter(|k| !k.is_empty()).map(String::from).collect())
        .unwrap_or_default();
    fill_arxiv_id(&mut item);
    let cited_by = string(&entry["citedby-count"]).and_then(|c| c.parse().ok());
    Some(Hit { key: format!("scopus:{eid}"), item, pdf_url: None, cited_by })
}

fn author(author: &Value) -> Option<Creator> {
    match (string(&author["surname"]), string(&author["given-name"])) {
        (Some(family), Some(given)) => Some(Creator::person(family, given)),
        _ => author["authname"].as_str().map(scopus_name),
    }
}

/// `Smith J.K.` — Scopus puts the initials after the family name.
fn scopus_name(name: &str) -> Creator {
    let name = name.trim();
    match name.rsplit_once(' ') {
        Some((family, initials))
            if initials.contains('.')
                && initials.chars().all(|c| c.is_uppercase() || c == '.' || c == '-') =>
        {
            Creator::person(family.trim(), initials)
        }
        _ => Creator::parse(name),
    }
}

/// `00280836` → `0028-0836`.
fn issn(value: &str) -> String {
    if value.len() == 8 && value.is_ascii() {
        format!("{}-{}", &value[..4], &value[4..])
    } else {
        value.to_string()
    }
}

fn item_type(subtype: &str, aggregation: Option<&str>) -> ItemType {
    match subtype {
        "cp" | "cr" => ItemType::ConferencePaper,
        "ch" => ItemType::BookChapter,
        "bk" => ItemType::Book,
        "dp" => ItemType::Dataset,
        _ => match aggregation {
            Some("Conference Proceeding") => ItemType::ConferencePaper,
            Some("Book") | Some("Book Series") => ItemType::BookChapter,
            _ => ItemType::JournalArticle,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_scopus_names_and_codes() {
        assert_eq!(scopus_name("Kucsko G."), Creator::person("Kucsko", "G."));
        assert_eq!(scopus_name("van der Berg J.-P."), Creator::person("van der Berg", "J.-P."));
        assert_eq!(issn("00280836"), "0028-0836");
        assert!(field_coded("AUTHOR-NAME(smith) AND DOI(10.1/x)"));
        assert!(!field_coded("graphene (oxide)"));
        assert_eq!(query_text("graphene oxide"), "TITLE-ABS-KEY(graphene oxide)");
    }
}
