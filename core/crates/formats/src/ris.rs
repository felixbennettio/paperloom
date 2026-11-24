//! RIS: `TY  - JOUR` … `ER  - `, two-letter tags.

use paperloom_model::{Creator, CreatorRole, Doi, Item, ItemType};

use crate::Parsed;
use crate::text::{inverted, iso_date, pages, split_list, year_in};

/// `XX  - value`: two letters or digits, two spaces, a dash.
pub(crate) fn is_tag_line(line: &str) -> bool {
    let b = line.as_bytes();
    b.len() >= 5
        && b[0].is_ascii_uppercase()
        && b[1].is_ascii_alphanumeric()
        && &b[2..4] == b"  "
        && b[4] == b'-'
}

fn item_type(ty: &str) -> ItemType {
    match ty {
        "JOUR" | "JFULL" | "MGZN" | "NEWS" | "EJOUR" => ItemType::JournalArticle,
        "CONF" | "CPAPER" => ItemType::ConferencePaper,
        "BOOK" | "EBOOK" | "EDBOOK" => ItemType::Book,
        "CHAP" | "ECHAP" => ItemType::BookChapter,
        "THES" => ItemType::Thesis,
        "RPRT" => ItemType::Report,
        "UNPB" | "INPR" => ItemType::Preprint,
        "PAT" => ItemType::Patent,
        "STAND" => ItemType::Standard,
        "DATA" | "DBASE" => ItemType::Dataset,
        "ELEC" | "WEB" | "BLOG" => ItemType::Webpage,
        _ => ItemType::Other,
    }
}

fn type_tag(ty: ItemType) -> &'static str {
    match ty {
        ItemType::JournalArticle => "JOUR",
        ItemType::ConferencePaper => "CPAPER",
        ItemType::Book => "BOOK",
        ItemType::BookChapter => "CHAP",
        ItemType::Thesis => "THES",
        ItemType::Report => "RPRT",
        ItemType::Preprint => "UNPB",
        ItemType::Patent => "PAT",
        ItemType::Standard => "STAND",
        ItemType::Dataset => "DATA",
        ItemType::Webpage => "ELEC",
        ItemType::Other => "GEN",
    }
}

pub(crate) fn parse(text: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let mut record: Vec<(String, String)> = Vec::new();
    let mut in_record = false;
    for raw in text.lines() {
        let line = raw.trim_end();
        if is_tag_line(line.trim_start()) {
            let line = line.trim_start();
            let tag = &line[..2];
            let value = line[5..].trim().to_string();
            match tag {
                "TY" => {
                    record.clear();
                    in_record = true;
                    record.push((tag.to_string(), value));
                }
                "ER" => {
                    if in_record {
                        match build(&record) {
                            Some(item) => parsed.items.push(item),
                            None => parsed.skipped += 1,
                        }
                    }
                    record.clear();
                    in_record = false;
                }
                _ if in_record => record.push((tag.to_string(), value)),
                _ => {}
            }
        } else if in_record && !line.trim().is_empty() {
            // A wrapped line continues the previous field (long abstracts).
            if let Some((_, value)) = record.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
        }
    }
    if in_record {
        match build(&record) {
            Some(item) => parsed.items.push(item),
            None => parsed.skipped += 1,
        }
    }
    parsed
}

fn build(record: &[(String, String)]) -> Option<Item> {
    // The first tag in `tags` (by priority) that has a value.
    let first = |tags: &[&str]| -> Option<String> {
        tags.iter()
            .find_map(|tag| record.iter().find(|(t, v)| t == tag && !v.is_empty()).map(|(_, v)| v.clone()))
    };
    let all = |tags: &[&str]| -> Vec<String> {
        record
            .iter()
            .filter(|(t, v)| tags.contains(&t.as_str()) && !v.is_empty())
            .map(|(_, v)| v.clone())
            .collect()
    };
    let item_type = item_type(first(&["TY"]).as_deref().unwrap_or(""));
    let mut item = Item::new(item_type, first(&["TI", "T1", "CT"])?);
    for (tags, role) in [
        (&["AU", "A1"][..], CreatorRole::Author),
        (&["A2", "ED"][..], CreatorRole::Editor),
        (&["A4"][..], CreatorRole::Translator),
    ] {
        for name in all(tags) {
            let mut creator = Creator::parse(&name);
            creator.role = role;
            item.creators.push(creator);
        }
    }
    let full_container = first(&["T2", "JF", "JO", "BT"]);
    let short_container = first(&["JA", "J2", "J1"]);
    item.container_title = full_container.clone().or_else(|| short_container.clone());
    item.container_short_title = short_container.filter(|_| full_container.is_some());
    let date = first(&["DA", "PY", "Y1"]);
    item.year = first(&["PY", "Y1", "DA"]).as_deref().and_then(year_in);
    item.date = date
        .as_deref()
        .and_then(iso_date)
        .filter(|d| d.len() > 4)
        .or_else(|| item.year.map(|y| y.to_string()));
    item.volume = first(&["VL"]);
    item.issue = first(&["IS"]);
    item.pages = pages(first(&["SP"]).as_deref(), first(&["EP"]).as_deref());
    item.doi = first(&["DO"]).and_then(|d| Doi::parse(&d).ok());
    if let Some(sn) = first(&["SN"]) {
        if matches!(item_type, ItemType::Book | ItemType::BookChapter) {
            item.isbn = Some(sn);
        } else {
            item.issn = Some(sn);
        }
    }
    item.url = first(&["UR", "L2"]);
    item.abstract_note = first(&["AB", "N2"]);
    item.keywords = all(&["KW"]).iter().flat_map(|k| split_list(k)).collect();
    item.publisher = first(&["PB"]);
    item.place = first(&["CY", "PP"]);
    item.language = first(&["LA"]);
    item.edition = first(&["ET"]);
    if matches!(item_type, ItemType::Report | ItemType::Patent | ItemType::Standard) {
        item.number = first(&["M1", "SN"]).filter(|_| item.issn.is_none());
    }
    Some(item)
}

pub(crate) fn write(items: &[Item]) -> String {
    let mut out = String::new();
    for item in items {
        let mut line = |tag: &str, value: &str| {
            if !value.trim().is_empty() {
                out.push_str(tag);
                out.push_str("  - ");
                out.push_str(&value.replace(['\r', '\n'], " "));
                out.push_str("\r\n");
            }
        };
        line("TY", type_tag(item.item_type));
        line("TI", &item.title);
        for creator in &item.creators {
            let tag = match creator.role {
                CreatorRole::Author | CreatorRole::Contributor => "AU",
                CreatorRole::Editor => "A2",
                CreatorRole::Translator => "A4",
            };
            line(tag, &inverted(creator));
        }
        let container_tag = match item.item_type {
            ItemType::JournalArticle => "JO",
            _ => "T2",
        };
        line(container_tag, item.container_title.as_deref().unwrap_or(""));
        line("J2", item.container_short_title.as_deref().unwrap_or(""));
        line("PY", &item.year.map(|y| y.to_string()).unwrap_or_default());
        line("DA", &item.date.as_deref().unwrap_or("").replace('-', "/"));
        line("VL", item.volume.as_deref().unwrap_or(""));
        line("IS", item.issue.as_deref().unwrap_or(""));
        if let Some(p) = item.pages.as_deref() {
            match p.split_once(['-', '–']) {
                Some((sp, ep)) => {
                    line("SP", sp.trim());
                    line("EP", ep.trim());
                }
                None => line("SP", p),
            }
        }
        line("PB", item.publisher.as_deref().unwrap_or(""));
        line("CY", item.place.as_deref().unwrap_or(""));
        line("ET", item.edition.as_deref().unwrap_or(""));
        line("M1", item.number.as_deref().unwrap_or(""));
        line("SN", item.isbn.as_deref().or(item.issn.as_deref()).unwrap_or(""));
        line("DO", &item.doi.as_ref().map(|d| d.to_string()).unwrap_or_default());
        line("UR", item.url.as_deref().unwrap_or(""));
        line("AB", item.abstract_note.as_deref().unwrap_or(""));
        for keyword in &item.keywords {
            line("KW", keyword);
        }
        line("LA", item.language.as_deref().unwrap_or(""));
        out.push_str("ER  - \r\n\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As exported by Web of Science / most publishers.
    const SAMPLE: &str = "TY  - JOUR\r\n\
AU  - Kucsko, G.\r\n\
AU  - Maurer, P. C.\r\n\
TI  - Nanometre-scale thermometry in a living cell\r\n\
T2  - Nature\r\n\
JA  - Nature\r\n\
PY  - 2013\r\n\
DA  - 2013/07/31/\r\n\
VL  - 500\r\n\
IS  - 7460\r\n\
SP  - 54\r\n\
EP  - 58\r\n\
SN  - 0028-0836\r\n\
DO  - 10.1038/nature12373\r\n\
AB  - Sensitive probing of temperature\r\n\
  continues on this line.\r\n\
KW  - thermometry\r\n\
KW  - NV centres\r\n\
ER  - \r\n\
\r\n\
TY  - JOUR\r\n\
AU  - 张三\r\n\
AU  - 李四\r\n\
TI  - 基于深度学习的文献分类\r\n\
JO  - 计算机学报\r\n\
PY  - 2021\r\n\
ER  - \r\n\
TY  - JOUR\r\n\
AU  - Nobody\r\n\
ER  - \r\n";

    #[test]
    fn reads_records() {
        let parsed = parse(SAMPLE);
        assert_eq!(parsed.items.len(), 2);
        assert_eq!(parsed.skipped, 1, "the record without a title");
        let a = &parsed.items[0];
        assert_eq!(a.item_type, ItemType::JournalArticle);
        assert_eq!(a.creators[1], Creator::person("Maurer", "P. C."));
        assert_eq!(a.container_title.as_deref(), Some("Nature"));
        assert_eq!(a.year, Some(2013));
        assert_eq!(a.date.as_deref(), Some("2013-07-31"));
        assert_eq!(a.pages.as_deref(), Some("54-58"));
        assert_eq!(a.issn.as_deref(), Some("0028-0836"));
        assert_eq!(a.doi.as_ref().map(|d| d.as_str()), Some("10.1038/nature12373"));
        assert_eq!(
            a.abstract_note.as_deref(),
            Some("Sensitive probing of temperature continues on this line.")
        );
        assert_eq!(a.keywords, ["thermometry", "NV centres"]);
        let b = &parsed.items[1];
        assert_eq!(b.creators, vec![Creator::literal("张三"), Creator::literal("李四")]);
        assert_eq!(b.container_title.as_deref(), Some("计算机学报"));
    }

    #[test]
    fn round_trips() {
        let items = parse(SAMPLE).items;
        let again = parse(&write(&items)).items;
        assert_eq!(again.len(), items.len());
        for (a, b) in items.iter().zip(&again) {
            assert_eq!(a.title, b.title);
            assert_eq!(a.creators, b.creators);
            assert_eq!((&a.pages, &a.doi, &a.year, &a.date), (&b.pages, &b.doi, &b.year, &b.date));
            assert_eq!(a.keywords, b.keywords);
        }
    }

    #[test]
    fn recognises_tag_lines() {
        assert!(is_tag_line("TY  - JOUR"));
        assert!(is_tag_line("A1  - Name"));
        assert!(is_tag_line("ER  -"));
        assert!(!is_tag_line("Title - nope"));
        assert!(!is_tag_line("ty  - lower"));
    }
}
