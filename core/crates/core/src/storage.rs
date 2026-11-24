//! Libraries sync through storage people bring: a folder other devices also reach
//! (iCloud Drive, a synced Nextcloud, Seafile or Syncthing folder, a NAS
//! share), a WebDAV folder (坚果云, Nextcloud, Synology), or an S3 bucket
//! (AWS, Cloudflare R2, MinIO, 阿里云 OSS, 腾讯云 COS), or a Seafile
//! library through its own Web API v2.
//!
//! No service numbers the log here. Each device writes its own changes as
//! files, `changes/<device>/<seq>.change`, never touched again, and reads
//! every other device's from where it stopped; attached files are
//! `blobs/<first two>/<hash>`. The change bytes are the same as with
//! Paperloom Cloud.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use paperloom_http::{BytesResponse, Http, Request, UreqHttp};
use paperloom_sync::{Positions, SyncError, Transport};
use paperloom_sync_proto::{BlobHash, Change, ChangesPage, LoggedChange, Pushed};
use uuid::Uuid;

use crate::cloud::{BlobSink, SyncReport, local, write_atomically};
use crate::{CoreError, PaperLibrary, PaperVault};

mod s3;
mod seafile;

/// A Seafile library or a folder in it provides storage for sync.
/// The password or account API token is given apart.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SeafilePlace {
    /// The server's address can be `https://cloud.example.com`.
    pub server: String,
    pub account: String,
    /// The Seafile library is found by this name or created if missing.
    pub library: String,
    /// This names a folder inside the library; empty means its top.
    pub folder: String,
}

/// An S3 bucket (or a folder in it) to sync through; the secret key is
/// given apart.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct S3Place {
    /// `https://s3.us-east-1.amazonaws.com`, `https://<account>.r2.cloudflarestorage.com`…
    pub endpoint: String,
    /// `us-east-1`; `auto` for R2. Empty means `us-east-1`.
    pub region: String,
    pub bucket: String,
    /// A folder in the bucket; empty for its top.
    pub prefix: String,
    pub access_key_id: String,
}

/// Files by `/`-separated path under the storage's root.
pub(crate) trait Shelf {
    /// Writes a file, making its folders.
    fn put(&self, path: &str, bytes: Vec<u8>) -> Result<(), SyncError>;
    /// A file's bytes; `None` if there is no such file.
    fn get(&self, path: &str) -> Result<Option<Vec<u8>>, SyncError>;
    fn exists(&self, path: &str) -> Result<bool, SyncError>;
    /// The names in a folder; none if there is no such folder.
    fn list(&self, folder: &str) -> Result<Vec<String>, SyncError>;
}

/// A folder on this device.
pub(crate) struct Folder {
    pub root: PathBuf,
}

impl Shelf for Folder {
    fn put(&self, path: &str, bytes: Vec<u8>) -> Result<(), SyncError> {
        let target = self.root.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| local(parent, e))?;
        }
        write_atomically(&target, &bytes)
    }

    fn get(&self, path: &str) -> Result<Option<Vec<u8>>, SyncError> {
        let target = self.root.join(path);
        match fs::read(&target) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(local(&target, e)),
        }
    }

    fn exists(&self, path: &str) -> Result<bool, SyncError> {
        Ok(self.root.join(path).exists())
    }

    fn list(&self, folder: &str) -> Result<Vec<String>, SyncError> {
        let target = self.root.join(folder);
        let entries = match fs::read_dir(&target) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(local(&target, e)),
        };
        Ok(entries.filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned())).collect())
    }
}

/// A WebDAV folder, signed in with a user name and (app) password.
pub(crate) struct Dav<'a> {
    pub http: &'a dyn Http,
    /// The folder's address, without a trailing `/`.
    pub base: String,
    pub authorization: String,
    /// Folders known to exist.
    pub made: Mutex<BTreeSet<String>>,
}

fn failure(method: &str, path: &str, response: &BytesResponse) -> SyncError {
    SyncError::Transport(format!("WebDAV {method} {path} answered HTTP {}.", response.status))
}

impl Dav<'_> {
    fn url(&self, path: &str) -> String {
        if path.is_empty() { format!("{}/", self.base) } else { format!("{}/{path}", self.base) }
    }

    fn request(
        &self,
        method: &'static str,
        url: String,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    ) -> Result<BytesResponse, SyncError> {
        let mut all = vec![("Authorization".to_string(), self.authorization.clone())];
        all.extend(headers);
        let response = self
            .http
            .send(Request { method, url, headers: all, body })
            .map_err(|e| SyncError::Transport(e.0))?;
        if response.status == 401 || response.status == 403 {
            return Err(SyncError::Transport(
                "The WebDAV server did not accept the user name or password.".into(),
            ));
        }
        Ok(response)
    }

    /// Makes the root folder and each folder on the way to `path` (a file's
    /// path), once.
    fn make_folders(&self, path: &str) -> Result<(), SyncError> {
        let parts: Vec<&str> = path.split('/').collect();
        for end in 0..parts.len() {
            let folder = parts[..end].join("/");
            if self.made.lock().map_err(|e| SyncError::Local(e.to_string()))?.contains(&folder) {
                continue;
            }
            let url = if folder.is_empty() {
                format!("{}/", self.base)
            } else {
                format!("{}/{folder}/", self.base)
            };
            let response = self.request("MKCOL", url, Vec::new(), None)?;
            // 405: it is there already.
            if !(response.is_success() || response.status == 405) {
                return Err(failure("MKCOL", &folder, &response));
            }
            self.made.lock().map_err(|e| SyncError::Local(e.to_string()))?.insert(folder);
        }
        Ok(())
    }
}

impl Shelf for Dav<'_> {
    fn put(&self, path: &str, bytes: Vec<u8>) -> Result<(), SyncError> {
        self.make_folders(path)?;
        let headers = vec![("Content-Type".to_string(), "application/octet-stream".to_string())];
        let response = self.request("PUT", self.url(path), headers, Some(bytes))?;
        if !response.is_success() {
            return Err(failure("PUT", path, &response));
        }
        Ok(())
    }

    fn get(&self, path: &str) -> Result<Option<Vec<u8>>, SyncError> {
        let response = self.request("GET", self.url(path), Vec::new(), None)?;
        match response.status {
            404 => Ok(None),
            _ if response.is_success() => Ok(Some(response.body)),
            _ => Err(failure("GET", path, &response)),
        }
    }

    fn exists(&self, path: &str) -> Result<bool, SyncError> {
        let response = self.request("HEAD", self.url(path), Vec::new(), None)?;
        match response.status {
            404 => Ok(false),
            _ if response.is_success() => Ok(true),
            _ => Err(failure("HEAD", path, &response)),
        }
    }

    fn list(&self, folder: &str) -> Result<Vec<String>, SyncError> {
        let url = format!("{}/{folder}/", self.base);
        let headers = vec![
            ("Depth".to_string(), "1".to_string()),
            ("Content-Type".to_string(), "application/xml; charset=utf-8".to_string()),
        ];
        let body = br#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/></d:prop></d:propfind>"#;
        let response = self.request("PROPFIND", url.clone(), headers, Some(body.to_vec()))?;
        if response.status == 404 {
            return Ok(Vec::new());
        }
        if !response.is_success() {
            return Err(failure("PROPFIND", folder, &response));
        }
        let own = url_path(&url);
        Ok(hrefs(&String::from_utf8_lossy(&response.body))
            .into_iter()
            .map(|href| url_path(&href))
            .filter(|path| *path != own)
            .filter_map(|path| path.rsplit('/').next().map(str::to_string))
            .filter(|name| !name.is_empty())
            .collect())
    }
}

/// The `href`s of a PROPFIND answer, whatever prefix the server gives DAV.
fn hrefs(xml: &str) -> Vec<String> {
    let mut found = Vec::new();
    for piece in xml.split('<').skip(1) {
        let Some((tag, text)) = piece.split_once('>') else { continue };
        let name = tag.split_whitespace().next().unwrap_or_default();
        if !name.starts_with('/') && name.rsplit(':').next().is_some_and(|n| n.eq_ignore_ascii_case("href")) {
            found.push(text.trim().to_string());
        }
    }
    found
}

/// An address's path, decoded, without a trailing `/`.
fn url_path(address: &str) -> String {
    let path = match address.split_once("://") {
        Some((_, rest)) => rest.find('/').map_or("", |at| &rest[at..]),
        None => address,
    };
    percent_decode(path.trim_end_matches('/'))
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[at] == b'%'
            && let (Some(&high), Some(&low)) = (bytes.get(at + 1), bytes.get(at + 2))
            && let (Some(high), Some(low)) = (hex(high), hex(low))
        {
            out.push((high * 16 + low) as u8);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn change_path(device: Uuid, seq: u64) -> String {
    format!("changes/{device}/{seq:020}.change")
}

fn blob_path(hash: &str) -> String {
    let start = hash.get(..2).unwrap_or("00");
    format!("blobs/{start}/{hash}")
}

/// The sync transport over storage people bring.
pub(crate) struct ShelfTransport<S: Shelf> {
    pub shelf: S,
}

impl<S: Shelf> Transport for ShelfTransport<S> {
    fn push(&self, changes: &[Change]) -> Result<Pushed, SyncError> {
        for change in changes {
            self.shelf.put(&change_path(change.device, change.seq), change.payload.clone())?;
        }
        Ok(Pushed { head: 0, cursors: vec![0; changes.len()] })
    }

    fn pull(&self, _after: u64, positions: &Positions) -> Result<ChangesPage, SyncError> {
        let mut changes = Vec::new();
        for name in self.shelf.list("changes")? {
            let Ok(device) = name.parse::<Uuid>() else { continue };
            let seen = positions.get(&device).copied().unwrap_or(0);
            let mut seqs: Vec<u64> = self
                .shelf
                .list(&format!("changes/{device}"))?
                .iter()
                .filter_map(|file| file.strip_suffix(".change")?.parse().ok())
                .filter(|&seq| seq > seen)
                .collect();
            seqs.sort_unstable();
            for seq in seqs {
                // A file still arriving (a synced folder) comes next time.
                let Some(payload) = self.shelf.get(&change_path(device, seq))? else { continue };
                changes.push(LoggedChange { cursor: 0, device, seq, payload });
            }
        }
        Ok(ChangesPage { changes, head: 0, more: false, snapshot: None })
    }

    fn blob(&self, hash: &BlobHash) -> Result<Vec<u8>, SyncError> {
        let bytes = self
            .shelf
            .get(&blob_path(hash.as_str()))?
            .ok_or_else(|| SyncError::Transport(format!("blob {hash} is not there yet")))?;
        if BlobHash::of(&bytes) != *hash {
            return Err(SyncError::Corrupt(format!("blob {hash} arrived damaged")));
        }
        Ok(bytes)
    }
}

impl<S: Shelf> BlobSink for ShelfTransport<S> {
    fn put_blob(&self, hash: &str, bytes: Vec<u8>) -> Result<(), SyncError> {
        let path = blob_path(hash);
        if self.shelf.exists(&path)? {
            return Ok(());
        }
        self.shelf.put(&path, bytes)
    }
}

#[uniffi::export]
impl PaperLibrary {
    /// Syncs through a folder other devices also reach: iCloud Drive, a
    /// synced Nextcloud, Seafile or Syncthing folder, a NAS share. Each
    /// library needs a folder of its own. `vault`'s notes sync too.
    pub fn sync_with_folder(
        &self,
        folder: String,
        vault: Option<Arc<PaperVault>>,
    ) -> Result<SyncReport, CoreError> {
        let folder = folder.trim().to_string();
        if folder.is_empty() {
            return Err(CoreError::InvalidInput { message: "Choose a folder to sync through.".into() });
        }
        let key = format!("folder:{folder}");
        self.sync_round(&key, vault.as_deref(), |_| {
            Ok(ShelfTransport { shelf: Folder { root: PathBuf::from(&folder) } })
        })
    }

    /// Syncs through a WebDAV folder (坚果云, Nextcloud, Synology…), signed
    /// in with a user name and password (an app password where the server
    /// has them). Blocks on the network: call it off the main thread.
    pub fn sync_with_webdav(
        &self,
        address: String,
        username: String,
        password: String,
        vault: Option<Arc<PaperVault>>,
    ) -> Result<SyncReport, CoreError> {
        self.sync_webdav_through(&UreqHttp::new(), &address, &username, &password, vault.as_deref())
    }

    /// Syncs through an S3 bucket (or a folder in it). The bucket must exist.
    pub fn sync_with_s3(
        &self,
        place: S3Place,
        secret_key: String,
        vault: Option<Arc<PaperVault>>,
    ) -> Result<SyncReport, CoreError> {
        self.sync_s3_through(&UreqHttp::new(), &place, &secret_key, vault.as_deref())
    }

    /// Syncs through Seafile's Web API v2. A password consisting of exactly
    /// 40 ASCII hexadecimal characters is used as an account API token;
    /// every other password is exchanged for a token by signing in.
    /// Blocks on the network: call it off the main thread.
    pub fn sync_with_seafile(
        &self,
        place: SeafilePlace,
        password: String,
        vault: Option<Arc<PaperVault>>,
    ) -> Result<SyncReport, CoreError> {
        self.sync_seafile_through(&UreqHttp::new(), &place, &password, vault.as_deref())
    }
}

impl PaperLibrary {
    pub(crate) fn sync_webdav_through(
        &self,
        http: &dyn Http,
        address: &str,
        username: &str,
        password: &str,
        vault: Option<&PaperVault>,
    ) -> Result<SyncReport, CoreError> {
        let base = address.trim().trim_end_matches('/').to_string();
        if !(base.starts_with("https://") || base.starts_with("http://")) {
            return Err(CoreError::InvalidInput { message: "A WebDAV address starts with https://.".into() });
        }
        let authorization = format!("Basic {}", STANDARD.encode(format!("{username}:{password}")));
        let key = format!("webdav:{base}");
        self.sync_round(&key, vault, |_| {
            Ok(ShelfTransport { shelf: Dav { http, base, authorization, made: Mutex::default() } })
        })
    }
}

impl PaperLibrary {
    pub(crate) fn sync_seafile_through(
        &self,
        http: &dyn Http,
        place: &SeafilePlace,
        password: &str,
        vault: Option<&PaperVault>,
    ) -> Result<SyncReport, CoreError> {
        let server = place.server.trim().trim_end_matches('/');
        if !(server.starts_with("https://") || server.starts_with("http://")) {
            return Err(CoreError::InvalidInput {
                message: "A Seafile address starts with https://.".into(),
            });
        }
        if place.account.trim().is_empty() || place.library.trim().is_empty() || password.is_empty() {
            return Err(CoreError::InvalidInput {
                message: "Seafile needs an account, a library name and a password or API token.".into(),
            });
        }
        let folder = place.folder.trim().trim_matches('/');
        if folder.split('/').any(|part| part == "." || part == "..") {
            return Err(CoreError::InvalidInput {
                message: "A Seafile folder cannot contain . or .. path components.".into(),
            });
        }
        let place = SeafilePlace {
            server: server.to_string(),
            account: place.account.trim().to_string(),
            library: place.library.trim().to_string(),
            folder: folder.split('/').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("/"),
        };
        let key = format!("seafile:{}/{}/{}", place.server, place.library, place.folder);
        self.sync_round(&key, vault, |_| {
            Ok(ShelfTransport { shelf: seafile::Seafile::connect(http, &place, password)? })
        })
    }

    pub(crate) fn sync_s3_through(
        &self,
        http: &dyn Http,
        place: &S3Place,
        secret_key: &str,
        vault: Option<&PaperVault>,
    ) -> Result<SyncReport, CoreError> {
        let endpoint = place.endpoint.trim().trim_end_matches('/').to_string();
        if !(endpoint.starts_with("https://") || endpoint.starts_with("http://")) {
            return Err(CoreError::InvalidInput { message: "An S3 address starts with https://.".into() });
        }
        let missing = [&place.bucket, &place.access_key_id].iter().any(|v| v.trim().is_empty());
        if missing || secret_key.trim().is_empty() {
            return Err(CoreError::InvalidInput {
                message: "S3 needs a bucket, an access key ID and a secret.".into(),
            });
        }
        let region = match place.region.trim() {
            "" => "us-east-1".to_string(),
            region => region.to_string(),
        };
        let prefix = place.prefix.trim().trim_matches('/').to_string();
        let key = format!("s3:{endpoint}/{}/{prefix}", place.bucket.trim());
        self.sync_round(&key, vault, |_| {
            Ok(ShelfTransport {
                shelf: s3::S3 {
                    http,
                    endpoint,
                    region,
                    bucket: place.bucket.trim().to_string(),
                    prefix,
                    access_key: place.access_key_id.trim().to_string(),
                    secret_key: secret_key.trim().to_string(),
                },
            })
        })
    }
}

#[cfg(test)]
mod tests;
