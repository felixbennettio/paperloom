//! Feeds, as Zotero follows them: a journal's table of contents or a
//! preprint server's listing, read now and then, whose entries can be added
//! to the library.

use std::time::Duration;

use paperloom_http::{Http, UreqHttp};
use paperloom_model::{CollectionId, Item};
use paperloom_search::{Feed, read_feed};
use paperloom_store as store;
use serde::{Deserialize, Serialize};

use crate::{CoreError, ItemSummary, PaperLibrary};

/// A feed followed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct FeedInfo {
    pub id: String,
    pub title: String,
    pub url: String,
    pub unread: u32,
    /// Milliseconds since 1970; none before the first reading.
    pub last_checked: Option<i64>,
    /// Why the last reading failed.
    pub problem: Option<String>,
}

/// An entry a feed announced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    pub id: String,
    pub feed_id: String,
    pub title: String,
    /// `Curie et al.`
    pub authors: String,
    pub container_title: Option<String>,
    pub year: Option<i32>,
    pub date: Option<String>,
    pub doi: Option<String>,
    pub abstract_note: Option<String>,
    pub link: Option<String>,
    pub read: bool,
    /// When it was first read from the feed, milliseconds since 1970.
    pub date_added: i64,
    /// The library's item for the same work (by DOI), if it has one.
    pub library_item_id: Option<String>,
}

impl From<store::FeedRow> for FeedInfo {
    fn from(f: store::FeedRow) -> Self {
        Self {
            id: f.id,
            title: f.title,
            url: f.url,
            unread: f.unread,
            last_checked: f.last_checked,
            problem: f.problem,
        }
    }
}

fn http() -> UreqHttp {
    UreqHttp::with_timeout(Duration::from_secs(30))
}

/// The feed an HTML page names in `<link rel="alternate" type="…rss+xml">`
/// (or Atom), made absolute against the page.
fn named_feed(html: &str, page: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find("<link").map(|i| i + from) {
        let end = lower[at..].find('>').map_or(lower.len(), |i| at + i);
        let tag = &lower[at..end];
        from = end;
        if !(tag.contains("rss+xml") || tag.contains("atom+xml")) {
            continue;
        }
        let start = tag.find("href=")? + 5;
        let quote = tag[start..].chars().next()?;
        let value = if quote == '"' || quote == '\'' {
            let body = &html[at + start + 1..end];
            &body[..body.find(quote)?]
        } else {
            let body = &html[at + start..end];
            &body[..body.find(char::is_whitespace).unwrap_or(body.len())]
        };
        let value = value.replace("&amp;", "&");
        if value.starts_with("http") {
            return Some(value);
        }
        let host = page.find("://").map_or(0, |i| i + 3);
        let origin = &page[..page[host..].find('/').map_or(page.len(), |i| host + i)];
        if value.starts_with('/') {
            return Some(format!("{origin}{value}"));
        }
        // Beside the page: its address up to its last `/`.
        let folder =
            page[host..].rfind('/').map_or_else(|| format!("{page}/"), |i| page[..host + i + 1].to_string());
        return Some(format!("{folder}{value}"));
    }
    None
}

/// Reads a feed from its address; a web page that names its feed leads to
/// it. The feed and the address it was read from.
pub(crate) fn fetch(http: &dyn Http, url: &str) -> Result<(Feed, String), String> {
    const ACCEPT: &str = "application/rss+xml, application/atom+xml, application/rdf+xml, application/xml;q=0.9, \
                          text/xml;q=0.9, */*;q=0.8";
    let response = http.get(url, ACCEPT).map_err(|e| e.to_string())?;
    if !response.is_success() {
        return Err(format!("The feed answered {}.", response.status));
    }
    if let Some(feed) = read_feed(&response.body) {
        return Ok((feed, url.to_string()));
    }
    let Some(named) = named_feed(&response.body, url).filter(|n| n != url) else {
        return Err("This address is not a feed.".into());
    };
    let response = http.get(&named, ACCEPT).map_err(|e| e.to_string())?;
    match read_feed(&response.body) {
        Some(feed) if response.is_success() => Ok((feed, named)),
        _ => Err("This address is not a feed.".into()),
    }
}

impl PaperLibrary {
    fn feed_info(&self, id: &str) -> Result<FeedInfo, CoreError> {
        let row = self.lock()?.feed(id)?;
        row.map(FeedInfo::from).ok_or_else(|| CoreError::not_found(format!("feed {id}")))
    }

    /// Reads a feed again and keeps what it announced.
    fn read_feed_with(&self, http: &dyn Http, id: &str) -> Result<FeedInfo, CoreError> {
        let url = self.feed_info(id)?.url;
        match fetch(http, &url) {
            Ok((feed, _)) => {
                let entries: Vec<store::NewFeedEntry<'_>> = feed
                    .entries
                    .iter()
                    .map(|e| store::NewFeedEntry { guid: &e.guid, item: &e.item, link: e.link.as_deref() })
                    .collect();
                self.lock()?.store_feed_entries(id, &entries)?;
            }
            Err(problem) => self.lock()?.set_feed_problem(id, &problem)?,
        }
        self.feed_info(id)
    }

    pub(crate) fn add_feed_with(&self, http: &dyn Http, url: &str) -> Result<FeedInfo, CoreError> {
        let url = url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(CoreError::InvalidInput { message: "A feed's address starts with https://.".into() });
        }
        let (feed, address) = fetch(http, url).map_err(|message| CoreError::Lookup { message })?;
        let title = if feed.title.trim().is_empty() { address.clone() } else { feed.title.clone() };
        let id = self.lock()?.add_feed(&address, &title)?.id;
        let entries: Vec<store::NewFeedEntry<'_>> = feed
            .entries
            .iter()
            .map(|e| store::NewFeedEntry { guid: &e.guid, item: &e.item, link: e.link.as_deref() })
            .collect();
        self.lock()?.store_feed_entries(&id, &entries)?;
        self.feed_info(&id)
    }

    /// A feed entry as the library would add it.
    fn feed_entry(&self, id: &str) -> Result<store::FeedItemRow, CoreError> {
        self.lock()?.feed_item(id)?.ok_or_else(|| CoreError::not_found(format!("feed entry {id}")))
    }

    /// Adds a work to the library as the feed gave it.
    fn add_as_given(&self, mut item: Item, collection_id: Option<String>) -> Result<ItemSummary, CoreError> {
        item.id = paperloom_model::ItemId::new();
        {
            let mut library = self.lock()?;
            library.add_item(&item)?;
            if let Some(collection) = collection_id {
                library.add_to_collection(&CollectionId::from_string(collection), &item.id)?;
            }
        }
        self.reindex_items(std::slice::from_ref(&item.id));
        self.stored_summary(&item)
    }
}

#[uniffi::export]
impl PaperLibrary {
    /// The feeds followed, with how many entries each has unread.
    pub fn feeds(&self) -> Result<Vec<FeedInfo>, CoreError> {
        Ok(self.lock()?.feeds()?.into_iter().map(FeedInfo::from).collect())
    }

    /// Follows a feed (an RSS or Atom address, or a web page that names
    /// its feed) and reads it. Talks to the network: call it off the main
    /// thread.
    pub fn add_feed(&self, url: String) -> Result<FeedInfo, CoreError> {
        self.add_feed_with(&http(), &url)
    }

    /// Reads every feed again; one that fails keeps its entries and notes
    /// why. Talks to the network: call it off the main thread.
    pub fn refresh_feeds(&self) -> Result<Vec<FeedInfo>, CoreError> {
        let http = http();
        let ids: Vec<String> = self.lock()?.feeds()?.into_iter().map(|f| f.id).collect();
        for id in ids {
            self.read_feed_with(&http, &id)?;
        }
        self.feeds()
    }

    pub fn rename_feed(&self, feed_id: String, title: String) -> Result<(), CoreError> {
        if title.trim().is_empty() {
            return Err(CoreError::InvalidInput { message: "Name the feed.".into() });
        }
        Ok(self.lock()?.rename_feed(&feed_id, &title)?)
    }

    /// Stops following a feed; its entries go with it.
    pub fn remove_feed(&self, feed_id: String) -> Result<(), CoreError> {
        Ok(self.lock()?.remove_feed(&feed_id)?)
    }

    /// A feed's entries, or every feed's: the newest reading first.
    pub fn feed_items(&self, feed_id: Option<String>) -> Result<Vec<FeedItem>, CoreError> {
        let library = self.lock()?;
        let rows = library.feed_items(feed_id.as_deref())?;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let in_library = match &row.item.doi {
                Some(doi) => library.find_by_doi(doi)?.map(|i| i.id.to_string()),
                None => None,
            };
            let item = row.item;
            items.push(FeedItem {
                id: row.id,
                feed_id: row.feed_id,
                authors: item.authors_short(),
                title: item.title,
                container_title: item.container_title,
                year: item.year,
                date: item.date,
                doi: item.doi.map(|d| d.to_string()),
                abstract_note: item.abstract_note,
                link: row.link,
                read: row.read,
                date_added: row.date_added,
                library_item_id: in_library,
            });
        }
        Ok(items)
    }

    pub fn set_feed_item_read(&self, item_id: String, read: bool) -> Result<(), CoreError> {
        Ok(self.lock()?.set_feed_item_read(&item_id, read)?)
    }

    /// Marks a feed's entries read, or every feed's.
    pub fn mark_feed_read(&self, feed_id: Option<String>) -> Result<(), CoreError> {
        Ok(self.lock()?.mark_feed_read(feed_id.as_deref())?)
    }

    /// Adds a feed entry to the library (and a collection): its full record
    /// looked up by DOI when there is one, else as the feed gave it; marks
    /// it read. The item, or the one already in the library. Talks to the
    /// network: call it off the main thread.
    pub fn add_feed_item(
        &self,
        item_id: String,
        collection_id: Option<String>,
    ) -> Result<ItemSummary, CoreError> {
        let entry = self.feed_entry(&item_id)?;
        let existing = match &entry.item.doi {
            Some(doi) => self.lock()?.find_by_doi(doi)?,
            None => None,
        };
        let added = if let Some(item) = existing {
            self.stored_summary(&item)?
        } else if let Some(doi) = entry.item.doi.as_ref().map(|d| d.to_string()) {
            match self.add_by_identifier(doi, collection_id.clone()) {
                Ok(summary) => summary,
                // Not found, or offline: what the feed says will do.
                Err(_) => self.add_as_given(entry.item, collection_id)?,
            }
        } else {
            self.add_as_given(entry.item, collection_id)?
        };
        self.lock()?.set_feed_item_read(&item_id, true)?;
        Ok(added)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperloom_http::{NetworkError, Response};

    /// A web where each address has one answer.
    struct Web(Vec<(&'static str, &'static str)>);

    impl Http for Web {
        fn get(&self, url: &str, _accept: &str) -> Result<Response, NetworkError> {
            Ok(self
                .0
                .iter()
                .find(|(address, _)| *address == url)
                .map_or_else(|| Response::new(404, "Not Found"), |(_, body)| Response::new(200, *body)))
        }

        fn post_form(&self, _url: &str, _form: &[(&str, &str)]) -> Result<Response, NetworkError> {
            Err(NetworkError("not used".into()))
        }
    }

    const RSS: &str = r#"<rss version="2.0"><channel><title>Thermometry Letters</title>
        <item><title>Nanoscale thermometry</title><guid>one</guid><link>https://example.org/one</link></item>
        <item><title>Cold atoms</title><guid>two</guid></item></channel></rss>"#;

    #[test]
    fn follows_feeds_found_from_their_pages() {
        let page =
            r#"<html><head><link rel="alternate" type="application/rss+xml" href="/rss.xml"></head></html>"#;
        let web = Web(vec![("https://example.org/journal", page), ("https://example.org/rss.xml", RSS)]);
        let lib = PaperLibrary::in_memory().unwrap();
        let feed = lib.add_feed_with(&web, "https://example.org/journal").unwrap();
        assert_eq!(
            (feed.title.as_str(), feed.url.as_str(), feed.unread),
            ("Thermometry Letters", "https://example.org/rss.xml", 2)
        );
        assert!(lib.add_feed_with(&web, "example.org").is_err(), "an address starts with https://");
        assert!(lib.add_feed_with(&web, "https://example.org/nothing").is_err());

        let items = lib.feed_items(None).unwrap();
        assert_eq!(
            items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(),
            ["Nanoscale thermometry", "Cold atoms"]
        );
        let added = lib.add_feed_item(items[1].id.clone(), None).unwrap();
        assert_eq!(added.title, "Cold atoms");
        assert_eq!(lib.feeds().unwrap()[0].unread, 1, "an added entry is read");

        // A feed that stops answering keeps its entries and says why.
        let refreshed = lib.read_feed_with(&Web(vec![]), &feed.id).unwrap();
        assert_eq!(refreshed.problem.as_deref(), Some("The feed answered 404."));
        assert_eq!(lib.feed_items(Some(feed.id.clone())).unwrap().len(), 2);
        lib.remove_feed(feed.id).unwrap();
        assert!(lib.feeds().unwrap().is_empty());
    }

    #[test]
    fn finds_feeds_named_in_pages() {
        let page = r#"<LINK REL="alternate" TYPE="application/atom+xml" HREF='feed.atom?a=1&amp;b=2'>"#;
        assert_eq!(
            named_feed(page, "https://x.org/j/").as_deref(),
            Some("https://x.org/j/feed.atom?a=1&b=2")
        );
        assert_eq!(named_feed(page, "https://x.org").as_deref(), Some("https://x.org/feed.atom?a=1&b=2"));
        assert_eq!(named_feed("<link rel=stylesheet href=a.css>", "https://x.org"), None);
    }
}
