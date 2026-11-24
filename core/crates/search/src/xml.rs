//! Just enough XML for arXiv's Atom feed and the MARCXML of library
//! catalogues: elements found by name, their attributes, and their text
//! with entities decoded.

pub(crate) struct Element<'a> {
    pub attrs: &'a str,
    pub body: &'a str,
}

/// The `name` elements in `xml`, in order. Elements of the same name must
/// not nest, which holds for Atom and MARCXML.
pub(crate) fn elements<'a>(xml: &'a str, name: &str) -> Vec<Element<'a>> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut found = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        if !after.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            rest = after;
            continue;
        }
        let Some(tag_end) = after.find('>') else { break };
        let attrs = &after[..tag_end];
        let after_tag = &after[tag_end + 1..];
        if let Some(attrs) = attrs.strip_suffix('/') {
            found.push(Element { attrs, body: "" });
            rest = after_tag;
            continue;
        }
        let Some(end) = after_tag.find(&close) else { break };
        found.push(Element { attrs, body: &after_tag[..end] });
        rest = &after_tag[end + close.len()..];
    }
    found
}

/// The text of the first `name` element, or `None` when there is none or
/// it is empty.
pub(crate) fn first_text(xml: &str, name: &str) -> Option<String> {
    elements(xml, name).first().map(|e| text(e.body)).filter(|t| !t.is_empty())
}

/// An attribute's value, decoded.
pub(crate) fn attr(attrs: &str, name: &str) -> Option<String> {
    let mut rest = attrs;
    while let Some(at) = rest.find(name) {
        let starts_name = at == 0 || rest[..at].ends_with(char::is_whitespace);
        let after = rest[at + name.len()..].trim_start();
        if starts_name && let Some(value) = after.strip_prefix('=') {
            let value = value.trim_start();
            let quote = value.chars().next().filter(|&q| q == '"' || q == '\'')?;
            let value = &value[1..];
            return Some(decode(&value[..value.find(quote)?]));
        }
        rest = &rest[at + name.len()..];
    }
    None
}

/// Element text: entities decoded, runs of whitespace (line breaks in long
/// titles and abstracts) made single spaces.
pub(crate) fn text(body: &str) -> String {
    decode(body).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Text with entities decoded and its spacing kept (MARC fixed fields mean
/// something by position).
pub(crate) fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let decoded = after.find(';').filter(|&end| end <= 10).and_then(|end| {
            let entity = &after[..end];
            let c = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .or_else(|| entity.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}
