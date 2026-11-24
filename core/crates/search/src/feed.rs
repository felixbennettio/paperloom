//! News feeds, as Zotero follows them: a journal's or a preprint server's
//! RSS 2.0, RSS 1.0 (RDF, with Dublin Core and PRISM, as publishers write
//! their tables of contents) or Atom feed, read into works.

use paperloom_model::{Creator, Doi, Item, ItemType, contains_cjk};

use crate::xml;

/// A feed as read: its title and its entries, in the feed's order.
#[derive(Debug, Clone, PartialEq)]
pub struct Feed {
    pub title: String,
    pub entries: Vec<FeedEntry>,
}

/// One entry: the work it announces, where it links, and what the feed
/// calls it, so that reading the feed again finds it again.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedEntry {
    /// The feed's own ID for it (`guid`, `id`, `rdf:about`), else its link
    /// or title.
    pub guid: String,
    pub item: Item,
    pub link: Option<String>,
}

/// The text of an element, its CDATA unwrapped, its markup (HTML in a
/// description) taken out and its entities decoded.
fn plain(body: &str) -> String {
    let body = body.trim();
    let body = body.strip_prefix("<![CDATA[").and_then(|b| b.strip_suffix("]]>")).unwrap_or(body);
    // Escaped HTML (`&lt;p&gt;`) is HTML once decoded.
    let decoded = if body.contains('<') { body.to_string() } else { xml::decode(body) };
    let mut out = String::with_capacity(decoded.len());
    let mut tag: Option<String> = None;
    let mut chars = decoded.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(name) = tag.as_mut() {
            if c == '>' {
                // Words run on across inline markup, not across blocks.
                let name =
                    name.trim_start_matches('/').split_whitespace().next().unwrap_or("").to_ascii_lowercase();
                if !INLINE.contains(&name.as_str()) {
                    out.push(' ');
                }
                tag = None;
            } else {
                name.push(c);
            }
        } else if c == '<' && chars.peek().is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, '/' | '!'))
        {
            tag = Some(String::new());
        } else {
            out.push(c);
        }
    }
    xml::decode(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Markup inside a line of text.
const INLINE: [&str; 11] = ["a", "b", "i", "em", "strong", "sub", "sup", "span", "u", "small", "mml:math"];

/// The first of some elements with text.
fn first(entry: &str, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| xml::elements(entry, name).first().map(|e| plain(e.body)).filter(|t| !t.is_empty()))
}

/// Every element of a name, as text.
fn all(entry: &str, name: &str) -> Vec<String> {
    xml::elements(entry, name).iter().map(|e| plain(e.body)).filter(|t| !t.is_empty()).collect()
}

const MONTHS: [&str; 12] =
    ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/// A feed's date as a year and an ISO date: `2025-06-10T08:00:00Z`, or
/// RSS's `Tue, 10 Jun 2025 08:00:00 GMT`.
fn date(text: &str) -> (Option<i32>, Option<String>) {
    let text = text.trim();
    if text.get(..4).is_some_and(|y| y.bytes().all(|b| b.is_ascii_digit())) {
        let year = text[..4].parse().ok();
        let full = text.get(..10).filter(|d| d.as_bytes().get(4) == Some(&b'-')).map(str::to_string);
        return (year, full.or_else(|| Some(text[..4].to_string())));
    }
    let words: Vec<&str> =
        text.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
    let year = words.iter().find(|w| w.len() == 4 && w.bytes().all(|b| b.is_ascii_digit()));
    let month = words.iter().find_map(|w| {
        let lower = w.to_ascii_lowercase();
        MONTHS.iter().position(|m| lower.starts_with(m))
    });
    let day = words.iter().find(|w| w.len() <= 2 && w.bytes().all(|b| b.is_ascii_digit()));
    let Some(year) = year.and_then(|y| y.parse::<i32>().ok()) else { return (None, None) };
    let full = match (month, day.and_then(|d| d.parse::<u32>().ok())) {
        (Some(m), Some(d)) => format!("{year}-{:02}-{d:02}", m + 1),
        (Some(m), None) => format!("{year}-{:02}", m + 1),
        _ => year.to_string(),
    };
    (Some(year), Some(full))
}

/// Names from what a feed gives: one per element, or several in one
/// (`A. Curie, P. Curie and H. Becquerel`, `张三；李四`). `Curie, Marie`
/// stays one name.
fn names(given: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for text in given {
        for part in text.split([';', '；', '、', '\n']).flat_map(|p| p.split(" and ")) {
            let part = part.trim().trim_start_matches("and ").trim();
            if part.is_empty() {
                continue;
            }
            let pieces: Vec<&str> =
                part.split([',', '，']).map(str::trim).filter(|p| !p.is_empty()).collect();
            let whole = pieces.len() > 1 && pieces.iter().all(|p| p.contains(' ') || contains_cjk(p));
            if whole {
                out.extend(pieces.iter().map(|p| (*p).to_string()));
            } else {
                out.push(part.to_string());
            }
        }
    }
    out
}

/// An Atom entry's link to read it: `rel="alternate"` or no `rel`.
fn atom_link(entry: &str) -> Option<String> {
    let links = xml::elements(entry, "link");
    let alternate =
        links.iter().find(|l| matches!(xml::attr(l.attrs, "rel").as_deref(), None | Some("alternate")));
    alternate.or(links.first()).and_then(|l| xml::attr(l.attrs, "href"))
}

/// One `item` or `entry` as a work.
fn entry(body: &str, attrs: &str, atom: bool) -> Option<FeedEntry> {
    let title = first(body, &["title", "dc:title"])?;
    let link =
        if atom { atom_link(body) } else { first(body, &["link"]) }.or_else(|| xml::attr(attrs, "rdf:about"));
    let guid = first(body, &["guid", "id"])
        .or_else(|| xml::attr(attrs, "rdf:about"))
        .or_else(|| link.clone())
        .unwrap_or_else(|| title.clone());
    let mut item = Item::new(ItemType::JournalArticle, title);
    let mut given = all(body, "dc:creator");
    if given.is_empty() {
        // Atom's `<author><name>`; RSS's `<author>` is an address, with the
        // name in brackets if at all.
        given = xml::elements(body, "author").iter().filter_map(|a| first(a.body, &["name"])).collect();
    }
    if given.is_empty() {
        given = all(body, "author")
            .into_iter()
            .filter_map(|a| match a.split_once('(') {
                Some((_, name)) => Some(name.trim_end_matches(')').trim().to_string()),
                None => (!a.contains('@')).then_some(a),
            })
            .collect();
    }
    item.creators = names(&given).iter().map(String::as_str).map(Creator::parse).collect();
    let when = first(
        body,
        &["prism:coverDate", "prism:publicationDate", "dc:date", "pubDate", "published", "updated"],
    );
    if let Some(when) = when {
        (item.year, item.date) = date(&when);
    }
    item.container_title = first(body, &["prism:publicationName", "dc:source"]);
    item.volume = first(body, &["prism:volume"]);
    item.issue = first(body, &["prism:number"]);
    item.pages = match (first(body, &["prism:startingPage"]), first(body, &["prism:endingPage"])) {
        (Some(start), Some(end)) => Some(format!("{start}–{end}")),
        (start, _) => start,
    };
    item.abstract_note =
        first(body, &["content:encoded", "description", "summary", "content", "dc:description"])
            .filter(|a| a.chars().count() > 20);
    let doi_text =
        [first(body, &["prism:doi"]), first(body, &["dc:identifier"]), link.clone(), Some(guid.clone())];
    item.doi = doi_text.iter().flatten().find_map(|t| Doi::find_in_text(t).into_iter().next());
    if let Some(arxiv) =
        link.as_deref().and_then(|l| l.split_once("arxiv.org/abs/")).map(|(_, id)| id.to_string())
    {
        item.item_type = ItemType::Preprint;
        item.arxiv_id = Some(arxiv.trim_end_matches('/').to_string());
    }
    item.url = link.clone();
    Some(FeedEntry { guid, item, link })
}

/// Reads RSS 2.0, RSS 1.0 or Atom; none when it is none of them.
pub fn read_feed(text: &str) -> Option<Feed> {
    let atom = text.contains("<feed") && !text.contains("<rss") && !text.contains("<rdf:RDF");
    let (kind, head) = if atom {
        ("entry", text.split("<entry").next().unwrap_or(text))
    } else {
        ("item", xml::elements(text, "channel").first().map_or(text, |c| c.body))
    };
    if !atom && !text.contains("<rss") && !text.contains("<rdf:RDF") && !text.contains("<channel") {
        return None;
    }
    let title = first(head.split("<item").next().unwrap_or(head), &["title"]).unwrap_or_default();
    let entries = xml::elements(text, kind).iter().filter_map(|e| entry(e.body, e.attrs, atom)).collect();
    Some(Feed { title, entries })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS2: &str = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:dc="http://purl.org/dc/elements/1.1/">
<channel>
  <title>Nature Physics</title>
  <link>https://www.nature.com/nphys</link>
  <item>
    <title><![CDATA[Quantum sensing with <i>nitrogen</i>-vacancy centres]]></title>
    <link>https://www.nature.com/articles/s41567-025-01234-5</link>
    <description>&lt;p&gt;Nanoscale thermometry inside living cells, with spins in diamond.&lt;/p&gt;</description>
    <pubDate>Tue, 10 Jun 2025 08:00:00 GMT</pubDate>
    <dc:creator>Marie Curie</dc:creator>
    <dc:creator>Pierre Curie</dc:creator>
    <guid isPermaLink="false">s41567-025-01234-5</guid>
  </item>
  <item>
    <title>Second &amp; last</title>
    <author>editor@example.org (Henri Becquerel)</author>
  </item>
</channel>
</rss>"#;

    const RDF: &str = r#"<rdf:RDF xmlns="http://purl.org/rss/1.0/" xmlns:prism="http://prismstandard.org/namespaces/basic/2.0/">
<channel rdf:about="https://journals.example.org/rss"><title>Physical Review Letters</title>
<items><rdf:Seq><rdf:li rdf:resource="https://doi.org/10.1103/PhysRevLett.134.010101"/></rdf:Seq></items></channel>
<item rdf:about="https://doi.org/10.1103/PhysRevLett.134.010101">
  <title>Topological order at room temperature</title>
  <link>https://link.aps.org/doi/10.1103/PhysRevLett.134.010101</link>
  <dc:creator>A. Curie, P. Curie, and H. Becquerel</dc:creator>
  <prism:publicationName>Physical Review Letters</prism:publicationName>
  <prism:volume>134</prism:volume>
  <prism:number>1</prism:number>
  <prism:startingPage>010101</prism:startingPage>
  <prism:doi>10.1103/PhysRevLett.134.010101</prism:doi>
  <dc:date>2025-01-03</dc:date>
</item>
</rdf:RDF>"#;

    const ATOM: &str = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>arXiv cs.CL</title>
<entry><id>oai:arXiv.org:2501.01234v1</id><title>Attention, again</title>
<link rel="alternate" href="http://arxiv.org/abs/2501.01234v1"/>
<author><name>Ashish Vaswani</name></author><author><name>Curie, Marie</name></author>
<published>2025-01-02T18:00:00Z</published><summary>We revisit attention, with care for long inputs.</summary></entry></feed>"#;

    #[test]
    fn reads_rss_feeds() {
        let feed = read_feed(RSS2).unwrap();
        assert_eq!(feed.title, "Nature Physics");
        assert_eq!(feed.entries.len(), 2);
        let first = &feed.entries[0];
        assert_eq!(first.item.title, "Quantum sensing with nitrogen-vacancy centres");
        assert_eq!(first.guid, "s41567-025-01234-5");
        assert_eq!((first.item.year, first.item.date.as_deref()), (Some(2025), Some("2025-06-10")));
        assert_eq!(first.item.creators.len(), 2);
        assert_eq!(
            first.item.abstract_note.as_deref(),
            Some("Nanoscale thermometry inside living cells, with spins in diamond.")
        );
        let second = &feed.entries[1];
        assert_eq!(second.item.title, "Second & last");
        assert_eq!(second.item.creators[0].short_name(), "Becquerel");
        assert_eq!(second.guid, "Second & last", "no guid or link: its title");
        assert_eq!(plain("x &lt; y and <b>bold</b>"), "x < y and bold");
    }

    #[test]
    fn reads_publishers_rdf_feeds() {
        let feed = read_feed(RDF).unwrap();
        assert_eq!(feed.title, "Physical Review Letters");
        let item = &feed.entries[0].item;
        assert_eq!(item.doi.as_ref().map(|d| d.as_str()), Some("10.1103/physrevlett.134.010101"));
        assert_eq!(
            item.creators.iter().map(|c| c.short_name()).collect::<Vec<_>>(),
            ["Curie", "Curie", "Becquerel"]
        );
        assert_eq!(item.container_title.as_deref(), Some("Physical Review Letters"));
        assert_eq!((item.volume.as_deref(), item.issue.as_deref()), (Some("134"), Some("1")));
        assert_eq!(item.date.as_deref(), Some("2025-01-03"));
    }

    #[test]
    fn reads_atom_feeds() {
        let feed = read_feed(ATOM).unwrap();
        assert_eq!(feed.title, "arXiv cs.CL");
        let entry = &feed.entries[0];
        assert_eq!(entry.link.as_deref(), Some("http://arxiv.org/abs/2501.01234v1"));
        assert_eq!(entry.item.arxiv_id.as_deref(), Some("2501.01234v1"));
        assert_eq!(entry.item.item_type, ItemType::Preprint);
        assert_eq!(entry.item.creators.len(), 2, "Curie, Marie is one name");
        assert_eq!(entry.item.year, Some(2025));
        assert!(read_feed("<html><body>Not a feed</body></html>").is_none());
    }
}
