//! Files attached to items. They live in the library's file store, a
//! `files` folder next to the database with one sub-folder per attachment,
//! so names never clash and sync can move each one on its own.

use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use paperloom_model::{Attachment, AttachmentId, ItemId, content_type_for};
use paperloom_store as store;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::CoreError;

/// An attached file as the apps show and open it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    pub id: String,
    pub item_id: String,
    pub title: String,
    /// Where the file is on this device.
    pub file_path: String,
    pub content_type: String,
    pub size: u64,
    pub is_pdf: bool,
}

pub(crate) struct FileStore {
    root: PathBuf,
}

impl FileStore {
    /// The store beside a library database file.
    pub(crate) fn beside(database: &Path) -> Self {
        let folder = database.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        Self { root: folder.join("files") }
    }

    pub(crate) fn path_of(&self, attachment: &Attachment) -> PathBuf {
        self.path_for(&attachment.path)
    }

    /// Where a file the library names `<attachment id>/<file name>` is.
    pub(crate) fn path_for(&self, relative: &str) -> PathBuf {
        relative.split('/').fold(self.root.clone(), |path, part| path.join(part))
    }

    /// Like [`FileStore::path_for`], for a path that came from elsewhere
    /// (sync): only `<folder>/<name>` inside the store, nothing that climbs
    /// out of it.
    pub(crate) fn synced_path(&self, relative: &str) -> Option<PathBuf> {
        let (folder, name) = relative.split_once('/')?;
        (plain(folder) && plain(name)).then(|| self.root.join(folder).join(name))
    }

    pub(crate) fn info(&self, attachment: &Attachment) -> AttachmentInfo {
        AttachmentInfo {
            id: attachment.id.to_string(),
            item_id: attachment.item_id.to_string(),
            title: attachment.title.clone(),
            file_path: self.path_of(attachment).display().to_string(),
            content_type: attachment.content_type.clone(),
            size: attachment.size,
            is_pdf: attachment.is_pdf(),
        }
    }

    /// The folder the files are kept in.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Copies a file into the store for an item. When the item already has
    /// a file with the same content, that one is returned instead.
    pub(crate) fn attach(
        &self,
        library: &mut store::Library,
        item: &ItemId,
        source: &Path,
    ) -> Result<Attachment, CoreError> {
        let sha256 = sha256_of(source).map_err(|e| io_error(source, e))?;
        if let Some(existing) = library.find_attachment(item, &sha256)? {
            return Ok(existing);
        }
        let file_name = clean_file_name(&source.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
        let id = AttachmentId::new();
        let folder = self.root.join(id.as_str());
        fs::create_dir_all(&folder).map_err(|e| io_error(&folder, e))?;
        let target = folder.join(&file_name);
        let size = match fs::copy(source, &target) {
            Ok(size) => size,
            Err(e) => {
                let _ = fs::remove_dir_all(&folder);
                return Err(io_error(source, e));
            }
        };
        let attachment = Attachment {
            path: format!("{id}/{file_name}"),
            id,
            item_id: item.clone(),
            content_type: content_type_for(&file_name).to_string(),
            title: file_name,
            size,
            sha256,
            date_added: store::now_ms(),
        };
        if let Err(e) = library.add_attachment(&attachment) {
            let _ = fs::remove_dir_all(&folder);
            return Err(e.into());
        }
        Ok(attachment)
    }

    /// Deletes an attachment's folder. A missing folder is not an error.
    pub(crate) fn remove(&self, attachment: &Attachment) -> Result<(), CoreError> {
        self.remove_folder(attachment.id.as_str())
    }

    pub(crate) fn remove_folder(&self, attachment_id: &str) -> Result<(), CoreError> {
        if !plain(attachment_id) {
            return Err(CoreError::InvalidInput { message: format!("not an attachment: {attachment_id}") });
        }
        let folder = self.root.join(attachment_id);
        match fs::remove_dir_all(&folder) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_error(&folder, e)),
            _ => Ok(()),
        }
    }
}

/// One path component that stays where it is put.
fn plain(part: &str) -> bool {
    !part.is_empty() && part != "." && part != ".." && !part.contains(['/', '\\', ':', '\0'])
}

fn io_error(path: &Path, err: io::Error) -> CoreError {
    CoreError::Storage { message: format!("Could not use the file “{}”: {err}", path.display()) }
}

/// A file's hex SHA-256 and size, as an attachment records them.
pub(crate) fn sha256_and_size(path: &Path) -> Result<(String, u64), CoreError> {
    let sha256 = sha256_of(path).map_err(|e| io_error(path, e))?;
    let size = fs::metadata(path).map_err(|e| io_error(path, e))?.len();
    Ok((sha256, size))
}

fn sha256_of(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

/// A file name safe on every platform: no separators or reserved
/// characters, not empty, at most about 120 characters with its extension.
pub(crate) fn clean_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.');
    if cleaned.is_empty() {
        return "file".to_string();
    }
    if cleaned.chars().count() <= 120 {
        return cleaned.to_string();
    }
    match cleaned.rsplit_once('.') {
        Some((stem, extension)) if extension.len() <= 8 => {
            format!("{}.{extension}", stem.chars().take(110).collect::<String>())
        }
        _ => cleaned.chars().take(120).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_file_names() {
        assert_eq!(clean_file_name("Kucsko 2013 - Nature.pdf"), "Kucsko 2013 - Nature.pdf");
        assert_eq!(clean_file_name("a/b\\c:d?.pdf"), "a_b_c_d_.pdf");
        assert_eq!(clean_file_name("  ..  "), "file");
        let long = format!("{}.pdf", "长".repeat(200));
        let short = clean_file_name(&long);
        assert!(short.ends_with(".pdf") && short.chars().count() == 114, "{short}");
    }

    #[test]
    fn keeps_synced_paths_inside_the_store() {
        let store = FileStore { root: PathBuf::from("/lib/files") };
        assert_eq!(store.synced_path("abc/paper.pdf"), Some(PathBuf::from("/lib/files/abc/paper.pdf")));
        for bad in ["../x", "abc/../../x", "abc", "/etc/passwd", "abc/", "a/b/c", "..\\x/y", "c:/x"] {
            assert_eq!(store.synced_path(bad), None, "{bad}");
        }
        assert!(store.remove_folder("..").is_err());
        assert!(store.remove_folder("a/b").is_err());
        assert!(store.remove_folder("missing").is_ok(), "a missing folder is fine");
    }

    #[test]
    fn hashes_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc.txt");
        fs::write(&path, "abc").unwrap();
        assert_eq!(
            sha256_of(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
