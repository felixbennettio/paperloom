//! Formats Pandoc citations in a copy of a Word or OpenDocument text
//! document, keeping its formatting and the rest of the package as written.

mod odt;

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read as _;
use std::ops::Range;
use std::path::Path;

use paperloom_citation::{Cite, Format};
use serde::{Deserialize, Serialize};

use crate::backup::ZipWriter;
use crate::notelinks::CitationResolver;
use crate::{CoreError, PaperLibrary, unzip};

const SOURCE_LIMIT: u64 = 200 << 20;
const XML_LIMIT: u64 = 64 << 20;
const PARTS: [&str; 3] = ["word/document.xml", "word/footnotes.xml", "word/endnotes.xml"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct WordCitations {
    /// Citations formatted.
    pub citations: u32,
    /// Entries in the reference list.
    pub references: u32,
    /// Keys the library does not have, as written, each once, in order.
    pub missing: Vec<String>,
}

fn invalid() -> CoreError {
    CoreError::InvalidInput { message: "This is not a Word document (.docx).".into() }
}

fn unreadable() -> CoreError {
    CoreError::InvalidInput { message: "The Word document could not be read.".into() }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Word's five XML entities and decimal/hexadecimal character references.
fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';') else { break };
        let entity = &rest[1..end];
        let character = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|n| u32::from_str_radix(n, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|n| n.parse().ok()))
                .and_then(char::from_u32),
        };
        if let Some(character) = character {
            out.push(character);
        } else {
            out.push_str(&rest[..=end]);
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

struct Tag<'a> {
    name: &'a str,
    start: usize,
    end: usize,
    closing: bool,
    empty: bool,
}

/// Quoted attributes may contain `>`; comments and processing instructions
/// do not participate in the element tree.
struct Tags<'a> {
    xml: &'a str,
    at: usize,
}

impl<'a> Tags<'a> {
    fn new(xml: &'a str) -> Self {
        Self { xml, at: 0 }
    }
}

impl<'a> Iterator for Tags<'a> {
    type Item = Tag<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let start = self.at + self.xml[self.at..].find('<')?;
            let tail = &self.xml[start..];
            let special = if tail.starts_with("<!--") {
                Some("-->")
            } else if tail.starts_with("<![CDATA[") {
                Some("]]>")
            } else if tail.starts_with("<?") {
                Some("?>")
            } else {
                None
            };
            if let Some(end) = special {
                self.at = start + tail.find(end)? + end.len();
                continue;
            }
            let mut quote = None;
            let mut end = None;
            for (offset, c) in tail.char_indices().skip(1) {
                match (quote, c) {
                    (Some(q), c) if q == c => quote = None,
                    (None, '\'' | '"') => quote = Some(c),
                    (None, '>') => {
                        end = Some(start + offset + 1);
                        break;
                    }
                    _ => {}
                }
            }
            let end = end?;
            self.at = end;
            let inside = self.xml[start + 1..end - 1].trim();
            if inside.starts_with('!') {
                continue;
            }
            let closing = inside.starts_with('/');
            let name = inside.trim_start_matches('/').split([' ', '\t', '\n', '\r', '/']).next()?;
            return Some(Tag { name, start, end, closing, empty: inside.ends_with('/') });
        }
    }
}

struct Element {
    start: usize,
    open_end: usize,
    close_start: usize,
    end: usize,
}

/// Innermost elements only: a text box's paragraph belongs to itself.
fn elements(xml: &str, name: &str) -> Vec<Element> {
    let mut open: Vec<(Element, bool)> = Vec::new();
    let mut found = Vec::new();
    for tag in Tags::new(xml).filter(|t| t.name == name) {
        if tag.closing {
            if let Some((mut element, nested)) = open.pop()
                && !nested
            {
                element.close_start = tag.start;
                element.end = tag.end;
                found.push(element);
            }
        } else {
            if let Some((_, nested)) = open.last_mut() {
                *nested = true;
            }
            let element = Element { start: tag.start, open_end: tag.end, close_start: tag.end, end: tag.end };
            if tag.empty {
                found.push(element);
            } else {
                open.push((element, false));
            }
        }
    }
    found.sort_by_key(|e| e.start);
    found
}

fn direct_child<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let mut depth = 0usize;
    let mut start = None;
    for tag in Tags::new(xml) {
        if tag.closing {
            depth = depth.saturating_sub(1);
            if depth == 0
                && let Some(start) = start
            {
                return Some(&xml[start..tag.end]);
            }
        } else {
            if depth == 0 && tag.name == name {
                if tag.empty {
                    return Some(&xml[tag.start..tag.end]);
                }
                start = Some(tag.start);
            }
            if !tag.empty {
                depth += 1;
            }
        }
    }
    None
}

struct TextNode {
    element: Element,
    /// Decoded byte range in the paragraph.
    range: Range<usize>,
    text: String,
}

struct Paragraph {
    element: Element,
    text: String,
    nodes: Vec<TextNode>,
    edits: Vec<Edit>,
}

impl Paragraph {
    fn read(xml: &str, element: Element) -> Self {
        let mut text = String::new();
        let mut nodes = Vec::new();
        let paragraph = &xml[element.open_end..element.close_start];
        let mut tags = Tags::new(paragraph);
        while let Some(tag) = tags.next() {
            if tag.closing {
                continue;
            }
            match tag.name {
                "w:t" if !tag.empty => {
                    let Some(close) = tags.find(|t| t.closing && t.name == "w:t") else { break };
                    let decoded = decode(&paragraph[tag.end..close.start]);
                    let start = text.len();
                    text.push_str(&decoded);
                    let base = element.open_end;
                    nodes.push(TextNode {
                        element: Element {
                            start: base + tag.start,
                            open_end: base + tag.end,
                            close_start: base + close.start,
                            end: base + close.end,
                        },
                        range: start..text.len(),
                        text: decoded,
                    });
                }
                "w:tab" => text.push('\t'),
                "w:br" | "w:cr" => text.push('\n'),
                _ => {}
            }
        }
        Self { element, text, nodes, edits: Vec::new() }
    }

    fn placeholder(&self) -> bool {
        let text = self.text.trim();
        text.eq_ignore_ascii_case("{bibliography}")
            || text.eq_ignore_ascii_case("{references}")
            || text == "{参考文献}"
    }

    fn rewrite(&self, xml: &str, edits: &mut Vec<Edit>) {
        let mut per_node: HashMap<usize, Vec<Edit>> = HashMap::new();
        for edit in &self.edits {
            let first = self.nodes.partition_point(|n| n.range.end <= edit.range.start);
            for (index, node) in self.nodes.iter().enumerate().skip(first) {
                if node.range.start >= edit.range.end {
                    break;
                }
                if node.range.is_empty() {
                    continue;
                }
                let start = edit.range.start.max(node.range.start) - node.range.start;
                let end = edit.range.end.min(node.range.end) - node.range.start;
                let replacement = if node.range.contains(&edit.range.start) {
                    edit.replacement.clone()
                } else {
                    String::new()
                };
                per_node.entry(index).or_default().push(Edit { range: start..end, replacement });
            }
        }
        for (index, changes) in per_node {
            let node = &self.nodes[index];
            let text = apply(&node.text, changes);
            let mut opening = xml[node.element.start..node.element.open_end].to_string();
            if text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace) {
                if let Some(value) = attribute(&opening, "xml:space") {
                    opening.replace_range(value, "preserve");
                } else {
                    opening.insert_str(opening.len() - 1, r#" xml:space="preserve""#);
                }
            }
            edits.push(Edit {
                range: node.element.start..node.element.close_start,
                replacement: format!("{opening}{}", escape(&text)),
            });
        }
    }
}

/// The value's byte range, without quotes, in an opening tag.
fn attribute(opening: &str, wanted: &str) -> Option<Range<usize>> {
    let bytes = opening.as_bytes();
    let mut at = opening.find(char::is_whitespace)?;
    while at < bytes.len() {
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let start = at;
        while bytes.get(at).is_some_and(|b| !b.is_ascii_whitespace() && *b != b'=' && *b != b'>') {
            at += 1;
        }
        let name = &opening[start..at];
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if bytes.get(at) != Some(&b'=') {
            return None;
        }
        at += 1;
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let quote = *bytes.get(at)?;
        at += 1;
        let start = at;
        at += bytes[at..].iter().position(|&b| b == quote)?;
        if name == wanted {
            return Some(start..at);
        }
        at += 1;
    }
    None
}

struct Edit {
    range: Range<usize>,
    replacement: String,
}

fn apply(text: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|e| e.range.start);
    let mut out = String::new();
    let mut at = 0;
    for edit in edits {
        out.push_str(&text[at..edit.range.start]);
        out.push_str(&edit.replacement);
        at = edit.range.end;
    }
    out.push_str(&text[at..]);
    out
}

struct Part {
    index: usize,
    xml: String,
    paragraphs: Vec<Paragraph>,
}

struct Location {
    part: usize,
    paragraph: usize,
    range: Range<usize>,
}

fn reference_paragraph(text: &str, paragraph_properties: &str, run_properties: &str) -> String {
    format!(
        r#"<w:p>{paragraph_properties}<w:r>{run_properties}<w:t xml:space="preserve">{}</w:t></w:r></w:p>"#,
        escape(text)
    )
}

fn reference_list(xml: &str, paragraph: &Paragraph, entries: &[String]) -> String {
    let inner = &xml[paragraph.element.open_end..paragraph.element.close_start];
    let properties = direct_child(inner, "w:pPr").unwrap_or_default();
    let run = elements(inner, "w:r").into_iter().next();
    let look = run
        .as_ref()
        .and_then(|r| direct_child(&inner[r.open_end..r.close_start], "w:rPr"))
        .unwrap_or_default();
    entries.iter().map(|e| reference_paragraph(e, properties, look)).collect()
}

/// Before the body's own final section properties, never a paragraph's.
fn body_end(xml: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut body_depth = None;
    let mut section = None;
    for tag in Tags::new(xml) {
        if tag.closing {
            depth = depth.saturating_sub(1);
            if tag.name == "w:body" && body_depth == Some(depth) {
                return Some(section.unwrap_or(tag.start));
            }
        } else {
            if tag.name == "w:body" {
                body_depth = Some(depth);
            } else if tag.name == "w:sectPr" && body_depth == depth.checked_sub(1) {
                section = Some(tag.start);
            }
            if !tag.empty {
                depth += 1;
            }
        }
    }
    None
}

#[uniffi::export]
impl PaperLibrary {
    /// Writes a copy of the Word or OpenDocument text document at `source`
    /// to `destination` with its `[@citations]` formatted in a style (an id
    /// from `citation_styles()`) and the reference list added.
    pub fn cite_word_document(
        &self,
        source: String,
        destination: String,
        style_id: String,
    ) -> Result<WordCitations, CoreError> {
        let mut bytes = Vec::new();
        File::open(&source)
            .and_then(|f| f.take(SOURCE_LIMIT + 1).read_to_end(&mut bytes))
            .map_err(|e| CoreError::Storage { message: format!("Reading {source}: {e}") })?;
        if bytes.len() as u64 > SOURCE_LIMIT {
            return Err(CoreError::InvalidInput {
                message: "The Word document is too large to read.".into(),
            });
        }
        let entries = unzip::entries(&bytes).ok_or_else(invalid)?;
        let open_document = odt::is_document(&entries);
        if !open_document && !entries.iter().any(|e| e.name == PARTS[0]) {
            return Err(invalid());
        }
        let mut parts = Vec::new();
        let names: &[&str] = if open_document { &["content.xml"] } else { &PARTS };
        for name in names {
            for (index, entry) in entries.iter().enumerate().filter(|(_, e)| e.name == *name) {
                let xml = String::from_utf8(entry.contents(XML_LIMIT).ok_or_else(unreadable)?)
                    .map_err(|_| unreadable())?;
                let paragraphs = if open_document {
                    odt::paragraphs(&xml)?
                } else {
                    elements(&xml, "w:p").into_iter().map(|e| Paragraph::read(&xml, e)).collect()
                };
                parts.push(Part { index, xml, paragraphs });
            }
        }
        if parts.is_empty() {
            return Err(invalid());
        }
        let resolver = {
            let store = self.lock()?;
            CitationResolver::new(&store)?
        };
        let mut items = Vec::new();
        let mut item_indices = HashMap::new();
        let mut groups: Vec<Vec<Cite>> = Vec::new();
        let mut locations = Vec::new();
        let mut missing = Vec::new();
        let mut seen_missing = HashSet::new();
        for (part_index, part) in parts.iter().enumerate() {
            let mut scanned: Vec<_> = part
                .paragraphs
                .iter()
                .enumerate()
                .flat_map(|(index, paragraph)| {
                    paperloom_vault::plain_citations(&paragraph.text).into_iter().map(move |c| (index, c))
                })
                .collect();
            if open_document {
                // Notes are inline: merge their citations at the anchor, not after the body paragraph.
                scanned.sort_by_key(|(index, citation)| {
                    let paragraph = &part.paragraphs[*index];
                    let first = paragraph.nodes.partition_point(|n| n.range.end <= citation.start);
                    let node = paragraph.nodes.get(first);
                    (node.map_or(paragraph.element.start, |n| n.element.start), citation.start)
                });
            }
            for (paragraph_index, citation) in scanned {
                let mut resolved = Vec::new();
                for work in &citation.works {
                    if let Some(item) = resolver.find(&work.key) {
                        resolved.push((item, work));
                    } else if seen_missing.insert(work.key.clone()) {
                        missing.push(work.key.clone());
                    }
                }
                if resolved.len() == citation.works.len() {
                    let group = resolved
                        .into_iter()
                        .map(|(item, work)| {
                            let index = *item_indices.entry(item.id.to_string()).or_insert_with(|| {
                                items.push(item.clone());
                                items.len() - 1
                            });
                            Cite { index, page: work.page.clone() }
                        })
                        .collect();
                    groups.push(group);
                    locations.push(Location {
                        part: part_index,
                        paragraph: paragraph_index,
                        range: citation.start..citation.end,
                    });
                }
            }
        }
        let formatted = if groups.is_empty() {
            paperloom_citation::Document::default()
        } else {
            paperloom_citation::format_document(&items, &groups, &style_id, Format::Text)?
        };
        let citations = formatted.citations.len() as u32;
        for (location, replacement) in locations.into_iter().zip(formatted.citations) {
            parts[location.part].paragraphs[location.paragraph]
                .edits
                .push(Edit { range: location.range, replacement });
        }
        let placeholder = parts.iter().any(|p| p.paragraphs.iter().any(Paragraph::placeholder));
        let mut references = 0;
        let mut rewritten = HashMap::new();
        for part in parts {
            if open_document {
                let (xml, count) = odt::rewrite(&part.xml, &part.paragraphs, &formatted.entries, &style_id)?;
                rewritten.insert(part.index, xml);
                references += count;
                continue;
            }
            let mut edits = Vec::new();
            for paragraph in &part.paragraphs {
                if !formatted.entries.is_empty() && paragraph.placeholder() {
                    edits.push(Edit {
                        range: paragraph.element.start..paragraph.element.end,
                        replacement: reference_list(&part.xml, paragraph, &formatted.entries),
                    });
                    references += formatted.entries.len() as u32;
                } else {
                    paragraph.rewrite(&part.xml, &mut edits);
                }
            }
            if !placeholder && !formatted.entries.is_empty() && entries[part.index].name == PARTS[0] {
                let at = body_end(&part.xml).ok_or_else(invalid)?;
                let heading = if style_id.starts_with("gb-") { "参考文献" } else { "References" };
                let mut list = reference_paragraph(heading, "", "<w:rPr><w:b/></w:rPr>");
                for entry in &formatted.entries {
                    list.push_str(&reference_paragraph(
                        entry,
                        r#"<w:pPr><w:ind w:left="480" w:hanging="480"/></w:pPr>"#,
                        "",
                    ));
                }
                edits.push(Edit { range: at..at, replacement: list });
                references += formatted.entries.len() as u32;
            }
            rewritten.insert(part.index, apply(&part.xml, edits));
        }
        let destination = Path::new(&destination);
        let partial = destination.with_extension(if open_document { "odt.partial" } else { "docx.partial" });
        let write = || -> Result<(), CoreError> {
            let mut zip = ZipWriter::create(&partial).map_err(|e| CoreError::Storage {
                message: format!("Writing {}: {e}", destination.display()),
            })?;
            let mut ordered: Vec<_> = entries.iter().enumerate().collect();
            if open_document {
                ordered.sort_by_key(|(_, entry)| entry.name != "mimetype");
            }
            for (index, entry) in ordered {
                let content = match rewritten.get(&index) {
                    Some(xml) => xml.as_bytes().to_vec(),
                    None => {
                        let limit = if entry.name.ends_with(".xml") || entry.name.ends_with(".rels") {
                            XML_LIMIT
                        } else {
                            u64::from(u32::MAX)
                        };
                        entry.contents(limit).ok_or_else(unreadable)?
                    }
                };
                zip.add_bytes(&entry.name, &content).map_err(|e| CoreError::Storage {
                    message: format!("Writing {}: {e}", destination.display()),
                })?;
            }
            zip.finish().map_err(|e| CoreError::Storage {
                message: format!("Writing {}: {e}", destination.display()),
            })?;
            fs::rename(&partial, destination)
                .map_err(|e| CoreError::Storage { message: format!("Saving {}: {e}", destination.display()) })
        };
        if let Err(error) = write() {
            let _ = fs::remove_file(&partial);
            return Err(error);
        }
        Ok(WordCitations { citations, references, missing })
    }
}

#[cfg(test)]
mod tests;
