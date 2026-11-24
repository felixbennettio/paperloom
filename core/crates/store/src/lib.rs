//! The local library database.
//!
//! One SQLite file per library. Items are stored whole as JSON next to the
//! columns that are queried (type, title, DOI, year), so the model can grow
//! without a migration for every new field.

mod duplicates;
mod feeds;
mod listing;
mod related;
mod schema;
mod searches;
mod sync;
mod tags;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use paperloom_model::{
    Annotation, AnnotationId, Attachment, AttachmentId, Collection, CollectionId, Doi, Item, ItemId,
};

pub use feeds::{FeedItemRow, FeedRow, NewFeedEntry};
pub use listing::{Entry, Filter};
use rusqlite::{Connection, OptionalExtension, Row, params};
pub use searches::{Condition, FIELDS as SEARCH_FIELDS, FieldKind, Op, SavedSearch};
pub use sync::{Fields, Record, RecordKind};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("stored data is unreadable: {0}")]
    Json(#[from] serde_json::Error),
    #[error("an item with DOI {doi} is already in the library")]
    DuplicateDoi { doi: Doi, existing: ItemId },
    #[error("{0} not found")]
    NotFound(String),
    #[error("the library was written by a newer version of Paperloom (schema {0})")]
    NewerSchema(i64),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Library {
    conn: Connection,
}

impl Library {
    /// Opens (or creates) the library database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// A library that lives only in memory, for tests and previews.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0))?;
        schema::migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Writes a consistent copy of the whole library to `path` (which must
    /// not exist yet), while it stays in use.
    pub fn back_up_to(&self, path: &Path) -> Result<()> {
        self.conn.execute("VACUUM INTO ?1", [path.to_string_lossy()])?;
        Ok(())
    }

    // Items

    /// Adds a new item. Fails with [`Error::DuplicateDoi`] when another item
    /// in the library (not in the trash) has the same DOI.
    pub fn add_item(&mut self, item: &Item) -> Result<()> {
        self.ensure_doi_is_free(item)?;
        let now = now_ms();
        self.conn.execute(
            "INSERT INTO items (id, item_type, title, doi, year, data, date_added, date_modified)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                item.id.as_str(),
                item.item_type.as_str(),
                item.title,
                item.doi.as_ref().map(Doi::as_str),
                item.year,
                serde_json::to_string(item)?,
                now,
            ],
        )?;
        Ok(())
    }

    /// Replaces an item's data.
    pub fn update_item(&mut self, item: &Item) -> Result<()> {
        self.ensure_doi_is_free(item)?;
        let changed = self.conn.execute(
            "UPDATE items SET item_type = ?2, title = ?3, doi = ?4, year = ?5, data = ?6, date_modified = ?7
             WHERE id = ?1",
            params![
                item.id.as_str(),
                item.item_type.as_str(),
                item.title,
                item.doi.as_ref().map(Doi::as_str),
                item.year,
                serde_json::to_string(item)?,
                now_ms(),
            ],
        )?;
        if changed == 0 {
            return Err(Error::NotFound(format!("item {}", item.id)));
        }
        Ok(())
    }

    pub fn item(&self, id: &ItemId) -> Result<Option<Item>> {
        self.conn
            .query_row("SELECT data FROM items WHERE id = ?1", [id.as_str()], read_item)
            .optional()?
            .transpose()
    }

    /// The item with this DOI, unless it is in the trash.
    pub fn find_by_doi(&self, doi: &Doi) -> Result<Option<Item>> {
        self.conn
            .query_row("SELECT data FROM items WHERE doi = ?1 AND deleted = 0", [doi.as_str()], read_item)
            .optional()?
            .transpose()
    }

    /// All items not in the trash, most recently added first.
    pub fn items(&self) -> Result<Vec<Item>> {
        self.query_items("SELECT data FROM items WHERE deleted = 0 ORDER BY date_added DESC, id DESC", [])
    }

    /// Items in the trash, most recently added first.
    pub fn trashed_items(&self) -> Result<Vec<Item>> {
        self.query_items("SELECT data FROM items WHERE deleted = 1 ORDER BY date_added DESC, id DESC", [])
    }

    /// Moves an item to the trash; [`Library::restore_item`] brings it back.
    pub fn trash_item(&mut self, id: &ItemId) -> Result<()> {
        self.set_deleted(id, true)
    }

    pub fn restore_item(&mut self, id: &ItemId) -> Result<()> {
        if let Some(item) = self.item(id)? {
            self.ensure_doi_is_free(&item)?;
        }
        self.set_deleted(id, false)
    }

    /// Fails with [`Error::DuplicateDoi`] when another live item (not in
    /// the trash) already has this item's DOI.
    fn ensure_doi_is_free(&self, item: &Item) -> Result<()> {
        if let Some(doi) = &item.doi
            && let Some(existing) = self.find_by_doi(doi)?
            && existing.id != item.id
        {
            return Err(Error::DuplicateDoi { doi: doi.clone(), existing: existing.id });
        }
        Ok(())
    }

    fn set_deleted(&mut self, id: &ItemId, deleted: bool) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE items SET deleted = ?2, date_modified = ?3 WHERE id = ?1",
            params![id.as_str(), deleted, now_ms()],
        )?;
        if changed == 0 {
            return Err(Error::NotFound(format!("item {id}")));
        }
        Ok(())
    }

    fn query_items<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<Vec<Item>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params, read_item)?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row??);
        }
        Ok(items)
    }

    // Collections

    /// Creates a collection at the end of its parent's children.
    pub fn create_collection(&mut self, name: &str, parent_id: Option<&CollectionId>) -> Result<Collection> {
        if let Some(parent) = parent_id {
            self.collection(parent)?.ok_or_else(|| Error::NotFound(format!("collection {parent}")))?;
        }
        let parent = parent_id.map(CollectionId::as_str);
        let position: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM collections WHERE parent_id IS ?1",
            [parent],
            |row| row.get(0),
        )?;
        let mut collection = Collection::new(name.trim(), parent_id.cloned());
        collection.position = position;
        self.conn.execute(
            "INSERT INTO collections (id, name, parent_id, position) VALUES (?1, ?2, ?3, ?4)",
            params![collection.id.as_str(), collection.name, parent, collection.position],
        )?;
        Ok(collection)
    }

    pub fn collection(&self, id: &CollectionId) -> Result<Option<Collection>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, parent_id, position FROM collections WHERE id = ?1",
                [id.as_str()],
                read_collection,
            )
            .optional()?)
    }

    /// Every collection: top-level ones first, then grouped by parent, each
    /// group in sibling order.
    pub fn collections(&self) -> Result<Vec<Collection>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, parent_id, position FROM collections
             ORDER BY parent_id IS NOT NULL, parent_id, position, name",
        )?;
        let rows = stmt.query_map([], read_collection)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn rename_collection(&mut self, id: &CollectionId, name: &str) -> Result<()> {
        let changed = self
            .conn
            .execute("UPDATE collections SET name = ?2 WHERE id = ?1", params![id.as_str(), name.trim()])?;
        if changed == 0 {
            return Err(Error::NotFound(format!("collection {id}")));
        }
        Ok(())
    }

    /// Deletes a collection and its sub-collections. The items stay in the
    /// library.
    pub fn delete_collection(&mut self, id: &CollectionId) -> Result<()> {
        let changed = self.conn.execute("DELETE FROM collections WHERE id = ?1", [id.as_str()])?;
        if changed == 0 {
            return Err(Error::NotFound(format!("collection {id}")));
        }
        Ok(())
    }

    pub fn add_to_collection(&mut self, collection: &CollectionId, item: &ItemId) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO collection_items (collection_id, item_id) VALUES (?1, ?2)",
            params![collection.as_str(), item.as_str()],
        )?;
        Ok(())
    }

    pub fn remove_from_collection(&mut self, collection: &CollectionId, item: &ItemId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM collection_items WHERE collection_id = ?1 AND item_id = ?2",
            params![collection.as_str(), item.as_str()],
        )?;
        Ok(())
    }

    /// Items directly in a collection (not in its sub-collections), most
    /// recently added first.
    pub fn items_in_collection(&self, collection: &CollectionId) -> Result<Vec<Item>> {
        self.query_items(
            "SELECT i.data FROM items i
             JOIN collection_items ci ON ci.item_id = i.id
             WHERE ci.collection_id = ?1 AND i.deleted = 0
             ORDER BY i.date_added DESC, i.id DESC",
            [collection.as_str()],
        )
    }
}

impl Library {
    // Attachments

    pub fn add_attachment(&mut self, attachment: &Attachment) -> Result<()> {
        self.conn.execute(
            "INSERT INTO attachments (id, item_id, title, path, content_type, size, sha256, date_added)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                attachment.id.as_str(),
                attachment.item_id.as_str(),
                attachment.title,
                attachment.path,
                attachment.content_type,
                attachment.size as i64,
                attachment.sha256,
                attachment.date_added,
            ],
        )?;
        Ok(())
    }

    pub fn attachment(&self, id: &AttachmentId) -> Result<Option<Attachment>> {
        Ok(self
            .conn
            .query_row(&format!("{ATTACHMENT_COLUMNS} WHERE id = ?1"), [id.as_str()], read_attachment)
            .optional()?)
    }

    /// An item's attachments, oldest first.
    pub fn attachments(&self, item: &ItemId) -> Result<Vec<Attachment>> {
        let mut stmt =
            self.conn.prepare(&format!("{ATTACHMENT_COLUMNS} WHERE item_id = ?1 ORDER BY date_added, id"))?;
        let rows = stmt.query_map([item.as_str()], read_attachment)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// An attachment of this item with exactly this content, if any.
    pub fn find_attachment(&self, item: &ItemId, sha256: &str) -> Result<Option<Attachment>> {
        Ok(self
            .conn
            .query_row(
                &format!("{ATTACHMENT_COLUMNS} WHERE item_id = ?1 AND sha256 = ?2"),
                [item.as_str(), sha256],
                read_attachment,
            )
            .optional()?)
    }

    /// Records that an attachment's file changed (its pages were edited).
    pub fn set_attachment_content(&mut self, id: &AttachmentId, size: u64, sha256: &str) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE attachments SET size = ?2, sha256 = ?3 WHERE id = ?1",
            params![id.as_str(), size as i64, sha256],
        )?;
        if changed == 0 {
            return Err(Error::NotFound(format!("attachment {id}")));
        }
        Ok(())
    }

    /// Forgets an attachment; the caller removes its file.
    pub fn remove_attachment(&mut self, id: &AttachmentId) -> Result<()> {
        let changed = self.conn.execute("DELETE FROM attachments WHERE id = ?1", [id.as_str()])?;
        if changed == 0 {
            return Err(Error::NotFound(format!("attachment {id}")));
        }
        Ok(())
    }
}

impl Library {
    // Annotations

    pub fn add_annotation(&mut self, annotation: &Annotation) -> Result<()> {
        self.conn.execute(
            "INSERT INTO annotations (id, attachment_id, page_index, data, date_added, date_modified)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                annotation.id.as_str(),
                annotation.attachment_id.as_str(),
                annotation.page_index,
                serde_json::to_string(annotation)?,
                annotation.date_added,
                annotation.date_modified,
            ],
        )?;
        Ok(())
    }

    /// Replaces an annotation's data (color, comment…).
    pub fn update_annotation(&mut self, annotation: &Annotation) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE annotations SET page_index = ?2, data = ?3, date_modified = ?4 WHERE id = ?1",
            params![
                annotation.id.as_str(),
                annotation.page_index,
                serde_json::to_string(annotation)?,
                annotation.date_modified,
            ],
        )?;
        if changed == 0 {
            return Err(Error::NotFound(format!("annotation {}", annotation.id)));
        }
        Ok(())
    }

    pub fn annotation(&self, id: &AnnotationId) -> Result<Option<Annotation>> {
        self.conn
            .query_row("SELECT data FROM annotations WHERE id = ?1", [id.as_str()], read_json)
            .optional()?
            .transpose()
    }

    /// An attachment's annotations, by page, then oldest first.
    pub fn annotations(&self, attachment: &AttachmentId) -> Result<Vec<Annotation>> {
        let mut stmt = self.conn.prepare(
            "SELECT data FROM annotations WHERE attachment_id = ?1 ORDER BY page_index, date_added, id",
        )?;
        let rows = stmt.query_map([attachment.as_str()], read_json)?;
        let mut annotations = Vec::new();
        for row in rows {
            annotations.push(row??);
        }
        Ok(annotations)
    }

    pub fn remove_annotation(&mut self, id: &AnnotationId) -> Result<()> {
        let changed = self.conn.execute("DELETE FROM annotations WHERE id = ?1", [id.as_str()])?;
        if changed == 0 {
            return Err(Error::NotFound(format!("annotation {id}")));
        }
        Ok(())
    }
}

fn read_json<T: serde::de::DeserializeOwned>(row: &Row<'_>) -> rusqlite::Result<Result<T>> {
    let data: String = row.get(0)?;
    Ok(serde_json::from_str(&data).map_err(Error::from))
}

impl Library {
    // Page text

    /// Replaces the text kept for an attachment's pages (from 0).
    pub fn set_attachment_pages(&mut self, attachment: &AttachmentId, pages: &[String]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM attachment_pages WHERE attachment_id = ?1", [attachment.as_str()])?;
        for (page, text) in pages.iter().enumerate() {
            if !text.trim().is_empty() {
                tx.execute(
                    "INSERT INTO attachment_pages (attachment_id, page, text) VALUES (?1, ?2, ?3)",
                    params![attachment.as_str(), page as i64, text],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The kept page text of an attachment: (page from 0, text).
    pub fn attachment_pages(&self, attachment: &AttachmentId) -> Result<Vec<(u32, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT page, text FROM attachment_pages WHERE attachment_id = ?1 ORDER BY page")?;
        let rows = stmt.query_map([attachment.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Every attachment, oldest first.
    pub fn all_attachments(&self) -> Result<Vec<Attachment>> {
        let mut stmt = self.conn.prepare(&format!("{ATTACHMENT_COLUMNS} ORDER BY date_added, id"))?;
        let rows = stmt.query_map([], read_attachment)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Every attachment of a live item that has page text, with its item.
    pub fn attachments_with_pages(&self) -> Result<Vec<Attachment>> {
        let mut stmt = self.conn.prepare(&format!(
            "{ATTACHMENT_COLUMNS} WHERE id IN (SELECT DISTINCT attachment_id FROM attachment_pages) ORDER BY date_added"
        ))?;
        let rows = stmt.query_map([], read_attachment)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Every annotation in the library, with the item it is on.
    pub fn all_annotations(&self) -> Result<Vec<(Annotation, ItemId)>> {
        let mut stmt = self.conn.prepare(
            "SELECT n.data, a.item_id FROM annotations n JOIN attachments a ON a.id = n.attachment_id",
        )?;
        let rows = stmt.query_map([], |row| {
            let data: String = row.get(0)?;
            let item: String = row.get(1)?;
            Ok((data, item))
        })?;
        let mut found = Vec::new();
        for row in rows {
            let (data, item) = row?;
            found.push((serde_json::from_str(&data)?, ItemId::from_string(item)));
        }
        Ok(found)
    }
}

const ATTACHMENT_COLUMNS: &str =
    "SELECT id, item_id, title, path, content_type, size, sha256, date_added FROM attachments";

fn read_attachment(row: &Row<'_>) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: AttachmentId::from_string(row.get::<_, String>(0)?),
        item_id: ItemId::from_string(row.get::<_, String>(1)?),
        title: row.get(2)?,
        path: row.get(3)?,
        content_type: row.get(4)?,
        size: row.get::<_, i64>(5)?.max(0) as u64,
        sha256: row.get(6)?,
        date_added: row.get(7)?,
    })
}

fn read_item(row: &Row<'_>) -> rusqlite::Result<Result<Item>> {
    let data: String = row.get(0)?;
    Ok(serde_json::from_str(&data).map_err(Error::from))
}

fn read_collection(row: &Row<'_>) -> rusqlite::Result<Collection> {
    Ok(Collection {
        id: CollectionId::from_string(row.get::<_, String>(0)?),
        name: row.get(1)?,
        parent_id: row.get::<_, Option<String>>(2)?.map(CollectionId::from_string),
        position: row.get(3)?,
    })
}

/// Milliseconds since 1970, the timestamps the library stores.
pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests;
