//! Reference files in and out.
//!
//! | Format | Read | Write | Where it comes from |
//! | --- | --- | --- | --- |
//! | RIS | ✓ | ✓ | Web of Science, Scopus, PubMed, most publishers, Zotero, EndNote |
//! | EndNote tagged (`%0 …`, .enw / Refer) | ✓ | ✓ | CNKI, Wanfang and VIP "EndNote" exports, EndNote |
//! | BibTeX / BibLaTeX | ✓ | ✓ | Google Scholar, arXiv, DBLP, LaTeX users |
//! | CSL-JSON | ✓ | ✓ | Zotero, Pandoc, citation.js |
//! | MEDLINE (.nbib) | ✓ | | PubMed |
//! | Web of Science plain text (savedrecs.txt) | ✓ | | Web of Science |
//! | Scopus CSV | ✓ | | Scopus |
//! | EndNote XML | ✓ | | EndNote libraries |

mod bibtex;
mod csljson;
mod endnote_xml;
mod enw;
mod medline;
mod ris;
mod scopus;
mod text;
mod wos;

use paperloom_model::Item;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Ris,
    EndNote,
    BibTex,
    CslJson,
    Medline,
    WebOfScience,
    Scopus,
    EndNoteXml,
}

impl Format {
    pub const ALL: [Format; 8] = [
        Format::Ris,
        Format::EndNote,
        Format::BibTex,
        Format::CslJson,
        Format::Medline,
        Format::WebOfScience,
        Format::Scopus,
        Format::EndNoteXml,
    ];

    /// The formats `write` produces; the others are read only.
    pub const WRITABLE: [Format; 4] = [Format::Ris, Format::EndNote, Format::BibTex, Format::CslJson];

    pub fn id(self) -> &'static str {
        match self {
            Format::Ris => "ris",
            Format::EndNote => "enw",
            Format::BibTex => "bibtex",
            Format::CslJson => "csl-json",
            Format::Medline => "medline",
            Format::WebOfScience => "wos",
            Format::Scopus => "scopus",
            Format::EndNoteXml => "endnote-xml",
        }
    }

    pub fn parse_id(id: &str) -> Option<Format> {
        Format::ALL.into_iter().find(|f| f.id() == id)
    }

    /// The usual file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Ris => "ris",
            Format::EndNote => "enw",
            Format::BibTex => "bib",
            Format::CslJson => "json",
            Format::Medline => "nbib",
            Format::WebOfScience => "txt",
            Format::Scopus => "csv",
            Format::EndNoteXml => "xml",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Ris => "RIS",
            Format::EndNote => "EndNote (.enw)",
            Format::BibTex => "BibTeX",
            Format::CslJson => "CSL-JSON",
            Format::Medline => "MEDLINE (PubMed)",
            Format::WebOfScience => "Web of Science",
            Format::Scopus => "Scopus CSV",
            Format::EndNoteXml => "EndNote XML",
        }
    }
}

/// What reading a file found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Parsed {
    pub items: Vec<Item>,
    /// Records that had no title or could not be read.
    pub skipped: usize,
}

/// Guesses the format of a reference file from its content.
pub fn detect(text: &str) -> Option<Format> {
    let text = text.trim_start_matches('\u{feff}');
    let trimmed = text.trim_start();
    if (trimmed.starts_with('[') || trimmed.starts_with('{'))
        && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
    {
        return Some(Format::CslJson);
    }
    if endnote_xml::looks_like(text) {
        return Some(Format::EndNoteXml);
    }
    let lines = || text.lines().map(str::trim_start);
    if lines().any(|l| ris::is_tag_line(l) && l.starts_with("TY")) {
        return Some(Format::Ris);
    }
    if lines().any(|l| medline::is_tag_line(l) && l.starts_with("PMID-")) {
        return Some(Format::Medline);
    }
    if wos::looks_like(text) {
        return Some(Format::WebOfScience);
    }
    if scopus::looks_like(text) {
        return Some(Format::Scopus);
    }
    if lines().any(|l| l.starts_with("%0 ")) {
        return Some(Format::EndNote);
    }
    if lines().any(|l| l.starts_with('@') && l.contains(['{', '('])) {
        return Some(Format::BibTex);
    }
    None
}

/// Reads a reference file in whatever supported format it is in.
pub fn parse(text: &str) -> Option<(Format, Parsed)> {
    let format = detect(text)?;
    Some((format, parse_as(format, text)))
}

pub fn parse_as(format: Format, text: &str) -> Parsed {
    let text = text.trim_start_matches('\u{feff}');
    match format {
        Format::Ris => ris::parse(text),
        Format::EndNote => enw::parse(text),
        Format::BibTex => bibtex::parse(text),
        Format::CslJson => csljson::parse(text),
        Format::Medline => medline::parse(text),
        Format::WebOfScience => wos::parse(text),
        Format::Scopus => scopus::parse(text),
        Format::EndNoteXml => endnote_xml::parse(text),
    }
}

/// Writes items in a format; BibTeX and CSL-JSON get citation keys
/// (`kucsko2013nanometre`). Read-only formats are written as RIS.
pub fn write(format: Format, items: &[Item]) -> String {
    match format {
        Format::Ris | Format::Medline | Format::WebOfScience | Format::Scopus | Format::EndNoteXml => {
            ris::write(items)
        }
        Format::EndNote => enw::write(items),
        Format::BibTex => bibtex::write(items),
        Format::CslJson => csljson::write(items),
    }
}

pub use endnote_xml::{EndNoteLibrary, EndNoteRecord, read_library as read_endnote_library};
pub use text::citation_keys;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_formats() {
        assert_eq!(detect("TY  - JOUR\nTI  - X\nER  - \n"), Some(Format::Ris));
        assert_eq!(detect("\u{feff}%0 Journal Article\n%T 标题\n"), Some(Format::EndNote));
        assert_eq!(detect("% comment\n@article{key,\n title={X}}"), Some(Format::BibTex));
        assert_eq!(detect(r#"[{"type":"book","title":"X"}]"#), Some(Format::CslJson));
        assert_eq!(detect("PMID- 1\nTI  - X\n"), Some(Format::Medline));
        assert_eq!(
            detect("FN Clarivate Analytics Web of Science\nVR 1.0\nPT J\nTI X\nER\n"),
            Some(Format::WebOfScience)
        );
        assert_eq!(
            detect("\"Authors\",\"Title\",\"Year\",\"Source title\"\n\"A\",\"X\",\"2020\",\"S\"\n"),
            Some(Format::Scopus)
        );
        assert_eq!(
            detect("<xml><records><record><titles><title>X</title></titles></record></records></xml>"),
            Some(Format::EndNoteXml)
        );
        assert_eq!(detect("just some text"), None);
    }

    #[test]
    fn format_ids_round_trip() {
        for f in Format::ALL {
            assert_eq!(Format::parse_id(f.id()), Some(f));
        }
    }
}
