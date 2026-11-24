//! Organising the library: filtered lists, editing details, stars, reading
//! status, tags and the trash.

use paperloom_model::{
    CollectionId, Creator, CreatorRole, Doi, Item, ItemId, ItemType, ReadStatus, is_color,
};
use paperloom_store::Filter;
use serde::{Deserialize, Serialize};

use crate::{CoreError, ItemDetail, ItemSummary, PaperLibrary};

/// Which items to list. The default is every item not in the trash.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase", default)]
pub struct ItemFilter {
    #[uniffi(default = None)]
    pub collection_id: Option<String>,
    #[uniffi(default = None)]
    pub tag: Option<String>,
    #[uniffi(default = false)]
    pub starred_only: bool,
    /// `unread`, `reading` or `read`.
    #[uniffi(default = None)]
    pub read_status: Option<String>,
    /// The trash instead of the library.
    #[uniffi(default = false)]
    pub trashed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct TagInfo {
    pub name: String,
    /// `#rrggbb`, or none for a plain tag.
    pub color: Option<String>,
    /// Items carrying it, not counting the trash.
    pub count: u32,
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidInput { message: message.into() }
}

fn read_status(value: &str) -> Result<ReadStatus, CoreError> {
    ReadStatus::parse(value).ok_or_else(|| invalid(format!("Unknown reading status “{value}”.")))
}

pub(crate) fn list_items(library: &PaperLibrary, filter: ItemFilter) -> Result<Vec<ItemSummary>, CoreError> {
    let status = filter.read_status.as_deref().map(read_status).transpose()?;
    let filter = Filter {
        collection: filter.collection_id.map(CollectionId::from_string),
        tag: filter.tag,
        starred_only: filter.starred_only,
        read_status: status,
        trashed: filter.trashed,
    };
    Ok(library.lock()?.entries(&filter)?.iter().map(ItemSummary::from).collect())
}

pub(crate) fn set_read_status(
    library: &PaperLibrary,
    item_id: String,
    status: &str,
) -> Result<(), CoreError> {
    let status = read_status(status)?;
    Ok(library.lock()?.set_read_status(&ItemId::from_string(item_id), status)?)
}

pub(crate) fn tags(library: &PaperLibrary) -> Result<Vec<TagInfo>, CoreError> {
    let tags = library.lock()?.tags()?;
    Ok(tags.into_iter().map(|(tag, count)| TagInfo { name: tag.name, color: tag.color, count }).collect())
}

pub(crate) fn set_tag_color(
    library: &PaperLibrary,
    name: &str,
    color: Option<String>,
) -> Result<(), CoreError> {
    let color = match color.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) if is_color(c) => Some(c.to_ascii_lowercase()),
        Some(c) => return Err(invalid(format!("“{c}” is not a #rrggbb color."))),
        None => None,
    };
    if name.trim().is_empty() {
        return Err(invalid("A tag needs a name."));
    }
    Ok(library.lock()?.set_tag_color(name, color.as_deref())?)
}

/// Deletes items for good, and their files.
pub(crate) fn delete_items(library: &PaperLibrary, ids: &[ItemId]) -> Result<u32, CoreError> {
    let mut store = library.lock()?;
    let mut deleted = 0;
    for id in ids {
        let attachments = store.delete_item(id)?;
        library.unindex_items(std::slice::from_ref(id));
        deleted += 1;
        if let Some(files) = &library.files {
            for attachment in &attachments {
                // The records are gone either way; a stray file is harmless.
                let _ = files.remove(attachment);
            }
        }
    }
    Ok(deleted)
}

pub(crate) fn empty_trash(library: &PaperLibrary) -> Result<u32, CoreError> {
    let trashed = library.lock()?.entries(&Filter { trashed: true, ..Filter::default() })?;
    let ids: Vec<ItemId> = trashed.into_iter().map(|entry| entry.item.id).collect();
    delete_items(library, &ids)
}

pub(crate) fn update_item(library: &PaperLibrary, detail: ItemDetail) -> Result<ItemDetail, CoreError> {
    let id = ItemId::from_string(detail.id.clone());
    let mut store = library.lock()?;
    let mut item = store.item(&id)?.ok_or_else(|| CoreError::not_found(format!("item {id}")))?;
    apply(&mut item, detail)?;
    store.update_item(&item)?;
    let entry = store.entry(&id)?.ok_or_else(|| CoreError::not_found(format!("item {id}")))?;
    drop(store);
    library.reindex_items(std::slice::from_ref(&id));
    Ok(ItemDetail::from(&entry))
}

/// Trimmed text; empty means none.
fn text(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Writes edited details over an item. Names that did not change keep how
/// they were split (so `World Health Organization` stays one name);
/// translators and other contributors, which the form does not show, stay.
fn apply(item: &mut Item, detail: ItemDetail) -> Result<(), CoreError> {
    item.item_type = ItemType::parse(&detail.item_type)
        .ok_or_else(|| invalid(format!("Unknown item type “{}”.", detail.item_type)))?;
    let title = detail.title.trim();
    if title.is_empty() {
        return Err(invalid("An item needs a title."));
    }
    item.title = title.to_string();

    let creators = {
        let named = |names: &[String], role: CreatorRole| -> Vec<Creator> {
            names
                .iter()
                .map(|n| n.trim())
                .filter(|n| !n.is_empty())
                .map(|name| {
                    item.creators
                        .iter()
                        .find(|c| c.role == role && c.display_name() == name)
                        .cloned()
                        .unwrap_or_else(|| Creator { role, ..Creator::parse(name) })
                })
                .collect()
        };
        let mut creators = named(&detail.authors, CreatorRole::Author);
        creators.extend(named(&detail.editors, CreatorRole::Editor));
        creators.extend(
            item.creators
                .iter()
                .filter(|c| !matches!(c.role, CreatorRole::Author | CreatorRole::Editor))
                .cloned(),
        );
        creators
    };
    item.creators = creators;

    item.container_title = text(detail.container_title);
    item.container_short_title = text(detail.container_short_title);
    item.date = text(detail.date);
    item.year =
        detail.year.or_else(|| item.date.as_deref().and_then(|d| d.get(..4)).and_then(|y| y.parse().ok()));
    item.volume = text(detail.volume);
    item.issue = text(detail.issue);
    item.pages = text(detail.pages);
    item.publisher = text(detail.publisher);
    item.place = text(detail.place);
    item.edition = text(detail.edition);
    item.number = text(detail.number);
    item.doi = text(detail.doi).map(|d| Doi::parse(&d).map_err(|e| invalid(e.to_string()))).transpose()?;
    item.isbn = text(detail.isbn);
    item.issn = text(detail.issn);
    item.arxiv_id = text(detail.arxiv_id);
    item.pmid = text(detail.pmid);
    item.url = text(detail.url);
    item.abstract_note = text(detail.abstract_note);
    item.extra = text(detail.extra);
    item.keywords =
        detail.keywords.into_iter().map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect();
    item.language = text(detail.language);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewItem;

    fn library() -> std::sync::Arc<PaperLibrary> {
        PaperLibrary::in_memory().unwrap()
    }

    #[test]
    fn edits_details_and_keeps_unchanged_names_whole() {
        let lib = library();
        let added = lib
            .add_item(NewItem {
                title: "Report".into(),
                authors: vec!["Curie, Marie".into()],
                ..NewItem::default()
            })
            .unwrap();
        let mut detail = lib.item_detail(added.id.clone()).unwrap().unwrap();
        detail.title = "  Annual report ".into();
        detail.authors = vec!["Marie Curie".into(), "Pierre Curie".into()];
        detail.date = Some("2024-05-01".into());
        detail.doi = Some("https://doi.org/10.1000/Report".into());
        detail.keywords = vec![" radium ".into(), "".into()];
        let saved = lib.update_item(detail.clone()).unwrap();
        assert_eq!(saved.title, "Annual report");
        assert_eq!(saved.authors, ["Marie Curie", "Pierre Curie"]);
        assert_eq!((saved.year, saved.doi.as_deref()), (Some(2024), Some("10.1000/report")));
        assert_eq!(saved.keywords, ["radium"]);

        let bad = ItemDetail { doi: Some("not a doi".into()), ..saved.clone() };
        assert!(matches!(lib.update_item(bad), Err(CoreError::InvalidInput { .. })));
        let untitled = ItemDetail { title: " ".into(), ..saved };
        assert!(matches!(lib.update_item(untitled), Err(CoreError::InvalidInput { .. })));
    }

    #[test]
    fn stars_statuses_and_tags_filter_lists() {
        let lib = library();
        let a = lib.add_item(NewItem { title: "A".into(), ..NewItem::default() }).unwrap();
        let b = lib.add_item(NewItem { title: "B".into(), ..NewItem::default() }).unwrap();
        lib.set_starred(a.id.clone(), true).unwrap();
        lib.set_read_status(b.id.clone(), "reading".into()).unwrap();
        assert!(matches!(
            lib.set_read_status(b.id.clone(), "skimmed".into()),
            Err(CoreError::InvalidInput { .. })
        ));
        lib.set_item_tags(b.id.clone(), vec!["to read".into()]).unwrap();
        lib.set_tag_color("to read".into(), Some("#FF6666".into())).unwrap();
        assert!(matches!(
            lib.set_tag_color("to read".into(), Some("red".into())),
            Err(CoreError::InvalidInput { .. })
        ));

        let titles = |filter: ItemFilter| -> Vec<String> {
            lib.list_items(filter).unwrap().into_iter().map(|i| i.title).collect()
        };
        assert_eq!(titles(ItemFilter { starred_only: true, ..ItemFilter::default() }), ["A"]);
        assert_eq!(
            titles(ItemFilter { read_status: Some("reading".into()), ..ItemFilter::default() }),
            ["B"]
        );
        assert_eq!(titles(ItemFilter { tag: Some("to read".into()), ..ItemFilter::default() }), ["B"]);
        assert_eq!(
            lib.tags().unwrap(),
            [TagInfo { name: "to read".into(), color: Some("#ff6666".into()), count: 1 }]
        );
        let detail = lib.item_detail(b.id).unwrap().unwrap();
        assert_eq!((detail.read_status.as_str(), detail.tags.len()), ("reading", 1));
    }

    #[test]
    fn empties_the_trash_with_its_files() {
        let dir = tempfile::tempdir().unwrap();
        let lib = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
        let pdf = dir.path().join("paper.pdf");
        std::fs::write(&pdf, b"%PDF-1.7").unwrap();
        let imported = lib.import_pdf(pdf.display().to_string(), String::new(), None, None).unwrap();
        let kept = lib.add_item(NewItem { title: "Kept".into(), ..NewItem::default() }).unwrap();
        lib.trash_item(imported.item.id.clone()).unwrap();
        let trash = lib.list_items(ItemFilter { trashed: true, ..ItemFilter::default() }).unwrap();
        assert_eq!(trash.len(), 1);
        assert!(trash[0].has_pdf);
        assert_eq!(lib.empty_trash().unwrap(), 1);
        assert!(!std::path::Path::new(&imported.attachment.file_path).exists());
        let ids: Vec<String> = lib.items().unwrap().into_iter().map(|i| i.id).collect();
        assert_eq!(ids, [kept.id.as_str()]);
        lib.trash_item(kept.id.clone()).unwrap();
        lib.restore_item(kept.id).unwrap();
        assert_eq!(lib.items().unwrap().len(), 1);
    }
}
