//! Related items, as Zotero relates them: a link both items show.

use paperloom_model::ItemId;

use crate::{CoreError, ItemSummary, PaperLibrary};

#[uniffi::export]
impl PaperLibrary {
    /// The items related to an item, not counting the trash.
    pub fn related_items(&self, item_id: String) -> Result<Vec<ItemSummary>, CoreError> {
        let entries = self.lock()?.related_entries(&ItemId::from_string(item_id))?;
        Ok(entries.iter().map(ItemSummary::from).collect())
    }

    /// Relates two items, each to the other.
    pub fn relate_items(&self, item_id: String, other_id: String) -> Result<(), CoreError> {
        if item_id == other_id {
            return Err(CoreError::InvalidInput { message: "An item cannot be related to itself.".into() });
        }
        Ok(self.lock()?.relate_items(&ItemId::from_string(item_id), &ItemId::from_string(other_id))?)
    }

    pub fn unrelate_items(&self, item_id: String, other_id: String) -> Result<(), CoreError> {
        Ok(self.lock()?.unrelate_items(&ItemId::from_string(item_id), &ItemId::from_string(other_id))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewItem;

    #[test]
    fn relates_items() {
        let lib = PaperLibrary::in_memory().unwrap();
        let add =
            |title: &str| lib.add_item(NewItem { title: title.into(), ..NewItem::default() }).unwrap().id;
        let (a, b) = (add("Theory"), add("Experiment"));
        lib.relate_items(a.clone(), b.clone()).unwrap();
        assert_eq!(lib.related_items(b.clone()).unwrap()[0].id, a);
        assert!(lib.relate_items(a.clone(), a.clone()).is_err());
        lib.unrelate_items(a.clone(), b).unwrap();
        assert!(lib.related_items(a).unwrap().is_empty());
    }
}
