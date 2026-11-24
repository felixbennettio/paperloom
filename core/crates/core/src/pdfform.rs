//! Filling in a PDF's form, as Acrobat and Foxit fill one: the fields of
//! the file's AcroForm (text, check boxes, radio buttons, lists and combo
//! boxes) with where their boxes lie, and values written back into the
//! file with appearances that other readers show.

use std::collections::HashMap;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, decode_text_string, text_string};
use serde::{Deserialize, Serialize};

use crate::pdfedit::{replace, unreadable};
use crate::{AttachmentInfo, CoreError, PaperLibrary};

/// One of a field's boxes on a page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct PdfWidget {
    /// Its page, from 0.
    pub page_index: u32,
    /// Its rectangle, in points from the page's bottom left (as PDF
    /// annotations lie).
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// What a check box or radio button holds when this box is on (`Yes`,
    /// a choice's name); empty for other fields.
    pub on_value: String,
}

/// A field of a PDF's form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct PdfFormField {
    /// Its full name (`Address.City`), which values name it by.
    pub name: String,
    /// `text`, `checkbox`, `radio`, `choice` (a list or combo box),
    /// `button` (one that only acts) or `signature`.
    pub kind: String,
    /// What it holds: a text field's text, the on value of the check box or
    /// radio button that is on (`Off` when none is), the choice made.
    pub value: String,
    /// A list's or combo box's choices, and the text each shows.
    pub options: Vec<String>,
    pub option_labels: Vec<String>,
    /// A text field of several lines.
    pub multiline: bool,
    /// A combo box (one choice shown) rather than a list.
    pub combo: bool,
    pub read_only: bool,
    /// The most characters a text field takes; 0 for any number.
    pub max_length: u32,
    /// Its text size in points; 0 to fit its box.
    pub font_size: f64,
    pub widgets: Vec<PdfWidget>,
}

/// A value for a field, by its full name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct PdfFieldValue {
    pub name: String,
    pub value: String,
}

// Field flags (ISO 32000-1, tables 221, 226, 228 and 230).
const READ_ONLY: i64 = 1;
const MULTILINE: i64 = 1 << 12;
const RADIO: i64 = 1 << 15;
const PUSH_BUTTON: i64 = 1 << 16;
const COMBO: i64 = 1 << 17;

/// A field as the file has it: its dictionary, and each of its boxes with
/// where it lies (on no page, `u32::MAX`, when the file does not say).
struct Field {
    id: ObjectId,
    info: PdfFormField,
    boxes: Vec<(ObjectId, PdfWidget)>,
    /// Its text colour, as the operator that sets it (`0 g`).
    color: String,
}

/// What a field takes from the fields above it.
#[derive(Clone, Default)]
struct Inherited {
    name: String,
    kind: Vec<u8>,
    flags: i64,
    value: Option<Object>,
    appearance: String,
}

fn resolve<'a>(doc: &'a Document, object: &'a Object) -> &'a Object {
    doc.dereference(object).map_or(object, |(_, found)| found)
}

fn get<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    dict.get(key).ok().map(|object| resolve(doc, object))
}

/// A name, text string or (the first of) an array of them, as text.
fn text_of(doc: &Document, object: &Object) -> String {
    let object = resolve(doc, object);
    match object {
        Object::Name(name) => String::from_utf8_lossy(name).into_owned(),
        Object::String(..) => decode_text_string(object).unwrap_or_default(),
        Object::Array(items) => items.first().map(|item| text_of(doc, item)).unwrap_or_default(),
        _ => String::new(),
    }
}

/// The page (from 0) each annotation is on, and each page's.
fn page_indexes(doc: &Document) -> (HashMap<ObjectId, u32>, HashMap<ObjectId, u32>) {
    let (mut annotations, mut pages) = (HashMap::new(), HashMap::new());
    for (number, page) in doc.get_pages() {
        let index = number.saturating_sub(1);
        pages.insert(page, index);
        let Ok(dict) = doc.get_dictionary(page) else { continue };
        for annotation in get(doc, dict, b"Annots").and_then(|a| a.as_array().ok()).into_iter().flatten() {
            if let Ok(id) = annotation.as_reference() {
                annotations.insert(id, index);
            }
        }
    }
    (annotations, pages)
}

/// A rectangle as its corner and size.
fn rect_of(doc: &Document, dict: &Dictionary) -> Option<[f64; 4]> {
    let numbers: Vec<f64> = get(doc, dict, b"Rect")?
        .as_array()
        .ok()?
        .iter()
        .filter_map(|n| resolve(doc, n).as_float().ok().map(f64::from))
        .collect();
    let &[a, b, c, d] = numbers.as_slice() else { return None };
    Some([a.min(c), b.min(d), (c - a).abs(), (d - b).abs()])
}

/// The name a check box or radio button's box draws as when it is on.
fn on_value(doc: &Document, widget: &Dictionary) -> String {
    get(doc, widget, b"AP")
        .and_then(|ap| ap.as_dict().ok())
        .and_then(|ap| get(doc, ap, b"N"))
        .and_then(|normal| normal.as_dict().ok())
        .and_then(|normal| normal.iter().map(|(key, _)| key).find(|key| key.as_slice() != b"Off"))
        .map(|key| String::from_utf8_lossy(key).into_owned())
        .unwrap_or_default()
}

/// A list's choices and the text they show: `(Red)` or `[(r) (Red)]`.
fn options_of(doc: &Document, dict: &Dictionary) -> (Vec<String>, Vec<String>) {
    let Some(Ok(list)) = get(doc, dict, b"Opt").map(Object::as_array) else { return Default::default() };
    list.iter()
        .map(|option| match resolve(doc, option) {
            Object::Array(pair) if pair.len() >= 2 => (text_of(doc, &pair[0]), text_of(doc, &pair[1])),
            other => {
                let text = text_of(doc, other);
                (text.clone(), text)
            }
        })
        .unzip()
}

/// The text size a default appearance (`/Helv 10 Tf 0 g`) sets.
fn appearance_size(appearance: &str) -> f64 {
    let tokens: Vec<&str> = appearance.split_whitespace().collect();
    tokens
        .iter()
        .position(|&t| t == "Tf")
        .and_then(|at| tokens.get(at.checked_sub(1)?))
        .and_then(|size| size.parse().ok())
        .unwrap_or(0.0)
}

/// The colour operator a default appearance sets; black when none.
fn appearance_color(appearance: &str) -> String {
    let tokens: Vec<&str> = appearance.split_whitespace().collect();
    for (at, &token) in tokens.iter().enumerate() {
        let operands = match token {
            "g" => 1,
            "rg" => 3,
            "k" => 4,
            _ => continue,
        };
        if at >= operands {
            return tokens[at - operands..=at].join(" ");
        }
    }
    "0 g".into()
}

fn walk(
    doc: &Document,
    id: ObjectId,
    parent: &Inherited,
    pages: &(HashMap<ObjectId, u32>, HashMap<ObjectId, u32>),
    depth: usize,
    out: &mut Vec<Field>,
) {
    let Ok(dict) = doc.get_dictionary(id) else { return };
    if depth > 32 {
        return;
    }
    let mut own = parent.clone();
    if let Some(part) = get(doc, dict, b"T") {
        let part = text_of(doc, part);
        own.name = if parent.name.is_empty() { part } else { format!("{}.{part}", parent.name) };
    }
    if let Some(Ok(kind)) = get(doc, dict, b"FT").map(Object::as_name) {
        own.kind = kind.to_vec();
    }
    if let Some(Ok(flags)) = get(doc, dict, b"Ff").map(Object::as_i64) {
        own.flags = flags;
    }
    if let Some(value) = get(doc, dict, b"V") {
        own.value = Some(value.clone());
    }
    if let Some(appearance) = get(doc, dict, b"DA") {
        own.appearance = text_of(doc, appearance);
    }
    // Kids with names are fields of their own; those without are this
    // field's boxes.
    let kids: Vec<ObjectId> = get(doc, dict, b"Kids")
        .and_then(|kids| kids.as_array().ok())
        .into_iter()
        .flatten()
        .filter_map(|kid| kid.as_reference().ok())
        .collect();
    let (fields, mut widgets): (Vec<ObjectId>, Vec<ObjectId>) =
        kids.into_iter().partition(|&kid| doc.get_dictionary(kid).is_ok_and(|d| d.has(b"T")));
    for &kid in &fields {
        walk(doc, kid, &own, pages, depth + 1, out);
    }
    if get(doc, dict, b"Subtype").and_then(|s| s.as_name().ok()) == Some(b"Widget".as_slice()) {
        widgets.insert(0, id);
    }
    if widgets.is_empty() {
        return;
    }
    let kind = match own.kind.as_slice() {
        b"Tx" => "text",
        b"Btn" if own.flags & PUSH_BUTTON != 0 => "button",
        b"Btn" if own.flags & RADIO != 0 => "radio",
        b"Btn" => "checkbox",
        b"Ch" => "choice",
        b"Sig" => "signature",
        _ => return,
    };
    let switch = matches!(kind, "checkbox" | "radio");
    let boxes: Vec<(ObjectId, PdfWidget)> = widgets
        .into_iter()
        .filter_map(|widget| {
            let wd = doc.get_dictionary(widget).ok()?;
            let [x, y, width, height] = rect_of(doc, wd).unwrap_or_default();
            let page = pages.0.get(&widget).copied().or_else(|| {
                let page = wd.get(b"P").ok()?.as_reference().ok()?;
                pages.1.get(&page).copied()
            });
            let on = if switch { on_value(doc, wd) } else { String::new() };
            Some((
                widget,
                PdfWidget { page_index: page.unwrap_or(u32::MAX), x, y, width, height, on_value: on },
            ))
        })
        .collect();
    let mut value = own.value.as_ref().map(|v| text_of(doc, v)).unwrap_or_default();
    if switch && value.is_empty() {
        value = "Off".into();
    }
    let (options, option_labels) = options_of(doc, dict);
    out.push(Field {
        id,
        info: PdfFormField {
            name: own.name.clone(),
            kind: kind.into(),
            value,
            options,
            option_labels,
            multiline: kind == "text" && own.flags & MULTILINE != 0,
            combo: kind == "choice" && own.flags & COMBO != 0,
            read_only: own.flags & READ_ONLY != 0,
            max_length: get(doc, dict, b"MaxLen")
                .and_then(|n| n.as_i64().ok())
                .map_or(0, |n| u32::try_from(n).unwrap_or(0)),
            font_size: appearance_size(&own.appearance),
            widgets: boxes.iter().filter(|(_, w)| w.page_index != u32::MAX).map(|(_, w)| w.clone()).collect(),
        },
        boxes,
        color: appearance_color(&own.appearance),
    });
}

/// The fields of a document's form, in its order.
fn read_fields(doc: &Document) -> Vec<Field> {
    let Ok(catalog) = doc.catalog() else { return Vec::new() };
    let Some(Ok(form)) = get(doc, catalog, b"AcroForm").map(Object::as_dict) else { return Vec::new() };
    let root = Inherited {
        appearance: get(doc, form, b"DA").map(|da| text_of(doc, da)).unwrap_or_default(),
        ..Inherited::default()
    };
    let pages = page_indexes(doc);
    let mut out = Vec::new();
    for field in get(doc, form, b"Fields").and_then(|f| f.as_array().ok()).into_iter().flatten() {
        if let Ok(id) = field.as_reference() {
            walk(doc, id, &root, &pages, 0, &mut out);
        }
    }
    out
}

/// Text in WinAnsi, the encoding of the standard fonts; none when it has
/// characters that encoding lacks.
fn win_ansi(text: &str) -> Option<Vec<u8>> {
    text.chars()
        .map(|c| match u32::from(c) {
            code @ (0x20..=0x7e | 0xa0..=0xff) => u8::try_from(code).ok(),
            _ => None,
        })
        .collect()
}

/// Lines of text no wider than `room` points, at a size, as Helvetica
/// sets them (about half an em to a character).
fn wrap_lines(text: &str, size: f64, room: f64) -> Vec<String> {
    let fits = ((room / (size * 0.52)).floor() as usize).max(1);
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > fits {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines
}

/// A PDF string of WinAnsi bytes: `(…)`, with `(`, `)` and `\` escaped.
fn pdf_string(bytes: &[u8], out: &mut Vec<u8>) {
    out.push(b'(');
    for &b in bytes {
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
}

/// How a text field's box draws its text, in Helvetica; none when the text
/// needs another font (readers then draw it themselves).
fn text_appearance(field: &Field, place: &PdfWidget, text: &str) -> Option<Stream> {
    let (w, h) = (place.width, place.height);
    let size = if field.info.font_size > 0.0 {
        field.info.font_size
    } else if field.info.multiline {
        10.0
    } else {
        (h * 0.7).clamp(4.0, 12.0)
    };
    let lines = if field.info.multiline {
        wrap_lines(text, size, w - 4.0)
    } else {
        vec![text.replace(['\n', '\r'], " ")]
    };
    let encoded: Vec<Vec<u8>> = lines.iter().map(String::as_str).map(win_ansi).collect::<Option<_>>()?;
    let mut content =
        format!("/Tx BMC q 1 1 {:.2} {:.2} re W n BT /Helv {size:.2} Tf {} ", w - 2.0, h - 2.0, field.color)
            .into_bytes();
    let first = if field.info.multiline { h - 2.0 - size } else { (h - size * 0.78) / 2.0 };
    content.extend(format!("2 {first:.2} Td {:.2} TL ", size * 1.15).into_bytes());
    for (i, line) in encoded.iter().enumerate() {
        if i > 0 {
            content.extend_from_slice(b"T* ");
        }
        pdf_string(line, &mut content);
        content.extend_from_slice(b" Tj ");
    }
    content.extend_from_slice(b"ET Q EMC");
    let mut font = Dictionary::new();
    font.set("Type", "Font");
    font.set("Subtype", "Type1");
    font.set("BaseFont", "Helvetica");
    font.set("Encoding", "WinAnsiEncoding");
    let mut fonts = Dictionary::new();
    fonts.set("Helv", font);
    let mut resources = Dictionary::new();
    resources.set("Font", fonts);
    let mut dict = Dictionary::new();
    dict.set("Type", "XObject");
    dict.set("Subtype", "Form");
    dict.set("BBox", vec![Object::Integer(0), Object::Integer(0), Object::from(w), Object::from(h)]);
    dict.set("Resources", resources);
    Some(Stream::new(dict, content))
}

fn set(doc: &mut Document, id: ObjectId, key: &str, value: Object) -> Result<(), CoreError> {
    doc.get_dictionary_mut(id).map_err(unreadable)?.set(key, value);
    Ok(())
}

/// Puts values into a document's fields (those not read only); how many
/// changed. Text the standard fonts cannot draw is left for readers to
/// draw (the form asks them to).
fn fill(doc: &mut Document, values: &[PdfFieldValue]) -> Result<u32, CoreError> {
    let fields = read_fields(doc);
    let mut changed = 0;
    let mut readers_draw = false;
    for value in values {
        let Some(field) = fields.iter().find(|f| f.info.name == value.name) else { continue };
        if field.info.read_only {
            continue;
        }
        match field.info.kind.as_str() {
            "text" | "choice" => {
                let text: String = match field.info.max_length {
                    0 => value.value.clone(),
                    most => value.value.chars().take(most as usize).collect(),
                };
                set(doc, field.id, "V", text_string(&text))?;
                for (widget, place) in &field.boxes {
                    let look =
                        if field.info.kind == "text" { text_appearance(field, place, &text) } else { None };
                    if let Some(look) = look {
                        let look = doc.add_object(look);
                        let mut appearance = Dictionary::new();
                        appearance.set("N", Object::Reference(look));
                        set(doc, *widget, "AP", appearance.into())?;
                    } else {
                        doc.get_dictionary_mut(*widget).map_err(unreadable)?.remove(b"AP");
                        readers_draw = true;
                    }
                }
            }
            "checkbox" | "radio" => {
                let on = value.value != "Off" && field.boxes.iter().any(|(_, w)| w.on_value == value.value);
                let state = if on { value.value.as_str() } else { "Off" };
                set(doc, field.id, "V", state.into())?;
                for (widget, place) in &field.boxes {
                    set(
                        doc,
                        *widget,
                        "AS",
                        if on && place.on_value == value.value { state } else { "Off" }.into(),
                    )?;
                }
            }
            _ => continue,
        }
        changed += 1;
    }
    if changed > 0 {
        let form = match doc.catalog().map_err(unreadable)?.get(b"AcroForm") {
            Ok(Object::Reference(id)) => Some(*id),
            _ => None,
        };
        let form = match form {
            Some(id) => doc.get_dictionary_mut(id),
            None => doc.catalog_mut().and_then(|c| c.get_mut(b"AcroForm")).and_then(Object::as_dict_mut),
        }
        .map_err(unreadable)?;
        // An XFA form would stand in for the values written here.
        form.remove(b"XFA");
        if readers_draw {
            form.set("NeedAppearances", true);
        }
    }
    Ok(changed)
}

#[uniffi::export]
impl PaperLibrary {
    /// The fields of an attached PDF's form, with where their boxes lie;
    /// none when it has no form.
    pub fn pdf_form_fields(&self, attachment_id: String) -> Result<Vec<PdfFormField>, CoreError> {
        let (_, path) = self.pdf_path(&attachment_id)?;
        let doc = Document::load(&path).map_err(unreadable)?;
        Ok(read_fields(&doc).into_iter().map(|f| f.info).filter(|f| !f.widgets.is_empty()).collect())
    }

    /// Fills in an attached PDF's form: each value goes into the field it
    /// names, and the file is replaced in one step, so other readers show
    /// what was filled in.
    pub fn fill_pdf_form(
        &self,
        attachment_id: String,
        values: Vec<PdfFieldValue>,
    ) -> Result<AttachmentInfo, CoreError> {
        let (attachment, path) = self.pdf_path(&attachment_id)?;
        let mut doc = Document::load(&path).map_err(unreadable)?;
        if doc.is_encrypted() {
            return Err(CoreError::InvalidInput {
                message: "This PDF is protected and cannot be changed.".into(),
            });
        }
        if fill(&mut doc, &values)? == 0 {
            return Ok(self.files()?.info(&attachment));
        }
        replace(&mut doc, &path)?;
        let (sha256, size) = crate::files::sha256_and_size(&path)?;
        let updated = {
            let mut store = self.lock()?;
            store.set_attachment_content(&attachment.id, size, &sha256)?;
            store.attachment(&attachment.id)?.unwrap_or_else(|| attachment.clone())
        };
        Ok(self.files()?.info(&updated))
    }
}

#[cfg(test)]
mod tests;
