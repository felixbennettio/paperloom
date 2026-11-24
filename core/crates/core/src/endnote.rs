//! Moving an EndNote library in from its XML export (File → Export, as
//! XML): items, the PDFs EndNote keeps in the library's `.Data/PDF` folder
//! (copied into this library), and research notes (as Markdown in the
//! vault, linked to their items).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use paperloom_formats::{EndNoteLibrary, read_endnote_library};
use paperloom_model::ItemId;
use serde::{Deserialize, Serialize};

use crate::{CoreError, PaperLibrary, PaperVault};

/// What moving an EndNote library in did.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct EndNoteImport {
    pub added: u32,
    /// Records whose DOI the library already had; their files joined the
    /// item that is here.
    pub duplicates: u32,
    pub files: u32,
    /// Files the export names that are not on this computer (or no
    /// `.Data` folder was found).
    pub missing_files: u32,
    pub notes: u32,
    /// Records with no title.
    pub skipped: u32,
    pub problems: Vec<String>,
}

/// `%20` and the like decoded.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = text.get(i + 1..i + 3).and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The file an EndNote link names: `internal-pdf://<folder>/<name>` in the
/// library's `.Data/PDF`, `file://<path>`, or a path.
fn linked_file(link: &str, data: Option<&Path>) -> Option<PathBuf> {
    if let Some(rest) = link.strip_prefix("internal-pdf://") {
        let mut path = data?.join("PDF");
        for part in percent_decoded(rest).split(['/', '\\']).filter(|p| !p.is_empty() && *p != "..") {
            path.push(part);
        }
        return Some(path);
    }
    if let Some(rest) = link.strip_prefix("file://") {
        let path = percent_decoded(rest.strip_prefix("localhost").unwrap_or(rest));
        // `file:///C:/…` on Windows.
        let path = if path.len() > 2 && path.as_bytes()[2] == b':' { &path[1..] } else { path.as_str() };
        return Some(PathBuf::from(path));
    }
    let path = PathBuf::from(link);
    path.is_absolute().then_some(path)
}

/// The library's `.Data` folder: where the export says the `.enl` is, or
/// beside the export (named after the library, or the only one there).
fn data_folder(xml: &Path, library: &EndNoteLibrary) -> Option<PathBuf> {
    let database = library.database.as_deref().unwrap_or_default();
    let name = database.rsplit(['/', '\\']).next().unwrap_or(database);
    let stem = name.strip_suffix(".enl").or_else(|| name.strip_suffix(".ENL")).unwrap_or(name);
    let mut candidates = Vec::new();
    // Only a path, not a bare name, says where the library was.
    if database.contains(['/', '\\'])
        && let Some(path) = database.strip_suffix(".enl")
    {
        candidates.push(PathBuf::from(format!("{path}.Data")));
    }
    let beside = xml.parent().unwrap_or(Path::new("."));
    if !stem.is_empty() {
        candidates.push(beside.join(format!("{stem}.Data")));
    }
    if let Some(found) = candidates.into_iter().find(|c| c.is_dir()) {
        return Some(found);
    }
    let mut data: Vec<PathBuf> = std::fs::read_dir(beside)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("data")))
        .collect();
    (data.len() == 1).then(|| data.remove(0))
}

/// The `.Data` folder an EndNote export's PDFs would be in, when it can be
/// found beside the export; otherwise ask for it.
#[uniffi::export]
pub fn find_endnote_data_folder(xml_path: String) -> Option<String> {
    let xml = std::fs::read_to_string(&xml_path).ok()?;
    data_folder(Path::new(&xml_path), &read_endnote_library(&xml)).map(|p| p.display().to_string())
}

impl PaperLibrary {
    pub(crate) fn import_endnote_from(
        &self,
        xml_path: &Path,
        data: Option<&Path>,
        vault: Option<&PaperVault>,
    ) -> Result<EndNoteImport, CoreError> {
        let xml = std::fs::read_to_string(xml_path).map_err(|e| CoreError::InvalidInput {
            message: format!("The EndNote export could not be read: {e}."),
        })?;
        let library = read_endnote_library(&xml);
        if library.records.is_empty() && library.skipped == 0 {
            return Err(CoreError::InvalidInput {
                message: "This file is not an EndNote XML export.".into(),
            });
        }
        let data = data.map(Path::to_path_buf).or_else(|| data_folder(xml_path, &library));
        let mut report = EndNoteImport { skipped: library.skipped as u32, ..EndNoteImport::default() };
        let files = self.files()?;
        let mut added_ids: Vec<ItemId> = Vec::new();
        let mut notes = Vec::new();
        {
            let mut store = self.lock()?;
            for record in &library.records {
                let existing = match &record.item.doi {
                    Some(doi) => store.find_by_doi(doi)?,
                    None => None,
                };
                let item = match existing {
                    Some(existing) => {
                        report.duplicates += 1;
                        existing
                    }
                    None => {
                        store.add_item(&record.item)?;
                        report.added += 1;
                        added_ids.push(record.item.id.clone());
                        record.item.clone()
                    }
                };
                for link in &record.files {
                    match linked_file(link, data.as_deref()).filter(|p| p.is_file()) {
                        Some(file) => match files.attach(&mut store, &item.id, &file) {
                            Ok(_) => report.files += 1,
                            Err(e) => report.problems.push(format!("{}: {e}", file.display())),
                        },
                        None => report.missing_files += 1,
                    }
                }
                if let Some(text) = record.research_notes.as_deref().filter(|t| !t.trim().is_empty()) {
                    notes.push((item, text.to_string()));
                }
            }
        }
        self.reindex_items(&added_ids);

        match vault {
            Some(vault) => {
                for (item, text) in notes {
                    let text = format!("{text}\n\n{}\n", crate::notelinks::item_link_text(&item));
                    match vault
                        .create_note("EndNote".into(), item.title.clone())
                        .and_then(|made| vault.write_note(made.path, text))
                    {
                        Ok(_) => report.notes += 1,
                        Err(e) => report.problems.push(e.to_string()),
                    }
                }
            }
            None if !notes.is_empty() => report.problems.push(format!(
                "Research notes left in EndNote, as there is no vault to put them in: {}.",
                notes.len()
            )),
            None => {}
        }
        Ok(report)
    }
}

#[uniffi::export]
impl PaperLibrary {
    /// Moves an EndNote library in from its XML export: items (joined, not
    /// doubled, when their DOI is here), their PDFs from the library's
    /// `.Data` folder (`data_folder`, or found beside the export), and
    /// research notes into the vault's `EndNote` folder. Copies files: call
    /// it off the main thread.
    pub fn import_endnote(
        &self,
        xml_path: String,
        data_folder: Option<String>,
        vault: Option<Arc<PaperVault>>,
    ) -> Result<EndNoteImport, CoreError> {
        self.import_endnote_from(
            Path::new(&xml_path),
            data_folder.as_deref().map(Path::new),
            vault.as_deref(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::NewItem;

    const EXPORT: &str = r#"<?xml version="1.0" encoding="UTF-8" ?><xml><records>
<record><database name="My Library.enl">My Library.enl</database><ref-type name="Journal Article">17</ref-type>
<contributors><authors><author>Kucsko, G.</author></authors></contributors>
<titles><title>Nanometre-scale thermometry in a living cell</title></titles>
<dates><year>2013</year></dates><electronic-resource-num>10.1038/nature12373</electronic-resource-num>
<urls><pdf-urls><url>internal-pdf://0123456789/Kucsko%202013.pdf</url><url>internal-pdf://9/Gone.pdf</url></pdf-urls></urls>
<research-notes>Read twice; see Fig. 2.</research-notes></record>
<record><ref-type name="Book">6</ref-type><titles><title>研究方法</title></titles></record>
</records></xml>"#;

    #[test]
    fn moves_an_endnote_library_in() {
        let dir = tempfile::tempdir().unwrap();
        let export = dir.path().join("Export.xml");
        fs::write(&export, EXPORT).unwrap();
        let pdfs = dir.path().join("My Library.Data/PDF/0123456789");
        fs::create_dir_all(&pdfs).unwrap();
        fs::write(pdfs.join("Kucsko 2013.pdf"), b"%PDF-1.4 thermometry").unwrap();
        assert_eq!(
            find_endnote_data_folder(export.display().to_string()),
            Some(dir.path().join("My Library.Data").display().to_string())
        );

        let library = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
        let vault = PaperVault::open(dir.path().join("Vault").display().to_string()).unwrap();
        library
            .add_item(NewItem {
                title: "Old copy".into(),
                doi: Some("10.1000/other".into()),
                ..NewItem::default()
            })
            .unwrap();
        let report = library.import_endnote(export.display().to_string(), None, Some(vault.clone())).unwrap();
        assert_eq!((report.added, report.duplicates, report.files, report.missing_files), (2, 0, 1, 1));
        assert_eq!((report.notes, report.skipped), (1, 0));
        let items = library.list_items(crate::ItemFilter::default()).unwrap();
        let paper = items.iter().find(|i| i.title.starts_with("Nanometre")).unwrap();
        let attached = library.attachments(paper.id.clone()).unwrap();
        assert_eq!(fs::read(&attached[0].file_path).unwrap(), b"%PDF-1.4 thermometry");
        let note = vault.read_note(format!("EndNote/{}.md", paper.title)).unwrap();
        assert!(note.starts_with("Read twice") && note.contains("[[@10.1038/nature12373|"), "{note}");

        let again = library.import_endnote(export.display().to_string(), None, None).unwrap();
        assert_eq!(
            (again.added, again.duplicates),
            (1, 1),
            "the paper is joined by its DOI; the book has none"
        );
        assert!(again.problems.iter().any(|p| p.contains("no vault")), "{:?}", again.problems);
        fs::write(&export, "<html/>").unwrap();
        assert!(library.import_endnote(export.display().to_string(), None, None).is_err());
    }

    #[test]
    fn follows_endnote_links() {
        let data = Path::new("/lib/My.Data");
        assert_eq!(
            linked_file("internal-pdf://12/A%20B.pdf", Some(data)),
            Some(data.join("PDF").join("12").join("A B.pdf"))
        );
        assert_eq!(
            linked_file("internal-pdf://../../etc/passwd", Some(data)),
            Some(data.join("PDF").join("etc").join("passwd"))
        );
        assert_eq!(linked_file("internal-pdf://12/A.pdf", None), None);
        assert_eq!(linked_file("file:///Users/me/A%20B.pdf", None), Some(PathBuf::from("/Users/me/A B.pdf")));
        assert_eq!(linked_file("relative.pdf", None), None);
        assert_eq!(percent_decoded("%E4%B8%AD%zz"), "中%zz");
    }
}
