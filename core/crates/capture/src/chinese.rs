//! Records the site rules read from CNKI, Wanfang and VIP pages, as items:
//! their names with affiliation marks, `《刊名》`, source lines such as
//! `2025年第19期 7785-7797` or `2026,75(6)`, and their document types.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use paperloom_model::{Creator, Doi, Item, ItemType};
use regex::Regex;
use serde_json::Value;

use crate::{Found, clean, has_cjk, keywords, page_range, parse_date};

type Record = BTreeMap<String, Value>;

pub(crate) fn record(
    site: &str,
    record: &Record,
    page_url: &str,
    detail: bool,
    index: usize,
) -> Option<Found> {
    let title = one(record, "title").map(|t| clean(&t)).filter(|t| !t.is_empty())?;
    let id = one(record, "id");
    let kind = one(record, "kind");
    let dbcode = one(record, "dbcode");
    let item_type = item_type(kind.as_deref(), dbcode.as_deref(), id.as_deref());
    let mut item = Item::new(item_type, title);
    item.creators = names(&many(record, "authors")).iter().map(|n| Creator::parse(n)).collect();
    let source = one(record, "source").map(|s| parse_source(&s)).unwrap_or_default();
    item.container_title =
        one(record, "container").map(|c| unquote(&c)).filter(|c| !c.is_empty()).or(source.container);
    let (date_year, date) = one(record, "date").map(|d| parse_date(&d)).unwrap_or_default();
    item.date = date;
    item.year = one(record, "year").and_then(|y| parse_date(&y).0).or(date_year).or(source.year);
    item.volume = source.volume;
    item.issue = source.issue;
    item.pages = one(record, "pages").and_then(|p| page_range(&p)).or(source.pages);
    item.doi = one(record, "doi").and_then(|d| Doi::parse(&d).ok());
    item.issn = one(record, "issn");
    item.isbn = one(record, "isbn");
    item.abstract_note = one(record, "abstract").map(|a| without_label(&a)).filter(|a| !a.is_empty());
    item.keywords = keywords(&many(record, "keywords"));
    item.publisher = match item_type {
        ItemType::Thesis => one(record, "institution").map(|i| clean(&i)),
        _ => one(record, "publisher"),
    };
    item.place = one(record, "place");
    item.url = one(record, "url")
        .filter(|u| u.starts_with("http"))
        .or_else(|| id.as_deref().and_then(|id| record_page(site, id)))
        .or_else(|| detail.then(|| page_url.to_string()))
        .map(|u| lasting_url(site, &u));
    if item.is_cjk() {
        item.language = Some("zh".into());
    }
    let cited_by = one(record, "citedBy").and_then(|c| c.trim().parse().ok());
    let id = id.or_else(|| item.url.clone()).unwrap_or_else(|| format!("{page_url}#{index}"));
    Some(Found { key: format!("capture:{site}:{id}"), item, pdf_url: None, cited_by })
}

/// A field's text: a string, or the first of a list.
fn one(record: &Record, key: &str) -> Option<String> {
    let text = match record.get(key)? {
        Value::String(s) => s.clone(),
        Value::Array(all) => {
            all.iter().find_map(|v| v.as_str().filter(|s| !s.trim().is_empty()))?.to_string()
        }
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let text = clean(&text);
    (!text.is_empty()).then_some(text)
}

/// A field's texts: each of a list, or the one string.
fn many(record: &Record, key: &str) -> Vec<String> {
    match record.get(key) {
        Some(Value::Array(all)) => all.iter().filter_map(Value::as_str).map(String::from).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// Names from `张三1,2`, `李四;王五`, `+2位作者` and the like: separated,
/// without affiliation marks, without "and others".
fn names(entries: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in entries {
        let parts: Vec<&str> = if has_cjk(entry) {
            entry.split([';', '；', ',', '，', '、']).collect()
        } else {
            entry.split([';', '；']).collect()
        };
        for part in parts {
            let name = clean(part.trim().trim_end_matches(|c: char| {
                c.is_ascii_digit() || c.is_whitespace() || "*†#,，¹²³⁰⁴⁵⁶⁷⁸⁹".contains(c)
            }));
            let others = name == "等" || name.starts_with('+') || name.to_lowercase().starts_with("et al");
            if !name.is_empty() && !others && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// `《无机材料学报》` → `无机材料学报`.
fn unquote(text: &str) -> String {
    clean(text.trim().trim_start_matches(['《', '<']).trim_end_matches(['》', '>']))
}

/// An abstract without its `摘要：` label.
fn without_label(text: &str) -> String {
    let text = text.trim();
    for label in ["摘要", "Abstract", "ABSTRACT"] {
        if let Some(rest) = text.strip_prefix(label) {
            return clean(rest.trim_start_matches([':', '：', ' ']));
        }
    }
    clean(text)
}

/// Where a record's own page is, from its ID, for a site that lists IDs
/// rather than links: Wanfang's `periodical_wlxb202606030`.
fn record_page(site: &str, id: &str) -> Option<String> {
    match site {
        "wanfang" => {
            let (kind, code) = id.split_once('_')?;
            let plain = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
            (plain(kind) && plain(code)).then(|| format!("https://d.wanfangdata.com.cn/{kind}/{code}"))
        }
        _ => None,
    }
}

/// A link that keeps working: VIP signs its links for a while, and the
/// address without the signature is the document's own.
fn lasting_url(site: &str, url: &str) -> String {
    match site {
        "cqvip" => url.split(['?', '#']).next().unwrap_or(url).to_string(),
        _ => url.to_string(),
    }
}

fn item_type(kind: Option<&str>, dbcode: Option<&str>, id: Option<&str>) -> ItemType {
    if let Some(code) = dbcode {
        let code = code.trim().to_ascii_uppercase();
        let by_code = match code.get(..4).unwrap_or(&code) {
            "CJFD" | "CJFQ" | "CJFN" | "CAPJ" | "CJZK" | "CJFR" => Some(ItemType::JournalArticle),
            "CDFD" | "CMFD" | "CDMD" => Some(ItemType::Thesis),
            "CPFD" | "IPFD" | "CPVD" => Some(ItemType::ConferencePaper),
            "SCPD" => Some(ItemType::Patent),
            "SCSF" | "SCHF" | "SMSD" => Some(ItemType::Standard),
            "CYFD" => Some(ItemType::Book),
            "SNAD" => Some(ItemType::Report),
            "CCND" => Some(ItemType::Other),
            _ => None,
        };
        if let Some(found) = by_code {
            return found;
        }
    }
    if let Some(found) = kind.and_then(type_from_label) {
        return found;
    }
    match id.and_then(|id| id.split_once('_')).map(|(kind, _)| kind) {
        Some("thesis") => ItemType::Thesis,
        Some("conference") => ItemType::ConferencePaper,
        Some("patent") => ItemType::Patent,
        Some("standard") => ItemType::Standard,
        Some("nstr" | "cstad") => ItemType::Report,
        _ => ItemType::JournalArticle,
    }
}

/// `期刊论文`, `【硕士论文】`, `会议`, `报纸`…
fn type_from_label(label: &str) -> Option<ItemType> {
    let label = label.trim_matches(|c: char| c.is_whitespace() || "[]【】".contains(c));
    let has = |word: &str| label.contains(word);
    Some(if has("硕士") || has("博士") || has("学位") {
        ItemType::Thesis
    } else if has("会议") {
        ItemType::ConferencePaper
    } else if has("期刊") || label.eq_ignore_ascii_case("journal") {
        ItemType::JournalArticle
    } else if has("专利") {
        ItemType::Patent
    } else if has("标准") {
        ItemType::Standard
    } else if has("图书") || has("年鉴") {
        ItemType::Book
    } else if has("成果") || has("报告") {
        ItemType::Report
    } else if has("报纸") {
        ItemType::Other
    } else {
        return None;
    })
}

/// What a Chinese database's source line says.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Source {
    pub container: Option<String>,
    pub year: Option<i32>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
}

static TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"《([^》]+)》").expect("title pattern"));
/// `2025年19期`, `2021年第5期`, `2020年第45卷第3期`.
static YEAR_ISSUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d{4})\s*年\s*(?:第?\s*([0-9A-Za-z]+)\s*卷\s*)?第?\s*([0-9A-Za-z]+)\s*期")
        .expect("issue pattern")
});
/// `2026,75(6)`, `2023,45(S1)`, `2023(05)`.
static YEAR_VOLUME_ISSUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d{4})\s*(?:[,，]\s*([0-9A-Za-z]+)\s*)?[(（]\s*([0-9A-Za-z+\-]+)\s*[)）]")
        .expect("volume pattern")
});
static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:19|20)\d{2}").expect("year pattern"));

/// `《中国电机工程学报》 • 北大核心 • 2025年19期 7785-7797，I0051，共14页`,
/// `2026,75(6)` and the like.
pub(crate) fn parse_source(text: &str) -> Source {
    let mut source = Source { container: TITLE.captures(text).map(|m| clean(&m[1])), ..Source::default() };
    let found = YEAR_ISSUE.captures(text).or_else(|| YEAR_VOLUME_ISSUE.captures(text));
    match found {
        Some(m) => {
            source.year = m[1].parse().ok();
            source.volume = m.get(2).map(|v| number(v.as_str()));
            source.issue = m.get(3).map(|v| number(v.as_str()));
            // Pages come after the issue; before it, a hyphen may be a date's.
            let rest = &text[m.get(0).map_or(0, |all| all.end())..];
            source.pages = crate::PAGES.captures(rest).map(|p| format!("{}-{}", &p[1], &p[2]));
        }
        None => source.year = YEAR.find(text).and_then(|y| y.as_str().parse().ok()),
    }
    source
}

/// `05` → `5`; `S1` stays.
fn number(text: &str) -> String {
    let trimmed = text.trim_start_matches('0');
    if trimmed.is_empty() || !text.chars().all(|c| c.is_ascii_digit()) {
        text.to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_source_lines() {
        let vip =
            parse_source("《中国电机工程学报》 • 北大核心CSCDCSTPCD • 2025年19期 7785-7797，I0051，共14页");
        assert_eq!(
            vip,
            Source {
                container: Some("中国电机工程学报".into()),
                year: Some(2025),
                volume: None,
                issue: Some("19".into()),
                pages: Some("7785-7797".into()),
            }
        );
        let list = parse_source("2021年第5期 50 - 53， 共4页");
        assert_eq!(
            (list.year, list.issue.as_deref(), list.pages.as_deref()),
            (Some(2021), Some("5"), Some("50-53"))
        );
        let wanfang = parse_source("年,卷(期)：2026,75(6)");
        assert_eq!(
            (wanfang.year, wanfang.volume.as_deref(), wanfang.issue.as_deref()),
            (Some(2026), Some("75"), Some("6"))
        );
        let cnki = parse_source("2023(05)");
        assert_eq!((cnki.year, cnki.volume, cnki.issue.as_deref()), (Some(2023), None, Some("5")));
        let supplement = parse_source("2023,45(S1)");
        assert_eq!(supplement.issue.as_deref(), Some("S1"));
        assert_eq!(parse_source("湖南师范大学 2008").year, Some(2008));
    }

    #[test]
    fn cleans_names() {
        let raw = vec![
            "李俊科 1".to_string(),
            "韩智云1,2".into(),
            "张三;李四".into(),
            "等".into(),
            "+2位作者".into(),
        ];
        assert_eq!(names(&raw), ["李俊科", "韩智云", "张三", "李四"]);
        assert_eq!(names(&["李如峰,李多生,叶寅".to_string()]), ["李如峰", "李多生", "叶寅"]);
        assert_eq!(names(&["Smith, John; Doe, Jane".to_string()]), ["Smith, John", "Doe, Jane"]);
    }

    #[test]
    fn knows_document_types() {
        assert_eq!(item_type(Some("【期刊论文】"), None, None), ItemType::JournalArticle);
        assert_eq!(item_type(Some("硕士"), None, None), ItemType::Thesis);
        assert_eq!(item_type(None, Some("CPFDLAST2024"), None), ItemType::ConferencePaper);
        assert_eq!(item_type(None, None, Some("thesis_Y1472157")), ItemType::Thesis);
        assert_eq!(
            item_type(Some("期刊"), Some("CMFD"), None),
            ItemType::Thesis,
            "the database code decides"
        );
    }
}
