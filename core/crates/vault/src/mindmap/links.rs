//! Topic links, as XMind puts a hyperlink on a topic: a web page, or
//! another note (as Obsidian links notes). A note keeps the link as
//! Markdown around the topic's text, `[Topic](https://…)` or
//! `[[Note|Topic]]`, so any editor follows it; the map shows the text alone.

use super::labels::{split_labels, with_labels};
use super::markers::{split_markers, with_markers};

/// A link in a topic's text: where it lies, its text and where it goes.
struct Found<'a> {
    end: usize,
    label: String,
    target: &'a str,
}

/// The link starting at `at` (on a `[`), if one does: `[[Note]]`,
/// `[[Note|text]]`, `[text](target)` or `[text](<target>)`. Embeds and
/// images (`![[…]]`, `![…](…)`) are not links.
fn link_at(text: &str, at: usize) -> Option<Found<'_>> {
    if text[..at].ends_with('!') {
        return None;
    }
    let rest = &text[at..];
    if let Some(inner) = rest.strip_prefix("[[") {
        let close = inner.find("]]")?;
        let body = &inner[..close];
        let (target, label) = match body.split_once('|') {
            Some((target, label)) => (target.trim(), label.trim()),
            None => (body.trim(), body.trim()),
        };
        if target.is_empty() {
            return None;
        }
        return Some(Found { end: at + 2 + close + 2, label: label.to_string(), target });
    }
    let inner = rest.strip_prefix('[')?;
    let mut label = String::new();
    let mut chars = inner.char_indices();
    let close = loop {
        let (i, c) = chars.next()?;
        match c {
            '\\' => label.push(chars.next()?.1),
            ']' => break i,
            '[' => return None,
            _ => label.push(c),
        }
    };
    // `[text](` is followed by the target and `)`.
    let dest_at = 1 + close + 2;
    let dest = inner[close + 1..].strip_prefix('(')?;
    let (target, used) = if let Some(angled) = dest.strip_prefix('<') {
        let end = angled.find('>')?;
        angled[end + 1..].starts_with(')').then_some((&angled[..end], end + 3))?
    } else {
        // Brackets in it pair up, as in `…/wiki/Mercury_(planet)`.
        let mut depth = 0;
        let mut end = None;
        for (i, c) in dest.char_indices() {
            match c {
                '(' => depth += 1,
                ')' if depth == 0 => {
                    end = Some(i);
                    break;
                }
                ')' => depth -= 1,
                c if c.is_whitespace() => return None,
                _ => {}
            }
        }
        let end = end?;
        (&dest[..end], end + 1)
    };
    if target.is_empty() {
        return None;
    }
    Some(Found { end: at + dest_at + used, label, target })
}

/// A topic's text as the map shows it, every link as its text, and where
/// its (first) link goes; empty when it has none.
pub fn split_link(text: &str) -> (String, String) {
    let mut shown = String::new();
    let mut link = String::new();
    let (mut at, mut copied) = (0, 0);
    while let Some(offset) = text[at..].find('[') {
        at += offset;
        match link_at(text, at) {
            Some(found) => {
                shown.push_str(&text[copied..at]);
                shown.push_str(&found.label);
                if link.is_empty() {
                    link = found.target.to_string();
                }
                at = found.end;
                copied = at;
            }
            None => at += 1,
        }
    }
    shown.push_str(&text[copied..]);
    (shown, link)
}

/// Whether a link goes to a page or app (`https:`, `mailto:`, `zotero:`…)
/// rather than a note: it starts with a scheme of two letters or more (one
/// would be a drive).
pub fn is_web_link(link: &str) -> bool {
    link.split_once(':').is_some_and(|(scheme, _)| {
        scheme.len() > 1
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// A topic's text linked to `link` (a web address or a note's name), or
/// with its link taken off when `link` is empty. The link takes the place
/// of any in the text; markers stay before it and labels after it.
pub fn with_link(text: &str, link: &str) -> String {
    let (shown, _) = split_link(text);
    let (markers, label) = split_markers(&shown);
    let (label, labels) = split_labels(label);
    let link = link.trim();
    let label = if label.trim().is_empty() { link } else { label.trim() };
    let wiki_safe = |s: &str| !s.contains(['[', ']', '|', '\n']);
    let linked = if link.is_empty() {
        label.to_string()
    } else if !is_web_link(link) && wiki_safe(link) && wiki_safe(label) {
        if label == link { format!("[[{link}]]") } else { format!("[[{link}|{label}]]") }
    } else {
        let mut escaped = String::new();
        for c in label.chars() {
            if matches!(c, '[' | ']' | '\\') {
                escaped.push('\\');
            }
            escaped.push(c);
        }
        if link.contains(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>')) {
            format!("[{escaped}](<{}>)", link.replace('>', "%3E"))
        } else {
            format!("[{escaped}]({link})")
        }
    };
    let marked = with_markers(&markers, &linked);
    if labels.is_empty() { marked } else { with_labels(&marked, &labels) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_links_as_their_text() {
        assert_eq!(split_link("Plain"), ("Plain".into(), String::new()));
        assert_eq!(split_link("[[Methods]]"), ("Methods".into(), "Methods".into()));
        assert_eq!(split_link("⭐ [[Methods|How we did it]]"), ("⭐ How we did it".into(), "Methods".into()));
        assert_eq!(
            split_link("[Mercury](https://en.wikipedia.org/wiki/Mercury_(planet)) facts"),
            ("Mercury facts".into(), "https://en.wikipedia.org/wiki/Mercury_(planet)".into())
        );
        assert_eq!(split_link(r"[a \] b](<My Note>)"), ("a ] b".into(), "My Note".into()));
        assert_eq!(split_link("See [[A]] and [[B]]"), ("See A and B".into(), "A".into()), "the first link");
        assert_eq!(
            split_link("![[picture.png]] [x] [y] (z)"),
            ("![[picture.png]] [x] [y] (z)".into(), String::new())
        );
    }

    #[test]
    fn tells_web_links_from_notes() {
        assert!(
            is_web_link("https://example.com")
                && is_web_link("mailto:a@b.org")
                && is_web_link("zotero://select")
        );
        assert!(
            !is_web_link("Methods")
                && !is_web_link(r"C:\notes\a.md")
                && !is_web_link("")
                && !is_web_link("1x:y")
        );
    }

    #[test]
    fn puts_links_on_and_takes_them_off() {
        assert_eq!(with_link("Topic", "https://example.com"), "[Topic](https://example.com)");
        assert_eq!(with_link("⭐ Topic", "Methods"), "⭐ [[Methods|Topic]]", "markers stay before it");
        assert_eq!(with_link("Methods", "Methods"), "[[Methods]]");
        assert_eq!(with_link("[[Old|Topic]]", "New"), "[[New|Topic]]");
        assert_eq!(with_link("⭐ Topic #draft", "Note"), "⭐ [[Note|Topic]] #draft", "labels stay after it");
        assert_eq!(with_link("⭐ [[Old|Topic]]", ""), "⭐ Topic");
        assert_eq!(with_link("a [b]", "Note"), r"[a \[b\]](Note)", "brackets in the text");
        assert_eq!(with_link("Page", "https://x.org/a b"), "[Page](<https://x.org/a b>)");
        assert_eq!(with_link("", "https://x.org"), "[https://x.org](https://x.org)");
        for text in [
            "[Topic](https://example.com)",
            "⭐ [[Methods|Topic]]",
            r"[a \[b\]](Note)",
            "[Page](<https://x.org/a b>)",
        ] {
            let (shown, link) = split_link(text);
            assert_eq!(with_link(&shown, &link), text, "reads back what it writes");
        }
    }
}
