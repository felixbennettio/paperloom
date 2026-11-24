//! Saved searches, as Zotero keeps them and EndNote calls smart groups: a
//! name and conditions, whose items the library finds afresh each time.

use std::collections::HashMap;

use paperloom_model::Item;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{Entry, Error, Filter, Library, Result, now_ms};

/// One test an item passes or not: `title contains quantum`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Condition {
    /// What is tested: see [`FIELDS`].
    pub field: String,
    /// How: see [`Op`].
    pub op: String,
    pub value: String,
}

/// A named search.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedSearch {
    pub id: String,
    pub name: String,
    /// Items must pass every condition, or else any one.
    pub match_all: bool,
    pub conditions: Vec<Condition>,
    pub position: i64,
}

/// What the conditions are stored as.
#[derive(Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Stored {
    match_all: bool,
    conditions: Vec<Condition>,
}

/// The tests a condition can make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Contains,
    DoesNotContain,
    Is,
    IsNot,
    IsBefore,
    IsAfter,
    InTheLast,
    NotInTheLast,
}

impl Op {
    pub const ALL: [Op; 8] = [
        Op::Contains,
        Op::DoesNotContain,
        Op::Is,
        Op::IsNot,
        Op::IsBefore,
        Op::IsAfter,
        Op::InTheLast,
        Op::NotInTheLast,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Op::Contains => "contains",
            Op::DoesNotContain => "doesNotContain",
            Op::Is => "is",
            Op::IsNot => "isNot",
            Op::IsBefore => "isBefore",
            Op::IsAfter => "isAfter",
            Op::InTheLast => "inTheLast",
            Op::NotInTheLast => "notInTheLast",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|op| op.as_str() == s)
    }
}

/// What a field holds, which says the tests it takes and what its value is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Words: contains, does not contain, is, is not.
    Text,
    /// A year: is, is not, is before, is after.
    Year,
    /// Days back from now: in the last, not in the last.
    Days,
    /// One of the item types.
    ItemType,
    /// `unread`, `reading` or `read`.
    ReadStatus,
    /// A tag's name.
    Tag,
    /// A collection's ID.
    Collection,
    /// `true` or `false`.
    Flag,
}

impl FieldKind {
    pub fn ops(self) -> &'static [Op] {
        match self {
            FieldKind::Text | FieldKind::Tag => &[Op::Contains, Op::DoesNotContain, Op::Is, Op::IsNot],
            FieldKind::Year => &[Op::Is, Op::IsNot, Op::IsBefore, Op::IsAfter],
            FieldKind::Days => &[Op::InTheLast, Op::NotInTheLast],
            FieldKind::ItemType | FieldKind::ReadStatus | FieldKind::Collection | FieldKind::Flag => {
                &[Op::Is, Op::IsNot]
            }
        }
    }
}

/// The fields a condition can test, in the order to offer them.
pub const FIELDS: [(&str, FieldKind); 16] = [
    ("anyField", FieldKind::Text),
    ("title", FieldKind::Text),
    ("creator", FieldKind::Text),
    ("publication", FieldKind::Text),
    ("year", FieldKind::Year),
    ("tag", FieldKind::Tag),
    ("collection", FieldKind::Collection),
    ("itemType", FieldKind::ItemType),
    ("keyword", FieldKind::Text),
    ("abstract", FieldKind::Text),
    ("doi", FieldKind::Text),
    ("publisher", FieldKind::Text),
    ("readStatus", FieldKind::ReadStatus),
    ("starred", FieldKind::Flag),
    ("hasPdf", FieldKind::Flag),
    ("dateAdded", FieldKind::Days),
];

fn kind_of(field: &str) -> Option<FieldKind> {
    FIELDS.iter().find(|(name, _)| *name == field).map(|(_, kind)| *kind)
}

/// The text an item has for a field, each piece lower case.
fn values(entry: &Entry, field: &str, collections: &HashMap<String, Vec<String>>) -> Vec<String> {
    let item: &Item = &entry.item;
    let one = |value: Option<&str>| value.map(str::to_lowercase).into_iter().collect::<Vec<_>>();
    match field {
        "title" => one(Some(&item.title)),
        "creator" => item.creators.iter().map(|c| c.display_name().to_lowercase()).collect(),
        "publication" => [&item.container_title, &item.container_short_title]
            .into_iter()
            .filter_map(|v| v.as_deref().map(str::to_lowercase))
            .collect(),
        "year" => one(item.year.map(|y| y.to_string()).as_deref()),
        "tag" => entry.tags.iter().map(|t| t.to_lowercase()).collect(),
        "collection" => collections.get(item.id.as_str()).cloned().unwrap_or_default(),
        "itemType" => one(Some(item.item_type.as_str())),
        "keyword" => item.keywords.iter().map(|k| k.to_lowercase()).collect(),
        "abstract" => one(item.abstract_note.as_deref()),
        "doi" => one(item.doi.as_ref().map(|d| d.as_str())),
        "publisher" => one(item.publisher.as_deref()),
        "readStatus" => one(Some(entry.read_status.as_str())),
        "starred" => one(Some(if entry.starred { "true" } else { "false" })),
        "hasPdf" => one(Some(if entry.pdf_count > 0 { "true" } else { "false" })),
        "anyField" => {
            ["title", "creator", "publication", "year", "tag", "keyword", "abstract", "doi", "publisher"]
                .into_iter()
                .flat_map(|f| values(entry, f, collections))
                .chain(
                    [&item.isbn, &item.issn, &item.url, &item.arxiv_id, &item.pmid]
                        .into_iter()
                        .filter_map(|v| v.as_deref().map(str::to_lowercase)),
                )
                .collect()
        }
        _ => Vec::new(),
    }
}

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// Whether an item passes a condition. A condition the library cannot
/// read (a field or test from a newer version) passes no item.
fn passes(
    entry: &Entry,
    condition: &Condition,
    collections: &HashMap<String, Vec<String>>,
    now: i64,
) -> bool {
    let (Some(kind), Some(op)) = (kind_of(&condition.field), Op::parse(&condition.op)) else {
        return false;
    };
    if !kind.ops().contains(&op) {
        return false;
    }
    let value = condition.value.trim();
    if kind == FieldKind::Days {
        let Ok(days) = value.parse::<i64>() else { return false };
        let recent = entry.date_added >= now - days.max(0) * DAY_MS;
        return (op == Op::InTheLast) == recent;
    }
    let needle = if kind == FieldKind::Collection { value.to_string() } else { value.to_lowercase() };
    let have = values(entry, &condition.field, collections);
    match op {
        Op::Contains => have.iter().any(|h| h.contains(&needle)),
        Op::DoesNotContain => !have.iter().any(|h| h.contains(&needle)),
        Op::Is => have.contains(&needle),
        Op::IsNot => !have.contains(&needle),
        Op::IsBefore | Op::IsAfter => {
            let (Ok(year), Some(Ok(have))) = (needle.parse::<i32>(), have.first().map(|h| h.parse::<i32>()))
            else {
                return false;
            };
            if op == Op::IsBefore { have < year } else { have > year }
        }
        Op::InTheLast | Op::NotInTheLast => false,
    }
}

impl Library {
    /// The saved searches, in their order.
    pub fn saved_searches(&self) -> Result<Vec<SavedSearch>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, data, position FROM saved_searches ORDER BY position, name")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut searches = Vec::new();
        for row in rows {
            let (id, name, data, position) = row?;
            let stored: Stored = serde_json::from_str(&data)?;
            searches.push(SavedSearch {
                id,
                name,
                match_all: stored.match_all,
                conditions: stored.conditions,
                position,
            });
        }
        Ok(searches)
    }

    pub fn saved_search(&self, id: &str) -> Result<Option<SavedSearch>> {
        Ok(self.saved_searches()?.into_iter().find(|s| s.id == id))
    }

    /// Adds a search, or changes the one with its ID. A new search goes
    /// last; a changed one keeps its place.
    pub fn save_search(&mut self, search: &SavedSearch) -> Result<()> {
        let data = serde_json::to_string(&Stored {
            match_all: search.match_all,
            conditions: search.conditions.clone(),
        })?;
        let position: i64 = match self
            .conn
            .query_row("SELECT position FROM saved_searches WHERE id = ?1", [&search.id], |row| row.get(0))
            .optional()?
        {
            Some(position) => position,
            None => self.conn.query_row(
                "SELECT coalesce(max(position) + 1, 0) FROM saved_searches",
                [],
                |row| row.get(0),
            )?,
        };
        self.conn.execute(
            "INSERT INTO saved_searches (id, name, data, position, date_modified) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (id) DO UPDATE SET name = ?2, data = ?3, date_modified = ?5",
            params![search.id, search.name.trim(), data, position, now_ms()],
        )?;
        Ok(())
    }

    pub fn delete_saved_search(&mut self, id: &str) -> Result<()> {
        if self.conn.execute("DELETE FROM saved_searches WHERE id = ?1", [id])? == 0 {
            return Err(Error::NotFound(format!("saved search {id}")));
        }
        Ok(())
    }

    /// The items (not in the trash) a search finds, most recently added
    /// first. With no conditions, every item.
    pub fn run_search(&self, search: &SavedSearch) -> Result<Vec<Entry>> {
        let mut collections: HashMap<String, Vec<String>> = HashMap::new();
        if search.conditions.iter().any(|c| c.field == "collection") {
            let mut stmt = self.conn.prepare("SELECT item_id, collection_id FROM collection_items")?;
            for row in stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))? {
                let (item, collection) = row?;
                collections.entry(item).or_default().push(collection);
            }
        }
        let now = now_ms();
        let entries = self.entries(&Filter::default())?;
        if search.conditions.is_empty() {
            return Ok(entries);
        }
        Ok(entries
            .into_iter()
            .filter(|entry| {
                let mut tests = search.conditions.iter().map(|c| passes(entry, c, &collections, now));
                if search.match_all { tests.all(|t| t) } else { tests.any(|t| t) }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperloom_model::{Creator, ItemType, ReadStatus};

    fn entry(title: &str, year: i32, tags: &[&str]) -> Entry {
        let mut item = Item::new(ItemType::JournalArticle, title);
        item.year = Some(year);
        item.creators.push(Creator::person("Curie", "Marie"));
        Entry {
            item,
            starred: false,
            read_status: ReadStatus::Unread,
            date_added: 0,
            date_modified: 0,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            pdf_count: 0,
        }
    }

    fn test(field: &str, op: &str, value: &str) -> Condition {
        Condition { field: field.into(), op: op.into(), value: value.into() }
    }

    #[test]
    fn tests_items_against_conditions() {
        let none = HashMap::new();
        let e = entry("Quantum Sensing", 2020, &["Review"]);
        let pass = |c: Condition| passes(&e, &c, &none, 10 * DAY_MS);
        assert!(pass(test("title", "contains", "quantum")));
        assert!(!pass(test("title", "doesNotContain", "QUANTUM")));
        assert!(pass(test("creator", "contains", "curie")));
        assert!(pass(test("tag", "is", "review")));
        assert!(pass(test("tag", "isNot", "draft")));
        assert!(pass(test("year", "isAfter", "2019")));
        assert!(!pass(test("year", "isBefore", "2020")));
        assert!(pass(test("anyField", "contains", "sensing")));
        assert!(pass(test("hasPdf", "is", "false")));
        assert!(pass(test("readStatus", "is", "unread")));
        assert!(!pass(test("dateAdded", "inTheLast", "7")));
        assert!(pass(test("dateAdded", "notInTheLast", "7")));
        // Tests the field does not take, and unknown ones, pass nothing.
        assert!(!pass(test("year", "contains", "20")));
        assert!(!pass(test("colour", "is", "red")));
        let mut collections = HashMap::new();
        collections.insert(e.item.id.to_string(), vec!["C1".to_string()]);
        assert!(passes(&e, &test("collection", "is", "C1"), &collections, 0));
        assert!(!passes(&e, &test("collection", "is", "c1"), &collections, 0));
    }
}
