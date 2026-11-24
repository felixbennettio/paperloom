//! Library catalogues over SRU (Search/Retrieve via URL), the web face of
//! Z39.50: the Library of Congress and K10plus, the union catalogue of
//! German libraries, and any catalogue the person adds. Records come as
//! MARC 21 (MARCXML) or Dublin Core.

use paperloom_http::Http;
use paperloom_model::{Creator, CreatorRole, Doi, Item, ItemType};

use crate::xml;
use crate::{Error, Hit, Page, Query, Result, Searcher, Source, encode};

/// A library catalogue that answers SRU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalogue {
    /// Starts the key of each hit.
    pub id: String,
    pub name: String,
    /// The SRU address, e.g. `https://sru.k10plus.de/opac-de-627`.
    /// Parameters it carries (`?version=1.1`) are kept.
    pub url: String,
    /// The CQL index each word is looked for in (`pica.all`); empty for
    /// the server's own choice.
    pub index: String,
    /// The record schema to ask for (`marcxml`, `MARC21-xml`, `dc`); empty
    /// for `marcxml`.
    pub schema: String,
}

impl Catalogue {
    fn new(id: &str, name: &str, url: &str, index: &str, schema: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            url: url.to_string(),
            index: index.to_string(),
            schema: schema.to_string(),
        }
    }

    /// A built-in source's catalogue.
    pub(crate) fn of(source: Source) -> Self {
        match source {
            Source::K10plus => Self::new(
                "k10plus",
                source.name(),
                "https://sru.k10plus.de/opac-de-627?version=1.1",
                "pica.all",
                "",
            ),
            // LoC's gateway answers plain HTTP only.
            _ => Self::new("loc", source.name(), "http://lx2.loc.gov:210/LCDB?version=1.1", "", ""),
        }
    }
}

/// National libraries whose catalogues answer SRU, to start from when
/// adding one.
pub fn catalogue_presets() -> Vec<Catalogue> {
    vec![
        Catalogue::new(
            "dnb",
            "Deutsche Nationalbibliothek",
            "https://services.dnb.de/sru/dnb?version=1.1",
            "",
            "MARC21-xml",
        ),
        Catalogue::new(
            "bnf",
            "Bibliothèque nationale de France",
            "https://catalogue.bnf.fr/api/SRU?version=1.2",
            "bib.anywhere",
            "dublincore",
        ),
        Catalogue::new(
            "ndl",
            "国立国会図書館 (NDL)",
            "https://ndlsearch.ndl.go.jp/api/sru?version=1.2",
            "anywhere",
            "dc",
        ),
        Catalogue::new(
            "loc-z3950",
            "Library of Congress (Z39.50)",
            "z3950://lx2.loc.gov:210/LCDB",
            "",
            "usmarc",
        ),
        Catalogue::new(
            "swisscovery",
            "swisscovery",
            "https://swisscovery.slsp.ch/view/sru/41SLSP_NETWORK?version=1.2",
            "alma.all_for_ui",
            "marcxml",
        ),
    ]
}

pub(crate) fn search<H: Http>(searcher: &Searcher<H>, catalogue: &Catalogue, query: &Query) -> Result<Page> {
    let cql = cql(&query.text, &catalogue.index);
    if cql.is_empty() {
        return Ok(Page::default());
    }
    let url = address(catalogue, &cql, query)?;
    let response = searcher.http.get(&url, "application/xml")?;
    let failed = |detail: String| Error::Catalogue { name: catalogue.name.clone(), detail };
    match response.status {
        200..=299 => parse(catalogue, &response.body),
        status => Err(failed(diagnostic(&response.body).unwrap_or_else(|| format!("answered {status}")))),
    }
}

/// The catalogue's address with the search in it. Search parameters a
/// pasted address already had give way; others (`version`, a server's
/// own `x-` ones) stay.
fn address(catalogue: &Catalogue, cql: &str, query: &Query) -> Result<String> {
    let invalid =
        || Error::Catalogue { name: catalogue.name.clone(), detail: "not a web address".to_string() };
    let base = url::Url::parse(catalogue.url.trim()).map_err(|_| invalid())?;
    if !matches!(base.scheme(), "http" | "https") {
        return Err(invalid());
    }
    let ours = ["operation", "query", "startRecord", "maximumRecords", "recordSchema"];
    let mut kept: Vec<(String, String)> = base
        .query_pairs()
        .filter(|(k, _)| !ours.contains(&&**k))
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
    if !kept.iter().any(|(k, _)| k == "version") {
        kept.insert(0, ("version".to_string(), "1.2".to_string()));
    }
    let schema = match catalogue.schema.trim() {
        "" => "marcxml",
        schema => schema,
    };
    let mut url = base;
    url.set_query(None);
    url.set_fragment(None);
    let mut params: Vec<String> = kept.iter().map(|(k, v)| format!("{}={}", encode(k), encode(v))).collect();
    params.push("operation=searchRetrieve".to_string());
    params.push(format!("query={}", encode(cql)));
    params.push(format!("startRecord={}", query.offset() + 1));
    params.push(format!("maximumRecords={}", query.per_page));
    params.push(format!("recordSchema={}", encode(schema)));
    Ok(format!("{url}?{}", params.join("&")))
}

/// Every word must appear: `pica.all="deep" and pica.all="learning"`.
fn cql(text: &str, index: &str) -> String {
    let index = index.trim().trim_end_matches('=').trim();
    text.split_whitespace()
        .map(|word| word.replace(['"', '\\'], ""))
        .filter(|word| !word.is_empty())
        .map(|word| if index.is_empty() { format!("\"{word}\"") } else { format!("{index}=\"{word}\"") })
        .collect::<Vec<_>>()
        .join(" and ")
}

/// The message of an SRU diagnostic, when the answer is one.
fn diagnostic(answer: &str) -> Option<String> {
    ["diag:message", "zs:message", "srw:message", "sru:message", "message"]
        .iter()
        .find_map(|name| xml::first_text(answer, name))
}

fn parse(catalogue: &Catalogue, answer: &str) -> Result<Page> {
    let Some(total) = sru_text(answer, "numberOfRecords") else {
        let detail = diagnostic(answer).unwrap_or_else(|| "no record count".to_string());
        return Err(Error::Catalogue { name: catalogue.name.clone(), detail });
    };
    // MARCXML may come with its namespace bound to `marc:`.
    let answer = answer.replace("<marc:", "<").replace("</marc:", "</");
    let Some(prefix) = SRU_PREFIXES.into_iter().find(|p| answer.contains(&format!("<{p}recordData"))) else {
        return Ok(Page { hits: Vec::new(), total: total.parse().ok() });
    };
    let (open, close) = (format!("<{prefix}recordData"), format!("</{prefix}recordData>"));
    let mut hits = Vec::new();
    // Each piece runs from a record's data to the next one's, through the
    // SRU record around it (its identifier and position).
    for piece in answer.split(&open).skip(1) {
        let (Some(start), Some(end)) = (piece.find('>'), piece.find(&close)) else { continue };
        if start >= end {
            continue;
        }
        let data = &piece[start + 1..end];
        // Records packed as a string come escaped.
        let data = if data.contains('<') { data.to_string() } else { xml::decode(data) };
        let around = &piece[end + close.len()..];
        let found = if data.contains("<datafield") || data.contains("<controlfield") {
            xml::elements(&data, "record")
                .first()
                .and_then(|record| hit(catalogue, &Record::parse(record.body)))
        } else {
            let identifier = xml::first_text(around, &format!("{prefix}recordIdentifier"));
            let position = xml::first_text(around, &format!("{prefix}recordPosition"));
            dublin_core(catalogue, &data, identifier, position)
        };
        hits.extend(found);
    }
    Ok(Page { hits, total: total.parse().ok() })
}

const SRU_PREFIXES: [&str; 4] = ["zs:", "srw:", "sru:", ""];

/// The text of an SRU element, whichever prefix the server binds its
/// namespace to.
fn sru_text(answer: &str, local: &str) -> Option<String> {
    SRU_PREFIXES.iter().find_map(|prefix| xml::first_text(answer, &format!("{prefix}{local}")))
}

/// A MARC 21 record: the leader, the control fields (kept as written, since
/// their meaning is by position) and the data fields.
pub(crate) struct Record {
    pub(crate) leader: String,
    pub(crate) control: Vec<(String, String)>,
    pub(crate) fields: Vec<Field>,
}

pub(crate) struct Field {
    pub(crate) tag: String,
    pub(crate) ind1: char,
    pub(crate) ind2: char,
    pub(crate) subfields: Vec<(char, String)>,
}

impl Field {
    pub(crate) fn first(&self, code: char) -> Option<&str> {
        self.all(code).next()
    }

    /// The tag the field stands for: an 880 names it in its `$6`.
    fn kind(&self) -> &str {
        match self.tag.as_str() {
            "880" => self.first('6').and_then(|l| l.get(..3)).unwrap_or_default(),
            tag => tag,
        }
    }

    pub(crate) fn all(&self, code: char) -> impl Iterator<Item = &str> {
        self.subfields
            .iter()
            .filter(move |(c, _)| *c == code)
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.is_empty())
    }
}

impl Record {
    pub(crate) fn parse(body: &str) -> Self {
        let indicator =
            |attrs: &str, name: &str| xml::attr(attrs, name).and_then(|i| i.chars().next()).unwrap_or(' ');
        Record {
            leader: xml::elements(body, "leader").first().map(|e| xml::decode(e.body)).unwrap_or_default(),
            control: xml::elements(body, "controlfield")
                .iter()
                .filter_map(|e| Some((xml::attr(e.attrs, "tag")?, xml::decode(e.body))))
                .collect(),
            fields: xml::elements(body, "datafield")
                .iter()
                .filter_map(|e| {
                    Some(Field {
                        tag: xml::attr(e.attrs, "tag")?,
                        ind1: indicator(e.attrs, "ind1"),
                        ind2: indicator(e.attrs, "ind2"),
                        subfields: xml::elements(e.body, "subfield")
                            .iter()
                            .filter_map(|s| {
                                let code = xml::attr(s.attrs, "code")?.chars().next()?;
                                Some((code, xml::text(s.body)))
                            })
                            .collect(),
                    })
                })
                .collect(),
        }
    }

    /// A control field as written: its meaning is by position.
    pub(crate) fn control(&self, tag: &str) -> Option<&str> {
        self.control.iter().find(|(t, _)| t == tag).map(|(_, v)| v.as_str())
    }

    pub(crate) fn fields<'a>(&'a self, tag: &str) -> impl Iterator<Item = &'a Field> {
        self.fields.iter().filter(move |f| f.tag == tag)
    }

    pub(crate) fn first(&self, tag: &str, code: char) -> Option<&str> {
        self.fields(tag).find_map(|f| f.first(code))
    }

    /// The field as written in its own script, when the record has it in
    /// an 880 alongside the romanised one (Chinese, Japanese, Cyrillic…).
    fn original<'a>(&'a self, field: &'a Field) -> &'a Field {
        let Some(link) = field.first('6').and_then(|l| l.strip_prefix("880-")) else { return field };
        let occurrence = link.get(..2).unwrap_or(link);
        let wanted = format!("{}-{occurrence}", field.tag);
        self.fields("880").find(|f| f.first('6').is_some_and(|l| l.starts_with(&wanted))).unwrap_or(field)
    }
}

pub(crate) fn hit(catalogue: &Catalogue, record: &Record) -> Option<Hit> {
    let id = record.control("001").map(str::trim).filter(|id| !id.is_empty())?.to_string();
    let title_field = record.fields("245").next().map(|f| record.original(f))?;
    let title =
        [title_field.first('a'), title_field.first('b'), title_field.first('n'), title_field.first('p')]
            .into_iter()
            .flatten()
            .map(isbd)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(": ");
    if title.is_empty() {
        return None;
    }
    let mut item = Item::new(item_type(record), title);
    item.creators = creators(record);
    let imprint = record
        .fields("264")
        .find(|f| f.ind2 == '1')
        .or_else(|| record.fields("264").next())
        .or_else(|| record.fields("260").next())
        .map(|f| record.original(f));
    if let Some(imprint) = imprint {
        item.place = imprint
            .first('a')
            .map(|p| isbd(p).trim_matches(['[', ']']).to_string())
            .filter(|p| !p.is_empty());
        item.publisher = imprint.first('b').map(isbd).filter(|p| !p.is_empty());
        item.year = imprint.first('c').and_then(first_year);
    }
    if item.year.is_none() {
        // 008/07-10: the first date.
        item.year = record.control("008").and_then(|f| f.get(7..11)).and_then(first_year);
    }
    item.edition = record.first("250", 'a').map(lead_in).filter(|e| !e.is_empty()).map(str::to_string);
    let isbns: Vec<&str> = record
        .fields("020")
        .filter_map(|f| f.first('a'))
        .filter_map(|a| a.split_whitespace().next())
        .collect();
    item.isbn = isbns.iter().find(|i| i.len() == 13).or(isbns.first()).map(|i| i.to_string());
    item.issn = record.first("022", 'a').or_else(|| record.first("773", 'x')).map(str::to_string);
    item.doi = record
        .fields("024")
        .filter(|f| f.ind1 == '7' && f.first('2').is_some_and(|s| s.eq_ignore_ascii_case("doi")))
        .find_map(|f| f.first('a'))
        .or_else(|| record.fields("856").flat_map(|f| f.all('u')).find(|u| u.contains("doi.org/")))
        .and_then(|doi| Doi::parse(doi).ok());
    if matches!(item.item_type, ItemType::BookChapter | ItemType::JournalArticle) {
        item.container_title = record.first("773", 't').map(isbd).filter(|t| !t.is_empty());
    }
    item.abstract_note =
        Some(record.fields("520").filter_map(|f| f.first('a')).collect::<Vec<_>>().join(" "))
            .filter(|a| !a.is_empty());
    for keyword in
        ["650", "653"].into_iter().flat_map(|tag| record.fields(tag)).filter_map(|f| f.first('a')).map(isbd)
    {
        if !keyword.is_empty() && !item.keywords.contains(&keyword) {
            item.keywords.push(keyword);
        }
    }
    item.language = record
        .first("041", 'a')
        .map(str::to_string)
        .or_else(|| record.control("008").and_then(|f| f.get(35..38)).map(str::to_string))
        .and_then(|code| language(&code));
    item.url = record
        .fields("856")
        .filter(|f| matches!(f.ind2, '0' | '1' | ' '))
        .flat_map(|f| f.all('u'))
        .find(|u| u.starts_with("http"))
        .map(str::to_string)
        .or_else(|| catalogue_link(catalogue, record, &id));
    Some(Hit { key: format!("{}:{id}", catalogue.id), item, pdf_url: None, cited_by: None })
}

/// Leader/07, the bibliographic level, says what the record describes: a
/// part of a serial (or of a host with an ISSN) is an article, a
/// dissertation note (502) makes a book a thesis.
fn item_type(record: &Record) -> ItemType {
    let kind = record.leader.chars().nth(6).unwrap_or('a');
    match record.leader.chars().nth(7).unwrap_or('m') {
        'b' => ItemType::JournalArticle,
        'a' if record.first("773", 'x').is_some() => ItemType::JournalArticle,
        'a' => ItemType::BookChapter,
        'm' if record.fields("502").next().is_some() => ItemType::Thesis,
        'm' | 'c' | 'd' if matches!(kind, 'a' | 't') => ItemType::Book,
        'm' if kind == 'm' => ItemType::Dataset,
        _ => ItemType::Other,
    }
}

/// People (100, 700) and bodies (110, 710), with their role from the
/// relator term or code.
fn creators(record: &Record) -> Vec<Creator> {
    let mut creators = Vec::new();
    for field in record.fields.iter().filter(|f| matches!(f.tag.as_str(), "100" | "110" | "700" | "710")) {
        let field = record.original(field);
        let Some(name) = field.first('a') else { continue };
        let mut creator = if field.kind().ends_with("10") {
            let mut parts = vec![isbd(name)];
            parts.extend(field.all('b').map(isbd));
            Creator::literal(parts.join(". "))
        } else {
            Creator::parse(&person(name))
        };
        let relators: Vec<String> =
            field.all('e').chain(field.all('4')).map(|r| isbd(r).to_lowercase()).collect();
        creator.role = role(&relators);
        if !creator.display_name().is_empty() && !creators.contains(&creator) {
            creators.push(creator);
        }
    }
    creators
}

pub(crate) fn role(relators: &[String]) -> CreatorRole {
    let any = |names: &[&str]| relators.iter().any(|r| names.contains(&r.as_str()));
    if relators.is_empty() || any(&["aut", "author", "verfasserin", "verfasser", "cre", "creator"]) {
        CreatorRole::Author
    } else if any(&["edt", "editor", "hrsg", "herausgeberin", "herausgeber", "eds", "ed"]) {
        CreatorRole::Editor
    } else if any(&["trl", "translator", "übersetzerin", "übersetzer"]) {
        CreatorRole::Translator
    } else {
        CreatorRole::Contributor
    }
}

/// `Smith, John A.` keeps the initial's full stop; `Turing, Alan.` loses
/// the one that closes the field.
fn person(name: &str) -> String {
    let name = name.trim().trim_end_matches([',', ' ']);
    match name.strip_suffix('.') {
        Some(rest) if rest.rsplit([' ', ',', '.']).next().is_some_and(|w| w.chars().count() > 1) => {
            rest.to_string()
        }
        _ => name.to_string(),
    }
}

/// A subfield without the ISBD punctuation that leads into the next one:
/// `2nd ed. /` → `2nd ed.`.
fn lead_in(text: &str) -> &str {
    text.trim().trim_end_matches([' ', '/', ':', ';', '=', ','])
}

/// A subfield without its lead-in punctuation or the full stop that ends
/// the field: `Machine learning :` → `Machine learning`.
pub(crate) fn isbd(text: &str) -> String {
    let text = lead_in(text);
    let text = match text.strip_suffix('.') {
        Some(rest) if !rest.ends_with('.') => rest,
        _ => text,
    };
    text.trim().to_string()
}

/// The first four-digit run: `©2020`, `[1998?]`, `c1987.`.
pub(crate) fn first_year(text: &str) -> Option<i32> {
    let chars: Vec<char> = text.chars().collect();
    chars
        .windows(4)
        .find(|w| w.iter().all(char::is_ascii_digit))
        .and_then(|w| w.iter().collect::<String>().parse().ok())
}

/// MARC's three-letter codes as BCP 47 tags.
pub(crate) fn language(code: &str) -> Option<String> {
    let code = code.trim().to_lowercase();
    let tag = match code.as_str() {
        "" | "und" | "zxx" | "mul" | "|||" => return None,
        "eng" => "en",
        "chi" | "zho" => "zh",
        "jpn" => "ja",
        "kor" => "ko",
        "ger" | "deu" => "de",
        "fre" | "fra" => "fr",
        "spa" => "es",
        "ita" => "it",
        "por" => "pt",
        "rus" => "ru",
        "dut" | "nld" => "nl",
        "lat" => "la",
        "ara" => "ar",
        other => other,
    };
    Some(tag.to_string())
}

/// The record in the library's own catalogue, for the ones whose record
/// pages are known.
fn catalogue_link(catalogue: &Catalogue, record: &Record, id: &str) -> Option<String> {
    let host = url::Url::parse(&catalogue.url).ok()?.host_str()?.to_string();
    if host.ends_with("k10plus.de") {
        Some(format!("https://kxp.k10plus.de/DB=2.1/PPNSET?PPN={id}"))
    } else if host.ends_with("dnb.de") {
        Some(format!("https://d-nb.info/{id}"))
    } else if host.ends_with("loc.gov") {
        record.first("010", 'a').map(|lccn| format!("https://lccn.loc.gov/{}", lccn.replace(' ', "")))
    } else {
        None
    }
}

/// A Dublin Core record: what library catalogues without MARC answer.
fn dublin_core(
    catalogue: &Catalogue,
    data: &str,
    identifier: Option<String>,
    position: Option<String>,
) -> Option<Hit> {
    let all = |name: &str| -> Vec<String> {
        xml::elements(data, &format!("dc:{name}"))
            .iter()
            .map(|e| xml::text(e.body))
            .filter(|t| !t.is_empty())
            .collect()
    };
    // `Title : subtitle / statement of responsibility`
    let title = all("title").into_iter().next()?;
    let title = isbd(title.split(" / ").next().unwrap_or_default()).replace(" : ", ": ");
    if title.is_empty() {
        return None;
    }
    let kinds = all("type").join(" ").to_lowercase();
    let item_type =
        if ["thesis", "thèse", "hochschulschrift", "dissertation"].iter().any(|t| kinds.contains(t)) {
            ItemType::Thesis
        } else if kinds.contains("article") {
            ItemType::JournalArticle
        } else {
            ItemType::Book
        };
    let mut item = Item::new(item_type, title);
    let named = all("creator")
        .into_iter()
        .map(|c| (c, false))
        .chain(all("contributor").into_iter().map(|c| (c, true)));
    for (text, contributor) in named {
        if let Some(creator) = dc_creator(&text, contributor)
            && !item.creators.contains(&creator)
        {
            item.creators.push(creator);
        }
    }
    if let Some(publisher) = all("publisher").into_iter().next() {
        match publisher.split_once(" : ") {
            Some((place, name)) => {
                item.place = Some(isbd(place)).filter(|p| !p.is_empty());
                item.publisher = Some(isbd(name)).filter(|p| !p.is_empty());
            }
            None => item.publisher = Some(isbd(&publisher)),
        }
    }
    item.year = all("date").iter().find_map(|d| first_year(d));
    item.language = all("language").first().and_then(|l| language(l));
    // Each with its kind, when the catalogue says (`xsi:type="tel:ISBN"`).
    let identifiers: Vec<(String, String)> = xml::elements(data, "dc:identifier")
        .iter()
        .map(|e| (xml::attr(e.attrs, "xsi:type").unwrap_or_default().to_uppercase(), xml::text(e.body)))
        .filter(|(_, text)| !text.is_empty())
        .collect();
    for (kind, text) in &identifiers {
        let compact: String =
            text.chars().filter(|c| !matches!(c, '-' | ' ')).collect::<String>().to_uppercase();
        // `ISBN 978…`, or a number of that kind.
        let labelled = |label: &str| {
            compact
                .strip_prefix(label)
                .map(|value| value.trim_start_matches(':').to_string())
                .or_else(|| kind.contains(label).then(|| compact.clone()))
        };
        let isbn13 = compact.len() == 13 && (compact.starts_with("978") || compact.starts_with("979"));
        if let Some(issn) = labelled("ISSN") {
            if item.issn.is_none() {
                item.issn = Some(issn);
            }
        } else if let Some(isbn) = labelled("ISBN").or_else(|| isbn13.then(|| compact.clone()))
            && matches!(isbn.len(), 10 | 13)
            && isbn.chars().all(|c| c.is_ascii_digit() || c == 'X')
        {
            if item.isbn.as_ref().is_none_or(|known| known.len() != 13) {
                item.isbn = Some(isbn);
            }
        } else if item.doi.is_none()
            && text.contains("10.")
            && (text.to_lowercase().contains("doi") || text.starts_with("10."))
        {
            item.doi = Doi::parse(text).ok();
        } else if item.url.is_none() && text.starts_with("http") {
            item.url = Some(text.clone());
        }
    }
    for subject in all("subject") {
        // `650 Management`: a classification number before the words.
        let words = match subject.split_once(' ') {
            Some((code, rest)) if code.chars().all(|c| c.is_ascii_digit() || c == '.') => rest,
            _ => subject.as_str(),
        };
        let keyword = isbd(words);
        if !keyword.is_empty() && !item.keywords.contains(&keyword) {
            item.keywords.push(keyword);
        }
    }
    item.abstract_note = Some(all("description").join(" ")).filter(|a| !a.is_empty());
    let id = identifier
        .or_else(|| identifiers.first().map(|(_, text)| text.clone()))
        .or_else(|| position.map(|p| format!("#{p}")))
        .unwrap_or_else(|| item.title.clone());
    Some(Hit { key: format!("{}:{id}", catalogue.id), item, pdf_url: None, cited_by: None })
}

/// A Dublin Core name with what the catalogue adds to it:
/// `Krämer, Karsten [Verfasser]`, `Qi, Zhixin. Auteur du texte`,
/// `Wang, Hongzhi (19..-.... ; chercheur)`. A name without a comma (an
/// organisation, a name in Chinese or Japanese) stays whole.
fn dc_creator(text: &str, contributor: bool) -> Option<Creator> {
    let (mut name, mut role_text) = (String::new(), String::new());
    let (mut square, mut round) = (false, 0usize);
    for c in text.chars() {
        match c {
            '[' => square = true,
            ']' => square = false,
            '(' => round += 1,
            ')' => round = round.saturating_sub(1),
            _ if square => role_text.push(c),
            _ if round > 0 => {}
            _ => name.push(c),
        }
    }
    let mut name = name.split_whitespace().collect::<Vec<_>>().join(" ").replace(" .", ".");
    if let Some(comma) = name.find(',') {
        // The role after the given name: `. Auteur du texte`.
        let split = name[comma..]
            .match_indices(". ")
            .map(|(at, _)| comma + at)
            .find(|&at| name[..at].rsplit([' ', ',', '.']).next().is_some_and(|w| w.chars().count() > 1));
        if let Some(at) = split {
            role_text = format!("{role_text} {}", &name[at + 2..]);
            name.truncate(at);
        }
    }
    let name = person(&name);
    if name.is_empty() {
        return None;
    }
    let mut creator = if name.contains(',') { Creator::parse(&name) } else { Creator::literal(name) };
    let role_text = role_text.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| role_text.contains(w));
    creator.role = if has(&["verfasser", "auteur", "author"]) {
        CreatorRole::Author
    } else if has(&["herausgeber", "hrsg", "éditeur", "editor"]) {
        CreatorRole::Editor
    } else if has(&["übersetz", "tradu", "translator"]) {
        CreatorRole::Translator
    } else if role_text.trim().is_empty() && !contributor {
        CreatorRole::Author
    } else {
        CreatorRole::Contributor
    };
    Some(creator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_for_every_word() {
        assert_eq!(cql("deep  \"learning\"", "pica.all="), r#"pica.all="deep" and pica.all="learning""#);
        assert_eq!(cql("deep learning", ""), r#""deep" and "learning""#);
        assert_eq!(cql("\"", ""), "");
    }

    #[test]
    fn trims_isbd_punctuation() {
        assert_eq!(isbd("Machine learning :"), "Machine learning");
        assert_eq!(isbd("Sinergetika /"), "Sinergetika");
        assert_eq!(isbd("Springer,"), "Springer");
        assert_eq!(lead_in("2nd ed. /"), "2nd ed.");
        assert_eq!(isbd("Wait..."), "Wait...");
        assert_eq!(person("Smith, John A."), "Smith, John A.");
        assert_eq!(person("Turing, Alan."), "Turing, Alan");
        assert_eq!(person("Nield, Thomas,"), "Nield, Thomas");
    }

    #[test]
    fn reads_years_and_languages() {
        assert_eq!(first_year("©2020"), Some(2020));
        assert_eq!(first_year("[1998?]"), Some(1998));
        assert_eq!(first_year("n.d."), None);
        assert_eq!(language("ger").as_deref(), Some("de"));
        assert_eq!(language("chi").as_deref(), Some("zh"));
        assert_eq!(language("|||"), None);
    }
}
