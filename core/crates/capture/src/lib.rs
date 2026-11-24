//! Reads works from the web pages a person browses in the app.
//!
//! The apps run [`script`] in the page. It reads the page's embedded
//! metadata and, on a site the rules know (CNKI, Wanfang, VIP), the works
//! the page lists or shows, and answers JSON: a [`Capture`]. [`read`] turns
//! that into items: the site's records first, then Highwire `citation_*`,
//! Dublin Core and PRISM tags, COinS and JSON-LD; failing all of those, the
//! page itself as a web page.
//!
//! The rules are data (`core/rules/sites.json`), so a site that changes its
//! pages needs new rules, not a new app. Nothing here goes around a site's
//! sign-in or its limits: the page is one the person opened, with their own
//! access, and the script only reads it.

mod chinese;
mod embedded;
pub use embedded::absolute as absolute_url;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use paperloom_model::{Creator, Item, ItemType};
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

const EXTRACT: &str = include_str!("extract.js");
const RULES: &str = include_str!("../../../rules/sites.json");

/// What the script answers about a page.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Capture {
    pub url: String,
    pub title: String,
    pub lang: Option<String>,
    /// The site the rules matched, if any.
    pub site: Option<String>,
    /// `list` or `detail`, when a site's page matched.
    pub page: Option<String>,
    /// `<meta>` names (or properties) and contents.
    pub meta: Vec<(String, String)>,
    /// The `title` of each COinS span.
    pub coins: Vec<String>,
    pub json_ld: Vec<String>,
    /// What the site's rules read: one per work.
    pub records: Vec<BTreeMap<String, Value>>,
}

/// A work found on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// The same each time the same work is found.
    pub key: String,
    pub item: Item,
    pub pdf_url: Option<String>,
    pub cited_by: Option<u64>,
}

/// A site the rules know, for a list of places to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub id: String,
    pub name: String,
    pub home: String,
}

/// The script to run in a page: an expression whose value is the JSON of a
/// [`Capture`].
pub fn script() -> String {
    format!("{}({})", EXTRACT.trim_end(), RULES.trim())
}

/// The sites the rules know.
pub fn sites() -> Vec<Site> {
    let rules: Value = serde_json::from_str(RULES).unwrap_or_default();
    rules["sites"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|site| {
            Some(Site {
                id: site["id"].as_str()?.to_string(),
                name: site["name"].as_str()?.to_string(),
                home: site["home"].as_str()?.to_string(),
            })
        })
        .collect()
}

/// The works on a page the script read; `today` (`YYYY-MM-DD`) is when a
/// web page was looked at.
pub fn read(capture: &Capture, today: &str) -> Vec<Found> {
    let page_url = capture.url.trim();
    let tagged = embedded::from_meta(&capture.meta, page_url);
    let mut found: Vec<Found> = Vec::new();
    if let Some(site) = capture.site.as_deref() {
        let detail = capture.page.as_deref() == Some("detail");
        for (index, record) in capture.records.iter().enumerate() {
            found.extend(chinese::record(site, record, page_url, detail, index));
        }
        // On a work's own page, its tags fill in what the rules missed.
        if detail && let ([one], Some((item, pdf_url))) = (found.as_mut_slice(), &tagged) {
            fill_missing(&mut one.item, item);
            if one.pdf_url.is_none() {
                one.pdf_url.clone_from(pdf_url);
            }
        }
    }
    if found.is_empty()
        && let Some((item, pdf_url)) = tagged
    {
        found.push(Found { key: key_for(&item, page_url, ""), item, pdf_url, cited_by: None });
    }
    let others = capture
        .coins
        .iter()
        .map(String::as_str)
        .filter_map(embedded::from_coins)
        .map(|item| (item, "coins"))
        .chain(
            capture
                .json_ld
                .iter()
                .map(String::as_str)
                .flat_map(embedded::from_json_ld)
                .map(|item| (item, "ld")),
        );
    for (index, (item, kind)) in others.enumerate() {
        if !found.iter().any(|f| same_work(&f.item, &item)) {
            let key = key_for(&item, page_url, &format!("#{kind}{index}"));
            found.push(Found { key, item, pdf_url: None, cited_by: None });
        }
    }
    if found.is_empty()
        && let Some(page) = web_page(capture, today)
    {
        found.push(page);
    }
    found
}

/// [`read`] from the script's JSON.
pub fn read_json(json: &str, today: &str) -> Result<Vec<Found>, serde_json::Error> {
    Ok(read(&serde_json::from_str(json)?, today))
}

/// Today's date (UTC), `YYYY-MM-DD`.
pub fn today() -> String {
    let days = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400);
    let (year, month, day) = civil_date(days as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Days since 1970-01-01 as a calendar date (Howard Hinnant's algorithm).
fn civil_date(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `capture:doi:<doi>` for a work with a DOI, otherwise the page (and
/// where on it).
fn key_for(item: &Item, page_url: &str, place: &str) -> String {
    match &item.doi {
        Some(doi) => format!("capture:doi:{doi}"),
        None => format!("capture:{page_url}{place}"),
    }
}

fn same_work(a: &Item, b: &Item) -> bool {
    match (&a.doi, &b.doi) {
        (Some(x), Some(y)) => x == y,
        _ => a.title.to_lowercase() == b.title.to_lowercase(),
    }
}

/// Fills in what `item` lacks from `other`, a description of the same work.
pub fn fill_missing(item: &mut Item, other: &Item) {
    if item.creators.is_empty() {
        item.creators.clone_from(&other.creators);
    }
    if item.keywords.is_empty() {
        item.keywords.clone_from(&other.keywords);
    }
    let pairs = [
        (&mut item.container_title, &other.container_title),
        (&mut item.container_short_title, &other.container_short_title),
        (&mut item.date, &other.date),
        (&mut item.volume, &other.volume),
        (&mut item.issue, &other.issue),
        (&mut item.pages, &other.pages),
        (&mut item.publisher, &other.publisher),
        (&mut item.place, &other.place),
        (&mut item.number, &other.number),
        (&mut item.isbn, &other.isbn),
        (&mut item.issn, &other.issn),
        (&mut item.pmid, &other.pmid),
        (&mut item.url, &other.url),
        (&mut item.abstract_note, &other.abstract_note),
        (&mut item.language, &other.language),
    ];
    for (mine, theirs) in pairs {
        if mine.is_none() {
            mine.clone_from(theirs);
        }
    }
    if item.year.is_none() {
        item.year = other.year;
    }
    if item.doi.is_none() {
        item.doi.clone_from(&other.doi);
    }
}

/// The page itself, when it names no work: its title, site, description
/// and when it was read.
fn web_page(capture: &Capture, today: &str) -> Option<Found> {
    let tag = |name: &str| {
        capture
            .meta
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| clean(v))
            .filter(|v| !v.is_empty())
    };
    let title = tag("og:title").or_else(|| tag("twitter:title")).or_else(|| {
        let title = clean(&capture.title);
        (!title.is_empty()).then_some(title)
    })?;
    let mut item = Item::new(ItemType::Webpage, title);
    let url =
        tag("og:url").filter(|u| u.starts_with("http")).unwrap_or_else(|| capture.url.trim().to_string());
    item.container_title = tag("og:site_name");
    item.abstract_note = tag("og:description").or_else(|| tag("description"));
    let published = tag("article:published_time").or_else(|| tag("og:article:published_time"));
    (item.year, item.date) = published.map(|d| parse_date(&d)).unwrap_or_default();
    item.creators =
        tag("author").filter(|a| !a.contains("://")).map(|a| Creator::parse(&a)).into_iter().collect();
    item.accessed = Some(today.to_string());
    item.language = capture.lang.as_deref().map(clean).filter(|l| !l.is_empty());
    item.url = (!url.is_empty()).then(|| url.clone());
    Some(Found { key: format!("capture:{url}"), item, pdf_url: None, cited_by: None })
}

/// Whitespace collapsed to single spaces.
fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn has_cjk(text: &str) -> bool {
    text.chars().any(|c| matches!(c as u32, 0x3400..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FFFF))
}

static DATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d{4})(?:\s*[-/.年]\s*(\d{1,2})(?:\s*[-/.月]\s*(\d{1,2}))?)?").expect("date pattern")
});

/// `2026-03-20 00:00:00`, `2021/5/20`, `2021年5月20日`, `2013`: the year,
/// and the date as far as it goes (`2021-05-20`, `2021-05`).
fn parse_date(text: &str) -> (Option<i32>, Option<String>) {
    let Some(m) = DATE.captures(text) else { return (None, None) };
    let year: Option<i32> = m[1].parse().ok().filter(|y| (1000..=2999).contains(y));
    let month = m.get(2).and_then(|v| v.as_str().parse::<u32>().ok()).filter(|v| (1..=12).contains(v));
    let day = m.get(3).and_then(|v| v.as_str().parse::<u32>().ok()).filter(|v| (1..=31).contains(v));
    let date = match (year, month, day) {
        (Some(y), Some(mo), Some(d)) => Some(format!("{y:04}-{mo:02}-{d:02}")),
        (Some(y), Some(mo), None) => Some(format!("{y:04}-{mo:02}")),
        _ => None,
    };
    (year, date)
}

static PAGES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Za-z]?\d+)\s*[-–—~]\s*([A-Za-z]?\d+)").expect("pages pattern"));

/// `387-402`, `50 - 53`, `e1001`: a page range or a single page.
fn page_range(text: &str) -> Option<String> {
    if let Some(m) = PAGES.captures(text) {
        return Some(format!("{}-{}", &m[1], &m[2]));
    }
    let text = text.trim();
    let single = !text.is_empty() && text.len() <= 12 && text.chars().all(|c| c.is_ascii_alphanumeric());
    single.then(|| text.to_string())
}

/// Pages from a first and a last one.
fn join_pages(first: Option<String>, last: Option<String>) -> Option<String> {
    match (first, last) {
        (Some(first), Some(last)) if first != last => Some(format!("{first}-{last}")),
        (Some(first), _) => Some(first),
        (None, last) => last,
    }
}

/// Keywords from `a; b; c` (or `a, b` when no semicolons), each list entry
/// split the same way.
fn keywords(entries: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in entries {
        let parts: Vec<&str> = if entry.contains([';', '；']) {
            entry.split([';', '；']).collect()
        } else {
            entry.split([',', '，', '、']).collect()
        };
        for part in parts {
            let keyword = clean(part.trim_matches(|c: char| c.is_whitespace() || ";；,，、.。".contains(c)));
            if !keyword.is_empty() && !out.iter().any(|k| k.eq_ignore_ascii_case(&keyword)) {
                out.push(keyword);
            }
        }
    }
    out
}
