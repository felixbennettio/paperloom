//! References and in-text citations in CSL styles.
//!
//! Items go through CSL-JSON (`paperloom_model::csl`) into hayagriva, the
//! CSL processor Typst uses, with the styles it ships: GB/T 7714—2015 in all
//! three systems, APA, IEEE, MLA, Chicago, Vancouver, Nature and more, and
//! any CSL style added.

mod added;

pub use added::{StyleFile, add_style, added_styles, has_style, read_style, remove_style, short_name};

use hayagriva::archive::locales;
use hayagriva::citationberg::IndependentStyle;
use hayagriva::citationberg::json::Item as CslItem;
use hayagriva::citationberg::taxonomy::Locator;
use hayagriva::{
    BibliographyDriver, BibliographyItem, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest,
    ElemChildren, LocatorPayload, SpecificLocator,
};
use paperloom_model::{Item, csl};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unknown citation style “{0}”")]
    UnknownStyle(String),
    #[error("not a CSL style: {0}")]
    InvalidStyle(String),
    #[error("this style cites like “{0}”, which has to be added first")]
    MissingParent(String),
    #[error("cannot format “{title}”: {detail}")]
    Item { title: String, detail: String },
}

pub type Result<T> = std::result::Result<T, Error>;

/// A style the apps offer, by hayagriva archive name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleInfo {
    pub id: &'static str,
    /// The style's usual name; not translated (standards keep their names).
    pub name: &'static str,
}

/// The styles offered first, Chinese national standard on top.
pub const STYLES: &[StyleInfo] = &[
    StyleInfo { id: "gb-7714-2015-numeric", name: "GB/T 7714—2015 (顺序编码制)" },
    StyleInfo { id: "gb-7714-2015-author-date", name: "GB/T 7714—2015 (著者-出版年制)" },
    StyleInfo { id: "gb-7714-2015-note", name: "GB/T 7714—2015 (注释)" },
    StyleInfo { id: "apa", name: "APA 7th" },
    StyleInfo { id: "ieee", name: "IEEE" },
    StyleInfo { id: "mla", name: "MLA 9th" },
    StyleInfo { id: "chicago-author-date", name: "Chicago (author-date)" },
    StyleInfo { id: "chicago-notes", name: "Chicago (notes)" },
    StyleInfo { id: "harvard-cite-them-right", name: "Harvard (Cite Them Right)" },
    StyleInfo { id: "vancouver", name: "Vancouver" },
    StyleInfo { id: "nature", name: "Nature" },
    StyleInfo { id: "cell", name: "Cell" },
    StyleInfo { id: "american-chemical-society", name: "ACS" },
    StyleInfo { id: "american-medical-association", name: "AMA" },
    StyleInfo { id: "elsevier-harvard", name: "Elsevier (Harvard)" },
    StyleInfo { id: "springer-basic-author-date", name: "Springer (author-date)" },
    StyleInfo { id: "the-lancet", name: "The Lancet" },
    StyleInfo { id: "association-for-computing-machinery", name: "ACM" },
];

/// Plain text, or HTML with italics and the like kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Text,
    Html,
}

/// The reference list entries for some items, and the in-text citation
/// that cites them all together.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Formatted {
    pub entries: Vec<String>,
    pub citation: String,
}

fn style(id: &str) -> Result<IndependentStyle> {
    added::find(id).ok_or_else(|| Error::UnknownStyle(id.to_string()))
}

fn to_csl(item: &Item, index: usize) -> Result<CslItem> {
    let value = csl::item_to_csl(item, &format!("item-{index}"));
    serde_json::from_value(value)
        .map_err(|e| Error::Item { title: item.title.clone(), detail: e.to_string() })
}

fn write(children: &ElemChildren, format: Format) -> String {
    let mut out = String::new();
    let buf = match format {
        Format::Text => BufWriteFormat::Plain,
        Format::Html => BufWriteFormat::Html,
    };
    // Writing into a String cannot fail.
    let _ = children.write_buf(&mut out, buf);
    out.trim().to_string()
}

/// GB/T 7714 ends a cut author list with 等 for a Chinese work and et al. for
/// any other, but a style has one locale; each entry is mended for its own
/// work's language.
fn mend_et_al(entry: String, item: Option<&Item>, style_id: &str) -> String {
    let Some(item) = item.filter(|_| style_id.starts_with("gb-7714")) else { return entry };
    if item.is_cjk() {
        entry.replace("et al.", "等")
    } else {
        entry.replace("等.", "et al.").replace('等', "et al.")
    }
}

/// One work cited at one place in a document: which of the items, and the
/// page, if given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cite {
    pub index: usize,
    pub page: Option<String>,
}

/// A document's citations, in order, and its reference list, numbered
/// across the whole document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    pub citations: Vec<String>,
    pub entries: Vec<String>,
}

/// A reference list entry as text or HTML, with its label (`[1]`) when the
/// style sets one apart, mended for its work's language.
fn entry_text(entry: &BibliographyItem, items: &[Item], style_id: &str, format: Format) -> String {
    let body = write(&entry.content, format);
    let item =
        entry.key.strip_prefix("item-").and_then(|i| i.parse::<usize>().ok()).and_then(|i| items.get(i));
    let body = mend_et_al(body, item, style_id);
    match &entry.first_field {
        Some(first) => {
            let mut label = String::new();
            let buf = if format == Format::Html { BufWriteFormat::Html } else { BufWriteFormat::Plain };
            let _ = first.write_buf(&mut label, buf);
            format!("{} {body}", label.trim())
        }
        None => body,
    }
}

/// Formats a document's citations (`groups`, each the works cited at one
/// place) and its reference list in the style `style_id`.
pub fn format_document(
    items: &[Item],
    groups: &[Vec<Cite>],
    style_id: &str,
    format: Format,
) -> Result<Document> {
    let style = style(style_id)?;
    let locales = locales();
    let csl_items = items.iter().enumerate().map(|(i, item)| to_csl(item, i)).collect::<Result<Vec<_>>>()?;
    let mut driver = BibliographyDriver::new();
    for group in groups {
        let cited: Vec<_> = group
            .iter()
            .filter_map(|cite| {
                let entry = csl_items.get(cite.index)?;
                let locator = cite
                    .page
                    .as_deref()
                    .map(|page| SpecificLocator(Locator::Page, LocatorPayload::Str(page)));
                Some(CitationItem::with_locator(entry, locator))
            })
            .collect();
        if !cited.is_empty() {
            driver.citation(CitationRequest::from_items(cited, &style, &locales));
        }
    }
    let rendered = driver.finish(BibliographyRequest::new(&style, None, &locales));
    let entries = rendered
        .bibliography
        .map(|bib| bib.items.iter().map(|entry| entry_text(entry, items, style_id, format)).collect())
        .unwrap_or_default();
    let citations = rendered.citations.iter().map(|c| write(&c.citation, format)).collect();
    Ok(Document { citations, entries })
}

/// Formats `items` in the style `style_id` (see [`STYLES`]; any hayagriva
/// archive name works).
pub fn format(items: &[Item], style_id: &str, format: Format) -> Result<Formatted> {
    let style = style(style_id)?;
    let locales = locales();
    let csl_items = items.iter().enumerate().map(|(i, item)| to_csl(item, i)).collect::<Result<Vec<_>>>()?;
    if csl_items.is_empty() {
        return Ok(Formatted::default());
    }

    let mut driver = BibliographyDriver::new();
    driver.citation(CitationRequest::from_items(
        csl_items.iter().map(CitationItem::with_entry).collect(),
        &style,
        &locales,
    ));
    let rendered = driver.finish(BibliographyRequest::new(&style, None, &locales));

    let entries = rendered
        .bibliography
        .map(|bib| bib.items.iter().map(|entry| entry_text(entry, items, style_id, format)).collect())
        .unwrap_or_default();
    let citation = rendered.citations.first().map(|c| write(&c.citation, format)).unwrap_or_default();
    Ok(Formatted { entries, citation })
}

#[cfg(test)]
mod tests;
