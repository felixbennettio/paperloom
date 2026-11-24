//! MARC records as Z39.50 sends them: ISO 2709, in MARC 21 or UNIMARC
//! (CNMARC is UNIMARC as Chinese libraries write it), their text in UTF-8,
//! MARC-8 or a character set the catalogue names.

use encoding_rs::Encoding;
use paperloom_model::{Creator, CreatorRole, Item, ItemType};
use unicode_normalization::UnicodeNormalization;

use crate::Hit;
use crate::sru::{Field, Record, first_year, isbd, language};

const FIELD_END: u8 = 0x1e;
const SUBFIELD: u8 = 0x1f;

/// MARC-8's special characters (ANSEL), from 0xA1.
const ANSEL_SPECIAL: [(u8, char); 35] = [
    (0xa1, 'Ł'),
    (0xa2, 'Ø'),
    (0xa3, 'Đ'),
    (0xa4, 'Þ'),
    (0xa5, 'Æ'),
    (0xa6, 'Œ'),
    (0xa7, 'ʹ'),
    (0xa8, '·'),
    (0xa9, '♭'),
    (0xaa, '®'),
    (0xab, '±'),
    (0xac, 'Ơ'),
    (0xad, 'Ư'),
    (0xae, 'ʼ'),
    (0xb0, 'ʻ'),
    (0xb1, 'ł'),
    (0xb2, 'ø'),
    (0xb3, 'đ'),
    (0xb4, 'þ'),
    (0xb5, 'æ'),
    (0xb6, 'œ'),
    (0xb7, 'ʺ'),
    (0xb8, 'ı'),
    (0xb9, '£'),
    (0xba, 'ð'),
    (0xbc, 'ơ'),
    (0xbd, 'ư'),
    (0xc0, '°'),
    (0xc1, 'ℓ'),
    (0xc2, '℗'),
    (0xc3, '©'),
    (0xc4, '♯'),
    (0xc5, '¿'),
    (0xc6, '¡'),
    (0xc7, 'ß'),
];

/// MARC-8's combining marks, from 0xE0; they come before the letter they
/// sit on.
const ANSEL_MARKS: [(u8, char); 31] = [
    (0xe0, '\u{309}'),
    (0xe1, '\u{300}'),
    (0xe2, '\u{301}'),
    (0xe3, '\u{302}'),
    (0xe4, '\u{303}'),
    (0xe5, '\u{304}'),
    (0xe6, '\u{306}'),
    (0xe7, '\u{307}'),
    (0xe8, '\u{308}'),
    (0xe9, '\u{30c}'),
    (0xea, '\u{30a}'),
    (0xeb, '\u{fe20}'),
    (0xec, '\u{fe21}'),
    (0xed, '\u{315}'),
    (0xee, '\u{30b}'),
    (0xef, '\u{310}'),
    (0xf0, '\u{327}'),
    (0xf1, '\u{328}'),
    (0xf2, '\u{323}'),
    (0xf3, '\u{324}'),
    (0xf4, '\u{325}'),
    (0xf5, '\u{333}'),
    (0xf6, '\u{332}'),
    (0xf7, '\u{326}'),
    (0xf8, '\u{31c}'),
    (0xf9, '\u{32e}'),
    (0xfa, '\u{fe22}'),
    (0xfb, '\u{fe23}'),
    (0xfc, '\u{338}'),
    (0xfd, '\u{337}'),
    (0xfe, '\u{313}'),
];

/// MARC-8 text in Unicode: ASCII, the Latin specials and combining marks
/// (moved after their letter). Escapes to other character sets are
/// skipped, and what they bring is lost.
pub(crate) fn marc8(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut marks: Vec<char> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        if b == 0x1b {
            // ESC, intermediates, and a final byte.
            while bytes.get(i).is_some_and(|c| (0x20..=0x2f).contains(c)) {
                i += 1;
            }
            i += 1;
            continue;
        }
        if let Some(&(_, mark)) = ANSEL_MARKS.iter().find(|(code, _)| *code == b) {
            marks.push(mark);
            continue;
        }
        let c = match b {
            0x00..=0x7f => char::from(b),
            0xc8 => '€',
            _ => match ANSEL_SPECIAL.iter().find(|(code, _)| *code == b) {
                Some(&(_, c)) => c,
                None => continue,
            },
        };
        out.push(c);
        out.extend(marks.drain(..));
    }
    out.nfc().collect()
}

/// A field's text: UTF-8 when it is, else in the catalogue's character
/// set, else (for UNIMARC, from Chinese libraries) GB 18030 when it reads
/// as that, else MARC-8.
fn text(bytes: &[u8], charset: Option<&'static Encoding>, unimarc: bool) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    if let Some(charset) = charset {
        return charset.decode_without_bom_handling(bytes).0.into_owned();
    }
    if unimarc
        && let Some(text) = encoding_rs::GB18030.decode_without_bom_handling_and_without_replacement(bytes)
    {
        return text.into_owned();
    }
    marc8(bytes)
}

/// A number written in ASCII digits.
fn digits(bytes: &[u8]) -> Option<usize> {
    std::str::from_utf8(bytes).ok()?.trim().parse().ok()
}

/// An ISO 2709 record: the leader, the directory, and the fields it points
/// to (control fields as written, data fields as indicators and subfields).
pub(crate) fn read_iso2709(
    bytes: &[u8],
    charset: Option<&'static Encoding>,
    unimarc: bool,
) -> Option<Record> {
    let leader = bytes.get(..24)?;
    let base = digits(&leader[12..17])?;
    let directory = bytes.get(24..base.checked_sub(1)?)?;
    let mut control = Vec::new();
    let mut fields = Vec::new();
    for entry in directory.as_chunks::<12>().0 {
        let tag = std::str::from_utf8(&entry[..3]).ok()?.to_string();
        let (length, start) = (digits(&entry[3..7])?, digits(&entry[7..12])?);
        let Some(data) = bytes.get(base + start..base + start + length) else { continue };
        let data = data.strip_suffix(&[FIELD_END]).unwrap_or(data);
        if tag.starts_with("00") {
            control.push((tag, text(data, charset, unimarc)));
            continue;
        }
        let indicator = |at: usize| data.get(at).map_or(' ', |&b| char::from(b));
        let subfields = data
            .get(2..)
            .unwrap_or_default()
            .split(|&b| b == SUBFIELD)
            .skip(1)
            .filter_map(|part| {
                let (&code, value) = part.split_first()?;
                Some((char::from(code), text(value, charset, unimarc)))
            })
            .collect();
        fields.push(Field { tag, ind1: indicator(0), ind2: indicator(1), subfields });
    }
    Some(Record { leader: String::from_utf8_lossy(leader).into_owned(), control, fields })
}

/// A UNIMARC relator's role: the code (070 author, 340 editor, 730
/// translator) or, as Chinese libraries write it, the word (著, 编, 译).
fn unimarc_role(relators: &[&str]) -> CreatorRole {
    let any = |names: &[&str]| relators.iter().any(|r| names.contains(&r.trim()));
    if relators.is_empty() || any(&["070", "072", "440", "著", "撰", "编著", "著者", "撰写"]) {
        CreatorRole::Author
    } else if any(&["340", "651", "编", "主编", "编辑", "合编", "编者"]) {
        CreatorRole::Editor
    } else if any(&["730", "译", "翻译", "合译", "译者"]) {
        CreatorRole::Translator
    } else {
        CreatorRole::Contributor
    }
}

/// People (700–702) and bodies (710–712), as UNIMARC records them: the
/// entry element in `$a`, the rest of a person's name in `$b`.
fn unimarc_creators(record: &Record) -> Vec<Creator> {
    let mut creators = Vec::new();
    for field in &record.fields {
        let body = matches!(field.tag.as_str(), "710" | "711" | "712");
        if !body && !matches!(field.tag.as_str(), "700" | "701" | "702") {
            continue;
        }
        let Some(entry) = field.first('a').map(isbd).filter(|a| !a.is_empty()) else { continue };
        let rest = field.first('b').map(isbd).filter(|b| !b.is_empty());
        let mut creator = match rest {
            _ if body => {
                let mut parts = vec![entry];
                parts.extend(field.all('b').map(isbd));
                Creator::literal(parts.join(". "))
            }
            Some(given) => Creator::person(entry.trim_end_matches(','), given),
            None => Creator::parse(&entry),
        };
        let codes: Vec<&str> = field.all('4').collect();
        creator.role = if field.tag == "702" && codes.is_empty() {
            CreatorRole::Contributor
        } else {
            unimarc_role(&codes)
        };
        if !creator.display_name().is_empty() && !creators.contains(&creator) {
            creators.push(creator);
        }
    }
    creators
}

/// Leader/07, the bibliographic level: an analytic part with a serial
/// host is an article, one in a book a chapter; a dissertation note (328)
/// makes a monograph a thesis.
fn unimarc_type(record: &Record) -> ItemType {
    match record.leader.chars().nth(7).unwrap_or('m') {
        'a' if record.first("011", 'a').is_some() || record.first("461", 'x').is_some() => {
            ItemType::JournalArticle
        }
        'a' => ItemType::BookChapter,
        'm' if record.fields("328").next().is_some() => ItemType::Thesis,
        'm' | 'c' => ItemType::Book,
        _ => ItemType::Other,
    }
}

/// A UNIMARC (or CNMARC) record as a search result.
pub(crate) fn unimarc_hit(catalogue_id: &str, record: &Record) -> Option<Hit> {
    let id = record.control("001").map(str::trim).filter(|id| !id.is_empty())?.to_string();
    let title_field = record.fields("200").next()?;
    let mut title: Vec<String> = title_field.all('a').map(isbd).filter(|a| !a.is_empty()).collect();
    if title.is_empty() {
        return None;
    }
    title.extend(title_field.all('e').map(isbd).filter(|e| !e.is_empty()));
    let mut item = Item::new(unimarc_type(record), title.join(": "));
    for part in ['h', 'i'] {
        if let Some(part) = title_field.first(part).map(isbd).filter(|p| !p.is_empty()) {
            item.title.push_str(". ");
            item.title.push_str(&part);
        }
    }
    item.creators = unimarc_creators(record);
    let imprint = record.fields("214").next().or_else(|| record.fields("210").next());
    if let Some(imprint) = imprint {
        item.place = imprint
            .first('a')
            .map(|p| isbd(p).trim_matches(['[', ']']).to_string())
            .filter(|p| !p.is_empty());
        item.publisher = imprint.first('c').map(isbd).filter(|p| !p.is_empty());
        item.year = imprint.first('d').and_then(first_year);
    }
    if item.year.is_none() {
        // 100$a/09-12: the first date.
        item.year = record.first("100", 'a').and_then(|a| a.get(9..13)).and_then(first_year);
    }
    item.edition = record.first("205", 'a').map(isbd).filter(|e| !e.is_empty());
    let isbns: Vec<&str> = record
        .fields("010")
        .filter_map(|f| f.first('a'))
        .filter_map(|a| a.split_whitespace().next())
        .collect();
    item.isbn =
        isbns.iter().find(|i| i.replace('-', "").len() == 13).or(isbns.first()).map(|i| i.to_string());
    item.issn = record.first("011", 'a').or_else(|| record.first("461", 'x')).map(str::to_string);
    if matches!(item.item_type, ItemType::BookChapter | ItemType::JournalArticle) {
        item.container_title =
            record.first("461", 't').or_else(|| record.first("463", 't')).map(isbd).filter(|t| !t.is_empty());
    }
    item.abstract_note =
        Some(record.fields("330").filter_map(|f| f.first('a')).collect::<Vec<_>>().join(" "))
            .filter(|a| !a.is_empty());
    for keyword in ["606", "610"].into_iter().flat_map(|tag| record.fields(tag)).filter_map(|f| f.first('a'))
    {
        let keyword = isbd(keyword);
        if !keyword.is_empty() && !item.keywords.contains(&keyword) {
            item.keywords.push(keyword);
        }
    }
    item.language = record.first("101", 'a').and_then(language);
    item.url =
        record.fields("856").flat_map(|f| f.all('u')).find(|u| u.starts_with("http")).map(str::to_string);
    Some(Hit { key: format!("{catalogue_id}:{id}"), item, pdf_url: None, cited_by: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ISO 2709 record of these control and data fields (`ab` are the
    /// indicators, then subfields as `$a…`).
    fn iso2709(leader: &str, fields: &[(&str, &[u8])]) -> Vec<u8> {
        let mut directory = Vec::new();
        let mut data = Vec::new();
        for (tag, body) in fields {
            let mut field: Vec<u8> = body.iter().map(|&b| if b == b'$' { SUBFIELD } else { b }).collect();
            field.push(FIELD_END);
            directory.extend(format!("{tag}{:04}{:05}", field.len(), data.len()).into_bytes());
            data.extend(field);
        }
        directory.push(FIELD_END);
        data.push(0x1d);
        let base = 24 + directory.len();
        let total = base + data.len();
        let mut record = format!("{:05}{}{:05}{}", total, &leader[5..12], base, &leader[17..24]).into_bytes();
        record.extend(directory);
        record.extend(data);
        record
    }

    #[test]
    fn reads_marc8() {
        assert_eq!(marc8(b"Caf\xe2e cr\xe8eme"), "Café crëme");
        assert_eq!(marc8(b"\xa1\xf3d\xe2z"), "Łd\u{324}ź");
        assert_eq!(marc8(b"a\x1b(Bb"), "ab", "escapes are skipped");
    }

    #[test]
    fn reads_unimarc_from_chinese_libraries() {
        let leader = "00000nam0 2200000   450 ";
        let record = iso2709(
            leader,
            &[
                ("001", "012345678".as_bytes()),
                ("010", "  $a978-7-04-012345-6$dCNY39.00".as_bytes()),
                ("100", "  $a20050101d2005    em y0chiy50      ea".as_bytes()),
                ("101", "0 $achi".as_bytes()),
                ("200", "1 $a机器学习$9ji qi xue xi$e理论与算法$f周志华著".as_bytes()),
                ("210", "  $a北京$c清华大学出版社$d2016".as_bytes()),
                ("606", "0 $a机器学习$x高等学校$j教材".as_bytes()),
                ("701", " 0$a周志华$4著".as_bytes()),
                ("702", " 0$aSmith$bJohn$4730".as_bytes()),
            ],
        );
        let read = read_iso2709(&record, None, true).unwrap();
        let hit = unimarc_hit("nlc", &read).unwrap();
        assert_eq!(hit.key, "nlc:012345678");
        let item = &hit.item;
        assert_eq!(item.title, "机器学习: 理论与算法");
        assert_eq!(item.item_type, ItemType::Book);
        assert_eq!(
            (item.place.as_deref(), item.publisher.as_deref(), item.year),
            (Some("北京"), Some("清华大学出版社"), Some(2016))
        );
        assert_eq!(item.isbn.as_deref(), Some("978-7-04-012345-6"));
        assert_eq!(item.language.as_deref(), Some("zh"));
        assert_eq!(item.keywords, ["机器学习"]);
        assert_eq!(item.creators[0], Creator::literal("周志华"));
        assert_eq!((item.creators[1].family.as_str(), item.creators[1].given.as_str()), ("Smith", "John"));
        assert_eq!(item.creators[1].role, CreatorRole::Translator);

        // GBK, as some catalogues send it.
        let (gbk, _, _) = encoding_rs::GBK.encode("$a数据库系统");
        let record = iso2709(leader, &[("001", b"7"), ("200", &[b"1 ".as_slice(), gbk.as_ref()].concat())]);
        let read = read_iso2709(&record, None, true).unwrap();
        assert_eq!(unimarc_hit("x", &read).unwrap().item.title, "数据库系统");
    }

    #[test]
    fn reads_marc21_in_marc8() {
        let record =
            iso2709("00000nam a2200000 a 4500", &[("001", b"42"), ("245", b"10$aLe\xe2ecole :$bun roman /")]);
        let read = read_iso2709(&record, None, false).unwrap();
        assert_eq!(read.first("245", 'a'), Some("Leécole :"), "the accent on the letter after it");
        assert_eq!(read.control("001"), Some("42"));
        assert_eq!(read.fields("245").next().unwrap().ind1, '1');
    }
}
