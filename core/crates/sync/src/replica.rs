//! The library's records in a Loro document. One root map per kind of
//! record, one mergeable map per record (so two devices that both make a
//! record end up with one), one entry per field holding the field's JSON.
//! Concurrent changes merge field by field; a record deleted anywhere is
//! deleted everywhere (`$gone`).

use loro::{
    Container, ExportMode, LoroDoc, LoroMap, LoroValue, UpdateOptions, ValueOrContainer, VersionVector,
};
use paperloom_store::{Fields, Record, RecordKind};

use crate::{RecordRefs, SyncError};

const GONE: &str = "$gone";
/// The root map of notes, by path; each holds its text and maybe `$gone`.
const NOTES: &str = "note";
const TEXT: &str = "text";

pub struct Replica {
    doc: LoroDoc,
}

impl Replica {
    /// An empty document, writing as `peer` (unique to this device and library).
    pub fn new(peer: u64) -> Result<Self, SyncError> {
        let doc = LoroDoc::new();
        doc.set_peer_id(peer)?;
        Ok(Self { doc })
    }

    /// A document saved with [`Replica::save`].
    pub fn load(saved: &[u8], peer: u64) -> Result<Self, SyncError> {
        let doc = LoroDoc::from_snapshot(saved)?;
        doc.set_peer_id(peer)?;
        Ok(Self { doc })
    }

    pub fn save(&self) -> Result<Vec<u8>, SyncError> {
        self.doc.commit();
        Ok(self.doc.export(ExportMode::Snapshot)?)
    }

    fn root(&self, kind: RecordKind) -> LoroMap {
        self.doc.get_map(kind.as_str())
    }

    /// Writes a record, touching only the fields that differ. True if
    /// anything changed.
    pub fn put(&self, record: &Record) -> Result<bool, SyncError> {
        let map = self.root(record.kind).ensure_mergeable_map(&record.id)?;
        let mut changed = false;
        let Some(fields) = &record.fields else {
            if text_of(map.get(GONE)).is_none() {
                map.insert(GONE, "true")?;
                changed = true;
            }
            return Ok(changed);
        };
        // Made again here after being deleted (a tag with the same name).
        if text_of(map.get(GONE)).is_some() {
            map.delete(GONE)?;
            changed = true;
        }
        for (key, value) in fields {
            let json = value.to_string();
            if text_of(map.get(key)).as_deref() != Some(json.as_str()) {
                map.insert(key, json)?;
                changed = true;
            }
        }
        let stale: Vec<String> = map
            .keys()
            .map(|k| k.as_str().to_string())
            .filter(|k| k != GONE && !fields.contains_key(k))
            .collect();
        for key in stale {
            map.delete(&key)?;
            changed = true;
        }
        Ok(changed)
    }

    /// A record as the document holds it: `None` if the document has never
    /// had it, `Some(None)` if it is gone.
    pub fn get(&self, kind: RecordKind, id: &str) -> Option<Option<Fields>> {
        let Some(ValueOrContainer::Container(Container::Map(map))) = self.root(kind).get(id) else {
            return None;
        };
        let mut fields = Fields::new();
        let mut gone = false;
        map.for_each(|key, value| {
            if key == GONE {
                gone = true;
            } else if let Some(text) = text_of(Some(value))
                && let Ok(value) = serde_json::from_str(&text)
            {
                fields.insert(key.to_string(), value);
            }
        });
        Some(if gone { None } else { Some(fields) })
    }

    /// A note's text: `None` if the document has never had the note,
    /// `Some(None)` if it was deleted.
    pub fn note(&self, path: &str) -> Option<Option<String>> {
        let Some(ValueOrContainer::Container(Container::Map(map))) = self.doc.get_map(NOTES).get(path) else {
            return None;
        };
        if text_of(map.get(GONE)).is_some() {
            return Some(None);
        }
        Some(Some(match map.get(TEXT) {
            Some(ValueOrContainer::Container(Container::Text(text))) => text.to_string(),
            _ => String::new(),
        }))
    }

    /// Writes a note's text, as edits from what the document has (so edits
    /// made elsewhere at the same time merge), or marks it deleted. True if
    /// anything changed.
    pub fn put_note(&self, path: &str, text: Option<&str>) -> Result<bool, SyncError> {
        let map = self.doc.get_map(NOTES).ensure_mergeable_map(path)?;
        let gone = text_of(map.get(GONE)).is_some();
        let Some(text) = text else {
            if gone {
                return Ok(false);
            }
            map.insert(GONE, "true")?;
            return Ok(true);
        };
        let mut changed = false;
        if gone {
            map.delete(GONE)?;
            changed = true;
        }
        let container = map.ensure_mergeable_text(TEXT)?;
        if container.to_string() != text {
            container
                .update(text, UpdateOptions::default())
                .map_err(|e| SyncError::Corrupt(format!("note {path}: {e}")))?;
            changed = true;
        }
        Ok(changed)
    }

    /// Every note the document has, deleted ones too.
    pub fn note_paths(&self) -> Vec<String> {
        self.doc.get_map(NOTES).keys().map(|path| path.as_str().to_string()).collect()
    }

    /// Every record the document holds, for loading a snapshot.
    pub fn all(&self) -> RecordRefs {
        RecordKind::ALL
            .into_iter()
            .flat_map(|kind| {
                self.root(kind).keys().map(move |id| (kind, id.as_str().to_string())).collect::<Vec<_>>()
            })
            .collect()
    }

    /// How far this device's own writes go (the end of its counter).
    pub fn own_end(&self) -> i32 {
        self.doc.oplog_vv().get(&self.doc.peer_id()).copied().unwrap_or(0)
    }

    /// This device's writes after `from` (an earlier [`Replica::own_end`]).
    pub fn own_updates_since(&self, from: i32) -> Result<Vec<u8>, SyncError> {
        self.doc.commit();
        let mut since: VersionVector = self.doc.oplog_vv();
        since.insert(self.doc.peer_id(), from);
        Ok(self.doc.export(ExportMode::updates(&since))?)
    }

    /// Takes in another device's writes. False when some wait for writes
    /// the document does not have yet: they are kept, and apply once those
    /// arrive (importing the same writes again is harmless).
    pub fn import(&self, update: &[u8]) -> Result<bool, SyncError> {
        let status = self.doc.import(update)?;
        Ok(status.pending.is_none())
    }
}

fn text_of(value: Option<ValueOrContainer>) -> Option<String> {
    match value {
        Some(ValueOrContainer::Value(LoroValue::String(text))) => Some(text.as_str().to_string()),
        _ => None,
    }
}
