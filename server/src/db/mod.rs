//! Accounts, devices, libraries and their change logs. Kept in PostgreSQL;
//! in memory for tests. Both keep the same rules:
//!
//! - a device belongs to one account;
//! - each device's changes in a library are numbered 1, 2, 3…: a number
//!   already kept is answered with its cursor (0 when a snapshot has since
//!   folded it in), a gap is refused with the number expected;
//! - one push is kept whole or not at all, and cursors follow the order
//!   pushes are kept in, so a reader never misses a change behind one it
//!   has seen.

mod memory;
mod postgres;

pub use memory::Memory;
pub use postgres::Postgres;

use paperloom_sync_proto::{
    BlobHash, Change, ChangesPage, Device, Library, NewDevice, NewLibrary, Pushed, Snapshot, Usage,
};
use uuid::Uuid;

use crate::error::ApiError;

/// An account, by its row.
pub type UserId = i64;

pub enum Database {
    Memory(Box<Memory>),
    Postgres(Postgres),
}

macro_rules! each {
    ($this:ident, $db:ident => $call:expr) => {
        match $this {
            Database::Memory($db) => $call,
            Database::Postgres($db) => $call.await,
        }
    };
}

impl Database {
    pub fn memory() -> Self {
        Self::Memory(Box::default())
    }

    /// The account for a token's subject, made on first sight.
    pub async fn user(&self, sub: &str) -> Result<UserId, ApiError> {
        each!(self, db => db.user(sub))
    }

    /// Registers a device, or renames one already registered.
    pub async fn register_device(&self, user: UserId, device: NewDevice) -> Result<Device, ApiError> {
        each!(self, db => db.register_device(user, device))
    }

    pub async fn devices(&self, user: UserId) -> Result<Vec<Device>, ApiError> {
        each!(self, db => db.devices(user))
    }

    pub async fn libraries(&self, user: UserId) -> Result<Vec<Library>, ApiError> {
        each!(self, db => db.libraries(user))
    }

    /// Makes a library owned by `user`; making it again returns it.
    pub async fn create_library(&self, user: UserId, library: NewLibrary) -> Result<Library, ApiError> {
        each!(self, db => db.create_library(user, library))
    }

    /// A library `user` is a member of.
    pub async fn library(&self, user: UserId, id: Uuid) -> Result<Option<Library>, ApiError> {
        each!(self, db => db.library(user, id))
    }

    /// Appends changes from `user`'s devices to a library's log.
    pub async fn push(&self, user: UserId, library: Uuid, changes: Vec<Change>) -> Result<Pushed, ApiError> {
        each!(self, db => db.push(user, library, changes))
    }

    /// Up to `limit` changes after `after`, oldest first.
    pub async fn changes(&self, library: Uuid, after: u64, limit: usize) -> Result<ChangesPage, ApiError> {
        each!(self, db => db.changes(library, after, limit))
    }

    /// Folds the log up to the snapshot's cursor into it.
    pub async fn set_snapshot(&self, library: Uuid, snapshot: Snapshot) -> Result<(), ApiError> {
        each!(self, db => db.set_snapshot(library, snapshot))
    }

    pub async fn add_blob(&self, library: Uuid, hash: &BlobHash, size: u64) -> Result<(), ApiError> {
        each!(self, db => db.add_blob(library, hash, size))
    }

    pub async fn has_blob(&self, library: Uuid, hash: &BlobHash) -> Result<bool, ApiError> {
        each!(self, db => db.has_blob(library, hash))
    }

    /// What the libraries `user` owns take up.
    pub async fn usage(&self, user: UserId) -> Result<Usage, ApiError> {
        each!(self, db => db.usage(user))
    }
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Checks a snapshot against the log it would fold.
pub(crate) fn check_snapshot(snapshot: &Snapshot, head: u64, current: Option<u64>) -> Result<(), ApiError> {
    if snapshot.cursor > head {
        return Err(ApiError::BadRequest(format!(
            "The snapshot is at cursor {} but the log ends at {head}.",
            snapshot.cursor
        )));
    }
    if current.is_some_and(|c| snapshot.cursor < c) {
        return Err(ApiError::conflict("A newer snapshot is already kept."));
    }
    Ok(())
}

pub(crate) fn gap(expected: u64) -> ApiError {
    ApiError::Conflict {
        message: format!("Changes are missing: the next one from this device is {expected}."),
        expected_seq: Some(expected),
    }
}

pub(crate) fn unknown_device() -> ApiError {
    ApiError::Forbidden("A change comes from a device not registered to this account.".into())
}
