//! Writing a note's properties back as YAML front matter, the way Obsidian
//! writes them, keeping the rest of the note as it is.

use crate::parse::parse;

/// Keys Obsidian always writes as lists.
const LIST_KEYS: [&str; 3] = ["tags", "aliases", "cssclasses"];

/// `text` with its front matter made from `properties` (keys and values,
/// a list's values joined by `, `). Keys the note wrote as lists stay
/// lists; no properties leaves no front matter.
pub fn with_properties(text: &str, properties: &[(String, String)]) -> String {
    let body = &text[parse(text).body_start..];
    let lists = list_keys(text);
    let mut out = String::new();
    for (key, value) in properties {
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value.trim();
        if LIST_KEYS.contains(&key) || lists.iter().any(|k| k == key) {
            out.push_str(&format!("{key}:\n"));
            let tags = key == "tags";
            for item in value.split(',').map(str::trim).filter(|v| !v.is_empty()) {
                let item = if tags { item.trim_start_matches('#') } else { item };
                out.push_str(&format!("  - {}\n", scalar(item)));
            }
        } else if value.is_empty() {
            out.push_str(&format!("{key}:\n"));
        } else {
            out.push_str(&format!("{key}: {}\n", scalar(value)));
        }
    }
    if out.is_empty() { body.to_string() } else { format!("---\n{out}---\n{body}") }
}

/// Keys the front matter writes as lists (`[a, b]` or `- item` lines).
fn list_keys(text: &str) -> Vec<String> {
    let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else {
        return Vec::new();
    };
    let mut keys: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for line in rest.lines() {
        if line == "---" || line == "..." {
            break;
        }
        if line.trim_start().starts_with("- ") && line.starts_with([' ', '-']) {
            if let Some(key) = current.take()
                && !keys.contains(&key)
            {
                keys.push(key);
            }
            continue;
        }
        if let Some((key, value)) = line.split_once(':').filter(|_| !line.starts_with([' ', '\t'])) {
            let key = key.trim().to_string();
            if value.trim().starts_with('[') && !keys.contains(&key) {
                keys.push(key.clone());
            }
            current = Some(key);
        }
    }
    keys
}

/// A value as YAML: quoted when it would read as something else.
fn scalar(value: &str) -> String {
    let special = value.starts_with([
        '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`',
    ]) || value.contains(": ")
        || value.contains(" #")
        || value.ends_with(':');
    if special {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn writes_front_matter_and_keeps_the_body() {
        let note = "---\ntitle: Old\nauthors: [Ada, Alan]\ntags:\n  - ml\n---\n# Body\n\nText.\n";
        let props = pairs(&[
            ("title", "New: a study"),
            ("authors", "Ada, Grace"),
            ("tags", "#ml, to read"),
            ("done", ""),
        ]);
        let written = with_properties(note, &props);
        assert_eq!(
            written,
            "---\ntitle: \"New: a study\"\nauthors:\n  - Ada\n  - Grace\ntags:\n  - ml\n  - to read\ndone:\n---\n# Body\n\nText.\n"
        );
        let parsed = parse(&written);
        assert_eq!(parsed.properties[0], ("title".to_string(), "New: a study".to_string()));
        assert_eq!(parsed.tags, ["ml", "to read"]);
        assert!(written.ends_with("# Body\n\nText.\n"));
    }

    #[test]
    fn adds_and_removes_front_matter() {
        assert_eq!(
            with_properties("Just text.\n", &pairs(&[("year", "2024")])),
            "---\nyear: 2024\n---\nJust text.\n"
        );
        assert_eq!(with_properties("---\nyear: 2024\n---\nJust text.\n", &[]), "Just text.\n");
        assert_eq!(with_properties("No front matter.", &[]), "No front matter.");
        assert_eq!(scalar("[[Linked note]]"), "\"[[Linked note]]\"");
        assert_eq!(scalar("say \"hi\": now"), "\"say \\\"hi\\\": now\"");
    }
}
