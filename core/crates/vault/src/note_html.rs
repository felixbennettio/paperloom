//! The raw HTML notes may use. Sanitize before inserting the renderer's own
//! links and embeds, which deliberately have a different set of attributes.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser};

use crate::render::escape;

pub(crate) const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_FOOTNOTES)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS)
    .union(Options::ENABLE_MATH)
    .union(Options::ENABLE_GFM);

/// Only raw HTML events are scanned: escaped HTML, autolinks, inline code,
/// fenced/indented code and front matter keep their Markdown meaning.
pub(crate) fn tag_ranges(text: &str) -> Vec<Range<usize>> {
    ranges(text).0
}

/// HTML syntax and Markdown code spans, including code that crosses lines.
/// These byte ranges are blanked while indexing a note.
pub(crate) fn hidden_ranges(text: &str) -> Vec<Range<usize>> {
    let (mut tags, code) = ranges(text);
    tags.extend(code);
    tags.sort_by_key(|range| range.start);
    tags
}

fn ranges(text: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let mut blocks: Vec<Range<usize>> = Vec::new();
    let mut code = Vec::new();
    for (event, range) in Parser::new_ext(text, OPTIONS).into_offset_iter() {
        if matches!(event, Event::Html(_) | Event::InlineHtml(_)) {
            if let Some(last) = blocks.last_mut()
                && last.end == range.start
            {
                last.end = range.end;
            } else {
                blocks.push(range);
            }
        } else if matches!(event, Event::Code(_)) {
            code.push(range);
        }
    }
    let mut tags = Vec::new();
    for block in blocks {
        let mut from = block.start;
        while let Some(open) = text[from..block.end].find('<').map(|i| from + i) {
            let mut quote = None;
            let mut end = block.end;
            for (i, c) in text[open + 1..block.end].char_indices() {
                match (quote, c) {
                    (Some(q), c) if q == c => quote = None,
                    (None, '\'' | '"') => quote = Some(c),
                    (None, '>') => {
                        end = open + 1 + i + 1;
                        break;
                    }
                    _ => {}
                }
            }
            tags.push(open..end);
            from = end;
        }
    }
    (tags, code)
}

struct SafeTag {
    name: String,
    close: bool,
    html: String,
}

pub(crate) struct HtmlEdit {
    pub range: Range<usize>,
    pub html: String,
    pub literal: bool,
}

/// Unknown, malformed and unmatched tags become literal text. A mismatched
/// closing tag invalidates the open stack rather than asking the browser to
/// repair it. Balanced children can still render inside an unmatched parent.
pub(crate) fn edits(text: &str, ranges: &[Range<usize>]) -> Vec<HtmlEdit> {
    let tags: Vec<_> = ranges.iter().map(|r| safe_tag(&text[r.clone()])).collect();
    let mut balanced = vec![false; tags.len()];
    let mut stack: Vec<usize> = Vec::new();
    for (i, tag) in tags.iter().enumerate() {
        let Some(tag) = tag else { continue };
        if tag.name == "br" {
            balanced[i] = true;
        } else if !tag.close {
            stack.push(i);
        } else if let Some(open) = stack.last().copied()
            && tags[open].as_ref().is_some_and(|t| t.name == tag.name)
        {
            balanced[open] = true;
            balanced[i] = true;
            stack.pop();
        } else {
            stack.clear();
        }
    }
    ranges
        .iter()
        .zip(tags)
        .zip(balanced)
        .map(|((range, tag), balanced)| {
            let html = match tag {
                Some(tag) if balanced => tag.html,
                _ => escape(&text[range.clone()]),
            };
            HtmlEdit { range: range.clone(), html, literal: !balanced }
        })
        .collect()
}

fn allowed_tag(name: &str) -> bool {
    matches!(
        name,
        "mark"
            | "span"
            | "sub"
            | "sup"
            | "kbd"
            | "u"
            | "s"
            | "del"
            | "ins"
            | "small"
            | "abbr"
            | "br"
            | "b"
            | "i"
            | "em"
            | "strong"
            | "code"
            | "details"
            | "summary"
            | "div"
            | "p"
    )
}

fn safe_tag(raw: &str) -> Option<SafeTag> {
    let mut rest = raw.strip_prefix('<')?.strip_suffix('>')?;
    let close = rest.starts_with('/');
    if close {
        rest = &rest[1..];
    }
    let length = rest.bytes().take_while(u8::is_ascii_alphanumeric).count();
    let name = rest[..length].to_ascii_lowercase();
    if !allowed_tag(&name) {
        return None;
    }
    rest = &rest[length..];
    if close {
        return (name != "br" && rest.trim().is_empty()).then(|| SafeTag {
            html: format!("</{name}>"),
            name,
            close,
        });
    }
    let mut html = format!("<{name}");
    let mut seen = Vec::new();
    loop {
        let separated = rest.starts_with(char::is_whitespace);
        rest = rest.trim_start();
        if rest.is_empty() || (rest == "/" && name == "br") {
            break;
        }
        if !separated {
            return None;
        }
        let n =
            rest.bytes().take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':')).count();
        if n == 0 {
            return None;
        }
        let attr = rest[..n].to_ascii_lowercase();
        rest = &rest[n..];
        let after_name = rest.trim_start();
        if !after_name.starts_with('=') {
            // Boolean attributes are not in the allowlist.
            continue;
        }
        rest = after_name[1..].trim_start();
        let value = if let Some(quote) = rest.chars().next().filter(|c| matches!(c, '\'' | '"')) {
            let end = rest[1..].find(quote)? + 1;
            let value = &rest[1..end];
            rest = &rest[end + 1..];
            value
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let value = &rest[..end];
            if value.is_empty() || value.contains(['"', '\'', '`', '=', '<', '>']) {
                return None;
            }
            rest = &rest[end..];
            value
        };
        if seen.contains(&attr) {
            continue;
        }
        seen.push(attr.clone());
        let value = match attr.as_str() {
            "title" => Some(value.to_string()),
            "class" => safe_class(value),
            "style" => safe_style(value),
            _ => None,
        };
        if let Some(value) = value {
            html.push_str(&format!(" {attr}=\"{}\"", escape(&value)));
        }
    }
    html.push('>');
    Some(SafeTag { name, close, html })
}

fn safe_class(value: &str) -> Option<String> {
    let names: Vec<_> = value.split_ascii_whitespace().collect();
    (!names.is_empty()
        && names.iter().all(|name| {
            name.starts_with(|c: char| c.is_ascii_alphabetic() || matches!(c, '_' | '-'))
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        }))
    .then(|| names.join(" "))
}

fn safe_style(value: &str) -> Option<String> {
    let mut declarations = Vec::new();
    for declaration in value.split(';') {
        let Some((property, value)) = declaration.split_once(':') else { continue };
        let property = property.trim().to_ascii_lowercase();
        let value = value.trim();
        let valid = match property.as_str() {
            "background" | "background-color" | "color" => safe_color(value),
            "text-decoration" => {
                let lower = value.to_ascii_lowercase();
                let words: Vec<_> = lower.split_ascii_whitespace().collect();
                !words.is_empty()
                    && words.iter().all(|word| {
                        matches!(
                            *word,
                            "none"
                                | "underline"
                                | "overline"
                                | "line-through"
                                | "solid"
                                | "double"
                                | "dotted"
                                | "dashed"
                                | "wavy"
                        )
                    })
                    && (!words.contains(&"none") || words.len() == 1)
            }
            _ => false,
        };
        if valid {
            // Keep the compact form used by existing rendered notes.
            declarations.push(format!("{property}:{value}"));
        }
    }
    (!declarations.is_empty()).then(|| declarations.join(";"))
}

fn safe_color(value: &str) -> bool {
    if let Some(hex) = value.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    let lower = value.to_ascii_lowercase();
    if let Some((function, args)) = lower.split_once('(') {
        let count = match function {
            "rgb" => 3,
            "rgba" => 4,
            _ => return false,
        };
        let Some(args) = args.strip_suffix(')') else { return false };
        let numbers: Vec<_> = args.split(',').map(str::trim).collect();
        return numbers.len() == count
            && numbers.iter().enumerate().all(|(i, n)| css_number(n, if i == 3 { 1.0 } else { 255.0 }));
    }
    NAMED_COLORS.split_ascii_whitespace().any(|name| name == lower)
}

fn css_number(value: &str, max: f64) -> bool {
    let mut dot = false;
    let mut digit = false;
    for b in value.bytes() {
        if b.is_ascii_digit() {
            digit = true;
        } else if b == b'.' && !dot {
            dot = true;
        } else {
            return false;
        }
    }
    digit && !value.ends_with('.') && value.parse::<f64>().is_ok_and(|n| n <= max)
}

// CSS named colours, plus the transparent keyword. No system colours,
// variables, escapes, entities, comments, URLs or general CSS functions.
const NAMED_COLORS: &str = "aliceblue antiquewhite aqua aquamarine azure beige bisque black \
    blanchedalmond blue blueviolet brown burlywood cadetblue chartreuse chocolate coral \
    cornflowerblue cornsilk crimson cyan darkblue darkcyan darkgoldenrod darkgray darkgreen \
    darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid darkred darksalmon \
    darkseagreen darkslateblue darkslategray darkslategrey darkturquoise darkviolet deeppink \
    deepskyblue dimgray dimgrey dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro \
    ghostwhite gold goldenrod gray green greenyellow grey honeydew hotpink indianred indigo \
    ivory khaki lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan \
    lightgoldenrodyellow lightgray lightgreen lightgrey lightpink lightsalmon lightseagreen \
    lightskyblue lightslategray lightslategrey lightsteelblue lightyellow lime limegreen linen \
    magenta maroon mediumaquamarine mediumblue mediumorchid mediumpurple mediumseagreen \
    mediumslateblue mediumspringgreen mediumturquoise mediumvioletred midnightblue mintcream \
    mistyrose moccasin navajowhite navy oldlace olive olivedrab orange orangered orchid \
    palegoldenrod palegreen paleturquoise palevioletred papayawhip peachpuff peru pink plum \
    powderblue purple rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown \
    seagreen seashell sienna silver skyblue slateblue slategray slategrey snow springgreen \
    steelblue tan teal thistle tomato transparent turquoise violet wheat white whitesmoke \
    yellow yellowgreen";
