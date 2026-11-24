//! Embedded metadata from a shared page, without running its scripts.

use paperloom_capture::{self as capture, Capture};
use paperloom_http::Http;

use crate::html::{attribute, decode, tags};
use crate::{CoreError, PaperLibrary, SearchPage, search};

const ACCEPT: &str = "text/html,application/xhtml+xml;q=0.9,*/*;q=0.5";
const PAGE_LIMIT: u64 = 8 * 1024 * 1024;

/// Fetches a page and keeps its works like a capture from the browser.
pub(crate) fn read_web_page(
    library: &PaperLibrary,
    http: &dyn Http,
    url: &str,
) -> Result<SearchPage, CoreError> {
    let url = capture::absolute_url("", url).ok_or_else(|| CoreError::InvalidInput {
        message: "The page link must be an HTTP or HTTPS URL.".into(),
    })?;
    let page = http.get_page(&url, ACCEPT, PAGE_LIMIT).map_err(|e| CoreError::Network { message: e.0 })?;
    let response = page.response;
    if !response.is_success() {
        return Err(CoreError::Network { message: format!("Reading the page: HTTP {}.", response.status) });
    }
    let pdf_type = response.content_type.as_deref().is_some_and(|t| {
        t.split(';').next().is_some_and(|t| t.trim().eq_ignore_ascii_case("application/pdf"))
    });
    let head = &response.body.as_bytes()[..response.body.len().min(1024)];
    if pdf_type || head.windows(5).any(|w| w == b"%PDF-") {
        return Err(CoreError::InvalidInput { message: "This link is a PDF file.".into() });
    }
    let base = page.url.as_deref().unwrap_or(&url);
    let captured = capture_html(&response.body, base);
    search::keep_captured(library, capture::read(&captured, &capture::today()))
}

/// What the browser capture collects, without site rules or script execution.
fn capture_html(html: &str, url: &str) -> Capture {
    let mut capture = Capture { url: url.to_string(), ..Capture::default() };
    let mut titled = false;
    for tag in tags(html) {
        if tag.name.eq_ignore_ascii_case("meta") && capture.meta.len() < 2_000 {
            let name = attribute(tag.source, "name")
                .filter(|n| !n.is_empty())
                .or_else(|| attribute(tag.source, "property"))
                .filter(|n| !n.is_empty());
            let content = attribute(tag.source, "content").filter(|v| !v.is_empty());
            if let (Some(name), Some(content)) = (name, content) {
                capture.meta.push((name, content));
            }
        } else if tag.name.eq_ignore_ascii_case("span") && capture.coins.len() < 200 {
            if attribute(tag.source, "class")
                .is_some_and(|classes| classes.split_ascii_whitespace().any(|c| c == "Z3988"))
                && let Some(title) = attribute(tag.source, "title").filter(|v| !v.is_empty())
            {
                capture.coins.push(title);
            }
        } else if tag.name.eq_ignore_ascii_case("script") && capture.json_ld.len() < 20 {
            if attribute(tag.source, "type").is_some_and(|t| t.eq_ignore_ascii_case("application/ld+json"))
                && let Some(text) = tag.text.filter(|t| !t.is_empty() && t.len() < 500_000)
            {
                capture.json_ld.push(text.to_string());
            }
        } else if tag.name.eq_ignore_ascii_case("title") && !titled {
            capture.title = decode(tag.text.unwrap_or_default()).trim().to_string();
            titled = true;
        } else if tag.name.eq_ignore_ascii_case("html") && capture.lang.is_none() {
            capture.lang = attribute(tag.source, "lang");
        }
    }
    capture
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperloom_http::{NetworkError, PageResponse, Response};
    use paperloom_model::ItemType;

    /// A client with just the original HTTP methods.
    struct Web(Response);

    impl Http for Web {
        fn get(&self, url: &str, accept: &str) -> Result<Response, NetworkError> {
            assert_eq!(url, "https://example.org/article");
            assert_eq!(accept, ACCEPT);
            Ok(self.0.clone())
        }

        fn post_form(&self, _url: &str, _form: &[(&str, &str)]) -> Result<Response, NetworkError> {
            panic!("not used")
        }
    }

    #[test]
    fn reads_highwire_and_keeps_the_same_results_as_browser_capture() {
        let library = PaperLibrary::in_memory().unwrap();
        let html = r#"<HTML lang=en><head><title>Publisher</title>
            <META NAME=citation_title CONTENT='Fish &amp; Chips'>
            <meta name='citation_author' content='Curie, Marie'>
            <meta name=citation_author content='Curie, Pierre'>
            <meta name=citation_doi content='10.1000/fish'>
            <meta name=citation_journal_title content='A Journal'>
            <meta name=citation_pdf_url content='/paper.pdf?a=1&amp;b=2'>
            </head></html>"#;
        let web = Web(Response::new(200, html));
        let page = read_web_page(&library, &web, "https://example.org/article").unwrap();
        assert_eq!(page.hits.len(), 1);
        assert_eq!(page.total, Some(1));
        assert!(!page.has_more);
        let hit = &page.hits[0];
        assert_eq!(hit.detail.item_type, "journal-article");
        assert_eq!(hit.detail.title, "Fish & Chips");
        assert_eq!(hit.detail.authors, ["Marie Curie", "Pierre Curie"]);
        assert_eq!(hit.detail.doi.as_deref(), Some("10.1000/fish"));
        assert_eq!(hit.pdf_url.as_deref(), Some("https://example.org/paper.pdf?a=1&b=2"));
        let browser = serde_json::json!({
            "url": "https://example.org/article",
            "title": "Publisher",
            "lang": "en",
            "meta": [
                ["citation_title", "Fish & Chips"],
                ["citation_author", "Curie, Marie"],
                ["citation_author", "Curie, Pierre"],
                ["citation_doi", "10.1000/fish"],
                ["citation_journal_title", "A Journal"],
                ["citation_pdf_url", "/paper.pdf?a=1&b=2"]
            ]
        });
        assert_eq!(page, library.read_captured_page(browser.to_string()).unwrap());
        let report = library.add_search_results(vec![hit.key.clone()], None).unwrap();
        assert_eq!(report.added, 1);
        let stored = library.item_detail(report.added_ids[0].clone()).unwrap().unwrap();
        assert_eq!(stored.title, "Fish & Chips");
        assert_eq!(stored.doi, hit.detail.doi);
        let again = read_web_page(&library, &web, "https://example.org/article").unwrap();
        assert_eq!(again.hits[0].library_item_id.as_deref(), Some(stored.id.as_str()));
    }

    #[test]
    fn reads_a_page_with_only_json_ld() {
        let library = PaperLibrary::in_memory().unwrap();
        let web = Web(Response::new(
            200,
            r#"<script nonce='extra' TYPE='APPLICATION/LD+JSON'>
            {"@context":"https://schema.org","@type":"ScholarlyArticle",
             "headline":"Structured article", "author":{"familyName":"Curie","givenName":"Marie"},
             "identifier":"10.1000/structured"}
            </SCRIPT>"#,
        ));
        let page = read_web_page(&library, &web, "https://example.org/article").unwrap();
        assert_eq!(page.hits.len(), 1);
        assert_eq!(page.hits[0].detail.item_type, "journal-article");
        assert_eq!(page.hits[0].detail.title, "Structured article");
        assert_eq!(page.hits[0].detail.doi.as_deref(), Some("10.1000/structured"));
    }

    #[test]
    fn keeps_a_plain_page_with_its_title_and_language() {
        let library = PaperLibrary::in_memory().unwrap();
        let web = Web(Response::new(200, "<html LANG=fr><TITLE>  A &lt; B &#38; C  </TITLE></html>"));
        let page = read_web_page(&library, &web, "https://example.org/article").unwrap();
        assert_eq!(page.hits.len(), 1);
        let hit = &page.hits[0];
        assert_eq!(hit.detail.item_type, "webpage");
        assert_eq!(hit.detail.title, "A < B & C");
        assert_eq!(hit.detail.language.as_deref(), Some("fr"));
        assert_eq!(hit.detail.url.as_deref(), Some("https://example.org/article"));
        assert_eq!(library.add_search_results(vec![hit.key.clone()], None).unwrap().added, 1);
    }

    #[test]
    fn rejects_non_http_links_before_fetching() {
        let library = PaperLibrary::in_memory().unwrap();
        let web = Web(Response::new(200, "<title>Unused</title>"));
        for url in ["file:///tmp/paper.html", "ftp://example.org/paper", "javascript:alert(1)", "not a URL"] {
            assert!(matches!(read_web_page(&library, &web, url), Err(CoreError::InvalidInput { .. })));
        }
        assert!(matches!(
            library.read_web_page("data:text/html,test".into()),
            Err(CoreError::InvalidInput { .. })
        ));
    }

    #[test]
    fn rejects_pdf_content_type_or_signature() {
        let library = PaperLibrary::in_memory().unwrap();
        for (content_type, body) in [
            (Some("APPLICATION/PDF; charset=binary"), "a PDF without a signature"),
            (Some("application/octet-stream"), "%PDF-1.7 binary"),
            (None, "\u{feff}\n%PDF-1.4"),
        ] {
            let web = Web(Response {
                status: 200,
                content_type: content_type.map(str::to_string),
                body: body.into(),
            });
            let error = read_web_page(&library, &web, "https://example.org/article").unwrap_err();
            assert!(
                matches!(&error, CoreError::InvalidInput { message } if message.contains("PDF file")),
                "{error}"
            );
        }
    }

    #[test]
    fn rejects_failed_and_oversized_responses() {
        let library = PaperLibrary::in_memory().unwrap();
        let missing = Web(Response::new(404, "<title>Not found</title>"));
        assert!(matches!(
            read_web_page(&library, &missing, "https://example.org/article"),
            Err(CoreError::Network { .. })
        ));
        let large = Web(Response::new(200, "x".repeat(8 * 1024 * 1024 + 1)));
        assert!(matches!(
            read_web_page(&library, &large, "https://example.org/article"),
            Err(CoreError::Network { .. })
        ));
    }

    #[test]
    fn uses_the_redirect_address_from_http() {
        struct Redirect(Web);

        impl Http for Redirect {
            fn get(&self, _url: &str, _accept: &str) -> Result<Response, NetworkError> {
                panic!("get_page should be used")
            }

            fn get_page(&self, url: &str, accept: &str, limit: u64) -> Result<PageResponse, NetworkError> {
                assert_eq!(limit, PAGE_LIMIT);
                let mut page = self.0.get_page(url, accept, limit)?;
                page.url = Some("https://publisher.org/articles/123/index.html".into());
                Ok(page)
            }

            fn post_form(&self, _url: &str, _form: &[(&str, &str)]) -> Result<Response, NetworkError> {
                panic!("not used")
            }
        }

        let library = PaperLibrary::in_memory().unwrap();
        let web = Redirect(Web(Response::new(
            200,
            r#"
            <meta name=citation_title content='Redirected paper'>
            <meta name=citation_pdf_url content='../paper.pdf'>"#,
        )));
        let page = read_web_page(&library, &web, "https://example.org/article").unwrap();
        assert_eq!(page.hits[0].detail.url.as_deref(), Some("https://publisher.org/articles/123/index.html"));
        assert_eq!(page.hits[0].pdf_url.as_deref(), Some("https://publisher.org/articles/paper.pdf"));
    }

    #[test]
    fn uses_the_final_page_address_for_relative_pdfs() {
        let page = capture_html(
            r#"<META NAME=citation_title CONTENT='A paper'>
               <meta name=citation_author content='Curie, Marie'>
               <meta name=citation_pdf_url content='../pdf/paper.pdf'>"#,
            "https://publisher.org/articles/123/index.html",
        );
        let found = capture::read(&page, "2026-10-18");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].pdf_url.as_deref(), Some("https://publisher.org/articles/pdf/paper.pdf"));
        assert!(page.site.is_none());
        assert!(page.page.is_none());
        assert!(page.records.is_empty());
    }

    #[test]
    fn decodes_attributes_and_title_but_keeps_json_text() {
        let page = capture_html(
            r#"<HTML LANG='en'><title>  Fish &amp; Chips &#x4E2D;  </title>
               <meta property=description content='&lt;A&gt; &quot;B&quot; &#39;C&#39; &#65;'>
               <span class='extra Z3988 another' title='rft.atitle=COinS&amp;rft.au=Curie'></span>
               <script data-extra='yes' TYPE='APPLICATION/LD+JSON'>{"name":"A &amp; B"}</script>"#,
            "https://example.org/",
        );
        assert_eq!(page.title, "Fish & Chips 中");
        assert_eq!(page.lang.as_deref(), Some("en"));
        assert_eq!(page.meta, [("description".into(), "<A> \"B\" 'C' A".into())]);
        assert_eq!(page.coins, ["rft.atitle=COinS&rft.au=Curie"]);
        assert_eq!(page.json_ld, [r#"{"name":"A &amp; B"}"#]);
        let found = capture::read(&page, "2026-10-18");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].item.item_type, ItemType::JournalArticle);
        assert_eq!(found[0].item.title, "COinS");
    }

    #[test]
    fn ignores_metadata_in_comments_scripts_and_similar_tag_names() {
        let page = capture_html(
            r#"<!-- <meta name=citation_title content=Wrong> -->
               <script>const sample = '<meta name=citation_title content=Wrong>';</script>
               <style>/* <meta name=citation_title content=Wrong> */</style>
               <textarea><meta name=citation_title content=Wrong></textarea>
               <metadata name=citation_title content=Wrong>
               <title>A plain page</title>"#,
            "https://example.org/",
        );
        assert!(page.meta.is_empty());
        assert!(page.json_ld.is_empty());
        assert_eq!(page.title, "A plain page");
    }
}
