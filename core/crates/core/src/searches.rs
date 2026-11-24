//! Saved searches, as Zotero keeps them and EndNote calls smart groups: a
//! name and conditions, whose items the library finds afresh each time.

use paperloom_model::ItemId;
use paperloom_store as store;
use serde::{Deserialize, Serialize};

use crate::{CoreError, ItemSummary, PaperLibrary};

/// One test an item passes or not: `title contains quantum`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SearchCondition {
    /// One of [`search_fields`].
    pub field: String,
    /// One of the field's tests: `contains`, `doesNotContain`, `is`,
    /// `isNot`, `isBefore`, `isAfter`, `inTheLast`, `notInTheLast`.
    pub op: String,
    /// Words; a year; a number of days; an item type, reading status, tag
    /// name or collection ID; `true` or `false`.
    pub value: String,
}

/// A named search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SavedSearch {
    /// Empty for a search not saved yet.
    pub id: String,
    pub name: String,
    /// Items must pass every condition, or else any one.
    pub match_all: bool,
    pub conditions: Vec<SearchCondition>,
}

/// A field conditions can test, for the search editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SearchField {
    /// `anyField`, `title`, `creator`, `publication`, `year`, `tag`,
    /// `collection`, `itemType`, `keyword`, `abstract`, `doi`, `publisher`,
    /// `readStatus`, `starred`, `hasPdf`, `dateAdded`.
    pub id: String,
    /// What its value is: `text`, `year`, `days`, `itemType`, `readStatus`,
    /// `tag`, `collection` or `flag`.
    pub kind: String,
    /// The tests it takes, the usual one first.
    pub ops: Vec<String>,
}

fn kind_name(kind: store::FieldKind) -> &'static str {
    match kind {
        store::FieldKind::Text => "text",
        store::FieldKind::Year => "year",
        store::FieldKind::Days => "days",
        store::FieldKind::ItemType => "itemType",
        store::FieldKind::ReadStatus => "readStatus",
        store::FieldKind::Tag => "tag",
        store::FieldKind::Collection => "collection",
        store::FieldKind::Flag => "flag",
    }
}

/// The fields conditions can test, in the order to offer them.
#[uniffi::export]
pub fn search_fields() -> Vec<SearchField> {
    store::SEARCH_FIELDS
        .iter()
        .map(|(id, kind)| SearchField {
            id: (*id).to_string(),
            kind: kind_name(*kind).to_string(),
            ops: kind.ops().iter().map(|op| op.as_str().to_string()).collect(),
        })
        .collect()
}

impl From<store::SavedSearch> for SavedSearch {
    fn from(search: store::SavedSearch) -> Self {
        Self {
            id: search.id,
            name: search.name,
            match_all: search.match_all,
            conditions: search
                .conditions
                .into_iter()
                .map(|c| SearchCondition { field: c.field, op: c.op, value: c.value })
                .collect(),
        }
    }
}

/// The search as the library keeps it, checked: a name, and conditions
/// the library knows how to test.
fn checked(search: SavedSearch) -> Result<store::SavedSearch, CoreError> {
    let invalid = |message: String| CoreError::InvalidInput { message };
    if search.name.trim().is_empty() {
        return Err(invalid("Name the search.".into()));
    }
    let fields = search_fields();
    for condition in &search.conditions {
        let Some(field) = fields.iter().find(|f| f.id == condition.field) else {
            return Err(invalid(format!("Searches cannot test “{}”.", condition.field)));
        };
        if !field.ops.contains(&condition.op) {
            return Err(invalid(format!("“{}” cannot be tested with “{}”.", condition.field, condition.op)));
        }
        let number = condition.value.trim().parse::<i64>().is_ok();
        let needs_number = field.kind == "days" || (field.kind == "year" && condition.op != "is");
        if needs_number && !number {
            return Err(invalid(format!("“{}” is not a number.", condition.value.trim())));
        }
    }
    Ok(store::SavedSearch {
        id: if search.id.is_empty() { ItemId::new().to_string() } else { search.id },
        name: search.name.trim().to_string(),
        match_all: search.match_all,
        conditions: search
            .conditions
            .into_iter()
            .map(|c| store::Condition { field: c.field, op: c.op, value: c.value.trim().to_string() })
            .collect(),
        position: 0,
    })
}

#[uniffi::export]
impl PaperLibrary {
    /// The saved searches, in their order.
    pub fn saved_searches(&self) -> Result<Vec<SavedSearch>, CoreError> {
        Ok(self.lock()?.saved_searches()?.into_iter().map(SavedSearch::from).collect())
    }

    /// Saves a search: a new one when its ID is empty, else changes the
    /// one with its ID. The search as saved.
    pub fn save_search(&self, search: SavedSearch) -> Result<SavedSearch, CoreError> {
        let search = checked(search)?;
        let mut library = self.lock()?;
        library.save_search(&search)?;
        let saved = library.saved_search(&search.id)?;
        saved
            .map(SavedSearch::from)
            .ok_or_else(|| CoreError::NotFound { message: "The search is gone.".into() })
    }

    pub fn delete_saved_search(&self, search_id: String) -> Result<(), CoreError> {
        Ok(self.lock()?.delete_saved_search(&search_id)?)
    }

    /// The items (not in the trash) a search finds, most recently added
    /// first: a saved search, or one being edited.
    pub fn run_search(&self, search: SavedSearch) -> Result<Vec<ItemSummary>, CoreError> {
        let search = store::SavedSearch {
            id: search.id,
            name: search.name,
            match_all: search.match_all,
            conditions: search
                .conditions
                .into_iter()
                .map(|c| store::Condition { field: c.field, op: c.op, value: c.value })
                .collect(),
            position: 0,
        };
        Ok(self.lock()?.run_search(&search)?.iter().map(ItemSummary::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewItem;

    #[test]
    fn saves_and_runs_searches() {
        let lib = PaperLibrary::in_memory().unwrap();
        for (title, year) in [("Quantum Sensing", 2020), ("Quantum Optics", 2012), ("Rust in Practice", 2021)]
        {
            lib.add_item(NewItem { title: title.into(), year: Some(year), ..NewItem::default() }).unwrap();
        }
        let condition = |field: &str, op: &str, value: &str| SearchCondition {
            field: field.into(),
            op: op.into(),
            value: value.into(),
        };
        let search = SavedSearch {
            id: String::new(),
            name: "Recent quantum".into(),
            match_all: true,
            conditions: vec![
                condition("title", "contains", "quantum"),
                condition("year", "isAfter", " 2015 "),
            ],
        };
        let saved = lib.save_search(search.clone()).unwrap();
        assert!(!saved.id.is_empty());
        assert_eq!(saved.conditions[1].value, "2015");
        let found = lib.run_search(saved.clone()).unwrap();
        assert_eq!(found.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["Quantum Sensing"]);
        assert_eq!(lib.saved_searches().unwrap(), vec![saved.clone()]);

        let renamed = lib.save_search(SavedSearch { name: "Quantum".into(), ..saved.clone() }).unwrap();
        assert_eq!(renamed.id, saved.id);
        assert_eq!(lib.saved_searches().unwrap().len(), 1);

        let bad = |conditions| lib.save_search(SavedSearch { conditions, ..search.clone() });
        assert!(bad(vec![condition("colour", "is", "red")]).is_err());
        assert!(bad(vec![condition("year", "contains", "20")]).is_err());
        assert!(bad(vec![condition("dateAdded", "inTheLast", "a week")]).is_err());
        assert!(lib.save_search(SavedSearch { name: " ".into(), ..search }).is_err());

        lib.delete_saved_search(saved.id).unwrap();
        assert!(lib.saved_searches().unwrap().is_empty());
        assert_eq!(search_fields()[0].id, "anyField");
    }
}
