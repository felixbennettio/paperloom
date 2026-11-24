//! PubMed's MEDLINE format (.nbib, "PubMed" export): four-letter tags,
//! `TAG - value`, wrapped lines indented by six spaces, records apart by a
//! blank line.

use paperloom_model::{Creator, Doi, Item, ItemType};

use crate::Parsed;
use crate::text::{date_from_words, language_code, split_list};

/// `PMID- 123`, `TI  - Title`: a tag of up to four characters, padded.
pub(crate) fn is_tag_line(line: &str) -> bool {
    let b = line.as_bytes();
    b.len() >= 6
        && &b[4..6] == b"- "
        && b[..4].iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b' ')
        && b[0].is_ascii_uppercase()
}

pub(crate) fn parse(text: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let mut record: Vec<(String, String)> = Vec::new();
    let finish = |record: &mut Vec<(String, String)>, parsed: &mut Parsed| {
        if !record.is_empty() {
            match build(record) {
                Some(item) => parsed.items.push(item),
                None => parsed.skipped += 1,
            }
            record.clear();
        }
    };
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            finish(&mut record, &mut parsed);
        } else if is_tag_line(line) {
            if line.starts_with("PMID-") {
                finish(&mut record, &mut parsed);
            }
            record.push((line[..4].trim().to_string(), line[6..].trim().to_string()));
        } else if let Some((_, value)) = record.last_mut() {
            value.push(' ');
            value.push_str(line.trim());
        }
    }
    finish(&mut record, &mut parsed);
    parsed
}

fn build(record: &[(String, String)]) -> Option<Item> {
    let first = |tag: &str| record.iter().find(|(t, v)| t == tag && !v.is_empty()).map(|(_, v)| v.clone());
    let all = |tag: &str| -> Vec<String> {
        record.iter().filter(|(t, v)| t == tag && !v.is_empty()).map(|(_, v)| v.clone()).collect()
    };
    let title = first("TI").or_else(|| first("BTI"))?;
    let types = all("PT");
    let item_type = if first("BTI").is_some() && first("TI").is_none() {
        ItemType::Book
    } else if types.iter().any(|t| t == "Preprint") {
        ItemType::Preprint
    } else {
        ItemType::JournalArticle
    };
    let mut item = Item::new(item_type, title.trim_end_matches('.'));
    let full = all("FAU");
    item.creators = if full.is_empty() {
        all("AU").iter().map(|n| short_name(n)).collect()
    } else {
        full.iter().map(|n| Creator::parse(n)).collect()
    };
    item.container_title = first("JT");
    item.container_short_title = first("TA");
    item.volume = first("VI");
    item.issue = first("IP");
    item.pages = first("PG").map(|p| expand_pages(&p));
    if let Some(date) = first("DP") {
        (item.year, item.date) = date_from_words(&date);
    }
    item.doi = all("AID")
        .into_iter()
        .chain(all("LID"))
        .find_map(|id| id.strip_suffix("[doi]").and_then(|d| Doi::parse(d.trim()).ok()));
    item.issn = first("IS").map(|i| i.split(' ').next().unwrap_or(&i).to_string());
    item.pmid = first("PMID");
    item.url = item.pmid.as_ref().map(|id| format!("https://pubmed.ncbi.nlm.nih.gov/{id}/"));
    item.abstract_note = first("AB");
    item.keywords = all("OT").iter().flat_map(|k| split_list(k)).collect();
    item.language = first("LA").map(|l| language_code(&l));
    item.publisher = first("PB");
    Some(item)
}

/// `Kucsko G` → Kucsko, G.
fn short_name(name: &str) -> Creator {
    match name.rsplit_once(' ') {
        Some((family, initials)) if initials.chars().all(|c| c.is_ascii_uppercase()) => {
            let given = initials.chars().map(|c| format!("{c}.")).collect::<Vec<_>>().join(" ");
            Creator::person(family, given)
        }
        _ => Creator::parse(name),
    }
}

/// PubMed shortens end pages: `54-8` → `54-58`, `1123-35` → `1123-1135`.
pub(crate) fn expand_pages(pages: &str) -> String {
    match pages.split_once('-') {
        Some((start, end))
            if end.len() < start.len()
                && start.chars().all(|c| c.is_ascii_digit())
                && end.chars().all(|c| c.is_ascii_digit()) =>
        {
            format!("{start}-{}{end}", &start[..start.len() - end.len()])
        }
        _ => pages.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "PMID- 23903748
OWN - NLM
TI  - Nanometre-scale thermometry in a living cell.
PG  - 54-8
LID - 10.1038/nature12373 [doi]
AB  - Sensitive probing of temperature variations on nanometre scales is an
      outstanding challenge.
FAU - Kucsko, Georg
AU  - Kucsko G
FAU - Lukin, Mikhail D
LA  - eng
PT  - Journal Article
DP  - 2013 Aug 1
TA  - Nature
JT  - Nature
VI  - 500
IP  - 7460
IS  - 1476-4687 (Electronic)
OT  - thermometry
OT  - nitrogen-vacancy centres

PMID- 1
AU  - Nobody N
";

    #[test]
    fn reads_records() {
        let parsed = parse(SAMPLE);
        assert_eq!((parsed.items.len(), parsed.skipped), (1, 1));
        let item = &parsed.items[0];
        assert_eq!(item.title, "Nanometre-scale thermometry in a living cell");
        assert_eq!(item.creators[1], Creator::person("Lukin", "Mikhail D"));
        assert_eq!(item.pages.as_deref(), Some("54-58"));
        assert_eq!(item.date.as_deref(), Some("2013-08-01"));
        assert_eq!(item.doi.as_ref().map(|d| d.as_str()), Some("10.1038/nature12373"));
        assert_eq!(item.issn.as_deref(), Some("1476-4687"));
        assert_eq!(item.language.as_deref(), Some("en"));
        assert_eq!(item.keywords, ["thermometry", "nitrogen-vacancy centres"]);
        assert!(item.abstract_note.as_deref().unwrap().ends_with("outstanding challenge."));
        assert_eq!(item.pmid.as_deref(), Some("23903748"));
    }

    #[test]
    fn expands_pages() {
        assert_eq!(expand_pages("1123-35"), "1123-1135");
        assert_eq!(expand_pages("e123"), "e123");
        assert_eq!(expand_pages("10-20"), "10-20");
    }
}
