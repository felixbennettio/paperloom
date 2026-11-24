use serde::{Deserialize, Serialize};

use crate::{AttachmentId, ItemId};

/// A file kept with an item: its PDF, supplementary material, a snapshot.
///
/// The file itself lives in the library's file store at `path`; the record
/// carries what lists show and what sync needs to compare copies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: AttachmentId,
    pub item_id: ItemId,
    /// What people see; the file name unless renamed.
    pub title: String,
    /// Inside the file store, `/`-separated: `<attachment id>/<file name>`.
    pub path: String,
    /// `application/pdf`, …
    pub content_type: String,
    pub size: u64,
    /// Hex SHA-256 of the content: spots the same file added twice.
    pub sha256: String,
    /// Milliseconds since 1970.
    pub date_added: i64,
}

impl Attachment {
    pub fn is_pdf(&self) -> bool {
        self.content_type == "application/pdf"
    }
}

/// The media type for a file name's extension; `application/octet-stream`
/// when unknown.
pub fn content_type_for(file_name: &str) -> &'static str {
    let extension = file_name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match extension.as_str() {
        "pdf" => "application/pdf",
        "epub" => "application/epub+zip",
        "html" | "htm" => "text/html",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "doc" => "application/msword",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "csv" => "text/csv",
        "zip" => "application/zip",
        "caj" => "application/x-caj",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_common_types() {
        assert_eq!(content_type_for("Paper.PDF"), "application/pdf");
        assert_eq!(content_type_for("figure.jpeg"), "image/jpeg");
        assert_eq!(content_type_for("README"), "application/octet-stream");
    }
}
