//! Reference file formats for the apps' import and export.

use paperloom_formats::Format;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceFormat {
    pub id: String,
    pub name: String,
    /// File extension without the dot.
    pub file_extension: String,
}

/// What an import did.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// The format found: `ris`, `enw`, `bibtex` or `csl-json`.
    pub format: String,
    pub added: u32,
    /// Items whose DOI was already in the library.
    pub duplicates: u32,
    /// Records without a title or that could not be read.
    pub skipped: u32,
    pub added_ids: Vec<String>,
}

/// The formats `export_references` writes. `import_references` reads these
/// and also MEDLINE (PubMed .nbib), Web of Science plain text, Scopus CSV and
/// EndNote XML.
#[uniffi::export]
pub fn reference_formats() -> Vec<ReferenceFormat> {
    Format::WRITABLE
        .iter()
        .map(|f| ReferenceFormat {
            id: f.id().to_string(),
            name: f.name().to_string(),
            file_extension: f.extension().to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CoreError, PaperLibrary};

    const RIS: &str = "TY  - JOUR\nTI  - First\nDO  - 10.1000/one\nER  - \nTY  - JOUR\nTI  - Second\nER  - \nTY  - JOUR\nER  - \n";

    #[test]
    fn imports_and_reports() {
        let lib = PaperLibrary::in_memory().unwrap();
        let collection = lib.create_collection("Imported".into(), None).unwrap();
        let report = lib.import_references(RIS.into(), Some(collection.id.clone())).unwrap();
        assert_eq!(
            (report.format.as_str(), report.added, report.duplicates, report.skipped),
            ("ris", 2, 0, 1)
        );
        assert_eq!(lib.items_in_collection(collection.id).unwrap().len(), 2);
        let again = lib.import_references(RIS.into(), None).unwrap();
        assert_eq!((again.added, again.duplicates), (1, 1), "the DOI one is a duplicate");
        assert!(matches!(lib.import_references("hello".into(), None), Err(CoreError::InvalidInput { .. })));
    }

    #[test]
    fn exports_every_format() {
        let lib = PaperLibrary::in_memory().unwrap();
        let report = lib.import_references(RIS.into(), None).unwrap();
        for format in reference_formats() {
            let text = lib.export_references(report.added_ids.clone(), format.id.clone()).unwrap();
            assert!(text.contains("First") && text.contains("Second"), "{}: {text}", format.id);
        }
        assert!(matches!(lib.export_references(vec![], "docx".into()), Err(CoreError::InvalidInput { .. })));
    }
}
