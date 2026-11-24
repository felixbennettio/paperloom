//! Full-text search over items, annotations, PDF page text and vault notes.
//! The index is derived from the library (and the vault): rebuilt when new,
//! then kept in step with every change.

use paperloom_fulltext::{self as fulltext, Document, Kind};
use paperloom_model::{Annotation, AttachmentId, ItemId};
use paperloom_store as store;
use serde::{Deserialize, Serialize};

use crate::{CoreError, PaperLibrary};

impl From<fulltext::Error> for CoreError {
    fn from(err: fulltext::Error) -> Self {
        CoreError::Storage { message: err.to_string() }
    }
}

/// A match from `search_everything`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct FullTextHit {
    /// `item`, `annotation`, `pdf` (a PDF page) or `note`.
    pub kind: String,
    pub title: String,
    /// The best passage, as HTML with the matched words in `<b>`.
    pub snippet_html: String,
    pub item_id: Option<String>,
    pub attachment_id: Option<String>,
    /// From 0.
    pub page_index: Option<u32>,
    pub annotation_id: Option<String>,
    pub note_path: Option<String>,
}

pub(crate) fn item_document(entry: &store::Entry) -> Document {
    let item = &entry.item;
    let mut body: Vec<String> = item.creators.iter().map(|c| c.display_name()).collect();
    body.extend(
        [
            &item.container_title,
            &item.container_short_title,
            &item.publisher,
            &item.isbn,
            &item.issn,
            &item.arxiv_id,
            &item.pmid,
            &item.abstract_note,
        ]
        .into_iter()
        .flatten()
        .cloned(),
    );
    body.extend(item.doi.as_ref().map(|d| d.to_string()));
    body.extend(item.year.map(|y| y.to_string()));
    body.extend(item.keywords.iter().cloned());
    body.extend(entry.tags.iter().cloned());
    Document {
        kind: Kind::Item,
        key: item.id.to_string(),
        item: Some(item.id.to_string()),
        attachment: None,
        page: None,
        title: item.title.clone(),
        body: body.join("\n"),
    }
}

pub(crate) fn annotation_document(annotation: &Annotation, item: &ItemId, title: &str) -> Document {
    let body =
        [annotation.text.as_deref(), annotation.comment.as_deref()].into_iter().flatten().collect::<Vec<_>>();
    Document {
        kind: Kind::Annotation,
        key: annotation.id.to_string(),
        item: Some(item.to_string()),
        attachment: Some(annotation.attachment_id.to_string()),
        page: Some(annotation.page_index),
        title: title.to_string(),
        body: body.join("\n"),
    }
}

fn page_key(attachment: &AttachmentId, page: u32) -> String {
    format!("{attachment}/{page}")
}

fn page_documents(
    attachment: &AttachmentId,
    item: &ItemId,
    title: &str,
    pages: &[(u32, String)],
) -> Vec<Document> {
    pages
        .iter()
        .map(|(page, text)| Document {
            kind: Kind::Pdf,
            key: page_key(attachment, *page),
            item: Some(item.to_string()),
            attachment: Some(attachment.to_string()),
            page: Some(*page),
            title: title.to_string(),
            body: text.clone(),
        })
        .collect()
}

fn title_of(store: &store::Library, item: &ItemId) -> String {
    store.item(item).ok().flatten().map(|i| i.title).unwrap_or_default()
}

impl PaperLibrary {
    /// Fills the index from the library (notes come when a vault connects).
    pub(crate) fn rebuild_index(&self) -> Result<(), CoreError> {
        let mut documents = Vec::new();
        {
            let store = self.lock()?;
            for entry in store.entries(&store::Filter::default())? {
                documents.push(item_document(&entry));
            }
            for (annotation, item) in store.all_annotations()? {
                documents.push(annotation_document(&annotation, &item, &title_of(&store, &item)));
            }
            for attachment in store.attachments_with_pages()? {
                let pages = store.attachment_pages(&attachment.id)?;
                let title = title_of(&store, &attachment.item_id);
                documents.extend(page_documents(&attachment.id, &attachment.item_id, &title, &pages));
            }
        }
        self.fulltext.remove_kind(Kind::Item)?;
        self.fulltext.remove_kind(Kind::Annotation)?;
        self.fulltext.remove_kind(Kind::Pdf)?;
        self.fulltext.put(&documents)?;
        Ok(())
    }

    /// Brings items' entries in the index up to date. Indexing never makes
    /// the change that caused it fail; a stale entry is fixed by the next.
    pub(crate) fn reindex_items(&self, ids: &[ItemId]) {
        let documents: Vec<Document> = match self.lock() {
            Ok(store) => ids
                .iter()
                .filter_map(|id| store.entry(id).ok().flatten())
                .map(|e| item_document(&e))
                .collect(),
            Err(_) => return,
        };
        let _ = self.fulltext.put(&documents);
    }

    pub(crate) fn unindex_items(&self, ids: &[ItemId]) {
        for id in ids {
            let _ = self.fulltext.remove_item(id.as_str());
        }
    }

    pub(crate) fn index_annotation(&self, annotation: &Annotation) {
        let found = self.lock().ok().and_then(|store| {
            let attachment = store.attachment(&annotation.attachment_id).ok().flatten()?;
            let title = title_of(&store, &attachment.item_id);
            Some((attachment.item_id, title))
        });
        if let Some((item, title)) = found {
            let _ = self.fulltext.put(&[annotation_document(annotation, &item, &title)]);
        }
    }

    pub(crate) fn unindex_keys(&self, keys: &[String]) {
        let _ = self.fulltext.remove_keys(keys);
    }

    pub(crate) fn unindex_attachment(&self, attachment: &AttachmentId) {
        let _ = self.fulltext.remove_attachment(attachment.as_str());
    }

    /// Indexes notes: all of them (replacing every note entry) or some.
    pub(crate) fn index_notes(&self, notes: &[(String, String, String)], all: bool) {
        if all {
            let _ = self.fulltext.remove_kind(Kind::Note);
        }
        let documents: Vec<Document> = notes
            .iter()
            .map(|(path, title, text)| Document {
                kind: Kind::Note,
                key: path.clone(),
                item: None,
                attachment: None,
                page: None,
                title: title.clone(),
                body: text.clone(),
            })
            .collect();
        let _ = self.fulltext.put(&documents);
    }
}

pub(crate) fn set_attachment_text(
    library: &PaperLibrary,
    attachment_id: String,
    pages: Vec<String>,
) -> Result<(), CoreError> {
    let id = AttachmentId::from_string(attachment_id);
    let (old, item, title, kept) = {
        let mut store = library.lock()?;
        let attachment =
            store.attachment(&id)?.ok_or_else(|| CoreError::not_found(format!("attachment {id}")))?;
        let old: Vec<String> =
            store.attachment_pages(&id)?.into_iter().map(|(p, _)| page_key(&id, p)).collect();
        store.set_attachment_pages(&id, &pages)?;
        let kept = store.attachment_pages(&id)?;
        let title = title_of(&store, &attachment.item_id);
        (old, attachment.item_id, title, kept)
    };
    library.fulltext.remove_keys(&old)?;
    library.fulltext.put(&page_documents(&id, &item, &title, &kept))?;
    Ok(())
}

pub(crate) fn search_everything(
    library: &PaperLibrary,
    query: &str,
    kinds: &[String],
    limit: u32,
) -> Result<Vec<FullTextHit>, CoreError> {
    let kinds: Vec<Kind> = kinds.iter().filter_map(|k| Kind::parse(k)).collect();
    let limit = limit.clamp(1, 500) as usize;
    let found = library.fulltext.search(query, &kinds, limit * 2)?;
    let store = library.lock()?;
    let mut hits = Vec::new();
    for hit in found {
        // Leave out what is in the trash (or gone).
        if let Some(item) = &hit.item
            && store.is_trashed(&ItemId::from_string(item.clone()))? != Some(false)
        {
            continue;
        }
        hits.push(FullTextHit {
            kind: hit.kind.as_str().to_string(),
            title: hit.title,
            snippet_html: hit.snippet,
            item_id: hit.item,
            attachment_id: hit.attachment,
            page_index: hit.page,
            annotation_id: (hit.kind == Kind::Annotation).then(|| hit.key.clone()),
            note_path: (hit.kind == Kind::Note).then_some(hit.key),
        });
        if hits.len() >= limit {
            break;
        }
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotations::AnnotationRect;
    use crate::{NewAnnotation, NewItem, PaperVault};

    fn kinds(hits: &[FullTextHit]) -> Vec<&str> {
        hits.iter().map(|h| h.kind.as_str()).collect()
    }

    #[test]
    fn searches_everything_and_keeps_up() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("library.sqlite").display().to_string();
        let lib = PaperLibrary::new(database.clone()).unwrap();
        let item = lib
            .add_item(NewItem {
                title: "基于深度学习的文献分类".into(),
                authors: vec!["张三".into()],
                ..NewItem::default()
            })
            .unwrap();
        let pdf = dir.path().join("paper.pdf");
        std::fs::write(&pdf, b"%PDF-1.7").unwrap();
        let imported = lib.import_pdf(pdf.display().to_string(), String::new(), None, None).unwrap();
        let attachment = imported.attachment.id.clone();
        lib.set_attachment_text(
            attachment.clone(),
            vec!["Transformers use self-attention.".into(), "卷积网络".into()],
        )
        .unwrap();
        assert!(lib.attachment_has_text(attachment.clone()).unwrap());
        lib.add_annotation(NewAnnotation {
            attachment_id: attachment.clone(),
            kind: "highlight".into(),
            color: "#ffd400".into(),
            page_index: 0,
            rects: vec![AnnotationRect { x: 1.0, y: 2.0, width: 3.0, height: 4.0 }],
            text: Some("self-attention".into()),
            comment: Some("key mechanism".into()),
            strokes: Vec::new(),
            font_size: None,
        })
        .unwrap();

        let hits = lib.search_everything("self-attention".into(), vec![], 10).unwrap();
        assert_eq!(kinds(&hits).len(), 2, "{hits:?}");
        assert!(kinds(&hits).contains(&"pdf") && kinds(&hits).contains(&"annotation"));
        let page = hits.iter().find(|h| h.kind == "pdf").unwrap();
        assert_eq!((page.page_index, page.attachment_id.as_deref()), (Some(0), Some(attachment.as_str())));
        assert_eq!(
            lib.search_everything("深度学习".into(), vec!["item".into()], 10).unwrap()[0].item_id.as_deref(),
            Some(item.id.as_str())
        );
        assert_eq!(kinds(&lib.search_everything("卷积".into(), vec![], 10).unwrap()), ["pdf"]);

        let vault = PaperVault::open(dir.path().join("Vault").display().to_string()).unwrap();
        vault.write_note("Ideas".into(), "Compare self-attention with 卷积.".into()).unwrap();
        vault.connect_library(lib.clone());
        let notes = lib.search_everything("卷积".into(), vec!["note".into()], 10).unwrap();
        assert_eq!(notes[0].note_path.as_deref(), Some("Ideas.md"));
        vault.rename_note("Ideas.md".into(), "Thoughts.md".into()).unwrap();
        let notes = lib.search_everything("卷积".into(), vec!["note".into()], 10).unwrap();
        assert_eq!(notes.iter().map(|n| n.note_path.clone().unwrap()).collect::<Vec<_>>(), ["Thoughts.md"]);

        lib.trash_item(imported.item.id.clone()).unwrap();
        assert_eq!(
            kinds(&lib.search_everything("self-attention".into(), vec![], 10).unwrap()),
            ["note"],
            "the trash is left out"
        );
        lib.restore_item(imported.item.id.clone()).unwrap();
        lib.delete_item(imported.item.id.clone()).unwrap();
        assert_eq!(kinds(&lib.search_everything("self-attention".into(), vec![], 10).unwrap()), ["note"]);

        // A new index is filled from the library.
        drop(vault);
        drop(lib);
        std::fs::remove_dir_all(dir.path().join("index")).unwrap();
        let reopened = PaperLibrary::new(database).unwrap();
        assert_eq!(reopened.search_everything("深度学习".into(), vec![], 10).unwrap().len(), 1);
    }
}
