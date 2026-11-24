//! Earlier versions of notes, kept in the vault's `.paperloom/history`
//! folder (out of sight: the vault, like Obsidian, skips dot folders).
//! Before a save changes a note, the note as it was is kept, unless a
//! version was kept in the last five minutes; the last 100 stay.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Error, Result, Vault, io_error};

/// At most one version in this long, while someone types.
const GAP_MS: i64 = 5 * 60 * 1000;
const KEEP: usize = 100;

/// A kept version and its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// When it was kept, in milliseconds since 1970: the note read like
    /// this until then.
    pub time: i64,
    pub size: u64,
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// FNV-1a: a short, stable folder name for a note's path.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, b| (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

impl Vault {
    fn history_of(&self, path: &str) -> PathBuf {
        self.root().join(".paperloom").join("history").join(format!("{:016x}", fnv(&path.to_lowercase())))
    }

    /// Keeps `text`, the note as it is `now`, unless a version was kept
    /// lately (`force` keeps it anyway) or it is the same as the last. Keeping
    /// is a courtesy: a failure here never stops a save.
    pub(crate) fn keep_version(&self, path: &str, text: &str, now: i64, force: bool) {
        let folder = self.history_of(path);
        let versions = self.versions(path);
        if let Some(latest) = versions.first() {
            if !force && latest.time > now - GAP_MS {
                return;
            }
            let same = latest.size == text.len() as u64
                && fs::read_to_string(folder.join(format!("{}.md", latest.time))).is_ok_and(|t| t == text);
            if same {
                return;
            }
        }
        if text.is_empty() || fs::create_dir_all(&folder).is_err() {
            return;
        }
        let time = versions.first().map_or(now, |latest| now.max(latest.time + 1));
        let _ = fs::write(folder.join(format!("{time}.md")), text);
        let _ = fs::write(folder.join("path.txt"), path);
        for old in versions.iter().skip(KEEP - 1) {
            let _ = fs::remove_file(folder.join(format!("{}.md", old.time)));
        }
    }

    /// A note's kept versions, newest first.
    pub fn versions(&self, path: &str) -> Vec<Version> {
        let Ok(entries) = fs::read_dir(self.history_of(path)) else { return Vec::new() };
        let mut versions: Vec<Version> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                let time = name.strip_suffix(".md")?.parse().ok()?;
                Some(Version { time, size: entry.metadata().map_or(0, |m| m.len()) })
            })
            .collect();
        versions.sort_by_key(|v| std::cmp::Reverse(v.time));
        versions
    }

    /// A kept version's text.
    pub fn version(&self, path: &str, time: i64) -> Result<String> {
        let file = self.history_of(path).join(format!("{time}.md"));
        if !file.exists() {
            return Err(Error::NotFound(format!("{path} as of {time}")));
        }
        fs::read_to_string(&file).map_err(|e| io_error(&file, e))
    }

    /// Puts a kept version back; the note as it is now is kept first, so
    /// going back can be undone.
    pub fn restore(&mut self, path: &str, time: i64) -> Result<()> {
        let text = self.version(path, time)?;
        let note = self.note(path).ok_or_else(|| Error::NotFound(path.to_string()))?;
        self.keep_version(path, &note.text, now_ms(), true);
        self.write(path, &text)
    }

    /// History follows a renamed note.
    pub(crate) fn move_history(&self, from: &str, to: &str) {
        let (old, new) = (self.history_of(from), self.history_of(to));
        if old != new && old.exists() && !new.exists() && fs::rename(&old, &new).is_ok() {
            let _ = fs::write(new.join("path.txt"), to);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_versions_now_and_then() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::open(dir.path()).unwrap();
        vault.write("Note.md", "one").unwrap();
        let later = 10_000_000 + GAP_MS + 1;
        vault.keep_version("Note.md", "one", 10_000_000, false);
        vault.keep_version("Note.md", "two", 10_000_000 + 60_000, false);
        assert_eq!(vault.versions("Note.md").len(), 1, "a minute later is too soon");
        vault.keep_version("Note.md", "one", later, false);
        assert_eq!(vault.versions("Note.md").len(), 1, "the same text is kept once");
        vault.keep_version("Note.md", "three", later, false);
        let versions = vault.versions("Note.md");
        assert_eq!(versions.iter().map(|v| v.time).collect::<Vec<_>>(), [later, 10_000_000]);
        assert_eq!(vault.version("Note.md", later).unwrap(), "three");

        vault.restore("Note.md", 10_000_000).unwrap();
        assert_eq!(vault.note("Note.md").unwrap().text, "one");
        assert_eq!(vault.versions("Note.md").len(), 3, "what was there before going back is kept");

        vault.rename("Note.md", "Renamed.md").unwrap();
        assert_eq!(vault.versions("Renamed.md").len(), 3);
        assert!(vault.version("Renamed.md", 5).is_err());
    }

    #[test]
    fn saves_keep_the_note_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::open(dir.path()).unwrap();
        vault.write("Note.md", "first").unwrap();
        vault.write("Note.md", "second").unwrap();
        vault.write("Note.md", "third").unwrap();
        let versions = vault.versions("Note.md");
        assert_eq!(versions.len(), 1, "one version while typing");
        assert_eq!(vault.version("Note.md", versions[0].time).unwrap(), "first");
    }
}
