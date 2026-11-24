//! Items that look like the same work, and merging them into one, as
//! Zotero's Duplicate Items does.

use paperloom_model::ItemId;
use serde::{Deserialize, Serialize};

use crate::{CoreError, ItemDetail, ItemSummary, PaperLibrary};

/// Items that look like the same work, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct DuplicateGroup {
    pub items: Vec<ItemSummary>,
}

#[uniffi::export]
impl PaperLibrary {
    /// The items (not in the trash) that look like the same work: the same
    /// DOI, book ISBN, arXiv or PubMed ID, or the same title with years at
    /// most one apart and an author in common.
    pub fn duplicate_groups(&self) -> Result<Vec<DuplicateGroup>, CoreError> {
        let library = self.lock()?;
        let mut groups = Vec::new();
        for ids in library.duplicate_groups()? {
            let mut items = Vec::new();
            for id in &ids {
                if let Some(entry) = library.entry(id)? {
                    items.push(ItemSummary::from(&entry));
                }
            }
            groups.push(DuplicateGroup { items });
        }
        Ok(groups)
    }

    /// Merges items into `master_id`: its details stay and its gaps are
    /// filled from the others; their files (and what is marked on them),
    /// tags, collections and star come to it; the others go to the trash.
    /// The merged item.
    pub fn merge_items(&self, master_id: String, other_ids: Vec<String>) -> Result<ItemDetail, CoreError> {
        let master = ItemId::from_string(master_id);
        let mut others: Vec<ItemId> = other_ids.into_iter().map(ItemId::from_string).collect();
        others.retain(|o| *o != master);
        if others.is_empty() {
            return Err(CoreError::InvalidInput { message: "Pick two items or more to merge.".into() });
        }
        let detail = {
            let mut library = self.lock()?;
            library.merge_items(&master, &others)?;
            library.entry(&master)?.as_ref().map(ItemDetail::from)
        };
        others.push(master);
        self.reindex_items(&others);
        detail.ok_or_else(|| CoreError::NotFound { message: "The merged item is gone.".into() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewItem;

    fn add(lib: &PaperLibrary, title: &str, year: i32) -> String {
        let item = NewItem {
            title: title.into(),
            authors: vec!["Curie, Marie".into()],
            year: Some(year),
            ..NewItem::default()
        };
        lib.add_item(item).unwrap().id
    }

    #[test]
    fn groups_and_merges_duplicates() {
        let lib = PaperLibrary::in_memory().unwrap();
        let a = add(&lib, "Radioactive Substances", 1903);
        let b = add(&lib, "Radioactive substances.", 1904);
        add(&lib, "Something Else Entirely", 1903);
        let groups = lib.duplicate_groups().unwrap();
        assert_eq!(groups.len(), 1);
        let mut ids: Vec<String> = groups[0].items.iter().map(|i| i.id.clone()).collect();
        ids.sort();
        let mut expected = vec![a.clone(), b.clone()];
        expected.sort();
        assert_eq!(ids, expected);
        let merged = lib.merge_items(a.clone(), vec![b.clone(), a.clone()]).unwrap();
        assert_eq!(merged.id, a);
        assert!(lib.duplicate_groups().unwrap().is_empty());
        assert!(matches!(lib.merge_items(a.clone(), vec![a]), Err(CoreError::InvalidInput { .. })));
    }
}
