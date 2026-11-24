//! What the app keeps secret on this device (the WebDAV password, the AI
//! key, keys for the databases), in `secrets.json` in its private data
//! folder, sealed for this user (see `private`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::CommandResult;
use crate::private::{read_private, write_private};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Kept {
    pub webdav_password: Option<String>,
    pub ai_key: Option<String>,
    #[serde(default)]
    pub s3_secret: Option<String>,
    #[serde(default)]
    pub seafile_password: Option<String>,
    /// By search source ID.
    #[serde(default)]
    pub database_keys: BTreeMap<String, String>,
    /// Seals what sync sends, end to end.
    #[serde(default)]
    pub sync_passphrase: Option<String>,
}

pub(crate) struct Secrets {
    file: PathBuf,
    /// One change at a time.
    writing: Mutex<()>,
}

impl Secrets {
    pub(crate) fn new(data_dir: &Path) -> Self {
        Self { file: data_dir.join("secrets.json"), writing: Mutex::new(()) }
    }

    pub(crate) fn load(&self) -> Kept {
        read_private(&self.file).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub(crate) fn update(&self, change: impl FnOnce(&mut Kept)) -> CommandResult<()> {
        let _one_at_a_time = self.writing.lock().map_err(|e| e.to_string())?;
        let mut kept = self.load();
        change(&mut kept);
        let text = serde_json::to_string(&kept).map_err(|e| e.to_string())?;
        write_private(&self.file, &text).map_err(|e| e.to_string())
    }
}
