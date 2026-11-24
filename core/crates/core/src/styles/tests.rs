use std::sync::Mutex;

use paperloom_http::{NetworkError, Response};
use serde_json::json;

use super::*;
use crate::NewItem;

const PARENT: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <info>
    <title>Core Test Parent</title>
    <id>http://www.zotero.org/styles/core-test-parent</id>
    <updated>2024-01-01T00:00:00+00:00</updated>
  </info>
  <citation>
    <layout prefix="(" suffix=")" delimiter="; ">
      <text variable="title"/>
    </layout>
  </citation>
  <bibliography>
    <layout>
      <text variable="title"/>
    </layout>
  </bibliography>
</style>"#;

const JOURNAL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" version="1.0">
  <info>
    <title>Core Test Journal</title>
    <id>http://www.zotero.org/styles/core-test-journal</id>
    <link href="http://www.zotero.org/styles/core-test-journal" rel="self"/>
    <link href="http://www.zotero.org/styles/core-test-parent" rel="independent-parent"/>
    <updated>2024-01-01T00:00:00+00:00</updated>
  </info>
</style>"#;

/// The style repository: its list and two styles.
#[derive(Default)]
struct Repository {
    asked: Mutex<Vec<String>>,
}

impl Repository {
    fn asked(&self, url: &str) -> usize {
        self.asked.lock().unwrap().iter().filter(|u| *u == url).count()
    }
}

impl Http for Repository {
    fn get(&self, url: &str, _accept: &str) -> Result<Response, NetworkError> {
        self.asked.lock().unwrap().push(url.to_string());
        let body = if url == INDEX_URL {
            json!([
                { "title": "Core Test Parent", "name": "core-test-parent", "dependent": 0,
                  "categories": { "format": "author-date", "fields": ["generic-base"] } },
                { "title": "Core Test Journal", "titleShort": "CTJ", "name": "core-test-journal", "dependent": 1,
                  "categories": { "format": "author-date", "fields": ["physics"] } },
                { "title": "APA Style 7th edition", "name": "apa", "dependent": 0,
                  "categories": { "format": "author-date", "fields": ["psychology"] } }
            ])
            .to_string()
        } else {
            match url.strip_prefix(STYLE_URL) {
                Some("core-test-parent") => PARENT.to_string(),
                Some("core-test-journal") => JOURNAL.to_string(),
                _ => return Ok(Response::new(404, "")),
            }
        };
        Ok(Response::new(200, body))
    }

    fn post_form(&self, _url: &str, _form: &[(&str, &str)]) -> Result<Response, NetworkError> {
        Err(NetworkError("not used".into()))
    }
}

#[test]
fn searches_downloads_and_removes_styles() {
    let dir = tempfile::tempdir().unwrap();
    let lib = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
    let repository = Repository::default();

    let found = lib.search_styles_through(&repository, "ctj", 10).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].id.as_str(), found[0].dependent, found[0].available),
        ("core-test-journal", true, false)
    );
    assert_eq!(found[0].fields, ["physics"]);
    let found = lib.search_styles_through(&repository, "APA", 10).unwrap();
    assert_eq!((found[0].id.as_str(), found[0].available), ("apa", true));
    let found = lib.search_styles_through(&repository, "core test", 1).unwrap();
    assert_eq!(found[0].id, "core-test-parent", "the shorter title first");
    assert_eq!(repository.asked(INDEX_URL), 1, "the list is kept for a week");

    let added = lib.download_style_through(&repository, "core-test-journal").unwrap();
    assert_eq!(
        added,
        CitationStyle { id: "core-test-journal".into(), name: "Core Test Journal".into(), added: true }
    );
    assert!(citation::has_style("core-test-parent"), "the style it cites like comes too");
    let styles = crate::citation_styles();
    assert!(styles.iter().any(|s| s.id == "core-test-journal" && s.added));
    let item = lib
        .add_item(NewItem { title: "Radioactivity".into(), year: Some(1903), ..NewItem::default() })
        .unwrap();
    let out = lib.format_citations(vec![item.id], "core-test-journal".into(), false).unwrap();
    assert_eq!(out.citation, "(Radioactivity)");

    citation::remove_style("core-test-journal");
    citation::remove_style("core-test-parent");
    load_styles(&dir.path().join("styles"));
    assert!(citation::has_style("core-test-journal"), "kept styles come back when the library opens");

    lib.remove_citation_style("core-test-journal".into()).unwrap();
    assert!(!citation::has_style("core-test-journal"));
    assert!(!dir.path().join("styles/core-test-journal.csl").exists());
    assert!(matches!(lib.download_style_through(&repository, "nope"), Err(CoreError::NotFound { .. })));

    let file = dir.path().join("My Lab Style.csl");
    fs::write(&file, PARENT).unwrap();
    let added = lib.add_citation_style_file(file.display().to_string()).unwrap();
    assert_eq!((added.id.as_str(), added.name.as_str()), ("my-lab-style", "Core Test Parent"));
    lib.remove_citation_style("my-lab-style".into()).unwrap();
    lib.remove_citation_style("core-test-parent".into()).unwrap();
    fs::write(&file, "<style/>").unwrap();
    assert!(matches!(
        lib.add_citation_style_file(file.display().to_string()),
        Err(CoreError::InvalidInput { .. })
    ));
}
