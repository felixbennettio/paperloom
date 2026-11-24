//! Schema versions, applied in order and recorded in `PRAGMA user_version`.

use rusqlite::Connection;

use crate::{Error, Result};

const MIGRATIONS: &[&str] = &[
    // 1: items, collections, tags.
    "CREATE TABLE items (
        id            TEXT PRIMARY KEY,
        item_type     TEXT NOT NULL,
        title         TEXT NOT NULL,
        doi           TEXT,
        year          INTEGER,
        data          TEXT NOT NULL,
        date_added    INTEGER NOT NULL,
        date_modified INTEGER NOT NULL,
        deleted       INTEGER NOT NULL DEFAULT 0
    );
    CREATE UNIQUE INDEX items_live_doi ON items (doi) WHERE doi IS NOT NULL AND deleted = 0;
    CREATE INDEX items_added ON items (deleted, date_added);

    CREATE TABLE collections (
        id        TEXT PRIMARY KEY,
        name      TEXT NOT NULL,
        parent_id TEXT REFERENCES collections (id) ON DELETE CASCADE,
        position  INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX collections_parent ON collections (parent_id, position);

    CREATE TABLE collection_items (
        collection_id TEXT NOT NULL REFERENCES collections (id) ON DELETE CASCADE,
        item_id       TEXT NOT NULL REFERENCES items (id) ON DELETE CASCADE,
        PRIMARY KEY (collection_id, item_id)
    );
    CREATE INDEX collection_items_item ON collection_items (item_id);

    CREATE TABLE tags (
        name  TEXT PRIMARY KEY,
        color TEXT
    );
    CREATE TABLE item_tags (
        item_id TEXT NOT NULL REFERENCES items (id) ON DELETE CASCADE,
        tag     TEXT NOT NULL REFERENCES tags (name) ON DELETE CASCADE ON UPDATE CASCADE,
        PRIMARY KEY (item_id, tag)
    );",
    // 2: files attached to items, and marks on them.
    "CREATE TABLE attachments (
        id           TEXT PRIMARY KEY,
        item_id      TEXT NOT NULL REFERENCES items (id) ON DELETE CASCADE,
        title        TEXT NOT NULL,
        path         TEXT NOT NULL,
        content_type TEXT NOT NULL,
        size         INTEGER NOT NULL,
        sha256       TEXT NOT NULL,
        date_added   INTEGER NOT NULL
    );
    CREATE INDEX attachments_item ON attachments (item_id, date_added);
    CREATE INDEX attachments_sha256 ON attachments (sha256);

    CREATE TABLE annotations (
        id            TEXT PRIMARY KEY,
        attachment_id TEXT NOT NULL REFERENCES attachments (id) ON DELETE CASCADE,
        page_index    INTEGER NOT NULL,
        data          TEXT NOT NULL,
        date_added    INTEGER NOT NULL,
        date_modified INTEGER NOT NULL
    );
    CREATE INDEX annotations_attachment ON annotations (attachment_id, page_index);",
    // 3: stars, reading status, and finding items by tag.
    "ALTER TABLE items ADD COLUMN starred INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE items ADD COLUMN read_status TEXT NOT NULL DEFAULT 'unread';
    CREATE INDEX item_tags_tag ON item_tags (tag);",
    // 4: the text of PDF pages, for full-text search.
    "CREATE TABLE attachment_pages (
        attachment_id TEXT NOT NULL REFERENCES attachments (id) ON DELETE CASCADE,
        page          INTEGER NOT NULL,
        text          TEXT NOT NULL,
        PRIMARY KEY (attachment_id, page)
    );",
    // 5: records changed here, for sync. Writes that sync itself makes
    // (`applying` = 1) are not noted. The triggers never conflict
    // (`WHERE NOT EXISTS` rather than `OR IGNORE`): a statement's own
    // conflict clause, such as an upsert's, would override theirs.
    SYNC_TRACKING,
    // 6: saved searches, synced like collections.
    "CREATE TABLE saved_searches (
        id            TEXT PRIMARY KEY,
        name          TEXT NOT NULL,
        data          TEXT NOT NULL,
        position      INTEGER NOT NULL DEFAULT 0,
        date_modified INTEGER NOT NULL
    );
    CREATE TRIGGER saved_searches_changed_insert AFTER INSERT ON saved_searches
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'search', NEW.id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'search' AND id = NEW.id);
    END;
    CREATE TRIGGER saved_searches_changed_update AFTER UPDATE ON saved_searches
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'search', NEW.id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'search' AND id = NEW.id);
    END;
    CREATE TRIGGER saved_searches_changed_delete AFTER DELETE ON saved_searches
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'search', OLD.id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'search' AND id = OLD.id);
    END;",
    // 7: related items, a row each way; synced as the items' `related:`
    // fields. An item gone for good takes its rows both ways with it.
    "CREATE TABLE item_relations (
        item_id    TEXT NOT NULL REFERENCES items (id) ON DELETE CASCADE,
        related_id TEXT NOT NULL,
        PRIMARY KEY (item_id, related_id)
    ) WITHOUT ROWID;
    CREATE INDEX item_relations_related ON item_relations (related_id);
    CREATE TRIGGER items_unrelate AFTER DELETE ON items
    BEGIN
        DELETE FROM item_relations WHERE related_id = OLD.id;
    END;
    CREATE TRIGGER item_relations_changed_insert AFTER INSERT ON item_relations
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'item', NEW.item_id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'item' AND id = NEW.item_id);
    END;
    CREATE TRIGGER item_relations_changed_delete AFTER DELETE ON item_relations
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'item', OLD.item_id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'item' AND id = OLD.item_id);
    END;",
    // 8: feeds followed and what they announced, kept on this device.
    "CREATE TABLE feeds (
        id           TEXT PRIMARY KEY,
        title        TEXT NOT NULL,
        url          TEXT NOT NULL UNIQUE,
        position     INTEGER NOT NULL DEFAULT 0,
        date_added   INTEGER NOT NULL,
        last_checked INTEGER,
        problem      TEXT
    );
    CREATE TABLE feed_items (
        id         TEXT PRIMARY KEY,
        feed_id    TEXT NOT NULL REFERENCES feeds (id) ON DELETE CASCADE,
        guid       TEXT NOT NULL,
        data       TEXT NOT NULL,
        link       TEXT,
        date_added INTEGER NOT NULL,
        seq        INTEGER NOT NULL DEFAULT 0,
        read       INTEGER NOT NULL DEFAULT 0,
        UNIQUE (feed_id, guid)
    );
    CREATE INDEX feed_items_feed ON feed_items (feed_id, date_added);",
];

const SYNC_TRACKING: &str = "
    CREATE TABLE sync_changed (
        kind TEXT NOT NULL,
        id   TEXT NOT NULL,
        PRIMARY KEY (kind, id)
    ) WITHOUT ROWID;
    CREATE TABLE sync_state (
        key   TEXT PRIMARY KEY,
        value INTEGER NOT NULL
    ) WITHOUT ROWID;
    INSERT INTO sync_state (key, value) VALUES ('applying', 0);

    CREATE TRIGGER items_changed_insert AFTER INSERT ON items
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'item', NEW.id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'item' AND id = NEW.id);
    END;
    CREATE TRIGGER items_changed_update AFTER UPDATE ON items
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'item', NEW.id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'item' AND id = NEW.id);
    END;
    CREATE TRIGGER items_changed_delete AFTER DELETE ON items
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'item', OLD.id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'item' AND id = OLD.id);
    END;

    CREATE TRIGGER item_tags_changed_insert AFTER INSERT ON item_tags
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'item', NEW.item_id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'item' AND id = NEW.item_id);
    END;
    CREATE TRIGGER item_tags_changed_update AFTER UPDATE ON item_tags
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'item', OLD.item_id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'item' AND id = OLD.item_id);
        INSERT INTO sync_changed (kind, id) SELECT 'item', NEW.item_id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'item' AND id = NEW.item_id);
    END;
    CREATE TRIGGER item_tags_changed_delete AFTER DELETE ON item_tags
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'item', OLD.item_id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'item' AND id = OLD.item_id);
    END;

    CREATE TRIGGER collection_items_changed_insert AFTER INSERT ON collection_items
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'item', NEW.item_id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'item' AND id = NEW.item_id);
    END;
    CREATE TRIGGER collection_items_changed_delete AFTER DELETE ON collection_items
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'item', OLD.item_id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'item' AND id = OLD.item_id);
    END;

    CREATE TRIGGER tags_changed_insert AFTER INSERT ON tags
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'tag', NEW.name
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'tag' AND id = NEW.name);
    END;
    CREATE TRIGGER tags_changed_update AFTER UPDATE ON tags
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'tag', OLD.name
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'tag' AND id = OLD.name);
        INSERT INTO sync_changed (kind, id) SELECT 'tag', NEW.name
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'tag' AND id = NEW.name);
    END;
    CREATE TRIGGER tags_changed_delete AFTER DELETE ON tags
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'tag', OLD.name
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'tag' AND id = OLD.name);
    END;

    CREATE TRIGGER collections_changed_insert AFTER INSERT ON collections
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'collection', NEW.id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'collection' AND id = NEW.id);
    END;
    CREATE TRIGGER collections_changed_update AFTER UPDATE ON collections
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'collection', NEW.id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'collection' AND id = NEW.id);
    END;
    CREATE TRIGGER collections_changed_delete AFTER DELETE ON collections
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'collection', OLD.id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'collection' AND id = OLD.id);
    END;

    CREATE TRIGGER attachments_changed_insert AFTER INSERT ON attachments
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'attachment', NEW.id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'attachment' AND id = NEW.id);
    END;
    CREATE TRIGGER attachments_changed_update AFTER UPDATE ON attachments
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'attachment', NEW.id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'attachment' AND id = NEW.id);
    END;
    CREATE TRIGGER attachments_changed_delete AFTER DELETE ON attachments
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'attachment', OLD.id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'attachment' AND id = OLD.id);
    END;

    CREATE TRIGGER annotations_changed_insert AFTER INSERT ON annotations
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'annotation', NEW.id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'annotation' AND id = NEW.id);
    END;
    CREATE TRIGGER annotations_changed_update AFTER UPDATE ON annotations
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'annotation', NEW.id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'annotation' AND id = NEW.id);
    END;
    CREATE TRIGGER annotations_changed_delete AFTER DELETE ON annotations
    WHEN (SELECT value FROM sync_state WHERE key = 'applying') = 0
    BEGIN
        INSERT INTO sync_changed (kind, id) SELECT 'annotation', OLD.id
        WHERE NOT EXISTS (SELECT 1 FROM sync_changed WHERE kind = 'annotation' AND id = OLD.id);
    END;
";

pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let latest = MIGRATIONS.len() as i64;
    if current > latest {
        return Err(Error::NewerSchema(current));
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", index as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}
