//! BibTeX and BibLaTeX.

use std::collections::HashMap;

use paperloom_model::{Creator, CreatorRole, Doi, Item, ItemType, contains_cjk};

use crate::Parsed;
use crate::text::{citation_keys, iso_date, year_in};

// Reading

struct Reader<'a> {
    chars: Vec<char>,
    pos: usize,
    strings: &'a mut HashMap<String, String>,
}

impl Reader<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn ident(&mut self) -> String {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_alphanumeric() || "-_.:+/".contains(c)) {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    /// The text between balanced delimiters; `pos` is just after `open`.
    fn balanced(&mut self, close: char) -> Option<String> {
        let mut depth = 0usize;
        let start = self.pos;
        while let Some(c) = self.peek() {
            match c {
                '\\' => self.pos += 1, // an escaped character never opens or closes
                '{' => depth += 1,
                '}' if depth > 0 => depth -= 1,
                c if c == close && depth == 0 => {
                    let text = self.chars[start..self.pos].iter().collect();
                    self.pos += 1;
                    return Some(text);
                }
                _ => {}
            }
            self.pos += 1;
        }
        None
    }

    /// A field value: `{…}`, `"…"`, a number or a `@string` name, joined by `#`.
    fn value(&mut self) -> Option<String> {
        let mut out = String::new();
        loop {
            self.skip_ws();
            match self.peek()? {
                '{' => {
                    self.pos += 1;
                    out.push_str(&self.balanced('}')?);
                }
                '"' => {
                    self.pos += 1;
                    out.push_str(&self.balanced('"')?);
                }
                _ => {
                    let word = self.ident();
                    if word.is_empty() {
                        return None;
                    }
                    let macro_value = self.strings.get(&word.to_lowercase()).cloned();
                    out.push_str(&macro_value.unwrap_or(word));
                }
            }
            self.skip_ws();
            if self.peek() == Some('#') {
                self.pos += 1;
            } else {
                return Some(out);
            }
        }
    }

    /// `name = value` pairs up to the entry's closing delimiter.
    fn fields(&mut self, close: char) -> Option<Vec<(String, String)>> {
        let mut fields = Vec::new();
        loop {
            self.skip_ws();
            match self.peek()? {
                ',' => self.pos += 1,
                c if c == close => {
                    self.pos += 1;
                    return Some(fields);
                }
                _ => {
                    let name = self.ident().to_lowercase();
                    self.skip_ws();
                    if name.is_empty() || self.peek()? != '=' {
                        return None;
                    }
                    self.pos += 1;
                    let value = self.value()?;
                    fields.push((name, value));
                }
            }
        }
    }
}

const MONTHS: [&str; 12] =
    ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

pub(crate) fn parse(text: &str) -> Parsed {
    let mut strings: HashMap<String, String> =
        MONTHS.iter().enumerate().map(|(i, m)| (m.to_string(), (i + 1).to_string())).collect();
    let mut reader = Reader { chars: text.chars().collect(), pos: 0, strings: &mut strings };
    let mut parsed = Parsed::default();
    while let Some(at) = reader.chars[reader.pos..].iter().position(|&c| c == '@') {
        reader.pos += at + 1;
        let kind = reader.ident().to_lowercase();
        reader.skip_ws();
        let close = match reader.peek() {
            Some('{') => '}',
            Some('(') => ')',
            _ => continue,
        };
        reader.pos += 1;
        match kind.as_str() {
            "comment" | "preamble" => {
                reader.balanced(close);
            }
            "string" => {
                if let Some(fields) = reader.fields(close) {
                    for (name, value) in fields {
                        reader.strings.insert(name, value);
                    }
                }
            }
            _ => {
                reader.skip_ws();
                // The citation key, up to the first comma.
                while reader.peek().is_some_and(|c| c != ',' && c != close) {
                    reader.pos += 1;
                }
                match reader.fields(close).and_then(|fields| build(&kind, &fields)) {
                    Some(item) => parsed.items.push(item),
                    None => parsed.skipped += 1,
                }
            }
        }
    }
    parsed
}

fn item_type(kind: &str, fields: &HashMap<&str, &str>) -> ItemType {
    match kind {
        "article" | "periodical" => ItemType::JournalArticle,
        "inproceedings" | "conference" => ItemType::ConferencePaper,
        "book" | "mvbook" | "booklet" | "proceedings" | "collection" => ItemType::Book,
        "inbook" | "incollection" | "inreference" | "bookinbook" | "suppbook" => ItemType::BookChapter,
        "phdthesis" | "mastersthesis" | "thesis" => ItemType::Thesis,
        "techreport" | "report" => ItemType::Report,
        "patent" => ItemType::Patent,
        "standard" => ItemType::Standard,
        "dataset" | "software" => ItemType::Dataset,
        "online" | "www" | "electronic" | "webpage" => ItemType::Webpage,
        "unpublished" | "preprint" => ItemType::Preprint,
        _ if fields.contains_key("eprint") => ItemType::Preprint,
        _ => ItemType::Other,
    }
}

fn build(kind: &str, raw: &[(String, String)]) -> Option<Item> {
    let fields: HashMap<&str, &str> = raw.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let get = |name: &str| fields.get(name).map(|v| decode_latex(v)).filter(|v| !v.is_empty());
    let item_type = item_type(kind, &fields);
    let mut item = Item::new(item_type, get("title")?);
    for (field, role) in [
        ("author", CreatorRole::Author),
        ("editor", CreatorRole::Editor),
        ("translator", CreatorRole::Translator),
    ] {
        if let Some(names) = fields.get(field) {
            for name in split_names(names) {
                let mut creator = parse_name(&name);
                creator.role = role;
                item.creators.push(creator);
            }
        }
    }
    item.container_title = get("journal")
        .or_else(|| get("journaltitle"))
        .or_else(|| get("booktitle"))
        .or_else(|| get("maintitle"));
    item.container_short_title = get("shortjournal").or_else(|| get("journalabbreviation"));
    let date = get("date");
    item.year = get("year").as_deref().and_then(year_in).or_else(|| date.as_deref().and_then(year_in));
    let month = get("month").and_then(|m| month_number(&m));
    item.date = date.as_deref().and_then(iso_date).or_else(|| match (item.year, month) {
        (Some(y), Some(m)) => Some(format!("{y:04}-{m:02}")),
        (Some(y), None) => Some(format!("{y:04}")),
        _ => None,
    });
    item.volume = get("volume");
    let number = get("number").or_else(|| get("issue"));
    if matches!(item_type, ItemType::Report | ItemType::Patent | ItemType::Standard) {
        item.number = number;
    } else {
        item.issue = number;
    }
    item.pages = get("pages").map(|p| p.replace("--", "-").replace('–', "-"));
    item.publisher = get("publisher")
        .or_else(|| get("school"))
        .or_else(|| get("institution"))
        .or_else(|| get("organization"));
    item.place = get("address").or_else(|| get("location"));
    item.edition = get("edition");
    item.doi = get("doi").and_then(|d| Doi::parse(&d).ok());
    item.isbn = get("isbn");
    item.issn = get("issn");
    item.pmid = get("pmid");
    item.url = get("url");
    item.accessed = get("urldate").as_deref().and_then(iso_date);
    item.abstract_note = get("abstract");
    item.keywords = get("keywords")
        .map(|k| k.split([',', ';']).map(str::trim).filter(|k| !k.is_empty()).map(String::from).collect())
        .unwrap_or_default();
    item.language = get("langid").or_else(|| get("language"));
    let archive = get("archiveprefix").or_else(|| get("eprinttype")).unwrap_or_default().to_lowercase();
    if let Some(eprint) = get("eprint")
        && (archive.is_empty() || archive == "arxiv")
    {
        item.arxiv_id = Some(eprint);
    }
    if item_type == ItemType::Thesis && item.number.is_none() {
        item.number = match kind {
            "phdthesis" => Some("PhD thesis".to_string()),
            "mastersthesis" => Some("Master's thesis".to_string()),
            _ => get("type"),
        };
    }
    Some(item)
}

fn month_number(text: &str) -> Option<u32> {
    if let Ok(n) = text.trim().parse::<u32>() {
        return (1..=12).contains(&n).then_some(n);
    }
    let lower = text.trim().to_lowercase();
    MONTHS.iter().position(|m| lower.starts_with(m)).map(|i| i as u32 + 1)
}

/// Splits `A and B and {C and D}` at top-level ` and `.
fn split_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    let words: Vec<&str> = text.split_inclusive(char::is_whitespace).collect();
    for word in words {
        if depth == 0 && word.trim().eq_ignore_ascii_case("and") && !current.trim().is_empty() {
            names.push(std::mem::take(&mut current));
            continue;
        }
        for c in word.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        current.push_str(word);
    }
    if !current.trim().is_empty() {
        names.push(current);
    }
    names.into_iter().map(|n| n.trim().to_string()).collect()
}

fn parse_name(raw: &str) -> Creator {
    let trimmed = raw.trim();
    // `{World Health Organization}`: kept whole.
    if trimmed.starts_with('{')
        && trimmed.ends_with('}')
        && split_names(&trimmed[1..trimmed.len() - 1]).len() == 1
    {
        let inner = &trimmed[1..trimmed.len() - 1];
        if !inner.contains(['{', '}']) || inner.matches('{').count() == inner.matches('}').count() {
            return Creator::literal(decode_latex(inner));
        }
    }
    let name = decode_latex(trimmed);
    if contains_cjk(&name) {
        return Creator::literal(name.replace(' ', ""));
    }
    let parts: Vec<&str> = name.split(',').map(str::trim).collect();
    match parts.as_slice() {
        [family, given] => Creator::person(*family, *given),
        // `von Last, Jr, First`
        [family, _suffix, given] => Creator::person(*family, *given),
        _ => Creator::parse(&name),
    }
}

/// Turns LaTeX markup into plain Unicode text: accents, escaped specials,
/// `\emph{…}`-style commands (their text kept), braces and `$` removed.
pub(crate) fn decode_latex(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() => {
                let next = chars[i + 1];
                if "'\"`^~=.".contains(next)
                    || (matches!(next, 'c' | 'v' | 'u' | 'H' | 'r' | 'k')
                        && i + 2 < chars.len()
                        && !chars[i + 2].is_alphabetic())
                {
                    // \'e  \'{e}  \c{c}  \v c
                    let mut j = i + 2;
                    while j < chars.len() && chars[j] == ' ' && next.is_alphabetic() {
                        j += 1;
                    }
                    let (base, end) = if chars.get(j) == Some(&'{') {
                        match (chars.get(j + 1), chars.get(j + 2)) {
                            (Some(&b), Some(&'}')) => (Some(b), j + 3),
                            _ => (None, j),
                        }
                    } else {
                        (chars.get(j).copied(), j + 1)
                    };
                    match base.and_then(|b| accent(next, b)) {
                        Some(accented) => {
                            out.push(accented);
                            i = end;
                        }
                        None => {
                            i += 2;
                        }
                    }
                    continue;
                }
                if "&%$#_{}".contains(next) {
                    out.push(next);
                    i += 2;
                    continue;
                }
                // A command name: \ss, \o, \aa, \emph, \textit…
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_ascii_alphabetic() {
                    j += 1;
                }
                let name: String = chars[i + 1..j].iter().collect();
                if let Some(letter) = letter_command(&name) {
                    out.push_str(letter);
                } else if name.is_empty() {
                    out.push(next);
                    j += 1;
                } else if chars.get(j) != Some(&'{') {
                    // \alpha, \LaTeX: the name is the best plain-text guess.
                    out.push_str(&name);
                }
                // \emph{…}-style commands drop out; their argument stays as text.
                i = j;
                // `\ss{}` and `\o ` swallow a following space or empty group.
                if letter_command(&name).is_some() {
                    if chars.get(i) == Some(&' ') {
                        i += 1;
                    } else if chars.get(i) == Some(&'{') && chars.get(i + 1) == Some(&'}') {
                        i += 2;
                    }
                }
            }
            '{' | '}' | '$' => i += 1,
            '~' => {
                out.push(' ');
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn letter_command(name: &str) -> Option<&'static str> {
    Some(match name {
        "ss" => "ß",
        "o" => "ø",
        "O" => "Ø",
        "aa" => "å",
        "AA" => "Å",
        "ae" => "æ",
        "AE" => "Æ",
        "oe" => "œ",
        "OE" => "Œ",
        "l" => "ł",
        "L" => "Ł",
        "i" => "ı",
        _ => return None,
    })
}

fn accent(mark: char, base: char) -> Option<char> {
    let table: &[(char, &str, &str)] = &[
        ('\'', "aeiouyAEIOUYcnszCNSZ", "áéíóúýÁÉÍÓÚÝćńśźĆŃŚŹ"),
        ('`', "aeiouAEIOU", "àèìòùÀÈÌÒÙ"),
        ('"', "aeiouyAEIOUY", "äëïöüÿÄËÏÖÜŸ"),
        ('^', "aeiouAEIOU", "âêîôûÂÊÎÔÛ"),
        ('~', "anoANO", "ãñõÃÑÕ"),
        ('c', "csCS", "çşÇŞ"),
        ('v', "cszrenCSZREN", "čšžřěňČŠŽŘĚŇ"),
        ('u', "agAG", "ăğĂĞ"),
        ('H', "ouOU", "őűŐŰ"),
        ('r', "auAU", "åůÅŮ"),
        ('=', "aeiouAEIOU", "āēīōūĀĒĪŌŪ"),
        ('.', "zZeE", "żŻėĖ"),
        ('k', "aeAE", "ąęĄĘ"),
    ];
    let (_, bases, marked) = table.iter().find(|(m, _, _)| *m == mark)?;
    let index = bases.chars().position(|b| b == base)?;
    marked.chars().nth(index)
}

// Writing

fn entry_type(item: &Item) -> &'static str {
    match item.item_type {
        ItemType::JournalArticle => "article",
        ItemType::ConferencePaper => "inproceedings",
        ItemType::Book => "book",
        ItemType::BookChapter => "incollection",
        ItemType::Thesis => {
            let degree = item.number.as_deref().unwrap_or("").to_lowercase();
            if degree.contains("master") || degree.contains("硕士") { "mastersthesis" } else { "phdthesis" }
        }
        ItemType::Report => "techreport",
        ItemType::Patent => "patent",
        ItemType::Webpage => "online",
        ItemType::Preprint | ItemType::Standard | ItemType::Dataset | ItemType::Other => "misc",
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "&%$#_".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out.replace(['\r', '\n'], " ")
}

fn name(creator: &Creator) -> String {
    if !creator.literal.is_empty() {
        // Braces keep organisations and CJK names in one piece.
        format!("{{{}}}", escape(&creator.literal))
    } else if creator.given.is_empty() {
        escape(&creator.family)
    } else {
        format!("{}, {}", escape(&creator.family), escape(&creator.given))
    }
}

pub(crate) fn write(items: &[Item]) -> String {
    let keys = citation_keys(items);
    let mut out = String::new();
    for (item, key) in items.iter().zip(keys) {
        let mut fields: Vec<(&str, String)> = Vec::new();
        let mut put = |name: &'static str, value: Option<String>| {
            if let Some(v) = value.filter(|v| !v.trim().is_empty()) {
                fields.push((name, v));
            }
        };
        let names = |role: CreatorRole| {
            let list: Vec<String> = item.creators.iter().filter(|c| c.role == role).map(name).collect();
            (!list.is_empty()).then(|| list.join(" and "))
        };
        put("author", names(CreatorRole::Author));
        put("editor", names(CreatorRole::Editor));
        put("title", Some(escape(&item.title)));
        let container = item.container_title.as_deref().map(escape);
        match item.item_type {
            ItemType::JournalArticle => put("journal", container),
            ItemType::ConferencePaper | ItemType::BookChapter => put("booktitle", container),
            _ => put("howpublished", container),
        }
        put("year", item.year.map(|y| y.to_string()));
        put("date", item.date.clone().filter(|d| d.len() > 4));
        put("volume", item.volume.clone());
        let number = if matches!(item.item_type, ItemType::Report | ItemType::Patent | ItemType::Standard) {
            item.number.clone()
        } else {
            item.issue.clone()
        };
        put("number", number);
        put("pages", item.pages.as_deref().map(|p| p.replacen('-', "--", 1)));
        let publisher_field = match item.item_type {
            ItemType::Thesis => "school",
            ItemType::Report => "institution",
            _ => "publisher",
        };
        put(publisher_field, item.publisher.as_deref().map(escape));
        put("address", item.place.as_deref().map(escape));
        put("edition", item.edition.clone());
        put("doi", item.doi.as_ref().map(|d| d.to_string()));
        put("isbn", item.isbn.clone());
        put("issn", item.issn.clone());
        if let Some(arxiv) = &item.arxiv_id {
            put("eprint", Some(arxiv.clone()));
            put("archiveprefix", Some("arXiv".to_string()));
        }
        put("pmid", item.pmid.clone());
        put("url", item.url.clone());
        put("urldate", item.accessed.clone());
        put("abstract", item.abstract_note.as_deref().map(escape));
        put("keywords", (!item.keywords.is_empty()).then(|| escape(&item.keywords.join(", "))));
        put("langid", item.language.clone());
        out.push_str(&format!("@{}{{{key},\n", entry_type(item)));
        for (name, value) in &fields {
            out.push_str(&format!("  {name} = {{{value}}},\n"));
        }
        out.push_str("}\n\n");
    }
    out
}

#[cfg(test)]
mod tests;
