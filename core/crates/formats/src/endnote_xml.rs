//! EndNote XML (File → Export → XML): `<records><record>…</record></records>`,
//! with text often wrapped in `<style>` elements.

use paperloom_model::{Creator, CreatorRole, Doi, Item, ItemType};

use crate::Parsed;
use crate::text::{date_from_words, language_code};

/// The bodies of the `name` elements in `xml` (not nested in each other).
fn elements<'a>(xml: &'a str, name: &str) -> Vec<(&'a str, &'a str)> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut found = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        if !after.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            rest = after;
            continue;
        }
        let Some(tag_end) = after.find('>') else { break };
        let attrs = &after[..tag_end];
        let body_start = &after[tag_end + 1..];
        if attrs.ends_with('/') {
            found.push((attrs, ""));
            rest = body_start;
            continue;
        }
        let Some(end) = body_start.find(&close) else { break };
        found.push((attrs, &body_start[..end]));
        rest = &body_start[end + close.len()..];
    }
    found
}

/// Text inside an element: tags dropped, entities decoded, spaces tidied.
fn text(body: &str) -> String {
    let mut plain = String::with_capacity(body.len());
    let mut in_tag = false;
    for c in body.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => plain.push(c),
            _ => {}
        }
    }
    let decoded = plain
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#xD;", "\n")
        .replace("&#13;", "\n")
        .replace("&amp;", "&");
    decoded.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

fn first(xml: &str, path: &[&str]) -> Option<String> {
    let mut scope = xml;
    for name in path {
        scope = elements(scope, name).first()?.1;
    }
    Some(text(scope)).filter(|t| !t.is_empty())
}

fn all(xml: &str, path: &[&str]) -> Vec<String> {
    let Some((last, parents)) = path.split_last() else { return Vec::new() };
    let mut scope = xml;
    for name in parents {
        let Some((_, body)) = elements(scope, name).into_iter().next() else { return Vec::new() };
        scope = body;
    }
    elements(scope, last).into_iter().map(|(_, b)| text(b)).filter(|t| !t.is_empty()).collect()
}

pub(crate) fn looks_like(text: &str) -> bool {
    text.contains("<records>") && text.contains("<record>")
}

pub(crate) fn parse(xml: &str) -> Parsed {
    let mut parsed = Parsed::default();
    for (_, record) in elements(xml, "record") {
        match build(record) {
            Some(item) => parsed.items.push(item),
            None => parsed.skipped += 1,
        }
    }
    parsed
}

/// A record with what EndNote keeps beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct EndNoteRecord {
    pub item: Item,
    /// Attached files as EndNote names them: `internal-pdf://…` (in the
    /// library's `.Data/PDF` folder), `file://…` or a path.
    pub files: Vec<String>,
    pub research_notes: Option<String>,
}

/// A whole EndNote library from its XML export.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EndNoteLibrary {
    pub records: Vec<EndNoteRecord>,
    /// Records with no title.
    pub skipped: usize,
    /// The `.enl` file the export came from, as EndNote wrote it (its
    /// `path`, else its name).
    pub database: Option<String>,
}

fn attribute(attrs: &str, name: &str) -> Option<String> {
    let start = attrs.find(&format!("{name}=\""))? + name.len() + 2;
    let value = &attrs[start..];
    Some(text(&value[..value.find('"')?])).filter(|v| !v.is_empty())
}

/// Reads an EndNote XML export with each record's files and notes.
pub fn read_library(xml: &str) -> EndNoteLibrary {
    let mut library = EndNoteLibrary::default();
    for (_, record) in elements(xml, "record") {
        if library.database.is_none()
            && let Some((attrs, body)) = elements(record, "database").into_iter().next()
        {
            library.database =
                attribute(attrs, "path").or_else(|| Some(text(body)).filter(|n| !n.is_empty()));
        }
        match build(record) {
            Some(item) => library.records.push(EndNoteRecord {
                item,
                files: all(record, &["urls", "pdf-urls", "url"]),
                research_notes: first(record, &["research-notes"]),
            }),
            None => library.skipped += 1,
        }
    }
    library
}

fn build(record: &str) -> Option<Item> {
    let title = first(record, &["titles", "title"])?;
    let kind = elements(record, "ref-type")
        .first()
        .and_then(|(attrs, _)| attrs.split("name=\"").nth(1).and_then(|n| n.split('"').next()))
        .unwrap_or("")
        .to_lowercase();
    let item_type = match kind.as_str() {
        "journal article" | "electronic article" | "magazine article" | "newspaper article" => {
            ItemType::JournalArticle
        }
        "conference proceedings" | "conference paper" => ItemType::ConferencePaper,
        "book" | "edited book" | "electronic book" => ItemType::Book,
        "book section" | "electronic book section" => ItemType::BookChapter,
        "thesis" => ItemType::Thesis,
        "report" | "government document" => ItemType::Report,
        "patent" => ItemType::Patent,
        "standard" => ItemType::Standard,
        "web page" => ItemType::Webpage,
        "dataset" => ItemType::Dataset,
        _ => ItemType::Other,
    };
    let mut item = Item::new(item_type, title);
    item.creators =
        all(record, &["contributors", "authors", "author"]).iter().map(|n| Creator::parse(n)).collect();
    for editor in all(record, &["contributors", "secondary-authors", "author"]) {
        item.creators.push(Creator { role: CreatorRole::Editor, ..Creator::parse(&editor) });
    }
    item.container_title = first(record, &["periodical", "full-title"])
        .or_else(|| first(record, &["titles", "secondary-title"]));
    item.container_short_title = first(record, &["periodical", "abbr-1"]);
    item.volume = first(record, &["volume"]);
    item.issue = first(record, &["number"]);
    item.pages = first(record, &["pages"]).map(|p| p.replace('–', "-"));
    item.edition = first(record, &["edition"]);
    let year = first(record, &["dates", "year"]).unwrap_or_default();
    let day = first(record, &["dates", "pub-dates", "date"]).unwrap_or_default();
    (item.year, item.date) = date_from_words(&format!("{year} {day}"));
    item.publisher = first(record, &["publisher"]);
    item.place = first(record, &["pub-location"]);
    if let Some(number) = first(record, &["isbn"]) {
        if matches!(item_type, ItemType::Book | ItemType::BookChapter) {
            item.isbn = Some(number);
        } else {
            item.issn = Some(number);
        }
    }
    item.doi = first(record, &["electronic-resource-num"]).and_then(|d| Doi::parse(&d).ok());
    item.url = first(record, &["urls", "related-urls", "url"]);
    item.abstract_note = first(record, &["abstract"]);
    item.keywords = all(record, &["keywords", "keyword"]);
    item.language = first(record, &["language"]).map(|l| language_code(&l));
    Some(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8" ?><xml><records>
<record><database name="My Library.enl">My Library.enl</database><ref-type name="Journal Article">17</ref-type>
<contributors><authors><author><style face="normal" font="default" size="100%">Kucsko, G.</style></author><author><style face="normal" font="default" size="100%">Maurer, P. C.</style></author></authors></contributors>
<titles><title><style face="normal" font="default" size="100%">Nanometre-scale thermometry in a living cell</style></title><secondary-title>Nature</secondary-title></titles>
<periodical><full-title>Nature</full-title></periodical><pages>54–58</pages><volume>500</volume><number>7460</number>
<keywords><keyword>thermometry</keyword><keyword>NV &amp; centres</keyword></keywords>
<dates><year>2013</year><pub-dates><date>Jul 31</date></pub-dates></dates>
<isbn>0028-0836</isbn><electronic-resource-num>10.1038/nature12373</electronic-resource-num>
<urls><related-urls><url>https://doi.org/10.1038/nature12373</url></related-urls><pdf-urls><url>internal-pdf://0123456789/Kucsko%202013.pdf</url></pdf-urls></urls>
<research-notes>Read twice; see Fig. 2.</research-notes></record>
<record><ref-type name="Book Section">5</ref-type><titles><title>研究方法</title><secondary-title>论文集</secondary-title></titles><contributors><authors><author>张三</author></authors><secondary-authors><author>李四</author></secondary-authors></contributors><isbn>978-7-111</isbn></record>
<record><ref-type name="Generic">13</ref-type></record>
</records></xml>"#;

    #[test]
    fn reads_records() {
        assert!(looks_like(SAMPLE));
        let parsed = parse(SAMPLE);
        assert_eq!((parsed.items.len(), parsed.skipped), (2, 1));
        let a = &parsed.items[0];
        assert_eq!(a.title, "Nanometre-scale thermometry in a living cell");
        assert_eq!(a.creators, vec![Creator::person("Kucsko", "G."), Creator::person("Maurer", "P. C.")]);
        assert_eq!(a.container_title.as_deref(), Some("Nature"));
        assert_eq!((a.pages.as_deref(), a.date.as_deref()), (Some("54-58"), Some("2013-07-31")));
        assert_eq!(a.keywords, ["thermometry", "NV & centres"]);
        assert_eq!(a.issn.as_deref(), Some("0028-0836"));
        let b = &parsed.items[1];
        assert_eq!(b.item_type, ItemType::BookChapter);
        assert_eq!(b.container_title.as_deref(), Some("论文集"));
        assert_eq!(b.creators[1], Creator { role: CreatorRole::Editor, ..Creator::literal("李四") });
        assert_eq!(b.isbn.as_deref(), Some("978-7-111"));
    }

    #[test]
    fn reads_a_library_with_its_files_and_notes() {
        let library = read_library(SAMPLE);
        assert_eq!((library.records.len(), library.skipped), (2, 1));
        assert_eq!(library.database.as_deref(), Some("My Library.enl"));
        assert_eq!(library.records[0].files, ["internal-pdf://0123456789/Kucsko%202013.pdf"]);
        assert_eq!(library.records[0].research_notes.as_deref(), Some("Read twice; see Fig. 2."));
        assert!(library.records[1].files.is_empty() && library.records[1].research_notes.is_none());
        let with_path = SAMPLE.replace(
            "<database name=\"My Library.enl\">",
            "<database name=\"My Library.enl\" path=\"C:\\Users\\me\\My Library.enl\">",
        );
        assert_eq!(read_library(&with_path).database.as_deref(), Some("C:\\Users\\me\\My Library.enl"));
    }
}
