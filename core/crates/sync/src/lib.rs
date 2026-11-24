//! The sync engine. A library syncs by keeping its records in a Loro
//! document beside the SQLite file ([`Replica`]) and exchanging this
//! device's writes to it as numbered changes through a [`Transport`]: the
//! Paperloom Cloud service, or storage the person brings.
//!
//! One round ([`sync`]):
//! 1. records changed here (noted by the store) go into the document;
//! 2. this device's new writes become its next change, kept in an outbox
//!    until the transport has them;
//! 3. other devices' changes after the last cursor are taken into the
//!    document, and the records they name are written back to the store.
//!
//! The store is only locked for steps that touch it, never across the
//! network; changes made here between the steps are picked up before
//! anything from elsewhere is written, so they are never overwritten.
//!
//! With a passphrase, changes and files are sealed end to end
//! ([`seal`]).

mod replica;
pub mod seal;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use paperloom_store::{Library, Record, RecordKind};
use paperloom_sync_proto::{Change, ChangesPage, LoggedChange, Pushed};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use replica::Replica;

/// Records by kind and ID.
pub type RecordRefs = Vec<(RecordKind, String)>;

/// How far each device's changes have been taken in: its newest seq.
pub type Positions = BTreeMap<Uuid, u64>;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("library: {0}")]
    Store(#[from] paperloom_store::Error),
    #[error("sync document: {0}")]
    Loro(#[from] loro::LoroError),
    #[error("sync document: {0}")]
    Encode(#[from] loro::LoroEncodeError),
    #[error("a change cannot be read: {0}")]
    Corrupt(String),
    /// The transport could not be reached or refused the request.
    #[error("{0}")]
    Transport(String),
    /// The transport wants this device's changes resent from `expected`.
    #[error("the service expects change {expected} next")]
    Gap { expected: u64 },
    /// The transport did not accept the credentials: sign in again.
    #[error("the sync service needs you to sign in again")]
    SignIn,
    /// Reading or writing this device's files.
    #[error("{0}")]
    Local(String),
    /// Sealed changes or files arrived, and this device has no passphrase.
    #[error("This library is end-to-end encrypted: enter its passphrase to sync.")]
    Locked,
    /// The passphrase does not open what another device sealed.
    #[error("The passphrase does not open this library's encrypted changes.")]
    Passphrase,
}

/// What a device remembers about syncing one library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncState {
    pub device: Uuid,
    /// The Loro peer this device writes as.
    pub peer: u64,
    /// The end of this device's writes already put into changes.
    pub exported: i32,
    /// The number the next change gets.
    pub next_seq: u64,
    /// The log read up to here (the service's cursor).
    pub cursor: u64,
    /// Each device's changes taken in up to here. Storage people bring
    /// has no cursor; it is read by these.
    #[serde(default)]
    pub positions: Positions,
    /// Changes made here that the transport does not have yet.
    pub outbox: Vec<Change>,
}

impl SyncState {
    pub fn new(device: Uuid, peer: u64) -> Self {
        Self {
            device,
            peer,
            exported: 0,
            next_seq: 1,
            cursor: 0,
            positions: Positions::new(),
            outbox: Vec::new(),
        }
    }
}

/// A change's bytes: the records and notes it touches, and the Loro update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Envelope {
    /// Format version.
    v: u32,
    records: Vec<(String, String)>,
    /// Note paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    notes: Vec<String>,
    update: String,
}

/// What a change holds, read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Contents {
    records: RecordRefs,
    notes: Vec<String>,
    update: Vec<u8>,
}

impl Envelope {
    fn encode(records: &[(RecordKind, String)], notes: &[String], update: &[u8]) -> Vec<u8> {
        let envelope = Envelope {
            v: 1,
            records: records.iter().map(|(k, id)| (k.as_str().to_string(), id.clone())).collect(),
            notes: notes.to_vec(),
            update: STANDARD.encode(update),
        };
        serde_json::to_vec(&envelope).unwrap_or_default()
    }

    fn decode(bytes: &[u8]) -> Result<Contents, SyncError> {
        let envelope: Envelope =
            serde_json::from_slice(bytes).map_err(|e| SyncError::Corrupt(e.to_string()))?;
        if envelope.v != 1 {
            return Err(SyncError::Corrupt(format!("format {} is newer than this app", envelope.v)));
        }
        let update = STANDARD.decode(envelope.update).map_err(|e| SyncError::Corrupt(e.to_string()))?;
        let records =
            envelope.records.into_iter().filter_map(|(k, id)| Some((RecordKind::parse(&k)?, id))).collect();
        Ok(Contents { records, notes: envelope.notes, update })
    }
}

/// Where changes go and come from: the service, or storage people bring.
pub trait Transport {
    fn push(&self, changes: &[Change]) -> Result<Pushed, SyncError>;
    /// Changes after `after` (the service), or after `positions` (storage
    /// people bring, whose changes have cursor 0).
    fn pull(&self, after: u64, positions: &Positions) -> Result<ChangesPage, SyncError>;
    fn blob(&self, hash: &paperloom_sync_proto::BlobHash) -> Result<Vec<u8>, SyncError>;
    /// Whether this device's changes (and the files they name) may go out
    /// now; a round that may not push fetches first and pushes after.
    fn ready_to_push(&self) -> bool {
        true
    }
}

/// What a round did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    pub sent: usize,
    pub received: usize,
    /// Records written to the library from elsewhere.
    pub applied: RecordRefs,
    /// Notes changed here from elsewhere.
    pub notes: Vec<String>,
}

/// Moves records changed in the library into the document and queues this
/// device's new writes (with the notes already put in, `notes`) as a
/// change. Returns the records to forget once the document and state are
/// saved. Call with the library locked.
pub fn collect(
    library: &Library,
    replica: &Replica,
    state: &mut SyncState,
    hooks: &mut dyn Hooks,
    notes: &[String],
) -> Result<RecordRefs, SyncError> {
    let changed = library.changed_records()?;
    let mut touched = Vec::new();
    for (kind, id) in &changed {
        let mut record = Record { kind: *kind, id: id.clone(), fields: library.record(*kind, id)? };
        hooks.decorate(replica, &mut record)?;
        if replica.put(&record)? {
            touched.push((*kind, id.clone()));
        }
    }
    if !touched.is_empty() || !notes.is_empty() {
        let update = replica.own_updates_since(state.exported)?;
        state.outbox.push(Change {
            device: state.device,
            seq: state.next_seq,
            payload: Envelope::encode(&touched, notes, &update),
        });
        state.next_seq += 1;
        state.exported = replica.own_end();
    }
    Ok(changed)
}

/// Sends the outbox.
pub fn push(transport: &dyn Transport, state: &mut SyncState) -> Result<usize, SyncError> {
    let mut sent = 0;
    while !state.outbox.is_empty() {
        let batch: Vec<Change> = state.outbox.iter().take(paperloom_sync_proto::MAX_BATCH).cloned().collect();
        transport.push(&batch)?;
        state.outbox = state.outbox.split_off(batch.len());
        sent += batch.len();
    }
    Ok(sent)
}

/// A page of the log, with its snapshot's bytes when it has one.
pub struct Fetched {
    pub page: ChangesPage,
    pub snapshot: Option<Vec<u8>>,
}

/// The log after the cursor, in order.
pub fn fetch(transport: &dyn Transport, state: &SyncState) -> Result<Vec<Fetched>, SyncError> {
    let mut fetched = Vec::new();
    let mut after = state.cursor;
    loop {
        let page = transport.pull(after, &state.positions)?;
        let snapshot = match &page.snapshot {
            Some(snapshot) => {
                after = snapshot.cursor;
                Some(transport.blob(&snapshot.blob)?)
            }
            None => None,
        };
        if let Some(last) = page.changes.last() {
            after = last.cursor;
        }
        let done = !page.more || (page.changes.is_empty() && snapshot.is_none());
        fetched.push(Fetched { page, snapshot });
        if done {
            return Ok(fetched);
        }
    }
}

/// What [`apply`] took in.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Applied {
    /// Changes from other devices.
    pub received: usize,
    /// Records written to the library.
    pub records: Vec<Record>,
    /// Notes they changed, for the app to write.
    pub notes: Vec<String>,
}

/// Takes fetched changes into the document and writes the records they
/// touch to the library. Local changes must be collected first, so none
/// is overwritten. Call with the library locked.
pub fn apply(
    library: &mut Library,
    replica: &Replica,
    state: &mut SyncState,
    fetched: &[Fetched],
) -> Result<Applied, SyncError> {
    let mut touched = BTreeSet::new();
    let mut notes = BTreeSet::new();
    let mut received = 0;
    let mut queue: Vec<&LoggedChange> = Vec::new();
    for Fetched { page, snapshot } in fetched {
        if let (Some(bytes), Some(at)) = (snapshot, &page.snapshot)
            && at.cursor > state.cursor
        {
            if !replica.import(bytes)? {
                return Err(SyncError::Corrupt("the snapshot is incomplete".into()));
            }
            touched.extend(replica.all());
            notes.extend(replica.note_paths());
            state.cursor = at.cursor;
        }
        queue.extend(&page.changes);
    }
    // A change waiting for another device's (not here yet, or later in this
    // batch) holds back the rest of its device's; the others go on, and the
    // waiting ones are tried again while that helps.
    let mut newest = state.cursor;
    let waiting = loop {
        let mut progress = false;
        let mut blocked = BTreeSet::new();
        let mut rest = Vec::new();
        for change in queue {
            let seen = state.positions.get(&change.device).copied().unwrap_or(0);
            if (change.cursor != 0 && change.cursor <= state.cursor) || change.seq <= seen {
                continue;
            }
            if blocked.contains(&change.device) {
                rest.push(change);
                continue;
            }
            if change.device != state.device {
                let contents = Envelope::decode(&change.payload)?;
                if !replica.import(&contents.update)? {
                    blocked.insert(change.device);
                    rest.push(change);
                    continue;
                }
                touched.extend(contents.records);
                notes.extend(contents.notes);
                received += 1;
            }
            state.positions.insert(change.device, change.seq);
            newest = newest.max(change.cursor);
            progress = true;
        }
        if rest.is_empty() || !progress {
            break rest;
        }
        queue = rest;
    };
    // The service's cursor stays before the first change still waiting, so
    // it comes again next time.
    state.cursor = match waiting.iter().map(|c| c.cursor).filter(|&c| c != 0).min() {
        Some(first) => state.cursor.max(first - 1),
        None => newest,
    };
    let records: Vec<Record> = touched
        .into_iter()
        .filter_map(|(kind, id)| {
            let fields = replica.get(kind, &id)?;
            Some(Record { kind, id, fields })
        })
        .collect();
    library.apply_records(&records)?;
    Ok(Applied { received, records, notes: notes.into_iter().collect() })
}

/// What the app does around a round.
pub trait Hooks {
    /// Adds to a record what the library does not keep, before it goes into
    /// the document (the hash an attachment's file is stored under). The
    /// document holds the record as it was, for fields kept from before.
    fn decorate(&mut self, _replica: &Replica, _record: &mut Record) -> Result<(), SyncError> {
        Ok(())
    }

    /// Puts notes changed here into the document ([`Replica::put_note`]);
    /// their paths. Called without the library locked: notes are files.
    fn collect_notes(&mut self, _replica: &Replica) -> Result<Vec<String>, SyncError> {
        Ok(Vec::new())
    }

    /// Writes notes changed elsewhere from the document. Called without the
    /// library locked, after the document is saved.
    fn write_notes(&mut self, _replica: &Replica, _paths: &[String]) -> Result<(), SyncError> {
        Ok(())
    }

    /// Before changes go out: stores the blobs they name.
    fn before_push(&mut self, _replica: &Replica, _state: &SyncState) -> Result<(), SyncError> {
        Ok(())
    }

    /// Saves the document and state. Changes are forgotten in the library
    /// only after this.
    fn save(&mut self, replica: &Replica, state: &SyncState) -> Result<(), SyncError>;
}

/// One whole round. The library is locked only around steps that touch
/// it, never across the network; notes are read and written without it.
pub fn sync(
    library: &Mutex<Library>,
    replica: &Replica,
    state: &mut SyncState,
    transport: &dyn Transport,
    hooks: &mut dyn Hooks,
) -> Result<Report, SyncError> {
    let lock = || library.lock().unwrap_or_else(|e| e.into_inner());
    let notes = hooks.collect_notes(replica)?;
    {
        let mut library = lock();
        let changed = collect(&library, replica, state, hooks, &notes)?;
        hooks.save(replica, state)?;
        library.forget_changed(&changed)?;
    }
    let mut sent = 0;
    if transport.ready_to_push() {
        hooks.before_push(replica, state)?;
        sent = push(transport, state)?;
        hooks.save(replica, state)?;
    }
    let fetched = fetch(transport, state)?;
    // Changes made here while fetching go in before anything is written.
    let notes = hooks.collect_notes(replica)?;
    let applied = {
        let mut library = lock();
        let changed = collect(&library, replica, state, hooks, &notes)?;
        let applied = apply(&mut library, replica, state, &fetched)?;
        hooks.save(replica, state)?;
        library.forget_changed(&changed)?;
        applied
    };
    hooks.write_notes(replica, &applied.notes)?;
    if transport.ready_to_push() {
        hooks.before_push(replica, state)?;
        sent += push(transport, state)?;
        hooks.save(replica, state)?;
    }
    Ok(Report {
        sent,
        received: applied.received,
        applied: applied.records.into_iter().map(|r| (r.kind, r.id)).collect(),
        notes: applied.notes,
    })
}

#[cfg(test)]
mod tests;
