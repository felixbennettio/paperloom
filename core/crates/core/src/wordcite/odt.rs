//! OpenDocument character data and inline notes, with edits limited to their source ranges.

use super::*;

pub(super) const MIMETYPE: &[u8] = b"application/vnd.oasis.opendocument.text";

pub(super) fn is_document(entries: &[unzip::Entry<'_>]) -> bool {
    entries.iter().any(|e| e.name == "mimetype" && e.contents(XML_LIMIT).as_deref() == Some(MIMETYPE))
        || entries.iter().any(|e| {
            e.name == "content.xml"
                && e.contents(XML_LIMIT)
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .is_some_and(|xml| Tags::new(&xml).any(|t| t.name == "office:text" && !t.closing))
        })
}

/// Text containers and inline markup carry plain text; other text elements are fields.
fn hidden(name: &str) -> bool {
    name == "office:annotation"
        || (name.starts_with("text:")
            && !matches!(
                name,
                "text:p"
                    | "text:h"
                    | "text:span"
                    | "text:a"
                    | "text:s"
                    | "text:tab"
                    | "text:line-break"
                    | "text:soft-page-break"
                    | "text:bookmark"
                    | "text:bookmark-start"
                    | "text:bookmark-end"
                    | "text:reference-mark"
                    | "text:reference-mark-start"
                    | "text:reference-mark-end"
                    | "text:change"
                    | "text:change-start"
                    | "text:change-end"
                    | "text:meta"
                    | "text:ruby"
                    | "text:ruby-base"
                    | "text:note"
                    | "text:note-body"
                    | "text:section"
                    | "text:list"
                    | "text:list-item"
                    | "text:list-header"
                    | "text:numbered-paragraph"
            ))
}

fn subtree_end(tags: &mut Tags<'_>, opening: &Tag<'_>) -> Result<usize, CoreError> {
    if opening.empty {
        return Ok(opening.end);
    }
    let mut depth = 1usize;
    for tag in tags.by_ref() {
        if tag.closing {
            depth -= 1;
            if depth == 0 {
                return Ok(tag.end);
            }
        } else if !tag.empty {
            depth += 1;
        }
    }
    Err(unreadable())
}

/// A note's paragraphs do not disqualify the paragraph containing its anchor.
pub(super) fn paragraphs(xml: &str) -> Result<Vec<Paragraph>, CoreError> {
    let body = elements(xml, "office:text").into_iter().next().ok_or_else(invalid)?;
    let base = body.open_end;
    let mut tags = Tags::new(&xml[base..body.close_start]);
    let mut open: Vec<(Element, bool, usize)> = Vec::new();
    let mut found = Vec::new();
    let mut note_depth = 0usize;
    while let Some(tag) = tags.next() {
        if !tag.closing && hidden(tag.name) {
            subtree_end(&mut tags, &tag)?;
            continue;
        }
        if tag.name == "text:note" && !tag.empty {
            if tag.closing {
                note_depth = note_depth.saturating_sub(1);
            } else {
                note_depth += 1;
            }
        }
        if !matches!(tag.name, "text:p" | "text:h") {
            continue;
        }
        if tag.closing {
            if let Some((mut element, nested, _)) = open.pop()
                && !nested
            {
                element.close_start = base + tag.start;
                element.end = base + tag.end;
                found.push(element);
            }
        } else {
            if let Some((_, nested, parent_note)) = open.last_mut()
                && *parent_note == note_depth
            {
                *nested = true;
            }
            let element = Element {
                start: base + tag.start,
                open_end: base + tag.end,
                close_start: base + tag.end,
                end: base + tag.end,
            };
            if tag.empty {
                found.push(element);
            } else {
                open.push((element, false, note_depth));
            }
        }
    }
    found.sort_by_key(|e| e.start);
    let mut remaining = XML_LIMIT;
    found
        .into_iter()
        .map(|e| {
            let paragraph = read(xml, e)?;
            remaining = remaining.checked_sub(paragraph.text.len() as u64).ok_or_else(unreadable)?;
            Ok(paragraph)
        })
        .collect()
}

fn push_node(paragraph: &mut Paragraph, raw: Range<usize>, text: String) -> Result<(), CoreError> {
    let start = paragraph.text.len();
    if start.checked_add(text.len()).is_none_or(|len| len as u64 > XML_LIMIT) {
        return Err(unreadable());
    }
    paragraph.text.push_str(&text);
    paragraph.nodes.push(TextNode {
        element: Element { start: raw.start, open_end: raw.start, close_start: raw.end, end: raw.end },
        range: start..paragraph.text.len(),
        text,
    });
    Ok(())
}

/// Tags skips comments, CDATA and processing instructions; separate them from character data.
fn characters(xml: &str, raw: Range<usize>, paragraph: &mut Paragraph) -> Result<(), CoreError> {
    let mut at = raw.start;
    while at < raw.end {
        let next = xml[at..raw.end].find('<').map_or(raw.end, |n| at + n);
        if at < next {
            push_node(paragraph, at..next, decode(&xml[at..next]))?;
        }
        if next == raw.end {
            break;
        }
        let tail = &xml[next..raw.end];
        let (marker, prefix) = if tail.starts_with("<![CDATA[") {
            ("]]>", 9)
        } else if tail.starts_with("<!--") {
            ("-->", 4)
        } else if tail.starts_with("<?") {
            ("?>", 2)
        } else {
            return Err(unreadable());
        };
        let close = next + prefix + tail[prefix..].find(marker).ok_or_else(unreadable)?;
        at = close + marker.len();
        if prefix == 9 {
            push_node(paragraph, next..at, xml[next + prefix..close].to_string())?;
        }
    }
    Ok(())
}

fn read(xml: &str, element: Element) -> Result<Paragraph, CoreError> {
    let base = element.open_end;
    let inner = &xml[base..element.close_start];
    let mut paragraph = Paragraph { element, text: String::new(), nodes: Vec::new(), edits: Vec::new() };
    let mut tags = Tags::new(inner);
    let mut at = 0;
    while let Some(tag) = tags.next() {
        characters(xml, base + at..base + tag.start, &mut paragraph)?;
        at = tag.end;
        if tag.closing {
            continue;
        }
        if tag.name == "text:note" || hidden(tag.name) {
            at = subtree_end(&mut tags, &tag)?;
            continue;
        }
        let text = match tag.name {
            "text:s" => {
                let opening = &inner[tag.start..tag.end];
                let count = attribute(opening, "text:c")
                    .and_then(|r| decode(&opening[r]).parse::<usize>().ok())
                    .unwrap_or(1);
                if count as u64 > XML_LIMIT.saturating_sub(paragraph.text.len() as u64) {
                    return Err(unreadable());
                }
                " ".repeat(count)
            }
            "text:tab" => "\t".into(),
            "text:line-break" => "\n".into(),
            _ => continue,
        };
        at = subtree_end(&mut tags, &tag)?;
        push_node(&mut paragraph, base + tag.start..base + at, text)?;
    }
    characters(xml, base + at..base + inner.len(), &mut paragraph)?;
    Ok(paragraph)
}

fn rewrite_nodes(paragraph: &Paragraph, edits: &mut Vec<Edit>) {
    let mut per_node: HashMap<usize, Vec<Edit>> = HashMap::new();
    for edit in &paragraph.edits {
        let first = paragraph.nodes.partition_point(|n| n.range.end <= edit.range.start);
        for (index, node) in paragraph.nodes.iter().enumerate().skip(first) {
            if node.range.start >= edit.range.end {
                break;
            }
            if node.range.is_empty() {
                continue;
            }
            let start = edit.range.start.max(node.range.start) - node.range.start;
            let end = edit.range.end.min(node.range.end) - node.range.start;
            let replacement =
                if node.range.contains(&edit.range.start) { edit.replacement.clone() } else { String::new() };
            per_node.entry(index).or_default().push(Edit { range: start..end, replacement });
        }
    }
    for (index, changes) in per_node {
        let node = &paragraph.nodes[index];
        edits.push(Edit {
            range: node.element.start..node.element.end,
            replacement: escape(&apply(&node.text, changes)),
        });
    }
}

pub(super) fn rewrite(
    xml: &str,
    paragraphs: &[Paragraph],
    entries: &[String],
    style: &str,
) -> Result<(String, u32), CoreError> {
    let mut edits = Vec::new();
    let mut references = 0;
    let mut covered_until = 0;
    for paragraph in paragraphs {
        if paragraph.element.start < covered_until {
            continue;
        }
        if !entries.is_empty() && paragraph.placeholder() {
            let opening = &xml[paragraph.element.start..paragraph.element.open_end];
            let closing = &xml[paragraph.element.close_start..paragraph.element.end];
            edits.push(Edit {
                range: paragraph.element.start..paragraph.element.end,
                replacement: entries.iter().map(|e| format!("{opening}{}{closing}", escape(e))).collect(),
            });
            covered_until = paragraph.element.end;
            references += entries.len() as u32;
        } else {
            rewrite_nodes(paragraph, &mut edits);
        }
    }
    if references == 0 && !entries.is_empty() {
        let body = elements(xml, "office:text").into_iter().next().ok_or_else(invalid)?;
        let at = body.close_start;
        let heading = if style.starts_with("gb-") { "参考文献" } else { "References" };
        let mut list = format!(r#"<text:h text:outline-level="1">{heading}</text:h>"#);
        for entry in entries {
            list.push_str(&format!("<text:p>{}</text:p>", escape(entry)));
        }
        edits.push(Edit { range: at..at, replacement: list });
        references = entries.len() as u32;
    }
    Ok((apply(xml, edits), references))
}
