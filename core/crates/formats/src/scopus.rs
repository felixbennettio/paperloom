//! Scopus CSV export: a header row naming the columns, one row per
//! document, fields quoted when they hold commas, quotes or line breaks.

use paperloom_model::{Creator, CreatorRole, Doi, Item, ItemType};

use crate::Parsed;
use crate::text::{language_code, pages, split_list};

/// Rows of a CSV file; quoted fields may hold commas, `""` and newlines.
fn rows(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            ('"', true) => quoted = false,
            ('"', false) if field.is_empty() => quoted = true,
            (',', false) => row.push(std::mem::take(&mut field)),
            ('\r', false) => {}
            ('\n', false) => {
                row.push(std::mem::take(&mut field));
                if row.iter().any(|f| !f.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            (c, _) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// The first line names Scopus's columns.
pub(crate) fn looks_like(text: &str) -> bool {
    let header = text.lines().next().unwrap_or("").to_lowercase();
    header.contains("title")
        && (header.contains("source title")
            || header.contains("\"authors\"")
            || header.contains("author full names"))
}

pub(crate) fn parse(text: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let mut all = rows(text).into_iter();
    let Some(header) = all.next() else { return parsed };
    let header: Vec<String> = header.iter().map(|h| h.trim().to_lowercase()).collect();
    for row in all {
        let get = |name: &str| {
            header
                .iter()
                .position(|h| h == name)
                .and_then(|i| row.get(i))
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty() && v != "[No abstract available]")
        };
        match build(&get) {
            Some(item) => parsed.items.push(item),
            None => parsed.skipped += 1,
        }
    }
    parsed
}

fn build(get: &dyn Fn(&str) -> Option<String>) -> Option<Item> {
    let title = get("title")?;
    let kind = get("document type").unwrap_or_default().to_lowercase();
    let item_type = match kind.as_str() {
        "conference paper" | "conference review" => ItemType::ConferencePaper,
        "book chapter" => ItemType::BookChapter,
        "book" => ItemType::Book,
        "data paper" => ItemType::Dataset,
        _ => ItemType::JournalArticle,
    };
    let mut item = Item::new(item_type, title);
    item.creators = match get("author full names") {
        // `Kucsko, Georg (6603…); Maurer, Peter C. (…)`
        Some(full) => {
            split_list(&full).iter().map(|n| Creator::parse(n.split(" (").next().unwrap_or(n))).collect()
        }
        None => get("authors").map(|a| short_names(&a)).unwrap_or_default(),
    };
    for editor in get("editors").map(|e| split_list(&e)).unwrap_or_default() {
        item.creators.push(Creator { role: CreatorRole::Editor, ..Creator::parse(&editor) });
    }
    item.year = get("year").and_then(|y| y.parse().ok());
    item.date = item.year.map(|y| y.to_string());
    item.container_title = get("source title");
    item.container_short_title = get("abbreviated source title");
    item.volume = get("volume");
    item.issue = get("issue");
    item.pages = pages(get("page start").as_deref(), get("page end").as_deref()).or_else(|| get("art. no."));
    item.doi = get("doi").and_then(|d| Doi::parse(&d).ok());
    item.url = get("link");
    item.abstract_note = get("abstract");
    item.keywords = get("author keywords").map(|k| split_list(&k)).unwrap_or_default();
    item.issn = get("issn");
    item.isbn = get("isbn").map(|i| split_list(&i).into_iter().next().unwrap_or(i));
    item.publisher = get("publisher");
    item.language = get("language of original document").map(|l| language_code(&l));
    item.pmid = get("pubmed id");
    Some(item)
}

/// `Kucsko G.; Maurer P.C.` (or, in older exports, `Kucsko, G., Maurer, P.C.`).
fn short_names(text: &str) -> Vec<Creator> {
    let names: Vec<String> = if text.contains(';') {
        split_list(text)
    } else {
        text.split(", ").collect::<Vec<_>>().chunks(2).map(|pair| pair.join(", ")).collect()
    };
    names
        .iter()
        .map(|name| match name.rsplit_once(' ').filter(|_| !name.contains(',')) {
            Some((family, initials)) if initials.ends_with('.') => Creator::person(family, initials),
            _ => Creator::parse(name),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{feff}\"Authors\",\"Author full names\",\"Title\",\"Year\",\"Source title\",\"Volume\",\"Issue\",\"Art. No.\",\"Page start\",\"Page end\",\"DOI\",\"Link\",\"Abstract\",\"Author Keywords\",\"ISSN\",\"Language of Original Document\",\"Document Type\"
\"Kucsko G.; Maurer P.C.\",\"Kucsko, Georg (6603); Maurer, Peter C. (7004)\",\"Nanometre-scale thermometry in a living cell\",\"2013\",\"Nature\",\"500\",\"7460\",\"\",\"54\",\"58\",\"10.1038/nature12373\",\"https://www.scopus.com/x\",\"Sensitive probing, \"\"quoted\"\",
on two lines\",\"thermometry; NV centres\",\"00280836\",\"English\",\"Article\"
\"Vaswani A.\",\"\",\"Attention is all you need\",\"2017\",\"NeurIPS\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"[No abstract available]\",\"\",\"\",\"English\",\"Conference Paper\"
\"Nobody\",\"\",\"\",\"2020\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\"
";

    #[test]
    fn reads_rows() {
        assert!(looks_like(SAMPLE.trim_start_matches('\u{feff}')));
        let parsed = parse(SAMPLE.trim_start_matches('\u{feff}'));
        assert_eq!((parsed.items.len(), parsed.skipped), (2, 1));
        let a = &parsed.items[0];
        assert_eq!(a.creators[1], Creator::person("Maurer", "Peter C."));
        assert_eq!(a.pages.as_deref(), Some("54-58"));
        assert_eq!(a.abstract_note.as_deref(), Some("Sensitive probing, \"quoted\",\non two lines"));
        assert_eq!(a.keywords, ["thermometry", "NV centres"]);
        assert_eq!(a.doi.as_ref().map(|d| d.as_str()), Some("10.1038/nature12373"));
        let b = &parsed.items[1];
        assert_eq!(b.item_type, ItemType::ConferencePaper);
        assert_eq!(b.creators, vec![Creator::person("Vaswani", "A.")]);
        assert_eq!(b.abstract_note, None);
    }
}
