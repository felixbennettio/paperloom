//! Citation styles and formatted references for the apps.

use paperloom_citation as citation;
use paperloom_model::Item;
use serde::{Deserialize, Serialize};

use crate::CoreError;

impl From<citation::Error> for CoreError {
    fn from(err: citation::Error) -> Self {
        CoreError::InvalidInput { message: err.to_string() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct CitationStyle {
    pub id: String,
    pub name: String,
    /// Downloaded or added from a file (and so removable), not built in.
    pub added: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct CitationOutput {
    /// One reference list entry per item.
    pub entries: Vec<String>,
    /// The in-text citation for all the items together, e.g. `[1-2]`.
    pub citation: String,
}

/// The citation styles the apps offer: the built-in ones, GB/T 7714 first,
/// then those added (an added style with a built-in one's id replaces it).
#[uniffi::export]
pub fn citation_styles() -> Vec<CitationStyle> {
    let mut styles: Vec<CitationStyle> = citation::STYLES
        .iter()
        .map(|s| CitationStyle { id: s.id.to_string(), name: s.name.to_string(), added: false })
        .collect();
    for (id, name) in citation::added_styles() {
        match styles.iter_mut().find(|s| s.id == id) {
            Some(built_in) => built_in.added = true,
            None => styles.push(CitationStyle { id, name, added: true }),
        }
    }
    styles
}

pub(crate) fn format(items: &[Item], style_id: &str, html: bool) -> Result<CitationOutput, CoreError> {
    let format = if html { citation::Format::Html } else { citation::Format::Text };
    let out = citation::format(items, style_id, format)?;
    Ok(CitationOutput { entries: out.entries, citation: out.citation })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewItem, PaperLibrary};

    #[test]
    fn lists_styles_with_gb_t_first() {
        let styles = citation_styles();
        assert_eq!(styles[0].id, "gb-7714-2015-numeric");
        assert!(styles.iter().any(|s| s.id == "apa"));
    }

    #[test]
    fn formats_items_of_a_library() {
        let lib = PaperLibrary::in_memory().unwrap();
        let item = lib
            .add_item(NewItem {
                title: "Radioactivity".into(),
                authors: vec!["Curie, Marie".into()],
                container_title: Some("Comptes rendus".into()),
                year: Some(1903),
                ..NewItem::default()
            })
            .unwrap();
        let out = lib.format_citations(vec![item.id.clone()], "apa".into(), false).unwrap();
        assert_eq!(out.entries.len(), 1);
        assert!(out.entries[0].contains("Curie"), "{}", out.entries[0]);
        assert!(matches!(
            lib.format_citations(vec!["missing".into()], "apa".into(), false),
            Err(CoreError::NotFound { .. })
        ));
        assert!(matches!(
            lib.format_citations(vec![item.id], "nope".into(), false),
            Err(CoreError::InvalidInput { .. })
        ));
    }
}
