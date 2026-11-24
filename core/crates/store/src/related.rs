//! Related items, as Zotero relates them: a link both items show, kept as
//! a row each way.

use paperloom_model::ItemId;
use rusqlite::params;

use crate::{Entry, Error, Library, Result};

impl Library {
    /// The items related to an item, not counting the trash; most recently
    /// added first.
    pub fn related_entries(&self, id: &ItemId) -> Result<Vec<Entry>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.related_id FROM item_relations r JOIN items i ON i.id = r.related_id
             WHERE r.item_id = ?1 AND i.deleted = 0 ORDER BY i.date_added DESC, i.id DESC",
        )?;
        let ids: Vec<String> =
            stmt.query_map([id.as_str()], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;
        let mut entries = Vec::new();
        for related in ids {
            if let Some(entry) = self.entry(&ItemId::from_string(related))? {
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    /// Relates two items, each to the other. An item is not related to
    /// itself.
    pub fn relate_items(&mut self, a: &ItemId, b: &ItemId) -> Result<()> {
        if a == b {
            return Ok(());
        }
        for id in [a, b] {
            if self.item(id)?.is_none() {
                return Err(Error::NotFound(format!("item {id}")));
            }
        }
        let tx = self.conn.transaction()?;
        for (from, to) in [(a, b), (b, a)] {
            tx.execute(
                "INSERT OR IGNORE INTO item_relations (item_id, related_id) VALUES (?1, ?2)",
                params![from.as_str(), to.as_str()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn unrelate_items(&mut self, a: &ItemId, b: &ItemId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM item_relations
             WHERE (item_id = ?1 AND related_id = ?2) OR (item_id = ?2 AND related_id = ?1)",
            params![a.as_str(), b.as_str()],
        )?;
        Ok(())
    }
}
