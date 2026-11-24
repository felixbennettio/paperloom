//! End-to-end encryption: changes and files leave the device sealed with
//! XChaCha20-Poly1305 under a key made from the person's passphrase with
//! Argon2id, so the service and the storage hold only ciphertext.
//!
//! Sealed bytes say how to open them: `PLE1`, the Argon2id memory (KiB,
//! u32 little-endian) and passes (u8), the 16-byte salt, the 24-byte nonce,
//! then the ciphertext; everything before the nonce is authenticated too.
//! Each device makes its key with its own salt; another device opens what
//! it sealed with the same passphrase and that salt. A file is sealed with
//! a nonce made from its contents, so it seals the same way every time and
//! keeps its blob name.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Mutex;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use paperloom_sync_proto::{BlobHash, Change, ChangesPage, Pushed};

use crate::{Positions, SyncError, Transport};

const MAGIC: &[u8; 4] = b"PLE1";
const SALT: usize = 16;
const NONCE: usize = 24;
/// Magic, memory, passes and salt: what a key is made from.
const HEADER: usize = 4 + 4 + 1 + SALT;

/// What making a key costs: memory in KiB and passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cost {
    pub memory_kib: u32,
    pub passes: u8,
}

impl Cost {
    /// 64 MiB and three passes: under a second on a phone.
    pub const DEFAULT: Cost = Cost { memory_kib: 64 * 1024, passes: 3 };
    /// The most another device's header may ask for, so damaged or hostile
    /// bytes cannot take all the memory.
    const MOST: Cost = Cost { memory_kib: 1024 * 1024, passes: 10 };
}

/// A device's salt: random, made once and kept.
pub fn new_salt() -> [u8; SALT] {
    let mut salt = [0u8; SALT];
    OsRng.fill_bytes(&mut salt);
    salt
}

/// Whether bytes are sealed (rather than a plain change or file).
pub fn is_sealed(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

struct Key {
    cipher: XChaCha20Poly1305,
    /// Makes a file's nonce from its contents.
    nonces: [u8; 32],
}

impl Key {
    fn make(passphrase: &str, salt: &[u8], cost: Cost) -> Result<Key, SyncError> {
        let params = Params::new(cost.memory_kib, u32::from(cost.passes), 1, Some(32))
            .map_err(|e| SyncError::Corrupt(format!("key settings: {e}")))?;
        let mut master = [0u8; 32];
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(passphrase.as_bytes(), salt, &mut master)
            .map_err(|e| SyncError::Corrupt(format!("making the key: {e}")))?;
        let cipher = XChaCha20Poly1305::new(&blake3::derive_key("paperloom 2026 seal key", &master).into());
        let nonces = blake3::derive_key("paperloom 2026 file nonce key", &master);
        Ok(Key { cipher, nonces })
    }
}

/// Seals what this device sends and opens what any device sealed with the
/// same passphrase.
pub struct Sealer {
    passphrase: String,
    header: [u8; HEADER],
    own: Key,
    /// Keys of other salts (other devices), made once.
    others: Mutex<HashMap<[u8; HEADER], std::sync::Arc<Key>>>,
}

fn header(salt: &[u8; SALT], cost: Cost) -> [u8; HEADER] {
    let mut header = [0u8; HEADER];
    header[..4].copy_from_slice(MAGIC);
    header[4..8].copy_from_slice(&cost.memory_kib.to_le_bytes());
    header[8] = cost.passes;
    header[9..].copy_from_slice(salt);
    header
}

impl Sealer {
    /// Makes this device's key: slow on purpose (see [`Cost`]).
    pub fn new(passphrase: &str, salt: [u8; SALT], cost: Cost) -> Result<Sealer, SyncError> {
        if passphrase.is_empty() {
            return Err(SyncError::Local("The passphrase is empty.".into()));
        }
        Ok(Sealer {
            passphrase: passphrase.to_string(),
            header: header(&salt, cost),
            own: Key::make(passphrase, &salt, cost)?,
            others: Mutex::new(HashMap::new()),
        })
    }

    fn seal_with(&self, plain: &[u8], nonce: &XNonce) -> Vec<u8> {
        let sealed = self
            .own
            .cipher
            .encrypt(nonce, Payload { msg: plain, aad: &self.header })
            .unwrap_or_else(|_| unreachable!("XChaCha20-Poly1305 seals any length that fits in memory"));
        let mut out = Vec::with_capacity(HEADER + NONCE + sealed.len());
        out.extend_from_slice(&self.header);
        out.extend_from_slice(nonce);
        out.extend_from_slice(&sealed);
        out
    }

    /// Seals with a random nonce (changes).
    pub fn seal(&self, plain: &[u8]) -> Vec<u8> {
        self.seal_with(plain, &XChaCha20Poly1305::generate_nonce(&mut OsRng))
    }

    /// Seals with a nonce made from the contents, so the same file always
    /// seals to the same bytes.
    pub fn seal_file(&self, plain: &[u8]) -> Vec<u8> {
        let made = blake3::keyed_hash(&self.own.nonces, plain);
        self.seal_with(plain, XNonce::from_slice(&made.as_bytes()[..NONCE]))
    }

    /// Opens sealed bytes from any device that used the same passphrase.
    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>, SyncError> {
        if sealed.len() < HEADER + NONCE || !is_sealed(sealed) {
            return Err(SyncError::Corrupt("sealed bytes are cut short".into()));
        }
        let head: [u8; HEADER] = sealed[..HEADER].try_into().unwrap_or([0; HEADER]);
        let nonce = XNonce::from_slice(&sealed[HEADER..HEADER + NONCE]);
        let payload = Payload { msg: &sealed[HEADER + NONCE..], aad: &head };
        let opened = if head == self.header {
            self.own.cipher.decrypt(nonce, payload)
        } else {
            self.key_for(&head)?.cipher.decrypt(nonce, payload)
        };
        opened.map_err(|_| SyncError::Passphrase)
    }

    fn key_for(&self, head: &[u8; HEADER]) -> Result<std::sync::Arc<Key>, SyncError> {
        let mut others = self.others.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(key) = others.get(head) {
            return Ok(key.clone());
        }
        let cost =
            Cost { memory_kib: u32::from_le_bytes([head[4], head[5], head[6], head[7]]), passes: head[8] };
        if cost.memory_kib > Cost::MOST.memory_kib || cost.passes > Cost::MOST.passes || cost.passes == 0 {
            return Err(SyncError::Corrupt("sealed bytes ask for an unreasonable key".into()));
        }
        let key = std::sync::Arc::new(Key::make(&self.passphrase, &head[9..], cost)?);
        others.insert(*head, key.clone());
        Ok(key)
    }
}

/// A transport that seals this device's changes on the way out and opens
/// sealed changes and blobs on the way in. Without a sealer, it passes
/// plain bytes and stops at sealed ones. Either way, a round reads the log
/// before it sends.
pub struct SealedTransport<'a> {
    inner: &'a dyn Transport,
    sealer: Option<&'a Sealer>,
    /// The log has been read to its end, or the passphrase has opened
    /// something from it: changes may go out.
    checked: Cell<bool>,
}

impl<'a> SealedTransport<'a> {
    pub fn new(inner: &'a dyn Transport, sealer: Option<&'a Sealer>) -> Self {
        SealedTransport { inner, sealer, checked: Cell::new(false) }
    }

    fn open(&self, bytes: Vec<u8>) -> Result<Vec<u8>, SyncError> {
        if !is_sealed(&bytes) {
            return Ok(bytes);
        }
        let opened = self.sealer.ok_or(SyncError::Locked)?.open(&bytes)?;
        self.checked.set(true);
        Ok(opened)
    }
}

impl Transport for SealedTransport<'_> {
    fn push(&self, changes: &[Change]) -> Result<Pushed, SyncError> {
        match self.sealer {
            Some(sealer) => {
                let sealed: Vec<Change> = changes
                    .iter()
                    .map(|c| Change { device: c.device, seq: c.seq, payload: sealer.seal(&c.payload) })
                    .collect();
                self.inner.push(&sealed)
            }
            None => self.inner.push(changes),
        }
    }

    fn pull(&self, after: u64, positions: &Positions) -> Result<ChangesPage, SyncError> {
        let mut page = self.inner.pull(after, positions)?;
        for change in &mut page.changes {
            change.payload = self.open(std::mem::take(&mut change.payload))?;
        }
        if !page.more {
            self.checked.set(true);
        }
        Ok(page)
    }

    fn blob(&self, hash: &BlobHash) -> Result<Vec<u8>, SyncError> {
        self.open(self.inner.blob(hash)?)
    }

    /// Changes wait until the log has been read: so a mistyped passphrase
    /// never sends changes other devices cannot open, and a device without
    /// one never sends plain changes into a sealed library.
    fn ready_to_push(&self) -> bool {
        self.checked.get()
    }
}
