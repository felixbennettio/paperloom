use serde::{Deserialize, Serialize};

use crate::{Creator, Doi, ItemId};

/// The kind of work an item describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemType {
    #[default]
    JournalArticle,
    ConferencePaper,
    Book,
    BookChapter,
    Thesis,
    Report,
    Preprint,
    Patent,
    Standard,
    Dataset,
    Webpage,
    Other,
}

impl ItemType {
    pub const ALL: [ItemType; 12] = [
        ItemType::JournalArticle,
        ItemType::ConferencePaper,
        ItemType::Book,
        ItemType::BookChapter,
        ItemType::Thesis,
        ItemType::Report,
        ItemType::Preprint,
        ItemType::Patent,
        ItemType::Standard,
        ItemType::Dataset,
        ItemType::Webpage,
        ItemType::Other,
    ];

    /// The identifier stored on disk and passed to the UIs.
    pub fn as_str(self) -> &'static str {
        match self {
            ItemType::JournalArticle => "journal-article",
            ItemType::ConferencePaper => "conference-paper",
            ItemType::Book => "book",
            ItemType::BookChapter => "book-chapter",
            ItemType::Thesis => "thesis",
            ItemType::Report => "report",
            ItemType::Preprint => "preprint",
            ItemType::Patent => "patent",
            ItemType::Standard => "standard",
            ItemType::Dataset => "dataset",
            ItemType::Webpage => "webpage",
            ItemType::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == value)
    }

    /// The matching CSL 1.0.2 item type.
    pub fn csl_type(self) -> &'static str {
        match self {
            ItemType::JournalArticle => "article-journal",
            ItemType::ConferencePaper => "paper-conference",
            ItemType::Book => "book",
            ItemType::BookChapter => "chapter",
            ItemType::Thesis => "thesis",
            ItemType::Report => "report",
            ItemType::Preprint => "article",
            ItemType::Patent => "patent",
            ItemType::Standard => "standard",
            ItemType::Dataset => "dataset",
            ItemType::Webpage => "webpage",
            ItemType::Other => "document",
        }
    }

    /// The document type code GB/T 7714 prints after the title, as in
    /// `[J]` for a journal article.
    pub fn gbt7714_code(self) -> &'static str {
        match self {
            ItemType::JournalArticle => "J",
            ItemType::ConferencePaper => "C",
            ItemType::Book | ItemType::BookChapter => "M",
            ItemType::Thesis => "D",
            ItemType::Report => "R",
            ItemType::Patent => "P",
            ItemType::Standard => "S",
            ItemType::Dataset => "DS",
            ItemType::Webpage => "EB",
            ItemType::Preprint | ItemType::Other => "Z",
        }
    }
}

/// A work in the library: a paper, book, thesis, standard…
///
/// Field names follow CSL-JSON where one exists (see `csl.rs`). Everything
/// but the ID, type and title is optional, and new fields must stay so: the
/// store keeps items as JSON, so old records must still read.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    pub id: ItemId,
    #[serde(rename = "type")]
    pub item_type: ItemType,
    pub title: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub creators: Vec<Creator>,
    /// Journal, proceedings or book title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_title: Option<String>,
    /// Abbreviated journal title, e.g. `Nat. Commun.`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_short_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    /// The full publication date when known: `2023`, `2023-05` or `2023-05-12`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// Place of publication (GB/T 7714 prints it for books).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edition: Option<String>,
    /// Report, standard or patent number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doi: Option<Doi>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isbn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arxiv_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pmid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// When a web page was read, `YYYY-MM-DD` (GB/T 7714 “引用日期”).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstract_note: Option<String>,
    /// Zotero's Extra field: free `Key: value` lines kept by plugins and other tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// BCP 47 language tag, e.g. `zh-CN` or `en`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// How many works cite it, as OpenAlex last counted them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cited_by: Option<u32>,
}

impl Item {
    pub fn new(item_type: ItemType, title: impl Into<String>) -> Self {
        Self { id: ItemId::new(), item_type, title: title.into(), ..Self::default() }
    }

    /// True if the item is written in Chinese, Japanese or Korean: an
    /// explicit language tag, or else CJK characters in the title.
    pub fn is_cjk(&self) -> bool {
        match self.language.as_deref() {
            Some(tag) => matches!(tag.split(['-', '_']).next(), Some("zh" | "ja" | "ko")),
            None => crate::contains_cjk(&self.title),
        }
    }

    /// The authors as a short label for lists: `Curie`, `Curie and Joliot`,
    /// `Curie et al.`; Chinese names use `、` and `等`.
    pub fn authors_short(&self) -> String {
        let names: Vec<&str> = self
            .creators
            .iter()
            .filter(|c| c.role == crate::CreatorRole::Author)
            .map(|c| c.short_name())
            .filter(|n| !n.is_empty())
            .collect();
        let cjk = names.first().is_some_and(|n| crate::contains_cjk(n));
        match (names.as_slice(), cjk) {
            ([], _) => String::new(),
            ([one], _) => (*one).to_string(),
            ([a, b], false) => format!("{a} and {b}"),
            ([a, b], true) => format!("{a}、{b}"),
            ([a, ..], false) => format!("{a} et al."),
            ([a, ..], true) => format!("{a} 等"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_type_ids_round_trip() {
        for t in ItemType::ALL {
            assert_eq!(ItemType::parse(t.as_str()), Some(t));
            let json = serde_json::to_string(&t).unwrap();
            assert_eq!(json, format!("\"{}\"", t.as_str()));
        }
        assert_eq!(ItemType::parse("nope"), None);
    }

    #[test]
    fn maps_to_csl_and_gbt7714() {
        assert_eq!(ItemType::JournalArticle.csl_type(), "article-journal");
        assert_eq!(ItemType::JournalArticle.gbt7714_code(), "J");
        assert_eq!(ItemType::Thesis.gbt7714_code(), "D");
        assert_eq!(ItemType::Standard.gbt7714_code(), "S");
        assert_eq!(ItemType::Webpage.gbt7714_code(), "EB");
    }

    #[test]
    fn short_author_labels() {
        let mut item = Item::new(ItemType::JournalArticle, "Radioactivity");
        assert_eq!(item.authors_short(), "");
        item.creators.push(Creator::person("Curie", "Marie"));
        assert_eq!(item.authors_short(), "Curie");
        item.creators.push(Creator::person("Curie", "Pierre"));
        assert_eq!(item.authors_short(), "Curie and Curie");
        item.creators.push(Creator::person("Becquerel", "Henri"));
        assert_eq!(item.authors_short(), "Curie et al.");

        let mut zh = Item::new(ItemType::JournalArticle, "研究");
        zh.creators = vec![Creator::literal("张三"), Creator::literal("李四")];
        assert_eq!(zh.authors_short(), "张三、李四");
        zh.creators.push(Creator::literal("王五"));
        assert_eq!(zh.authors_short(), "张三 等");
    }

    #[test]
    fn detects_cjk_items() {
        assert!(Item::new(ItemType::JournalArticle, "基于深度学习的研究").is_cjk());
        assert!(!Item::new(ItemType::JournalArticle, "Deep learning").is_cjk());
        let mut tagged = Item::new(ItemType::Book, "Kokoro");
        tagged.language = Some("ja-JP".into());
        assert!(tagged.is_cjk());
    }

    #[test]
    fn reads_records_written_before_new_fields() {
        let old = r#"{"id":"x","type":"book","title":"Old","year":1999}"#;
        let item: Item = serde_json::from_str(old).unwrap();
        assert_eq!(item.title, "Old");
        assert_eq!(item.publisher, None);
        assert_eq!(item.extra, None);
        assert!(item.keywords.is_empty());
        assert!(serde_json::to_value(&item).unwrap().get("extra").is_none());
    }

    #[test]
    fn serializes_with_csl_like_keys() {
        let mut item = Item::new(ItemType::Book, "On Growth and Form");
        item.year = Some(1917);
        item.extra = Some("Read_Status: Read\nCitation Key: growth1917".into());
        let value = serde_json::to_value(&item).unwrap();
        assert_eq!(value["type"], "book");
        assert_eq!(value["year"], 1917);
        assert!(value.get("doi").is_none());
        let back: Item = serde_json::from_value(value).unwrap();
        assert_eq!(back, item);
    }
}
