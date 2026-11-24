//! Feeds the library follows (journals' tables of contents, preprint
//! servers) and what they announced: kept on this device, apart from the
//! library's items, until one is added to the library.

use paperloom_model::{Item, ItemId};
use rusqlite::{OptionalExtension, Row, params};

use crate::{Error, Library, Result, now_ms};

/// How many entries a feed keeps; older ones already read go.
const KEEP: i64 = 500;

/// A feed followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedRow {
    pub id: String,
    pub title: String,
    pub url: String,
    /// Milliseconds since 1970; none before the first reading.
    pub last_checked: Option<i64>,
    /// Why the last reading failed.
    pub problem: Option<String>,
    pub unread: u32,
}

/// An entry a feed announced.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedItemRow {
    pub id: String,
    pub feed_id: String,
    pub item: Item,
    pub link: Option<String>,
    /// When it was first read from the feed.
    pub date_added: i64,
    pub read: bool,
}

/// An entry read from a feed: its ID there, the work, its link.
pub struct NewFeedEntry<'a> {
    pub guid: &'a str,
    pub item: &'a Item,
    pub link: Option<&'a str>,
}

const FEED_COLUMNS: &str = "SELECT f.id, f.title, f.url, f.last_checked, f.problem,
        (SELECT count(*) FROM feed_items WHERE feed_id = f.id AND read = 0)
     FROM feeds f";

fn read_feed_row(row: &Row<'_>) -> rusqlite::Result<FeedRow> {
    Ok(FeedRow {
        id: row.get(0)?,
        title: row.get(1)?,
        url: row.get(2)?,
        last_checked: row.get(3)?,
        problem: row.get(4)?,
        unread: row.get(5)?,
    })
}

fn read_item_row(row: &Row<'_>) -> rusqlite::Result<Result<FeedItemRow>> {
    let data: String = row.get(2)?;
    let (id, feed_id, link, date_added, read) =
        (row.get(0)?, row.get(1)?, row.get(3)?, row.get(4)?, row.get(5)?);
    Ok(serde_json::from_str(&data).map_err(Error::from).map(|item| FeedItemRow {
        id,
        feed_id,
        item,
        link,
        date_added,
        read,
    }))
}

impl Library {
    /// The feeds followed, by title.
    pub fn feeds(&self) -> Result<Vec<FeedRow>> {
        let mut stmt =
            self.conn.prepare(&format!("{FEED_COLUMNS} ORDER BY f.position, f.title COLLATE NOCASE"))?;
        Ok(stmt.query_map([], read_feed_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn feed(&self, id: &str) -> Result<Option<FeedRow>> {
        Ok(self.conn.query_row(&format!("{FEED_COLUMNS} WHERE f.id = ?1"), [id], read_feed_row).optional()?)
    }

    /// Follows a feed; the one already followed at that address, if any.
    pub fn add_feed(&mut self, url: &str, title: &str) -> Result<FeedRow> {
        let existing: Option<String> =
            self.conn.query_row("SELECT id FROM feeds WHERE url = ?1", [url], |row| row.get(0)).optional()?;
        let id = match existing {
            Some(id) => id,
            None => {
                let id = ItemId::new().to_string();
                self.conn.execute(
                    "INSERT INTO feeds (id, title, url, date_added) VALUES (?1, ?2, ?3, ?4)",
                    params![id, title.trim(), url, now_ms()],
                )?;
                id
            }
        };
        self.feed(&id)?.ok_or_else(|| Error::NotFound(format!("feed {id}")))
    }

    pub fn rename_feed(&mut self, id: &str, title: &str) -> Result<()> {
        if self.conn.execute("UPDATE feeds SET title = ?2 WHERE id = ?1", params![id, title.trim()])? == 0 {
            return Err(Error::NotFound(format!("feed {id}")));
        }
        Ok(())
    }

    /// Stops following a feed; its entries go with it.
    pub fn remove_feed(&mut self, id: &str) -> Result<()> {
        if self.conn.execute("DELETE FROM feeds WHERE id = ?1", [id])? == 0 {
            return Err(Error::NotFound(format!("feed {id}")));
        }
        Ok(())
    }

    /// Keeps a reading of a feed: entries not seen before come in unread,
    /// in the feed's order; those seen before keep their state and take
    /// the feed's newer details. How many were new.
    pub fn store_feed_entries(&mut self, id: &str, entries: &[NewFeedEntry<'_>]) -> Result<u32> {
        let now = now_ms();
        let tx = self.conn.transaction()?;
        let mut added = 0;
        for (seq, entry) in entries.iter().enumerate() {
            let data = serde_json::to_string(entry.item)?;
            let changed = tx.execute(
                "UPDATE feed_items SET data = ?3, link = ?4 WHERE feed_id = ?1 AND guid = ?2",
                params![id, entry.guid, data, entry.link],
            )?;
            if changed == 0 {
                tx.execute(
                    "INSERT INTO feed_items (id, feed_id, guid, data, link, date_added, seq) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![ItemId::new().to_string(), id, entry.guid, data, entry.link, now, seq as i64],
                )?;
                added += 1;
            }
        }
        tx.execute(
            "DELETE FROM feed_items WHERE feed_id = ?1 AND read = 1 AND id NOT IN
                (SELECT id FROM feed_items WHERE feed_id = ?1 ORDER BY date_added DESC, seq LIMIT ?2)",
            params![id, KEEP],
        )?;
        tx.execute("UPDATE feeds SET last_checked = ?2, problem = NULL WHERE id = ?1", params![id, now])?;
        tx.commit()?;
        Ok(added)
    }

    /// Notes why reading a feed failed.
    pub fn set_feed_problem(&mut self, id: &str, problem: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE feeds SET last_checked = ?2, problem = ?3 WHERE id = ?1",
            params![id, now_ms(), problem],
        )?;
        Ok(())
    }

    /// A feed's entries, or every feed's: the newest reading first, each in
    /// its feed's order.
    pub fn feed_items(&self, feed_id: Option<&str>) -> Result<Vec<FeedItemRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, feed_id, data, link, date_added, read FROM feed_items
             WHERE ?1 IS NULL OR feed_id = ?1 ORDER BY date_added DESC, seq",
        )?;
        let rows = stmt.query_map([feed_id], read_item_row)?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row??);
        }
        Ok(items)
    }

    pub fn feed_item(&self, id: &str) -> Result<Option<FeedItemRow>> {
        self.conn
            .query_row(
                "SELECT id, feed_id, data, link, date_added, read FROM feed_items WHERE id = ?1",
                [id],
                read_item_row,
            )
            .optional()?
            .transpose()
    }

    pub fn set_feed_item_read(&mut self, id: &str, read: bool) -> Result<()> {
        if self.conn.execute("UPDATE feed_items SET read = ?2 WHERE id = ?1", params![id, read])? == 0 {
            return Err(Error::NotFound(format!("feed entry {id}")));
        }
        Ok(())
    }

    /// Marks a feed's entries read, or every feed's.
    pub fn mark_feed_read(&mut self, feed_id: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE feed_items SET read = 1 WHERE ?1 IS NULL OR feed_id = ?1", [feed_id])?;
        Ok(())
    }
}
