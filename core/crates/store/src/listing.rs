//! Item lists as the apps show them: filtered (collection, tag, starred,
//! reading status, trash), with each item's stars, tags and PDF count.

use paperloom_model::{Attachment, CollectionId, Item, ItemId, ReadStatus};
use rusqlite::types::Value;
use rusqlite::{OptionalExtension, Row, params, params_from_iter};

use crate::{Error, Library, Result, now_ms};

/// Which items a list shows. The default is every item not in the trash.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub collection: Option<CollectionId>,
    pub tag: Option<String>,
    pub starred_only: bool,
    pub read_status: Option<ReadStatus>,
    /// The trash instead of the library.
    pub trashed: bool,
}

/// An item and what lists show beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub item: Item,
    pub starred: bool,
    pub read_status: ReadStatus,
    /// Milliseconds since 1970.
    pub date_added: i64,
    pub date_modified: i64,
    /// Sorted, ignoring case.
    pub tags: Vec<String>,
    pub pdf_count: u32,
}

/// Separates tag names in `group_concat` (the ASCII unit separator).
const SEPARATOR: char = '\u{1f}';

const ENTRY_COLUMNS: &str = "SELECT i.data, i.starred, i.read_status, i.date_added, i.date_modified,
        (SELECT group_concat(tag, char(31)) FROM item_tags WHERE item_id = i.id),
        (SELECT count(*) FROM attachments WHERE item_id = i.id AND content_type = 'application/pdf')
     FROM items i";

fn read_entry(row: &Row<'_>) -> rusqlite::Result<Result<Entry>> {
    let data: String = row.get(0)?;
    let status: String = row.get(2)?;
    let mut tags: Vec<String> = row
        .get::<_, Option<String>>(5)?
        .map(|t| t.split(SEPARATOR).map(String::from).collect())
        .unwrap_or_default();
    tags.sort_by_key(|t| t.to_lowercase());
    let starred: bool = row.get(1)?;
    let date_added: i64 = row.get(3)?;
    let date_modified: i64 = row.get(4)?;
    let pdf_count: u32 = row.get(6)?;
    Ok(serde_json::from_str(&data).map_err(Error::from).map(|item| Entry {
        item,
        starred,
        read_status: ReadStatus::parse(&status).unwrap_or_default(),
        date_added,
        date_modified,
        tags,
        pdf_count,
    }))
}

impl Library {
    /// The items a filter selects, most recently added first.
    pub fn entries(&self, filter: &Filter) -> Result<Vec<Entry>> {
        let mut sql = format!("{ENTRY_COLUMNS} WHERE i.deleted = ?1");
        let mut values = vec![Value::Integer(i64::from(filter.trashed))];
        let mut condition = |sql_part: &str, value: Value, sql: &mut String| {
            values.push(value);
            sql.push_str(&sql_part.replace("?N", &format!("?{}", values.len())));
        };
        if let Some(collection) = &filter.collection {
            condition(
                " AND i.id IN (SELECT item_id FROM collection_items WHERE collection_id = ?N)",
                Value::Text(collection.to_string()),
                &mut sql,
            );
        }
        if let Some(tag) = &filter.tag {
            condition(
                " AND i.id IN (SELECT item_id FROM item_tags WHERE tag = ?N)",
                Value::Text(tag.clone()),
                &mut sql,
            );
        }
        if let Some(status) = filter.read_status {
            condition(" AND i.read_status = ?N", Value::Text(status.as_str().into()), &mut sql);
        }
        if filter.starred_only {
            sql.push_str(" AND i.starred = 1");
        }
        sql.push_str(" ORDER BY i.date_added DESC, i.id DESC");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values), read_entry)?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row??);
        }
        Ok(entries)
    }

    /// One item with its list details, in the trash or not.
    pub fn entry(&self, id: &ItemId) -> Result<Option<Entry>> {
        self.conn
            .query_row(&format!("{ENTRY_COLUMNS} WHERE i.id = ?1"), [id.as_str()], read_entry)
            .optional()?
            .transpose()
    }

    /// Whether an item is in the trash; `None` if there is no such item.
    pub fn is_trashed(&self, id: &ItemId) -> Result<Option<bool>> {
        Ok(self
            .conn
            .query_row("SELECT deleted FROM items WHERE id = ?1", [id.as_str()], |row| row.get(0))
            .optional()?)
    }

    pub fn set_starred(&mut self, id: &ItemId, starred: bool) -> Result<()> {
        self.set_column(id, "starred", Value::Integer(i64::from(starred)))
    }

    pub fn set_read_status(&mut self, id: &ItemId, status: ReadStatus) -> Result<()> {
        self.set_column(id, "read_status", Value::Text(status.as_str().into()))
    }

    fn set_column(&mut self, id: &ItemId, column: &str, value: Value) -> Result<()> {
        let changed = self.conn.execute(
            &format!("UPDATE items SET {column} = ?2, date_modified = ?3 WHERE id = ?1"),
            params![id.as_str(), value, now_ms()],
        )?;
        if changed == 0 {
            return Err(Error::NotFound(format!("item {id}")));
        }
        Ok(())
    }

    /// Deletes an item for good, with its place in collections, its tags,
    /// attachments and their annotations. Returns the attachments, whose
    /// files the caller deletes.
    pub fn delete_item(&mut self, id: &ItemId) -> Result<Vec<Attachment>> {
        let attachments = self.attachments(id)?;
        let changed = self.conn.execute("DELETE FROM items WHERE id = ?1", [id.as_str()])?;
        if changed == 0 {
            return Err(Error::NotFound(format!("item {id}")));
        }
        Ok(attachments)
    }
}
