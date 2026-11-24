//! What sync needs from the library: the records changed here (noted by
//! triggers, schema 5), each record as a flat map of fields, and writing
//! records changed elsewhere without noting them again.
//!
//! Fields are flat so that two devices changing different fields of one
//! record both keep their change: an item's bibliographic data is one field
//! per key (`data.title`), its tags and collections one field per member
//! (`tag:<name>`, `in:<collection id>`). A record gone for good has no
//! fields.

use std::collections::BTreeMap;

use paperloom_model::{Annotation, Item};
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::{Map, Value};

use crate::{Library, Result, now_ms};

/// The kinds of records, in the order they are written: what a record
/// refers to comes before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RecordKind {
    Tag,
    Collection,
    Item,
    Attachment,
    Annotation,
    /// A saved search; it names collections by ID but works without them.
    Search,
}

impl RecordKind {
    pub const ALL: [RecordKind; 6] = [
        RecordKind::Tag,
        RecordKind::Collection,
        RecordKind::Item,
        RecordKind::Attachment,
        RecordKind::Annotation,
        RecordKind::Search,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            RecordKind::Tag => "tag",
            RecordKind::Collection => "collection",
            RecordKind::Item => "item",
            RecordKind::Attachment => "attachment",
            RecordKind::Annotation => "annotation",
            RecordKind::Search => "search",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

pub type Fields = BTreeMap<String, Value>;

/// A record as sync carries it; `fields` is `None` once it is gone.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub kind: RecordKind,
    pub id: String,
    pub fields: Option<Fields>,
}

/// The JSON object a model value serializes to, as `data.<key>` fields.
fn data_fields(value: Value, fields: &mut Fields) {
    if let Value::Object(map) = value {
        for (key, value) in map {
            if key != "id" {
                fields.insert(format!("data.{key}"), value);
            }
        }
    }
}

/// The `data.<key>` fields back as a JSON object, with the ID.
fn data_object(id: &str, fields: &Fields) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), Value::String(id.into()));
    for (key, value) in fields {
        if let Some(key) = key.strip_prefix("data.") {
            map.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(map)
}

fn text(fields: &Fields, key: &str) -> Option<String> {
    fields.get(key).and_then(Value::as_str).map(str::to_string)
}

fn number(fields: &Fields, key: &str) -> Option<i64> {
    fields.get(key).and_then(Value::as_i64)
}

fn flag(fields: &Fields, key: &str) -> bool {
    fields.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn members<'a>(fields: &'a Fields, prefix: &'a str) -> impl Iterator<Item = &'a str> {
    fields
        .iter()
        .filter(|(_, value)| value.as_bool() == Some(true))
        .filter_map(move |(key, _)| key.strip_prefix(prefix))
}

impl Library {
    /// Records changed here since [`Library::forget_changed`] last cleared
    /// them, in the order to write them.
    pub fn changed_records(&self) -> Result<Vec<(RecordKind, String)>> {
        let mut stmt = self.conn.prepare("SELECT kind, id FROM sync_changed")?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        let mut changed = Vec::new();
        for row in rows {
            let (kind, id) = row?;
            if let Some(kind) = RecordKind::parse(&kind) {
                changed.push((kind, id));
            }
        }
        changed.sort();
        Ok(changed)
    }

    pub fn forget_changed(&mut self, records: &[(RecordKind, String)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare("DELETE FROM sync_changed WHERE kind = ?1 AND id = ?2")?;
            for (kind, id) in records {
                stmt.execute([kind.as_str(), id.as_str()])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Notes every record as changed: the first sync sends the whole library.
    pub fn mark_all_changed(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "INSERT OR IGNORE INTO sync_changed SELECT 'tag', name FROM tags;
             INSERT OR IGNORE INTO sync_changed SELECT 'collection', id FROM collections;
             INSERT OR IGNORE INTO sync_changed SELECT 'item', id FROM items;
             INSERT OR IGNORE INTO sync_changed SELECT 'attachment', id FROM attachments;
             INSERT OR IGNORE INTO sync_changed SELECT 'annotation', id FROM annotations;
             INSERT OR IGNORE INTO sync_changed SELECT 'search', id FROM saved_searches;",
        )?;
        Ok(())
    }

    /// A record's fields; `None` if it is not (or no longer) in the library.
    pub fn record(&self, kind: RecordKind, id: &str) -> Result<Option<Fields>> {
        let mut fields = Fields::new();
        match kind {
            RecordKind::Tag => {
                let Some(color) = self
                    .conn
                    .query_row("SELECT color FROM tags WHERE name = ?1", [id], |row| {
                        row.get::<_, Option<String>>(0)
                    })
                    .optional()?
                else {
                    return Ok(None);
                };
                fields.insert("color".into(), color.map_or(Value::Null, Value::String));
            }
            RecordKind::Collection => {
                let Some((name, parent, position)) = self
                    .conn
                    .query_row(
                        "SELECT name, parent_id, position FROM collections WHERE id = ?1",
                        [id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, Option<String>>(1)?,
                                row.get::<_, i64>(2)?,
                            ))
                        },
                    )
                    .optional()?
                else {
                    return Ok(None);
                };
                fields.insert("name".into(), name.into());
                fields.insert("parent".into(), parent.map_or(Value::Null, Value::String));
                fields.insert("position".into(), position.into());
            }
            RecordKind::Item => {
                let Some((data, starred, read_status, trashed, added, modified)) = self
                    .conn
                    .query_row(
                        "SELECT data, starred, read_status, deleted, date_added, date_modified
                         FROM items WHERE id = ?1",
                        [id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, bool>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, bool>(3)?,
                                row.get::<_, i64>(4)?,
                                row.get::<_, i64>(5)?,
                            ))
                        },
                    )
                    .optional()?
                else {
                    return Ok(None);
                };
                data_fields(serde_json::from_str(&data)?, &mut fields);
                fields.insert("starred".into(), starred.into());
                fields.insert("readStatus".into(), read_status.into());
                fields.insert("trashed".into(), trashed.into());
                fields.insert("dateAdded".into(), added.into());
                fields.insert("dateModified".into(), modified.into());
                let mut stmt = self.conn.prepare("SELECT tag FROM item_tags WHERE item_id = ?1")?;
                for tag in stmt.query_map([id], |row| row.get::<_, String>(0))? {
                    fields.insert(format!("tag:{}", tag?), true.into());
                }
                let mut stmt =
                    self.conn.prepare("SELECT collection_id FROM collection_items WHERE item_id = ?1")?;
                for collection in stmt.query_map([id], |row| row.get::<_, String>(0))? {
                    fields.insert(format!("in:{}", collection?), true.into());
                }
                let mut stmt =
                    self.conn.prepare("SELECT related_id FROM item_relations WHERE item_id = ?1")?;
                for related in stmt.query_map([id], |row| row.get::<_, String>(0))? {
                    fields.insert(format!("related:{}", related?), true.into());
                }
            }
            RecordKind::Attachment => {
                let Some(attachment) = self.attachment(&paperloom_model::AttachmentId::from_string(id))?
                else {
                    return Ok(None);
                };
                fields.insert("item".into(), attachment.item_id.as_str().into());
                fields.insert("title".into(), attachment.title.into());
                fields.insert("path".into(), attachment.path.into());
                fields.insert("contentType".into(), attachment.content_type.into());
                fields.insert("size".into(), attachment.size.into());
                fields.insert("sha256".into(), attachment.sha256.into());
                fields.insert("dateAdded".into(), attachment.date_added.into());
            }
            RecordKind::Annotation => {
                let Some(data) = self
                    .conn
                    .query_row("SELECT data FROM annotations WHERE id = ?1", [id], |row| {
                        row.get::<_, String>(0)
                    })
                    .optional()?
                else {
                    return Ok(None);
                };
                data_fields(serde_json::from_str(&data)?, &mut fields);
            }
            RecordKind::Search => {
                let Some((name, data, position)) = self
                    .conn
                    .query_row("SELECT name, data, position FROM saved_searches WHERE id = ?1", [id], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
                    })
                    .optional()?
                else {
                    return Ok(None);
                };
                fields.insert("name".into(), name.into());
                fields.insert("position".into(), position.into());
                data_fields(serde_json::from_str(&data)?, &mut fields);
            }
        }
        Ok(Some(fields))
    }

    /// Writes records changed elsewhere, in one transaction, without noting
    /// them as changed here. A record whose parent is missing (an
    /// attachment of an item gone for good) is skipped.
    pub fn apply_records(&mut self, records: &[Record]) -> Result<()> {
        let mut records: Vec<&Record> = records.iter().collect();
        records.sort_by_key(|r| r.kind);
        let tx = self.conn.transaction()?;
        tx.execute("UPDATE sync_state SET value = 1 WHERE key = 'applying'", [])?;
        // Collections first without parents, which may come later in the batch.
        for record in &records {
            apply(&tx, record)?;
        }
        for record in records.iter().filter(|r| r.kind == RecordKind::Collection) {
            if let Some(fields) = &record.fields {
                tx.execute(
                    "UPDATE collections SET parent_id = ?2
                     WHERE id = ?1 AND (?2 IS NULL OR EXISTS (SELECT 1 FROM collections WHERE id = ?2))",
                    params![record.id, text(fields, "parent")],
                )?;
            }
        }
        tx.execute("UPDATE sync_state SET value = 0 WHERE key = 'applying'", [])?;
        tx.commit()?;
        Ok(())
    }
}

fn apply(tx: &Transaction<'_>, record: &Record) -> Result<()> {
    let id = record.id.as_str();
    let Some(fields) = &record.fields else {
        let sql = match record.kind {
            RecordKind::Tag => "DELETE FROM tags WHERE name = ?1",
            RecordKind::Collection => "DELETE FROM collections WHERE id = ?1",
            RecordKind::Item => "DELETE FROM items WHERE id = ?1",
            RecordKind::Attachment => "DELETE FROM attachments WHERE id = ?1",
            RecordKind::Annotation => "DELETE FROM annotations WHERE id = ?1",
            RecordKind::Search => "DELETE FROM saved_searches WHERE id = ?1",
        };
        tx.execute(sql, [id])?;
        return Ok(());
    };
    match record.kind {
        RecordKind::Tag => {
            tx.execute(
                "INSERT INTO tags (name, color) VALUES (?1, ?2) ON CONFLICT (name) DO UPDATE SET color = ?2",
                params![id, text(fields, "color")],
            )?;
        }
        RecordKind::Collection => {
            tx.execute(
                "INSERT INTO collections (id, name, position) VALUES (?1, ?2, ?3)
                 ON CONFLICT (id) DO UPDATE SET name = ?2, position = ?3",
                params![
                    id,
                    text(fields, "name").unwrap_or_default(),
                    number(fields, "position").unwrap_or(0)
                ],
            )?;
        }
        RecordKind::Item => {
            let item: Item = serde_json::from_value(data_object(id, fields))?;
            let data = serde_json::to_string(&item)?;
            let added = number(fields, "dateAdded").unwrap_or_else(now_ms);
            let modified = number(fields, "dateModified").unwrap_or_else(now_ms);
            let status = text(fields, "readStatus").unwrap_or_else(|| "unread".into());
            let write = |doi: Option<&str>| {
                tx.execute(
                    "INSERT INTO items (id, item_type, title, doi, year, data, date_added, date_modified,
                        deleted, starred, read_status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                     ON CONFLICT (id) DO UPDATE SET item_type = ?2, title = ?3, doi = ?4, year = ?5,
                        data = ?6, date_added = ?7, date_modified = ?8, deleted = ?9, starred = ?10,
                        read_status = ?11",
                    params![
                        id,
                        item.item_type.as_str(),
                        item.title,
                        doi,
                        item.year,
                        data,
                        added,
                        modified,
                        flag(fields, "trashed"),
                        flag(fields, "starred"),
                        status,
                    ],
                )
            };
            if let Err(err) = write(item.doi.as_ref().map(|d| d.as_str())) {
                // Another item here already has this DOI (both devices added
                // the paper): keep both, this one without the DOI column.
                if !is_constraint(&err) {
                    return Err(err.into());
                }
                write(None)?;
            }
            tx.execute("DELETE FROM item_tags WHERE item_id = ?1", [id])?;
            for tag in members(fields, "tag:") {
                tx.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [tag])?;
                tx.execute("INSERT OR IGNORE INTO item_tags (item_id, tag) VALUES (?1, ?2)", [id, tag])?;
            }
            tx.execute("DELETE FROM collection_items WHERE item_id = ?1", [id])?;
            for collection in members(fields, "in:") {
                tx.execute(
                    "INSERT OR IGNORE INTO collection_items (collection_id, item_id)
                     SELECT ?1, ?2 WHERE EXISTS (SELECT 1 FROM collections WHERE id = ?1)",
                    [collection, id],
                )?;
            }
            // The other item may come later in the batch, or not at all:
            // lists show only related items that are here.
            tx.execute("DELETE FROM item_relations WHERE item_id = ?1", [id])?;
            for related in members(fields, "related:") {
                tx.execute(
                    "INSERT OR IGNORE INTO item_relations (item_id, related_id) VALUES (?1, ?2)",
                    [id, related],
                )?;
            }
        }
        RecordKind::Attachment => {
            let Some(item) = text(fields, "item") else { return Ok(()) };
            tx.execute(
                "INSERT INTO attachments (id, item_id, title, path, content_type, size, sha256, date_added)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8 WHERE EXISTS (SELECT 1 FROM items WHERE id = ?2)
                 ON CONFLICT (id) DO UPDATE SET item_id = ?2, title = ?3, path = ?4, content_type = ?5,
                    size = ?6, sha256 = ?7, date_added = ?8",
                params![
                    id,
                    item,
                    text(fields, "title").unwrap_or_default(),
                    text(fields, "path").unwrap_or_default(),
                    text(fields, "contentType").unwrap_or_default(),
                    number(fields, "size").unwrap_or(0),
                    text(fields, "sha256").unwrap_or_default(),
                    number(fields, "dateAdded").unwrap_or_else(now_ms),
                ],
            )?;
        }
        RecordKind::Annotation => {
            let annotation: Annotation = serde_json::from_value(data_object(id, fields))?;
            tx.execute(
                "INSERT INTO annotations (id, attachment_id, page_index, data, date_added, date_modified)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6 WHERE EXISTS (SELECT 1 FROM attachments WHERE id = ?2)
                 ON CONFLICT (id) DO UPDATE SET attachment_id = ?2, page_index = ?3, data = ?4,
                    date_added = ?5, date_modified = ?6",
                params![
                    id,
                    annotation.attachment_id.as_str(),
                    annotation.page_index,
                    serde_json::to_string(&annotation)?,
                    annotation.date_added,
                    annotation.date_modified,
                ],
            )?;
        }
        RecordKind::Search => {
            // The conditions as stored: `data.matchAll`, `data.conditions`.
            let mut data = data_object(id, fields);
            if let Value::Object(map) = &mut data {
                map.remove("id");
            }
            tx.execute(
                "INSERT INTO saved_searches (id, name, data, position, date_modified) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (id) DO UPDATE SET name = ?2, data = ?3, position = ?4, date_modified = ?5",
                params![
                    id,
                    text(fields, "name").unwrap_or_default(),
                    serde_json::to_string(&data)?,
                    number(fields, "position").unwrap_or(0),
                    now_ms(),
                ],
            )?;
        }
    }
    Ok(())
}

fn is_constraint(err: &rusqlite::Error) -> bool {
    matches!(err, rusqlite::Error::SqliteFailure(e, _) if e.code == rusqlite::ErrorCode::ConstraintViolation)
}
