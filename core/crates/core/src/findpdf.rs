//! Finding an open-access PDF of an item and attaching it, as Zotero's
//! Find Available PDF does: the arXiv copy, the open copies OpenAlex knows,
//! then the PDF the publisher's page names.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::time::Duration;

use paperloom_http::{Http, Request, UreqHttp};
use paperloom_model::{Item, ItemId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::files::{AttachmentInfo, clean_file_name};
use crate::html::{attribute, tags};
use crate::{CoreError, PaperLibrary};

/// A PDF found and attached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct FoundPdf {
    pub attachment: AttachmentInfo,
    /// Where it was fetched from.
    pub url: String,
}

/// Pages that answer programs with a challenge instead of the article.
const CHALLENGED: [&str; 2] = ["ncbi.nlm.nih.gov", "europepmc.org"];

/// How many article pages to read for the PDF they name.
const PAGES: usize = 4;

/// Percent-encodes what a URL path may not hold.
fn encode_path(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// The item's arXiv identifier, from its own field or an arXiv DOI.
fn arxiv_id(item: &Item) -> Option<String> {
    let own =
        item.arxiv_id.as_deref().map(str::trim).filter(|id| !id.is_empty()).map(|id| match id.get(..6) {
            Some(prefix) if prefix.eq_ignore_ascii_case("arxiv:") => id[6..].trim().to_string(),
            _ => id.to_string(),
        });
    own.or_else(|| {
        let doi = item.doi.as_ref()?.as_str().to_ascii_lowercase();
        doi.strip_prefix("10.48550/arxiv.").map(str::to_string)
    })
}

/// The work as OpenAlex knows it, by DOI or PubMed ID.
fn openalex(http: &dyn Http, item: &Item) -> Option<Value> {
    let key = match (&item.doi, item.pmid.as_deref().map(str::trim).filter(|p| !p.is_empty())) {
        (Some(doi), _) => format!("doi:{}", encode_path(doi.as_str())),
        (None, Some(pmid)) => format!("pmid:{}", encode_path(pmid)),
        (None, None) => return None,
    };
    let url = format!("https://api.openalex.org/works/{key}?select=best_oa_location,locations");
    let response = http.get(&url, "application/json").ok().filter(|r| r.is_success())?;
    serde_json::from_str(&response.body).ok()
}

/// The open copies OpenAlex lists: PDFs to fetch, and pages that may name
/// one; the best copy first.
fn open_copies(work: &Value) -> (Vec<String>, Vec<String>) {
    let (mut pdfs, mut pages): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let best = std::iter::once(&work["best_oa_location"]);
    let all = work["locations"].as_array().into_iter().flatten();
    for location in best.chain(all).filter(|l| l["is_oa"].as_bool() == Some(true)) {
        if let Some(pdf) = location["pdf_url"].as_str().filter(|u| u.starts_with("http"))
            && !pdfs.iter().any(|p| p.as_str() == pdf)
        {
            pdfs.push(pdf.to_string());
        }
        if let Some(page) = location["landing_page_url"].as_str().filter(|u| u.starts_with("http"))
            && !pages.iter().any(|p| p.as_str() == page)
        {
            pages.push(page.to_string());
        }
    }
    (pdfs, pages)
}

/// The PDF an article page names in its `citation_pdf_url` tag, made
/// absolute against the page.
fn named_pdf(html: &str, page: &str) -> Option<String> {
    for tag in tags(html).filter(|tag| tag.name.eq_ignore_ascii_case("meta")) {
        let tag = tag.source;
        let named = attribute(tag, "name").or_else(|| attribute(tag, "property"));
        if named.is_some_and(|n| n.eq_ignore_ascii_case("citation_pdf_url"))
            && let Some(url) = attribute(tag, "content").filter(|u| !u.is_empty())
        {
            if url.starts_with("http") {
                return Some(url);
            }
            // A path on the page's own site.
            let host = page.find("://").map_or(0, |i| i + 3);
            let origin = &page[..page[host..].find('/').map_or(page.len(), |i| host + i)];
            return url.starts_with('/').then(|| format!("{origin}{url}"));
        }
    }
    None
}

/// The file at `url` if it is a PDF.
fn fetch_pdf(http: &dyn Http, url: &str) -> Option<Vec<u8>> {
    let request = Request {
        method: "GET",
        url: url.to_string(),
        headers: vec![("Accept".into(), "application/pdf,*/*;q=0.8".into())],
        body: None,
    };
    let response = http.send(request).ok().filter(|r| r.is_success())?;
    let head = &response.body[..response.body.len().min(1024)];
    head.windows(5).any(|w| w == b"%PDF-").then_some(response.body)
}

/// The first open-access PDF of an item and where it came from.
fn find(http: &dyn Http, item: &Item) -> Option<(String, Vec<u8>)> {
    let mut tried = HashSet::new();
    let mut attempt = |url: &str| -> Option<(String, Vec<u8>)> {
        if !tried.insert(url.to_string()) {
            return None;
        }
        fetch_pdf(http, url).map(|pdf| (url.to_string(), pdf))
    };
    if let Some(id) = arxiv_id(item)
        && let Some(found) = attempt(&format!("https://arxiv.org/pdf/{}", encode_path(&id)))
    {
        return Some(found);
    }
    let (pdfs, mut pages) = openalex(http, item).map(|work| open_copies(&work)).unwrap_or_default();
    for url in &pdfs {
        if let Some(found) = attempt(url) {
            return Some(found);
        }
    }
    if let Some(doi) = &item.doi {
        pages.insert(0, format!("https://doi.org/{}", encode_path(doi.as_str())));
    }
    pages.retain(|page| !CHALLENGED.iter().any(|host| page.contains(host)));
    for page in pages.iter().take(PAGES) {
        let Some(html) = http.get(page, "text/html").ok().filter(|r| r.is_success()) else { continue };
        if let Some(url) = named_pdf(&html.body, page)
            && let Some(found) = attempt(&url)
        {
            return Some(found);
        }
    }
    None
}

/// The name the file is kept under: `Curie 1903 - Radioactive Substances.pdf`.
fn file_name(item: &Item) -> String {
    let title: String = item.title.chars().take(80).collect();
    let lead = [Some(item.authors_short()), item.year.map(|y| y.to_string())]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let name = if lead.is_empty() { title } else { format!("{lead} - {title}") };
    clean_file_name(&format!("{}.pdf", name.trim()))
}

impl PaperLibrary {
    pub(crate) fn find_pdf_with(
        &self,
        http: &dyn Http,
        item_id: String,
    ) -> Result<Option<FoundPdf>, CoreError> {
        self.files()?;
        let id = ItemId::from_string(item_id.clone());
        let item = self.lock()?.item(&id)?.ok_or_else(|| CoreError::not_found(format!("item {id}")))?;
        let Some((url, pdf)) = find(http, &item) else { return Ok(None) };
        let folder = tempfile::tempdir().map_err(|e| CoreError::Storage { message: e.to_string() })?;
        let path = folder.path().join(file_name(&item));
        std::fs::write(&path, pdf).map_err(|e| CoreError::Storage { message: e.to_string() })?;
        let attachment = self.attach_file(item_id, path.display().to_string())?;
        Ok(Some(FoundPdf { attachment, url }))
    }
}

#[uniffi::export]
impl PaperLibrary {
    /// Looks for an open-access PDF of an item (its arXiv copy, the open
    /// copies OpenAlex knows, the PDF the publisher's page names) and
    /// attaches the first that really is a PDF; None when there is none.
    /// Talks to the network: call it off the main thread.
    pub fn find_available_pdf(&self, item_id: String) -> Result<Option<FoundPdf>, CoreError> {
        self.find_pdf_with(&UreqHttp::with_timeout(Duration::from_secs(60)), item_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperloom_http::{BytesResponse, NetworkError, Response};

    /// A web where each address has one answer.
    struct Web(Vec<(&'static str, &'static str)>);

    impl Web {
        fn body(&self, url: &str) -> Option<&'static str> {
            self.0.iter().find(|(address, _)| *address == url).map(|(_, body)| *body)
        }
    }

    impl Http for Web {
        fn get(&self, url: &str, _accept: &str) -> Result<Response, NetworkError> {
            Ok(self
                .body(url)
                .map_or_else(|| Response::new(404, "Not Found"), |body| Response::new(200, body)))
        }

        fn post_form(&self, _url: &str, _form: &[(&str, &str)]) -> Result<Response, NetworkError> {
            Err(NetworkError("not used".into()))
        }

        fn send(&self, request: Request) -> Result<BytesResponse, NetworkError> {
            Ok(match self.body(&request.url) {
                Some(body) => BytesResponse { status: 200, body: body.as_bytes().to_vec() },
                None => BytesResponse { status: 404, body: Vec::new() },
            })
        }
    }

    fn item(doi: Option<&str>, arxiv: Option<&str>) -> Item {
        let mut item = Item::new(paperloom_model::ItemType::JournalArticle, "Radioactive Substances");
        item.doi = doi.map(|d| paperloom_model::Doi::parse(d).unwrap());
        item.arxiv_id = arxiv.map(str::to_string);
        item
    }

    #[test]
    fn reads_the_pdf_a_page_names() {
        let html = r#"<head><META content='https://x.org/a.pdf?x=1&amp;y=2' NAME="citation_pdf_url"></head>"#;
        assert_eq!(named_pdf(html, "https://x.org/a").as_deref(), Some("https://x.org/a.pdf?x=1&y=2"));
        let relative = r#"<meta name="citation_pdf_url" content="/doi/pdf/10.1/x">"#;
        assert_eq!(
            named_pdf(relative, "https://pubs.example.org/doi/10.1/x").as_deref(),
            Some("https://pubs.example.org/doi/pdf/10.1/x")
        );
        assert_eq!(named_pdf(r#"<meta name="citation_title" content="x">"#, "https://x.org"), None);
        assert_eq!(attribute(r#"<meta data-name="a" name=b>"#, "name").as_deref(), Some("b"));
    }

    #[test]
    fn knows_arxiv_copies() {
        assert_eq!(arxiv_id(&item(None, Some("arXiv:1706.03762v5"))).as_deref(), Some("1706.03762v5"));
        assert_eq!(arxiv_id(&item(Some("10.48550/arXiv.1706.03762"), None)).as_deref(), Some("1706.03762"));
        assert_eq!(arxiv_id(&item(Some("10.1000/x"), None)), None);
        assert_eq!(encode_path("10.1002/(sici)1;2"), "10.1002/%28sici%291%3B2");
    }

    #[test]
    fn tries_open_copies_then_the_publisher() {
        let work = r#"{"best_oa_location": {"is_oa": true, "pdf_url": null, "landing_page_url": "https://repo.org/1"},
            "locations": [{"is_oa": true, "pdf_url": "https://repo.org/1.pdf", "landing_page_url": "https://repo.org/1"},
                          {"is_oa": false, "pdf_url": "https://closed.org/1.pdf"},
                          {"is_oa": true, "landing_page_url": "https://www.ncbi.nlm.nih.gov/pmc/articles/1"}]}"#;
        let (pdfs, pages) = open_copies(&serde_json::from_str(work).unwrap());
        assert_eq!(pdfs, ["https://repo.org/1.pdf"]);
        assert_eq!(pages, ["https://repo.org/1", "https://www.ncbi.nlm.nih.gov/pmc/articles/1"]);

        // The open copy is not a PDF; the publisher's page names one that is.
        let web = Web(vec![
            ("https://api.openalex.org/works/doi:10.1000/x?select=best_oa_location,locations", work),
            ("https://repo.org/1.pdf", "<html>Sign in</html>"),
            (
                "https://doi.org/10.1000/x",
                r#"<meta name="citation_pdf_url" content="https://pub.org/x.pdf">"#,
            ),
            ("https://pub.org/x.pdf", "%PDF-1.7 the paper"),
        ]);
        let found = find(&web, &item(Some("10.1000/x"), None)).unwrap();
        assert_eq!(found.0, "https://pub.org/x.pdf");
        assert!(find(&Web(Vec::new()), &item(Some("10.1000/x"), None)).is_none());
    }

    #[test]
    fn attaches_what_it_finds() {
        let dir = tempfile::tempdir().unwrap();
        let lib = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
        let added = lib
            .add_item(crate::NewItem {
                title: "Attention Is All You Need".into(),
                authors: vec!["Vaswani, Ashish".into()],
                year: Some(2017),
                ..crate::NewItem::default()
            })
            .unwrap();
        let mut detail = lib.item_detail(added.id.clone()).unwrap().unwrap();
        detail.arxiv_id = Some("1706.03762".into());
        lib.update_item(detail).unwrap();
        let web = Web(vec![("https://arxiv.org/pdf/1706.03762", "%PDF-1.5 attention")]);
        let found = lib.find_pdf_with(&web, added.id.clone()).unwrap().unwrap();
        assert_eq!(found.url, "https://arxiv.org/pdf/1706.03762");
        assert_eq!(found.attachment.title, "Vaswani 2017 - Attention Is All You Need.pdf");
        assert!(found.attachment.is_pdf);
        assert_eq!(lib.attachments(added.id.clone()).unwrap().len(), 1);
        assert!(lib.find_pdf_with(&Web(Vec::new()), added.id).unwrap().is_none());
    }
}
