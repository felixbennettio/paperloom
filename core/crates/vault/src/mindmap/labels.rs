//! Topic labels, as XMind puts labels under a topic. A note keeps them as
//! tags at the end of the topic's text (`Topic #draft #review`), so the
//! vault's tag search finds them; the map shows them under the text.

/// Whether a word is a tag: `#` and a name of letters, digits, `_`, `-`
/// and `/`, not all digits (as Obsidian reads tags).
fn is_tag(word: &str) -> bool {
    word.strip_prefix('#').is_some_and(|name| {
        !name.is_empty()
            && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/'))
            && !name.chars().all(|c| c.is_ascii_digit())
    })
}

/// A topic's text without the tags at its end, and their names. A topic
/// that is only tags keeps the first as its text.
pub fn split_labels(text: &str) -> (&str, Vec<String>) {
    let mut rest = text.trim_end();
    let mut labels = Vec::new();
    while let Some(at) = rest.rfind(char::is_whitespace) {
        let word = rest[at..].trim_start();
        let before = rest[..at].trim_end();
        if before.is_empty() || !is_tag(word) {
            break;
        }
        labels.push(word[1..].to_string());
        rest = before;
    }
    labels.reverse();
    (rest, labels)
}

/// A label as a tag's name: spaces as `-`, and what a tag cannot hold
/// left out.
pub fn label_name(label: &str) -> String {
    let name: String = label
        .trim()
        .trim_start_matches('#')
        .chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .filter(|&c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/'))
        .collect();
    if name.chars().all(|c| c.is_ascii_digit()) { String::new() } else { name }
}

/// A topic's text with these labels at its end in place of its own; the
/// same label once.
pub fn with_labels(text: &str, labels: &[String]) -> String {
    let (rest, _) = split_labels(text);
    let mut out = rest.to_string();
    let mut seen: Vec<String> = Vec::new();
    for name in labels.iter().map(|l| label_name(l.as_str())) {
        if name.is_empty() || seen.contains(&name) {
            continue;
        }
        out.push_str(" #");
        out.push_str(&name);
        seen.push(name);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tags_at_the_end_as_labels() {
        assert_eq!(split_labels("Topic"), ("Topic", vec![]));
        assert_eq!(
            split_labels("Topic #draft  #to-do/later "),
            ("Topic", vec!["draft".into(), "to-do/later".into()])
        );
        assert_eq!(
            split_labels("Use #ml here #ml"),
            ("Use #ml here", vec!["ml".into()]),
            "tags inside are text"
        );
        assert_eq!(split_labels("#todo"), ("#todo", vec![]), "a topic that is only a tag");
        assert_eq!(split_labels("#a #b"), ("#a", vec!["b".into()]));
        assert_eq!(split_labels("Year #2024"), ("Year #2024", vec![]), "numbers are not tags");
        assert_eq!(split_labels("[[Note|Topic #a]]"), ("[[Note|Topic #a]]", vec![]));
        assert_eq!(split_labels("主题\u{3000}#草稿"), ("主题", vec!["草稿".into()]));
    }

    #[test]
    fn writes_labels_as_tags() {
        assert_eq!(
            with_labels("Topic #old", &["New one".into(), "#x".into(), "x".into(), "2024".into()]),
            "Topic #New-one #x"
        );
        assert_eq!(with_labels("Topic #old", &[]), "Topic");
        assert_eq!(label_name(" a, b! "), "a-b");
    }
}
