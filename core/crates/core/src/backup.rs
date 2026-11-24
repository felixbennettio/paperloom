//! Backing a library up to one `.zip` (the database, attached files and the
//! vault's notes), and putting a backup back.
//!
//! The archive is a plain ZIP (ZIP64 when big) any unzip tool opens; files
//! are stored as they are, since PDFs are compressed already. Putting a
//! backup back writes the notes straight away (the vault keeps what they
//! replace as versions) and unpacks the library beside the open one; the
//! apps call [`finish_restore`] when they next start, before opening the
//! library, and the library that was there is kept aside.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{CoreError, PaperLibrary, PaperVault};

/// What a backup holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct BackupReport {
    pub path: String,
    /// Files in the archive: the database, attachments and notes.
    pub files: u32,
    pub bytes: u64,
}

/// What putting a backup back did now; the library follows on restart.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub notes: u32,
    pub attachments: u32,
}

const MANIFEST: &str = "paperloom-backup.json";
/// Backups made on a schedule are `Paperloom <date> <time>.zip` (UTC), so
/// their names sort by time.
const AUTOMATIC: &str = "Paperloom ";
const DATABASE: &str = "library.sqlite";

/// `what` is a step such as "Saving"; the path is quoted so that a
/// translation can tell the two apart (messages.rs).
fn storage(what: &str, path: &Path, e: impl std::fmt::Display) -> CoreError {
    CoreError::Storage { message: format!("{what} “{}” failed: {e}", path.display()) }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

// CRC-32 (IEEE), as ZIP wants it.
static CRC_TABLE: LazyLock<[u32; 256]> = LazyLock::new(|| {
    let mut table = [0u32; 256];
    for (n, entry) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 == 1 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *entry = c;
    }
    table
});

fn crc32_update(crc: u32, bytes: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in bytes {
        c = CRC_TABLE[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

/// A file's CRC-32 and size, read once.
fn crc32_of(path: &Path) -> io::Result<(u32, u64)> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut buffer = vec![0u8; 64 * 1024];
    let (mut crc, mut size) = (0u32, 0u64);
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            return Ok((crc, size));
        }
        crc = crc32_update(crc, &buffer[..n]);
        size += n as u64;
    }
}

/// The time and date fields of a ZIP entry (UTC; 1980 at the earliest).
fn dos_time(ms: u64) -> (u16, u16) {
    let seconds = ms / 1000;
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = (yoe + era * 400 + i64::from(month <= 2)).clamp(1980, 2107);
    let time = (((rest / 3600) << 11) | ((rest % 3600 / 60) << 5) | ((rest % 60) / 2)) as u16;
    let date = (((year - 1980) << 9) | (month << 5) | day) as u16;
    (time, date)
}

struct Written {
    name: String,
    crc: u32,
    size: u64,
    offset: u64,
}

/// Writes a ZIP of stored files, ZIP64 where sizes or offsets need it.
pub(crate) struct ZipWriter {
    out: BufWriter<File>,
    at: u64,
    written: Vec<Written>,
    time: (u16, u16),
}

const BIG: u64 = 0xffff_ffff;

impl ZipWriter {
    pub(crate) fn create(path: &Path) -> io::Result<Self> {
        Ok(Self {
            out: BufWriter::new(File::create(path)?),
            at: 0,
            written: Vec::new(),
            time: dos_time(now_ms()),
        })
    }

    fn put(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.out.write_all(bytes)?;
        self.at += bytes.len() as u64;
        Ok(())
    }

    /// Adds bytes held in memory.
    pub(crate) fn add_bytes(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let (crc, size) = (crc32_update(0, bytes), bytes.len() as u64);
        let offset = self.at;
        self.local_header(name, crc, size)?;
        self.put(bytes)?;
        self.written.push(Written { name: name.to_string(), crc, size, offset });
        Ok(())
    }

    fn add_file(&mut self, name: &str, source: &Path) -> io::Result<()> {
        let (crc, size) = crc32_of(source)?;
        let offset = self.at;
        self.local_header(name, crc, size)?;
        let mut reader = BufReader::new(File::open(source)?);
        let mut buffer = vec![0u8; 64 * 1024];
        let mut copied = 0u64;
        loop {
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            self.put(&buffer[..n])?;
            copied += n as u64;
        }
        if copied != size {
            return Err(io::Error::other(format!("{name} changed while it was backed up")));
        }
        self.written.push(Written { name: name.to_string(), crc, size, offset });
        Ok(())
    }

    fn local_header(&mut self, name: &str, crc: u32, size: u64) -> io::Result<()> {
        let zip64 = size >= BIG;
        let mut header = Vec::with_capacity(64 + name.len());
        header.extend(0x0403_4b50u32.to_le_bytes());
        header.extend(45u16.to_le_bytes());
        header.extend(0x0800u16.to_le_bytes());
        header.extend(0u16.to_le_bytes());
        header.extend(self.time.0.to_le_bytes());
        header.extend(self.time.1.to_le_bytes());
        header.extend(crc.to_le_bytes());
        let small = if zip64 { BIG as u32 } else { size as u32 };
        header.extend(small.to_le_bytes());
        header.extend(small.to_le_bytes());
        header.extend((name.len() as u16).to_le_bytes());
        header.extend((if zip64 { 20u16 } else { 0 }).to_le_bytes());
        header.extend(name.as_bytes());
        if zip64 {
            header.extend(1u16.to_le_bytes());
            header.extend(16u16.to_le_bytes());
            header.extend(size.to_le_bytes());
            header.extend(size.to_le_bytes());
        }
        self.put(&header)?;
        Ok(())
    }

    pub(crate) fn finish(mut self) -> io::Result<u64> {
        let start = self.at;
        let written = std::mem::take(&mut self.written);
        for entry in &written {
            let mut extra = Vec::new();
            if entry.size >= BIG {
                extra.extend(entry.size.to_le_bytes());
                extra.extend(entry.size.to_le_bytes());
            }
            if entry.offset >= BIG {
                extra.extend(entry.offset.to_le_bytes());
            }
            let mut record = Vec::with_capacity(80 + entry.name.len());
            record.extend(0x0201_4b50u32.to_le_bytes());
            record.extend(((3u16 << 8) | 45).to_le_bytes());
            record.extend(45u16.to_le_bytes());
            record.extend(0x0800u16.to_le_bytes());
            record.extend(0u16.to_le_bytes());
            record.extend(self.time.0.to_le_bytes());
            record.extend(self.time.1.to_le_bytes());
            record.extend(entry.crc.to_le_bytes());
            let small = if entry.size >= BIG { BIG as u32 } else { entry.size as u32 };
            record.extend(small.to_le_bytes());
            record.extend(small.to_le_bytes());
            record.extend((entry.name.len() as u16).to_le_bytes());
            record.extend((if extra.is_empty() { 0 } else { extra.len() as u16 + 4 }).to_le_bytes());
            record.extend(0u16.to_le_bytes());
            record.extend(0u16.to_le_bytes());
            record.extend(0u16.to_le_bytes());
            record.extend((0o100_644u32 << 16).to_le_bytes());
            record.extend((if entry.offset >= BIG { BIG as u32 } else { entry.offset as u32 }).to_le_bytes());
            record.extend(entry.name.as_bytes());
            if !extra.is_empty() {
                record.extend(1u16.to_le_bytes());
                record.extend((extra.len() as u16).to_le_bytes());
                record.extend(extra);
            }
            self.put(&record)?;
        }
        let size = self.at - start;
        let count = written.len() as u64;
        let zip64 = start >= BIG || count >= 0xffff;
        if zip64 {
            let at = self.at;
            let mut end = Vec::new();
            end.extend(0x0606_4b50u32.to_le_bytes());
            end.extend(44u64.to_le_bytes());
            end.extend(((3u16 << 8) | 45).to_le_bytes());
            end.extend(45u16.to_le_bytes());
            end.extend(0u32.to_le_bytes());
            end.extend(0u32.to_le_bytes());
            end.extend(count.to_le_bytes());
            end.extend(count.to_le_bytes());
            end.extend(size.to_le_bytes());
            end.extend(start.to_le_bytes());
            end.extend(0x0706_4b50u32.to_le_bytes());
            end.extend(0u32.to_le_bytes());
            end.extend(at.to_le_bytes());
            end.extend(1u32.to_le_bytes());
            self.put(&end)?;
        }
        let mut end = Vec::new();
        end.extend(0x0605_4b50u32.to_le_bytes());
        end.extend(0u16.to_le_bytes());
        end.extend(0u16.to_le_bytes());
        let short = count.min(0xffff) as u16;
        end.extend(short.to_le_bytes());
        end.extend(short.to_le_bytes());
        end.extend((size.min(BIG) as u32).to_le_bytes());
        end.extend((if zip64 { BIG as u32 } else { start as u32 }).to_le_bytes());
        end.extend(0u16.to_le_bytes());
        self.put(&end)?;
        self.out.flush()?;
        Ok(self.at)
    }
}

/// An entry of a ZIP: its name, where its local header is, its size and
/// CRC-32.
pub(crate) struct Entry {
    pub name: String,
    offset: u64,
    size: u64,
    crc: u32,
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

fn u64_at(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap_or_default())
}

fn bad(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

/// The entries of a ZIP whose files are stored (as backups are).
pub(crate) fn read_entries(file: &mut File) -> io::Result<Vec<Entry>> {
    let length = file.seek(SeekFrom::End(0))?;
    let tail = length.min(66_000);
    file.seek(SeekFrom::Start(length - tail))?;
    let mut end = vec![0u8; tail as usize];
    file.read_exact(&mut end)?;
    let at = (0..end.len().saturating_sub(21))
        .rev()
        .find(|&i| u32_at(&end, i) == 0x0605_4b50)
        .ok_or_else(|| bad("not a ZIP archive"))?;
    let mut count = u64::from(u16_at(&end, at + 10));
    let mut size = u64::from(u32_at(&end, at + 12));
    let mut start = u64::from(u32_at(&end, at + 16));
    if (count == 0xffff || start == BIG) && at >= 20 && u32_at(&end, at - 20) == 0x0706_4b50 {
        let record = u64_at(&end, at - 20 + 8);
        let mut zip64 = [0u8; 56];
        file.seek(SeekFrom::Start(record))?;
        file.read_exact(&mut zip64)?;
        if u32_at(&zip64, 0) != 0x0606_4b50 {
            return Err(bad("a damaged ZIP64 archive"));
        }
        count = u64_at(&zip64, 32);
        size = u64_at(&zip64, 40);
        start = u64_at(&zip64, 48);
    }
    let mut directory = vec![0u8; usize::try_from(size).map_err(|_| bad("too big"))?];
    file.seek(SeekFrom::Start(start))?;
    file.read_exact(&mut directory)?;
    let mut entries = Vec::new();
    let mut i = 0;
    for _ in 0..count {
        if i + 46 > directory.len() || u32_at(&directory, i) != 0x0201_4b50 {
            return Err(bad("a damaged ZIP directory"));
        }
        let method = u16_at(&directory, i + 10);
        let crc = u32_at(&directory, i + 16);
        let mut size = u64::from(u32_at(&directory, i + 24));
        let name_length = usize::from(u16_at(&directory, i + 28));
        let extra_length = usize::from(u16_at(&directory, i + 30));
        let comment_length = usize::from(u16_at(&directory, i + 32));
        let mut offset = u64::from(u32_at(&directory, i + 42));
        let name_start = i + 46;
        let name = String::from_utf8_lossy(
            directory.get(name_start..name_start + name_length).ok_or_else(|| bad("a damaged ZIP name"))?,
        )
        .into_owned();
        let extra = directory
            .get(name_start + name_length..name_start + name_length + extra_length)
            .unwrap_or_default();
        // ZIP64 sizes and offset, in that order, for fields that are full.
        let mut j = 0;
        while j + 4 <= extra.len() {
            let (id, length) = (u16_at(extra, j), usize::from(u16_at(extra, j + 2)));
            if id == 1 {
                let mut k = j + 4;
                if size == BIG && k + 8 <= extra.len() {
                    size = u64_at(extra, k);
                    k += 16;
                }
                if offset == BIG && k + 8 <= extra.len() {
                    offset = u64_at(extra, k);
                }
            }
            j += 4 + length;
        }
        if method != 0 && !name.ends_with('/') {
            return Err(bad("this archive was not made by Paperloom (its files are compressed)"));
        }
        entries.push(Entry { name, offset, size, crc });
        i = name_start + name_length + extra_length + comment_length;
    }
    Ok(entries)
}

/// Copies an entry's bytes to a file, checking its CRC-32.
pub(crate) fn extract(file: &mut File, entry: &Entry, target: &Path) -> io::Result<()> {
    let mut header = [0u8; 30];
    file.seek(SeekFrom::Start(entry.offset))?;
    file.read_exact(&mut header)?;
    if u32_at(&header, 0) != 0x0403_4b50 {
        return Err(bad("a damaged ZIP entry"));
    }
    let skip = u64::from(u16_at(&header, 26)) + u64::from(u16_at(&header, 28));
    file.seek(SeekFrom::Start(entry.offset + 30 + skip))?;
    if let Some(folder) = target.parent() {
        fs::create_dir_all(folder)?;
    }
    let mut out = BufWriter::new(File::create(target)?);
    let mut left = entry.size;
    let mut crc = 0u32;
    let mut buffer = vec![0u8; 64 * 1024];
    while left > 0 {
        let n = file.read(&mut buffer[..usize::try_from(left.min(65_536)).unwrap_or(65_536)])?;
        if n == 0 {
            return Err(bad("a ZIP entry ends early"));
        }
        crc = crc32_update(crc, &buffer[..n]);
        out.write_all(&buffer[..n])?;
        left -= n as u64;
    }
    out.flush()?;
    if crc != entry.crc {
        return Err(bad(&format!("{} is damaged in the archive", entry.name)));
    }
    Ok(())
}

/// Every file under a folder, by `/`-separated path; `.trash` is left out.
fn files_under(root: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let mut folders = vec![(String::new(), root.to_path_buf())];
    while let Some((prefix, folder)) = folders.pop() {
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".trash" || name.ends_with(".paperloom-tmp") {
                continue;
            }
            let path = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => folders.push((path, entry.path())),
                Ok(kind) if kind.is_file() => found.push((path, entry.path())),
                _ => {}
            }
        }
    }
    found.sort();
    found
}

/// A name from an archive as a path under `root`, if it stays there.
fn inside(root: &Path, name: &str) -> Option<PathBuf> {
    let relative = Path::new(name);
    let plain = !name.contains('\\') && relative.components().all(|c| matches!(c, Component::Normal(_)));
    plain.then(|| root.join(relative))
}

impl PaperLibrary {
    /// Writes the archive: a manifest, the database snapshot, the attached
    /// files, the added citation styles and the vault. Returns how many
    /// files it holds.
    fn write_backup(
        &self,
        partial: &Path,
        folder: &Path,
        snapshot: &Path,
        vault: Option<&PaperVault>,
    ) -> io::Result<u32> {
        let mut zip = ZipWriter::create(partial)?;
        let manifest = folder.join(format!("{MANIFEST}.{}", now_ms()));
        fs::write(&manifest, serde_json::json!({ "format": 1, "created": now_ms() }).to_string())?;
        zip.add_file(MANIFEST, &manifest)?;
        let _ = fs::remove_file(&manifest);
        zip.add_file(DATABASE, snapshot)?;
        let mut count = 2;
        if let Some(files) = &self.files {
            for (name, path) in files_under(files.root()) {
                zip.add_file(&format!("files/{name}"), &path)?;
                count += 1;
            }
        }
        if let Some(styles) = &self.styles_dir {
            for (name, path) in files_under(styles).into_iter().filter(|(name, _)| name.ends_with(".csl")) {
                zip.add_file(&format!("styles/{name}"), &path)?;
                count += 1;
            }
        }
        if let Some(root) = vault.and_then(|v| v.root().ok()) {
            for (name, path) in files_under(Path::new(&root)) {
                zip.add_file(&format!("vault/{name}"), &path)?;
                count += 1;
            }
        }
        zip.finish()?;
        Ok(count)
    }

    /// The automatic backups in a folder, oldest first.
    fn automatic_backups(folder: &Path) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(folder) else { return Vec::new() };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(AUTOMATIC) && n.ends_with(".zip"))
            })
            .collect();
        found.sort();
        found
    }

    /// The folder the library's database is in.
    fn folder(&self) -> Result<PathBuf, CoreError> {
        self.sync_dir.as_ref().and_then(|d| d.parent()).map(Path::to_path_buf).ok_or_else(|| {
            CoreError::Storage {
                message: "This library is kept only in memory and cannot be backed up.".into(),
            }
        })
    }
}

#[uniffi::export]
impl PaperLibrary {
    /// Backs the library up to a `.zip` at `destination` (replacing it):
    /// the database, attached files, and the vault's notes when given.
    /// Reads every file: call it off the main thread.
    pub fn back_up(
        &self,
        destination: String,
        vault: Option<Arc<PaperVault>>,
    ) -> Result<BackupReport, CoreError> {
        let folder = self.folder()?;
        let destination = PathBuf::from(destination);
        let partial = destination.with_extension("zip.partial");
        let snapshot = folder.join(format!("backup-{}.sqlite", now_ms()));
        let _ = fs::remove_file(&snapshot);
        self.lock()?.back_up_to(&snapshot)?;
        let result = self.write_backup(&partial, &folder, &snapshot, vault.as_deref());
        let _ = fs::remove_file(&snapshot);
        let files = result.map_err(|e| {
            let _ = fs::remove_file(&partial);
            storage("Backing up to", &destination, e)
        })?;
        fs::rename(&partial, &destination).map_err(|e| storage("Saving", &destination, e))?;
        let bytes = fs::metadata(&destination).map_or(0, |m| m.len());
        Ok(BackupReport { path: destination.display().to_string(), files, bytes })
    }

    /// Backs the library up into `folder` (by default `backups/` beside it)
    /// when its newest automatic backup is older than `every_hours`, then
    /// keeps only the newest `keep`. None when no backup was due. Reads
    /// every file: call it off the main thread.
    pub fn back_up_if_due(
        &self,
        folder: Option<String>,
        vault: Option<Arc<PaperVault>>,
        every_hours: u32,
        keep: u32,
    ) -> Result<Option<BackupReport>, CoreError> {
        let folder = match folder.filter(|f| !f.trim().is_empty()) {
            Some(folder) => PathBuf::from(folder),
            None => self.folder()?.join("backups"),
        };
        let mut made = Self::automatic_backups(&folder);
        let age = made
            .last()
            .and_then(|newest| fs::metadata(newest).ok()?.modified().ok())
            .and_then(|modified| SystemTime::now().duration_since(modified).ok());
        if age.is_some_and(|age| age.as_secs() < u64::from(every_hours) * 3600) {
            return Ok(None);
        }
        fs::create_dir_all(&folder).map_err(|e| storage("Making", &folder, e))?;
        let now = now_ms() as i64;
        let seconds = now.div_euclid(1000).rem_euclid(86_400);
        let name = format!(
            "{AUTOMATIC}{} {:02}{:02}{:02}.zip",
            paperloom_vault::date_of(now),
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        );
        let path = folder.join(name);
        let report = self.back_up(path.display().to_string(), vault)?;
        if !made.contains(&path) {
            made.push(path);
        }
        let keep = keep.max(1) as usize;
        for old in made.iter().take(made.len().saturating_sub(keep)) {
            let _ = fs::remove_file(old);
        }
        Ok(Some(report))
    }

    /// Puts a backup back: its notes go into the vault now (what they
    /// replace is kept as versions), and its library waits beside this
    /// one until the app next starts and calls `finish_restore`.
    pub fn restore_backup(
        &self,
        backup: String,
        vault: Option<Arc<PaperVault>>,
    ) -> Result<RestoreReport, CoreError> {
        let folder = self.folder()?;
        let backup = PathBuf::from(backup);
        let mut file = File::open(&backup).map_err(|e| storage("Opening", &backup, e))?;
        let entries = read_entries(&mut file).map_err(|e| storage("Reading", &backup, e))?;
        if !entries.iter().any(|e| e.name == MANIFEST) || !entries.iter().any(|e| e.name == DATABASE) {
            return Err(CoreError::InvalidInput { message: "This file is not a Paperloom backup.".into() });
        }
        let staging = folder.join("restoring");
        let _ = fs::remove_dir_all(&staging);
        let mut report = RestoreReport::default();
        let unpack = |file: &mut File, entry: &Entry, target: &Path| {
            extract(file, entry, target).map_err(|e| storage("Unpacking", &backup, e))
        };
        for entry in &entries {
            if entry.name.ends_with('/') {
                continue;
            }
            if entry.name == DATABASE || entry.name.starts_with("files/") {
                let Some(target) = inside(&staging, &entry.name) else { continue };
                unpack(&mut file, entry, &target)?;
                if entry.name != DATABASE {
                    report.attachments += 1;
                }
            } else if let Some(style) = entry.name.strip_prefix("styles/")
                && style.ends_with(".csl")
                && let Some(target) = inside(&folder.join("styles"), style)
            {
                // Added styles join this library's own; it adds them on restart.
                unpack(&mut file, entry, &target)?;
            } else if let (Some(note), Some(vault)) = (entry.name.strip_prefix("vault/"), &vault)
                && note.to_lowercase().ends_with(".md")
                && inside(Path::new("."), note).is_some()
            {
                let scratch = staging.join("note.md");
                unpack(&mut file, entry, &scratch)?;
                let text = fs::read_to_string(&scratch).map_err(|e| storage("Reading", &scratch, e))?;
                vault.write_note(note.to_string(), text)?;
                let _ = fs::remove_file(&scratch);
                report.notes += 1;
            }
        }
        fs::write(staging.join("ready"), b"").map_err(|e| storage("Writing", &staging, e))?;
        Ok(report)
    }
}

/// Swaps in a library a backup put beside this one, before the app opens
/// it; the library that was there moves to `before-restore-<time>`.
/// Returns whether there was one.
#[uniffi::export]
pub fn finish_restore(library_path: String) -> Result<bool, CoreError> {
    let database = PathBuf::from(&library_path);
    let folder =
        database.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
    let staging = folder.join("restoring");
    if !staging.join("ready").exists() {
        return Ok(false);
    }
    let aside = folder.join(format!("before-restore-{}", now_ms()));
    fs::create_dir_all(&aside).map_err(|e| storage("Making", &aside, e))?;
    let name =
        database.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| DATABASE.into());
    let moving = [
        name.clone(),
        format!("{name}-wal"),
        format!("{name}-shm"),
        "files".into(),
        "index".into(),
        "sync".into(),
    ];
    for part in &moving {
        let from = folder.join(part);
        if from.exists() {
            fs::rename(&from, aside.join(part)).map_err(|e| storage("Moving aside", &from, e))?;
        }
    }
    fs::rename(staging.join(DATABASE), &database).map_err(|e| storage("Putting back", &database, e))?;
    if staging.join("files").exists() {
        fs::rename(staging.join("files"), folder.join("files"))
            .map_err(|e| storage("Putting back", &folder, e))?;
    }
    let _ = fs::remove_dir_all(&staging);
    Ok(true)
}

#[cfg(test)]
mod tests;
