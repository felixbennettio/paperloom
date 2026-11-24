//! Reading the structure of a note: properties (YAML front matter),
//! headings, `[[links]]` and `![[embeds]]`, `#tags` and `^block` IDs, the
//! way Obsidian writes them, and Pandoc citations (`[@key, p. 33]`). Code
//! blocks, inline code and `%%comments%%` are skipped.

/// A `[[link]]` or `![[embed]]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The note named, as written: `Paper`, `folder/Paper`, `Paper.md`.
    /// Empty for a link within the same note (`[[#Heading]]`).
    pub target: String,
    /// `Heading` in `[[Paper#Heading]]`, `^id` in `[[Paper#^id]]`.
    pub subpath: Option<String>,
    /// `text` in `[[Paper|text]]`.
    pub alias: Option<String>,
    pub embed: bool,
    /// From 0.
    pub line: usize,
    /// Byte range of the whole `[[…]]` (with `!`) in the note.
    pub start: usize,
    pub end: usize,
}

/// A Pandoc citation: `[@key]`, `[@key, p. 33]`, `[see @a; -@b]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    pub works: Vec<CitedWork>,
    /// From 0.
    pub line: usize,
    /// Byte range of the whole `[…]` in the note.
    pub start: usize,
    pub end: usize,
}

/// One work a citation names: its key (a citation key, a DOI or an item
/// ID) and the page, if given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitedWork {
    pub key: String,
    pub page: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Parsed {
    /// Front matter, key by key, values as written (lists joined by `, `).
    pub properties: Vec<(String, String)>,
    pub aliases: Vec<String>,
    /// From the front matter and the text, without `#`, in order of first
    /// appearance.
    pub tags: Vec<String>,
    pub headings: Vec<Heading>,
    pub links: Vec<Link>,
    pub block_ids: Vec<String>,
    /// Byte ranges of `#tags` in the text (with the `#`), for rendering.
    pub tag_spans: Vec<(usize, usize)>,
    /// Byte ranges of `%%comments%%` (with the `%%`), hidden when rendering.
    pub comment_spans: Vec<(usize, usize)>,
    pub citations: Vec<Citation>,
    /// Where the body starts, after the front matter.
    pub body_start: usize,
}

pub fn parse(text: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let body_start = front_matter(text, &mut parsed);
    parsed.body_start = body_start;
    let mut comment_from: Option<usize> = None;
    let mut fence: Option<(char, usize)> = None;
    let mut in_comment = false;
    let hidden = crate::note_html::hidden_ranges(text);
    let mut hidden = hidden.iter().peekable();
    let mut offset = 0;
    for (number, raw) in text.split_inclusive('\n').enumerate() {
        let line_start = offset;
        offset += raw.len();
        if line_start < body_start {
            continue;
        }
        let line = raw.trim_end_matches(['\n', '\r']);
        let trimmed = line.trim_start();
        // Fenced code: ``` or ~~~, closed by the same character at least as long.
        if let Some(marker) = fence_marker(trimmed) {
            match fence {
                None => fence = Some(marker),
                Some((c, n))
                    if marker.0 == c && marker.1 >= n && trimmed.trim_end().chars().all(|x| x == c) =>
                {
                    fence = None
                }
                _ => {}
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        // Blank HTML syntax and inline code before looking for comments,
        // links or tags. Quoted `>` and multiline syntax stay hidden.
        let mut visible = line.to_string();
        while let Some(range) = hidden.peek() {
            if range.end <= line_start {
                hidden.next();
            } else if range.start >= line_start + line.len() {
                break;
            } else {
                let start = range.start.saturating_sub(line_start);
                let end = (range.end - line_start).min(line.len());
                visible.replace_range(start..end, &" ".repeat(end - start));
                if range.end > line_start + line.len() {
                    break;
                }
                hidden.next();
            }
        }
        let was_in_comment = in_comment;
        comment_spans(&visible, line_start, was_in_comment, &mut comment_from, &mut parsed.comment_spans);
        let visible = strip_comments(&visible, &mut in_comment);
        if let Some(heading) = heading(&visible, number) {
            parsed.headings.push(heading);
        }
        for link in links(line, &visible, number, line_start) {
            parsed.links.push(link);
        }
        for (works, start, end) in citations(&visible) {
            parsed.citations.push(Citation {
                works,
                line: number,
                start: line_start + start,
                end: line_start + end,
            });
        }
        for (tag, start, end) in tags(&visible) {
            push_unique(&mut parsed.tags, tag);
            parsed.tag_spans.push((line_start + start, line_start + end));
        }
        if let Some(id) = block_id(&visible) {
            parsed.block_ids.push(id);
        }
    }
    parsed
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.iter().any(|v| v.eq_ignore_ascii_case(&value)) {
        list.push(value);
    }
}

fn fence_marker(line: &str) -> Option<(char, usize)> {
    let c = line.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = line.chars().take_while(|x| *x == c).count();
    (n >= 3).then_some((c, n))
}

/// The line with `%%comments%%` blanked out (same length,
/// so byte positions still match the original).
fn strip_comments(line: &str, in_comment: &mut bool) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < bytes.len() {
        if line[i..].starts_with("%%") {
            *in_comment = !*in_comment;
            out.push_str("  ");
            i += 2;
            continue;
        }
        if *in_comment {
            let c = line[i..].chars().next().unwrap_or(' ');
            out.extend(std::iter::repeat_n(' ', c.len_utf8()));
            i += c.len_utf8();
            continue;
        }
        let c = line[i..].chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn heading(line: &str, number: usize) -> Option<Heading> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let level = trimmed.bytes().take_while(|b| *b == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &trimmed[level..];
    if !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim_end().to_string();
    Some(Heading { level: level as u8, text, line: number })
}

fn links(original: &str, visible: &str, number: usize, line_start: usize) -> Vec<Link> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(pos) = visible[from..].find("[[") {
        let open = from + pos;
        let Some(close) = visible[open + 2..].find("]]") else { break };
        let inner = &original[open + 2..open + 2 + close];
        let end = open + 2 + close + 2;
        let embed = open > 0 && visible.as_bytes()[open - 1] == b'!';
        let start = if embed { open - 1 } else { open };
        if !inner.contains('\n') && !inner.trim().is_empty() {
            let (target_part, alias) = match split_unescaped_pipe(inner) {
                Some((t, a)) => (t, Some(a.trim().to_string()).filter(|a| !a.is_empty())),
                None => (inner, None),
            };
            let (target, subpath) = match target_part.split_once('#') {
                Some((t, s)) => (t.trim().to_string(), Some(s.trim().to_string()).filter(|s| !s.is_empty())),
                None => (target_part.trim().to_string(), None),
            };
            found.push(Link {
                target,
                subpath,
                alias,
                embed,
                line: number,
                start: line_start + start,
                end: line_start + end,
            });
        }
        from = end;
    }
    found
}

/// `a|b` or `a\|b` (inside tables) → (`a`, `b`).
fn split_unescaped_pipe(inner: &str) -> Option<(&str, &str)> {
    let pos = inner.find('|')?;
    let target = inner[..pos].strip_suffix('\\').unwrap_or(&inner[..pos]);
    Some((target, &inner[pos + 1..]))
}

/// Finds Pandoc citations in plain text, without Markdown's code, comment
/// or front-matter rules. Byte ranges are relative to `text`.
pub fn plain_citations(text: &str) -> Vec<Citation> {
    let mut line = 0;
    let mut at = 0;
    citations(text)
        .into_iter()
        .map(|(works, start, end)| {
            line += text[at..start].bytes().filter(|&b| b == b'\n').count();
            at = start;
            Citation { works, line, start, end }
        })
        .collect()
}

/// Pandoc citations in a line (code and comments already blanked): the
/// works and the byte range of each `[…]`. Links (`[text](…)`, `[[…]]`),
/// footnotes and e-mail addresses are not citations.
fn citations(line: &str) -> Vec<(Vec<CitedWork>, usize, usize)> {
    let bytes = line.as_bytes();
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(pos) = line[from..].find('[') {
        let open = from + pos;
        from = open + 1;
        let before = open.checked_sub(1).map(|i| bytes[i]);
        if matches!(before, Some(b'[' | b'!')) || matches!(bytes.get(open + 1), Some(b'[' | b'^')) {
            continue;
        }
        let Some(close) = line[open + 1..].find([']', '[']).map(|c| open + 1 + c) else { break };
        if bytes[close] != b']' || matches!(bytes.get(close + 1), Some(b'(' | b'[')) {
            continue;
        }
        if let Some(works) = cited_works(&line[open + 1..close]) {
            found.push((works, open, close + 1));
            from = close + 1;
        }
    }
    found
}

/// `see @a, p. 3; -@b` → the works, or none unless every part names one.
fn cited_works(inner: &str) -> Option<Vec<CitedWork>> {
    let mut works = Vec::new();
    for part in inner.split(';') {
        let at = part.char_indices().find(|&(i, c)| {
            c == '@' && part[..i].chars().next_back().is_none_or(|p| p.is_whitespace() || p == '-')
        })?;
        let rest = &part[at.0 + 1..];
        let (key, after) = if let Some(braced) = rest.strip_prefix('{') {
            let end = braced.find('}')?;
            (braced[..end].trim().to_string(), &braced[end + 1..])
        } else {
            let length = rest
                .char_indices()
                .find(|&(_, c)| !(c.is_alphanumeric() || "_:.#$%&-+?<>~/".contains(c)))
                .map_or(rest.len(), |(i, _)| i);
            let key = rest[..length].trim_end_matches(|c: char| ".:/?#$%&-+<>~".contains(c));
            (key.to_string(), &rest[key.len()..])
        };
        if key.is_empty() {
            return None;
        }
        works.push(CitedWork { key, page: page_of(after) });
    }
    (!works.is_empty()).then_some(works)
}

/// `, p. 33`, `, pp. 33–35`, `, 12`: the page; other locators are not pages.
fn page_of(after: &str) -> Option<String> {
    let text = after.trim().trim_start_matches(',').trim();
    let text = ["pages", "page", "pp.", "p."]
        .iter()
        .find_map(|label| text.strip_prefix(label))
        .unwrap_or(text)
        .trim();
    let page = text.split_whitespace().next()?;
    page.chars().next().filter(char::is_ascii_digit)?;
    Some(page.trim_end_matches([',', '.']).to_string())
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '/')
}

/// Tags in a line with their byte ranges (including the `#`).
fn tags(line: &str) -> Vec<(String, usize, usize)> {
    let mut found = Vec::new();
    let mut previous = ' ';
    for (i, c) in line.char_indices() {
        if c == '#' && (previous.is_whitespace() || previous == '(') {
            let name: String = line[i + 1..].chars().take_while(|c| is_tag_char(*c)).collect();
            let name = name.trim_end_matches('/');
            if !name.is_empty() && !name.chars().all(|c| c.is_ascii_digit()) {
                found.push((name.to_string(), i, i + 1 + name.len()));
            }
        }
        previous = c;
    }
    found
}

/// Records `%%…%%` ranges, which may span lines.
fn comment_spans(
    line: &str,
    line_start: usize,
    mut inside: bool,
    from: &mut Option<usize>,
    spans: &mut Vec<(usize, usize)>,
) {
    let mut search = 0;
    while let Some(pos) = line[search..].find("%%") {
        let at = line_start + search + pos;
        if inside {
            if let Some(start) = from.take() {
                spans.push((start, at + 2));
            }
        } else {
            *from = Some(at);
        }
        inside = !inside;
        search += pos + 2;
    }
}

/// `^id` at the end of a line.
fn block_id(line: &str) -> Option<String> {
    let last = line.trim_end().rsplit(char::is_whitespace).next()?;
    let id = last.strip_prefix('^')?;
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then(|| id.to_string())
}

/// Reads YAML front matter; returns where the body starts.
fn front_matter(text: &str, parsed: &mut Parsed) -> usize {
    let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else { return 0 };
    let opening = text.len() - rest.len();
    let mut offset = opening;
    let mut key: Option<String> = None;
    for raw in rest.split_inclusive('\n') {
        offset += raw.len();
        let line = raw.trim_end_matches(['\n', '\r']);
        if line == "---" || line == "..." {
            return offset;
        }
        if let Some(item) = line.trim_start().strip_prefix("- ").filter(|_| line.starts_with([' ', '-'])) {
            if let Some(k) = &key {
                add_property(parsed, k, unquote(item));
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':').filter(|_| !line.starts_with([' ', '\t'])) {
            let k = k.trim().to_string();
            let v = v.trim();
            if let Some(list) = v.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
                for item in list.split(',').map(unquote).filter(|i| !i.is_empty()) {
                    add_property(parsed, &k, item);
                }
            } else if !v.is_empty() {
                add_property(parsed, &k, unquote(v));
            } else {
                parsed.properties.push((k.clone(), String::new()));
            }
            key = Some(k);
        }
    }
    // No closing line: not front matter after all.
    parsed.properties.clear();
    parsed.aliases.clear();
    parsed.tags.clear();
    0
}

fn unquote(value: &str) -> String {
    let v = value.trim();
    if let Some(inner) = v.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        // `\"` and `\\` inside double quotes.
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            out.push(if c == '\\' { chars.next().unwrap_or('\\') } else { c });
        }
        return out;
    }
    v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')).unwrap_or(v).to_string()
}

fn add_property(parsed: &mut Parsed, key: &str, value: String) {
    match key {
        "tags" | "tag" => {
            // One list item is one tag; a plain `tags: a, b` line lists several.
            for tag in value.split(',').map(|t| t.trim().trim_start_matches('#')).filter(|t| !t.is_empty()) {
                push_unique(&mut parsed.tags, tag.to_string());
            }
        }
        "aliases" | "alias" => push_unique(&mut parsed.aliases, value.clone()),
        _ => {}
    }
    match parsed.properties.iter_mut().find(|(k, _)| k == key) {
        Some((_, existing)) if !existing.is_empty() => {
            existing.push_str(", ");
            existing.push_str(&value);
        }
        Some((_, existing)) => *existing = value,
        None => parsed.properties.push((key.to_string(), value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_pandoc_citations() {
        let note = "See [@kucsko2013, p. 54] and [see @a; -@{10.1038/x y}, pp. 3–5].\n\
                    Not [a link](x), [[wiki]], [^1], [mail a@b.c] or `[@code]`.\n\
                    [@doi:10.1/x.]";
        let p = parse(note);
        assert_eq!(p.citations.len(), 3);
        let first = &p.citations[0];
        assert_eq!(first.works, [CitedWork { key: "kucsko2013".into(), page: Some("54".into()) }]);
        assert_eq!(&note[first.start..first.end], "[@kucsko2013, p. 54]");
        let second = &p.citations[1].works;
        assert_eq!(second[0], CitedWork { key: "a".into(), page: None });
        assert_eq!(second[1], CitedWork { key: "10.1038/x y".into(), page: Some("3–5".into()) });
        assert_eq!((p.citations[2].line, p.citations[2].works[0].key.as_str()), (2, "doi:10.1/x"));
    }

    const NOTE: &str = "---
title: Reading notes
tags: [ml, \"to read\"]
aliases:
  - RN
  - 读书笔记
---
# Reading notes ^top

See [[Attention Is All You Need|the transformer paper]] and [[folder/Other#Method]].
![[figure.png]] and a table cell [[Paper\\|alias]].
Tagged #deep-learning and #中文/子标签 but not #123 or a#b.

```python
# not a heading [[not a link]] #nottag
```

Inline `[[code]]` and %%hidden [[comment]]%% and [[#Local heading]].
## Method ##
Last line ^block-1
";

    #[test]
    fn reads_notes() {
        let p = parse(NOTE);
        assert_eq!(p.aliases, ["RN", "读书笔记"]);
        assert_eq!(p.tags, ["ml", "to read", "deep-learning", "中文/子标签"]);
        assert_eq!(p.properties[0], ("title".to_string(), "Reading notes".to_string()));
        assert_eq!(
            p.headings.iter().map(|h| (h.level, h.text.as_str())).collect::<Vec<_>>(),
            [(1, "Reading notes ^top"), (2, "Method")]
        );
        let targets: Vec<(&str, Option<&str>, Option<&str>, bool)> = p
            .links
            .iter()
            .map(|l| (l.target.as_str(), l.subpath.as_deref(), l.alias.as_deref(), l.embed))
            .collect();
        assert_eq!(
            targets,
            [
                ("Attention Is All You Need", None, Some("the transformer paper"), false),
                ("folder/Other", Some("Method"), None, false),
                ("figure.png", None, None, true),
                ("Paper", None, Some("alias"), false),
                ("", Some("Local heading"), None, false),
            ]
        );
        let first = &p.links[0];
        assert_eq!(&NOTE[first.start..first.end], "[[Attention Is All You Need|the transformer paper]]");
        let embed = &p.links[2];
        assert_eq!(&NOTE[embed.start..embed.end], "![[figure.png]]");
        assert_eq!(p.block_ids, ["top", "block-1"]);
        let spans: Vec<&str> = p.tag_spans.iter().map(|(a, b)| &NOTE[*a..*b]).collect();
        assert_eq!(spans, ["#deep-learning", "#中文/子标签"]);
        let comments: Vec<&str> = p.comment_spans.iter().map(|(a, b)| &NOTE[*a..*b]).collect();
        assert_eq!(comments, ["%%hidden [[comment]]%%"]);
        assert!(NOTE[p.body_start..].starts_with("# Reading notes"));
    }

    #[test]
    fn needs_a_closing_front_matter_line() {
        let p = parse("---\ntags: x\nno end\n# Title");
        assert!(p.tags.is_empty() && p.properties.is_empty());
        assert_eq!(p.headings.len(), 1);
    }

    #[test]
    fn skips_tags_in_html_attributes_and_code() {
        let note = "The <mark style=\"background: #FFF3A3A6;\">quick</mark> #real\n\
                    <widget title='a > #hidden' data-value=#unknown>#visible</widget>\n\
                    <span\n title=\"a >\n #multiline [[Not a link]] [@notacite] %%notacomment%%\">#inside</span>\n\
                    `#code` and ``code ` #more`` #after\n\
                    `multiline\n#multilinecode\n` #after-multiline\n\
                    ```html\n<span title=' #fenced'> #stillcode\n```\n\
                    ~~~~\n#tildecode\n~~~~\n\
                    <span title=' #最后'>中文</span> #中文";
        let parsed = parse(note);
        assert_eq!(parsed.tags, ["real", "visible", "inside", "after", "after-multiline", "中文"]);
        let spans: Vec<_> = parsed.tag_spans.iter().map(|&(a, b)| &note[a..b]).collect();
        assert_eq!(spans, ["#real", "#visible", "#inside", "#after", "#after-multiline", "#中文"]);
        assert!(parsed.links.is_empty());
        assert!(parsed.citations.is_empty());
        assert!(parsed.comment_spans.is_empty());
    }
}
