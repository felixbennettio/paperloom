//! The database in PostgreSQL.

use paperloom_sync_proto::{
    BlobHash, Change, ChangesPage, Device, Library, LoggedChange, NewDevice, NewLibrary, Pushed, Role,
    Snapshot, Usage,
};
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::{Row, query, query_scalar};
use uuid::Uuid;

use super::{UserId, check_snapshot, gap, now_ms, unknown_device};
use crate::error::ApiError;

const SCHEMA: &str = include_str!("schema.sql");

pub struct Postgres {
    pool: PgPool,
}

fn device(row: &PgRow) -> Result<Device, sqlx::Error> {
    Ok(Device {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        platform: row.try_get("platform")?,
        created_at: row.try_get("created_at")?,
    })
}

fn library(row: &PgRow) -> Result<Library, ApiError> {
    let role: String = row.try_get("role")?;
    Ok(Library {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        role: Role::parse(&role).ok_or_else(|| ApiError::Internal(format!("unknown role {role}")))?,
        head: row.try_get::<i64, _>("head")? as u64,
        created_at: row.try_get("created_at")?,
    })
}

const LIBRARY: &str = "SELECT l.id, l.name, m.role, l.head, l.created_at
    FROM libraries l JOIN members m ON m.library_id = l.id WHERE m.user_id = $1";

impl Postgres {
    /// Connects and brings the schema up to date.
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new().max_connections(16).connect(url).await?;
        sqlx::raw_sql(SCHEMA).execute(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn user(&self, sub: &str) -> Result<UserId, ApiError> {
        Ok(query_scalar::<_, i64>(
            "INSERT INTO users (sub, created_at) VALUES ($1, $2)
             ON CONFLICT (sub) DO UPDATE SET sub = EXCLUDED.sub RETURNING id",
        )
        .bind(sub)
        .bind(now_ms())
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn register_device(&self, user: UserId, new: NewDevice) -> Result<Device, ApiError> {
        let row = query(
            "INSERT INTO devices (id, user_id, name, platform, created_at) VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, platform = EXCLUDED.platform
             WHERE devices.user_id = EXCLUDED.user_id
             RETURNING id, name, platform, created_at",
        )
        .bind(new.id)
        .bind(user)
        .bind(&new.name)
        .bind(&new.platform)
        .bind(now_ms())
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => Ok(device(&row)?),
            None => Err(ApiError::conflict("This device ID belongs to another account.")),
        }
    }

    pub async fn devices(&self, user: UserId) -> Result<Vec<Device>, ApiError> {
        let rows = query(
            "SELECT id, name, platform, created_at FROM devices WHERE user_id = $1 ORDER BY created_at, id",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(device).collect::<Result<_, _>>()?)
    }

    pub async fn libraries(&self, user: UserId) -> Result<Vec<Library>, ApiError> {
        let rows =
            query(&format!("{LIBRARY} ORDER BY l.created_at, l.id")).bind(user).fetch_all(&self.pool).await?;
        rows.iter().map(library).collect()
    }

    pub async fn create_library(&self, user: UserId, new: NewLibrary) -> Result<Library, ApiError> {
        let mut tx = self.pool.begin().await?;
        let made = query(
            "INSERT INTO libraries (id, name, owner_id, created_at) VALUES ($1, $2, $3, $4)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(new.id)
        .bind(&new.name)
        .bind(user)
        .bind(now_ms())
        .execute(&mut *tx)
        .await?;
        if made.rows_affected() == 1 {
            query("INSERT INTO members (library_id, user_id, role) VALUES ($1, $2, 'owner')")
                .bind(new.id)
                .bind(user)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        self.library(user, new.id).await?.ok_or_else(|| ApiError::conflict("This library ID is taken."))
    }

    pub async fn library(&self, user: UserId, id: Uuid) -> Result<Option<Library>, ApiError> {
        let row =
            query(&format!("{LIBRARY} AND l.id = $2")).bind(user).bind(id).fetch_optional(&self.pool).await?;
        row.as_ref().map(library).transpose()
    }

    pub async fn push(&self, user: UserId, library: Uuid, changes: Vec<Change>) -> Result<Pushed, ApiError> {
        let mut tx = self.pool.begin().await?;
        // Locks the library until this push commits (or is dropped and
        // rolled back on an error).
        let head: Option<i64> = query_scalar("SELECT head FROM libraries WHERE id = $1 FOR UPDATE")
            .bind(library)
            .fetch_optional(&mut *tx)
            .await?;
        let mut head = head.ok_or_else(|| ApiError::NotFound("No such library".into()))? as u64;
        let owned: Vec<Uuid> =
            query_scalar("SELECT id FROM devices WHERE user_id = $1").bind(user).fetch_all(&mut *tx).await?;
        if changes.iter().any(|c| !owned.contains(&c.device)) {
            return Err(unknown_device());
        }
        let mut cursors = Vec::with_capacity(changes.len());
        for change in changes {
            let last: Option<i64> =
                query_scalar("SELECT last_seq FROM device_seqs WHERE library_id = $1 AND device_id = $2")
                    .bind(library)
                    .bind(change.device)
                    .fetch_optional(&mut *tx)
                    .await?;
            let last = last.unwrap_or(0) as u64;
            if change.seq <= last {
                let kept: Option<i64> = query_scalar(
                    "SELECT log_cursor FROM changes WHERE library_id = $1 AND device_id = $2 AND seq = $3",
                )
                .bind(library)
                .bind(change.device)
                .bind(change.seq as i64)
                .fetch_optional(&mut *tx)
                .await?;
                cursors.push(kept.unwrap_or(0) as u64);
                continue;
            }
            if change.seq != last + 1 {
                return Err(gap(last + 1));
            }
            head += 1;
            query(
                "INSERT INTO changes (library_id, log_cursor, device_id, seq, payload, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(library)
            .bind(head as i64)
            .bind(change.device)
            .bind(change.seq as i64)
            .bind(&change.payload)
            .bind(now_ms())
            .execute(&mut *tx)
            .await?;
            query(
                "INSERT INTO device_seqs (library_id, device_id, last_seq) VALUES ($1, $2, $3)
                 ON CONFLICT (library_id, device_id) DO UPDATE SET last_seq = EXCLUDED.last_seq",
            )
            .bind(library)
            .bind(change.device)
            .bind(change.seq as i64)
            .execute(&mut *tx)
            .await?;
            cursors.push(head);
        }
        query("UPDATE libraries SET head = $2 WHERE id = $1")
            .bind(library)
            .bind(head as i64)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Pushed { head, cursors })
    }

    pub async fn changes(&self, library: Uuid, after: u64, limit: usize) -> Result<ChangesPage, ApiError> {
        let row = query("SELECT head, snapshot_cursor, snapshot_blob FROM libraries WHERE id = $1")
            .bind(library)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ApiError::NotFound("No such library".into()))?;
        let head = row.try_get::<i64, _>("head")? as u64;
        let snapshot = snapshot(&row)?;
        if let Some(snapshot) = snapshot.filter(|s| after < s.cursor) {
            return Ok(ChangesPage { changes: Vec::new(), head, more: true, snapshot: Some(snapshot) });
        }
        let rows = query(
            "SELECT log_cursor, device_id, seq, payload FROM changes
             WHERE library_id = $1 AND log_cursor > $2 ORDER BY log_cursor LIMIT $3",
        )
        .bind(library)
        .bind(after as i64)
        .bind(limit as i64 + 1)
        .fetch_all(&self.pool)
        .await?;
        let mut changes = rows
            .iter()
            .map(|row| {
                Ok(LoggedChange {
                    cursor: row.try_get::<i64, _>("log_cursor")? as u64,
                    device: row.try_get("device_id")?,
                    seq: row.try_get::<i64, _>("seq")? as u64,
                    payload: row.try_get("payload")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;
        let more = changes.len() > limit;
        changes.truncate(limit);
        Ok(ChangesPage { changes, head, more, snapshot: None })
    }

    pub async fn set_snapshot(&self, library: Uuid, new: Snapshot) -> Result<(), ApiError> {
        let mut tx = self.pool.begin().await?;
        let row =
            query("SELECT head, snapshot_cursor, snapshot_blob FROM libraries WHERE id = $1 FOR UPDATE")
                .bind(library)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| ApiError::NotFound("No such library".into()))?;
        let head = row.try_get::<i64, _>("head")? as u64;
        check_snapshot(&new, head, snapshot(&row)?.map(|s| s.cursor))?;
        query("UPDATE libraries SET snapshot_cursor = $2, snapshot_blob = $3 WHERE id = $1")
            .bind(library)
            .bind(new.cursor as i64)
            .bind(new.blob.as_str())
            .execute(&mut *tx)
            .await?;
        query("DELETE FROM changes WHERE library_id = $1 AND log_cursor <= $2")
            .bind(library)
            .bind(new.cursor as i64)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn add_blob(&self, library: Uuid, hash: &BlobHash, size: u64) -> Result<(), ApiError> {
        query(
            "INSERT INTO blobs (library_id, hash, size, created_at) VALUES ($1, $2, $3, $4)
             ON CONFLICT (library_id, hash) DO NOTHING",
        )
        .bind(library)
        .bind(hash.as_str())
        .bind(size as i64)
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn has_blob(&self, library: Uuid, hash: &BlobHash) -> Result<bool, ApiError> {
        Ok(query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM blobs WHERE library_id = $1 AND hash = $2)")
            .bind(library)
            .bind(hash.as_str())
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn usage(&self, user: UserId) -> Result<Usage, ApiError> {
        let row = query(
            "SELECT
                (SELECT COUNT(*) FROM libraries WHERE owner_id = $1) AS libraries,
                (SELECT COALESCE(SUM(octet_length(c.payload)), 0)::BIGINT FROM changes c
                    JOIN libraries l ON l.id = c.library_id WHERE l.owner_id = $1) AS change_bytes,
                (SELECT COALESCE(SUM(b.size), 0)::BIGINT FROM blobs b
                    JOIN libraries l ON l.id = b.library_id WHERE l.owner_id = $1) AS blob_bytes",
        )
        .bind(user)
        .fetch_one(&self.pool)
        .await?;
        Ok(Usage {
            libraries: row.try_get::<i64, _>("libraries")? as u32,
            change_bytes: row.try_get::<i64, _>("change_bytes")? as u64,
            blob_bytes: row.try_get::<i64, _>("blob_bytes")? as u64,
        })
    }
}

fn snapshot(row: &PgRow) -> Result<Option<Snapshot>, ApiError> {
    let cursor: Option<i64> = row.try_get("snapshot_cursor")?;
    let blob: Option<String> = row.try_get("snapshot_blob")?;
    match (cursor, blob) {
        (Some(cursor), Some(blob)) => Ok(Some(Snapshot {
            cursor: cursor as u64,
            blob: blob.parse().map_err(|e| ApiError::Internal(format!("stored snapshot: {e}")))?,
        })),
        _ => Ok(None),
    }
}
