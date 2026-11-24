//! Where notes meet the library: `[[@paper]]` links to items,
//! `![[@paper#hl-<annotation id>]]` embeds of PDF highlights, and literature
//! notes made from an item and its highlights.
//!
//! `@` takes a DOI, an item ID or a citation key; a highlight is found by its
//! annotation ID alone, so the key before `#` only has to be readable.

use std::collections::{HashMap, HashSet};

use paperloom_model::{Annotation, AnnotationId, Doi, Item, ItemId};
use paperloom_store::{self as store, Filter};
use paperloom_vault::{External, Link};
use serde::{Deserialize, Serialize};

use crate::{CoreError, PaperLibrary};

/// Where a highlight is, to open its PDF at the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationLocation {
    pub annotation_id: String,
    pub attachment_id: String,
    pub item_id: String,
    pub item_title: String,
    pub file_path: String,
    /// From 0.
    pub page_index: u32,
}

/// The citation key of one item, readable in notes: `kucsko2013nanometre`.
pub(crate) fn key_of(item: &Item) -> String {
    paperloom_formats::citation_keys(std::slice::from_ref(item)).pop().unwrap_or_default()
}

/// `Kucsko et al. 2013`.
pub(crate) fn short_cite(item: &Item) -> String {
    let authors = item.authors_short();
    match (authors.is_empty(), item.year) {
        (false, Some(year)) => format!("{authors} {year}"),
        (false, None) => authors,
        (true, Some(year)) => format!("{} {year}", item.title),
        (true, None) => item.title.clone(),
    }
}

/// How notes name an item: its DOI, else its ID.
fn reference_of(item: &Item) -> String {
    item.doi.as_ref().map_or_else(|| item.id.to_string(), |d| d.to_string())
}

/// A note's Pandoc citations, formatted, by where each starts, and its
/// reference list entries.
#[derive(Default)]
pub(crate) struct CitedNote {
    pub citations: HashMap<usize, String>,
    pub entries: Vec<String>,
}

/// The works a note cites, in order of first citation, and its citations
/// (by where each starts) as indexes into them; a citation naming a work
/// the library lacks is left out.
#[derive(Default)]
pub(crate) struct NoteWorks {
    pub items: Vec<Item>,
    pub groups: Vec<(usize, Vec<paperloom_citation::Cite>)>,
}

/// What `[@` completion offers: an item's citation key, and how it reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct CitationCompletion {
    /// What goes after `@`.
    pub key: String,
    pub title: String,
    /// `Kucsko et al. 2013`.
    pub cite: String,
}

/// A note's link to an item: `[[@10.1038/…|Kucsko 2013]]`.
pub(crate) fn item_link_text(item: &Item) -> String {
    format!("[[@{}|{}]]", reference_of(item), short_cite(item))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The item a `@reference` names: a DOI, an item ID or a citation key.
fn find_item(store: &store::Library, reference: &str) -> Option<Item> {
    let reference = reference.trim();
    if let Ok(doi) = Doi::parse(reference)
        && let Ok(Some(item)) = store.find_by_doi(&doi)
    {
        return Some(item);
    }
    if let Ok(Some(item)) = store.item(&ItemId::from_string(reference)) {
        return Some(item);
    }
    let lower = reference.to_lowercase();
    store.entries(&Filter::default()).ok()?.into_iter().map(|e| e.item).find(|item| key_of(item) == lower)
}

/// The same DOI, ID and citation-key lookup, indexed once for a document.
/// Trashed items can still be named by ID, as in `find_item`.
pub(crate) struct CitationResolver {
    items: Vec<Item>,
    dois: HashMap<String, usize>,
    ids: HashMap<String, usize>,
    keys: HashMap<String, usize>,
}

impl CitationResolver {
    pub(crate) fn new(store: &store::Library) -> Result<Self, CoreError> {
        let mut resolver =
            Self { items: Vec::new(), dois: HashMap::new(), ids: HashMap::new(), keys: HashMap::new() };
        for item in store.items()? {
            let index = resolver.items.len();
            if let Some(doi) = &item.doi {
                resolver.dois.insert(doi.to_string(), index);
            }
            resolver.keys.entry(key_of(&item).to_lowercase()).or_insert(index);
            resolver.ids.insert(item.id.to_string().to_lowercase(), index);
            resolver.items.push(item);
        }
        for item in store.trashed_items()? {
            resolver.ids.insert(item.id.to_string().to_lowercase(), resolver.items.len());
            resolver.items.push(item);
        }
        Ok(resolver)
    }

    pub(crate) fn find(&self, reference: &str) -> Option<&Item> {
        let reference = reference.trim();
        let doi = Doi::parse(reference).ok();
        let lower = reference.to_lowercase();
        let index = doi
            .as_ref()
            .and_then(|d| self.dois.get(d.as_str()))
            .or_else(|| self.ids.get(&lower))
            .or_else(|| self.keys.get(&lower))?;
        self.items.get(*index)
    }
}

fn find_highlight(store: &store::Library, id: &str) -> Option<(Annotation, Item, String)> {
    let annotation = store.annotation(&AnnotationId::from_string(id)).ok()??;
    let attachment = store.attachment(&annotation.attachment_id).ok()??;
    let item = store.item(&attachment.item_id).ok()??;
    Some((annotation, item, attachment.path))
}

impl PaperLibrary {
    /// Formats a note's `[@key]` citations (keys are citation keys, DOIs or
    /// item IDs) in a style, numbered across the note, with the reference
    /// list to put after it. A citation naming a work the library lacks
    /// stays as written.
    pub(crate) fn cite_note(
        &self,
        text: &str,
        style_id: &str,
        format: paperloom_citation::Format,
    ) -> CitedNote {
        let NoteWorks { items, groups } = self.note_works(text);
        if groups.is_empty() {
            return CitedNote::default();
        }
        let wanted: Vec<Vec<paperloom_citation::Cite>> = groups.iter().map(|(_, g)| g.clone()).collect();
        let Ok(document) = paperloom_citation::format_document(&items, &wanted, style_id, format) else {
            return CitedNote::default();
        };
        let citations = groups.iter().map(|(start, _)| *start).zip(document.citations).collect();
        CitedNote { citations, entries: document.entries }
    }

    /// Whether a note link names an item (by DOI, ID or citation key) or
    /// embeds one of its highlights.
    pub(crate) fn links_to_item(&self, item_id: &str) -> Result<impl Fn(&Link) -> bool + use<>, CoreError> {
        let store = self.lock()?;
        let item = store
            .item(&ItemId::from_string(item_id))?
            .ok_or_else(|| CoreError::not_found(format!("item {item_id}")))?;
        let mut names: HashSet<String> = [item.id.to_string(), key_of(&item)].into_iter().collect();
        names.extend(item.doi.as_ref().map(|d| d.to_string()));
        let names: HashSet<String> = names.into_iter().map(|n| n.to_lowercase()).collect();
        let mut highlights = HashSet::new();
        for attachment in store.attachments(&item.id)? {
            highlights.extend(store.annotations(&attachment.id)?.into_iter().map(|a| a.id.to_string()));
        }
        Ok(move |link: &Link| {
            let Some(reference) = link.target.strip_prefix('@') else { return false };
            if let Some(id) = link.subpath.as_deref().and_then(|s| s.strip_prefix("hl-")) {
                return highlights.contains(id);
            }
            let reference = reference.trim();
            let reference = Doi::parse(reference).map_or_else(|_| reference.to_string(), |d| d.to_string());
            names.contains(&reference.to_lowercase())
        })
    }

    /// An item's title and the link a note gets to it; none for an item
    /// gone or in the trash.
    pub(crate) fn item_to_link(&self, item_id: &str) -> Option<(String, String)> {
        let store = self.lock().ok()?;
        let id = ItemId::from_string(item_id);
        if store.is_trashed(&id).ok()? != Some(false) {
            return None;
        }
        let item = store.item(&id).ok()??;
        Some((item.title.clone(), item_link_text(&item)))
    }

    pub(crate) fn note_works(&self, text: &str) -> NoteWorks {
        let parsed = paperloom_vault::parse(text);
        let mut works = NoteWorks::default();
        if parsed.citations.is_empty() {
            return works;
        }
        let mut index_of: HashMap<String, usize> = HashMap::new();
        let Ok(store) = self.lock() else { return works };
        for citation in &parsed.citations {
            let cites: Vec<paperloom_citation::Cite> = citation
                .works
                .iter()
                .filter_map(|work| {
                    let item = find_item(&store, &work.key)?;
                    let index = *index_of.entry(item.id.to_string()).or_insert_with(|| {
                        works.items.push(item);
                        works.items.len() - 1
                    });
                    Some(paperloom_citation::Cite { index, page: work.page.clone() })
                })
                .collect();
            if !cites.is_empty() && cites.len() == citation.works.len() {
                works.groups.push((citation.start, cites));
            }
        }
        works
    }

    /// Shows `[[@…]]` links and `![[@…#hl-…]]` embeds in a note's reading
    /// view.
    pub(crate) fn render_note_link(&self, link: &Link) -> Option<External> {
        let reference = link.target.strip_prefix('@')?;
        let store = self.lock().ok()?;
        if let Some(id) = link.subpath.as_deref().and_then(|s| s.strip_prefix("hl-")) {
            let Some((annotation, item, _)) = find_highlight(&store, id) else {
                return Some(External {
                    html: format!(
                        r#"<span class="wikilink unresolved">{}</span>"#,
                        escape(&format!("@{reference}#hl-{id}"))
                    ),
                    block: false,
                });
            };
            let text = annotation.text.as_deref().map(escape).unwrap_or_default();
            let comment = annotation
                .comment
                .as_deref()
                .map(|c| format!(r#"<p class="comment">{}</p>"#, escape(c)))
                .unwrap_or_default();
            let source = format!(
                r#"<p class="source"><a href="paperloom:annotation/{}">{} · p. {}</a></p>"#,
                annotation.id,
                escape(&short_cite(&item)),
                annotation.page_index + 1
            );
            let html = format!(
                r#"<blockquote class="pdf-highlight {}" style="border-left-color: {}"><p>{text}</p>{comment}{source}</blockquote>"#,
                annotation.kind.as_str(),
                escape(&annotation.color)
            );
            return Some(External { html, block: link.embed });
        }
        let html = match find_item(&store, reference) {
            Some(item) => format!(
                r#"<a class="item-link" href="paperloom:item/{}" title="{}">{}</a>"#,
                item.id,
                escape(&item.title),
                escape(link.alias.as_deref().unwrap_or(&short_cite(&item)))
            ),
            None => format!(r#"<span class="wikilink unresolved">@{}</span>"#, escape(reference)),
        };
        Some(External { html, block: false })
    }

    /// The items a `from: library` query starts from (in its collection,
    /// under its tag), with the fields queries can use: `title`, `authors`,
    /// `year`, `date`, `journal`, `type`, `doi`, `key`, `tags`, `status`,
    /// `starred` and `added`. No rows for a collection the library lacks.
    pub(crate) fn query_rows(&self, query: &paperloom_vault::Query) -> Vec<paperloom_vault::Row> {
        let Ok(store) = self.lock() else { return Vec::new() };
        let collection = match query.folder.as_deref() {
            None => None,
            Some(name) => {
                let all = store.collections().unwrap_or_default();
                let Some(found) = all.into_iter().find(|c| c.name.eq_ignore_ascii_case(name)) else {
                    return Vec::new();
                };
                Some(found.id)
            }
        };
        let Ok(entries) = store.entries(&Filter { collection, ..Filter::default() }) else {
            return Vec::new();
        };
        entries
            .into_iter()
            .filter(|e| query.tag.as_deref().is_none_or(|tag| paperloom_vault::has_tag(&e.tags, tag)))
            .map(|entry| {
                let item = &entry.item;
                let names: Vec<String> = item.creators.iter().map(|c| c.display_name()).collect();
                let fields = [
                    ("title", item.title.clone()),
                    ("authors", names.join(", ")),
                    ("year", item.year.map(|y| y.to_string()).unwrap_or_default()),
                    ("date", item.date.clone().unwrap_or_default()),
                    ("journal", item.container_title.clone().unwrap_or_default()),
                    ("type", item.item_type.as_str().to_string()),
                    ("doi", item.doi.as_ref().map(|d| d.to_string()).unwrap_or_default()),
                    ("key", key_of(item)),
                    ("tags", entry.tags.join(", ")),
                    ("status", entry.read_status.as_str().to_string()),
                    ("starred", if entry.starred { "yes" } else { "no" }.to_string()),
                    ("added", paperloom_vault::date_of(entry.date_added)),
                ];
                paperloom_vault::Row {
                    href: format!("paperloom:item/{}", item.id),
                    title: item.title.clone(),
                    fields: fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
                }
            })
            .collect()
    }
}

#[uniffi::export]
impl PaperLibrary {
    /// Items whose title, authors, year or citation key hold every word
    /// typed, for `[@` completion; the most recently added first.
    pub fn citation_completions(
        &self,
        query: String,
        limit: u32,
    ) -> Result<Vec<CitationCompletion>, CoreError> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let store = self.lock()?;
        let mut found = Vec::new();
        for entry in store.entries(&Filter::default())? {
            let item = entry.item;
            let key = key_of(&item);
            let names: Vec<String> = item.creators.iter().map(|c| c.display_name()).collect();
            let year = item.year.map(|y| y.to_string()).unwrap_or_default();
            let haystack = format!("{} {} {year} {key}", item.title, names.join(" ")).to_lowercase();
            if words.iter().all(|w| haystack.contains(w.as_str())) {
                found.push(CitationCompletion { key, title: item.title.clone(), cite: short_cite(&item) });
                if found.len() >= limit as usize {
                    break;
                }
            }
        }
        Ok(found)
    }

    /// Markdown that embeds a highlight in a note: `![[@key#hl-<id>]]`.
    pub fn annotation_embed(&self, annotation_id: String) -> Result<String, CoreError> {
        let store = self.lock()?;
        let (annotation, item, _) = find_highlight(&store, &annotation_id)
            .ok_or_else(|| CoreError::not_found(format!("annotation {annotation_id}")))?;
        Ok(format!("![[@{}#hl-{}]]", key_of(&item), annotation.id))
    }

    /// Markdown that links a note to an item: `[[@10.1038/…|Kucsko 2013]]`.
    pub fn item_link(&self, item_id: String) -> Result<String, CoreError> {
        let item = self
            .lock()?
            .item(&ItemId::from_string(item_id.clone()))?
            .ok_or_else(|| CoreError::not_found(format!("item {item_id}")))?;
        Ok(format!("[[@{}|{}]]", reference_of(&item), short_cite(&item)))
    }

    /// The citation notes and Word documents take for some items:
    /// `[@kucsko2013nanometre; @curie1903radioactivity]`. An item whose
    /// citation key another item shares is named by its DOI or ID instead.
    pub fn citation_marker(&self, item_ids: Vec<String>) -> Result<String, CoreError> {
        let store = self.lock()?;
        let mut taken: HashMap<String, usize> = HashMap::new();
        for entry in store.entries(&Filter::default())? {
            *taken.entry(key_of(&entry.item)).or_default() += 1;
        }
        let mut works = Vec::new();
        for id in item_ids {
            let item = store
                .item(&ItemId::from_string(id.clone()))?
                .ok_or_else(|| CoreError::not_found(format!("item {id}")))?;
            let key = key_of(&item);
            let name = if taken.get(&key).copied().unwrap_or(0) > 1 { reference_of(&item) } else { key };
            works.push(format!("@{name}"));
        }
        Ok(format!("[{}]", works.join("; ")))
    }

    /// The item a `[[@reference]]` names (a DOI, item ID or citation key).
    pub fn resolve_item_reference(&self, reference: String) -> Result<Option<String>, CoreError> {
        let store = self.lock()?;
        Ok(find_item(&store, reference.trim_start_matches('@')).map(|item| item.id.to_string()))
    }

    /// Where a highlight is, for opening its PDF at the page.
    pub fn annotation_location(
        &self,
        annotation_id: String,
    ) -> Result<Option<AnnotationLocation>, CoreError> {
        let store = self.lock()?;
        let Some((annotation, item, path)) = find_highlight(&store, &annotation_id) else { return Ok(None) };
        let file_path = match &self.files {
            Some(files) => {
                store.attachment(&annotation.attachment_id)?.map(|a| files.info(&a).file_path).unwrap_or(path)
            }
            None => path,
        };
        Ok(Some(AnnotationLocation {
            annotation_id: annotation.id.to_string(),
            attachment_id: annotation.attachment_id.to_string(),
            item_id: item.id.to_string(),
            item_title: item.title.clone(),
            file_path,
            page_index: annotation.page_index,
        }))
    }
}

/// The front-matter keys that give a note a lasting identity and the item
/// it belongs to, for Zotero plugins. Written when a note is made for an
/// item, never while reading.
pub(crate) const NOTE_ID: &str = "paperloom-note-id";
pub(crate) const NOTE_PARENT: &str = "paperloom-parent-item";
/// `plugin` on notes a Zotero plugin made: the only notes plugins change.
pub(crate) const NOTE_SOURCE: &str = "paperloom-note-source";

/// A literature note for an item: its details as properties, a link to
/// it, the abstract, and every highlight embedded, page by page.
pub(crate) fn literature_note(library: &PaperLibrary, item_id: &str) -> Result<(String, String), CoreError> {
    let store = library.lock()?;
    let item = store
        .item(&ItemId::from_string(item_id))?
        .ok_or_else(|| CoreError::not_found(format!("item {item_id}")))?;
    let key = key_of(&item);
    let quote = |text: &str| format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""));
    let mut text = String::from("---\n");
    text.push_str(&format!("title: {}\n", quote(&item.title)));
    let authors: Vec<String> = item.creators.iter().map(|c| quote(&c.display_name())).collect();
    if !authors.is_empty() {
        text.push_str(&format!("authors: [{}]\n", authors.join(", ")));
    }
    if let Some(year) = item.year {
        text.push_str(&format!("year: {year}\n"));
    }
    if let Some(doi) = &item.doi {
        text.push_str(&format!("doi: {doi}\n"));
    }
    text.push_str("tags: [literature]\n");
    // The note's own identity and its item, as Zotero plugins see it.
    text.push_str(&format!("{NOTE_ID}: {}\n", uuid::Uuid::now_v7()));
    text.push_str(&format!("{NOTE_PARENT}: {}\n---\n\n", item.id));
    text.push_str(&format!("# {}\n\n", item.title));
    let mut source = format!("[[@{}|{}]]", reference_of(&item), short_cite(&item));
    if let Some(container) = &item.container_title {
        source.push_str(&format!(" · *{container}*"));
    }
    text.push_str(&format!("{source}\n\n"));
    if let Some(summary) = &item.abstract_note {
        text.push_str(&format!("## Abstract\n\n{summary}\n\n"));
    }
    let mut highlights: Vec<Annotation> = Vec::new();
    for attachment in store.attachments(&item.id)? {
        highlights.extend(store.annotations(&attachment.id)?);
    }
    if !highlights.is_empty() {
        text.push_str("## Highlights\n\n");
        for annotation in &highlights {
            text.push_str(&format!("![[@{key}#hl-{}]]\n\n", annotation.id));
        }
    }
    text.push_str("## Notes\n\n");
    Ok((format!("Literature/{key}.md"), text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotations::AnnotationRect;
    use crate::{NewAnnotation, NewItem, PaperVault};

    #[test]
    fn citation_markers_use_keys_unless_two_items_share_one() {
        let lib = PaperLibrary::in_memory().unwrap();
        let add = |title: &str, doi: Option<&str>| {
            lib.add_item(NewItem {
                title: title.into(),
                authors: vec!["Marie Curie".into()],
                year: Some(1903),
                doi: doi.map(Into::into),
                ..NewItem::default()
            })
            .unwrap()
            .id
        };
        let radioactivity = add("Radioactivity", None);
        let first = add("Polonium", Some("10.1000/one"));
        let second = add("Polonium", None);
        assert_eq!(lib.citation_marker(vec![radioactivity.clone()]).unwrap(), "[@curie1903radioactivity]");
        assert_eq!(
            lib.citation_marker(vec![radioactivity, first, second.clone()]).unwrap(),
            format!("[@curie1903radioactivity; @10.1000/one; @{second}]")
        );
        assert!(lib.citation_marker(vec!["missing".into()]).is_err());
    }

    #[test]
    fn literature_notes_become_zotero_child_notes() {
        let dir = tempfile::tempdir().unwrap();
        let lib = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
        let item = lib
            .add_item(crate::NewItem {
                item_type: String::new(),
                title: "Deep learning".into(),
                authors: vec!["Yann LeCun".into()],
                container_title: None,
                year: Some(2015),
                doi: None,
            })
            .unwrap();
        let vault = PaperVault::open(dir.path().join("vault").display().to_string()).unwrap();
        vault.connect_library(lib.clone());
        let note = vault.create_literature_note(item.id.clone()).unwrap();
        let text = vault.read_note(note.path.clone()).unwrap();
        assert!(text.contains(&format!("{NOTE_PARENT}: {}", item.id)));
        // A plain note has no identity yet, so plugins do not see it.
        vault.write_note("Ideas.md".into(), "# Ideas\n".into()).unwrap();
        // A copied note repeats an id: only the first counts.
        vault.write_note("Literature/Copy.md".into(), text.clone()).unwrap();
        // A note whose item is gone stands alone.
        vault
            .write_note(
                "Orphan.md".into(),
                format!("---\n{NOTE_ID}: orphan-1\n{NOTE_PARENT}: missing\n---\nAlone\n"),
            )
            .unwrap();

        let notes: serde_json::Value = serde_json::from_str(&vault.zotero_notes().unwrap()).unwrap();
        let notes = notes.as_array().unwrap();
        assert_eq!(notes.len(), 2, "{notes:?}");
        let child = notes.iter().find(|n| n["parentItem"] == item.id).unwrap();
        assert_eq!(child["itemType"], "note");
        assert!(child["note"].as_str().unwrap().starts_with(r#"<div data-schema-version="9">"#));
        assert!(child["note"].as_str().unwrap().contains("Deep learning"));
        let orphan = notes.iter().find(|n| n["key"] == "orphan-1").unwrap();
        assert!(orphan.get("parentItem").is_none());

        // A plugin's note: made in Zotero Notes/, under the paper, changed by the plugin.
        let made = vault
            .zotero_create_note(Some(item.id.clone()), "<h1>Summary</h1><p>Deep nets <b>work</b>.</p>".into())
            .unwrap();
        let notes: serde_json::Value = serde_json::from_str(&vault.zotero_notes().unwrap()).unwrap();
        let mine = notes.as_array().unwrap().iter().find(|n| n["key"] == made).unwrap();
        assert_eq!(
            (mine["parentItem"].as_str(), mine["paperloomPluginNote"].as_bool()),
            (Some(item.id.as_str()), Some(true))
        );
        assert!(vault.read_note("Zotero Notes/Summary.md".into()).unwrap().contains("Deep nets **work**."));
        vault.zotero_update_note(made.clone(), "<p>Revised</p>".into()).unwrap();
        let text = vault.read_note("Zotero Notes/Summary.md".into()).unwrap();
        assert!(
            text.contains(&format!("{NOTE_ID}: {made}")) && text.contains(&format!("{NOTE_SOURCE}: plugin"))
        );
        assert!(text.contains("Revised") && !text.contains("Deep nets"));
        // The person's own notes stay theirs.
        let own = child["key"].as_str().unwrap().to_string();
        assert!(vault.zotero_update_note(own, "<p>Overwritten</p>".into()).is_err());
        assert!(vault.zotero_update_note(made, "x".repeat((1 << 20) + 1)).is_err());
    }

    #[test]
    fn links_notes_to_items_and_highlights() {
        let dir = tempfile::tempdir().unwrap();
        let lib = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
        let pdf = dir.path().join("Kucsko 2013.pdf");
        std::fs::write(&pdf, b"%PDF-1.7").unwrap();
        let imported = lib.import_pdf(pdf.display().to_string(), "doi:10.1000/none".into(), None, None);
        // No network in tests: the lookup fails and the item keeps the DOI.
        let imported = imported.unwrap();
        let mut detail = lib.item_detail(imported.item.id.clone()).unwrap().unwrap();
        detail.title = "Nanometre-scale thermometry".into();
        detail.authors = vec!["Georg Kucsko".into()];
        detail.year = Some(2013);
        lib.update_item(detail).unwrap();
        let mark = lib
            .add_annotation(NewAnnotation {
                attachment_id: imported.attachment.id.clone(),
                kind: "highlight".into(),
                color: "#ffd400".into(),
                page_index: 2,
                rects: vec![AnnotationRect { x: 1.0, y: 2.0, width: 3.0, height: 4.0 }],
                text: Some("living cell <thermometry>".into()),
                comment: Some("key result".into()),
                strokes: Vec::new(),
                font_size: None,
            })
            .unwrap();

        let embed = lib.annotation_embed(mark.id.clone()).unwrap();
        assert_eq!(embed, format!("![[@kucsko2013nanometre#hl-{}]]", mark.id));
        let link = lib.item_link(imported.item.id.clone()).unwrap();
        assert_eq!(link, "[[@10.1000/none|Kucsko 2013]]");
        assert_eq!(
            lib.resolve_item_reference("@kucsko2013nanometre".into()).unwrap(),
            Some(imported.item.id.clone())
        );
        assert_eq!(
            lib.resolve_item_reference("10.1000/NONE".into()).unwrap(),
            Some(imported.item.id.clone())
        );
        assert_eq!(lib.resolve_item_reference("nobody".into()).unwrap(), None);
        let place = lib.annotation_location(mark.id.clone()).unwrap().unwrap();
        assert_eq!((place.page_index, place.item_title.as_str()), (2, "Nanometre-scale thermometry"));
        assert!(place.file_path.ends_with("Kucsko 2013.pdf"));

        let vault = PaperVault::open(dir.path().join("Vault").display().to_string()).unwrap();
        vault.connect_library(lib.clone());
        let html = vault
            .render(
                "A.md".into(),
                format!("See {link} and [[@kucsko2013nanometre]].\n\n{embed}\n\n[[@nobody]]"),
            )
            .unwrap();
        assert!(html.contains(&format!(r#"href="paperloom:item/{}""#, imported.item.id)), "{html}");
        assert!(html.contains(">Kucsko 2013</a>"), "{html}");
        assert!(html.contains("living cell &lt;thermometry&gt;") && html.contains("key result"), "{html}");
        assert!(
            html.contains(&format!("paperloom:annotation/{}", mark.id)) && html.contains("p. 3"),
            "{html}"
        );
        assert!(html.contains(r#"<span class="wikilink unresolved">@nobody</span>"#), "{html}");

        let note = vault.create_literature_note(imported.item.id.clone()).unwrap();
        assert_eq!(note.path, "Literature/kucsko2013nanometre.md");
        let text = vault.read_note(note.path.clone()).unwrap();
        assert!(text.contains("doi: 10.1000/none") && text.contains(&embed), "{text}");
        assert_eq!(
            vault.create_literature_note(imported.item.id.clone()).unwrap().path,
            note.path,
            "made once"
        );

        let a_text = format!("See {link} and [[@kucsko2013nanometre]].\n\n{embed}\n\n[[@nobody]]");
        vault.write_note("A.md".into(), a_text).unwrap();
        vault.write_note("B.md".into(), "[[@nobody]] and [[Other]]".into()).unwrap();
        let mentions = vault.item_mentions(imported.item.id.clone()).unwrap();
        let in_a: Vec<u32> = mentions.iter().filter(|m| m.path == "A.md").map(|m| m.line).collect();
        assert_eq!(in_a, [0, 2], "{mentions:?}");
        assert!(mentions.iter().any(|m| m.path == note.path));
        assert!(!mentions.iter().any(|m| m.path == "B.md"));
        assert!(matches!(vault.item_mentions("nothing".into()), Err(CoreError::NotFound { .. })));
    }
}
