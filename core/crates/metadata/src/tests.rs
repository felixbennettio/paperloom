use std::collections::HashMap;
use std::sync::Mutex;

use paperloom_http::Response;

use super::*;

/// Answers GETs from a table and records the Accept headers.
#[derive(Default)]
struct FakeHttp {
    routes: HashMap<String, Response>,
    accepts: Mutex<Vec<String>>,
}

impl FakeHttp {
    fn with(routes: &[(&str, u16, &str)]) -> Self {
        Self {
            routes: routes
                .iter()
                .map(|(url, status, body)| (url.to_string(), Response::new(*status, *body)))
                .collect(),
            accepts: Mutex::default(),
        }
    }
}

impl Http for FakeHttp {
    fn get(&self, url: &str, accept: &str) -> std::result::Result<Response, NetworkError> {
        self.accepts.lock().unwrap().push(accept.to_string());
        self.routes.get(url).cloned().ok_or_else(|| NetworkError(format!("no route to {url}")))
    }

    fn post_form(&self, url: &str, _form: &[(&str, &str)]) -> std::result::Result<Response, NetworkError> {
        Err(NetworkError(format!("unexpected POST {url}")))
    }
}

fn resolve(http: FakeHttp, input: &str) -> Result<Item> {
    Resolver::with_http(http).resolve(&Identifier::detect(input).expect("an identifier"))
}

const NATURE: &str = r#"{"type":"article-journal","title":"Nanometre-scale thermometry in a living cell",
    "author":[{"given":"G.","family":"Kucsko"}],"container-title":"Nature","issued":{"date-parts":[[2013,7,31]]},
    "volume":"500","issue":"7458","page":"54-58","DOI":"10.1038/NATURE12373"}"#;

#[test]
fn looks_up_a_doi_with_csl_json() {
    let http = FakeHttp::with(&[("https://doi.org/10.1038/nature12373", 200, NATURE)]);
    let item = resolve(http, "https://doi.org/10.1038/nature12373").unwrap();
    assert_eq!(item.title, "Nanometre-scale thermometry in a living cell");
    assert_eq!(item.doi.as_ref().map(|d| d.as_str()), Some("10.1038/nature12373"));
    assert_eq!(item.year, Some(2013));
}

#[test]
fn asks_for_csl_json() {
    let http = FakeHttp::with(&[("https://doi.org/10.1038/nature12373", 200, NATURE)]);
    let resolver = Resolver::with_http(http);
    resolver.resolve(&Identifier::detect("10.1038/nature12373").unwrap()).unwrap();
    assert_eq!(resolver.http.accepts.lock().unwrap()[0], "application/vnd.citationstyles.csl+json");
}

#[test]
fn encodes_unusual_doi_characters() {
    let doi = "10.1002/(sici)1097-4636(199706)35:4<425::aid-jbm3>3.0.co;2-f";
    let url =
        "https://doi.org/10.1002/%28sici%291097-4636%28199706%2935%3A4%3C425%3A%3Aaid-jbm3%3E3.0.co%3B2-f";
    let http = FakeHttp::with(&[(url, 200, r#"{"type":"article-journal","title":"Old SICI paper"}"#)]);
    assert_eq!(resolve(http, doi).unwrap().title, "Old SICI paper");
}

#[test]
fn reports_unknown_dois() {
    let http = FakeHttp::with(&[("https://doi.org/10.1000/missing", 404, "Not found")]);
    assert!(matches!(resolve(http, "10.1000/missing"), Err(Error::NotFound(_))));
    let http = FakeHttp::with(&[("https://doi.org/10.1000/broken", 200, "<html>")]);
    assert!(matches!(resolve(http, "10.1000/broken"), Err(Error::Parse { .. })));
    let http = FakeHttp::with(&[]);
    assert!(matches!(resolve(http, "10.1000/offline"), Err(Error::Network(_))));
}

#[test]
fn looks_up_arxiv_through_datacite() {
    let body = r#"{"type":"article","title":"Attention Is All You Need","publisher":"arXiv",
        "author":[{"family":"Vaswani","given":"Ashish"}],"issued":{"date-parts":[[2017]]},
        "DOI":"10.48550/ARXIV.1706.03762","URL":"https://arxiv.org/abs/1706.03762"}"#;
    let http = FakeHttp::with(&[("https://doi.org/10.48550/arxiv.1706.03762", 200, body)]);
    let item = resolve(http, "arXiv:1706.03762v7").unwrap();
    assert_eq!(item.item_type, ItemType::Preprint);
    assert_eq!(item.arxiv_id.as_deref(), Some("1706.03762"));
    assert_eq!(item.container_title.as_deref(), Some("arXiv"));
    assert_eq!(item.number.as_deref(), Some("arXiv:1706.03762"));
    assert_eq!(item.url.as_deref(), Some("https://arxiv.org/abs/1706.03762"));
}

#[test]
fn looks_up_pubmed() {
    let body = r#"{"header":{},"result":{"uids":["23903748"],"23903748":{
        "uid":"23903748","pubdate":"2013 Aug 1","sortpubdate":"2013/08/01 00:00","source":"Nature",
        "authors":[{"name":"Kucsko G","authtype":"Author"},{"name":"Lukin MD","authtype":"Author"},
                   {"name":"Some Consortium","authtype":"CollectiveName"}],
        "title":"Nanometre-scale thermometry in a living cell.","volume":"500","issue":"7460","pages":"54-8",
        "lang":["eng"],"fulljournalname":"Nature",
        "articleids":[{"idtype":"pubmed","value":"23903748"},{"idtype":"doi","value":"10.1038/nature12373"}]}}}"#;
    let url =
        "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&retmode=json&id=23903748";
    let item = resolve(FakeHttp::with(&[(url, 200, body)]), "PMID: 23903748").unwrap();
    assert_eq!(item.title, "Nanometre-scale thermometry in a living cell");
    assert_eq!(item.creators, vec![Creator::person("Kucsko", "G."), Creator::person("Lukin", "M. D.")]);
    assert_eq!(item.year, Some(2013));
    assert_eq!(item.doi.as_ref().map(|d| d.as_str()), Some("10.1038/nature12373"));
    assert_eq!(item.language.as_deref(), Some("en"));
    assert_eq!(item.pmid.as_deref(), Some("23903748"));

    let missing =
        r#"{"result":{"uids":[],"99999999":{"uid":"99999999","error":"cannot get document summary"}}}"#;
    let url =
        "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&retmode=json&id=99999999";
    assert!(matches!(resolve(FakeHttp::with(&[(url, 200, missing)]), "99999999"), Err(Error::NotFound(_))));
}

#[test]
fn looks_up_isbn() {
    let body = r#"{"ISBN:9780262033848":{"title":"Introduction to Algorithms","subtitle":"Third Edition",
        "authors":[{"name":"Thomas H. Cormen"},{"name":"Charles E. Leiserson"}],
        "publishers":[{"name":"MIT Press"}],"publish_places":[{"name":"Cambridge, Mass"}],
        "publish_date":"July 31, 2009","url":"https://openlibrary.org/books/OL1/x"}}"#;
    let url = "https://openlibrary.org/api/books?format=json&jscmd=data&bibkeys=ISBN:9780262033848";
    let item = resolve(FakeHttp::with(&[(url, 200, body)]), "ISBN 978-0-262-03384-8").unwrap();
    assert_eq!(item.item_type, ItemType::Book);
    assert_eq!(item.title, "Introduction to Algorithms: Third Edition");
    assert_eq!(item.creators[0], Creator::person("Cormen", "Thomas H."));
    assert_eq!(item.publisher.as_deref(), Some("MIT Press"));
    assert_eq!(item.place.as_deref(), Some("Cambridge, Mass"));
    assert_eq!(item.year, Some(2009));

    let url = "https://openlibrary.org/api/books?format=json&jscmd=data&bibkeys=ISBN:0306406152";
    assert!(matches!(resolve(FakeHttp::with(&[(url, 200, "{}")]), "0306406152"), Err(Error::NotFound(_))));
}
