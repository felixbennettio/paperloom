//! Styles added to the built-in ones: CSL files from the style repository
//! or the person's own. A dependent style (a journal that cites like
//! another style) uses its parent's rules, so the parent must be built in
//! or added first.

use std::collections::BTreeMap;
use std::sync::{PoisonError, RwLock};

use hayagriva::archive::ArchivedStyle;
use hayagriva::citationberg::{IndependentStyle, Style};

use crate::{Error, Result};

struct Added {
    name: String,
    style: IndependentStyle,
}

static ADDED: RwLock<BTreeMap<String, Added>> = RwLock::new(BTreeMap::new());

/// What a CSL file holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleFile {
    /// The id it gives itself, as a short name: `nature-communications` for
    /// `http://www.zotero.org/styles/nature-communications`.
    pub id: String,
    pub name: String,
    /// For a dependent style, the short name of the style it cites like.
    pub parent: Option<String>,
}

/// The last part of a style URL, kept to letters, digits and `-`.
pub fn short_name(url: &str) -> String {
    let last = url.trim_end_matches('/').rsplit('/').next().unwrap_or(url);
    let mut name = String::new();
    for c in last.trim_end_matches(".csl").chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            name.push(c);
        } else if !name.is_empty() && !name.ends_with('-') {
            name.push('-');
        }
    }
    name.trim_end_matches('-').to_string()
}

fn parse(xml: &str) -> Result<Style> {
    Style::from_xml(xml).map_err(|e| Error::InvalidStyle(e.to_string()))
}

/// Reads a CSL file without adding it.
pub fn read_style(xml: &str) -> Result<StyleFile> {
    Ok(match parse(xml)? {
        Style::Independent(style) => {
            StyleFile { id: short_name(&style.info.id), name: style.info.title.value, parent: None }
        }
        Style::Dependent(style) => StyleFile {
            id: short_name(&style.info.id),
            name: style.info.title.value,
            parent: Some(short_name(&style.parent_link.href)),
        },
    })
}

/// Whether `id` names a style that can be used: built in or added.
pub fn has_style(id: &str) -> bool {
    ADDED.read().unwrap_or_else(PoisonError::into_inner).contains_key(id)
        || ArchivedStyle::by_name(id).is_some()
}

/// The style `id`: one added, else one built in.
pub(crate) fn find(id: &str) -> Option<IndependentStyle> {
    if let Some(added) = ADDED.read().unwrap_or_else(PoisonError::into_inner).get(id) {
        return Some(added.style.clone());
    }
    match ArchivedStyle::by_name(id).map(ArchivedStyle::get) {
        Some(Style::Independent(style)) => Some(style),
        _ => None,
    }
}

/// Adds a style under `id`, in place of one added before; returns its
/// name.
pub fn add_style(id: &str, xml: &str) -> Result<String> {
    let (name, style) = match parse(xml)? {
        Style::Independent(style) => (style.info.title.value.clone(), style),
        Style::Dependent(dependent) => {
            let parent = short_name(&dependent.parent_link.href);
            let mut style = find(&parent).ok_or(Error::MissingParent(parent))?;
            if dependent.default_locale.is_some() {
                style.default_locale = dependent.default_locale;
            }
            (dependent.info.title.value, style)
        }
    };
    ADDED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id.to_string(), Added { name: name.clone(), style });
    Ok(name)
}

pub fn remove_style(id: &str) {
    ADDED.write().unwrap_or_else(PoisonError::into_inner).remove(id);
}

/// The added styles as (id, name), by name.
pub fn added_styles() -> Vec<(String, String)> {
    let added = ADDED.read().unwrap_or_else(PoisonError::into_inner);
    let mut styles: Vec<(String, String)> =
        added.iter().map(|(id, a)| (id.clone(), a.name.clone())).collect();
    styles.sort_by_cached_key(|(_, name)| name.to_lowercase());
    styles
}
