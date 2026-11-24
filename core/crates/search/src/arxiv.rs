//! arXiv API, `export.arxiv.org/api/query`: an Atom feed. Every preprint gets
//! its arXiv DOI, `10.48550/arXiv.<id>`.

use paperloom_http::Http;
use paperloom_model::{Creator, Doi, Item, ItemType};

use crate::xml;
use crate::{Hit, Page, Query, Result, Searcher, Source, encode, year_and_date};

pub(crate) fn search<H: Http>(searcher: &Searcher<H>, query: &Query) -> Result<Page> {
    let url = format!(
        "https://export.arxiv.org/api/query?search_query={}&start={}&max_results={}",
        encode(&search_query(&query.text)),
        query.offset(),
        query.per_page,
    );
    let feed = searcher.fetch(Source::Arxiv, &url, "application/atom+xml")?;
    Ok(parse(&feed))
}

/// Words arXiv's index leaves out (stop words, and `all`, which reads as the
/// field), so requiring one of them finds nothing at all.
const SKIPPED: &[&str] = &[
    "a", "all", "an", "and", "andnot", "are", "as", "at", "be", "but", "by", "for", "if", "in", "into", "is",
    "it", "no", "not", "of", "on", "or", "such", "that", "the", "their", "then", "there", "these", "they",
    "this", "to", "was", "will", "with",
];

/// The words as a phrase, or every word that counts somewhere:
/// `all:"deep learning" OR (all:deep AND all:learning)`. Works whose text has
/// the phrase come first.
fn search_query(text: &str) -> String {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|word| word.replace(['"', '(', ')', ':'], ""))
        .filter(|word| !word.is_empty())
        .collect();
    let terms: Vec<String> = words
        .iter()
        .filter(|word| !SKIPPED.contains(&word.to_lowercase().as_str()))
        .map(|word| format!("all:{word}"))
        .collect();
    let phrase = format!("all:\"{}\"", words.join(" "));
    match (words.len(), terms.len()) {
        (0, _) => String::new(),
        (_, 0) => phrase,
        (1, _) => terms.join(""),
        _ => format!("{phrase} OR ({})", terms.join(" AND ")),
    }
}

fn parse(feed: &str) -> Page {
    let total = xml::first_text(feed, "opensearch:totalResults").and_then(|t| t.parse().ok());
    let hits = xml::elements(feed, "entry").iter().filter_map(|entry| hit(entry.body)).collect();
    Page { hits, total }
}

fn hit(entry: &str) -> Option<Hit> {
    // <id>http://arxiv.org/abs/1706.03762v7</id>
    let abs = xml::first_text(entry, "id")?;
    let id = without_version(abs.split_once("/abs/")?.1);
    let mut item = Item::new(ItemType::Preprint, xml::first_text(entry, "title")?);
    item.creators = xml::elements(entry, "author")
        .iter()
        .filter_map(|author| xml::first_text(author.body, "name"))
        .map(|name| Creator::parse(&name))
        .collect();
    if let Some(published) = xml::first_text(entry, "published") {
        (item.year, item.date) = year_and_date(&published);
    }
    item.abstract_note = xml::first_text(entry, "summary");
    item.doi = Doi::parse(&format!("10.48550/arXiv.{id}")).ok();
    item.container_title = Some("arXiv".to_string());
    item.number = Some(format!("arXiv:{id}"));
    item.url = Some(format!("https://arxiv.org/abs/{id}"));
    item.arxiv_id = Some(id.clone());
    let pdf_url = xml::elements(entry, "link")
        .iter()
        .find(|link| xml::attr(link.attrs, "title").as_deref() == Some("pdf"))
        .and_then(|link| xml::attr(link.attrs, "href"));
    Some(Hit { key: format!("arxiv:{id}"), item, pdf_url, cited_by: None })
}

/// `1706.03762v7` → `1706.03762`; `hep-th/9901001v2` → `hep-th/9901001`.
fn without_version(id: &str) -> String {
    match id.rsplit_once('v') {
        Some((base, version))
            if !version.is_empty()
                && version.chars().all(|c| c.is_ascii_digit())
                && base.ends_with(|c: char| c.is_ascii_digit()) =>
        {
            base.to_string()
        }
        _ => id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_versions() {
        assert_eq!(without_version("1706.03762v7"), "1706.03762");
        assert_eq!(without_version("hep-th/9901001v2"), "hep-th/9901001");
        assert_eq!(without_version("solv-int/9901001"), "solv-int/9901001");
        assert_eq!(without_version("2101.00001"), "2101.00001");
    }

    #[test]
    fn asks_for_the_phrase_or_every_word_that_counts() {
        assert_eq!(search_query("deep  (learning)"), "all:\"deep learning\" OR (all:deep AND all:learning)");
        // "is" and "all" are not in arXiv's index; requiring them found nothing.
        assert_eq!(
            search_query("Attention is all you need"),
            "all:\"Attention is all you need\" OR (all:Attention AND all:you AND all:need)"
        );
        assert_eq!(search_query("transformers"), "all:transformers");
        assert_eq!(search_query("the"), "all:\"the\"");
        assert_eq!(search_query("  "), "");
    }
}
