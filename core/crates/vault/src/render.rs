//! Markdown to HTML for the reading view, with Obsidian's `[[links]]`,
//! `![[embeds]]`, `#tags` and `%%comments%%`. Both apps show this HTML, so
//! a note reads the same everywhere. Raw HTML (colored text, `<mark>`,
//! `<u>`) is restricted to the safe subset in `note_html` for both apps.
//!
//! Links use URLs the apps catch: `paperloom:note/<path>#<heading id>`,
//! `paperloom:new/<name>` for a note that does not exist yet and
//! `paperloom:tag/<name>`. Images load from `paperloom-vault:///<path>`.
//! The same values are in `data-*` attributes. A `query` code block shows
//! what it finds.

use pulldown_cmark::{CodeBlockKind, CowStr, Event, Parser, Tag, TagEnd, html};

use crate::{External, Hooks, Link, Vault, note_html, parse, parse_query, render_rows, select};

/// Marks where an embedded note goes until the HTML is written (private-use
/// characters never appear in notes in practice).
const EMBED: char = '\u{E000}';
/// Escaped raw tags are restored after Markdown, so punctuation in their
/// attributes cannot introduce code spans or other Markdown syntax.
const LITERAL: char = '\u{E001}';

pub(crate) fn render(vault: &Vault, path: &str, text: &str, depth: usize, hooks: &Hooks) -> String {
    let parsed = parse(text);
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut embeds: Vec<String> = Vec::new();
    for link in &parsed.links {
        let html = match (hooks.link)(link) {
            Some(External { html, block: true }) => {
                embeds.push(html);
                format!("{EMBED}{}{EMBED}", embeds.len() - 1)
            }
            Some(External { html, block: false }) => html,
            None => link_html(vault, path, link, depth, &mut embeds, hooks),
        };
        edits.push((link.start, link.end, html));
    }
    for &(start, end) in &parsed.tag_spans {
        let name = &text[start + 1..end];
        let (tag, url) = (escape(name), encode(name));
        edits.push((
            start,
            end,
            format!(r#"<a class="tag" href="paperloom:tag/{url}" data-tag="{tag}">#{tag}</a>"#),
        ));
    }
    for &(start, end) in &parsed.comment_spans {
        edits.push((start, end, String::new()));
    }
    for citation in &parsed.citations {
        if let Some(html) = (hooks.cite)(citation) {
            edits.push((citation.start, citation.end, html));
        }
    }
    // HTML hidden by a comment or replaced by a hook/link cannot balance a
    // tag that will actually reach the reading view.
    let raw_tags: Vec<_> = note_html::tag_ranges(text)
        .into_iter()
        .filter(|range| !edits.iter().any(|(start, end, _)| *start < range.end && range.start < *end))
        .collect();
    let mut literals = Vec::new();
    for edit in note_html::edits(text, &raw_tags) {
        let replacement = if edit.literal {
            literals.push(edit.html);
            format!("{LITERAL}{}{LITERAL}", literals.len() - 1)
        } else {
            edit.html
        };
        edits.push((edit.range.start, edit.range.end, replacement));
    }
    edits.sort_by_key(|(start, end, _)| (*start, std::cmp::Reverse(*end)));
    let mut source = String::with_capacity(text.len());
    let mut at = 0;
    for (start, end, html) in edits {
        if start < at {
            continue; // inside an earlier edit (a tag within a link)
        }
        source.push_str(&text[at..start]);
        source.push_str(&html);
        at = end;
    }
    source.push_str(&text[at..]);

    let mut events: Vec<Event> = Parser::new_ext(&source, note_html::OPTIONS).collect();
    add_heading_ids(&mut events);
    let events = run_queries(vault, events, hooks);
    let mut out = String::with_capacity(source.len() * 3 / 2);
    html::push_html(&mut out, events.into_iter());
    for (n, embed) in embeds.iter().enumerate() {
        let token = format!("{EMBED}{n}{EMBED}");
        // A block on its own line replaces its paragraph, not just the token.
        out = out.replace(&format!("<p>{token}</p>"), embed).replace(&token, embed);
    }
    for (n, literal) in literals.iter().enumerate() {
        out = out.replace(&format!("{LITERAL}{n}{LITERAL}"), literal);
    }
    out
}

fn link_html(
    vault: &Vault,
    from: &str,
    link: &Link,
    depth: usize,
    embeds: &mut Vec<String>,
    hooks: &Hooks,
) -> String {
    let label = escape(&match (&link.alias, link.target.is_empty(), &link.subpath) {
        (Some(alias), _, _) => alias.clone(),
        (None, true, Some(sub)) => sub.clone(),
        (None, false, Some(sub)) => format!("{} › {}", link.target, sub),
        (None, _, None) => link.target.clone(),
    });
    let subpath =
        link.subpath.as_deref().map(|s| format!(r#" data-subpath="{}""#, escape(s))).unwrap_or_default();
    let fragment = link
        .subpath
        .as_deref()
        .map(|s| format!("#{}", s.strip_prefix('^').map_or_else(|| heading_id(s), str::to_string)))
        .unwrap_or_default();
    let Some(resolved) = vault.resolve(&link.target, from) else {
        let (target, url) = (escape(&link.target), encode(&link.target));
        return format!(
            r#"<a class="wikilink unresolved" href="paperloom:new/{url}" data-target="{target}"{subpath}>{label}</a>"#
        );
    };
    let (path, url) = (escape(&resolved), encode(&resolved));
    if link.embed {
        // A recording plays where it is: the apps put their player there.
        if is_audio(&resolved) {
            return format!(
                r#"<span class="embed-audio" data-audio="{path}"><a class="audio-link" href="paperloom:audio/{url}">{label}</a></span>"#
            );
        }
        if is_image(&resolved) {
            let alt = escape(link.alias.as_deref().unwrap_or(&link.target));
            let image = format!(
                r#"<img class="embed-image" src="paperloom-vault:///{url}" data-src="{path}" alt="{alt}">"#
            );
            // A diagram's picture opens the diagram.
            return if crate::is_diagram(&resolved) {
                format!(
                    r#"<a class="embed-diagram" href="paperloom:diagram/{url}" data-diagram="{path}">{image}</a>"#
                )
            } else {
                image
            };
        }
        // A drawing shows as its picture, which opens it.
        if resolved.to_lowercase().ends_with(".excalidraw")
            && let Ok(drawing) = vault.read_drawing(&resolved)
        {
            embeds.push(format!(
                r#"<a class="embed-drawing" href="paperloom:drawing/{url}" data-drawing="{path}">{}</a>"#,
                crate::drawing_svg(&drawing)
            ));
            return format!("{EMBED}{}{EMBED}", embeds.len() - 1);
        }
        if let Some(note) = vault.note(&resolved).filter(|_| depth == 0 && resolved != from) {
            let inner = render(vault, &resolved, &note.text, depth + 1, &Hooks { cite: &|_| None, ..*hooks });
            embeds.push(format!(
                r#"<div class="embed" data-note="{path}"><a class="wikilink embed-title" href="paperloom:note/{url}" data-note="{path}">{label}</a>{inner}</div>"#
            ));
            return format!("{EMBED}{}{EMBED}", embeds.len() - 1);
        }
    }
    format!(
        r#"<a class="wikilink" href="paperloom:note/{url}{fragment}" data-note="{path}"{subpath}>{label}</a>"#
    )
}

fn is_audio(path: &str) -> bool {
    let lower = path.to_lowercase();
    [".m4a", ".mp3", ".wav", ".webm", ".ogg", ".oga", ".flac", ".aac"].iter().any(|e| lower.ends_with(e))
}

fn is_image(path: &str) -> bool {
    let lower = path.to_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".bmp", ".avif"].iter().any(|e| lower.ends_with(e))
}

/// Gives headings an `id` from their text, so `[[Note#Heading]]` can scroll
/// to them.
/// Puts what each `query` code block finds in its place, or what is wrong
/// with it.
fn run_queries<'a>(vault: &Vault, events: Vec<Event<'a>>, hooks: &Hooks) -> Vec<Event<'a>> {
    let mut out = Vec::with_capacity(events.len());
    let mut body: Option<String> = None;
    for event in events {
        if let Some(text) = body.as_mut() {
            match event {
                Event::Text(t) => text.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    out.push(Event::Html(CowStr::from(query_html(vault, text, hooks))));
                    body = None;
                }
                _ => {}
            }
            continue;
        }
        if let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) = &event
            && info.split_whitespace().next() == Some("query")
        {
            body = Some(String::new());
            continue;
        }
        out.push(event);
    }
    out
}

fn query_html(vault: &Vault, body: &str, hooks: &Hooks) -> String {
    let query = match parse_query(body) {
        Ok(query) => query,
        Err(message) => return format!(r#"<p class="query-error">{}</p>"#, escape(&message)),
    };
    let rows = match (query.library, hooks.library) {
        (false, _) => vault.note_rows(&query),
        (true, Some(library)) => select(&query, library(&query)),
        (true, None) => {
            return r#"<p class="query-error">This vault is not connected to a library.</p>"#.into();
        }
    };
    render_rows(&query, &rows)
}

fn add_heading_ids(events: &mut [Event]) {
    for i in 0..events.len() {
        if !matches!(events[i], Event::Start(Tag::Heading { .. })) {
            continue;
        }
        let mut text = String::new();
        for event in &events[i + 1..] {
            match event {
                Event::End(TagEnd::Heading(_)) => break,
                Event::Text(t) | Event::Code(t) => text.push_str(t),
                _ => {}
            }
        }
        if let Event::Start(Tag::Heading { id, .. }) = &mut events[i]
            && id.is_none()
        {
            *id = Some(CowStr::from(heading_id(&text)));
        }
    }
}

/// `Method & Results` → `method-results`; letters in any script stay.
pub fn heading_id(text: &str) -> String {
    let mut id = String::new();
    for c in text.trim().chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            id.push(c);
        } else if (c.is_whitespace() || c == '-' || c == '_') && !id.ends_with('-') && !id.is_empty() {
            id.push('-');
        }
    }
    id.trim_end_matches('-').to_string()
}

/// Percent-encodes everything but letters, digits, `-._~` and `/`.
pub(crate) fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}
