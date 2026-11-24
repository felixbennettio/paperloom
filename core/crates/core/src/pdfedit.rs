//! Changing an attached PDF's pages: turning, removing, moving, adding a
//! blank one; and a copy with the app's highlights and notes written into
//! it as PDF annotations, for other readers. A changed file replaces the
//! old one in one step, and the marks and page text follow their pages.

use std::path::{Path, PathBuf};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use paperloom_model::{Annotation, AnnotationKind, AttachmentId};
use serde::{Deserialize, Serialize};

use crate::{AttachmentInfo, CoreError, PaperLibrary};

mod textlayer;

/// A line read from a page's picture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct RecognizedLine {
    /// The line's words, as read.
    pub text: String,
    /// Its box on the page as shown (the page's /Rotate applied, within its
    /// CropBox, else MediaBox), as shares of the shown page's width and
    /// height from its top-left corner: 0 to 1.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Lines read from a page; pages count from 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct RecognizedPage {
    pub page_index: u32,
    pub lines: Vec<RecognizedLine>,
}

/// A change to a PDF's pages; pages count from 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PageEdit {
    /// Turns pages clockwise by `degrees`, a multiple of 90.
    Rotate {
        pages: Vec<u32>,
        degrees: i32,
    },
    Delete {
        pages: Vec<u32>,
    },
    /// Moves a page so it ends up at `to`.
    Move {
        from: u32,
        to: u32,
    },
    /// Adds a blank page at `at`, the size of the page before it.
    InsertBlank {
        at: u32,
    },
}

pub(crate) fn unreadable(e: impl std::fmt::Display) -> CoreError {
    CoreError::InvalidInput { message: format!("This PDF cannot be changed: {e}.") }
}

/// Attributes a page can take from the page tree above it.
const INHERITED: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// Gives a page its own copy of what it takes from the tree above it, so
/// it keeps it wherever it moves.
fn settle(doc: &mut Document, page: ObjectId) -> Result<(), CoreError> {
    let mut found: Vec<(&[u8], Object)> = Vec::new();
    let mut parent =
        doc.get_dictionary(page).map_err(unreadable)?.get(b"Parent").and_then(Object::as_reference).ok();
    let mut depth = 0;
    while let Some(id) = parent.filter(|_| depth < 64) {
        let Ok(node) = doc.get_dictionary(id) else { break };
        for key in INHERITED {
            if !found.iter().any(|(k, _)| *k == key)
                && let Ok(value) = node.get(key)
            {
                found.push((key, value.clone()));
            }
        }
        parent = node.get(b"Parent").and_then(Object::as_reference).ok();
        depth += 1;
    }
    let dict = doc.get_dictionary_mut(page).map_err(unreadable)?;
    for (key, value) in found {
        if !dict.has(key) {
            dict.set(key, value);
        }
    }
    Ok(())
}

/// Puts the pages in this order directly under the root of the page tree;
/// pages left out, and the tree's inner nodes, are dropped.
fn set_pages(doc: &mut Document, pages: &[ObjectId]) -> Result<(), CoreError> {
    let root =
        doc.catalog().and_then(|c| c.get(b"Pages")).and_then(Object::as_reference).map_err(unreadable)?;
    for &page in pages {
        settle(doc, page)?;
    }
    for &page in pages {
        doc.get_dictionary_mut(page).map_err(unreadable)?.set("Parent", Object::Reference(root));
    }
    let tree = doc.get_dictionary_mut(root).map_err(unreadable)?;
    tree.set("Kids", pages.iter().map(|&id| Object::Reference(id)).collect::<Vec<Object>>());
    tree.set("Count", pages.len() as i64);
    doc.prune_objects();
    Ok(())
}

/// The value of a number in a PDF (an integer or a real).
fn number(object: &Object) -> Option<f64> {
    object.as_float().map(f64::from).ok().or_else(|| object.as_i64().ok().map(|n| n as f64))
}

/// A page's size box (its own or inherited), `[x0 y0 x1 y1]`.
fn media_box(doc: &Document, page: ObjectId) -> Vec<Object> {
    let mut current = Some(page);
    let mut depth = 0;
    while let Some(id) = current.filter(|_| depth < 64) {
        let Ok(dict) = doc.get_dictionary(id) else { break };
        if let Ok(found) = dict.get(b"MediaBox").and_then(Object::as_array)
            && found.len() == 4
        {
            return found.clone();
        }
        current = dict.get(b"Parent").and_then(Object::as_reference).ok();
        depth += 1;
    }
    // A4.
    [0, 0, 595, 842].map(Object::Integer).to_vec()
}

/// What a page edit does: the new order of pages, and where each old page
/// went (none when removed).
fn apply(doc: &mut Document, edit: &PageEdit) -> Result<Vec<Option<u32>>, CoreError> {
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let count = pages.len() as u32;
    if count == 0 {
        return Err(CoreError::InvalidInput { message: "This PDF has no pages.".into() });
    }
    let check = |page: u32| {
        if page < count {
            Ok(())
        } else {
            Err(CoreError::InvalidInput { message: format!("This PDF has no page {}.", page + 1) })
        }
    };
    let same: Vec<Option<u32>> = (0..count).map(Some).collect();
    match edit {
        PageEdit::Rotate { pages: turned, degrees } => {
            if degrees % 90 != 0 {
                return Err(CoreError::InvalidInput { message: "Pages turn by quarter turns.".into() });
            }
            for &page in turned {
                check(page)?;
                let id = pages[page as usize];
                settle(doc, id)?;
                let dict = doc.get_dictionary_mut(id).map_err(unreadable)?;
                let now = dict.get(b"Rotate").ok().and_then(number).unwrap_or(0.0) as i64;
                dict.set("Rotate", (now + i64::from(*degrees)).rem_euclid(360));
            }
            Ok(same)
        }
        PageEdit::Delete { pages: removed } => {
            for &page in removed {
                check(page)?;
            }
            if removed.iter().collect::<std::collections::BTreeSet<_>>().len() as u32 >= count {
                return Err(CoreError::InvalidInput { message: "A PDF keeps at least one page.".into() });
            }
            let mut mapping = Vec::with_capacity(count as usize);
            let mut kept = Vec::new();
            for (index, id) in pages.iter().enumerate() {
                if removed.contains(&(index as u32)) {
                    mapping.push(None);
                } else {
                    mapping.push(Some(kept.len() as u32));
                    kept.push(*id);
                }
            }
            set_pages(doc, &kept)?;
            Ok(mapping)
        }
        PageEdit::Move { from, to } => {
            check(*from)?;
            let mut order = pages.clone();
            let page = order.remove(*from as usize);
            let to = (*to as usize).min(order.len());
            order.insert(to, page);
            let mapping =
                pages.iter().map(|id| order.iter().position(|p| p == id).map(|i| i as u32)).collect();
            set_pages(doc, &order)?;
            Ok(mapping)
        }
        PageEdit::InsertBlank { at } => {
            let at = (*at).min(count);
            let neighbour = pages[at.saturating_sub(1).min(count.saturating_sub(1)) as usize];
            let size = media_box(doc, neighbour);
            let contents = doc.add_object(Stream::new(Dictionary::new(), Vec::new()));
            let mut page = Dictionary::new();
            page.set("Type", "Page");
            page.set("MediaBox", size);
            page.set("Resources", Dictionary::new());
            page.set("Contents", Object::Reference(contents));
            let blank = doc.add_object(page);
            let mut order = pages.clone();
            order.insert(at as usize, blank);
            set_pages(doc, &order)?;
            Ok((0..count).map(|i| Some(if i < at { i } else { i + 1 })).collect())
        }
    }
}

/// Saves a document over a file in one step.
pub(crate) fn replace(doc: &mut Document, path: &Path) -> Result<(), CoreError> {
    let temporary = path.with_extension("pdf.paperloom-tmp");
    doc.save(&temporary).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        CoreError::Storage { message: format!("Writing {}: {e}", path.display()) }
    })?;
    std::fs::rename(&temporary, path)
        .map_err(|e| CoreError::Storage { message: format!("Saving {}: {e}", path.display()) })
}

/// A text string in a PDF: plain when ASCII, UTF-16 otherwise.
fn text_string(text: &str) -> Object {
    if text.is_ascii() {
        return Object::string_literal(text);
    }
    let mut bytes = vec![0xfe, 0xff];
    bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
    Object::String(bytes, StringFormat::Hexadecimal)
}

/// `#ffd400` as PDF colour components.
fn components(color: &str) -> Vec<Object> {
    let hex = color.trim_start_matches('#');
    (0..3)
        .map(|i| {
            let value = hex.get(i * 2..i * 2 + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).unwrap_or(0);
            Object::Real(f32::from(value) / 255.0)
        })
        .collect()
}

/// The standard name of a stamp's word (`Not Approved` → `NotApproved`),
/// which viewers draw by themselves; other words as they are.
fn stamp_name(word: &str) -> String {
    const STANDARD: [&str; 14] = [
        "Approved",
        "Experimental",
        "NotApproved",
        "AsIs",
        "Expired",
        "NotForPublicRelease",
        "Confidential",
        "Final",
        "Sold",
        "Departmental",
        "ForComment",
        "TopSecret",
        "Draft",
        "ForPublicRelease",
    ];
    let joined: String = word.split_whitespace().collect();
    STANDARD.iter().find(|name| name.eq_ignore_ascii_case(&joined)).map_or_else(
        || if joined.is_empty() { "Draft".to_string() } else { joined.clone() },
        |n| (*n).to_string(),
    )
}

/// How a stamp looks, for viewers that do not draw it by its name: its
/// word in capitals, framed, in its colour. None for words beyond the
/// standard fonts (they show the stamp's name instead).
fn stamp_appearance(mark: &Annotation) -> Option<Stream> {
    let word = mark.comment.as_deref()?.trim().to_uppercase();
    let rect = mark.rects.first().filter(|r| r.is_valid() && r.width > 4.0 && r.height > 4.0)?;
    if word.is_empty() || !word.is_ascii() {
        return None;
    }
    let (w, h) = (rect.width, rect.height);
    let channel = |i: usize| {
        mark.color
            .get(1 + i * 2..3 + i * 2)
            .and_then(|x| u8::from_str_radix(x, 16).ok())
            .map_or(0.0, |v| f64::from(v) / 255.0)
    };
    let (r, g, b) = (channel(0), channel(1), channel(2));
    // Helvetica Bold capitals are about 0.72 em wide.
    let chars = word.chars().count() as f64;
    let size = (h * 0.55).min((w - 12.0) / (0.72 * chars)).max(4.0);
    let (x, y) = ((w - 0.72 * chars * size) / 2.0, (h - 0.7 * size) / 2.0);
    let escaped = word.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)");
    let content = format!(
        "q {r:.3} {g:.3} {b:.3} RG 2 w 1 1 {:.2} {:.2} re S BT /F1 {size:.2} Tf {r:.3} {g:.3} {b:.3} rg {x:.2} {y:.2} Td ({escaped}) Tj ET Q",
        w - 2.0,
        h - 2.0
    );
    let mut font = Dictionary::new();
    font.set("Type", "Font");
    font.set("Subtype", "Type1");
    font.set("BaseFont", "Helvetica-Bold");
    let mut fonts = Dictionary::new();
    fonts.set("F1", font);
    let mut resources = Dictionary::new();
    resources.set("Font", fonts);
    let mut dict = Dictionary::new();
    dict.set("Type", "XObject");
    dict.set("Subtype", "Form");
    dict.set("BBox", [0.0, 0.0, w, h].map(Object::from).to_vec());
    dict.set("Resources", resources);
    Some(Stream::new(dict, content.into_bytes()))
}

/// A mark as a PDF annotation on its page.
fn annotation_dict(mark: &Annotation, page: ObjectId) -> Option<Dictionary> {
    let rects = mark.rects.iter().filter(|r| r.is_valid()).collect::<Vec<_>>();
    let first = rects.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x + first.width, first.y + first.height);
    for r in &rects {
        x0 = x0.min(r.x);
        y0 = y0.min(r.y);
        x1 = x1.max(r.x + r.width);
        y1 = y1.max(r.y + r.height);
    }
    let mut dict = Dictionary::new();
    dict.set("Type", "Annot");
    let subtype = match mark.kind {
        AnnotationKind::Highlight => "Highlight",
        AnnotationKind::Underline => "Underline",
        AnnotationKind::StrikeOut => "StrikeOut",
        AnnotationKind::Note => "Text",
        AnnotationKind::Ink => "Ink",
        AnnotationKind::Text => "FreeText",
        AnnotationKind::Square => "Square",
        AnnotationKind::Circle => "Circle",
        AnnotationKind::Arrow => "Line",
        AnnotationKind::Stamp => "Stamp",
    };
    dict.set("Subtype", subtype);
    let rect: Vec<Object> = [x0, y0, x1, y1].map(Object::from).to_vec();
    dict.set("Rect", rect);
    let border = || {
        let mut border = Dictionary::new();
        border.set("W", 2_i64);
        border
    };
    if mark.kind == AnnotationKind::Stamp {
        dict.set("Name", stamp_name(mark.comment.as_deref().unwrap_or_default()));
    } else if matches!(mark.kind, AnnotationKind::Text | AnnotationKind::Square | AnnotationKind::Circle) {
        if mark.kind == AnnotationKind::Text {
            // Drawn by the viewer from its default appearance: Helvetica
            // in the mark's colour, at its size.
            let channel = |i: usize| {
                mark.color
                    .get(1 + i * 2..3 + i * 2)
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                    .map_or(0.0, |v| f64::from(v) / 255.0)
            };
            let (r, g, b) = (channel(0), channel(1), channel(2));
            let size = mark.font_size.unwrap_or(14.0);
            dict.set("DA", Object::string_literal(format!("/Helv {size:.1} Tf {r:.3} {g:.3} {b:.3} rg")));
            let mut border = Dictionary::new();
            border.set("W", 0_i64);
            dict.set("BS", border);
        } else {
            dict.set("BS", border());
        }
    } else if mark.kind == AnnotationKind::Arrow {
        let (Some(tail), Some(head)) = (mark.strokes.first()?.first(), mark.strokes.first()?.last()) else {
            return None;
        };
        let line: Vec<Object> = [tail.x, tail.y, head.x, head.y].map(Object::from).to_vec();
        dict.set("L", line);
        let ends: Vec<Object> = vec!["None".into(), "OpenArrow".into()];
        dict.set("LE", ends);
        dict.set("BS", border());
    } else if mark.kind == AnnotationKind::Ink {
        // Each stroke as x y pairs, two points wide.
        let strokes: Vec<Object> = mark
            .strokes
            .iter()
            .map(|stroke| Object::Array(stroke.iter().flat_map(|p| [p.x, p.y].map(Object::from)).collect()))
            .collect();
        dict.set("InkList", strokes);
        dict.set("BS", border());
    } else if mark.kind != AnnotationKind::Note {
        // Each line: upper left, upper right, lower left, lower right.
        let quads: Vec<Object> = rects
            .iter()
            .flat_map(|r| {
                [r.x, r.y + r.height, r.x + r.width, r.y + r.height, r.x, r.y, r.x + r.width, r.y]
                    .map(Object::from)
            })
            .collect();
        dict.set("QuadPoints", quads);
    } else {
        dict.set("Name", "Comment");
    }
    // A text box's colour is its text's; C would fill the box.
    if mark.kind != AnnotationKind::Text {
        dict.set("C", components(&mark.color));
    }
    // Printed with the page.
    dict.set("F", 4_i64);
    dict.set("T", text_string("Paperloom"));
    dict.set("P", Object::Reference(page));
    let contents = match (&mark.comment, &mark.text) {
        (Some(comment), _) if !comment.trim().is_empty() => Some(comment.as_str()),
        (_, Some(text)) if mark.kind == AnnotationKind::Note => Some(text.as_str()),
        _ => None,
    };
    if let Some(contents) = contents {
        dict.set("Contents", text_string(contents));
    }
    Some(dict)
}

impl PaperLibrary {
    pub(crate) fn pdf_path(
        &self,
        attachment_id: &str,
    ) -> Result<(paperloom_model::Attachment, PathBuf), CoreError> {
        let files = self.files()?;
        let attachment = self
            .lock()?
            .attachment(&AttachmentId::from_string(attachment_id))?
            .ok_or_else(|| CoreError::not_found(format!("attachment {attachment_id}")))?;
        let path = files.path_of(&attachment);
        Ok((attachment, path))
    }
}

#[uniffi::export]
impl PaperLibrary {
    /// Writes recognised lines into an attached PDF as invisible text over
    /// its pages, so readers can select and search them. Pages that already
    /// carry text are left as they are.
    pub fn add_text_layer(
        &self,
        attachment_id: String,
        pages: Vec<RecognizedPage>,
    ) -> Result<AttachmentInfo, CoreError> {
        let (attachment, path) = self.pdf_path(&attachment_id)?;
        if !attachment.is_pdf() {
            return Err(CoreError::InvalidInput {
                message: "A text layer can only be added to a PDF attachment.".into(),
            });
        }
        let mut doc = Document::load(&path).map_err(unreadable)?;
        if doc.is_encrypted() {
            return Err(CoreError::InvalidInput {
                message: "This PDF is protected and cannot be changed.".into(),
            });
        }
        if !textlayer::apply(&mut doc, pages)? {
            return Ok(self.files()?.info(&attachment));
        }
        replace(&mut doc, &path)?;
        let (sha256, size) = crate::files::sha256_and_size(&path)?;
        let updated = {
            let mut store = self.lock()?;
            store.set_attachment_content(&attachment.id, size, &sha256)?;
            store.attachment(&attachment.id)?.unwrap_or(attachment)
        };
        Ok(self.files()?.info(&updated))
    }

    /// Changes an attached PDF's pages. The file is replaced in one step;
    /// highlights and notes move with their pages (those on removed pages
    /// go), and so does the page text search uses.
    pub fn edit_pdf_pages(&self, attachment_id: String, edit: PageEdit) -> Result<AttachmentInfo, CoreError> {
        let (attachment, path) = self.pdf_path(&attachment_id)?;
        let mut doc = Document::load(&path).map_err(unreadable)?;
        if doc.is_encrypted() {
            return Err(CoreError::InvalidInput {
                message: "This PDF is protected and cannot be changed.".into(),
            });
        }
        let mapping = apply(&mut doc, &edit)?;
        let new_count = doc.get_pages().len();
        replace(&mut doc, &path)?;

        let (sha256, size) = crate::files::sha256_and_size(&path)?;
        let (updated, pages, moved, gone) = {
            let mut store = self.lock()?;
            store.set_attachment_content(&attachment.id, size, &sha256)?;
            let mut moved = Vec::new();
            let mut gone = Vec::new();
            for mut mark in store.annotations(&attachment.id)? {
                match mapping.get(mark.page_index as usize).copied().flatten() {
                    None => {
                        store.remove_annotation(&mark.id)?;
                        gone.push(mark.id.to_string());
                    }
                    Some(page) if page != mark.page_index => {
                        mark.page_index = page;
                        store.update_annotation(&mark)?;
                        moved.push(mark);
                    }
                    Some(_) => {}
                }
            }
            let text = store.attachment_pages(&attachment.id)?;
            let pages = (!text.is_empty()).then(|| {
                let mut pages = vec![String::new(); new_count];
                for (old, page) in text {
                    if let Some(Some(new)) = mapping.get(old as usize)
                        && let Some(slot) = pages.get_mut(*new as usize)
                    {
                        *slot = page;
                    }
                }
                pages
            });
            let updated = store.attachment(&attachment.id)?.unwrap_or_else(|| attachment.clone());
            (updated, pages, moved, gone)
        };
        self.unindex_keys(&gone);
        for mark in &moved {
            self.index_annotation(mark);
        }
        if let Some(pages) = pages {
            crate::fulltext::set_attachment_text(self, attachment_id, pages)?;
        }
        Ok(self.files()?.info(&updated))
    }

    /// Writes a copy of an attached PDF with its highlights, underlines,
    /// strike-outs and notes as PDF annotations, which other PDF readers
    /// show. The file in the library stays as it is.
    pub fn export_annotated_pdf(&self, attachment_id: String, destination: String) -> Result<u32, CoreError> {
        let (attachment, path) = self.pdf_path(&attachment_id)?;
        let mut doc = Document::load(&path).map_err(unreadable)?;
        if doc.is_encrypted() {
            return Err(CoreError::InvalidInput {
                message: "This PDF is protected and cannot be changed.".into(),
            });
        }
        let marks = self.lock()?.annotations(&attachment.id)?;
        let pages = doc.get_pages();
        let mut written = 0;
        for mark in &marks {
            let Some(&page) = pages.get(&(mark.page_index + 1)) else { continue };
            let Some(mut dict) = annotation_dict(mark, page) else { continue };
            if mark.kind == AnnotationKind::Stamp
                && let Some(look) = stamp_appearance(mark)
            {
                let look = doc.add_object(look);
                let mut appearance = Dictionary::new();
                appearance.set("N", Object::Reference(look));
                dict.set("AP", appearance);
            }
            let id = doc.add_object(dict);
            // The page's list of annotations: kept apart, in the page, or none yet.
            let list = match doc.get_dictionary(page).map_err(unreadable)?.get(b"Annots") {
                Ok(Object::Reference(list)) => Some(Some(*list)),
                Ok(Object::Array(_)) => Some(None),
                _ => None,
            };
            match list {
                Some(Some(list)) => {
                    doc.get_object_mut(list)
                        .and_then(Object::as_array_mut)
                        .map_err(unreadable)?
                        .push(id.into());
                }
                Some(None) => {
                    let dict = doc.get_dictionary_mut(page).map_err(unreadable)?;
                    dict.get_mut(b"Annots")
                        .and_then(Object::as_array_mut)
                        .map_err(unreadable)?
                        .push(id.into());
                }
                None => {
                    doc.get_dictionary_mut(page).map_err(unreadable)?.set("Annots", vec![Object::from(id)])
                }
            }
            written += 1;
        }
        doc.save(&destination)
            .map_err(|e| CoreError::Storage { message: format!("Writing {destination}: {e}") })?;
        Ok(written)
    }
}

#[cfg(test)]
mod tests;
