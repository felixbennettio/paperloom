//! Syncing a library with Paperloom Cloud.
//!
//! The service is a [`Transport`] for the sync engine. What the library
//! keeps about syncing is one file, `sync/state.bin` beside it: the state
//! and the Loro document together, replaced whole, so the two never
//! disagree after a crash. Attached files go up as blobs before the
//! changes that name them; after each sync, files this device lacks come
//! down (a failed download is tried again next time).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

use paperloom_http::{BytesResponse, Http, Request, UreqHttp};
use paperloom_model::{AnnotationId, AttachmentId, ItemId};
use paperloom_store::{Fields, Record, RecordKind};
use paperloom_sync::seal::{Cost, SealedTransport, Sealer};
use paperloom_sync::{Hooks, Positions, Replica, SyncError, SyncState, Transport};
use paperloom_sync_proto::{
    BlobHash, Change, ChangesPage, ErrorBody, Library as CloudLibrary, NewDevice, NewLibrary, Push, Pushed,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::files::FileStore;
use crate::{CoreError, PaperLibrary, PaperVault};

const STATE_FILE: &str = "state.bin";
const MAGIC: &[u8] = b"PAPERLOOM-SYNC-1\n";

impl From<SyncError> for CoreError {
    fn from(err: SyncError) -> Self {
        let message = err.to_string();
        match err {
            SyncError::SignIn => CoreError::Account { message },
            SyncError::Transport(_) => CoreError::Network { message },
            SyncError::Locked | SyncError::Passphrase => CoreError::InvalidInput { message },
            _ => CoreError::Storage { message },
        }
    }
}

/// What a sync did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    /// Changes sent from this device.
    pub sent: u32,
    /// Changes received from other devices.
    pub received: u32,
    /// Records (items, tags, annotations…) other devices changed here.
    pub changed: u32,
    /// Attached files downloaded.
    pub downloaded: u32,
    /// Notes other devices changed here.
    pub notes: u32,
    /// Milliseconds since 1970.
    pub finished_at: i64,
}

/// Whether and how this library syncs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    /// The service this library syncs with; none before the first sync.
    pub service: Option<String>,
    /// Milliseconds since 1970.
    pub last_synced: Option<i64>,
    /// Changes made here that the service does not have yet.
    pub waiting: u32,
}

/// What the library keeps about syncing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudState {
    /// The endpoint: the service's address, `folder:<path>` or
    /// `webdav:<address>`.
    service: String,
    library: Uuid,
    /// The device is registered and the library exists on the service.
    joined: bool,
    sync: SyncState,
    /// Each attached file's blob (BLAKE3), by its SHA-256.
    #[serde(default)]
    blobs: BTreeMap<String, String>,
    /// Blobs to upload: hash, and the file's place in the store.
    #[serde(default)]
    waiting: BTreeMap<String, String>,
    /// Each note's text when this device and the document last agreed on
    /// it (BLAKE3), by path. A note whose file still has that text has not
    /// changed here.
    #[serde(default)]
    notes: BTreeMap<String, String>,
    #[serde(default)]
    last_synced: Option<i64>,
}

impl CloudState {
    fn new(service: &str) -> Self {
        let device = Uuid::now_v7();
        // The Loro peer: the random half of the device's ID, never zero.
        let mut half = [0u8; 8];
        half.copy_from_slice(&device.as_bytes()[8..]);
        let peer = u64::from_be_bytes(half) | 1;
        Self {
            service: service.to_string(),
            library: Uuid::now_v7(),
            joined: false,
            sync: SyncState::new(device, peer),
            blobs: BTreeMap::new(),
            waiting: BTreeMap::new(),
            notes: BTreeMap::new(),
            last_synced: None,
        }
    }
}

pub(crate) fn local(path: &Path, err: io::Error) -> SyncError {
    SyncError::Local(format!("{}: {err}", path.display()))
}

pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), SyncError> {
    let partial = path.with_extension("partial");
    fs::write(&partial, bytes).map_err(|e| local(&partial, e))?;
    fs::rename(&partial, path).map_err(|e| local(path, e))
}

fn write_state(dir: &Path, state: &CloudState, replica: &Replica) -> Result<(), SyncError> {
    let json = serde_json::to_vec(state).map_err(|e| SyncError::Local(e.to_string()))?;
    let document = replica.save()?;
    let mut bytes = Vec::with_capacity(MAGIC.len() + 8 + json.len() + document.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(json.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&json);
    bytes.extend_from_slice(&document);
    write_atomically(&dir.join(STATE_FILE), &bytes)
}

/// The state and the document's bytes; none before the first sync.
fn read_state(dir: &Path) -> Result<Option<(CloudState, Vec<u8>)>, SyncError> {
    let path = dir.join(STATE_FILE);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(local(&path, e)),
    };
    let damaged = || SyncError::Corrupt(format!("{} is damaged", path.display()));
    let rest = bytes.strip_prefix(MAGIC).ok_or_else(damaged)?;
    let (length, rest) = rest.split_at_checked(8).ok_or_else(damaged)?;
    let length = <[u8; 8]>::try_from(length).map_err(|_| damaged())?;
    let length = usize::try_from(u64::from_le_bytes(length)).map_err(|_| damaged())?;
    let (json, document) = rest.split_at_checked(length).ok_or_else(damaged)?;
    let state = serde_json::from_slice(json).map_err(|_| damaged())?;
    Ok(Some((state, document.to_vec())))
}

fn json_body<T: Serialize>(value: &T) -> Option<(Vec<u8>, &'static str)> {
    Some((serde_json::to_vec(value).unwrap_or_default(), "application/json"))
}

fn read_json<T: DeserializeOwned>(response: BytesResponse) -> Result<T, SyncError> {
    serde_json::from_slice(&response.body)
        .map_err(|e| SyncError::Transport(format!("The sync service's answer cannot be read: {e}")))
}

/// The service, for one library.
struct Cloud<'a> {
    http: &'a dyn Http,
    base: String,
    token: String,
    library: Uuid,
}

impl Cloud<'_> {
    fn call(
        &self,
        method: &'static str,
        path: &str,
        body: Option<(Vec<u8>, &str)>,
    ) -> Result<BytesResponse, SyncError> {
        let mut headers = vec![
            ("Authorization".to_string(), format!("Bearer {}", self.token)),
            ("Accept".to_string(), "application/json".to_string()),
        ];
        let body = body.map(|(bytes, content_type)| {
            headers.push(("Content-Type".to_string(), content_type.to_string()));
            bytes
        });
        let request = Request { method, url: format!("{}{path}", self.base), headers, body };
        let response = self.http.send(request).map_err(|e| SyncError::Transport(e.0))?;
        if response.is_success() {
            return Ok(response);
        }
        let error = serde_json::from_slice::<ErrorBody>(&response.body).ok();
        Err(match (response.status, error) {
            (401, _) => SyncError::SignIn,
            (409, Some(ErrorBody { expected_seq: Some(expected), .. })) => SyncError::Gap { expected },
            (status, Some(error)) => SyncError::Transport(format!("{} (HTTP {status})", error.message)),
            (status, None) => SyncError::Transport(format!("The sync service answered HTTP {status}.")),
        })
    }

    fn in_library(&self, rest: &str) -> String {
        format!("/v1/libraries/{}{rest}", self.library)
    }

    /// Registers the device, then joins the account's library, or makes it.
    fn join(&mut self, device: &NewDevice) -> Result<(), SyncError> {
        self.call("POST", "/v1/devices", json_body(device))?;
        let existing: Vec<CloudLibrary> = read_json(self.call("GET", "/v1/libraries", None)?)?;
        match existing.first() {
            Some(library) => self.library = library.id,
            None => {
                let new = NewLibrary { id: self.library, name: "Paperloom".into() };
                self.call("POST", "/v1/libraries", json_body(&new))?;
            }
        }
        Ok(())
    }
}

/// Where attached files go: the service, or storage people bring.
pub(crate) trait BlobSink {
    /// Stores a blob named by its hash; one already there is left alone.
    fn put_blob(&self, hash: &str, bytes: Vec<u8>) -> Result<(), SyncError>;
}

impl BlobSink for Cloud<'_> {
    fn put_blob(&self, hash: &str, bytes: Vec<u8>) -> Result<(), SyncError> {
        let path = self.in_library(&format!("/blobs/{hash}"));
        self.call("PUT", &path, Some((bytes, "application/octet-stream")))?;
        Ok(())
    }
}

impl Transport for Cloud<'_> {
    fn push(&self, changes: &[Change]) -> Result<Pushed, SyncError> {
        let push = Push { changes: changes.to_vec() };
        read_json(self.call("POST", &self.in_library("/changes"), json_body(&push))?)
    }

    fn pull(&self, after: u64, _positions: &Positions) -> Result<ChangesPage, SyncError> {
        read_json(self.call("GET", &self.in_library(&format!("/changes?after={after}")), None)?)
    }

    fn blob(&self, hash: &BlobHash) -> Result<Vec<u8>, SyncError> {
        let bytes = self.call("GET", &self.in_library(&format!("/blobs/{hash}")), None)?.body;
        if BlobHash::of(&bytes) != *hash {
            return Err(SyncError::Corrupt(format!("blob {hash} arrived damaged")));
        }
        Ok(bytes)
    }
}

struct CloudHooks<'a> {
    blobs: &'a dyn BlobSink,
    /// Seals files before they go up, when syncing end to end.
    sealer: Option<&'a Sealer>,
    files: &'a FileStore,
    vault: Option<&'a PaperVault>,
    dir: &'a Path,
    state: CloudState,
}

fn note_hash(text: &str) -> String {
    BlobHash::of(text.as_bytes()).to_string()
}

fn vault_error(err: CoreError) -> SyncError {
    SyncError::Local(err.to_string())
}

impl CloudHooks<'_> {
    /// The bytes a file is stored as: sealed when syncing end to end (the
    /// same way each time, so its blob keeps its name).
    fn blob_bytes(&self, plain: Vec<u8>) -> Vec<u8> {
        match self.sealer {
            Some(sealer) => sealer.seal_file(&plain),
            None => plain,
        }
    }

    /// Writes notes from the document into the vault, unless a note
    /// changed here since this device last agreed with the document: that
    /// change goes in at the next sync and the two merge.
    fn write_vault_notes(
        &mut self,
        vault: &PaperVault,
        replica: &Replica,
        paths: &[String],
    ) -> Result<(), SyncError> {
        for path in paths {
            let current = vault.read_note(path.clone()).ok();
            if current.as_deref().map(note_hash).as_ref() != self.state.notes.get(path) && current.is_some() {
                continue;
            }
            match replica.note(path) {
                Some(Some(text)) => {
                    if current.as_deref() != Some(text.as_str()) {
                        vault.write_note(path.clone(), text.clone()).map_err(vault_error)?;
                    }
                    self.state.notes.insert(path.clone(), note_hash(&text));
                }
                Some(None) => {
                    if current.is_some() {
                        vault.delete_note(path.clone()).map_err(vault_error)?;
                    }
                    self.state.notes.remove(path);
                }
                None => {}
            }
        }
        Ok(())
    }
}

fn text<'a>(fields: &'a Fields, key: &str) -> Option<&'a str> {
    fields.get(key).and_then(Value::as_str)
}

impl Hooks for CloudHooks<'_> {
    /// Names an attachment's file by its blob, so other devices can fetch it.
    fn decorate(&mut self, replica: &Replica, record: &mut Record) -> Result<(), SyncError> {
        if record.kind != RecordKind::Attachment {
            return Ok(());
        }
        let Some(fields) = record.fields.as_mut() else { return Ok(()) };
        let sha256 = text(fields, "sha256").unwrap_or_default().to_string();
        let place = text(fields, "path").unwrap_or_default().to_string();
        // A device without the file keeps the blob the document names.
        let kept = replica
            .get(RecordKind::Attachment, &record.id)
            .flatten()
            .filter(|kept| text(kept, "sha256") == Some(sha256.as_str()))
            .and_then(|kept| text(&kept, "blob").map(str::to_string));
        let hash = match self.state.blobs.get(&sha256).cloned().or(kept) {
            Some(hash) => hash,
            None => {
                let Some(bytes) = self.files.synced_path(&place).and_then(|path| fs::read(path).ok()) else {
                    return Ok(());
                };
                let hash = BlobHash::of(&self.blob_bytes(bytes)).to_string();
                self.state.waiting.insert(hash.clone(), place);
                hash
            }
        };
        self.state.blobs.insert(sha256, hash.clone());
        fields.insert("blob".into(), Value::String(hash));
        Ok(())
    }

    /// Puts notes edited, added or deleted here (in the app or in another
    /// one, such as Obsidian) into the document. Notes the document has
    /// that this device never wrote (after a crash) are written now.
    fn collect_notes(&mut self, replica: &Replica) -> Result<Vec<String>, SyncError> {
        let Some(vault) = self.vault else { return Ok(Vec::new()) };
        vault.refresh().map_err(vault_error)?;
        let mut changed = Vec::new();
        let mut present = BTreeSet::new();
        let mut unwritten = Vec::new();
        for info in vault.notes().map_err(vault_error)? {
            let text = vault.read_note(info.path.clone()).map_err(vault_error)?;
            let hash = note_hash(&text);
            present.insert(info.path.clone());
            if self.state.notes.get(&info.path) == Some(&hash) {
                if replica.note(&info.path).is_some_and(|doc| doc.as_deref() != Some(text.as_str())) {
                    unwritten.push(info.path);
                }
                continue;
            }
            if replica.put_note(&info.path, Some(&text))? {
                changed.push(info.path.clone());
            }
            self.state.notes.insert(info.path, hash);
        }
        let deleted: Vec<String> =
            self.state.notes.keys().filter(|p| !present.contains(*p)).cloned().collect();
        for path in deleted {
            if replica.put_note(&path, None)? {
                changed.push(path.clone());
            }
            self.state.notes.remove(&path);
        }
        unwritten.extend(
            replica
                .note_paths()
                .into_iter()
                .filter(|p| !present.contains(p) && !self.state.notes.contains_key(p)),
        );
        self.write_vault_notes(vault, replica, &unwritten)?;
        Ok(changed)
    }

    fn write_notes(&mut self, replica: &Replica, paths: &[String]) -> Result<(), SyncError> {
        match self.vault {
            Some(vault) => self.write_vault_notes(vault, replica, paths),
            None => Ok(()),
        }
    }

    /// Uploads the files the changes about to go out name.
    fn before_push(&mut self, _replica: &Replica, _state: &SyncState) -> Result<(), SyncError> {
        let waiting: Vec<(String, String)> = self.state.waiting.clone().into_iter().collect();
        for (hash, place) in waiting {
            let bytes = self.files.synced_path(&place).and_then(|path| fs::read(path).ok());
            // A file gone or changed since has nothing to send.
            if let Some(bytes) = bytes.map(|plain| self.blob_bytes(plain))
                && BlobHash::of(&bytes).as_str() == hash
            {
                self.blobs.put_blob(&hash, bytes)?;
            }
            self.state.waiting.remove(&hash);
        }
        Ok(())
    }

    fn save(&mut self, replica: &Replica, state: &SyncState) -> Result<(), SyncError> {
        self.state.sync = state.clone();
        write_state(self.dir, &self.state, replica)
    }
}

#[uniffi::export]
impl PaperLibrary {
    /// Syncs this library with Paperloom Cloud at `service`, signed in with
    /// an access token for the sync service (`AccountService::refresh` with
    /// `for_sync`). The first sync sends the whole library. Blocks on the
    /// network: call it off the main thread.
    pub fn sync_with_cloud(
        &self,
        service: String,
        access_token: String,
        device_name: String,
        platform: String,
        vault: Option<Arc<PaperVault>>,
    ) -> Result<SyncReport, CoreError> {
        let device = ThisDevice { name: &device_name, platform: &platform };
        self.sync_through(&UreqHttp::new(), &service, &access_token, device, vault.as_deref())
    }

    /// Seals what this library sends when it syncs, and opens what other
    /// devices sealed, with a key made from `passphrase` (every device needs
    /// the same one); none stops sealing. Making the key takes a moment:
    /// call it off the main thread.
    pub fn set_sync_passphrase(&self, passphrase: Option<String>) -> Result<(), CoreError> {
        self.seal_with(passphrase, Cost::DEFAULT)
    }

    /// Whether syncing seals changes and files end to end.
    pub fn sync_sealed(&self) -> bool {
        self.sealer.lock().is_ok_and(|s| s.is_some())
    }

    /// Whether this library syncs, when it last did, and what waits to go.
    pub fn sync_status(&self) -> Result<SyncStatus, CoreError> {
        let changed = self.lock()?.changed_records()?.len();
        let Some(dir) = self.sync_dir.as_deref() else {
            return Ok(SyncStatus { service: None, last_synced: None, waiting: 0 });
        };
        Ok(match read_state(dir)? {
            Some((state, _)) => SyncStatus {
                service: Some(state.service),
                last_synced: state.last_synced,
                waiting: (state.sync.outbox.len() + changed) as u32,
            },
            None => SyncStatus { service: None, last_synced: None, waiting: 0 },
        })
    }
}

/// This device, as the service lists it.
pub(crate) struct ThisDevice<'a> {
    pub name: &'a str,
    pub platform: &'a str,
}

impl PaperLibrary {
    /// This device's salt for its sealing key, made once and kept in the
    /// sync folder (a library in memory makes a new one).
    fn device_salt(&self) -> Result<[u8; 16], CoreError> {
        let Some(dir) = self.sync_dir.as_deref() else { return Ok(paperloom_sync::seal::new_salt()) };
        let path = dir.join("seal-salt");
        if let Ok(kept) = fs::read(&path)
            && let Ok(salt) = <[u8; 16]>::try_from(kept.as_slice())
        {
            return Ok(salt);
        }
        let salt = paperloom_sync::seal::new_salt();
        fs::create_dir_all(dir).map_err(|e| local(dir, e))?;
        fs::write(&path, salt).map_err(|e| local(&path, e))?;
        Ok(salt)
    }

    pub(crate) fn seal_with(&self, passphrase: Option<String>, cost: Cost) -> Result<(), CoreError> {
        let sealer = match passphrase.filter(|p| !p.is_empty()) {
            Some(passphrase) => Some(Arc::new(Sealer::new(&passphrase, self.device_salt()?, cost)?)),
            None => None,
        };
        let mut kept = self
            .sealer
            .lock()
            .map_err(|_| CoreError::Storage { message: "Sync is unavailable after a crash.".into() })?;
        *kept = sealer;
        Ok(())
    }

    /// One sync with Paperloom Cloud; `vault`'s notes sync too.
    pub(crate) fn sync_through(
        &self,
        http: &dyn Http,
        service: &str,
        token: &str,
        device: ThisDevice<'_>,
        vault: Option<&PaperVault>,
    ) -> Result<SyncReport, CoreError> {
        let service = service.trim().trim_end_matches('/');
        self.sync_round(service, vault, |state| {
            let mut cloud =
                Cloud { http, base: service.to_string(), token: token.to_string(), library: state.library };
            if !state.joined {
                let device = NewDevice {
                    id: state.sync.device,
                    name: device.name.trim().chars().take(200).collect(),
                    platform: device.platform.trim().chars().take(40).collect(),
                };
                cloud.join(&device)?;
                state.library = cloud.library;
                state.joined = true;
            }
            Ok(cloud)
        })
    }

    /// One sync with an endpoint, named by `key` in the state (another key
    /// starts over: the first sync with an endpoint sends the whole
    /// library). `connect` makes its transport.
    pub(crate) fn sync_round<T, F>(
        &self,
        key: &str,
        vault: Option<&PaperVault>,
        connect: F,
    ) -> Result<SyncReport, CoreError>
    where
        T: Transport + BlobSink,
        F: FnOnce(&mut CloudState) -> Result<T, CoreError>,
    {
        let dir = self.sync_dir.as_deref().ok_or_else(|| CoreError::Storage {
            message: "This library is kept only in memory and cannot sync.".into(),
        })?;
        let files = self.files()?;
        let _one_at_a_time = self.syncing.lock().map_err(|_| CoreError::Storage {
            message: "Sync is unavailable after a crash; restart the app.".into(),
        })?;
        fs::create_dir_all(dir).map_err(|e| local(dir, e))?;
        let (mut state, replica) = match read_state(dir)? {
            Some((state, document)) if state.service == key => {
                let replica = Replica::load(&document, state.sync.peer)?;
                (state, replica)
            }
            _ => {
                self.lock()?.mark_all_changed()?;
                let state = CloudState::new(key);
                let replica = Replica::new(state.sync.peer)?;
                (state, replica)
            }
        };
        let transport = connect(&mut state)?;
        let sealer = self.sealer.lock().ok().and_then(|s| s.clone());
        let sealed = SealedTransport::new(&transport, sealer.as_deref());
        let mut sync = state.sync.clone();
        let mut hooks = CloudHooks { blobs: &transport, sealer: sealer.as_deref(), files, vault, dir, state };
        let report = paperloom_sync::sync(&self.inner, &replica, &mut sync, &sealed, &mut hooks)?;
        self.reindex_synced(&replica, &report.applied);
        let downloaded = self.download_missing(&sealed, &replica, files)?;
        let finished_at = paperloom_store::now_ms();
        hooks.state.last_synced = Some(finished_at);
        hooks.save(&replica, &sync)?;
        Ok(SyncReport {
            sent: report.sent as u32,
            received: report.received as u32,
            changed: report.applied.len() as u32,
            downloaded,
            notes: report.notes.len() as u32,
            finished_at,
        })
    }

    /// Brings the search index up to date with records changed elsewhere.
    fn reindex_synced(&self, replica: &Replica, applied: &[(RecordKind, String)]) {
        let mut items = Vec::new();
        let mut gone_items = Vec::new();
        let mut annotations = Vec::new();
        for (kind, id) in applied {
            let present = replica.get(*kind, id).flatten().is_some();
            match (kind, present) {
                (RecordKind::Item, true) => items.push(ItemId::from_string(id.clone())),
                (RecordKind::Item, false) => gone_items.push(ItemId::from_string(id.clone())),
                (RecordKind::Annotation, true) => annotations.push(AnnotationId::from_string(id.clone())),
                (RecordKind::Annotation, false) => self.unindex_keys(std::slice::from_ref(id)),
                (RecordKind::Attachment, false) => {
                    self.unindex_attachment(&AttachmentId::from_string(id.clone()));
                    if let Some(files) = &self.files {
                        let _ = files.remove_folder(id);
                    }
                }
                _ => {}
            }
        }
        self.reindex_items(&items);
        self.unindex_items(&gone_items);
        for id in annotations {
            let found = self.lock().ok().and_then(|store| store.annotation(&id).ok().flatten());
            if let Some(annotation) = found {
                self.index_annotation(&annotation);
            }
        }
    }

    /// Fetches the files of attachments this device has no file for. One
    /// that cannot be fetched now is tried again after the next sync.
    fn download_missing(
        &self,
        transport: &dyn Transport,
        replica: &Replica,
        files: &FileStore,
    ) -> Result<u32, CoreError> {
        let attachments = self.lock()?.all_attachments()?;
        let mut downloaded = 0;
        for attachment in attachments {
            let Some(path) = files.synced_path(&attachment.path) else { continue };
            if path.exists() {
                continue;
            }
            let blob = replica
                .get(RecordKind::Attachment, attachment.id.as_str())
                .flatten()
                .and_then(|fields| text(&fields, "blob").and_then(|b| b.parse::<BlobHash>().ok()));
            let Some(blob) = blob else { continue };
            let Ok(bytes) = transport.blob(&blob) else { continue };
            if let Some(folder) = path.parent() {
                fs::create_dir_all(folder).map_err(|e| local(folder, e))?;
            }
            write_atomically(&path, &bytes)?;
            downloaded += 1;
        }
        Ok(downloaded)
    }
}

#[cfg(test)]
mod tests;
