//! The tags and attributes needed for a page's embedded metadata.

/// Character references used in titles and metadata attributes.
pub(crate) fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        let end = rest.bytes().take(16).position(|b| b == b';');
        let decoded = end.and_then(|end| match &rest[..end] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            entity => {
                let number = entity.strip_prefix('#')?;
                let value = match number.strip_prefix('x').or_else(|| number.strip_prefix('X')) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                    None => number.parse().ok()?,
                };
                char::from_u32(value)
            }
        });
        if let (Some(end), Some(ch)) = (end, decoded) {
            out.push(ch);
            rest = &rest[end + 1..];
        } else {
            out.push('&');
        }
    }
    out.push_str(rest);
    out
}

/// An attribute of a tag: any case, quoted or unquoted.
pub(crate) fn attribute(tag: &str, name: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut at = 0;
    while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    while at < bytes.len() {
        while at < bytes.len() && (bytes[at].is_ascii_whitespace() || bytes[at] == b'/') {
            at += 1;
        }
        let start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() && !b"=/>".contains(&bytes[at]) {
            at += 1;
        }
        if at == start {
            break;
        }
        let key = &tag[start..at];
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if bytes.get(at) != Some(&b'=') {
            continue;
        }
        at += 1;
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let quote = bytes.get(at).copied()?;
        let quoted = matches!(quote, b'\'' | b'"');
        at += usize::from(quoted);
        let start = at;
        while at < bytes.len()
            && if quoted { bytes[at] != quote } else { !bytes[at].is_ascii_whitespace() && bytes[at] != b'>' }
        {
            at += 1;
        }
        if key.eq_ignore_ascii_case(name) {
            return Some(decode(&tag[start..at]).trim().to_string());
        }
        at += usize::from(quoted && at < bytes.len());
    }
    None
}

pub(crate) struct Tag<'a> {
    pub(crate) name: &'a str,
    pub(crate) source: &'a str,
    /// Raw text inside a script, style or title, without the closing tag.
    pub(crate) text: Option<&'a str>,
}

pub(crate) struct Tags<'a> {
    rest: &'a str,
}

pub(crate) fn tags(html: &str) -> Tags<'_> {
    Tags { rest: html }
}

/// The end of a tag, allowing `>` inside quotes.
fn tag_end(tag: &str) -> Option<usize> {
    let mut quote = None;
    for (at, byte) in tag.bytes().enumerate() {
        match (quote, byte) {
            (Some(q), b) if q == b => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return Some(at + 1),
            _ => {}
        }
    }
    None
}

/// Where a raw text element closes; apparent tags inside it are text.
fn raw_end(text: &str, name: &str) -> usize {
    let mut from = 0;
    while let Some(at) = text[from..].find("</").map(|at| from + at) {
        let tail = &text[at + 2..];
        if tail.get(..name.len()).is_some_and(|n| n.eq_ignore_ascii_case(name))
            && tail.as_bytes().get(name.len()).is_some_and(|b| b.is_ascii_whitespace() || *b == b'>')
        {
            return at;
        }
        from = at + 2;
    }
    text.len()
}

impl<'a> Iterator for Tags<'a> {
    type Item = Tag<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let at = self.rest.find('<')?;
            self.rest = &self.rest[at..];
            if self.rest.starts_with("<!--") {
                let end = self.rest.find("-->").map_or(self.rest.len(), |at| at + 3);
                self.rest = &self.rest[end..];
                continue;
            }
            if !self.rest.as_bytes().get(1).is_some_and(u8::is_ascii_alphabetic) {
                self.rest = &self.rest[1..];
                continue;
            }
            let end = tag_end(self.rest)?;
            let source = &self.rest[..end];
            self.rest = &self.rest[end..];
            let name_end = source[1..]
                .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
                .map_or(source.len(), |at| at + 1);
            let name = &source[1..name_end];
            let text = if ["script", "style", "title", "textarea", "xmp", "iframe", "noembed", "noframes"]
                .iter()
                .any(|n| name.eq_ignore_ascii_case(n))
            {
                let end = raw_end(self.rest, name);
                let text = &self.rest[..end];
                self.rest = &self.rest[end..];
                Some(text)
            } else {
                None
            };
            return Some(Tag { name, source, text });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_entities_once() {
        assert_eq!(decode("&amp; &lt; &gt; &quot; &#39; &#65; &#x4E2D; &#X1F4DA;"), "& < > \" ' A 中 📚");
        assert_eq!(
            decode("&amp;lt; &unknown; &#xD800; &#99999999; &"),
            "&lt; &unknown; &#xD800; &#99999999; &"
        );
    }

    #[test]
    fn reads_attributes_without_matching_values_or_longer_names() {
        let tag =
            r#"<META data-name='wrong' data-note=' name=wrong' disabled NAME = 'A &gt; B' content=plain>"#;
        assert_eq!(attribute(tag, "name").as_deref(), Some("A > B"));
        assert_eq!(attribute(tag, "content").as_deref(), Some("plain"));
        assert_eq!(attribute(tag, "missing"), None);
    }

    #[test]
    fn skips_comments_and_raw_text_and_keeps_quoted_angles() {
        let html = r#"<!-- <meta name=wrong> --><script>"<meta name=wrong>"</SCRIPT><meta content='a > b'><metadata>"#;
        let found: Vec<_> = tags(html).collect();
        assert_eq!(found.iter().map(|t| t.name).collect::<Vec<_>>(), ["script", "meta", "metadata"]);
        assert_eq!(found[0].text, Some(r#""<meta name=wrong>""#));
        assert_eq!(attribute(found[1].source, "content").as_deref(), Some("a > b"));
    }
}
