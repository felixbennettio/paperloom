//! Looking up an item's details from an identifier, and the full-detail
//! record the apps show on an item's page.

use paperloom_metadata::{self as metadata, Identifier, Resolver};
use paperloom_model::{CreatorRole, Item};
use serde::{Deserialize, Serialize};

use crate::CoreError;

impl From<metadata::Error> for CoreError {
    fn from(err: metadata::Error) -> Self {
        let message = err.to_string();
        match err {
            metadata::Error::Network(_) => CoreError::Network { message },
            metadata::Error::NotFound(_) => CoreError::NotFound { message },
            _ => CoreError::Lookup { message },
        }
    }
}

/// Every field of an item. `id` is empty for an item that was only looked
/// up, not added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct ItemDetail {
    pub id: String,
    pub item_type: String,
    pub title: String,
    /// Authors as shown: `Marie Curie`, `张三`.
    pub authors: Vec<String>,
    pub editors: Vec<String>,
    pub container_title: Option<String>,
    pub container_short_title: Option<String>,
    pub year: Option<i32>,
    pub date: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub publisher: Option<String>,
    pub place: Option<String>,
    pub edition: Option<String>,
    pub number: Option<String>,
    pub doi: Option<String>,
    pub isbn: Option<String>,
    pub issn: Option<String>,
    pub arxiv_id: Option<String>,
    pub pmid: Option<String>,
    pub url: Option<String>,
    pub abstract_note: Option<String>,
    #[serde(default)]
    pub extra: Option<String>,
    pub keywords: Vec<String>,
    pub language: Option<String>,
    /// Shown only; `update_item` leaves these alone.
    pub starred: bool,
    pub read_status: String,
    pub tags: Vec<String>,
    /// How many works cite it, as OpenAlex last counted them. Not edited.
    #[serde(default)]
    pub cited_by: Option<u32>,
}

impl From<&paperloom_store::Entry> for ItemDetail {
    fn from(entry: &paperloom_store::Entry) -> Self {
        Self {
            starred: entry.starred,
            read_status: entry.read_status.as_str().to_string(),
            tags: entry.tags.clone(),
            ..Self::from(&entry.item)
        }
    }
}

impl From<&Item> for ItemDetail {
    fn from(item: &Item) -> Self {
        let names = |role: CreatorRole| -> Vec<String> {
            item.creators.iter().filter(|c| c.role == role).map(|c| c.display_name()).collect()
        };
        Self {
            id: item.id.to_string(),
            item_type: item.item_type.as_str().to_string(),
            title: item.title.clone(),
            authors: names(CreatorRole::Author),
            editors: names(CreatorRole::Editor),
            container_title: item.container_title.clone(),
            container_short_title: item.container_short_title.clone(),
            year: item.year,
            date: item.date.clone(),
            volume: item.volume.clone(),
            issue: item.issue.clone(),
            pages: item.pages.clone(),
            publisher: item.publisher.clone(),
            place: item.place.clone(),
            edition: item.edition.clone(),
            number: item.number.clone(),
            doi: item.doi.as_ref().map(|d| d.to_string()),
            isbn: item.isbn.clone(),
            issn: item.issn.clone(),
            arxiv_id: item.arxiv_id.clone(),
            pmid: item.pmid.clone(),
            url: item.url.clone(),
            abstract_note: item.abstract_note.clone(),
            extra: item.extra.clone(),
            keywords: item.keywords.clone(),
            language: item.language.clone(),
            starred: false,
            read_status: paperloom_model::ReadStatus::Unread.as_str().to_string(),
            tags: Vec::new(),
            cited_by: item.cited_by,
        }
    }
}

/// What kind of identifier `input` is: `doi`, `arxiv`, `pmid`, `isbn`, or
/// nothing. Cheap: no network.
#[uniffi::export]
pub fn recognize_identifier(input: String) -> Option<String> {
    Identifier::detect(&input).map(|id| id.kind().to_string())
}

/// Looks up the details of the work a DOI, arXiv ID, PubMed ID or ISBN
/// names, without adding it. Talks to the network: call it off the main
/// thread.
#[uniffi::export]
pub fn lookup_identifier(input: String) -> Result<ItemDetail, CoreError> {
    let mut detail = ItemDetail::from(&resolve(&input)?);
    detail.id = String::new();
    Ok(detail)
}

pub(crate) fn resolve(input: &str) -> Result<Item, CoreError> {
    let id = Identifier::detect(input).ok_or_else(|| CoreError::InvalidInput {
        message: format!("“{}” is not a DOI, arXiv ID, PubMed ID or ISBN.", input.trim()),
    })?;
    Ok(Resolver::new().resolve(&id)?)
}

#[cfg(test)]
mod tests {
    use paperloom_model::{Creator, ItemType};

    use super::*;

    #[test]
    fn recognizes_identifiers() {
        assert_eq!(recognize_identifier("doi:10.1038/nature12373".into()).as_deref(), Some("doi"));
        assert_eq!(recognize_identifier("arXiv:1706.03762".into()).as_deref(), Some("arxiv"));
        assert_eq!(recognize_identifier("hello".into()), None);
    }

    #[test]
    fn rejects_text_that_is_no_identifier() {
        assert!(matches!(lookup_identifier("deep learning".into()), Err(CoreError::InvalidInput { .. })));
    }

    #[test]
    fn details_list_authors_and_editors() {
        let mut item = Item::new(ItemType::BookChapter, "Chapter");
        item.creators = vec![
            Creator::person("Curie", "Marie"),
            Creator { role: CreatorRole::Editor, ..Creator::literal("张三") },
        ];
        let detail = ItemDetail::from(&item);
        assert_eq!(detail.authors, ["Marie Curie"]);
        assert_eq!(detail.editors, ["张三"]);
        assert_eq!(detail.item_type, "book-chapter");
    }

    #[test]
    fn maps_lookup_errors() {
        assert!(matches!(CoreError::from(metadata::Error::NotFound("x".into())), CoreError::NotFound { .. }));
        assert!(matches!(CoreError::from(metadata::Error::Network("x".into())), CoreError::Network { .. }));
        let parse = metadata::Error::Parse { what: "x".into(), detail: "y".into() };
        assert!(matches!(CoreError::from(parse), CoreError::Lookup { .. }));
    }
}
