//! Tags: names on items, optionally colored.

use paperloom_model::{ItemId, Tag};
use rusqlite::{OptionalExtension, params};

use crate::{Library, Result};

impl Library {
    /// Every tag with how many items (not in the trash) carry it, by name.
    pub fn tags(&self) -> Result<Vec<(Tag, u32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.name, t.color, count(i.id) FROM tags t
             LEFT JOIN item_tags it ON it.tag = t.name
             LEFT JOIN items i ON i.id = it.item_id AND i.deleted = 0
             GROUP BY t.name ORDER BY t.name COLLATE NOCASE, t.name",
        )?;
        let rows =
            stmt.query_map([], |row| Ok((Tag { name: row.get(0)?, color: row.get(1)? }, row.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// An item's tags, by name.
    pub fn item_tags(&self, item: &ItemId) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag FROM item_tags WHERE item_id = ?1 ORDER BY tag COLLATE NOCASE, tag")?;
        let rows = stmt.query_map([item.as_str()], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Replaces an item's tags, creating tags that do not exist yet. Names
    /// are trimmed; empty ones are skipped.
    pub fn set_item_tags(&mut self, item: &ItemId, tags: &[String]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM item_tags WHERE item_id = ?1", [item.as_str()])?;
        for name in tags.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
            tx.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [name])?;
            tx.execute(
                "INSERT OR IGNORE INTO item_tags (item_id, tag) VALUES (?1, ?2)",
                params![item.as_str(), name],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Sets a tag's color (`#rrggbb`), or makes it plain; creates the tag if
    /// needed.
    pub fn set_tag_color(&mut self, name: &str, color: Option<&str>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tags (name, color) VALUES (?1, ?2) ON CONFLICT (name) DO UPDATE SET color = excluded.color",
            params![name.trim(), color],
        )?;
        Ok(())
    }

    /// Renames a tag on every item; renaming onto an existing tag merges the
    /// two.
    pub fn rename_tag(&mut self, from: &str, to: &str) -> Result<()> {
        let to = to.trim();
        if from == to || to.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        let exists =
            tx.query_row("SELECT 1 FROM tags WHERE name = ?1", [to], |_| Ok(())).optional()?.is_some();
        if exists {
            tx.execute(
                "INSERT OR IGNORE INTO item_tags (item_id, tag) SELECT item_id, ?2 FROM item_tags WHERE tag = ?1",
                params![from, to],
            )?;
            tx.execute("DELETE FROM tags WHERE name = ?1", [from])?;
        } else {
            tx.execute("UPDATE tags SET name = ?2 WHERE name = ?1", params![from, to])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Deletes a tag from every item.
    pub fn delete_tag(&mut self, name: &str) -> Result<()> {
        self.conn.execute("DELETE FROM tags WHERE name = ?1", [name])?;
        Ok(())
    }
}
