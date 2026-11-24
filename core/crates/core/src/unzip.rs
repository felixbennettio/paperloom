//! ZIP archives held in memory, their files stored or deflated: XMind
//! files and Zotero plugins. (Backups, which Paperloom writes itself, have
//! a streaming reader of their own.)

use std::io::Read as _;

use flate2::read::DeflateDecoder;

/// A little-endian number in a ZIP header.
fn le(bytes: &[u8], at: usize, width: usize) -> Option<usize> {
    let field = bytes.get(at..at.checked_add(width)?)?;
    Some(field.iter().rev().fold(0, |n, &b| (n << 8) | usize::from(b)))
}

/// A file in an archive.
pub(crate) struct Entry<'a> {
    pub name: String,
    method: usize,
    data: &'a [u8],
}

impl Entry<'_> {
    pub fn is_folder(&self) -> bool {
        self.name.ends_with('/')
    }

    /// The file's bytes, if they unpack to no more than `limit`.
    pub fn contents(&self, limit: u64) -> Option<Vec<u8>> {
        match self.method {
            0 => (self.data.len() as u64 <= limit).then(|| self.data.to_vec()),
            8 => {
                let mut out = Vec::new();
                DeflateDecoder::new(self.data).take(limit + 1).read_to_end(&mut out).ok()?;
                (out.len() as u64 <= limit).then_some(out)
            }
            _ => None,
        }
    }
}

/// Where the central directory starts and how many files it lists, from
/// the end record (or its ZIP64 form).
fn directory(bytes: &[u8]) -> Option<(usize, usize)> {
    let end = (0..bytes.len().saturating_sub(21)).rev().find(|&i| le(bytes, i, 4) == Some(0x0605_4b50))?;
    let count = le(bytes, end + 10, 2)?;
    let start = le(bytes, end + 16, 4)?;
    if (count == 0xffff || start == 0xffff_ffff) && end >= 20 && le(bytes, end - 20, 4) == Some(0x0706_4b50) {
        let record = le(bytes, end - 20 + 8, 8)?;
        if le(bytes, record, 4)? != 0x0606_4b50 {
            return None;
        }
        return Some((le(bytes, record + 48, 8)?, le(bytes, record + 32, 8)?));
    }
    Some((start, count))
}

/// The files an archive lists, or none if it is not a ZIP this can read.
pub(crate) fn entries(bytes: &[u8]) -> Option<Vec<Entry<'_>>> {
    let (mut at, count) = directory(bytes)?;
    let mut found = Vec::with_capacity(count.min(100_000));
    for _ in 0..count {
        if le(bytes, at, 4)? != 0x0201_4b50 {
            return None;
        }
        let method = le(bytes, at + 10, 2)?;
        let compressed = le(bytes, at + 20, 4)?;
        let name_length = le(bytes, at + 28, 2)?;
        let skip = name_length + le(bytes, at + 30, 2)? + le(bytes, at + 32, 2)?;
        let name = String::from_utf8_lossy(bytes.get(at + 46..at + 46 + name_length)?).into_owned();
        let offset = le(bytes, at + 42, 4).filter(|&offset| offset < bytes.len())?;
        let start = offset + 30 + le(bytes, offset + 26, 2)? + le(bytes, offset + 28, 2)?;
        let data = bytes.get(start..start.checked_add(compressed)?)?;
        found.push(Entry { name, method, data });
        at += 46 + skip;
    }
    Some(found)
}

/// One file of an archive, unpacked (up to 64 MB).
pub(crate) fn file(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
    entries(bytes)?.into_iter().find(|e| e.name == name)?.contents(64 << 20)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A ZIP of deflated files, as XMind and Java's `jar` write them.
    pub(crate) fn deflated_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write as _;
        let mut zip = Vec::new();
        let mut directory = Vec::new();
        for (name, content) in files {
            let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(content).unwrap();
            let data = encoder.finish().unwrap();
            let offset = zip.len() as u32;
            zip.extend(0x0403_4b50u32.to_le_bytes());
            // Version, flags, method (deflate), time, date, CRC.
            zip.extend([20u8, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            zip.extend((data.len() as u32).to_le_bytes());
            zip.extend((content.len() as u32).to_le_bytes());
            zip.extend((name.len() as u16).to_le_bytes());
            zip.extend(0u16.to_le_bytes());
            zip.extend(name.as_bytes());
            zip.extend(&data);
            directory.extend(0x0201_4b50u32.to_le_bytes());
            // Made by, needed, flags, method, time, date, CRC.
            directory.extend([20u8, 0, 20, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            directory.extend((data.len() as u32).to_le_bytes());
            directory.extend((content.len() as u32).to_le_bytes());
            directory.extend((name.len() as u16).to_le_bytes());
            // Extra, comment, disk, attributes.
            directory.extend([0u8; 12]);
            directory.extend(offset.to_le_bytes());
            directory.extend(name.as_bytes());
        }
        let start = zip.len() as u32;
        let size = directory.len() as u32;
        zip.extend(directory);
        zip.extend(0x0605_4b50u32.to_le_bytes());
        zip.extend([0u8; 4]);
        zip.extend((files.len() as u16).to_le_bytes());
        zip.extend((files.len() as u16).to_le_bytes());
        zip.extend(size.to_le_bytes());
        zip.extend(start.to_le_bytes());
        zip.extend(0u16.to_le_bytes());
        zip
    }

    #[test]
    fn reads_deflated_files() {
        let zip = deflated_zip(&[("a/", b""), ("a/one.txt", b"first file"), ("two.json", b"{\"n\": 2}")]);
        let all = entries(&zip).unwrap();
        assert_eq!(all.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["a/", "a/one.txt", "two.json"]);
        assert!(all[0].is_folder() && !all[1].is_folder());
        assert_eq!(all[1].contents(100).unwrap(), b"first file");
        assert!(all[1].contents(5).is_none(), "over the limit");
        assert_eq!(file(&zip, "two.json").unwrap(), b"{\"n\": 2}");
        assert!(file(&zip, "three").is_none());
        assert!(entries(b"not a ZIP archive, only some bytes").is_none());
        assert!(entries(&zip[..zip.len() - 30]).is_none(), "cut short");
    }
}
