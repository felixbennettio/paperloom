//! Messages in the reader's language. Core and the apps' Rust code write
//! their messages (errors, mostly) in English; [`localize_message`] turns
//! one into another language with the translations in `messages.json`,
//! which scripts/i18n.py writes from the "core" section of
//! i18n/strings.json.
//!
//! A translation's key is the English text with `{name}` where a value goes.
//! A message made with `format!` is matched against those keys and its
//! values are carried over, translated in turn when they are messages too
//! (the reason after a colon, a step such as "Saving").

use std::collections::HashMap;
use std::sync::OnceLock;

/// How many levels of messages inside messages are translated.
const DEPTH: usize = 3;
/// Longer messages (a server's whole reply, say) stay as they are: the
/// matching would take long, and there is little to gain.
const LONGEST: usize = 1_000;
/// Placeholders whose values are themselves messages or steps written by
/// the code, and so translated too. Others (names, titles, paths, ids)
/// are the person's own words and stay as they are.
const MESSAGE_NAMES: [&str; 7] = ["what", "e", "err", "error", "detail", "reason", "message"];

/// `message`, written in English by Core or an app's Rust code, in
/// `language` (`zh-Hans`, `zh-Hant`, `fr`, `ja` or `ru`; other languages
/// and messages without a translation stay as they are).
#[uniffi::export]
pub fn localize_message(message: String, language: String) -> String {
    localize(&message, &language)
}

/// [`localize_message`] for Rust callers.
pub fn localize(message: &str, language: &str) -> String {
    let Some(language) = language_code(language).and_then(|code| catalog().get(code)) else {
        return message.to_string();
    };
    if message.len() > LONGEST {
        return message.to_string();
    }
    language.translate(message, DEPTH).unwrap_or_else(|| message.to_string())
}

/// The catalog's name for a language tag such as `zh-Hans`, `zh-CN` or
/// `fr-CA`; None for English and languages without translations.
fn language_code(tag: &str) -> Option<&'static str> {
    let lower = tag.to_ascii_lowercase().replace('_', "-");
    if lower.starts_with("zh") {
        let traditional = ["hant", "-tw", "-hk", "-mo"].iter().any(|mark| lower.contains(mark));
        return Some(if traditional { "zh-Hant" } else { "zh-Hans" });
    }
    match lower.split(['-', '_']).next() {
        Some("fr") => Some("fr"),
        Some("ja") => Some("ja"),
        Some("ru") => Some("ru"),
        _ => None,
    }
}

fn catalog() -> &'static HashMap<String, Language> {
    static CATALOG: OnceLock<HashMap<String, Language>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let raw: HashMap<String, HashMap<String, String>> =
            serde_json::from_str(include_str!("messages.json")).unwrap_or_default();
        raw.into_iter().map(|(code, entries)| (code, Language::new(entries))).collect()
    })
}

/// Text with placeholders: `literals` and `names` take turns, starting
/// and ending with a literal (possibly empty).
#[derive(Debug, PartialEq)]
struct Pattern {
    literals: Vec<String>,
    names: Vec<String>,
}

/// `"Could not read {path}: {reason}"` as literals `["Could not read ",
/// ": ", ""]` and names `["path", "reason"]`. A brace that does not
/// enclose a name is text.
fn pattern(text: &str) -> Pattern {
    let mut literals = vec![String::new()];
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let name = after.find('}').map(|close| &after[..close]);
        match name {
            Some(name) if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') => {
                push(&mut literals, &rest[..open]);
                names.push(name.to_string());
                literals.push(String::new());
                rest = &after[name.len() + 1..];
            }
            _ => {
                push(&mut literals, &rest[..=open]);
                rest = after;
            }
        }
    }
    push(&mut literals, rest);
    Pattern { literals, names }
}

fn push(literals: &mut [String], text: &str) {
    if let Some(last) = literals.last_mut() {
        last.push_str(text);
    }
}

struct Template {
    key: Pattern,
    translation: Pattern,
    /// Letters in the key's literal text: the most specific key is tried first.
    weight: usize,
}

#[derive(Default)]
struct Language {
    exact: HashMap<String, String>,
    templates: Vec<Template>,
}

impl Language {
    fn new(entries: HashMap<String, String>) -> Self {
        let mut language = Language::default();
        for (key, translation) in entries {
            let key_pattern = pattern(&key);
            if key_pattern.names.is_empty() {
                language.exact.insert(key, translation);
                continue;
            }
            let weight =
                key_pattern.literals.iter().flat_map(|l| l.chars()).filter(|c| c.is_alphabetic()).count();
            language.templates.push(Template {
                key: key_pattern,
                translation: pattern(&translation),
                weight,
            });
        }
        language
            .templates
            .sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.key.literals.cmp(&b.key.literals)));
        language
    }

    fn translate(&self, message: &str, depth: usize) -> Option<String> {
        if let Some(text) = self.exact.get(message) {
            return Some(text.clone());
        }
        self.templates.iter().find_map(|template| {
            let found = captures(&template.key.literals, message)?;
            let values: HashMap<&str, String> = template
                .key
                .names
                .iter()
                .zip(found)
                .map(|(name, value)| {
                    let translated = (depth > 0 && holds_message(name, value))
                        .then(|| self.translate(value, depth - 1))
                        .flatten();
                    (name.as_str(), translated.unwrap_or_else(|| value.to_string()))
                })
                .collect();
            Some(fill(&template.translation, &values))
        })
    }
}

/// Whether a placeholder's value is a message of the code's own, to be
/// translated too: named as one, or a positional value that reads as a
/// sentence (a reason passed on as `{0}`). A name or title in quotes, or a
/// single word, is the person's.
fn holds_message(name: &str, value: &str) -> bool {
    MESSAGE_NAMES.contains(&name)
        || (name.chars().all(|c| c.is_ascii_digit()) && value.contains(' ') && !value.starts_with('“'))
}

/// `translation` with each name replaced by its value; a name the key did
/// not have stays as written.
fn fill(translation: &Pattern, values: &HashMap<&str, String>) -> String {
    let mut text = translation.literals[0].clone();
    for (name, literal) in translation.names.iter().zip(&translation.literals[1..]) {
        match values.get(name.as_str()) {
            Some(value) => text.push_str(value),
            None => {
                text.push('{');
                text.push_str(name);
                text.push('}');
            }
        }
        text.push_str(literal);
    }
    text
}

/// The values in `message` between `literals`, if it has that shape. An
/// earlier value takes as little as it can. A value may be empty only
/// between two literals (`“”` for an empty name), where nothing else could
/// be meant.
fn captures<'a>(literals: &[String], message: &'a str) -> Option<Vec<&'a str>> {
    let (first, rest) = literals.split_first()?;
    let message = message.strip_prefix(first.as_str())?;
    let mut values = Vec::with_capacity(rest.len());
    find(!first.is_empty(), rest, message, &mut values).then_some(values)
}

/// `after_text`: the literal before the next value is not empty.
fn find<'a>(after_text: bool, literals: &[String], message: &'a str, values: &mut Vec<&'a str>) -> bool {
    let Some((literal, later)) = literals.split_first() else {
        return message.is_empty();
    };
    let may_be_empty = after_text && !literal.is_empty();
    if later.is_empty() {
        return match message.strip_suffix(literal.as_str()) {
            Some(value) if may_be_empty || !value.is_empty() => {
                values.push(value);
                true
            }
            _ => false,
        };
    }
    for (index, _) in message.match_indices(literal.as_str()) {
        if index == 0 && !may_be_empty {
            continue;
        }
        values.push(&message[..index]);
        if find(!literal.is_empty(), later, &message[index + literal.len()..], values) {
            return true;
        }
        values.pop();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn language(entries: &[(&str, &str)]) -> Language {
        Language::new(entries.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
    }

    #[test]
    fn exact_messages_and_templates() {
        let zh = language(&[
            ("A tag needs a name.", "标签需要名称。"),
            ("Unknown item type “{0}”.", "未知的文献类型“{0}”。"),
        ]);
        assert_eq!(zh.translate("A tag needs a name.", DEPTH).as_deref(), Some("标签需要名称。"));
        assert_eq!(
            zh.translate("Unknown item type “film”.", DEPTH).as_deref(),
            Some("未知的文献类型“film”。")
        );
        assert_eq!(zh.translate("Something else.", DEPTH), None);
    }

    #[test]
    fn values_that_are_messages_are_translated_too() {
        let zh = language(&[
            ("Translators: {error}", "转换器：{error}"),
            ("This library is kept only in memory.", "这个文库只保存在内存中。"),
            ("{what} “{0}” failed: {e}", "{what}“{0}”失败：{e}"),
            ("Saving", "保存"),
        ]);
        assert_eq!(
            zh.translate("Translators: This library is kept only in memory.", DEPTH).as_deref(),
            Some("转换器：这个文库只保存在内存中。")
        );
        assert_eq!(
            zh.translate("Saving “/tmp/a b.zip” failed: disk full", DEPTH).as_deref(),
            Some("保存“/tmp/a b.zip”失败：disk full")
        );
    }

    #[test]
    fn the_most_specific_template_wins() {
        let fr = language(&[
            ("{0} not found", "{0} introuvable"),
            ("The page {0} not found", "La page {0} est introuvable"),
        ]);
        assert_eq!(fr.translate("The page 3 not found", DEPTH).as_deref(), Some("La page 3 est introuvable"));
        assert_eq!(fr.translate("item 7 not found", DEPTH).as_deref(), Some("item 7 introuvable"));
    }

    #[test]
    fn values_are_not_empty_and_literals_must_match() {
        assert_eq!(captures(&["A ".into(), " and ".into(), ".".into()], "A b and c."), Some(vec!["b", "c"]));
        // Empty only between two literals.
        assert_eq!(captures(&["A ".into(), ".".into()], "A ."), Some(vec![""]));
        assert_eq!(captures(&["".into(), " x".into()], " x"), None);
        assert_eq!(captures(&["A ".into(), ".".into()], "B c."), None);
        // An earlier value takes as little as it can.
        assert_eq!(captures(&["".into(), ": ".into(), "!".into()], "a: b: c!"), Some(vec!["a", "b: c"]));
    }

    #[test]
    fn names_and_titles_stay_as_written() {
        let zh = language(&[
            ("Catalogue {name}: {detail}", "目录 {name}：{detail}"),
            ("not a web address", "不是网址"),
            ("Reading", "读取"),
            ("“{0}” is not a number.", "“{0}”不是数字。"),
            ("{0} not found", "找不到{0}"),
            ("item {id}", "文献 {id}"),
        ]);
        assert_eq!(
            zh.translate("Catalogue Reading: not a web address", DEPTH).as_deref(),
            Some("目录 Reading：不是网址")
        );
        assert_eq!(zh.translate("“” is not a number.", DEPTH).as_deref(), Some("“”不是数字。"));
        assert_eq!(zh.translate("“Reading” is not a number.", DEPTH).as_deref(), Some("“Reading”不是数字。"));
        assert_eq!(zh.translate("item 42 not found", DEPTH).as_deref(), Some("找不到文献 42"));
    }

    #[test]
    fn long_messages_stay_as_they_are() {
        let long = format!("Saving “{}” failed", "x “".repeat(100_000));
        assert_eq!(localize(&long, "zh-Hans"), long);
    }

    #[test]
    fn patterns_keep_stray_braces_as_text() {
        let parsed = pattern("{a} {not a name} {b}");
        assert_eq!(parsed.names, ["a", "b"]);
        assert_eq!(parsed.literals, ["", " {not a name} ", ""]);
    }

    #[test]
    fn language_tags() {
        assert_eq!(language_code("zh-Hans"), Some("zh-Hans"));
        assert_eq!(language_code("zh-CN"), Some("zh-Hans"));
        assert_eq!(language_code("zh-Hant-TW"), Some("zh-Hant"));
        assert_eq!(language_code("zh_TW"), Some("zh-Hant"));
        assert_eq!(language_code("fr_CA"), Some("fr"));
        assert_eq!(language_code("en"), None);
        assert_eq!(language_code("de"), None);
    }

    #[test]
    fn the_catalog_has_core_messages() {
        assert_eq!(localize("A tag needs a name.", "en"), "A tag needs a name.");
        for code in ["zh-Hans", "zh-Hant", "fr", "ja", "ru"] {
            assert!(
                catalog().get(code).is_some_and(|l| !l.exact.is_empty() && !l.templates.is_empty()),
                "{code}"
            );
            assert_ne!(localize("A tag needs a name.", code), "A tag needs a name.", "{code}");
        }
    }
}
