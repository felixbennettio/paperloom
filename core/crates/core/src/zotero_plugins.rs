//! Zotero 7 plugins: bootstrap `.xpi` files, kept unpacked beside the
//! library in `plugins/zotero/<id>/`, with `plugins/zotero/enabled.json`
//! listing the ones turned on and `<id>.prefs.json` their preferences (kept
//! when a plugin is updated). The runtime (`core/plugin-runtime/zotero.js`)
//! gives them the `Zotero` object; the library's items reach them in
//! Zotero's JSON shape, and their changes come back the same way.

use std::fs;
use std::path::{Component, Path, PathBuf};

use paperloom_model::{
    Annotation, AnnotationId, AnnotationKind, Attachment, Creator, CreatorRole, Doi, Item, ItemId, ItemType,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{CoreError, PaperLibrary, unzip};

/// A Zotero plugin installed for the library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct ZoteroPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub homepage: String,
    pub enabled: bool,
}

/// A text file of a plugin, by its path inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct PluginFile {
    pub path: String,
    pub text: String,
}

/// What it takes to run a Zotero plugin: its text files (scripts, panes,
/// locales), `bootstrap.js` among them, and its saved preferences (JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct ZoteroPluginCode {
    pub files: Vec<PluginFile>,
    pub prefs: Option<String>,
}

/// The largest text file a plugin's runtime is given.
const MAX_TEXT: u64 = 16 << 20;
const TEXT_KINDS: [&str; 12] =
    ["js", "mjs", "jsm", "xhtml", "html", "htm", "css", "ftl", "properties", "json", "dtd", "svg"];

fn storage(what: &str, e: impl std::fmt::Display) -> CoreError {
    CoreError::Storage { message: format!("{what} failed: {e}") }
}

/// A plugin id as a folder name: letters, digits, `@`, `.`, `-`, `_`.
pub(crate) fn checked_id(id: &str) -> Result<&str, CoreError> {
    let plain = !id.is_empty()
        && !id.starts_with('.')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '-' | '_'));
    if plain { Ok(id) } else { Err(CoreError::InvalidInput { message: format!("{id} is not a plugin id") }) }
}

/// What a plugin's `manifest.json` says, its id from `applications.zotero`.
fn read_manifest(text: &str) -> Result<(ZoteroPlugin, String), CoreError> {
    let manifest: Value = serde_json::from_str(text)
        .map_err(|e| CoreError::InvalidInput { message: format!("The plugin's manifest: {e}") })?;
    let id = ["applications", "browser_specific_settings"]
        .iter()
        .find_map(|key| manifest[key]["zotero"]["id"].as_str())
        .ok_or_else(|| CoreError::InvalidInput {
            message: "This is not a Zotero 7 plugin (no Zotero id).".into(),
        })?;
    let string = |key: &str| manifest[key].as_str().unwrap_or_default().to_string();
    let author = match &manifest["author"] {
        Value::String(name) => name.clone(),
        Value::Object(person) => person.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
        _ => String::new(),
    };
    let plugin = ZoteroPlugin {
        id: checked_id(id)?.to_string(),
        name: Some(string("name")).filter(|n| !n.is_empty()).unwrap_or_else(|| id.to_string()),
        version: string("version"),
        description: string("description"),
        author,
        homepage: string("homepage_url"),
        enabled: false,
    };
    Ok((plugin, id.to_string()))
}

/// A name from an archive as a path under `root`, if it stays there.
fn inside(root: &Path, name: &str) -> Option<PathBuf> {
    let relative = Path::new(name);
    let plain = !name.contains('\\') && relative.components().all(|c| matches!(c, Component::Normal(_)));
    plain.then(|| root.join(relative))
}

/// The text files under a folder, by `/`-separated path.
fn text_files(root: &Path) -> Vec<PluginFile> {
    let mut found = Vec::new();
    let mut folders = vec![(String::new(), root.to_path_buf())];
    while let Some((prefix, folder)) = folders.pop() {
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                folders.push((path, entry.path()));
                continue;
            }
            let extension =
                Path::new(&name).extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
            let small = entry.metadata().is_ok_and(|m| m.len() <= MAX_TEXT);
            if kind.is_file()
                && small
                && TEXT_KINDS.contains(&extension.as_str())
                && let Ok(text) = fs::read_to_string(entry.path())
            {
                found.push(PluginFile { path, text });
            }
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

/// Zotero's name for an item type.
fn zotero_type(item_type: ItemType) -> &'static str {
    match item_type {
        ItemType::JournalArticle => "journalArticle",
        ItemType::ConferencePaper => "conferencePaper",
        ItemType::Book => "book",
        ItemType::BookChapter => "bookSection",
        ItemType::Thesis => "thesis",
        ItemType::Report => "report",
        ItemType::Preprint => "preprint",
        ItemType::Patent => "patent",
        ItemType::Standard => "standard",
        ItemType::Dataset => "dataset",
        ItemType::Webpage => "webpage",
        ItemType::Other => "document",
    }
}

/// An item as Zotero's API gives it to plugins.
fn item_json(item: &Item, tags: &[String], attachments: &[Attachment]) -> Value {
    let text = |value: &Option<String>| value.clone().unwrap_or_default();
    let creators: Vec<Value> = item
        .creators
        .iter()
        .map(|c| {
            let role = serde_json::to_value(c.role).unwrap_or(json!("author"));
            if c.literal.is_empty() {
                json!({ "firstName": c.given, "lastName": c.family, "creatorType": role })
            } else {
                json!({ "name": c.literal, "creatorType": role })
            }
        })
        .collect();
    json!({
        "key": item.id.to_string(),
        "itemType": zotero_type(item.item_type),
        "title": item.title,
        "creators": creators,
        "date": item.date.clone().or_else(|| item.year.map(|y| y.to_string())).unwrap_or_default(),
        "DOI": item.doi.as_ref().map(|d| d.as_str().to_string()).unwrap_or_default(),
        "url": text(&item.url),
        "abstractNote": text(&item.abstract_note),
        "extra": text(&item.extra),
        "publicationTitle": text(&item.container_title),
        "journalAbbreviation": text(&item.container_short_title),
        "volume": text(&item.volume),
        "issue": text(&item.issue),
        "pages": text(&item.pages),
        "publisher": text(&item.publisher),
        "place": text(&item.place),
        "edition": text(&item.edition),
        "ISBN": text(&item.isbn),
        "ISSN": text(&item.issn),
        "language": text(&item.language),
        "tags": tags.iter().map(|t| json!({ "tag": t })).collect::<Vec<_>>(),
        "attachments": attachments.iter().map(|a| json!({
            "key": a.id.to_string(), "contentType": a.content_type, "title": a.title
        })).collect::<Vec<_>>(),
    })
}

/// Zotero SQL dates in UTC. Gregorian civil-date conversion from Unix days;
/// keep this adapter independent of the annotation storage representation.
pub(crate) fn sql_date(milliseconds: i64) -> String {
    let seconds = milliseconds.div_euclid(1000);
    let days = seconds.div_euclid(86400) + 719468;
    let era = days.div_euclid(146097);
    let day = days - era * 146097;
    let year = (day - day / 1460 + day / 36524 - day / 146096) / 365;
    let ordinal = day - (365 * year + year / 4 - year / 100);
    let month = (5 * ordinal + 2) / 153;
    let date = ordinal - (153 * month + 2) / 5 + 1;
    let month = month + if month < 10 { 3 } else { -9 };
    let year = year + era * 400 + i64::from(month <= 2);
    let time = seconds.rem_euclid(86400);
    format!("{year:04}-{month:02}-{date:02} {:02}:{:02}:{:02}", time / 3600, time / 60 % 60, time % 60)
}

const MAX_ANNOTATION_POSITION: usize = 65_000;

/// Unsupported kinds or invalid/oversized geometry are counted on the parent
/// attachment, never truncated or changed into a different annotation type.
fn annotation_json(mark: &Annotation, deleted: bool) -> Option<Value> {
    if !matches!(
        mark.kind,
        AnnotationKind::Highlight
            | AnnotationKind::Underline
            | AnnotationKind::Note
            | AnnotationKind::Ink
            | AnnotationKind::Text
    ) || !paperloom_model::is_color(&mark.color)
        || mark
            .rects
            .iter()
            .any(|r| !r.is_valid() || !(r.x + r.width).is_finite() || !(r.y + r.height).is_finite())
        || mark.strokes.iter().flatten().any(|p| !p.x.is_finite() || !p.y.is_finite())
        || mark.font_size.is_some_and(|size| !size.is_finite() || size <= 0.0)
    {
        return None;
    }
    let mut position = json!({ "pageIndex": mark.page_index });
    let (x, top) = if mark.kind == AnnotationKind::Ink {
        if mark.strokes.is_empty() || mark.strokes.iter().any(|stroke| stroke.len() < 2) {
            return None;
        }
        position["paths"] = json!(
            mark.strokes
                .iter()
                .map(|stroke| { stroke.iter().flat_map(|p| [p.x, p.y]).collect::<Vec<_>>() })
                .collect::<Vec<_>>()
        );
        // Paperloom stores no stroke width; Zotero readers need a fixed default.
        position["width"] = json!(1);
        mark.strokes
            .iter()
            .flatten()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(x, y), p| (x.min(p.x), y.max(p.y)))
    } else {
        if mark.rects.is_empty() {
            return None;
        }
        position["rects"] =
            json!(mark.rects.iter().map(|r| [r.x, r.y, r.x + r.width, r.y + r.height]).collect::<Vec<_>>());
        if mark.kind == AnnotationKind::Text {
            position["fontSize"] = json!(mark.font_size.unwrap_or(14.0));
        }
        mark.rects
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(x, y), r| (x.min(r.x), y.max(r.y + r.height)))
    };
    let position = position.to_string();
    if position.len() > MAX_ANNOTATION_POSITION {
        return None;
    }
    // Geometric order, top to bottom then left to right. Exact text offsets
    // are unavailable. Fixed-width bounded buckets keep lexical order stable.
    let order = ((100_000.0 - top) * 10.0).clamp(0.0, 999_999.0) as u32;
    let horizontal = (x * 10.0).clamp(0.0, 99_999.0) as u32;
    Some(json!({
        "key": mark.id.to_string(), "itemType": "annotation",
        "parentItem": mark.attachment_id.to_string(), "deleted": deleted,
        "annotationType": mark.kind.as_str(),
        "annotationText": mark.text.as_deref().unwrap_or_default(),
        "annotationComment": mark.comment.as_deref().unwrap_or_default(),
        "annotationColor": mark.color.to_ascii_lowercase(),
        "annotationPosition": position,
        "annotationPageLabel": (u64::from(mark.page_index) + 1).to_string(),
        "annotationSortIndex": format!("{:05}|{order:06}|{horizontal:05}", mark.page_index),
        "dateAdded": sql_date(mark.date_added), "dateModified": sql_date(mark.date_modified),
        "tags": mark.tags.iter().map(|tag| json!({ "tag": tag, "type": 0 })).collect::<Vec<_>>(),
    }))
}

/// Validate the entire patch before touching storage. Count Unicode characters in tags.
fn annotation_tags(value: &Value) -> Result<Vec<String>, CoreError> {
    let invalid = || CoreError::InvalidInput { message: "Invalid annotation field tags.".into() };
    let entries = value.as_array().filter(|entries| entries.len() <= 100).ok_or_else(invalid)?;
    let mut tags = Vec::new();
    for entry in entries {
        let tag =
            entry.as_str().filter(|tag| (1..=255).contains(&tag.chars().count())).ok_or_else(invalid)?;
        if !tags.iter().any(|existing| existing == tag) {
            tags.push(tag.to_string());
        }
    }
    Ok(tags)
}

/// Validates Zotero's person/organisation JSON before any item or tags are saved.
fn parse_creators(value: &Value) -> Result<Vec<Creator>, CoreError> {
    let invalid = || CoreError::InvalidInput { message: "Invalid Zotero creators.".into() };
    let entries = value.as_array().filter(|entries| entries.len() <= 500).ok_or_else(invalid)?;
    entries
        .iter()
        .map(|entry| {
            let entry = entry.as_object().ok_or_else(invalid)?;
            // Bound UTF-8 bytes, including for unknown creator types.
            let text = |key: &str, limit: usize| {
                entry.get(key).and_then(Value::as_str).filter(|s| s.len() <= limit).ok_or_else(invalid)
            };
            let role = text("creatorType", 64)?;
            let role = serde_json::from_value::<CreatorRole>(json!(role)).unwrap_or_default();
            let mut creator = if entry.contains_key("name") {
                if entry.contains_key("firstName") || entry.contains_key("lastName") {
                    return Err(invalid());
                }
                Creator::literal(text("name", 1024)?)
            } else {
                Creator::person(text("lastName", 1024)?, text("firstName", 1024)?)
            };
            creator.role = role;
            Ok(creator)
        })
        .collect()
}

/// Puts a Zotero field's new value on an item; false for a field this
/// library does not keep.
fn set_field(item: &mut Item, field: &str, value: &str) -> bool {
    let value = value.trim();
    let optional = || Some(value.to_string()).filter(|v| !v.is_empty());
    match field {
        "title" if !value.is_empty() => item.title = value.to_string(),
        "abstractNote" => item.abstract_note = optional(),
        "extra" => item.extra = optional(),
        "date" => {
            item.date = optional();
            item.year = value.get(..4).and_then(|y| y.parse().ok());
        }
        "DOI" => item.doi = Doi::parse(value).ok(),
        "url" => item.url = optional(),
        "publicationTitle" => item.container_title = optional(),
        "journalAbbreviation" => item.container_short_title = optional(),
        "volume" => item.volume = optional(),
        "issue" => item.issue = optional(),
        "pages" => item.pages = optional(),
        "publisher" => item.publisher = optional(),
        "place" => item.place = optional(),
        "edition" => item.edition = optional(),
        "ISBN" => item.isbn = optional(),
        "ISSN" => item.issn = optional(),
        "language" => item.language = optional(),
        _ => return false,
    }
    true
}

impl PaperLibrary {
    fn zotero_plugins_dir(&self) -> Result<PathBuf, CoreError> {
        self.plugins_dir.as_ref().map(|dir| dir.join("zotero")).ok_or_else(|| CoreError::Storage {
            message: "This library is kept only in memory and cannot keep plugins.".into(),
        })
    }

    fn enabled_zotero_plugins(&self) -> Vec<String> {
        self.zotero_plugins_dir()
            .ok()
            .and_then(|dir| fs::read_to_string(dir.join("enabled.json")).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }
}

#[uniffi::export]
impl PaperLibrary {
    /// The Zotero plugins installed for the library, by name.
    pub fn zotero_plugins(&self) -> Result<Vec<ZoteroPlugin>, CoreError> {
        let dir = self.zotero_plugins_dir()?;
        let enabled = self.enabled_zotero_plugins();
        let Ok(entries) = fs::read_dir(&dir) else { return Ok(Vec::new()) };
        let mut found: Vec<ZoteroPlugin> = entries
            .flatten()
            .filter(|e| e.path().is_dir() && e.path().join("bootstrap.js").is_file())
            .filter_map(|e| read_manifest(&fs::read_to_string(e.path().join("manifest.json")).ok()?).ok())
            .map(|(mut plugin, _)| {
                plugin.enabled = enabled.contains(&plugin.id);
                plugin
            })
            .collect();
        found.sort_by_key(|p| p.name.to_lowercase());
        Ok(found)
    }

    /// Installs (or updates) a Zotero 7 plugin from its `.xpi` file; it
    /// stays off until turned on, and keeps its preferences.
    pub fn install_zotero_plugin(&self, xpi: String) -> Result<ZoteroPlugin, CoreError> {
        let bytes = fs::read(&xpi).map_err(|e| storage(&format!("Reading {xpi}"), e))?;
        let entries = unzip::entries(&bytes).ok_or_else(|| CoreError::InvalidInput {
            message: format!("{xpi} is not a plugin file (.xpi)"),
        })?;
        let manifest = entries
            .iter()
            .find(|e| e.name == "manifest.json")
            .and_then(|e| e.contents(MAX_TEXT))
            .ok_or_else(|| CoreError::InvalidInput { message: "The plugin has no manifest.json.".into() })?;
        let (mut plugin, id) = read_manifest(&String::from_utf8_lossy(&manifest))?;
        if !entries.iter().any(|e| e.name == "bootstrap.js") {
            return Err(CoreError::InvalidInput {
                message: "Only Zotero 7 plugins (with bootstrap.js) can be installed.".into(),
            });
        }
        let dir = self.zotero_plugins_dir()?;
        let partial = dir.join(format!("{id}.partial"));
        let _ = fs::remove_dir_all(&partial);
        for entry in entries.iter().filter(|e| !e.is_folder() && !e.name.starts_with("META-INF/")) {
            let Some(target) = inside(&partial, &entry.name) else { continue };
            let content = entry.contents(64 << 20).ok_or_else(|| CoreError::InvalidInput {
                message: format!("{} in the plugin could not be unpacked.", entry.name),
            })?;
            if let Some(folder) = target.parent() {
                fs::create_dir_all(folder).map_err(|e| storage("Installing the plugin", e))?;
            }
            fs::write(&target, content).map_err(|e| storage("Installing the plugin", e))?;
        }
        let target = dir.join(&id);
        let _ = fs::remove_dir_all(&target);
        fs::rename(&partial, &target).map_err(|e| storage("Installing the plugin", e))?;
        plugin.enabled = self.enabled_zotero_plugins().contains(&id);
        Ok(plugin)
    }

    pub fn set_zotero_plugin_enabled(&self, id: String, enabled: bool) -> Result<(), CoreError> {
        let id = checked_id(&id)?.to_string();
        let mut list = self.enabled_zotero_plugins();
        list.retain(|other| *other != id);
        if enabled {
            list.push(id);
        }
        let dir = self.zotero_plugins_dir()?;
        fs::create_dir_all(&dir).map_err(|e| storage("Saving the plugins", e))?;
        let text = serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".into());
        fs::write(dir.join("enabled.json"), text).map_err(|e| storage("Saving the plugins", e))
    }

    pub fn zotero_plugin_code(&self, id: String) -> Result<ZoteroPluginCode, CoreError> {
        let dir = self.zotero_plugins_dir()?;
        let folder = dir.join(checked_id(&id)?);
        if !folder.join("bootstrap.js").is_file() {
            return Err(CoreError::not_found(format!("the plugin {id}")));
        }
        Ok(ZoteroPluginCode {
            files: text_files(&folder),
            prefs: fs::read_to_string(dir.join(format!("{id}.prefs.json"))).ok(),
        })
    }

    /// Keeps a plugin's preferences (JSON).
    pub fn save_zotero_plugin_prefs(&self, id: String, prefs: String) -> Result<(), CoreError> {
        if serde_json::from_str::<Value>(&prefs).is_err() {
            return Err(CoreError::InvalidInput { message: format!("{id}'s preferences are not JSON") });
        }
        let dir = self.zotero_plugins_dir()?;
        fs::create_dir_all(&dir).map_err(|e| storage("Saving the preferences", e))?;
        fs::write(dir.join(format!("{}.prefs.json", checked_id(&id)?)), prefs)
            .map_err(|e| storage("Saving the preferences", e))
    }

    /// The library's items as Zotero's API gives them to plugins (JSON).
    pub fn zotero_items(&self) -> Result<String, CoreError> {
        let store = self.lock()?;
        let mut items = Vec::new();
        for (item, deleted) in store
            .items()?
            .into_iter()
            .map(|item| (item, false))
            .chain(store.trashed_items()?.into_iter().map(|item| (item, true)))
        {
            let attachments = store.attachments(&item.id)?;
            let mut data = item_json(&item, &store.item_tags(&item.id)?, &attachments);
            if deleted {
                data["deleted"] = json!(true);
            }
            items.push(data);
            for attachment in attachments {
                let mut annotations = Vec::new();
                let mut unsupported = 0;
                for mark in store.annotations(&attachment.id)? {
                    if let Some(data) = annotation_json(&mark, deleted) {
                        annotations.push(data);
                    } else {
                        unsupported += 1;
                    }
                }
                items.push(json!({
                    "key": attachment.id.to_string(), "itemType": "attachment",
                    "parentItem": item.id.to_string(), "deleted": deleted,
                    "contentType": attachment.content_type, "title": attachment.title,
                    "unsupportedAnnotations": unsupported,
                }));
                items.extend(annotations);
            }
        }
        Ok(Value::Array(items).to_string())
    }

    /// A plugin's change to an item: Zotero fields (including creators) and,
    /// if given, its tags.
    pub fn zotero_update_item(
        &self,
        key: String,
        fields: String,
        tags: Option<Vec<String>>,
    ) -> Result<(), CoreError> {
        let fields: serde_json::Map<String, Value> = serde_json::from_str(&fields)
            .map_err(|e| CoreError::InvalidInput { message: format!("The plugin's change: {e}") })?;
        let id = ItemId::from_string(&key);
        {
            let mut store = self.lock()?;
            if let Some(mut annotation) = store.annotation(&AnnotationId::from_string(&key))? {
                let before = annotation.clone();
                let invalid = |field: &str| CoreError::InvalidInput {
                    message: format!("Invalid annotation field {field}."),
                };
                for (field, value) in &fields {
                    match field.as_str() {
                        "annotationComment" => {
                            let value = value
                                .as_str()
                                .filter(|value| value.len() <= 64 * 1024)
                                .ok_or_else(|| invalid(field))?;
                            annotation.comment = (!value.is_empty()).then(|| value.to_string());
                        }
                        "annotationColor" => {
                            let value = value
                                .as_str()
                                .filter(|value| paperloom_model::is_color(value))
                                .ok_or_else(|| invalid(field))?;
                            annotation.color = value.to_ascii_lowercase();
                        }
                        "tags" => {
                            annotation.tags = annotation_tags(value)?;
                        }
                        _ => {
                            return Err(CoreError::InvalidInput {
                                message: format!("Annotation field {field} writes are not supported."),
                            });
                        }
                    }
                }
                if let Some(tags) = tags {
                    if fields.contains_key("tags") {
                        return Err(invalid("tags"));
                    }
                    annotation.tags = annotation_tags(&json!(tags))?;
                }
                // Tags are a set: a reordered/deduplicated save is still a no-op.
                if annotation.tags.len() == before.tags.len()
                    && annotation.tags.iter().all(|tag| before.tags.contains(tag))
                {
                    annotation.tags.clone_from(&before.tags);
                }
                if annotation != before {
                    annotation.date_modified = paperloom_store::now_ms();
                    store.update_annotation(&annotation)?;
                    drop(store);
                    self.index_annotation(&annotation);
                }
                return Ok(());
            }
            let creators = fields.get("creators").map(parse_creators).transpose()?;
            let mut item = store.item(&id)?.ok_or_else(|| CoreError::not_found(format!("item {key}")))?;
            let mut changed = false;
            for (field, value) in &fields {
                changed |= set_field(&mut item, field, value.as_str().unwrap_or_default());
            }
            if let Some(creators) = creators {
                item.creators = creators;
                changed = true;
            }
            if changed {
                store.update_item(&item)?;
            }
            if let Some(tags) = tags {
                store.set_item_tags(&id, &tags)?;
            }
        }
        self.reindex_items(std::slice::from_ref(&id));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewItem;

    fn annotation_fixture() -> Annotation {
        Annotation {
            id: AnnotationId::new(),
            attachment_id: paperloom_model::AttachmentId::new(),
            kind: AnnotationKind::Highlight,
            color: "#AbCDEF".into(),
            page_index: 2,
            rects: vec![paperloom_model::Rect { x: 10.0, y: 20.0, width: 30.0, height: 4.0 }],
            text: Some("  quote\n".into()),
            comment: Some("comment".into()),
            tags: Vec::new(),
            strokes: vec![],
            font_size: None,
            date_added: 0,
            date_modified: 1_709_251_199_000,
        }
    }

    #[test]
    fn annotation_patches_are_atomic_bounded_and_preserve_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let library = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
        let pdf = dir.path().join("paper.pdf");
        fs::write(&pdf, b"%PDF-1.7").unwrap();
        let imported = library.import_pdf(pdf.display().to_string(), String::new(), None, None).unwrap();
        let mut mark = annotation_fixture();
        mark.attachment_id = paperloom_model::AttachmentId::from_string(imported.attachment.id);
        library.lock().unwrap().add_annotation(&mark).unwrap();
        let patch = |fields: Value, tags: Option<Vec<String>>| {
            library.zotero_update_item(mark.id.to_string(), fields.to_string(), tags)
        };
        patch(
            json!({"annotationComment": "  translated\n", "annotationColor": "#ABCDEF"}),
            Some(vec!["#auto".into(), "#auto".into(), "翻译".into()]),
        )
        .unwrap();
        let saved = library.lock().unwrap().annotation(&mark.id).unwrap().unwrap();
        assert_eq!(saved.comment.as_deref(), Some("  translated\n"));
        assert_eq!(saved.color, "#abcdef");
        assert_eq!(saved.tags, ["#auto", "翻译"]);
        assert_eq!(saved.text, mark.text);
        assert_eq!(saved.rects, mark.rects);
        assert_eq!(
            annotation_json(&saved, false).unwrap()["tags"],
            json!([{"tag":"#auto","type":0},{"tag":"翻译","type":0}])
        );
        // No-op saves do not update timestamps, sync records, or projection fields.
        {
            let mut store = library.lock().unwrap();
            let records = store.changed_records().unwrap();
            store.forget_changed(&records).unwrap();
        }
        patch(
            json!({"annotationComment": "  translated\n", "annotationColor": "#ABCDEF"}),
            Some(saved.tags.clone()),
        )
        .unwrap();
        assert_eq!(library.lock().unwrap().annotation(&mark.id).unwrap().unwrap(), saved);
        patch(json!({}), Some(vec!["翻译".into(), "#auto".into(), "#auto".into()])).unwrap();
        assert_eq!(library.lock().unwrap().annotation(&mark.id).unwrap().unwrap(), saved);
        assert!(library.lock().unwrap().changed_records().unwrap().is_empty());
        for field in [
            "annotationText",
            "annotationPosition",
            "annotationType",
            "parentItem",
            "parentID",
            "deleted",
            "creators",
            "title",
        ] {
            let error = patch(
                json!({"annotationComment":"must not persist", field: "refused"}),
                Some(vec!["refused".into()]),
            )
            .unwrap_err();
            assert!(
                matches!(error, CoreError::InvalidInput { ref message } if message.contains(field)),
                "{error}"
            );
            assert_eq!(library.lock().unwrap().annotation(&mark.id).unwrap().unwrap(), saved);
        }
        for (fields, field) in [
            (json!({"annotationComment": "x".repeat(65537)}), "annotationComment"),
            (json!({"annotationComment": null}), "annotationComment"),
            (json!({"annotationColor": "red"}), "annotationColor"),
            (json!({"tags": vec!["x"; 101]}), "tags"),
            (json!({"tags": [""]}), "tags"),
            (json!({"tags": ["x".repeat(256)]}), "tags"),
            (json!({"tags": [1]}), "tags"),
            (json!({"tags": null}), "tags"),
        ] {
            let error = patch(fields, None).unwrap_err();
            assert!(matches!(error, CoreError::InvalidInput { ref message } if message.contains(field)));
            assert_eq!(library.lock().unwrap().annotation(&mark.id).unwrap().unwrap(), saved);
        }
        assert!(patch(json!({"annotationComment":"refused"}), Some(vec!["x".into(); 101])).is_err());
        assert_eq!(library.lock().unwrap().annotation(&mark.id).unwrap().unwrap(), saved);
        patch(json!({"annotationComment":"x".repeat(65536), "tags": vec!["界".repeat(255);100]}), None)
            .unwrap();
        patch(json!({"annotationComment":"", "tags":[]}), None).unwrap();
        let cleared = library.lock().unwrap().annotation(&mark.id).unwrap().unwrap();
        assert_eq!(cleared.comment, None);
        assert!(cleared.tags.is_empty());
    }

    #[test]
    fn zotero_annotation_projection_contract() {
        let mut mark = annotation_fixture();
        let data = annotation_json(&mark, false).unwrap();
        assert_eq!(data["key"], mark.id.to_string());
        assert_eq!(data["parentItem"], mark.attachment_id.to_string());
        assert_eq!(data["annotationText"], "  quote\n");
        assert_eq!(data["annotationColor"], "#abcdef");
        assert_eq!(data["annotationPageLabel"], "3");
        assert_eq!(data["annotationSortIndex"], "00002|999760|00100");
        assert_eq!(data["dateAdded"], "1970-01-01 00:00:00");
        assert_eq!(data["dateModified"], "2024-02-29 23:59:59");
        assert_eq!(data["tags"], json!([]));
        assert_eq!(
            serde_json::from_str::<Value>(data["annotationPosition"].as_str().unwrap()).unwrap(),
            json!({ "pageIndex": 2, "rects": [[10.0, 20.0, 40.0, 24.0]] })
        );
        mark.comment = Some("another".into());
        mark.color = "#ffffff".into();
        assert_eq!(
            annotation_json(&mark, false).unwrap()["annotationSortIndex"],
            data["annotationSortIndex"]
        );
        mark.rects[0].y += 1.0;
        assert!(
            annotation_json(&mark, false).unwrap()["annotationSortIndex"].as_str().unwrap()
                < data["annotationSortIndex"].as_str().unwrap()
        );
        for kind in AnnotationKind::ALL {
            mark.kind = kind;
            mark.strokes = vec![vec![
                paperloom_model::Point { x: 1.0, y: 2.0 },
                paperloom_model::Point { x: 3.0, y: 4.0 },
            ]];
            let projected = annotation_json(&mark, true);
            let supported = matches!(
                kind,
                AnnotationKind::Highlight
                    | AnnotationKind::Underline
                    | AnnotationKind::Note
                    | AnnotationKind::Ink
                    | AnnotationKind::Text
            );
            assert_eq!(projected.is_some(), supported, "{kind:?}");
            if let Some(data) = projected {
                assert_eq!(data["itemType"], "annotation");
                assert_eq!(data["annotationType"], kind.as_str());
                assert_eq!(data["deleted"], true);
                let position: Value =
                    serde_json::from_str(data["annotationPosition"].as_str().unwrap()).unwrap();
                if kind == AnnotationKind::Ink {
                    assert_eq!(
                        position,
                        json!({ "pageIndex": 2, "paths": [[1.0, 2.0, 3.0, 4.0]], "width": 1 })
                    );
                }
                if kind == AnnotationKind::Text {
                    assert_eq!(position["fontSize"], 14.0);
                }
            }
        }
        assert_eq!(sql_date(-1), "1969-12-31 23:59:59");
    }

    #[test]
    fn zotero_annotation_rejects_nonfinite_and_oversized_positions() {
        let mut mark = annotation_fixture();
        mark.rects[0].x = f64::NAN;
        assert!(annotation_json(&mark, false).is_none());
        mark = annotation_fixture();
        mark.font_size = Some(f64::INFINITY);
        assert!(annotation_json(&mark, false).is_none());
        mark = annotation_fixture();
        mark.rects = vec![mark.rects[0]; 5000];
        assert!(annotation_json(&mark, false).is_none());
        mark = annotation_fixture();
        mark.kind = AnnotationKind::Ink;
        mark.strokes = vec![vec![paperloom_model::Point { x: f64::INFINITY, y: 0.0 }; 2]];
        assert!(annotation_json(&mark, false).is_none());
    }

    #[test]
    fn gives_plugins_large_bundles_with_a_text_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = "x".repeat(6 << 20);
        fs::write(dir.path().join("bundle.js"), &bundle).unwrap();
        // Include the boundary, but skip anything larger.
        fs::write(dir.path().join("boundary.js"), "x".repeat(MAX_TEXT as usize)).unwrap();
        fs::write(dir.path().join("too-large.js"), "x".repeat(MAX_TEXT as usize + 1)).unwrap();
        let files = text_files(dir.path());
        assert_eq!(files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["boundary.js", "bundle.js"]);
        assert_eq!(files[1].text, bundle);
        assert_eq!(files[0].text.len(), 16 << 20);
    }

    #[test]
    fn installs_and_runs_zotero_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let library = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
        let manifest = r#"{"manifest_version": 2, "name": "Make It Red", "version": "1.2",
            "author": {"name": "Zotero"}, "applications": {"zotero": {"id": "make-it-red@zotero.org"}}}"#;
        let xpi = unzip::tests::deflated_zip(&[
            ("manifest.json", manifest.as_bytes()),
            ("bootstrap.js", b"function startup() {}".as_slice()),
            ("locale/en-US/make-it-red.ftl", b"title = Make It Red".as_slice()),
            ("META-INF/manifest.mf", b"signed".as_slice()),
            ("../escape.js", b"no".as_slice()),
        ]);
        let file = dir.path().join("make-it-red.xpi");
        fs::write(&file, &xpi).unwrap();
        let installed = library.install_zotero_plugin(file.display().to_string()).unwrap();
        assert_eq!(
            (installed.id.as_str(), installed.author.as_str(), installed.enabled),
            ("make-it-red@zotero.org", "Zotero", false)
        );
        library.set_zotero_plugin_enabled(installed.id.clone(), true).unwrap();
        assert!(library.zotero_plugins().unwrap()[0].enabled);
        let code = library.zotero_plugin_code(installed.id.clone()).unwrap();
        let paths: Vec<&str> = code.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["bootstrap.js", "locale/en-US/make-it-red.ftl", "manifest.json"]);
        assert!(!dir.path().join("escape.js").exists());
        library.save_zotero_plugin_prefs(installed.id.clone(), r#"{"color": "red"}"#.into()).unwrap();
        // Updating keeps the preferences.
        library.install_zotero_plugin(file.display().to_string()).unwrap();
        assert_eq!(
            library.zotero_plugin_code(installed.id).unwrap().prefs.as_deref(),
            Some(r#"{"color": "red"}"#)
        );

        let not_zotero = dir.path().join("other.xpi");
        fs::write(
            &not_zotero,
            unzip::tests::deflated_zip(&[("manifest.json", br#"{"name": "x"}"#.as_slice())]),
        )
        .unwrap();
        assert!(library.install_zotero_plugin(not_zotero.display().to_string()).is_err());
    }

    #[test]
    fn zotero_attachments_export_only_keys_types_and_titles() {
        let dir = tempfile::tempdir().unwrap();
        let library = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
        let item = library
            .add_item(NewItem {
                item_type: String::new(),
                title: "Paper".into(),
                authors: vec![],
                container_title: None,
                year: None,
                doi: None,
            })
            .unwrap();
        let path = dir.path().join("paper.pdf");
        fs::write(&path, b"%PDF-1.7 attachment projection test").unwrap();
        let file = library.attach_file(item.id.clone(), path.display().to_string()).unwrap();
        let projection: Value = serde_json::from_str(&library.zotero_items().unwrap()).unwrap();
        let expected = json!([{ "key": file.id, "contentType": "application/pdf", "title": "paper.pdf" }]);
        assert_eq!(projection[0]["attachments"], expected);
        let mut mark = annotation_fixture();
        mark.attachment_id = paperloom_model::AttachmentId::from_string(&file.id);
        let mut shape = mark.clone();
        shape.id = AnnotationId::new();
        shape.kind = AnnotationKind::Square;
        let mut oversized = mark.clone();
        oversized.id = AnnotationId::new();
        oversized.rects = vec![mark.rects[0]; 5000];
        {
            let mut store = library.lock().unwrap();
            store.add_annotation(&mark).unwrap();
            store.add_annotation(&shape).unwrap();
            store.add_annotation(&oversized).unwrap();
        }
        let records: Value = serde_json::from_str(&library.zotero_items().unwrap()).unwrap();
        assert_eq!(records.as_array().unwrap().len(), 3);
        assert_eq!(records[1]["itemType"], "attachment");
        assert_eq!(records[1]["parentItem"], item.id);
        assert_eq!(records[1]["unsupportedAnnotations"], 2);
        assert_eq!(records[2]["key"], mark.id.to_string());
        assert!(matches!(
            library.zotero_update_item(
                mark.id.to_string(),
                json!({ "annotationText": "refused" }).to_string(),
                Some(vec!["tag".into()])
            ),
            Err(CoreError::InvalidInput { .. })
        ));
        assert_eq!(library.lock().unwrap().annotation(&mark.id).unwrap().unwrap(), mark);
        library.trash_item(item.id).unwrap();
        let trashed: Value = serde_json::from_str(&library.zotero_items().unwrap()).unwrap();
        assert_eq!(trashed[0]["attachments"], expected);
        assert_eq!(trashed[0]["deleted"], true);
        assert_eq!(trashed[1]["deleted"], true);
        assert_eq!(trashed[2]["deleted"], true);
    }

    #[test]
    fn gives_plugins_items_and_takes_their_changes() {
        let library = PaperLibrary::in_memory().unwrap();
        let item = library
            .add_item(NewItem {
                item_type: String::new(),
                title: "Old title".into(),
                authors: vec!["Ada Lovelace".into()],
                container_title: Some("Nature".into()),
                year: Some(2013),
                doi: None,
            })
            .unwrap();
        let mut detail = library.item_detail(item.id.clone()).unwrap().unwrap();
        detail.extra = Some("Read_Status: Unread\nCitation Key: lovelace2013".into());
        let saved = library.update_item(detail).unwrap();
        let items: Value = serde_json::from_str(&library.zotero_items().unwrap()).unwrap();
        assert_eq!(items[0]["itemType"], "journalArticle");
        assert_eq!(items[0]["publicationTitle"], "Nature");
        assert_eq!(items[0]["creators"][0]["creatorType"], "author");
        assert_eq!(items[0]["extra"].as_str(), saved.extra.as_deref());
        library.trash_item(item.id.clone()).unwrap();
        let trashed: Value = serde_json::from_str(&library.zotero_items().unwrap()).unwrap();
        assert_eq!(trashed.as_array().unwrap().len(), 1);
        assert_eq!(trashed[0]["key"], item.id);
        assert_eq!(trashed[0]["deleted"], true);
        assert_eq!(trashed[0]["title"], items[0]["title"]);
        library.restore_item(item.id.clone()).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&library.zotero_items().unwrap()).unwrap(), items);
        library
            .zotero_update_item(
                item.id.clone(),
                r#"{"title": "New title", "date": "2014-02-01", "extra": "Read_Status: Read\n\nCitation Key: lovelace2014", "unknownField": "x"}"#.into(),
                Some(vec!["red".into()]),
            )
            .unwrap();
        let items: Value = serde_json::from_str(&library.zotero_items().unwrap()).unwrap();
        assert_eq!(
            (items[0]["title"].as_str(), items[0]["date"].as_str()),
            (Some("New title"), Some("2014-02-01"))
        );
        assert_eq!(items[0]["tags"][0]["tag"], "red");
        let extra = "Read_Status: Read\n\nCitation Key: lovelace2014";
        assert_eq!(items[0]["extra"], extra);
        assert_eq!(library.item_detail(item.id.clone()).unwrap().unwrap().extra.as_deref(), Some(extra));
        library.zotero_update_item(item.id.clone(), r#"{"extra": ""}"#.into(), None).unwrap();
        let items: Value = serde_json::from_str(&library.zotero_items().unwrap()).unwrap();
        assert_eq!(items[0]["extra"], "");
        assert_eq!(library.item_detail(item.id.clone()).unwrap().unwrap().extra, None);
        let creators = json!([
            { "firstName": "Grace", "lastName": "Hopper", "creatorType": "author" },
            { "name": "Test Institute", "creatorType": "contributor" },
            { "firstName": "Ada", "lastName": "Lovelace", "creatorType": "editor" },
            { "name": "Translator", "creatorType": "translator" },
            { "name": "Inventor", "creatorType": "inventor" },
        ]);
        library
            .zotero_update_item(item.id.clone(), json!({ "creators": creators }).to_string(), None)
            .unwrap();
        let mut expected = creators;
        expected[4]["creatorType"] = json!("author");
        let items: Value = serde_json::from_str(&library.zotero_items().unwrap()).unwrap();
        assert_eq!(items[0]["creators"], expected);
        let stored = library.lock().unwrap().item(&ItemId::from_string(&item.id)).unwrap().unwrap();
        assert_eq!(stored.creators[0], Creator::person("Hopper", "Grace"));
        assert_eq!(stored.creators[1].literal, "Test Institute");
        assert_eq!(stored.creators[1].role, CreatorRole::Contributor);
        assert_eq!(stored.creators[2].role, CreatorRole::Editor);
        assert_eq!(stored.creators[3].role, CreatorRole::Translator);
        assert_eq!(stored.creators[4].role, CreatorRole::Author);
        for invalid in [
            Value::Null,
            json!({}),
            json!([null]),
            json!([{ "name": "Missing role" }]),
            json!([{ "name": "Bad role", "creatorType": 1 }]),
            json!([{ "firstName": "Missing last name", "creatorType": "author" }]),
            json!([{ "firstName": 1, "lastName": "Bad given", "creatorType": "author" }]),
            json!([{ "firstName": "Bad family", "lastName": false, "creatorType": "author" }]),
            json!([{ "name": null, "creatorType": "author" }]),
            json!([{ "name": "Mixed", "firstName": "Given", "lastName": "Family", "creatorType": "author" }]),
            json!([{ "name": "x".repeat(1025), "creatorType": "author" }]),
            json!([{ "firstName": "x".repeat(1025), "lastName": "Family", "creatorType": "author" }]),
            json!([{ "firstName": "Given", "lastName": "x".repeat(1025), "creatorType": "author" }]),
            json!([{ "name": "Name", "creatorType": "x".repeat(65) }]),
            json!(vec![json!({ "name": "Name", "creatorType": "author" }); 501]),
            json!([{ "name": "Valid", "creatorType": "author" }, { "name": false, "creatorType": "author" }]),
        ] {
            let result = library.zotero_update_item(
                item.id.clone(),
                json!({ "title": "Must not save", "creators": invalid }).to_string(),
                Some(vec!["must-not-save".into()]),
            );
            assert!(matches!(result, Err(CoreError::InvalidInput { .. })));
            assert_eq!(serde_json::from_str::<Value>(&library.zotero_items().unwrap()).unwrap(), items);
        }
        // Include the count and string-length boundaries, and permit clearing.
        let boundary = json!({ "name": "x".repeat(1024), "creatorType": "x".repeat(64) });
        library
            .zotero_update_item(item.id.clone(), json!({ "creators": vec![boundary; 500] }).to_string(), None)
            .unwrap();
        library.zotero_update_item(item.id.clone(), r#"{"creators": []}"#.into(), None).unwrap();
        let items: Value = serde_json::from_str(&library.zotero_items().unwrap()).unwrap();
        assert_eq!(items[0]["creators"], json!([]));
        library.trash_item(item.id.clone()).unwrap();
        library.delete_item(item.id).unwrap();
        assert_eq!(library.zotero_items().unwrap(), "[]");
        assert!(library.zotero_plugins().is_err(), "a library in memory keeps no plugins");
    }
}
