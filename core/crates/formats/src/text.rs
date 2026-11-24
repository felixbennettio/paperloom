//! Helpers the formats share.

use std::collections::HashMap;

use paperloom_model::{Creator, Item, contains_cjk};

/// The first four-digit year in a date-ish string (`2013/07/31/`, `July 2013`).
pub(crate) fn year_in(text: &str) -> Option<i32> {
    text.split(|c: char| !c.is_ascii_digit()).find(|p| p.len() == 4).and_then(|y| y.parse().ok())
}

/// `2013/07/31`, `2013-7-31` or `2013` as `2013-07-31` / `2013`; `None`
/// for anything else.
pub(crate) fn iso_date(text: &str) -> Option<String> {
    let parts: Vec<&str> = text.split(['/', '-', '.']).map(str::trim).filter(|p| !p.is_empty()).collect();
    let numbers: Vec<u32> = parts.iter().map_while(|p| p.parse().ok()).collect();
    match numbers.as_slice() {
        [y] if *y >= 1000 => Some(format!("{y:04}")),
        [y, m] if *y >= 1000 && (1..=12).contains(m) => Some(format!("{y:04}-{m:02}")),
        [y, m, d, ..] if *y >= 1000 && (1..=12).contains(m) && (1..=31).contains(d) => {
            Some(format!("{y:04}-{m:02}-{d:02}"))
        }
        _ => None,
    }
}

/// A month by English name or abbreviation (`Aug`, `AUGUST`), 1–12.
pub(crate) fn month_number(word: &str) -> Option<u32> {
    let lower = word.trim_end_matches('.').to_lowercase();
    let months = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    (lower.len() >= 3)
        .then(|| months.iter().position(|m| lower.starts_with(m)).map(|i| i as u32 + 1))
        .flatten()
}

/// A date written with words, as databases do: `2013 Aug 1`, `AUG 1 2013`,
/// `2013 Jul-Aug`, `Spring 2013`. The year, and the date as `2013-08-01`,
/// `2013-08` or `2013`.
pub(crate) fn date_from_words(text: &str) -> (Option<i32>, Option<String>) {
    let words: Vec<&str> =
        text.split(|c: char| c.is_whitespace() || c == '-' || c == ',').filter(|w| !w.is_empty()).collect();
    let year = words
        .iter()
        .find(|w| w.len() == 4 && w.chars().all(|c| c.is_ascii_digit()))
        .and_then(|y| y.parse::<i32>().ok());
    let Some(year) = year else { return (None, None) };
    let month = words.iter().find_map(|w| month_number(w));
    let day = words
        .iter()
        .filter(|w| w.len() <= 2)
        .find_map(|w| w.parse::<u32>().ok().filter(|d| (1..=31).contains(d)));
    let date = match (month, day) {
        (Some(m), Some(d)) => format!("{year:04}-{m:02}-{d:02}"),
        (Some(m), None) => format!("{year:04}-{m:02}"),
        _ => format!("{year:04}"),
    };
    (Some(year), Some(date))
}

/// A language code from a name or a MEDLINE/ISO 639-2 code: `English`,
/// `eng` → `en`, `Chinese`, `chi` → `zh`. Unknown values are kept.
pub(crate) fn language_code(value: &str) -> String {
    let lower = value.trim().to_lowercase();
    let code = match lower.as_str() {
        "english" | "eng" | "en" => "en",
        "chinese" | "chi" | "zho" | "zh" | "中文" => "zh",
        "japanese" | "jpn" | "ja" => "ja",
        "french" | "fre" | "fra" | "fr" => "fr",
        "russian" | "rus" | "ru" => "ru",
        "german" | "ger" | "deu" | "de" => "de",
        "spanish" | "spa" | "es" => "es",
        "korean" | "kor" | "ko" => "ko",
        "italian" | "ita" | "it" => "it",
        "portuguese" | "por" | "pt" => "pt",
        _ => return value.trim().to_string(),
    };
    code.to_string()
}

/// `NATURE COMMUNICATIONS` → `Nature Communications`: databases that shout
/// source titles get them in title case; anything with lower case stays.
pub(crate) fn title_case_if_shouting(text: &str) -> String {
    if text.chars().any(|c| c.is_lowercase()) || !text.chars().any(|c| c.is_alphabetic()) {
        return text.to_string();
    }
    let small = ["of", "and", "in", "the", "for", "on", "at", "to", "a", "an", "de", "der", "und"];
    text.split(' ')
        .enumerate()
        .map(|(i, word)| {
            let lower = word.to_lowercase();
            if i > 0 && small.contains(&lower.as_str()) {
                lower
            } else {
                let mut chars = lower.chars();
                chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// Splits a field holding several names or keywords (`A; B；C`).
pub(crate) fn split_list(text: &str) -> Vec<String> {
    text.split([';', '；', '\n']).map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

/// Joins page bounds: `54` and `58` into `54-58`.
pub(crate) fn pages(start: Option<&str>, end: Option<&str>) -> Option<String> {
    match (start.filter(|s| !s.is_empty()), end.filter(|s| !s.is_empty())) {
        (Some(s), Some(e)) if s != e => Some(format!("{s}-{e}")),
        (Some(s), _) => Some(s.to_string()),
        (None, Some(e)) => Some(e.to_string()),
        (None, None) => None,
    }
}

/// A name as `Family, Given` for formats that want it that way.
pub(crate) fn inverted(creator: &Creator) -> String {
    if !creator.literal.is_empty() {
        creator.literal.clone()
    } else if creator.given.is_empty() {
        creator.family.clone()
    } else {
        format!("{}, {}", creator.family, creator.given)
    }
}

/// ASCII letters and digits of a string, lower-cased, with common Latin
/// accents folded (`Müller` → `muller`).
fn ascii_slug(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            'a'..='z' | '0'..='9' => Some(c),
            'A'..='Z' => Some(c.to_ascii_lowercase()),
            'à'..='å' | 'À'..='Å' => Some('a'),
            'ç' | 'Ç' => Some('c'),
            'è'..='ë' | 'È'..='Ë' => Some('e'),
            'ì'..='ï' | 'Ì'..='Ï' => Some('i'),
            'ñ' | 'Ñ' => Some('n'),
            'ò'..='ö' | 'ø' | 'Ò'..='Ö' | 'Ø' => Some('o'),
            'ù'..='ü' | 'Ù'..='Ü' => Some('u'),
            'ý' | 'ÿ' | 'Ý' => Some('y'),
            'ß' => Some('s'),
            _ => None,
        })
        .collect()
}

const STOP_WORDS: &[&str] =
    &["a", "an", "the", "on", "of", "in", "for", "and", "to", "with", "from", "by", "at"];

/// Citation keys for items: `family` + `year` + first significant title
/// word (`kucsko2013nanometre`), made unique with `a`, `b`, … suffixes.
/// Names and titles without Latin letters fall back to `ref` + year.
pub fn citation_keys(items: &[Item]) -> Vec<String> {
    let mut keys: Vec<String> = items
        .iter()
        .map(|item| {
            let name = item.creators.first().map(|c| c.short_name()).unwrap_or_default();
            let name = if contains_cjk(name) { String::new() } else { ascii_slug(name) };
            let word = item
                .title
                .split(|c: char| !c.is_alphanumeric())
                .map(ascii_slug)
                .find(|w| w.len() > 2 && !STOP_WORDS.contains(&w.as_str()))
                .unwrap_or_default();
            let year = item.year.map(|y| y.to_string()).unwrap_or_default();
            if name.is_empty() && word.is_empty() {
                format!("ref{year}")
            } else {
                format!("{name}{year}{word}")
            }
        })
        .collect();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for key in &keys {
        *seen.entry(key.clone()).or_default() += 1;
    }
    let mut used: HashMap<String, usize> = HashMap::new();
    for key in &mut keys {
        if seen[key.as_str()] > 1 {
            let n = used.entry(key.clone()).or_default();
            let suffix = char::from(b'a' + (*n % 26) as u8);
            *n += 1;
            key.push(suffix);
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use paperloom_model::ItemType;

    use super::*;

    #[test]
    fn reads_dates() {
        assert_eq!(year_in("2013/07/31/"), Some(2013));
        assert_eq!(year_in("July 2013"), Some(2013));
        assert_eq!(iso_date("2013/7/31/"), Some("2013-07-31".into()));
        assert_eq!(iso_date("2013"), Some("2013".into()));
        assert_eq!(iso_date("2013/13"), None);
        assert_eq!(pages(Some("54"), Some("58")), Some("54-58".into()));
        assert_eq!(pages(Some("e123"), None), Some("e123".into()));
    }

    #[test]
    fn makes_unique_keys() {
        let mut a = Item::new(ItemType::JournalArticle, "The Nanometre-scale thermometry");
        a.creators.push(Creator::person("Kučsko", "G."));
        a.year = Some(2013);
        let b = a.clone();
        let mut c = Item::new(ItemType::JournalArticle, "深度学习");
        c.creators.push(Creator::literal("张三"));
        c.year = Some(2021);
        let keys = citation_keys(&[a, b, c]);
        // `č` is not folded and drops out; the duplicate pair gets suffixes.
        assert_eq!(keys, ["kusko2013nanometrea", "kusko2013nanometreb", "ref2021"]);
    }

    #[test]
    fn reads_dates_in_words() {
        assert_eq!(date_from_words("2013 Aug 1"), (Some(2013), Some("2013-08-01".into())));
        assert_eq!(date_from_words("2013 AUG 1"), (Some(2013), Some("2013-08-01".into())));
        assert_eq!(date_from_words("2013 Jul-Aug"), (Some(2013), Some("2013-07".into())));
        assert_eq!(date_from_words("Spring 2013"), (Some(2013), Some("2013".into())));
        assert_eq!(date_from_words("no date"), (None, None));
    }

    #[test]
    fn names_languages_and_quiets_shouting() {
        assert_eq!(language_code("English"), "en");
        assert_eq!(language_code("chi"), "zh");
        assert_eq!(language_code("Esperanto"), "Esperanto");
        assert_eq!(
            title_case_if_shouting("JOURNAL OF THE AMERICAN CHEMICAL SOCIETY"),
            "Journal of the American Chemical Society"
        );
        assert_eq!(title_case_if_shouting("Nature"), "Nature");
    }
}
