//! Recognising what a person pasted: a DOI, an arXiv ID, a PubMed ID or an
//! ISBN.

use std::fmt;

use paperloom_model::Doi;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identifier {
    Doi(Doi),
    /// New style `2301.01234` or old style `hep-th/9901001`, without version.
    Arxiv(String),
    Pmid(String),
    /// Digits only (ISBN-13, or ISBN-10 possibly ending in `X`).
    Isbn(String),
}

impl Identifier {
    /// Recognises an identifier in pasted text: bare, labelled (`arXiv:`,
    /// `PMID:`, `ISBN`) or as a link.
    pub fn detect(input: &str) -> Option<Identifier> {
        let text = input.trim();
        if text.is_empty() {
            return None;
        }
        if let Some(id) = arxiv(text) {
            return Some(Identifier::Arxiv(id));
        }
        if let Ok(doi) = Doi::parse(text) {
            return Some(Identifier::Doi(doi));
        }
        if let Some(id) = pmid(text) {
            return Some(Identifier::Pmid(id));
        }
        isbn(text).map(Identifier::Isbn)
    }

    /// The identifier a paper's first pages most likely name it by: the
    /// first DOI printed there (or, on a scanned page, one misread after a
    /// `doi` label), else an `arXiv:` stamp.
    pub fn find_in_text(text: &str) -> Option<Identifier> {
        if let Some(doi) = Doi::find_in_text(text).into_iter().next().or_else(|| Doi::find_misread(text)) {
            return Some(match arxiv(doi.as_str()) {
                Some(id) => Identifier::Arxiv(id),
                None => Identifier::Doi(doi),
            });
        }
        // ASCII lowercasing keeps byte offsets, so positions carry over.
        let lower = text.to_ascii_lowercase();
        let mut from = 0;
        while let Some(pos) = lower[from..].find("arxiv:") {
            let start = from + pos + "arxiv:".len();
            let candidate: String = text[start..]
                .trim_start()
                .chars()
                .take_while(|c| !c.is_whitespace() && !matches!(c, ',' | ';' | ')' | ']'))
                .collect();
            if let Some(id) = arxiv(&format!("arXiv:{}", candidate.trim_end_matches('.'))) {
                return Some(Identifier::Arxiv(id));
            }
            from = start;
        }
        None
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Identifier::Doi(_) => "doi",
            Identifier::Arxiv(_) => "arxiv",
            Identifier::Pmid(_) => "pmid",
            Identifier::Isbn(_) => "isbn",
        }
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Identifier::Doi(d) => write!(f, "DOI {d}"),
            Identifier::Arxiv(id) => write!(f, "arXiv:{id}"),
            Identifier::Pmid(id) => write!(f, "PMID {id}"),
            Identifier::Isbn(id) => write!(f, "ISBN {id}"),
        }
    }
}

fn strip_prefix_ci<'a>(text: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    let lower = text.to_ascii_lowercase();
    prefixes.iter().find(|p| lower.starts_with(*p)).map(|p| text[p.len()..].trim_start())
}

fn arxiv(text: &str) -> Option<String> {
    let rest = strip_prefix_ci(
        text,
        &[
            "https://arxiv.org/abs/",
            "http://arxiv.org/abs/",
            "https://arxiv.org/pdf/",
            "http://arxiv.org/pdf/",
            "arxiv.org/abs/",
            "arxiv.org/pdf/",
            "arxiv:",
        ],
    );
    let labelled = rest.is_some();
    let rest = rest.unwrap_or(text);
    // The arXiv DOI 10.48550/arXiv.2301.01234 is the same paper.
    let rest = strip_prefix_ci(rest, &["10.48550/arxiv."]).unwrap_or(rest);
    let id = rest.trim_end_matches(".pdf").trim_end_matches('/');
    let id = match id.rfind('v') {
        Some(i) if i > 0 && id[i + 1..].chars().all(|c| c.is_ascii_digit()) && i + 1 < id.len() => &id[..i],
        _ => id,
    };
    let new_style = id.split_once('.').is_some_and(|(a, b)| {
        a.len() == 4
            && a.bytes().all(|c| c.is_ascii_digit())
            && (4..=5).contains(&b.len())
            && b.bytes().all(|c| c.is_ascii_digit())
    });
    // Old style needs the label: `math/0601001` alone could be anything.
    let old_style = labelled
        && id.split_once('/').is_some_and(|(archive, number)| {
            !archive.is_empty()
                && archive.chars().all(|c| c.is_ascii_alphabetic() || c == '-' || c == '.')
                && number.len() == 7
                && number.bytes().all(|c| c.is_ascii_digit())
        });
    (new_style || old_style).then(|| id.to_string())
}

fn pmid(text: &str) -> Option<String> {
    let labelled = strip_prefix_ci(
        text,
        &["pmid:", "pmid", "https://pubmed.ncbi.nlm.nih.gov/", "pubmed.ncbi.nlm.nih.gov/"],
    );
    let id = labelled.unwrap_or(text).trim_end_matches('/');
    let digits = !id.is_empty() && id.len() <= 8 && id.bytes().all(|c| c.is_ascii_digit());
    // Bare numbers of 7–8 digits are PubMed IDs; shorter ones need the label.
    (digits && (labelled.is_some() || id.len() >= 7)).then(|| id.trim_start_matches('0').to_string())
}

fn isbn(text: &str) -> Option<String> {
    let rest = strip_prefix_ci(text, &["isbn-13:", "isbn-10:", "isbn:", "isbn"]).unwrap_or(text);
    let compact: String =
        rest.chars().filter(|c| !matches!(c, '-' | ' ')).collect::<String>().to_ascii_uppercase();
    let valid = match compact.len() {
        10 => {
            let digits: Option<Vec<u32>> = compact
                .chars()
                .enumerate()
                .map(|(i, c)| if i == 9 && c == 'X' { Some(10) } else { c.to_digit(10) })
                .collect();
            digits.is_some_and(|d| {
                d.iter().enumerate().map(|(i, v)| (10 - i as u32) * v).sum::<u32>().is_multiple_of(11)
            })
        }
        13 => {
            let digits: Option<Vec<u32>> = compact.chars().map(|c| c.to_digit(10)).collect();
            digits.is_some_and(|d| {
                (compact.starts_with("978") || compact.starts_with("979"))
                    && d.iter()
                        .enumerate()
                        .map(|(i, v)| if i.is_multiple_of(2) { *v } else { 3 * v })
                        .sum::<u32>()
                        .is_multiple_of(10)
            })
        }
        _ => false,
    };
    valid.then_some(compact)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(input: &str) -> Option<(&'static str, String)> {
        Identifier::detect(input).map(|id| {
            let value = match &id {
                Identifier::Doi(d) => d.to_string(),
                Identifier::Arxiv(s) | Identifier::Pmid(s) | Identifier::Isbn(s) => s.clone(),
            };
            (id.kind(), value)
        })
    }

    #[test]
    fn recognises_dois() {
        assert_eq!(kind("https://doi.org/10.1038/NATURE12373"), Some(("doi", "10.1038/nature12373".into())));
    }

    #[test]
    fn recognises_arxiv_ids() {
        for input in [
            "2301.01234",
            "arXiv:2301.01234v2",
            "https://arxiv.org/abs/2301.01234",
            "https://arxiv.org/pdf/2301.01234v1.pdf",
            "10.48550/arXiv.2301.01234",
        ] {
            assert_eq!(kind(input), Some(("arxiv", "2301.01234".into())), "{input}");
        }
        assert_eq!(kind("arXiv:hep-th/9901001"), Some(("arxiv", "hep-th/9901001".into())));
        assert_eq!(kind("hep-th/9901001"), None);
        assert_eq!(kind("1234.5"), None);
    }

    #[test]
    fn recognises_pubmed_ids() {
        assert_eq!(kind("PMID: 23903748"), Some(("pmid", "23903748".into())));
        assert_eq!(kind("https://pubmed.ncbi.nlm.nih.gov/23903748/"), Some(("pmid", "23903748".into())));
        assert_eq!(kind("23903748"), Some(("pmid", "23903748".into())));
        assert_eq!(kind("pmid 123"), Some(("pmid", "123".into())));
        assert_eq!(kind("123"), None);
    }

    #[test]
    fn recognises_isbns() {
        assert_eq!(kind("ISBN 978-0-262-03384-8"), Some(("isbn", "9780262033848".into())));
        assert_eq!(kind("0-306-40615-2"), Some(("isbn", "0306406152".into())));
        assert_eq!(kind("080442957X"), Some(("isbn", "080442957X".into())));
        assert_eq!(kind("978-0-262-03384-9"), None, "bad check digit");
    }

    #[test]
    fn ignores_other_text() {
        for input in ["", "   ", "hello world", "deep learning 2023"] {
            assert_eq!(kind(input), None, "{input}");
        }
    }

    #[test]
    fn finds_identifiers_on_a_first_page() {
        let published = "Nature 500, 54–58 (2013)\nhttps://doi.org/10.1038/nature12373\nReceived 3 May";
        assert_eq!(
            Identifier::find_in_text(published),
            Some(Identifier::Doi(Doi::parse("10.1038/nature12373").unwrap()))
        );
        let preprint = "Attention Is All You Need\narXiv:1706.03762v7 [cs.CL] 2 Aug 2023";
        assert_eq!(Identifier::find_in_text(preprint), Some(Identifier::Arxiv("1706.03762".into())));
        let old = "see ARXIV: hep-th/9901001v2, and more";
        assert_eq!(Identifier::find_in_text(old), Some(Identifier::Arxiv("hep-th/9901001".into())));
        let arxiv_doi = "DOI: 10.48550/arXiv.2301.01234";
        assert_eq!(Identifier::find_in_text(arxiv_doi), Some(Identifier::Arxiv("2301.01234".into())));
        assert_eq!(Identifier::find_in_text("基于深度学习的文献分类 张三 李四 arXiv: none"), None);
        // Read by text recognition from a scanned page.
        assert_eq!(
            Identifier::find_in_text(
                "Nanometre-scale thermometry in a living cell doi:l O. 1038/nature12373"
            ),
            Some(Identifier::Doi(Doi::parse("10.1038/nature12373").unwrap()))
        );
    }
}
