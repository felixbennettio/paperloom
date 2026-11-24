//! What devices and the Paperloom sync service send each other (API v1).
//!
//! A library's history is a log of opaque changes. Each device appends its
//! own, numbered 1, 2, 3… (`seq`); the service gives every change it keeps
//! a place in the library's log (`cursor`, from 1) and hands the log out in
//! that order. Pushing a change again is harmless: the service answers with
//! the place it already has. Attachments are blobs named by the BLAKE3 hash
//! of their bytes. The service never looks inside a change or a blob, so
//! they can be end-to-end encrypted.
//!
//! JSON throughout; bytes are base64 (standard alphabet, padded).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The path prefix of this version of the API.
pub const API: &str = "/v1";

/// The scope reading needs.
pub const SCOPE_READ: &str = "sync:read";
/// The scope changing anything needs.
pub const SCOPE_WRITE: &str = "sync:write";

/// The most changes one push or one page holds.
pub const MAX_BATCH: usize = 500;

/// A blob's name: the BLAKE3 hash of its bytes, 64 lowercase hex digits.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BlobHash(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a BLAKE3 hash (64 lowercase hex digits): {0}")]
pub struct BadHash(pub String);

impl BlobHash {
    pub fn of(bytes: &[u8]) -> Self {
        Self(blake3::hash(bytes).to_hex().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for BlobHash {
    type Err = BadHash;

    fn from_str(s: &str) -> Result<Self, BadHash> {
        if s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            Ok(Self(s.to_string()))
        } else {
            Err(BadHash(s.chars().take(80).collect()))
        }
    }
}

impl TryFrom<String> for BlobHash {
    type Error = BadHash;

    fn try_from(s: String) -> Result<Self, BadHash> {
        s.parse()
    }
}

impl From<BlobHash> for String {
    fn from(hash: BlobHash) -> Self {
        hash.0
    }
}

impl fmt::Display for BlobHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `GET /v1/me`: who the token belongs to and what they use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    /// The account's subject at the identity provider.
    pub sub: String,
    /// `free` for now; subscriptions come later.
    pub plan: String,
    pub devices: Vec<Device>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub libraries: u32,
    /// Bytes of changes kept, in libraries the account owns.
    pub change_bytes: u64,
    pub blob_bytes: u64,
}

/// `POST /v1/devices`. The device makes its ID once and keeps it, so
/// registering again changes only the name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewDevice {
    pub id: Uuid,
    pub name: String,
    /// `macos`, `ios`, `windows`, `linux`, `android`.
    pub platform: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: Uuid,
    pub name: String,
    pub platform: String,
    /// Milliseconds since 1970.
    pub created_at: i64,
}

/// What an account may do in a library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Owner,
    Editor,
    Viewer,
}

impl Role {
    pub fn can_write(self) -> bool {
        matches!(self, Role::Owner | Role::Editor)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Editor => "editor",
            Role::Viewer => "viewer",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "owner" => Some(Role::Owner),
            "editor" => Some(Role::Editor),
            "viewer" => Some(Role::Viewer),
            _ => None,
        }
    }
}

/// `POST /v1/libraries`. A library made offline already has its ID;
/// creating it again returns it unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewLibrary {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub id: Uuid,
    pub name: String,
    pub role: Role,
    /// The cursor of the newest change; 0 for none.
    pub head: u64,
    /// Milliseconds since 1970.
    pub created_at: i64,
}

/// One device's change, as pushed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub device: Uuid,
    /// From 1, one more than the device's previous change in this library.
    pub seq: u64,
    #[serde(with = "base64_bytes")]
    pub payload: Vec<u8>,
}

/// `POST /v1/libraries/{id}/changes`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Push {
    pub changes: Vec<Change>,
}

/// Where each pushed change is in the log (also for ones sent before).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pushed {
    pub head: u64,
    pub cursors: Vec<u64>,
}

/// A change in the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoggedChange {
    pub cursor: u64,
    pub device: Uuid,
    pub seq: u64,
    #[serde(with = "base64_bytes")]
    pub payload: Vec<u8>,
}

/// `GET /v1/libraries/{id}/changes?after=<cursor>&limit=<n>`: the changes
/// after a cursor, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangesPage {
    pub changes: Vec<LoggedChange>,
    pub head: u64,
    /// More changes follow the last one here.
    pub more: bool,
    /// Set when changes after `after` were folded into a snapshot: load it,
    /// then ask again after its cursor.
    pub snapshot: Option<Snapshot>,
}

/// The whole library up to a cursor, in one blob. `POST
/// /v1/libraries/{id}/snapshot` sets it; the changes it covers are dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub cursor: u64,
    pub blob: BlobHash,
}

/// A server-sent event on `GET /v1/libraries/{id}/events`: the log grew.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadEvent {
    pub head: u64,
}

/// Every error answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
    /// Stable, for programs: `unauthorized`, `forbidden`, `not_found`,
    /// `conflict`, `bad_request`, `too_large`, `internal`.
    pub error: String,
    /// For people.
    pub message: String,
    /// With `conflict` on a push: the seq the device has to send next.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_seq: Option<u64>,
}

mod base64_bytes {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        STANDARD.decode(text.as_bytes()).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_hashes_are_checked() {
        let hash = BlobHash::of(b"paper");
        assert_eq!(hash.as_str().len(), 64);
        assert_eq!(hash.as_str().parse::<BlobHash>().unwrap(), hash);
        assert!("ABC".parse::<BlobHash>().is_err());
        assert!(hash.as_str().to_uppercase().parse::<BlobHash>().is_err());
        let json = serde_json::to_string(&hash).unwrap();
        assert_eq!(serde_json::from_str::<BlobHash>(&json).unwrap(), hash);
        assert!(serde_json::from_str::<BlobHash>("\"nope\"").is_err());
    }

    #[test]
    fn changes_carry_bytes_as_base64() {
        let change = Change { device: Uuid::nil(), seq: 1, payload: vec![0, 1, 2, 255] };
        let json = serde_json::to_value(&change).unwrap();
        assert_eq!(json["payload"], "AAEC/w==");
        assert_eq!(json["seq"], 1);
        assert_eq!(serde_json::from_value::<Change>(json).unwrap(), change);
    }

    #[test]
    fn errors_leave_out_what_they_lack() {
        let body = ErrorBody { error: "forbidden".into(), message: "No.".into(), expected_seq: None };
        assert_eq!(serde_json::to_string(&body).unwrap(), r#"{"error":"forbidden","message":"No."}"#);
        let role: Role = serde_json::from_str("\"editor\"").unwrap();
        assert!(role.can_write() && !Role::Viewer.can_write());
        assert_eq!(Role::parse(role.as_str()), Some(role));
    }
}
