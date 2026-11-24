use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use paperloom_model::{Creator, Doi, Item, ItemType};
use paperloom_sync_proto::{BlobHash, LoggedChange, Snapshot};

use super::*;

/// The service's log, in memory.
#[derive(Default)]
struct Log {
    changes: Mutex<Vec<LoggedChange>>,
    blobs: Mutex<HashMap<BlobHash, Vec<u8>>>,
    snapshot: Mutex<Option<Snapshot>>,
}

impl Log {
    /// Folds the log so far into a snapshot made from `replica`.
    fn fold(&self, replica: &Replica) {
        let bytes = replica.save().unwrap();
        let hash = BlobHash::of(&bytes);
        self.blobs.lock().unwrap().insert(hash.clone(), bytes);
        let mut changes = self.changes.lock().unwrap();
        let cursor = changes.last().map_or(0, |c| c.cursor);
        changes.clear();
        *self.snapshot.lock().unwrap() = Some(Snapshot { cursor, blob: hash });
    }
}

impl Transport for Log {
    fn push(&self, pushed: &[Change]) -> Result<Pushed, SyncError> {
        let mut changes = self.changes.lock().unwrap();
        let mut cursors = Vec::new();
        for change in pushed {
            let head = changes
                .last()
                .map_or(0, |c| c.cursor)
                .max(self.snapshot.lock().unwrap().as_ref().map_or(0, |s| s.cursor));
            if let Some(kept) = changes.iter().find(|c| c.device == change.device && c.seq == change.seq) {
                cursors.push(kept.cursor);
                continue;
            }
            changes.push(LoggedChange {
                cursor: head + 1,
                device: change.device,
                seq: change.seq,
                payload: change.payload.clone(),
            });
            cursors.push(head + 1);
        }
        Ok(Pushed { head: changes.last().map_or(0, |c| c.cursor), cursors })
    }

    fn pull(&self, after: u64, _positions: &Positions) -> Result<ChangesPage, SyncError> {
        let changes = self.changes.lock().unwrap();
        let head = changes.last().map_or(0, |c| c.cursor);
        if let Some(snapshot) = self.snapshot.lock().unwrap().clone().filter(|s| after < s.cursor) {
            return Ok(ChangesPage { changes: Vec::new(), head, more: true, snapshot: Some(snapshot) });
        }
        // Two at a time, to page.
        let page: Vec<LoggedChange> = changes.iter().filter(|c| c.cursor > after).take(3).cloned().collect();
        let more = page.len() == 3;
        Ok(ChangesPage { changes: page.into_iter().take(2).collect(), head, more, snapshot: None })
    }

    fn blob(&self, hash: &BlobHash) -> Result<Vec<u8>, SyncError> {
        self.blobs
            .lock()
            .unwrap()
            .get(hash)
            .cloned()
            .ok_or_else(|| SyncError::Transport("no such blob".into()))
    }
}

/// A device's side of a round: notes in memory, and the text each note
/// had when this device and the document last agreed on it.
struct DeviceHooks<'a> {
    saves: &'a mut usize,
    notes: &'a mut BTreeMap<String, String>,
    agreed: &'a mut BTreeMap<String, String>,
}

impl Hooks for DeviceHooks<'_> {
    fn collect_notes(&mut self, replica: &Replica) -> Result<Vec<String>, SyncError> {
        let mut changed = Vec::new();
        for (path, text) in self.notes.iter() {
            if self.agreed.get(path) != Some(text) {
                if replica.put_note(path, Some(text))? {
                    changed.push(path.clone());
                }
                self.agreed.insert(path.clone(), text.clone());
            }
        }
        let deleted: Vec<String> =
            self.agreed.keys().filter(|p| !self.notes.contains_key(*p)).cloned().collect();
        for path in deleted {
            if replica.put_note(&path, None)? {
                changed.push(path.clone());
            }
            self.agreed.remove(&path);
        }
        Ok(changed)
    }

    fn write_notes(&mut self, replica: &Replica, paths: &[String]) -> Result<(), SyncError> {
        for path in paths {
            match replica.note(path) {
                Some(Some(text)) => {
                    self.notes.insert(path.clone(), text.clone());
                    self.agreed.insert(path.clone(), text);
                }
                Some(None) => {
                    self.notes.remove(path);
                    self.agreed.remove(path);
                }
                None => {}
            }
        }
        Ok(())
    }

    fn save(&mut self, _: &Replica, _: &SyncState) -> Result<(), SyncError> {
        *self.saves += 1;
        Ok(())
    }
}

/// Storage people bring: each device's changes as files, no cursor. Some
/// devices' files can be held back, as a synced folder does before they
/// arrive.
#[derive(Default)]
struct Shelf {
    files: Mutex<BTreeMap<(Uuid, u64), Vec<u8>>>,
    held_back: Mutex<Vec<Uuid>>,
}

impl Transport for Shelf {
    fn push(&self, changes: &[Change]) -> Result<Pushed, SyncError> {
        let mut files = self.files.lock().unwrap();
        for change in changes {
            files.insert((change.device, change.seq), change.payload.clone());
        }
        Ok(Pushed { head: 0, cursors: vec![0; changes.len()] })
    }

    fn pull(&self, _after: u64, positions: &Positions) -> Result<ChangesPage, SyncError> {
        let held_back = self.held_back.lock().unwrap();
        let changes = self
            .files
            .lock()
            .unwrap()
            .iter()
            .filter(|((device, seq), _)| {
                !held_back.contains(device) && *seq > positions.get(device).copied().unwrap_or(0)
            })
            .map(|((device, seq), payload)| LoggedChange {
                cursor: 0,
                device: *device,
                seq: *seq,
                payload: payload.clone(),
            })
            .collect();
        Ok(ChangesPage { changes, head: 0, more: false, snapshot: None })
    }

    fn blob(&self, _hash: &BlobHash) -> Result<Vec<u8>, SyncError> {
        Err(SyncError::Transport("no blobs here".into()))
    }
}

/// A device: its library, document and sync state.
struct Device {
    library: Mutex<Library>,
    replica: Replica,
    state: SyncState,
    saves: usize,
    notes: BTreeMap<String, String>,
    agreed: BTreeMap<String, String>,
}

impl Device {
    fn new(peer: u64) -> Self {
        Self {
            library: Mutex::new(Library::open_in_memory().unwrap()),
            replica: Replica::new(peer).unwrap(),
            state: SyncState::new(Uuid::now_v7(), peer),
            saves: 0,
            notes: BTreeMap::new(),
            agreed: BTreeMap::new(),
        }
    }

    fn sync(&mut self, log: &dyn Transport) -> Report {
        let mut hooks =
            DeviceHooks { saves: &mut self.saves, notes: &mut self.notes, agreed: &mut self.agreed };
        sync(&self.library, &self.replica, &mut self.state, log, &mut hooks).unwrap()
    }

    fn lib(&self) -> std::sync::MutexGuard<'_, Library> {
        self.library.lock().unwrap()
    }
}

fn article(title: &str, doi: &str) -> Item {
    let mut item = Item::new(ItemType::JournalArticle, title);
    item.doi = Some(Doi::parse(doi).unwrap());
    item.creators.push(Creator::person("Curie", "Marie"));
    item
}

#[test]
fn two_devices_converge_field_by_field() {
    let log = Log::default();
    let mut mac = Device::new(1);
    let mut phone = Device::new(2);

    let paper = article("Radioactive substances", "10.1000/curie");
    mac.lib().add_item(&paper).unwrap();
    mac.lib().set_item_tags(&paper.id, &["physics".into()]).unwrap();
    let report = mac.sync(&log);
    assert_eq!((report.sent, report.received), (1, 0));
    assert!(mac.saves >= 2, "saved before forgetting changes");
    let report = phone.sync(&log);
    assert_eq!((report.sent, report.received, report.applied.len()), (0, 1, 2), "the item and its tag");
    assert_eq!(phone.lib().item(&paper.id).unwrap(), Some(paper.clone()));
    assert_eq!(phone.lib().item_tags(&paper.id).unwrap(), ["physics"]);

    // Different fields of one item, changed on both at once: both stay.
    let mut on_mac = paper.clone();
    on_mac.title = "Radioactive Substances".into();
    mac.lib().update_item(&on_mac).unwrap();
    phone.lib().set_starred(&paper.id, true).unwrap();
    phone.lib().set_item_tags(&paper.id, &["physics".into(), "nobel".into()]).unwrap();
    mac.lib().set_item_tags(&paper.id, &["physics".into(), "classic".into()]).unwrap();
    mac.sync(&log);
    phone.sync(&log);
    mac.sync(&log);
    for device in [&mac, &phone] {
        let lib = device.lib();
        let entry = lib.entry(&paper.id).unwrap().unwrap();
        assert_eq!(entry.item.title, "Radioactive Substances");
        assert!(entry.starred);
        assert_eq!(entry.tags, ["classic", "nobel", "physics"], "tags merge as a set");
    }
    assert_eq!(mac.lib().entry(&paper.id).unwrap(), phone.lib().entry(&paper.id).unwrap());
    assert!(mac.lib().changed_records().unwrap().is_empty());
    assert!(phone.lib().changed_records().unwrap().is_empty());

    // Nothing new: nothing sent.
    assert_eq!(mac.sync(&log).sent, 0);

    // Deleted for good on one device, gone on the other.
    phone.lib().delete_item(&paper.id).unwrap();
    phone.sync(&log);
    mac.sync(&log);
    assert_eq!(mac.lib().item(&paper.id).unwrap(), None);
}

#[test]
fn a_new_device_starts_from_the_snapshot() {
    let log = Log::default();
    let mut mac = Device::new(1);
    let papers: Vec<Item> = (0..5).map(|n| article(&format!("Paper {n}"), &format!("10.1000/{n}"))).collect();
    for paper in &papers {
        mac.lib().add_item(paper).unwrap();
    }
    mac.sync(&log);
    log.fold(&mac.replica);
    let late = article("Late", "10.1000/late");
    mac.lib().add_item(&late).unwrap();
    mac.sync(&log);

    let mut ipad = Device::new(3);
    let report = ipad.sync(&log);
    assert_eq!(report.applied.len(), 6);
    assert_eq!(ipad.lib().items().unwrap().len(), 6);
    assert_eq!(ipad.state.cursor, log.changes.lock().unwrap().last().unwrap().cursor);
}

#[test]
fn an_existing_library_sends_everything_first() {
    let log = Log::default();
    let mut mac = Device::new(1);
    let old = article("Old", "10.1000/old");
    mac.lib().add_item(&old).unwrap();
    {
        let mut lib = mac.lib();
        let changed = lib.changed_records().unwrap();
        lib.forget_changed(&changed).unwrap();
        lib.mark_all_changed().unwrap();
    }
    mac.sync(&log);
    let mut phone = Device::new(2);
    phone.sync(&log);
    assert_eq!(phone.lib().item(&old.id).unwrap(), Some(old));
}

#[test]
fn changes_survive_the_trip_and_their_format_is_checked() {
    let records = vec![(RecordKind::Item, "a".to_string())];
    let notes = vec!["Reading.md".to_string()];
    let bytes = Envelope::encode(&records, &notes, &[1, 2, 3]);
    assert_eq!(Envelope::decode(&bytes).unwrap(), Contents { records, notes, update: vec![1, 2, 3] });
    let without_notes = Envelope::encode(&[], &[], &[]);
    assert!(!String::from_utf8_lossy(&without_notes).contains("notes"), "left out when empty");
    let newer = br#"{"v":2,"records":[],"update":""}"#;
    assert!(matches!(Envelope::decode(newer), Err(SyncError::Corrupt(_))));
    assert!(Envelope::decode(b"junk").is_err());

    let replica = Replica::new(9).unwrap();
    let saved = replica.save().unwrap();
    assert!(Replica::load(&saved, 9).is_ok());
    assert_eq!(replica.get(RecordKind::Item, "missing"), None);
}

#[test]
fn a_tag_deleted_and_made_again_comes_back() {
    let log = Log::default();
    let mut mac = Device::new(1);
    let mut phone = Device::new(2);
    let paper = article("Tagged", "10.1000/tagged");
    mac.lib().add_item(&paper).unwrap();
    mac.lib().set_item_tags(&paper.id, &["draft".into()]).unwrap();
    mac.sync(&log);
    mac.lib().delete_tag("draft").unwrap();
    mac.sync(&log);
    mac.lib().set_item_tags(&paper.id, &["draft".into()]).unwrap();
    mac.lib().set_tag_color("draft", Some("#00ff00")).unwrap();
    mac.sync(&log);
    phone.sync(&log);
    assert_eq!(phone.lib().item_tags(&paper.id).unwrap(), ["draft"]);
    assert_eq!(phone.lib().tags().unwrap(), mac.lib().tags().unwrap());
}

#[test]
fn notes_edited_on_two_devices_at_once_merge() {
    let log = Log::default();
    let mut mac = Device::new(1);
    let mut phone = Device::new(2);
    let note = "Reading list.md".to_string();
    mac.notes.insert(note.clone(), "Line one\n".into());
    assert_eq!(mac.sync(&log).sent, 1);
    let report = phone.sync(&log);
    assert_eq!(report.notes, [note.as_str()]);
    assert_eq!(phone.notes[&note], "Line one\n");

    mac.notes.insert(note.clone(), "Line one\nFrom the Mac\n".into());
    phone.notes.insert(note.clone(), "From the phone\nLine one\n".into());
    mac.sync(&log);
    phone.sync(&log);
    mac.sync(&log);
    assert_eq!(mac.notes[&note], "From the phone\nLine one\nFrom the Mac\n");
    assert_eq!(phone.notes, mac.notes);
    assert_eq!(mac.sync(&log).sent, 0, "nothing new");

    phone.notes.remove(&note);
    phone.sync(&log);
    mac.sync(&log);
    assert!(mac.notes.is_empty(), "deleted on the phone, gone on the Mac");
}

#[test]
fn a_change_waits_for_the_one_it_builds_on() {
    let shelf = Shelf::default();
    let mut mac = Device::new(1);
    let mut phone = Device::new(2);
    let mut ipad = Device::new(3);
    let paper = article("Waiting", "10.1000/waiting");
    mac.lib().add_item(&paper).unwrap();
    mac.sync(&shelf);
    phone.sync(&shelf);
    phone.lib().set_starred(&paper.id, true).unwrap();
    phone.sync(&shelf);

    // The iPad sees the phone's change before the Mac's it builds on.
    shelf.held_back.lock().unwrap().push(mac.state.device);
    let report = ipad.sync(&shelf);
    assert_eq!((report.received, report.applied.len()), (0, 0), "nothing it can use yet");
    assert_eq!(ipad.lib().item(&paper.id).unwrap(), None);
    assert_eq!(ipad.state.positions.get(&phone.state.device), None, "the phone's change comes again");

    shelf.held_back.lock().unwrap().clear();
    let report = ipad.sync(&shelf);
    assert_eq!(report.received, 2);
    assert!(ipad.lib().entry(&paper.id).unwrap().unwrap().starred);
    assert_eq!(ipad.sync(&shelf).received, 0, "each change taken in once");
}

/// Cheap keys for tests; devices use [`seal::Cost::DEFAULT`].
const TEST_COST: seal::Cost = seal::Cost { memory_kib: 64, passes: 1 };

#[test]
fn sealed_changes_reach_devices_with_the_passphrase() {
    let log = Log::default();
    let mut mac = Device::new(1);
    let mut phone = Device::new(2);
    let mac_key = seal::Sealer::new("correct horse", [1; 16], TEST_COST).unwrap();
    let phone_key = seal::Sealer::new("correct horse", [2; 16], TEST_COST).unwrap();

    let paper = article("Radioactive substances", "10.1000/curie");
    mac.lib().add_item(&paper).unwrap();
    let report = mac.sync(&seal::SealedTransport::new(&log, Some(&mac_key)));
    assert_eq!(report.sent, 1, "nothing sealed in the log yet: the first device sends");
    let kept = log.changes.lock().unwrap()[0].payload.clone();
    assert!(seal::is_sealed(&kept), "the service holds only ciphertext");
    assert!(!String::from_utf8_lossy(&kept).contains("Radioactive"));

    let report = phone.sync(&seal::SealedTransport::new(&log, Some(&phone_key)));
    assert_eq!(report.received, 1);
    assert_eq!(phone.lib().item(&paper.id).unwrap(), Some(paper.clone()));

    // Without the passphrase, or with another one, nothing is read and
    // nothing goes out.
    let mut stranger = Device::new(3);
    stranger.lib().add_item(&article("Other", "10.1000/other")).unwrap();
    let mut hooks =
        DeviceHooks { saves: &mut stranger.saves, notes: &mut stranger.notes, agreed: &mut stranger.agreed };
    let plain = seal::SealedTransport::new(&log, None);
    let locked = sync(&stranger.library, &stranger.replica, &mut stranger.state, &plain, &mut hooks);
    assert!(matches!(locked, Err(SyncError::Locked)), "{locked:?}");
    let wrong = seal::Sealer::new("battery staple", [3; 16], TEST_COST).unwrap();
    let mistyped = seal::SealedTransport::new(&log, Some(&wrong));
    let refused = sync(&stranger.library, &stranger.replica, &mut stranger.state, &mistyped, &mut hooks);
    assert!(matches!(refused, Err(SyncError::Passphrase)), "{refused:?}");
    assert_eq!(log.changes.lock().unwrap().len(), 1, "a mistyped passphrase sends nothing");
}

#[test]
fn seals_and_opens() {
    let sealer = seal::Sealer::new("pass", [7; 16], TEST_COST).unwrap();
    let sealed = sealer.seal(b"hello");
    assert!(seal::is_sealed(&sealed) && !seal::is_sealed(b"{\"v\":1}"));
    assert_eq!(sealer.open(&sealed).unwrap(), b"hello");
    assert_ne!(sealer.seal(b"hello"), sealed, "changes get fresh nonces");
    assert_eq!(sealer.seal_file(b"file"), sealer.seal_file(b"file"), "a file seals the same way each time");

    let other = seal::Sealer::new("pass", [8; 16], TEST_COST).unwrap();
    assert_eq!(other.open(&sealed).unwrap(), b"hello", "another salt, the same passphrase");
    let mut damaged = sealed.clone();
    let last = damaged.len() - 1;
    damaged[last] ^= 1;
    assert!(matches!(sealer.open(&damaged), Err(SyncError::Passphrase)));
    let mut greedy = sealed;
    greedy[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(other.open(&greedy), Err(SyncError::Corrupt(_))), "unreasonable key settings");
    assert!(seal::Sealer::new("", [0; 16], TEST_COST).is_err());
}
