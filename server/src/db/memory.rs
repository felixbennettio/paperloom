//! The database in memory, for tests.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use paperloom_sync_proto::{
    BlobHash, Change, ChangesPage, Device, Library, LoggedChange, NewDevice, NewLibrary, Pushed, Role,
    Snapshot, Usage,
};
use uuid::Uuid;

use super::{UserId, check_snapshot, gap, now_ms, unknown_device};
use crate::error::ApiError;

#[derive(Default)]
pub struct Memory {
    data: Mutex<Data>,
}

#[derive(Default)]
struct Data {
    /// Subjects; an account's ID is its place here plus one.
    users: Vec<String>,
    devices: HashMap<Uuid, (UserId, Device)>,
    libraries: HashMap<Uuid, Log>,
}

struct Log {
    name: String,
    owner: UserId,
    created_at: i64,
    members: HashMap<UserId, Role>,
    head: u64,
    changes: Vec<LoggedChange>,
    last_seq: HashMap<Uuid, u64>,
    snapshot: Option<Snapshot>,
    blobs: HashMap<BlobHash, u64>,
}

impl Log {
    fn summary(&self, id: Uuid, role: Role) -> Library {
        Library { id, name: self.name.clone(), role, head: self.head, created_at: self.created_at }
    }
}

impl Memory {
    fn data(&self) -> MutexGuard<'_, Data> {
        self.data.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn log(data: &mut Data, id: Uuid) -> Result<&mut Log, ApiError> {
        data.libraries.get_mut(&id).ok_or_else(|| ApiError::NotFound("No such library".into()))
    }

    pub fn user(&self, sub: &str) -> Result<UserId, ApiError> {
        let mut data = self.data();
        if let Some(at) = data.users.iter().position(|s| s == sub) {
            return Ok(at as UserId + 1);
        }
        data.users.push(sub.to_string());
        Ok(data.users.len() as UserId)
    }

    pub fn register_device(&self, user: UserId, new: NewDevice) -> Result<Device, ApiError> {
        let mut data = self.data();
        if let Some((owner, device)) = data.devices.get_mut(&new.id) {
            if *owner != user {
                return Err(ApiError::conflict("This device ID belongs to another account."));
            }
            device.name = new.name;
            device.platform = new.platform;
            return Ok(device.clone());
        }
        let device = Device { id: new.id, name: new.name, platform: new.platform, created_at: now_ms() };
        data.devices.insert(device.id, (user, device.clone()));
        Ok(device)
    }

    pub fn devices(&self, user: UserId) -> Result<Vec<Device>, ApiError> {
        let mut devices: Vec<Device> = self
            .data()
            .devices
            .values()
            .filter(|(owner, _)| *owner == user)
            .map(|(_, d)| d.clone())
            .collect();
        devices.sort_by_key(|d| (d.created_at, d.id));
        Ok(devices)
    }

    pub fn libraries(&self, user: UserId) -> Result<Vec<Library>, ApiError> {
        let data = self.data();
        let mut found: Vec<Library> = data
            .libraries
            .iter()
            .filter_map(|(id, log)| log.members.get(&user).map(|role| log.summary(*id, *role)))
            .collect();
        found.sort_by_key(|l| (l.created_at, l.id));
        Ok(found)
    }

    pub fn create_library(&self, user: UserId, new: NewLibrary) -> Result<Library, ApiError> {
        let mut data = self.data();
        let log = data.libraries.entry(new.id).or_insert_with(|| Log {
            name: new.name,
            owner: user,
            created_at: now_ms(),
            members: HashMap::from([(user, Role::Owner)]),
            head: 0,
            changes: Vec::new(),
            last_seq: HashMap::new(),
            snapshot: None,
            blobs: HashMap::new(),
        });
        match log.members.get(&user) {
            Some(role) => Ok(log.summary(new.id, *role)),
            None => Err(ApiError::conflict("This library ID is taken.")),
        }
    }

    pub fn library(&self, user: UserId, id: Uuid) -> Result<Option<Library>, ApiError> {
        let data = self.data();
        Ok(data.libraries.get(&id).and_then(|log| log.members.get(&user).map(|role| log.summary(id, *role))))
    }

    pub fn push(&self, user: UserId, library: Uuid, changes: Vec<Change>) -> Result<Pushed, ApiError> {
        let mut data = self.data();
        if changes.iter().any(|c| data.devices.get(&c.device).is_none_or(|(owner, _)| *owner != user)) {
            return Err(unknown_device());
        }
        let log = Self::log(&mut data, library)?;
        // Worked out first, kept only if every change fits.
        let mut head = log.head;
        let mut last_seq = log.last_seq.clone();
        let mut added = Vec::new();
        let mut cursors = Vec::with_capacity(changes.len());
        for change in changes {
            let last = last_seq.get(&change.device).copied().unwrap_or(0);
            if change.seq <= last {
                let kept = log
                    .changes
                    .iter()
                    .chain(&added)
                    .find(|c| c.device == change.device && c.seq == change.seq);
                cursors.push(kept.map_or(0, |c| c.cursor));
                continue;
            }
            if change.seq != last + 1 {
                return Err(gap(last + 1));
            }
            head += 1;
            last_seq.insert(change.device, change.seq);
            cursors.push(head);
            added.push(LoggedChange {
                cursor: head,
                device: change.device,
                seq: change.seq,
                payload: change.payload,
            });
        }
        log.head = head;
        log.last_seq = last_seq;
        log.changes.extend(added);
        Ok(Pushed { head, cursors })
    }

    pub fn changes(&self, library: Uuid, after: u64, limit: usize) -> Result<ChangesPage, ApiError> {
        let mut data = self.data();
        let log = Self::log(&mut data, library)?;
        if let Some(snapshot) = log.snapshot.as_ref().filter(|s| after < s.cursor) {
            return Ok(ChangesPage {
                changes: Vec::new(),
                head: log.head,
                more: true,
                snapshot: Some(snapshot.clone()),
            });
        }
        let mut changes: Vec<LoggedChange> =
            log.changes.iter().filter(|c| c.cursor > after).take(limit + 1).cloned().collect();
        let more = changes.len() > limit;
        changes.truncate(limit);
        Ok(ChangesPage { changes, head: log.head, more, snapshot: None })
    }

    pub fn set_snapshot(&self, library: Uuid, snapshot: Snapshot) -> Result<(), ApiError> {
        let mut data = self.data();
        let log = Self::log(&mut data, library)?;
        check_snapshot(&snapshot, log.head, log.snapshot.as_ref().map(|s| s.cursor))?;
        log.changes.retain(|c| c.cursor > snapshot.cursor);
        log.snapshot = Some(snapshot);
        Ok(())
    }

    pub fn add_blob(&self, library: Uuid, hash: &BlobHash, size: u64) -> Result<(), ApiError> {
        let mut data = self.data();
        Self::log(&mut data, library)?.blobs.insert(hash.clone(), size);
        Ok(())
    }

    pub fn has_blob(&self, library: Uuid, hash: &BlobHash) -> Result<bool, ApiError> {
        let mut data = self.data();
        Ok(Self::log(&mut data, library)?.blobs.contains_key(hash))
    }

    pub fn usage(&self, user: UserId) -> Result<Usage, ApiError> {
        let data = self.data();
        let mut usage = Usage::default();
        for log in data.libraries.values().filter(|l| l.owner == user) {
            usage.libraries += 1;
            usage.change_bytes += log.changes.iter().map(|c| c.payload.len() as u64).sum::<u64>();
            usage.blob_bytes += log.blobs.values().sum::<u64>();
        }
        Ok(usage)
    }
}
