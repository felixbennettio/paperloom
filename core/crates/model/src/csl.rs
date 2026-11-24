//! CSL-JSON, the item format of the Citation Style Language: what doi.org
//! content negotiation returns, what citation processors read, and the
//! common ground of most import and export formats.

use serde_json::{Map, Value, json};

use crate::{Creator, CreatorRole, Doi, Item, ItemType, contains_cjk};

/// Maps a CSL item type to ours. doi.org's CSL-JSON for Crossref DOIs
/// sometimes carries Crossref's own type names (`journal-article`), so
/// those count too.
pub fn item_type_from_csl(csl_type: &str) -> ItemType {
    match csl_type {
        "article-journal" | "article-magazine" | "article-newspaper" | "review" | "review-book"
        | "periodical" | "journal-article" => ItemType::JournalArticle,
        "paper-conference" | "event" | "speech" | "proceedings-article" => ItemType::ConferencePaper,
        "book" | "classic" | "collection" | "pamphlet" | "manuscript" | "monograph" | "edited-book"
        | "reference-book" => ItemType::Book,
        "chapter" | "entry" | "entry-dictionary" | "entry-encyclopedia" | "book-chapter" | "book-section"
        | "reference-entry" => ItemType::BookChapter,
        "thesis" | "dissertation" => ItemType::Thesis,
        "posted-content" => ItemType::Preprint,
        "report" => ItemType::Report,
        // Crossref types preprints ("posted-content") as plain "article".
        "article" => ItemType::Preprint,
        "patent" => ItemType::Patent,
        "standard" | "regulation" | "legislation" => ItemType::Standard,
        "dataset" | "software" => ItemType::Dataset,
        "webpage" | "post" | "post-weblog" => ItemType::Webpage,
        _ => ItemType::Other,
    }
}

/// Reads a CSL-JSON item. Returns `None` when it has no title.
pub fn item_from_csl(value: &Value) -> Option<Item> {
    let obj = value.as_object()?;
    let title = text(obj.get("title"))?;
    let item_type = obj.get("type").and_then(Value::as_str).map_or(ItemType::Other, item_type_from_csl);
    let mut item = Item::new(item_type, title);

    for (key, role) in [
        ("author", CreatorRole::Author),
        ("editor", CreatorRole::Editor),
        ("translator", CreatorRole::Translator),
    ] {
        if let Some(names) = obj.get(key).and_then(Value::as_array) {
            item.creators.extend(names.iter().filter_map(|n| creator_from_csl(n, role)));
        }
    }

    item.container_title = text(obj.get("container-title"));
    item.container_short_title =
        text(obj.get("container-title-short")).or_else(|| text(obj.get("journalAbbreviation")));
    if let Some(parts) = ["issued", "published-print", "published-online", "created"]
        .iter()
        .find_map(|k| date_parts(obj.get(*k)))
    {
        item.year = parts.first().copied();
        item.date = Some(format_date(&parts));
    }
    item.volume = text(obj.get("volume"));
    item.issue = text(obj.get("issue"));
    item.pages = text(obj.get("page")).map(|p| p.replace('–', "-"));
    item.publisher = text(obj.get("publisher"));
    item.place = text(obj.get("publisher-place"));
    item.edition = text(obj.get("edition"));
    item.number = text(obj.get("number"));
    item.doi = text(obj.get("DOI")).and_then(|d| Doi::parse(&d).ok());
    item.isbn = text(obj.get("ISBN"));
    item.issn = text(obj.get("ISSN"));
    item.pmid = text(obj.get("PMID"));
    item.url = text(obj.get("URL"));
    item.accessed = date_parts(obj.get("accessed")).map(|p| format_date(&p));
    item.abstract_note = text(obj.get("abstract")).map(|a| strip_markup(&a)).filter(|a| !a.is_empty());
    item.keywords = text(obj.get("keyword"))
        .map(|k| {
            k.split([',', ';', '，', '；'])
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    item.language = text(obj.get("language"));
    Some(item)
}

/// Writes an item as CSL-JSON, with `id` as given (a citation key or the
/// item's ID).
pub fn item_to_csl(item: &Item, id: &str) -> Value {
    let mut obj = Map::new();
    obj.insert("id".into(), id.into());
    obj.insert("type".into(), item.item_type.csl_type().into());
    obj.insert("title".into(), item.title.clone().into());
    for (key, role) in [
        ("author", CreatorRole::Author),
        ("editor", CreatorRole::Editor),
        ("translator", CreatorRole::Translator),
    ] {
        let names: Vec<Value> = item.creators.iter().filter(|c| c.role == role).map(creator_to_csl).collect();
        if !names.is_empty() {
            obj.insert(key.into(), names.into());
        }
    }
    let mut put = |key: &str, value: &Option<String>| {
        if let Some(v) = value.as_deref().filter(|v| !v.is_empty()) {
            obj.insert(key.into(), v.into());
        }
    };
    put("container-title", &item.container_title);
    put("container-title-short", &item.container_short_title);
    put("volume", &item.volume);
    put("issue", &item.issue);
    put("page", &item.pages);
    put("publisher", &item.publisher);
    put("publisher-place", &item.place);
    put("edition", &item.edition);
    put("number", &item.number);
    put("DOI", &item.doi.as_ref().map(|d| d.to_string()));
    put("ISBN", &item.isbn);
    put("ISSN", &item.issn);
    put("PMID", &item.pmid);
    put("URL", &item.url);
    put("abstract", &item.abstract_note);
    put("language", &item.language);
    if !item.keywords.is_empty() {
        obj.insert("keyword".into(), item.keywords.join(", ").into());
    }
    let issued = item.date.as_deref().and_then(parse_date).or_else(|| item.year.map(|y| vec![y]));
    if let Some(parts) = issued {
        obj.insert("issued".into(), json!({ "date-parts": [parts] }));
    }
    if let Some(parts) = item.accessed.as_deref().and_then(parse_date) {
        obj.insert("accessed".into(), json!({ "date-parts": [parts] }));
    }
    Value::Object(obj)
}

fn creator_from_csl(value: &Value, role: CreatorRole) -> Option<Creator> {
    let obj = value.as_object()?;
    let family = text(obj.get("family")).unwrap_or_default();
    let given = text(obj.get("given")).unwrap_or_default();
    let mut creator = if let Some(literal) = text(obj.get("literal")) {
        Creator::literal(literal)
    } else if contains_cjk(&family) || contains_cjk(&given) {
        // 张 + 三 → 张三: CJK names are printed whole, family name first.
        Creator::literal(format!("{family}{given}"))
    } else if family.is_empty() && given.is_empty() {
        return None;
    } else {
        Creator::person(family, given)
    };
    creator.role = role;
    Some(creator)
}

fn creator_to_csl(creator: &Creator) -> Value {
    if !creator.literal.is_empty() {
        json!({ "literal": creator.literal })
    } else if creator.given.is_empty() {
        json!({ "family": creator.family })
    } else {
        json!({ "family": creator.family, "given": creator.given })
    }
}

/// A field as text: strings, numbers, or the first element of an array.
fn text(value: Option<&Value>) -> Option<String> {
    let s = match value? {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(list) => return list.iter().find_map(|v| text(Some(v))),
        _ => return None,
    };
    (!s.is_empty()).then_some(s)
}

/// `{"date-parts": [[2013, 7, 31]]}` (numbers or strings), or a `raw` date.
fn date_parts(value: Option<&Value>) -> Option<Vec<i32>> {
    let obj = value?.as_object()?;
    if let Some(first) = obj.get("date-parts").and_then(Value::as_array).and_then(|a| a.first()) {
        let parts: Vec<i32> = first
            .as_array()?
            .iter()
            .filter_map(|p| p.as_i64().or_else(|| p.as_str().and_then(|s| s.trim().parse().ok())))
            .map(|p| p as i32)
            .collect();
        if !parts.is_empty() {
            return Some(parts);
        }
    }
    obj.get("raw").and_then(Value::as_str).and_then(parse_date)
}

/// `2023`, `2023-05` or `2023-05-12` (also with `/` or `.`) into parts.
pub(crate) fn parse_date(text: &str) -> Option<Vec<i32>> {
    let parts: Vec<i32> = text
        .trim()
        .split(['-', '/', '.'])
        .take(3)
        .map(|p| p.trim().parse::<i32>())
        .collect::<Result<_, _>>()
        .ok()?;
    let year = *parts.first()?;
    (1000..=9999).contains(&year).then_some(parts)
}

fn format_date(parts: &[i32]) -> String {
    match parts {
        [y] => format!("{y:04}"),
        [y, m] => format!("{y:04}-{m:02}"),
        [y, m, d, ..] => format!("{y:04}-{m:02}-{d:02}"),
        [] => String::new(),
    }
}

/// Removes JATS/HTML tags (Crossref abstracts) and decodes basic entities.
fn strip_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let decoded = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_crossref_type_names_as_doi_org_sends_them() {
        assert_eq!(item_type_from_csl("journal-article"), ItemType::JournalArticle);
        assert_eq!(item_type_from_csl("proceedings-article"), ItemType::ConferencePaper);
        assert_eq!(item_type_from_csl("book-chapter"), ItemType::BookChapter);
        assert_eq!(item_type_from_csl("posted-content"), ItemType::Preprint);
        assert_eq!(item_type_from_csl("dissertation"), ItemType::Thesis);
    }

    /// Shortened from https://doi.org/10.1038/nature12373 (Accept: CSL-JSON).
    const CROSSREF: &str = r#"{
        "indexed": {"date-parts": [[2024, 1, 2]]},
        "publisher": "Springer Science and Business Media LLC",
        "issue": "7458",
        "DOI": "10.1038/nature12373",
        "type": "article-journal",
        "page": "541–545",
        "title": "Nanometre-scale thermometry in a living cell",
        "volume": "500",
        "author": [
            {"given": "G.", "family": "Kucsko", "sequence": "first"},
            {"given": "P. C.", "family": "Maurer", "sequence": "additional"},
            {"given": "M. D.", "family": "Lukin", "sequence": "additional"}
        ],
        "container-title": "Nature",
        "container-title-short": "Nature",
        "language": "en",
        "issued": {"date-parts": [[2013, 7, 31]]},
        "URL": "https://doi.org/10.1038/nature12373",
        "ISSN": ["0028-0836", "1476-4687"],
        "abstract": "<jats:p>Sensitive probing of temperature &amp; heat <jats:italic>in vivo</jats:italic>.</jats:p>"
    }"#;

    #[test]
    fn reads_crossref_csl() {
        let item = item_from_csl(&serde_json::from_str(CROSSREF).unwrap()).unwrap();
        assert_eq!(item.item_type, ItemType::JournalArticle);
        assert_eq!(item.title, "Nanometre-scale thermometry in a living cell");
        assert_eq!(item.creators.len(), 3);
        assert_eq!(item.creators[1], Creator::person("Maurer", "P. C."));
        assert_eq!(item.authors_short(), "Kucsko et al.");
        assert_eq!(item.container_title.as_deref(), Some("Nature"));
        assert_eq!(item.year, Some(2013));
        assert_eq!(item.date.as_deref(), Some("2013-07-31"));
        assert_eq!((item.volume.as_deref(), item.issue.as_deref()), (Some("500"), Some("7458")));
        assert_eq!(item.pages.as_deref(), Some("541-545"));
        assert_eq!(item.doi.as_ref().map(|d| d.as_str()), Some("10.1038/nature12373"));
        assert_eq!(item.issn.as_deref(), Some("0028-0836"));
        assert_eq!(item.abstract_note.as_deref(), Some("Sensitive probing of temperature & heat in vivo ."));
    }

    #[test]
    fn keeps_chinese_names_whole() {
        let value = json!({
            "type": "article-journal", "title": "基于深度学习的文献分类",
            "author": [{"family": "张", "given": "三"}, {"literal": "中国科学院"}, {"family": "", "given": ""}],
            "issued": {"date-parts": [["2021", "3"]]}, "volume": 12
        });
        let item = item_from_csl(&value).unwrap();
        assert_eq!(item.creators, vec![Creator::literal("张三"), Creator::literal("中国科学院")]);
        assert_eq!(item.date.as_deref(), Some("2021-03"));
        assert_eq!(item.volume.as_deref(), Some("12"));
    }

    #[test]
    fn needs_a_title() {
        assert!(item_from_csl(&json!({"type": "book"})).is_none());
        assert!(item_from_csl(&json!("not an object")).is_none());
    }

    #[test]
    fn round_trips() {
        let mut item = item_from_csl(&serde_json::from_str(CROSSREF).unwrap()).unwrap();
        item.creators.push(Creator { role: CreatorRole::Editor, ..Creator::literal("WHO") });
        item.keywords = vec!["thermometry".into(), "NV centres".into()];
        item.accessed = Some("2026-10-11".into());
        let csl = item_to_csl(&item, "kucsko2013");
        assert_eq!(csl["id"], "kucsko2013");
        assert_eq!(csl["issued"]["date-parts"][0], json!([2013, 7, 31]));
        assert_eq!(csl["editor"][0]["literal"], "WHO");
        let back = item_from_csl(&csl).unwrap();
        assert_eq!(back.creators, item.creators);
        assert_eq!(
            (&back.title, &back.container_title, &back.pages),
            (&item.title, &item.container_title, &item.pages)
        );
        assert_eq!(
            (back.date, back.doi, back.keywords, back.accessed),
            (item.date, item.doi, item.keywords, item.accessed)
        );
    }

    #[test]
    fn maps_types() {
        assert_eq!(item_type_from_csl("paper-conference"), ItemType::ConferencePaper);
        assert_eq!(item_type_from_csl("chapter"), ItemType::BookChapter);
        assert_eq!(item_type_from_csl("article"), ItemType::Preprint);
        assert_eq!(item_type_from_csl("musical_score"), ItemType::Other);
        for t in ItemType::ALL {
            if t != ItemType::Other {
                assert_eq!(item_type_from_csl(t.csl_type()), t, "{t:?}");
            }
        }
    }

    #[test]
    fn parses_dates() {
        assert_eq!(parse_date("2023"), Some(vec![2023]));
        assert_eq!(parse_date("2023/5/12"), Some(vec![2023, 5, 12]));
        assert_eq!(parse_date("May 2023"), None);
        assert_eq!(parse_date("23"), None);
    }
}
