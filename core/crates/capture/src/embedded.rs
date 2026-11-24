//! What publishers embed in their pages for reference managers: Highwire
//! Press `citation_*` tags (and bepress's), Dublin Core and PRISM tags,
//! COinS spans (OpenURL ContextObjects) and schema.org JSON-LD.

use paperloom_model::{Creator, Doi, Item, ItemType};
use serde_json::Value;

use crate::{clean, join_pages, keywords, parse_date};

/// Meta tags, by lower-cased name; bepress's `bepress_citation_*` read as
/// `citation_*`.
struct Tags(Vec<(String, String)>);

impl Tags {
    fn new(meta: &[(String, String)]) -> Self {
        Self(
            meta.iter()
                .map(|(name, value)| {
                    let name = name.trim().to_ascii_lowercase();
                    let name = name.strip_prefix("bepress_").map(String::from).unwrap_or(name);
                    (name, clean(value))
                })
                .filter(|(_, value)| !value.is_empty())
                .collect(),
        )
    }

    /// The value of the first of `names` the page has.
    fn first(&self, names: &[&str]) -> Option<String> {
        names.iter().find_map(|name| self.0.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone()))
    }

    /// Every value of the first of `names` the page has.
    fn all(&self, names: &[&str]) -> Vec<String> {
        names
            .iter()
            .map(|name| self.0.iter().filter(|(n, _)| n == name).map(|(_, v)| v.clone()).collect::<Vec<_>>())
            .find(|values| !values.is_empty())
            .unwrap_or_default()
    }

    fn has(&self, name: &str) -> bool {
        self.0.iter().any(|(n, _)| n == name)
    }
}

/// The work a page's tags describe, and its PDF.
pub(crate) fn from_meta(meta: &[(String, String)], page_url: &str) -> Option<(Item, Option<String>)> {
    let tags = Tags::new(meta);
    let title = tags.first(&["citation_title", "dc.title", "dcterms.title", "prism.title"])?;
    let scholarly = tags.0.iter().any(|(n, _)| n.starts_with("citation_") || n.starts_with("prism."));
    let mut item = Item::new(item_type(&tags, scholarly), title);
    let mut creators = tags.all(&["citation_author"]);
    if creators.is_empty() {
        creators =
            tags.first(&["citation_authors"]).map(|a| a.split(';').map(clean).collect()).unwrap_or_default();
    }
    if creators.is_empty() {
        creators = tags.all(&["dc.creator", "dcterms.creator"]);
    }
    item.creators = creators.iter().filter(|c| !c.is_empty()).map(|c| Creator::parse(c)).collect();
    let date = tags.first(&[
        "citation_publication_date",
        "citation_date",
        "citation_cover_date",
        "citation_online_date",
        "prism.publicationdate",
        "prism.coverdate",
        "dc.date",
        "dcterms.issued",
        "dcterms.date",
        "dcterms.created",
        "citation_year",
    ]);
    (item.year, item.date) = date.map(|d| parse_date(&d)).unwrap_or_default();
    item.container_title = match item.item_type {
        ItemType::ConferencePaper => tags.first(&["citation_conference_title", "citation_conference"]),
        ItemType::BookChapter => tags.first(&["citation_inbook_title", "citation_book_title"]),
        _ => tags.first(&["citation_journal_title", "prism.publicationname"]),
    };
    item.container_short_title = tags.first(&["citation_journal_abbrev"]);
    item.volume = tags.first(&["citation_volume", "prism.volume"]);
    item.issue = tags.first(&["citation_issue", "prism.number", "prism.issueidentifier"]);
    item.pages = join_pages(
        tags.first(&["citation_firstpage", "prism.startingpage"]),
        tags.first(&["citation_lastpage", "prism.endingpage"]),
    );
    item.doi = tags
        .first(&["citation_doi", "prism.doi"])
        .into_iter()
        .chain(tags.all(&["dc.identifier", "dcterms.identifier"]))
        .find_map(|d| Doi::parse(&d).ok());
    item.issn = tags.first(&["citation_issn", "prism.issn", "prism.eissn"]);
    item.isbn = tags.first(&["citation_isbn"]);
    item.publisher = match item.item_type {
        ItemType::Thesis => tags.first(&["citation_dissertation_institution", "citation_publisher"]),
        ItemType::Report => tags.first(&["citation_technical_report_institution", "citation_publisher"]),
        _ => tags.first(&["citation_publisher", "dc.publisher", "dcterms.publisher", "prism.publisher"]),
    };
    item.number = tags.first(&["citation_technical_report_number", "citation_patent_number"]);
    item.pmid = tags.first(&["citation_pmid"]);
    item.arxiv_id = tags.first(&["citation_arxiv_id"]);
    // Every arXiv preprint has a DataCite DOI, `10.48550/arXiv.<id>`; with it
    // the work is recognised when it is already in the library.
    if item.doi.is_none()
        && let Some(id) = &item.arxiv_id
    {
        let id = id.trim().trim_start_matches("arXiv:");
        let id = id
            .rsplit_once('v')
            .filter(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            .map_or(id, |(base, _)| base);
        item.doi = Doi::parse(&format!("10.48550/arXiv.{id}")).ok();
    }
    item.abstract_note =
        tags.first(&["citation_abstract", "dcterms.abstract", "dc.description", "dcterms.description"]);
    item.keywords = keywords(&tags.all(&["citation_keywords", "dc.subject", "dcterms.subject"]));
    item.language = tags.first(&["citation_language", "dc.language", "dcterms.language"]);
    item.url = tags
        .first(&["citation_abstract_html_url", "citation_fulltext_html_url", "citation_public_url", "og:url"])
        .and_then(|u| absolute(page_url, &u))
        .or_else(|| absolute(page_url, page_url));
    let pdf_url = tags.first(&["citation_pdf_url"]).and_then(|u| absolute(page_url, &u));
    Some((item, pdf_url))
}

fn item_type(tags: &Tags, scholarly: bool) -> ItemType {
    let has = |name: &str| tags.has(name);
    if has("citation_dissertation_institution") || has("citation_dissertation_name") {
        ItemType::Thesis
    } else if has("citation_technical_report_institution") || has("citation_technical_report_number") {
        ItemType::Report
    } else if has("citation_conference_title") || has("citation_conference") {
        ItemType::ConferencePaper
    } else if has("citation_inbook_title") {
        ItemType::BookChapter
    } else if has("citation_patent_number") {
        ItemType::Patent
    } else if has("citation_journal_title") || has("prism.publicationname") {
        ItemType::JournalArticle
    } else if has("citation_arxiv_id") {
        ItemType::Preprint
    } else if has("citation_isbn") || has("citation_book_title") {
        ItemType::Book
    } else if scholarly {
        ItemType::JournalArticle
    } else {
        // Dublin Core alone says little about what a page is.
        match tags.first(&["dc.type", "dcterms.type"]).map(|t| t.to_ascii_lowercase()) {
            Some(t) if t.contains("thesis") || t.contains("dissertation") => ItemType::Thesis,
            Some(t) if t.contains("article") => ItemType::JournalArticle,
            Some(t) if t.contains("book") => ItemType::Book,
            Some(t) if t.contains("report") => ItemType::Report,
            _ => ItemType::Webpage,
        }
    }
}

/// A link on the page as an absolute `http(s)` address.
pub fn absolute(page_url: &str, link: &str) -> Option<String> {
    let base = url::Url::parse(page_url).ok();
    let url = match base {
        Some(base) => base.join(link.trim()).ok()?,
        None => url::Url::parse(link.trim()).ok()?,
    };
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

/// The work a COinS span describes.
pub(crate) fn from_coins(kev: &str) -> Option<Item> {
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(kev.as_bytes())
        .map(|(k, v)| (k.into_owned(), clean(&v)))
        .filter(|(_, v)| !v.is_empty())
        .collect();
    let get = |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    let all = |key: &str| pairs.iter().filter(|(k, _)| k == key).map(|(_, v)| v.clone()).collect::<Vec<_>>();
    let format = get("rft_val_fmt").unwrap_or_default();
    let genre = get("rft.genre").unwrap_or_default().to_ascii_lowercase();
    let item_type = if format.ends_with(":dissertation") {
        ItemType::Thesis
    } else if format.ends_with(":patent") {
        ItemType::Patent
    } else if format.ends_with(":book") {
        match genre.as_str() {
            "bookitem" => ItemType::BookChapter,
            "proceeding" | "conference" => ItemType::ConferencePaper,
            "report" => ItemType::Report,
            _ => ItemType::Book,
        }
    } else {
        match genre.as_str() {
            "proceeding" | "conference" => ItemType::ConferencePaper,
            "preprint" => ItemType::Preprint,
            "book" => ItemType::Book,
            _ => ItemType::JournalArticle,
        }
    };
    let title = get("rft.atitle").or_else(|| get("rft.btitle")).or_else(|| get("rft.title"))?;
    let mut item = Item::new(item_type, title);
    if get("rft.atitle").is_some() {
        item.container_title = get("rft.jtitle").or_else(|| get("rft.btitle")).or_else(|| get("rft.title"));
    }
    let mut creators: Vec<Creator> = all("rft.au").iter().map(|a| Creator::parse(a)).collect();
    if creators.is_empty()
        && let Some(last) = get("rft.aulast")
    {
        let given = get("rft.aufirst").or_else(|| get("rft.auinit")).unwrap_or_default();
        creators.push(Creator::person(last, given));
    }
    item.creators = creators;
    (item.year, item.date) = get("rft.date").map(|d| parse_date(&d)).unwrap_or_default();
    item.volume = get("rft.volume");
    item.issue = get("rft.issue");
    item.pages = get("rft.pages").or_else(|| join_pages(get("rft.spage"), get("rft.epage")));
    item.issn = get("rft.issn").or_else(|| get("rft.eissn"));
    item.isbn = get("rft.isbn");
    item.publisher = get("rft.pub").or_else(|| get("rft.inst"));
    item.place = get("rft.place");
    for id in all("rft_id") {
        if let Some(doi) = id.strip_prefix("info:doi/") {
            item.doi = item.doi.take().or_else(|| Doi::parse(doi).ok());
        } else if let Some(pmid) = id.strip_prefix("info:pmid/") {
            if item.pmid.is_none() {
                item.pmid = Some(pmid.to_string());
            }
        } else if id.starts_with("http") {
            match Doi::parse(&id) {
                Ok(doi) if item.doi.is_none() => item.doi = Some(doi),
                _ if item.url.is_none() => item.url = Some(id),
                _ => {}
            }
        }
    }
    Some(item)
}

/// The scholarly works in a JSON-LD block.
pub(crate) fn from_json_ld(text: &str) -> Vec<Item> {
    let Ok(value) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    let mut nodes = Vec::new();
    gather(&value, &mut nodes);
    nodes.into_iter().filter_map(node_item).collect()
}

/// The nodes of a JSON-LD document: lists and `@graph`s opened up.
fn gather<'a>(value: &'a Value, nodes: &mut Vec<&'a Value>) {
    match value {
        Value::Array(all) => all.iter().for_each(|v| gather(v, nodes)),
        Value::Object(map) => match map.get("@graph") {
            Some(graph) => gather(graph, nodes),
            None => nodes.push(value),
        },
        _ => {}
    }
}

fn types(node: &Value) -> Vec<&str> {
    match &node["@type"] {
        Value::String(t) => vec![t.as_str()],
        Value::Array(all) => all.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

/// Text from a string, a number, `{"@value": …}`, `{"name": …}` or the
/// first of a list.
fn text(value: &Value) -> Option<String> {
    let text = match value {
        Value::String(s) => clean(s),
        Value::Number(n) => n.to_string(),
        Value::Object(map) => return map.get("@value").or_else(|| map.get("name")).and_then(text),
        Value::Array(all) => return all.iter().find_map(text),
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

fn node_item(node: &Value) -> Option<Item> {
    let item_type = types(node).into_iter().find_map(|t| match t {
        "ScholarlyArticle" | "MedicalScholarlyArticle" => Some(ItemType::JournalArticle),
        "Thesis" => Some(ItemType::Thesis),
        "Book" => Some(ItemType::Book),
        "Chapter" => Some(ItemType::BookChapter),
        "Report" => Some(ItemType::Report),
        "Dataset" => Some(ItemType::Dataset),
        _ => None,
    })?;
    let title = text(&node["name"]).or_else(|| text(&node["headline"]))?;
    let mut item = Item::new(item_type, title);
    let authors: Vec<&Value> = match &node["author"] {
        Value::Array(all) => all.iter().collect(),
        Value::Null => Vec::new(),
        one => vec![one],
    };
    item.creators = authors
        .into_iter()
        .filter_map(|author| match (text(&author["familyName"]), text(&author["givenName"])) {
            (Some(family), given) => Some(Creator::person(family, given.unwrap_or_default())),
            _ => text(author).map(|name| Creator::parse(&name)),
        })
        .collect();
    let date = text(&node["datePublished"]).or_else(|| text(&node["dateCreated"]));
    (item.year, item.date) = date.map(|d| parse_date(&d)).unwrap_or_default();
    // isPartOf climbs from issue to volume to the periodical or book.
    let mut part = &node["isPartOf"];
    for _ in 0..3 {
        let kinds = types(part);
        if kinds.contains(&"PublicationIssue") {
            item.issue = item.issue.take().or_else(|| text(&part["issueNumber"]));
        } else if kinds.contains(&"PublicationVolume") {
            item.volume = item.volume.take().or_else(|| text(&part["volumeNumber"]));
        } else if part.is_object() {
            item.container_title = item.container_title.take().or_else(|| text(&part["name"]));
            item.issn = item.issn.take().or_else(|| text(&part["issn"]));
        }
        part = &part["isPartOf"];
    }
    item.pages =
        text(&node["pagination"]).or_else(|| join_pages(text(&node["pageStart"]), text(&node["pageEnd"])));
    item.publisher = text(&node["publisher"]);
    let identifiers = [&node["identifier"], &node["sameAs"], &node["@id"], &node["url"]];
    item.doi = identifiers.iter().find_map(|value| {
        let candidates: Vec<&Value> = match value {
            Value::Array(all) => all.iter().collect(),
            other => vec![*other],
        };
        candidates.into_iter().find_map(|c| {
            let raw = match c {
                Value::Object(map) => map.get("value").and_then(text),
                other => text(other),
            }?;
            Doi::parse(&raw).ok()
        })
    });
    item.url = text(&node["url"]).filter(|u| u.starts_with("http"));
    item.abstract_note = text(&node["abstract"]).or_else(|| text(&node["description"]));
    item.keywords = match &node["keywords"] {
        Value::Array(all) => keywords(&all.iter().filter_map(text).collect::<Vec<_>>()),
        other => keywords(&text(other).into_iter().collect::<Vec<_>>()),
    };
    item.language = text(&node["inLanguage"]);
    item.isbn = text(&node["isbn"]);
    Some(item)
}
