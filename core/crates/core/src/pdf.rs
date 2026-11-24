//! Adding a PDF: find out which paper it is, fill in its details, and keep
//! the file with the item.
//!
//! The apps read the first pages' text and the PDF's own title with their
//! PDF engines (PDFKit, pdf.js) and pass them in. The paper is recognised
//! by, in order: the first DOI printed there, an `arXiv:` stamp, or a
//! Crossref work with the same title. Failing all three, the item is named
//! after the PDF's title or file name, ready to be filled in by hand.

use std::path::Path;

use paperloom_metadata::{Identifier, Resolver};
use paperloom_model::{CollectionId, Doi, Item, ItemType, contains_cjk};
use paperloom_search::{Query, Searcher, Source};
use serde::{Deserialize, Serialize};

use crate::files::AttachmentInfo;
use crate::{CoreError, ItemSummary, PaperLibrary};

/// What adding a PDF did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct PdfImport {
    pub item: ItemSummary,
    pub attachment: AttachmentInfo,
    /// How the paper was recognised: `doi`, `arxiv`, `title` or `none`.
    pub matched_by: String,
    /// True when the paper was already in the library; the PDF joined it.
    pub existing: bool,
    /// Why the details could not be looked up, when they could not.
    pub note: Option<String>,
}

struct Recognised {
    item: Option<Item>,
    doi: Option<Doi>,
    arxiv: Option<String>,
    matched_by: &'static str,
    note: Option<String>,
}

pub(crate) fn import_pdf(
    library: &PaperLibrary,
    path: &str,
    first_pages: &str,
    pdf_title: Option<String>,
    collection_id: Option<String>,
) -> Result<PdfImport, CoreError> {
    let files = library.files()?;
    let source = Path::new(path);
    if !source.is_file() {
        return Err(CoreError::NotFound { message: format!("{path} is not a file.") });
    }
    let pdf_title =
        pdf_title.map(|t| t.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|t| plausible_title(t));
    let found = recognise(first_pages, pdf_title.as_deref());
    let doi = found.item.as_ref().and_then(|i| i.doi.clone()).or(found.doi);

    let mut store = library.lock()?;
    let existing = match &doi {
        Some(doi) => store.find_by_doi(doi)?,
        None => None,
    };
    let is_existing = existing.is_some();
    let item = match existing {
        Some(item) => item,
        None => {
            let item = found.item.unwrap_or_else(|| {
                let title = pdf_title.unwrap_or_else(|| file_stem(source));
                let kind = if found.arxiv.is_some() { ItemType::Preprint } else { ItemType::JournalArticle };
                let mut item = Item::new(kind, title);
                item.doi = doi;
                item.arxiv_id = found.arxiv;
                item
            });
            store.add_item(&item)?;
            item
        }
    };
    if let Some(collection) = collection_id.map(CollectionId::from_string) {
        store.add_to_collection(&collection, &item.id)?;
    }
    let attachment = files.attach(&mut store, &item.id, source)?;
    drop(store);
    library.reindex_items(std::slice::from_ref(&item.id));
    Ok(PdfImport {
        item: ItemSummary::from(&item),
        attachment: files.info(&attachment),
        matched_by: found.matched_by.to_string(),
        existing: is_existing,
        note: found.note,
    })
}

/// Looks the paper up. Talks to the network.
fn recognise(first_pages: &str, pdf_title: Option<&str>) -> Recognised {
    if let Some(id) = Identifier::find_in_text(first_pages) {
        let (doi, arxiv) = match &id {
            Identifier::Doi(doi) => (Some(doi.clone()), None),
            Identifier::Arxiv(arxiv) => (None, Some(arxiv.clone())),
            _ => (None, None),
        };
        let (item, note) = match Resolver::new().resolve(&id) {
            Ok(item) => (Some(item), None),
            Err(e) => (None, Some(e.to_string())),
        };
        return Recognised { item, doi, arxiv, matched_by: id.kind(), note };
    }
    if let Some(item) = pdf_title.and_then(match_title) {
        return Recognised { item: Some(item), doi: None, arxiv: None, matched_by: "title", note: None };
    }
    Recognised { item: None, doi: None, arxiv: None, matched_by: "none", note: None }
}

/// Crossref's work with this title, if one of its best matches has it.
fn match_title(title: &str) -> Option<Item> {
    let query = Query { per_page: 3, ..Query::new(title) };
    let page = Searcher::new().search(Source::Crossref, &query).ok()?;
    page.hits.into_iter().map(|hit| hit.item).find(|item| same_title(&item.title, title))
}

/// Letters and digits only, lowercased: `Attention Is All You Need!` →
/// `attentionisallyouneed`.
fn letters(title: &str) -> String {
    title.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The same title, or one is the other plus a subtitle.
fn same_title(a: &str, b: &str) -> bool {
    let (a, b) = (letters(a), letters(b));
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    !short.is_empty() && (short == long || (short.chars().count() >= 20 && long.starts_with(&short)))
}

/// Whether a PDF's own title is worth using: word processors and LaTeX
/// often leave file names or placeholders there.
fn plausible_title(title: &str) -> bool {
    let lower = title.to_lowercase();
    let words = title.split_whitespace().count();
    let long_enough = words >= 3 || (contains_cjk(title) && title.chars().count() >= 6);
    long_enough
        && !lower.starts_with("microsoft word")
        && !lower.starts_with("untitled")
        && ![".pdf", ".doc", ".docx", ".tex", ".dvi", ".indd"].iter().any(|ext| lower.ends_with(ext))
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "PDF".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_titles_loosely() {
        assert!(same_title("Attention Is All You Need", "Attention is all you need."));
        assert!(same_title(
            "Nanometre-scale thermometry in a living cell",
            "Nanometre-scale thermometry in a living cell: a quantum sensor"
        ));
        assert!(
            !same_title("Deep learning", "Deep learning for everything"),
            "too short to be a prefix match"
        );
        assert!(!same_title("", ""));
        assert!(same_title("基于深度学习的文献分类", "基于深度学习的文献分类"));
    }

    #[test]
    fn skips_placeholder_titles() {
        assert!(plausible_title("Nanometre-scale thermometry in a living cell"));
        assert!(plausible_title("基于深度学习的文献分类研究"));
        assert!(!plausible_title("Microsoft Word - draft v3.docx"));
        assert!(!plausible_title("main.tex"));
        assert!(!plausible_title("paper"));
        assert!(!plausible_title("Untitled document here"));
    }

    #[test]
    fn imports_an_unrecognised_pdf_under_its_file_name() {
        let dir = tempfile::tempdir().unwrap();
        let lib = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
        let pdf = dir.path().join("Some notes.pdf");
        std::fs::write(&pdf, b"%PDF-1.7 not really").unwrap();
        let collection = lib.create_collection("Inbox".into(), None).unwrap();
        let path = pdf.display().to_string();

        let first = lib
            .import_pdf(path.clone(), "no identifiers here".into(), None, Some(collection.id.clone()))
            .unwrap();
        assert_eq!((first.matched_by.as_str(), first.existing), ("none", false));
        assert_eq!(first.item.title, "Some notes");
        assert!(first.attachment.is_pdf);
        assert!(Path::new(&first.attachment.file_path).is_file());
        assert_eq!(lib.items_in_collection(collection.id).unwrap().len(), 1);

        // The same file attached again is kept once.
        let again = lib.attach_file(first.item.id.clone(), path).unwrap();
        assert_eq!(again.id, first.attachment.id);
        assert_eq!(lib.attachments(first.item.id.clone()).unwrap().len(), 1);

        lib.remove_attachment(first.attachment.id.clone()).unwrap();
        assert!(!Path::new(&first.attachment.file_path).exists());
        assert!(lib.attachments(first.item.id).unwrap().is_empty());
    }

    #[test]
    fn in_memory_libraries_keep_no_files() {
        let lib = PaperLibrary::in_memory().unwrap();
        let err = lib.import_pdf("/nonexistent.pdf".into(), String::new(), None, None).unwrap_err();
        assert!(matches!(err, CoreError::Storage { .. }), "{err}");
    }
}
