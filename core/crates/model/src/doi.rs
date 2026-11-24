//! Digital Object Identifiers.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A DOI in canonical form: `10.<registrant>/<suffix>`, without resolver URL
/// or `doi:` label, lower-cased (DOIs are case-insensitive).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Doi(String);

/// The input does not contain a well-formed DOI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidDoi(pub String);

impl fmt::Display for InvalidDoi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a DOI: {:?}", self.0)
    }
}

impl std::error::Error for InvalidDoi {}

/// Labels and resolver URLs people paste in front of a DOI, lower-case.
const PREFIXES: &[&str] = &[
    "https://doi.org/",
    "http://doi.org/",
    "https://dx.doi.org/",
    "http://dx.doi.org/",
    "doi.org/",
    "dx.doi.org/",
    "doi:",
    "doi：",
    "doi ",
];

/// Characters that end a DOI found in running text. DOIs may contain
/// almost anything, but in practice not these.
fn ends_doi_in_text(c: char) -> bool {
    c.is_whitespace() || "\"<>{}|\\^`".contains(c) || "。，；：、）（》《」「』『【】".contains(c)
}

impl Doi {
    /// Parses a DOI typed or pasted by a person: accepts resolver URLs,
    /// `doi:` labels, percent-encoding and trailing punctuation.
    pub fn parse(input: &str) -> Result<Self, InvalidDoi> {
        let mut s = input.trim();
        loop {
            let lower = s.to_lowercase();
            match PREFIXES.iter().find(|p| lower.starts_with(*p)) {
                // Lower-casing keeps the byte length of these ASCII/CJK prefixes.
                Some(p) => s = s[p.len()..].trim_start(),
                None => break,
            }
        }
        let decoded = percent_decode(s);
        let candidate = trim_trailing_punctuation(decoded.trim());
        if is_well_formed(candidate) {
            Ok(Self(candidate.to_lowercase()))
        } else {
            Err(InvalidDoi(input.to_string()))
        }
    }

    /// Every distinct DOI in a piece of text (a PDF page, a web page, a
    /// reference list), in order of first appearance.
    pub fn find_in_text(text: &str) -> Vec<Self> {
        let mut found: Vec<Self> = Vec::new();
        let mut rest = text;
        while let Some(pos) = rest.find("10.") {
            let preceded_by_word =
                rest[..pos].chars().next_back().is_some_and(|c| c.is_ascii_alphanumeric() || c == '.');
            let tail = &rest[pos..];
            rest = &tail[3..];
            if preceded_by_word {
                continue;
            }
            let end = tail.find(ends_doi_in_text).unwrap_or(tail.len());
            match Self::parse(&tail[..end]) {
                Ok(doi) if !found.contains(&doi) => found.push(doi),
                _ => {}
            }
        }
        found
    }

    /// A DOI printed after a `doi` label but misread by text recognition,
    /// as Windows' recognizer read `doi:10.1038/…` from a scanned page:
    /// `doi:l O. 1038/nature12373`. Only the `10.<registrant>` prefix is
    /// mended (1 read as l, I or |; 0 as O or o; stray spaces), and only
    /// right after the label, where nothing else is expected; the suffix is
    /// kept as read. For scanned pages where `find_in_text` finds none.
    pub fn find_misread(text: &str) -> Option<Self> {
        // ASCII lowercasing keeps byte offsets.
        let lower = text.to_ascii_lowercase();
        let mut from = 0;
        while let Some(pos) = lower[from..].find("doi") {
            let start = from + pos + "doi".len();
            from = start;
            let after =
                text[start..].trim_start_matches(|c: char| c == ':' || c == '：' || c.is_whitespace());
            let Some(slash) = after.find('/') else { continue };
            let (prefix, rest) = (&after[..slash], &after[slash + 1..]);
            if prefix.chars().count() > 20
                || !prefix.chars().all(|c| c.is_ascii_digit() || ".lIO|o ".contains(c))
            {
                continue;
            }
            let mended: String = prefix
                .chars()
                .filter(|c| *c != ' ')
                .map(|c| match c {
                    'l' | 'I' | '|' => '1',
                    'O' | 'o' => '0',
                    c => c,
                })
                .collect();
            let suffix = &rest[..rest.find(ends_doi_in_text).unwrap_or(rest.len())];
            if let Ok(doi) = Self::parse(&format!("{mended}/{suffix}")) {
                return Some(doi);
            }
        }
        None
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The registrant prefix, e.g. `10.1038`.
    pub fn prefix(&self) -> &str {
        self.0.split_once('/').map_or(self.0.as_str(), |(p, _)| p)
    }

    /// The https URL that resolves this DOI.
    pub fn url(&self) -> String {
        format!("https://doi.org/{}", self.0)
    }
}

fn is_well_formed(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("10.") else {
        return false;
    };
    let Some((registrant, suffix)) = rest.split_once('/') else {
        return false;
    };
    let mut parts = registrant.split('.');
    let first = parts.next().unwrap_or("");
    (4..=9).contains(&first.len())
        && first.bytes().all(|b| b.is_ascii_digit())
        && parts.all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        && !suffix.is_empty()
        && !suffix.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Drops sentence punctuation stuck to the end, and closing brackets that
/// have no opening partner inside the DOI (`(see 10.1000/x)`).
fn trim_trailing_punctuation(mut s: &str) -> &str {
    loop {
        let Some(last) = s.chars().next_back() else {
            return s;
        };
        let drop = match last {
            '.' | ',' | ';' | ':' | '\'' | '"' | '。' | '，' | '；' => true,
            ')' => s.matches('(').count() < s.matches(')').count(),
            ']' => s.matches('[').count() < s.matches(']').count(),
            '}' => s.matches('{').count() < s.matches('}').count(),
            _ => false,
        };
        if !drop {
            return s;
        }
        s = &s[..s.len() - last.len_utf8()];
    }
}

/// Decodes `%XX` escapes (DOIs copied from URLs); leaves the text alone if
/// the result would not be UTF-8.
fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

impl fmt::Display for Doi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Doi {
    type Error = InvalidDoi;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<Doi> for String {
    fn from(doi: Doi) -> Self {
        doi.0
    }
}

impl std::str::FromStr for Doi {
    type Err = InvalidDoi;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doi(s: &str) -> String {
        Doi::parse(s).map(String::from).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn accepts_the_usual_spellings() {
        for input in [
            "10.1038/nature12373",
            "  10.1038/NATURE12373 ",
            "doi:10.1038/nature12373",
            "DOI: 10.1038/nature12373",
            "DOI：10.1038/nature12373",
            "https://doi.org/10.1038/nature12373",
            "http://dx.doi.org/10.1038/nature12373",
            "doi.org/10.1038/nature12373",
            "https://doi.org/10.1038%2Fnature12373",
            "10.1038/nature12373.",
        ] {
            assert_eq!(doi(input), "10.1038/nature12373", "input {input:?}");
        }
    }

    #[test]
    fn keeps_balanced_brackets_inside_the_suffix() {
        assert_eq!(
            doi("10.1002/(SICI)1097-4636(199706)35:4<425::AID-JBM3>3.0.CO;2-F"),
            "10.1002/(sici)1097-4636(199706)35:4<425::aid-jbm3>3.0.co;2-f"
        );
        assert_eq!(doi("10.1000/abc(1))"), "10.1000/abc(1)");
    }

    #[test]
    fn accepts_dotted_registrants() {
        assert_eq!(doi("10.1000.10/123456"), "10.1000.10/123456");
    }

    #[test]
    fn rejects_non_dois() {
        for input in [
            "",
            "hello",
            "11.1038/nature12373",
            "10.1/x",
            "10.1038",
            "10.1038/",
            "10.10a8/x",
            "10.1038/has space",
        ] {
            assert!(Doi::parse(input).is_err(), "input {input:?}");
        }
    }

    #[test]
    fn finds_dois_in_text() {
        let text = "See doi:10.1038/nature12373, and (https://doi.org/10.1126/science.169.3946.635). \
                    Again 10.1038/NATURE12373; version 210.1000/not-a-doi. \
                    中文：DOI：10.13336/j.1003-6520.hve.20200101001。";
        let found: Vec<String> = Doi::find_in_text(text).into_iter().map(String::from).collect();
        assert_eq!(
            found,
            ["10.1038/nature12373", "10.1126/science.169.3946.635", "10.13336/j.1003-6520.hve.20200101001",]
        );
    }

    #[test]
    fn mends_dois_misread_by_text_recognition() {
        let found = |text: &str| Doi::find_misread(text).map(String::from);
        assert_eq!(
            found("Nanometre-scale thermometry doi:l O. 1038/nature12373 Notes").as_deref(),
            Some("10.1038/nature12373")
        );
        assert_eq!(
            found("DOI： IO.1126/science.169.3946.635.").as_deref(),
            Some("10.1126/science.169.3946.635")
        );
        assert_eq!(found("doi: 10.1038/nature12373").as_deref(), Some("10.1038/nature12373"));
        // Nothing mended away from a label, or into what is not a prefix.
        assert_eq!(found("l O. 1038/nature12373"), None);
        assert_eq!(found("doing l0 things / more"), None);
        assert_eq!(found("doi: see the journal / page"), None);
    }

    #[test]
    fn exposes_prefix_and_url() {
        let d = Doi::parse("10.1038/nature12373").unwrap();
        assert_eq!(d.prefix(), "10.1038");
        assert_eq!(d.url(), "https://doi.org/10.1038/nature12373");
    }

    #[test]
    fn round_trips_through_json() {
        let d = Doi::parse("10.1038/nature12373").unwrap();
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, "\"10.1038/nature12373\"");
        let back: Doi = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
        assert!(serde_json::from_str::<Doi>("\"nope\"").is_err());
    }
}
