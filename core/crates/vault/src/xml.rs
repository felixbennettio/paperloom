//! Just enough XML for other apps' mind map files: elements, attributes
//! and text. Comments, processing instructions and doctypes are skipped;
//! CDATA is kept as text. Mismatched closing tags are forgiven.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Content>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Content {
    Element(Element),
    Text(String),
}

impl Element {
    /// The name without a namespace prefix.
    pub fn local(&self) -> &str {
        self.name.rsplit(':').next().unwrap_or(&self.name)
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            Content::Element(e) => Some(e),
            Content::Text(_) => None,
        })
    }

    /// The first child element with this (local) name.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.local() == name)
    }

    /// All the text inside, tags dropped.
    pub fn text(&self) -> String {
        let mut out = String::new();
        collect(self, &mut out);
        out
    }
}

fn collect(element: &Element, out: &mut String) {
    for content in &element.children {
        match content {
            Content::Text(text) => out.push_str(text),
            Content::Element(child) => collect(child, out),
        }
    }
}

/// Text with its entities (`&amp;`, `&#x4E2D;`, …) replaced.
pub(crate) fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let entity = after.find(';').filter(|&end| end <= 10).map(|end| (&after[..end], end));
        let decoded = entity.and_then(|(name, end)| {
            let c = match name {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ => name
                    .strip_prefix('#')
                    .and_then(|n| match n.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => n.parse().ok(),
                    })
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

/// Text made safe for an attribute value or element content.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// An element as XML, its attributes and children as they are.
pub(crate) fn write_element(element: &Element, out: &mut String) {
    out.push('<');
    out.push_str(&element.name);
    for (key, value) in &element.attrs {
        out.push(' ');
        out.push_str(key);
        out.push_str("=\"");
        out.push_str(&escape(value));
        out.push('"');
    }
    if element.children.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for child in &element.children {
        match child {
            Content::Text(text) => out.push_str(&escape(text)),
            Content::Element(inner) => write_element(inner, out),
        }
    }
    out.push_str("</");
    out.push_str(&element.name);
    out.push('>');
}

/// Where a tag ends: the `>` not inside a quoted value.
fn tag_end(tag: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in tag.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// What follows a `<!DOCTYPE …>`, internal subset and all.
fn skip_declaration(rest: &str) -> &str {
    let mut depth = 0i32;
    for (i, c) in rest.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            '>' if depth <= 0 => return &rest[i + 1..],
            _ => {}
        }
    }
    ""
}

/// A start tag's name and attributes, from what is between `<` and `>`.
fn start_tag(inner: &str) -> Element {
    let inner = inner.trim();
    let name_end = inner.find(char::is_whitespace).unwrap_or(inner.len());
    let mut element = Element { name: inner[..name_end].to_string(), ..Element::default() };
    let mut rest = inner[name_end..].trim_start();
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
        let value = &after[1..];
        let Some(close) = value.find(quote) else { break };
        element.attrs.push((key, decode(&value[..close])));
        rest = value[close + 1..].trim_start();
    }
    element
}

/// Text between tags; the indentation between elements is dropped.
fn push_text(stack: &mut [Element], text: &str) {
    if text.is_empty() || (text.trim().is_empty() && text.contains('\n')) {
        return;
    }
    if let Some(top) = stack.last_mut() {
        top.children.push(Content::Text(decode(text)));
    }
}

/// The document's root element; `None` when there is none.
pub(crate) fn parse_xml(source: &str) -> Option<Element> {
    // The bottom of the stack holds the root once it closes.
    let mut stack = vec![Element::default()];
    let mut rest = source.trim_start_matches('\u{feff}');
    while !rest.is_empty() {
        let Some(at) = rest.find('<') else {
            push_text(&mut stack, rest);
            break;
        };
        push_text(&mut stack, &rest[..at]);
        rest = &rest[at..];
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |end| &after[end + 3..]);
        } else if let Some(after) = rest.strip_prefix("<![CDATA[") {
            let end = after.find("]]>").unwrap_or(after.len());
            if let Some(top) = stack.last_mut() {
                top.children.push(Content::Text(after[..end].to_string()));
            }
            rest = after.get(end + 3..).unwrap_or("");
        } else if rest.starts_with("<?") {
            rest = rest.find("?>").map_or("", |end| &rest[end + 2..]);
        } else if rest.starts_with("<!") {
            rest = skip_declaration(rest);
        } else if let Some(after) = rest.strip_prefix("</") {
            let end = after.find('>')?;
            if stack.len() > 1 {
                let element = stack.pop()?;
                stack.last_mut()?.children.push(Content::Element(element));
            }
            rest = &after[end + 1..];
        } else {
            let end = tag_end(rest)?;
            let inner = &rest[1..end];
            match inner.strip_suffix('/') {
                Some(inner) => {
                    let element = start_tag(inner);
                    stack.last_mut()?.children.push(Content::Element(element));
                }
                None => stack.push(start_tag(inner)),
            }
            rest = &rest[end + 1..];
        }
    }
    while stack.len() > 1 {
        let element = stack.pop()?;
        stack.last_mut()?.children.push(Content::Element(element));
    }
    stack.pop()?.children.into_iter().find_map(|c| match c {
        Content::Element(e) => Some(e),
        Content::Text(_) => None,
    })
}
