//! Moving a Zotero library in: items with their details, creators, tags
//! and collections, attached files (copied into this library), PDF
//! annotations (highlights, underlines, notes) and notes (as Markdown in
//! the vault, linked to their items).
//!
//! Zotero holds its database while it runs. The database is copied first;
//! when Zotero keeps even that from happening, its last automatic backup
//! (`zotero.sqlite.bak`) is read instead, and the report says so.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use paperloom_model::{
    Annotation, AnnotationId, AnnotationKind, AttachmentId, CollectionId, Creator, CreatorRole, Doi, Item,
    ItemId, ItemType, Point, Rect, contains_cjk, is_color,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::{CoreError, PaperLibrary, PaperVault};

/// What moving a Zotero library in did.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct ZoteroImport {
    pub added: u32,
    /// Items whose DOI the library already had; their files, tags and
    /// collections joined the item that is here.
    pub duplicates: u32,
    pub collections: u32,
    pub files: u32,
    /// Attached files Zotero lists but that are not on this computer.
    pub missing_files: u32,
    pub annotations: u32,
    pub notes: u32,
    /// Read from Zotero's backup because Zotero held its database.
    pub from_backup: bool,
    pub problems: Vec<String>,
}

fn failed(what: &str, e: impl std::fmt::Display) -> CoreError {
    CoreError::InvalidInput { message: format!("The Zotero library could not be read ({what}): {e}.") }
}

#[derive(Default)]
pub(crate) struct ZItem {
    pub(crate) type_name: String,
    pub(crate) fields: HashMap<String, String>,
    /// First name, last name, single field, role.
    pub(crate) creators: Vec<(String, String, bool, String)>,
    pub(crate) tags: Vec<String>,
}

struct ZAttachment {
    id: i64,
    /// None for a file kept on its own.
    parent: Option<i64>,
    link_mode: i64,
    path: Option<String>,
    key: String,
}

struct ZAnnotation {
    attachment: i64,
    kind: i64,
    text: Option<String>,
    comment: Option<String>,
    color: Option<String>,
    position: String,
    added: String,
    modified: String,
}

struct ZNote {
    parent: Option<i64>,
    html: String,
    title: String,
}

/// A copy of the database (or of its backup) to read, and whether it is
/// the backup.
fn open_copy(data_dir: &Path, scratch: &Path) -> Result<(Connection, bool), CoreError> {
    let database = data_dir.join("zotero.sqlite");
    if !database.exists() {
        return Err(CoreError::InvalidInput {
            message: format!("{} is not a Zotero data folder: it has no zotero.sqlite.", data_dir.display()),
        });
    }
    let copy = scratch.join("zotero.sqlite");
    let backup = data_dir.join("zotero.sqlite.bak");
    let from_backup = match std::fs::copy(&database, &copy) {
        Ok(_) => false,
        Err(_) if backup.exists() => {
            std::fs::copy(&backup, &copy).map_err(|e| failed("copying its backup", e))?;
            true
        }
        Err(e) => return Err(failed("copying it; quit Zotero and try again", e)),
    };
    let connection = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| failed("opening it", e))?;
    Ok((connection, from_backup))
}

fn has_table(db: &Connection, name: &str) -> bool {
    db.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |_| Ok(()))
        .optional()
        .is_ok_and(|found| found.is_some())
}

/// Every row of a query, mapped.
fn rows<T>(
    db: &Connection,
    sql: &str,
    map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, CoreError> {
    let mut statement = db.prepare(sql).map_err(|e| failed("reading it", e))?;
    let found = statement.query_map([], map).map_err(|e| failed("reading it", e))?;
    found.collect::<rusqlite::Result<Vec<T>>>().map_err(|e| failed("reading it", e))
}

const NOT_DELETED: &str = "NOT IN (SELECT itemID FROM deletedItems)";

fn read_items(db: &Connection) -> Result<BTreeMap<i64, ZItem>, CoreError> {
    let mut items: BTreeMap<i64, ZItem> = rows(
        db,
        &format!(
            "SELECT i.itemID, t.typeName FROM items i JOIN itemTypes t ON t.itemTypeID = i.itemTypeID \
             WHERE t.typeName NOT IN ('attachment', 'note', 'annotation') AND i.itemID {NOT_DELETED}"
        ),
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
    )?
    .into_iter()
    .map(|(id, type_name)| (id, ZItem { type_name, ..ZItem::default() }))
    .collect();
    let fields = rows(
        db,
        "SELECT d.itemID, f.fieldName, v.value FROM itemData d \
         JOIN fields f ON f.fieldID = d.fieldID JOIN itemDataValues v ON v.valueID = d.valueID",
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, rusqlite::types::Value>(2)?)),
    )?;
    for (id, field, value) in fields {
        let value = match value {
            rusqlite::types::Value::Text(text) => text,
            rusqlite::types::Value::Integer(n) => n.to_string(),
            rusqlite::types::Value::Real(n) => n.to_string(),
            _ => continue,
        };
        if let Some(item) = items.get_mut(&id) {
            item.fields.insert(field, value);
        }
    }
    let creators = rows(
        db,
        "SELECT ic.itemID, c.firstName, c.lastName, c.fieldMode, ct.creatorType FROM itemCreators ic \
         JOIN creators c ON c.creatorID = ic.creatorID JOIN creatorTypes ct ON ct.creatorTypeID = ic.creatorTypeID \
         ORDER BY ic.itemID, ic.orderIndex",
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                r.get::<_, Option<i64>>(3)?.unwrap_or(0) == 1,
                r.get::<_, String>(4)?,
            ))
        },
    )?;
    for (id, first, last, single, role) in creators {
        if let Some(item) = items.get_mut(&id) {
            item.creators.push((first, last, single, role));
        }
    }
    let tags =
        rows(db, "SELECT it.itemID, t.name FROM itemTags it JOIN tags t ON t.tagID = it.tagID", |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
    for (id, tag) in tags {
        if let Some(item) = items.get_mut(&id) {
            item.tags.push(tag);
        }
    }
    Ok(items)
}

/// `2013-07-31 July 31, 2013` (Zotero keeps a parsed date first, `00` for
/// what is unknown): the year and the date as far as it is known.
fn zotero_date(value: &str) -> (Option<i32>, Option<String>) {
    let parsed = value.get(..10).unwrap_or(value);
    let mut parts = parsed.split('-');
    let year = parts.next().and_then(|y| y.parse::<i32>().ok()).filter(|y| *y > 0);
    let month = parts.next().and_then(|m| m.parse::<u32>().ok()).filter(|m| (1..=12).contains(m));
    let day = parts.next().and_then(|d| d.parse::<u32>().ok()).filter(|d| (1..=31).contains(d));
    let date = match (year, month, day) {
        (Some(y), Some(m), Some(d)) => Some(format!("{y:04}-{m:02}-{d:02}")),
        (Some(y), Some(m), None) => Some(format!("{y:04}-{m:02}")),
        _ => None,
    };
    (year, date)
}

/// `2024-05-01 08:30:00` (UTC) in milliseconds since 1970.
fn zotero_time(value: &str) -> Option<i64> {
    let (date, time) = value.split_once(' ')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (year, month, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|p| p.parse::<i64>().ok());
    let (hour, minute, second) = (t.next()??, t.next()??, t.next()??);
    // Days since 1970 (Howard Hinnant's algorithm).
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    Some(((days * 24 + hour) * 60 + minute) * 60_000 + second * 1000)
}

fn item_type(type_name: &str) -> ItemType {
    match type_name {
        "journalArticle" | "magazineArticle" | "newspaperArticle" => ItemType::JournalArticle,
        "conferencePaper" => ItemType::ConferencePaper,
        "book" => ItemType::Book,
        "bookSection" | "encyclopediaArticle" | "dictionaryEntry" => ItemType::BookChapter,
        "thesis" => ItemType::Thesis,
        "report" => ItemType::Report,
        "preprint" => ItemType::Preprint,
        "patent" => ItemType::Patent,
        "standard" => ItemType::Standard,
        "dataset" => ItemType::Dataset,
        "webpage" | "blogPost" | "forumPost" => ItemType::Webpage,
        _ => ItemType::Other,
    }
}

fn creator(first: &str, last: &str, single: bool, role: &str) -> Creator {
    let mut creator = if single || first.trim().is_empty() {
        Creator::literal(last.trim())
    } else if contains_cjk(last) || contains_cjk(first) {
        // 张 + 三: Chinese, Japanese and Korean names stay whole.
        Creator::literal(format!("{}{}", last.trim(), first.trim()))
    } else {
        Creator::person(last.trim(), first.trim())
    };
    creator.role = match role {
        "editor" | "seriesEditor" => CreatorRole::Editor,
        "translator" => CreatorRole::Translator,
        "author" | "inventor" | "presenter" | "programmer" | "artist" | "director" | "podcaster"
        | "interviewee" | "cartographer" | "performer" | "composer" | "sponsor" => CreatorRole::Author,
        _ => CreatorRole::Contributor,
    };
    creator
}

/// An item's details from its Zotero fields.
pub(crate) fn to_item(z: &ZItem) -> Option<Item> {
    let field = |name: &str| z.fields.get(name).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let first = |names: &[&str]| names.iter().find_map(|name| field(name));
    let title = first(&["title", "caseName", "nameOfAct", "subject"])?;
    let mut item = Item::new(item_type(&z.type_name), title);
    item.creators = z.creators.iter().map(|(f, l, single, role)| creator(f, l, *single, role)).collect();
    item.container_title = first(&[
        "publicationTitle",
        "proceedingsTitle",
        "bookTitle",
        "websiteTitle",
        "blogTitle",
        "forumTitle",
        "encyclopediaTitle",
        "dictionaryTitle",
        "programTitle",
    ]);
    item.container_short_title = field("journalAbbreviation");
    if let Some(date) = field("date") {
        (item.year, item.date) = zotero_date(&date);
    }
    item.volume = field("volume");
    item.issue = field("issue");
    item.pages = field("pages");
    item.publisher =
        first(&["publisher", "university", "institution", "company", "label", "network", "studio"]);
    item.place = field("place");
    item.edition = field("edition");
    item.number = first(&["number", "reportNumber", "patentNumber", "billNumber", "docketNumber"]);
    item.isbn = field("ISBN");
    item.issn = field("ISSN");
    item.url = field("url");
    item.accessed = field("accessDate").map(|d| d.chars().take(10).collect());
    item.abstract_note = field("abstractNote");
    item.language = field("language");
    item.doi = field("DOI").and_then(|d| Doi::parse(&d).ok());
    item.extra = field("extra");
    // Types without a DOI field keep it in Extra, with PMID and arXiv IDs.
    for line in item.extra.as_deref().unwrap_or_default().lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "doi" if item.doi.is_none() => item.doi = Doi::parse(value).ok(),
            "pmid" => item.pmid = Some(value.to_string()),
            "arxiv" => item.arxiv_id = Some(value.to_string()),
            _ => {}
        }
    }
    if let Some(id) = field("archiveID").and_then(|a| a.strip_prefix("arXiv:").map(str::to_string)) {
        item.arxiv_id = Some(id);
    }
    Some(item)
}

/// Where an attached file is: in Zotero's `storage/<key>/` folder, or
/// linked where it lies.
fn attachment_file(data_dir: &Path, attachment: &ZAttachment) -> Option<PathBuf> {
    let path = attachment.path.as_deref()?;
    match attachment.link_mode {
        // Imported files and imported web pages.
        0 | 1 => {
            let name = path.strip_prefix("storage:")?;
            Some(data_dir.join("storage").join(&attachment.key).join(name))
        }
        // A linked file: its own path (`attachments:` paths depend on a
        // Zotero setting this cannot read).
        2 if !path.starts_with("attachments:") => Some(PathBuf::from(path)),
        _ => None,
    }
}

/// Zotero's `{"pageIndex": 0, "rects": [[x1, y1, x2, y2], …]}`, in PDF
/// points from the bottom left: as this library keeps them.
/// A Zotero mark's page, rectangles and (for ink) strokes, from its
/// `position`.
fn position(json: &str) -> Option<(u32, Vec<Rect>, Vec<Vec<Point>>)> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let page = u32::try_from(value["pageIndex"].as_u64()?).ok()?;
    let numbers = |v: &serde_json::Value| -> Vec<f64> {
        v.as_array().into_iter().flatten().filter_map(|n| n.as_f64()).collect()
    };
    let strokes: Vec<Vec<Point>> = value["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|path| numbers(path).as_chunks::<2>().0.iter().map(|&[x, y]| Point { x, y }).collect::<Vec<_>>())
        .filter(|stroke| !stroke.is_empty())
        .collect();
    let mut rects = value["rects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let [x1, y1, x2, y2] = numbers(r)[..] else { return None };
            Some(Rect { x: x1.min(x2), y: y1.min(y2), width: (x2 - x1).abs(), height: (y2 - y1).abs() })
        })
        .filter(Rect::is_valid)
        .collect::<Vec<_>>();
    if rects.is_empty() {
        rects.extend(Rect::around(strokes.iter().flatten().copied()));
    }
    (!rects.is_empty()).then_some((page, rects, strokes))
}

/// A text box's font size in Zotero's position JSON.
fn text_box_font_size(json: &str) -> Option<f64> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    value["fontSize"].as_f64().filter(|size| size.is_finite() && *size > 0.0)
}

/// A Zotero note's HTML as Markdown: headings, paragraphs, lists, emphasis,
/// code and links; anything else as its text.
pub(crate) fn note_markdown(html: &str) -> String {
    let mut out = String::new();
    let mut links: Vec<Option<String>> = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&entities(&rest[..start]));
        let Some(end) = rest[start..].find('>') else {
            rest = &rest[start..];
            break;
        };
        let tag = &rest[start + 1..start + end];
        rest = &rest[start + end + 1..];
        let closing = tag.starts_with('/');
        let name =
            tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        match (name.to_ascii_lowercase().as_str(), closing) {
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", false) => {
                let level = name[1..].parse::<usize>().unwrap_or(1);
                out.push_str(&format!("\n\n{} ", "#".repeat(level)));
            }
            (
                "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "blockquote" | "pre",
                _,
            ) => out.push_str("\n\n"),
            ("br", _) => out.push('\n'),
            ("li", false) => out.push_str("\n- "),
            ("strong" | "b", _) => out.push_str("**"),
            ("em" | "i", _) => out.push('*'),
            ("code", _) => out.push('`'),
            ("a", false) => {
                let href = attribute(tag, "href");
                if href.is_some() {
                    out.push('[');
                }
                links.push(href);
            }
            ("a", true) => {
                if let Some(Some(href)) = links.pop() {
                    out.push_str(&format!("]({href})"));
                }
            }
            _ => {}
        }
    }
    out.push_str(&entities(rest));
    // At most one blank line at a time.
    let mut text = String::new();
    let mut blank = 0;
    for line in out.lines().map(str::trim_end) {
        if line.trim().is_empty() {
            blank += 1;
            continue;
        }
        if !text.is_empty() {
            text.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        text.push_str(line.trim_start());
    }
    text
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let value = &tag[at..at + tag[at..].find('"')?];
    Some(entities(value))
}

fn entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

impl PaperLibrary {
    /// Moves a Zotero library in from its data folder (where
    /// `zotero.sqlite` and `storage/` are). Notes go to the vault's
    /// `Zotero` folder when a vault is given. Copies files: call it off the
    /// main thread.
    pub(crate) fn import_zotero_from(
        &self,
        data_dir: &Path,
        vault: Option<&PaperVault>,
    ) -> Result<ZoteroImport, CoreError> {
        let scratch = tempfile::tempdir().map_err(|e| failed("making room for a copy", e))?;
        let (db, from_backup) = open_copy(data_dir, scratch.path())?;
        let mut report = ZoteroImport { from_backup, ..ZoteroImport::default() };
        let items = read_items(&db)?;
        let collections = rows(
            &db,
            if has_table(&db, "deletedCollections") {
                "SELECT collectionID, collectionName, parentCollectionID FROM collections \
                 WHERE collectionID NOT IN (SELECT collectionID FROM deletedCollections)"
            } else {
                "SELECT collectionID, collectionName, parentCollectionID FROM collections"
            },
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<i64>>(2)?)),
        )?;
        let members = rows(&db, "SELECT collectionID, itemID FROM collectionItems", |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        })?;
        let attachments = rows(
            &db,
            &format!(
                "SELECT a.itemID, a.parentItemID, a.linkMode, a.path, i.key FROM itemAttachments a \
                 JOIN items i ON i.itemID = a.itemID WHERE a.itemID {NOT_DELETED}"
            ),
            |r| {
                Ok(ZAttachment {
                    id: r.get(0)?,
                    parent: r.get(1)?,
                    link_mode: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    path: r.get(3)?,
                    key: r.get(4)?,
                })
            },
        )?;
        // The titles of files kept on their own, which become items.
        let titles: HashMap<i64, String> = rows(
            &db,
            "SELECT d.itemID, v.value FROM itemData d JOIN fields f ON f.fieldID = d.fieldID \
             JOIN itemDataValues v ON v.valueID = d.valueID \
             JOIN itemAttachments a ON a.itemID = d.itemID \
             WHERE f.fieldName = 'title' AND a.parentItemID IS NULL",
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, rusqlite::types::Value>(1)?)),
        )?
        .into_iter()
        .filter_map(|(id, value)| match value {
            rusqlite::types::Value::Text(text) => Some((id, text)),
            _ => None,
        })
        .collect();
        let annotations = if has_table(&db, "itemAnnotations") {
            rows(
                &db,
                &format!(
                    "SELECT a.parentItemID, a.type, a.text, a.comment, a.color, a.position, i.dateAdded, \
                     i.dateModified FROM itemAnnotations a JOIN items i ON i.itemID = a.itemID \
                     WHERE a.itemID {NOT_DELETED}"
                ),
                |r| {
                    Ok(ZAnnotation {
                        attachment: r.get(0)?,
                        kind: r.get(1)?,
                        text: r.get(2)?,
                        comment: r.get(3)?,
                        color: r.get(4)?,
                        position: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                        added: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                        modified: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    })
                },
            )?
        } else {
            Vec::new()
        };
        let notes = rows(
            &db,
            &format!("SELECT parentItemID, note, title FROM itemNotes WHERE itemID {NOT_DELETED}"),
            |r| {
                Ok(ZNote {
                    parent: r.get(0)?,
                    html: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    title: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                })
            },
        )?;
        drop(db);

        let files = self.files()?;
        let mut ours: HashMap<i64, Item> = HashMap::new();
        let mut added_ids: Vec<ItemId> = Vec::new();
        {
            let mut store = self.lock()?;
            // Items, or the ones already here with the same DOI.
            for (id, z) in &items {
                let Some(item) = to_item(z) else { continue };
                let existing = match &item.doi {
                    Some(doi) => store.find_by_doi(doi)?,
                    None => None,
                };
                let item = match existing {
                    Some(existing) => {
                        report.duplicates += 1;
                        existing
                    }
                    None => {
                        store.add_item(&item)?;
                        report.added += 1;
                        added_ids.push(item.id.clone());
                        item
                    }
                };
                if !z.tags.is_empty() {
                    let mut tags = store.item_tags(&item.id)?;
                    for tag in &z.tags {
                        if !tags.contains(tag) {
                            tags.push(tag.clone());
                        }
                    }
                    store.set_item_tags(&item.id, &tags)?;
                }
                ours.insert(*id, item);
            }

            // A file kept on its own is an item of its own, named as Zotero
            // shows it (its title, else the file's name).
            for attachment in attachments.iter().filter(|a| a.parent.is_none()) {
                let Some(file) = attachment_file(data_dir, attachment) else { continue };
                if !file.is_file() {
                    report.missing_files += 1;
                    continue;
                }
                let title = titles
                    .get(&attachment.id)
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .or_else(|| file.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    .unwrap_or_else(|| "Untitled".into());
                let item = Item::new(ItemType::Other, title);
                store.add_item(&item)?;
                report.added += 1;
                added_ids.push(item.id.clone());
                ours.insert(attachment.id, item);
            }

            // Collections, parents first; ones already here by name and
            // parent are reused.
            let mut made: HashMap<i64, CollectionId> = HashMap::new();
            let mut waiting = collections;
            while !waiting.is_empty() {
                let before = waiting.len();
                let mut later = Vec::new();
                for (id, name, parent) in waiting {
                    let parent_id = match parent {
                        Some(p) => match made.get(&p) {
                            Some(found) => Some(found.clone()),
                            None => {
                                later.push((id, name, parent));
                                continue;
                            }
                        },
                        None => None,
                    };
                    let existing =
                        store.collections()?.into_iter().find(|c| c.name == name && c.parent_id == parent_id);
                    let collection = match existing {
                        Some(found) => found,
                        None => {
                            report.collections += 1;
                            store.create_collection(&name, parent_id.as_ref())?
                        }
                    };
                    made.insert(id, collection.id);
                }
                // A parent that never comes (a loop, or deleted): the rest
                // go to the top.
                if later.len() == before {
                    later.iter_mut().for_each(|(_, _, parent)| *parent = None);
                }
                waiting = later;
            }
            for (collection, item) in members {
                if let (Some(collection), Some(item)) = (made.get(&collection), ours.get(&item)) {
                    store.add_to_collection(collection, &item.id)?;
                }
            }

            // Files, then the marks on them.
            let mut copied: HashMap<i64, AttachmentId> = HashMap::new();
            for attachment in &attachments {
                let Some(item) = ours.get(&attachment.parent.unwrap_or(attachment.id)) else { continue };
                let Some(file) = attachment_file(data_dir, attachment) else { continue };
                if !file.is_file() {
                    report.missing_files += 1;
                    continue;
                }
                match files.attach(&mut store, &item.id, &file) {
                    Ok(stored) => {
                        report.files += 1;
                        copied.insert(attachment.id, stored.id);
                    }
                    Err(e) => report.problems.push(format!("{}: {e}", file.display())),
                }
            }
            let mut seen: HashSet<(String, u32, String)> = HashSet::new();
            for z in &annotations {
                let Some(attachment) = copied.get(&z.attachment) else { continue };
                let kind = match z.kind {
                    1 => AnnotationKind::Highlight,
                    2 => AnnotationKind::Note,
                    4 => AnnotationKind::Ink,
                    5 => AnnotationKind::Underline,
                    // A text box: its text is its comment.
                    6 => AnnotationKind::Text,
                    _ => continue,
                };
                let Some((page_index, rects, strokes)) = position(&z.position) else { continue };
                let font_size =
                    if kind == AnnotationKind::Text { text_box_font_size(&z.position) } else { None };
                let text = z.text.clone().filter(|t| !t.trim().is_empty());
                // Marks already here (an earlier import) are not made twice.
                let fingerprint = format!("{:?}{}", rects.first(), text.as_deref().unwrap_or_default());
                if !seen.insert((attachment.to_string(), page_index, fingerprint.clone())) {
                    continue;
                }
                let here = store.annotations(attachment)?;
                if here.iter().any(|a| {
                    a.page_index == page_index
                        && format!("{:?}{}", a.rects.first(), a.text.as_deref().unwrap_or_default())
                            == fingerprint
                }) {
                    continue;
                }
                let color = z.color.clone().filter(|c| is_color(c)).unwrap_or_else(|| "#ffd400".to_string());
                let added = zotero_time(&z.added).unwrap_or_else(paperloom_store::now_ms);
                let annotation = Annotation {
                    id: AnnotationId::new(),
                    attachment_id: attachment.clone(),
                    kind,
                    color,
                    page_index,
                    rects,
                    text,
                    comment: z.comment.clone().filter(|c| !c.trim().is_empty()),
                    tags: Vec::new(),
                    strokes,
                    font_size,
                    date_added: added,
                    date_modified: zotero_time(&z.modified).unwrap_or(added),
                };
                store.add_annotation(&annotation)?;
                report.annotations += 1;
            }
        }
        self.reindex_items(&added_ids);

        // Notes, into the vault.
        let with_text: Vec<&ZNote> =
            notes.iter().filter(|n| !note_markdown(&n.html).trim().is_empty()).collect();
        match vault {
            Some(vault) => {
                for note in with_text {
                    let markdown = note_markdown(&note.html);
                    let item = note.parent.and_then(|p| ours.get(&p));
                    let title = match (note.title.trim(), item) {
                        ("", Some(item)) => item.title.clone(),
                        ("", None) => markdown
                            .lines()
                            .next()
                            .unwrap_or("Zotero note")
                            .trim_start_matches('#')
                            .trim()
                            .to_string(),
                        (title, _) => title.to_string(),
                    };
                    let mut text = markdown;
                    if let Some(item) = item {
                        text.push_str(&format!("\n\n{}\n", crate::notelinks::item_link_text(item)));
                    }
                    match vault
                        .create_note("Zotero".into(), title)
                        .and_then(|made| vault.write_note(made.path, text))
                    {
                        Ok(_) => report.notes += 1,
                        Err(e) => report.problems.push(e.to_string()),
                    }
                }
            }
            None if !with_text.is_empty() => report.problems.push(format!(
                "Notes left in Zotero, as there is no vault to put them in: {}.",
                with_text.len()
            )),
            None => {}
        }
        Ok(report)
    }
}

#[uniffi::export]
impl PaperLibrary {
    /// Moves a Zotero library in from its data folder (usually `~/Zotero`):
    /// items, creators, tags, collections, attached files, PDF annotations,
    /// and notes into the vault's `Zotero` folder. Items whose DOI is here
    /// already are joined, not doubled. Copies files: call it off the main
    /// thread.
    pub fn import_zotero(
        &self,
        data_dir: String,
        vault: Option<Arc<PaperVault>>,
    ) -> Result<ZoteroImport, CoreError> {
        self.import_zotero_from(Path::new(&data_dir), vault.as_deref())
    }
}

#[cfg(test)]
mod tests;
