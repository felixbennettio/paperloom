//! Web of Science "Plain text" export (savedrecs.txt): two-letter tags at
//! the start of a line, more values on lines indented by three spaces, `ER`
//! after each record.

use paperloom_model::{Creator, Doi, Item, ItemType};

use crate::Parsed;
use crate::text::{date_from_words, language_code, pages, split_list, title_case_if_shouting};

fn tag_of(line: &str) -> Option<&str> {
    let b = line.as_bytes();
    (b.len() >= 2
        && b[0].is_ascii_uppercase()
        && (b[1].is_ascii_uppercase() || b[1].is_ascii_digit())
        && (b.len() == 2 || b[2] == b' '))
        .then(|| &line[..2])
}

/// A file starts with `FN` (Clarivate's header) or has `PT` records ending
/// in `ER`.
pub(crate) fn looks_like(text: &str) -> bool {
    let mut lines = text.lines().map(str::trim_end).filter(|l| !l.is_empty());
    if lines.next().is_some_and(|l| l.starts_with("FN ")) {
        return true;
    }
    text.lines().any(|l| l.starts_with("PT ")) && text.lines().any(|l| l.trim_end() == "ER")
}

pub(crate) fn parse(text: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let mut record: Vec<(String, Vec<String>)> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line == "ER" {
            if !record.is_empty() {
                match build(&record) {
                    Some(item) => parsed.items.push(item),
                    None => parsed.skipped += 1,
                }
            }
            record.clear();
        } else if let Some(more) = line.strip_prefix("   ") {
            if let Some((_, values)) = record.last_mut() {
                values.push(more.trim().to_string());
            }
        } else if let Some(tag) = tag_of(line) {
            if matches!(tag, "FN" | "VR" | "EF") {
                continue;
            }
            record.push((tag.to_string(), vec![line[2..].trim().to_string()]));
        }
    }
    parsed
}

fn build(record: &[(String, Vec<String>)]) -> Option<Item> {
    let values =
        |tag: &str| record.iter().find(|(t, _)| t == tag).map(|(_, v)| v.clone()).unwrap_or_default();
    // Fields that wrap long text: the lines belong together.
    let joined = |tag: &str| Some(values(tag).join(" ")).filter(|v| !v.is_empty());
    let title = joined("TI")?;
    let document_type = joined("DT").unwrap_or_default().to_lowercase();
    let publication_type = joined("PT").unwrap_or_default();
    let item_type = if document_type.contains("proceedings") || document_type.contains("meeting") {
        ItemType::ConferencePaper
    } else if document_type.contains("book chapter") {
        ItemType::BookChapter
    } else if publication_type == "B" || document_type == "book" {
        ItemType::Book
    } else if publication_type == "P" || document_type.contains("patent") {
        ItemType::Patent
    } else {
        ItemType::JournalArticle
    };
    let mut item = Item::new(item_type, title);
    let full = values("AF");
    let names = if full.is_empty() { values("AU") } else { full };
    item.creators = names.iter().map(|n| Creator::parse(n)).collect();
    for editor in values("BE") {
        let mut creator = Creator::parse(&editor);
        creator.role = paperloom_model::CreatorRole::Editor;
        item.creators.push(creator);
    }
    item.container_title = joined("SO").map(|s| title_case_if_shouting(&s));
    item.container_short_title = joined("JI").or_else(|| joined("J9"));
    item.volume = joined("VL");
    item.issue = joined("IS");
    item.pages = pages(joined("BP").as_deref(), joined("EP").as_deref()).or_else(|| joined("AR"));
    let year = joined("PY");
    let date_words = format!("{} {}", year.clone().unwrap_or_default(), joined("PD").unwrap_or_default());
    (item.year, item.date) = date_from_words(&date_words);
    item.doi = joined("DI").and_then(|d| Doi::parse(&d).ok());
    item.abstract_note = joined("AB");
    item.keywords = split_list(&values("DE").join("; "));
    item.issn = joined("SN").or_else(|| joined("EI"));
    item.isbn = joined("BN");
    item.publisher = joined("PU").map(|p| title_case_if_shouting(&p));
    item.place = joined("PI").map(|p| title_case_if_shouting(&p));
    item.language = joined("LA").map(|l| language_code(&l));
    item.pmid = joined("PM");
    Some(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "FN Clarivate Analytics Web of Science
VR 1.0
PT J
AU Kucsko, G
   Maurer, PC
AF Kucsko, G.
   Maurer, P. C.
TI Nanometre-scale thermometry in a living
   cell
SO NATURE
JI Nature
DT Article
DE thermometry; NV centres
AB Sensitive probing of temperature.
SN 0028-0836
PD AUG 1
PY 2013
VL 500
IS 7460
BP 54
EP 58
DI 10.1038/nature12373
LA English
ER

PT J
AU Nobody
ER

EF
";

    #[test]
    fn reads_records() {
        assert!(looks_like(SAMPLE));
        let parsed = parse(SAMPLE);
        assert_eq!((parsed.items.len(), parsed.skipped), (1, 1));
        let item = &parsed.items[0];
        assert_eq!(item.title, "Nanometre-scale thermometry in a living cell");
        assert_eq!(item.creators, vec![Creator::person("Kucsko", "G."), Creator::person("Maurer", "P. C.")]);
        assert_eq!(item.container_title.as_deref(), Some("Nature"));
        assert_eq!(item.date.as_deref(), Some("2013-08-01"));
        assert_eq!(item.pages.as_deref(), Some("54-58"));
        assert_eq!(item.keywords, ["thermometry", "NV centres"]);
        assert_eq!(item.language.as_deref(), Some("en"));
        assert_eq!(item.doi.as_ref().map(|d| d.as_str()), Some("10.1038/nature12373"));
    }
}
