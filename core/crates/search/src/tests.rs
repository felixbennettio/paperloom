use std::collections::HashMap;
use std::sync::Mutex;

use paperloom_http::{BytesResponse, Response};
use paperloom_model::{Creator, ItemType};

use super::*;

/// Answers GETs (and PUTs) from a table, and keeps the headers and body
/// each one came with.
#[derive(Default)]
struct FakeHttp {
    routes: HashMap<String, Response>,
    headers: Mutex<Vec<Vec<(String, String)>>>,
    bodies: Mutex<Vec<Vec<u8>>>,
}

impl FakeHttp {
    fn with(routes: &[(&str, u16, &str)]) -> Self {
        Self {
            routes: routes
                .iter()
                .map(|(url, status, body)| (url.to_string(), Response::new(*status, *body)))
                .collect(),
            headers: Mutex::default(),
            bodies: Mutex::default(),
        }
    }
}

impl Http for FakeHttp {
    fn get(&self, url: &str, _accept: &str) -> std::result::Result<Response, NetworkError> {
        self.routes.get(url).cloned().ok_or_else(|| NetworkError(format!("no route to {url}")))
    }

    fn post_form(&self, url: &str, _form: &[(&str, &str)]) -> std::result::Result<Response, NetworkError> {
        Err(NetworkError(format!("unexpected POST {url}")))
    }

    fn send(&self, request: Request) -> std::result::Result<BytesResponse, NetworkError> {
        assert!(matches!(request.method, "GET" | "PUT"), "{}", request.method);
        self.headers.lock().unwrap().push(request.headers);
        self.bodies.lock().unwrap().push(request.body.unwrap_or_default());
        let response = self.get(&request.url, "")?;
        Ok(BytesResponse { status: response.status, body: response.body.into_bytes() })
    }
}

/// Searches a source that takes a key, giving back the headers sent.
fn search_with_key(
    routes: &[(&str, u16, &str)],
    source: Source,
    text: &str,
    key: &str,
) -> (Result<Page>, Vec<(String, String)>) {
    let query = Query { per_page: 2, key: Some(key.to_string()), ..Query::new(text) };
    let searcher = Searcher::with_http(FakeHttp::with(routes));
    let page = searcher.search(source, &query);
    let headers = searcher.http.headers.lock().unwrap().pop().unwrap_or_default();
    (page, headers)
}

fn search(routes: &[(&str, u16, &str)], source: Source, text: &str) -> Result<Page> {
    let query = Query { per_page: 2, ..Query::new(text) };
    Searcher::with_http(FakeHttp::with(routes)).search(source, &query)
}

const CROSSREF_URL: &str = "https://api.crossref.org/works?query.bibliographic=thermometry+living+cell&rows=2&offset=0&select=DOI,title,subtitle,author,editor,container-title,short-container-title,issued,published-print,published-online,volume,issue,page,publisher,publisher-location,ISSN,ISBN,URL,abstract,type,is-referenced-by-count";

const CROSSREF: &str = r#"{"status":"ok","message":{"total-results":1234,"items":[
  {"DOI":"10.1038/nature12373","type":"journal-article","title":["Nanometre-scale thermometry in a living cell"],
   "author":[{"given":"G.","family":"Kucsko","sequence":"first"},{"given":"P. C.","family":"Maurer"}],
   "container-title":["Nature"],"short-container-title":["Nature"],"issued":{"date-parts":[[2013,7,31]]},
   "volume":"500","issue":"7460","page":"54-58","publisher":"Springer Science and Business Media LLC",
   "ISSN":["0028-0836","1476-4687"],"URL":"https://doi.org/10.1038/nature12373","is-referenced-by-count":1500},
  {"DOI":"10.48550/arxiv.1706.03762","type":"posted-content","title":["Attention Is All You Need"],
   "subtitle":["A study"],"author":[{"given":"Ashish","family":"Vaswani"}],"issued":{"date-parts":[[2017]]}},
  {"type":"journal-article","author":[]}
]}}"#;

/// Crossref turns the whole search away when one selected field is not one
/// it offers (it dropped `language`); these are the ones it named in 2026.
#[test]
fn crossref_selects_only_fields_it_offers() {
    const OFFERED: &[&str] = &[
        "DOI",
        "ISBN",
        "ISSN",
        "URL",
        "abstract",
        "accepted",
        "alternative-id",
        "approved",
        "archive",
        "article-number",
        "assertion",
        "author",
        "chair",
        "clinical-trial-number",
        "container-title",
        "content-created",
        "content-domain",
        "contributor",
        "created",
        "degree",
        "deposited",
        "editor",
        "event",
        "funder",
        "group-title",
        "indexed",
        "is-referenced-by-count",
        "issn-type",
        "issue",
        "issued",
        "license",
        "link",
        "member",
        "original-title",
        "page",
        "posted",
        "prefix",
        "published",
        "published-online",
        "published-print",
        "publisher",
        "publisher-location",
        "reference",
        "references-count",
        "relation",
        "resource",
        "score",
        "short-container-title",
        "short-title",
        "standards-body",
        "subject",
        "subtitle",
        "title",
        "translator",
        "type",
        "update-policy",
        "update-to",
        "updated-by",
        "volume",
    ];
    let select = CROSSREF_URL.split("select=").nth(1).unwrap();
    for field in select.split(',') {
        assert!(OFFERED.contains(&field), "Crossref offers no select {field}");
    }
}

#[test]
fn searches_crossref() {
    let page =
        search(&[(CROSSREF_URL, 200, CROSSREF)], Source::Crossref, "  thermometry living\ncell ").unwrap();
    assert_eq!(page.total, Some(1234));
    assert_eq!(page.hits.len(), 2, "the record without a title is left out");
    let a = &page.hits[0];
    assert_eq!(a.key, "crossref:10.1038/nature12373");
    assert_eq!(a.item.item_type, ItemType::JournalArticle);
    assert_eq!(a.item.creators[1], Creator::person("Maurer", "P. C."));
    assert_eq!(a.item.container_short_title.as_deref(), Some("Nature"));
    assert_eq!(a.item.date.as_deref(), Some("2013-07-31"));
    assert_eq!(a.item.issn.as_deref(), Some("0028-0836"));
    assert_eq!(a.cited_by, Some(1500));
    let b = &page.hits[1];
    assert_eq!(b.item.item_type, ItemType::Preprint);
    assert_eq!(b.item.title, "Attention Is All You Need: A study");
    assert_eq!(b.item.arxiv_id.as_deref(), Some("1706.03762"));
    assert!(page.has_more(&Query { per_page: 2, ..Query::new("x") }));
}

const OPENALEX_URL: &str = "https://api.openalex.org/works?search=attention&per-page=2&page=2";

const OPENALEX: &str = r#"{"meta":{"count":3,"page":2,"per_page":2},"results":[
  {"id":"https://openalex.org/W2963403868","doi":"https://doi.org/10.48550/arxiv.1706.03762",
   "title":"Attention Is All You Need","publication_year":2017,"publication_date":"2017-06-12","type":"preprint",
   "authorships":[{"author":{"display_name":"Ashish Vaswani"}},{"author":{"display_name":"Noam Shazeer"}}],
   "primary_location":{"landing_page_url":"https://arxiv.org/abs/1706.03762","pdf_url":"https://arxiv.org/pdf/1706.03762",
     "source":{"display_name":"arXiv (Cornell University)","type":"repository","issn_l":null}},
   "biblio":{"volume":null,"issue":null,"first_page":null,"last_page":null},
   "abstract_inverted_index":{"The":[0],"dominant":[1],"models":[3],"sequence":[2]},
   "ids":{"openalex":"https://openalex.org/W2963403868"},"cited_by_count":100000,"language":"en",
   "keywords":[{"display_name":"Transformer"}],"best_oa_location":null},
  {"id":"https://openalex.org/W1","title":"Thermometry","type":"article","publication_year":2013,
   "primary_location":{"source":{"display_name":"Nature","type":"journal","issn_l":"0028-0836",
     "host_organization_name":"Nature Portfolio"}},
   "biblio":{"volume":"500","issue":"7460","first_page":"54","last_page":"58"},
   "ids":{"pmid":"https://pubmed.ncbi.nlm.nih.gov/23903748"},"abstract_inverted_index":null}
]}"#;

#[test]
fn searches_openalex() {
    let query = Query { page: 1, per_page: 2, ..Query::new("attention") };
    let page = Searcher::with_http(FakeHttp::with(&[(OPENALEX_URL, 200, OPENALEX)]))
        .search(Source::OpenAlex, &query)
        .unwrap();
    assert_eq!(page.total, Some(3));
    assert!(!page.has_more(&query), "2 + 2 ≥ 3");
    let a = &page.hits[0];
    assert_eq!(a.key, "openalex:W2963403868");
    assert_eq!(a.item.item_type, ItemType::Preprint);
    assert_eq!(a.item.creators[0], Creator::person("Vaswani", "Ashish"));
    assert_eq!(a.item.abstract_note.as_deref(), Some("The dominant sequence models"));
    assert_eq!(a.item.arxiv_id.as_deref(), Some("1706.03762"));
    assert_eq!(a.item.date.as_deref(), Some("2017-06-12"));
    assert_eq!(a.pdf_url.as_deref(), Some("https://arxiv.org/pdf/1706.03762"));
    assert_eq!(a.item.keywords, ["Transformer"]);
    let b = &page.hits[1];
    assert_eq!(b.item.item_type, ItemType::JournalArticle);
    assert_eq!(b.item.pages.as_deref(), Some("54-58"));
    assert_eq!(b.item.pmid.as_deref(), Some("23903748"));
    assert_eq!(b.item.publisher.as_deref(), Some("Nature Portfolio"));
    assert_eq!(b.item.year, Some(2013));
    assert_eq!(b.item.date, None);
}

const ARXIV_URL: &str = "https://export.arxiv.org/api/query?search_query=all%3A%22attention+need%22+OR+%28all%3Aattention+AND+all%3Aneed%29&start=0&max_results=2";

const ARXIV: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title type="html">ArXiv Query: search_query=all:attention AND all:need</title>
  <opensearch:totalResults xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/">42</opensearch:totalResults>
  <entry>
    <id>http://arxiv.org/abs/1706.03762v7</id>
    <updated>2023-08-02T00:41:18Z</updated>
    <published>2017-06-12T17:57:34Z</published>
    <title>Attention Is All
  You Need</title>
    <summary>  The dominant sequence transduction models &amp; more &lt;b&gt; &#233;t&#xE9;.
</summary>
    <author>
      <name>Ashish Vaswani</name>
    </author>
    <author>
      <name>Noam Shazeer</name>
    </author>
    <arxiv:doi xmlns:arxiv="http://arxiv.org/schemas/atom">10.1000/published</arxiv:doi>
    <link href="http://arxiv.org/abs/1706.03762v7" rel="alternate" type="text/html"/>
    <link title="pdf" href="http://arxiv.org/pdf/1706.03762v7" rel="related" type="application/pdf"/>
  </entry>
</feed>
"#;

#[test]
fn searches_arxiv() {
    let page = search(&[(ARXIV_URL, 200, ARXIV)], Source::Arxiv, "attention need").unwrap();
    assert_eq!(page.total, Some(42));
    let a = &page.hits[0];
    assert_eq!(a.key, "arxiv:1706.03762");
    assert_eq!(a.item.title, "Attention Is All You Need");
    assert_eq!(
        a.item.abstract_note.as_deref(),
        Some("The dominant sequence transduction models & more <b> été.")
    );
    assert_eq!(
        a.item.creators,
        vec![Creator::person("Vaswani", "Ashish"), Creator::person("Shazeer", "Noam")]
    );
    assert_eq!(a.item.date.as_deref(), Some("2017-06-12"));
    assert_eq!(a.item.doi.as_ref().map(|d| d.as_str()), Some("10.48550/arxiv.1706.03762"));
    assert_eq!(a.item.arxiv_id.as_deref(), Some("1706.03762"));
    assert_eq!(a.pdf_url.as_deref(), Some("http://arxiv.org/pdf/1706.03762v7"));
}

const ESEARCH_URL: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi?db=pubmed&retmode=json&sort=relevance&term=nv+thermometry&retstart=0&retmax=2";
const ESEARCH: &str = r#"{"esearchresult":{"count":"57","retmax":"2","idlist":["23903748","1"]}}"#;
const ESUMMARY_URL: &str =
    "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&retmode=json&id=23903748,1";
const ESUMMARY: &str = r#"{"result":{"uids":["23903748","1"],
  "23903748":{"uid":"23903748","title":"Nanometre-scale thermometry in a living cell.",
    "authors":[{"name":"Kucsko G","authtype":"Author"}],"source":"Nature","fulljournalname":"Nature",
    "sortpubdate":"2013/08/01 00:00","volume":"500","issue":"7460","pages":"54-8",
    "articleids":[{"idtype":"doi","value":"10.1038/nature12373"}]},
  "1":{"uid":"1","error":"cannot get document summary"}}}"#;

#[test]
fn searches_pubmed() {
    let routes = [(ESEARCH_URL, 200, ESEARCH), (ESUMMARY_URL, 200, ESUMMARY)];
    let page = search(&routes, Source::PubMed, "nv thermometry").unwrap();
    assert_eq!(page.total, Some(57));
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].key, "pubmed:23903748");
    assert_eq!(page.hits[0].item.title, "Nanometre-scale thermometry in a living cell");
}

#[test]
fn an_empty_pubmed_search_asks_once() {
    let empty = r#"{"esearchresult":{"count":"0","idlist":[]}}"#;
    let page = search(&[(ESEARCH_URL, 200, empty)], Source::PubMed, "nv thermometry").unwrap();
    assert_eq!((page.hits.len(), page.total), (0, Some(0)));
}

const S2_URL: &str = "https://api.semanticscholar.org/graph/v1/paper/search?query=attention&offset=0&limit=2&fields=title,authors,year,venue,externalIds,abstract,publicationTypes,journal,publicationDate,openAccessPdf,citationCount,url";

const S2: &str = r#"{"total":2,"offset":0,"data":[
  {"paperId":"204e3073","externalIds":{"ArXiv":"1706.03762","CorpusId":13756489},"title":"Attention is All you Need",
   "venue":"Neural Information Processing Systems","year":2017,"publicationTypes":["JournalArticle","Conference"],
   "publicationDate":"2017-06-12","journal":{"name":"ArXiv","volume":"abs/1706.03762"},
   "authors":[{"authorId":"40348417","name":"Ashish Vaswani"}],"citationCount":120000,
   "openAccessPdf":{"url":"","status":null},"url":"https://www.semanticscholar.org/paper/204e3073"},
  {"paperId":"x","externalIds":{"DOI":"10.1038/nature12373","PubMed":"23903748"},"title":"Thermometry",
   "year":2013,"publicationTypes":null,"journal":{"name":"Nature","volume":"500","pages":" 54-58 "},
   "authors":[],"openAccessPdf":{"url":"https://europepmc.org/x.pdf"}}
]}"#;

#[test]
fn searches_semantic_scholar() {
    let page = search(&[(S2_URL, 200, S2)], Source::SemanticScholar, "attention").unwrap();
    assert_eq!(page.total, Some(2));
    let a = &page.hits[0];
    assert_eq!(a.key, "semantic-scholar:204e3073");
    assert_eq!(a.item.item_type, ItemType::ConferencePaper);
    assert_eq!(a.item.doi.as_ref().map(|d| d.as_str()), Some("10.48550/arxiv.1706.03762"));
    assert_eq!(a.item.arxiv_id.as_deref(), Some("1706.03762"));
    assert_eq!(a.pdf_url, None, "an empty link is no link");
    let b = &page.hits[1];
    assert_eq!(b.item.item_type, ItemType::JournalArticle);
    assert_eq!(b.item.pages.as_deref(), Some("54-58"));
    assert_eq!(b.item.pmid.as_deref(), Some("23903748"));
    assert_eq!(b.pdf_url.as_deref(), Some("https://europepmc.org/x.pdf"));
}

#[test]
fn busy_services_say_so() {
    let err = search(&[(S2_URL, 429, "")], Source::SemanticScholar, "attention").unwrap_err();
    assert!(matches!(err, Error::RateLimited("Semantic Scholar")), "{err}");
    let err = search(&[(S2_URL, 500, "")], Source::SemanticScholar, "attention").unwrap_err();
    assert!(matches!(err, Error::Http { status: 500, .. }), "{err}");
}

#[test]
fn blank_queries_find_nothing_without_asking() {
    let page = search(&[], Source::Crossref, "   ").unwrap();
    assert!(page.hits.is_empty());
    assert!(!page.has_more(&Query::new("")));
}

const WOS_URL: &str = "https://api.clarivate.com/apis/wos-starter/v1/documents?db=WOS&q=TS%3D%28nv+thermometry%29&limit=2&page=1";

const WOS: &str = r#"{"metadata":{"total":31,"page":1,"limit":2},"hits":[
  {"uid":"WOS:000322613500041","title":"Nanometre-scale thermometry in a living cell","types":["Article"],
   "source":{"sourceTitle":"NATURE","publishYear":2013,"publishMonth":"AUG 1","volume":"500","issue":"7460",
     "pages":{"range":"54-58","begin":"54","end":"58","count":5}},
   "names":{"authors":[{"displayName":"Kucsko, G.","wosStandard":"Kucsko, G"},{"displayName":"Maurer, P. C."}]},
   "links":{"record":"https://www.webofscience.com/wos/woscc/full-record/WOS:000322613500041"},
   "citations":[{"db":"WOS","count":1890}],
   "identifiers":{"doi":"10.1038/nature12373","issn":"0028-0836","pmid":"MEDLINE:23903748"},
   "keywords":{"authorKeywords":["NV centre"]}},
  {"uid":"WOS:2","title":"Proceedings only","types":["Proceedings Paper"],"source":{"publishYear":"2019"},
   "names":{"authors":[]},"links":{"record":"https://www.webofscience.com/wos/woscc/full-record/WOS:2"}}
]}"#;

#[test]
fn searches_web_of_science() {
    let (page, headers) =
        search_with_key(&[(WOS_URL, 200, WOS)], Source::WebOfScience, "nv thermometry", "k1");
    let page = page.unwrap();
    assert!(headers.contains(&("X-ApiKey".into(), "k1".into())));
    assert_eq!(page.total, Some(31));
    let a = &page.hits[0];
    assert_eq!(a.key, "wos:WOS:000322613500041");
    assert_eq!(a.item.item_type, ItemType::JournalArticle);
    assert_eq!(a.item.creators[1], Creator::person("Maurer", "P. C."));
    assert_eq!(a.item.year, Some(2013));
    assert_eq!(a.item.pages.as_deref(), Some("54-58"));
    assert_eq!(a.item.pmid.as_deref(), Some("23903748"));
    assert_eq!(a.item.url.as_deref(), Some("https://doi.org/10.1038/nature12373"));
    assert_eq!(a.cited_by, Some(1890));
    let b = &page.hits[1];
    assert_eq!(b.item.item_type, ItemType::ConferencePaper);
    assert_eq!(b.item.year, Some(2019));
    assert_eq!(b.item.url.as_deref(), Some("https://www.webofscience.com/wos/woscc/full-record/WOS:2"));
}

const SCOPUS_URL: &str =
    "https://api.elsevier.com/content/search/scopus?query=TITLE-ABS-KEY%28nv+thermometry%29&start=2&count=2";

const SCOPUS: &str = r#"{"search-results":{"opensearch:totalResults":"57","opensearch:startIndex":"2","entry":[
  {"@_fa":"true","link":[{"@ref":"scopus","@href":"https://www.scopus.com/inward/record.uri?eid=2-s2.0-84881"}],
   "dc:identifier":"SCOPUS_ID:84881","eid":"2-s2.0-84881","dc:title":"Nanometre-scale thermometry in a living cell",
   "dc:creator":"Kucsko G.","prism:publicationName":"Nature","prism:issn":"00280836","prism:volume":"500",
   "prism:issueIdentifier":"7460","prism:pageRange":"54-58","prism:coverDate":"2013-08-01",
   "prism:doi":"10.1038/nature12373","citedby-count":"1650","pubmed-id":"23903748",
   "prism:aggregationType":"Journal","subtype":"ar","subtypeDescription":"Article"},
  {"@_fa":"true","eid":"2-s2.0-1","dc:title":"Spin <inf>1/2</inf> sensing","prism:aggregationType":"Conference Proceeding",
   "subtype":"cp","prism:coverDate":"2020-01-01","citedby-count":"0",
   "author":[{"authname":"Lee J.","surname":"Lee","given-name":"Jin"},{"authname":"Kim S.-H."}],
   "dc:description":"An abstract.","authkeywords":"spin | sensing"}
]}}"#;

#[test]
fn searches_scopus() {
    let query = Query { page: 1, per_page: 2, key: Some("k2".into()), ..Query::new("nv thermometry") };
    let searcher = Searcher::with_http(FakeHttp::with(&[(SCOPUS_URL, 200, SCOPUS)]));
    let page = searcher.search(Source::Scopus, &query).unwrap();
    assert!(searcher.http.headers.lock().unwrap()[0].contains(&("X-ELS-APIKey".into(), "k2".into())));
    assert_eq!(page.total, Some(57));
    let a = &page.hits[0];
    assert_eq!(a.key, "scopus:2-s2.0-84881");
    assert_eq!(a.item.creators, vec![Creator::person("Kucsko", "G.")]);
    assert_eq!(a.item.issn.as_deref(), Some("0028-0836"));
    assert_eq!(a.item.date.as_deref(), Some("2013-08-01"));
    assert_eq!(a.item.pmid.as_deref(), Some("23903748"));
    assert_eq!(a.cited_by, Some(1650));
    let b = &page.hits[1];
    assert_eq!(b.item.item_type, ItemType::ConferencePaper);
    assert_eq!(b.item.title, "Spin 1/2 sensing");
    assert_eq!(b.item.creators, vec![Creator::person("Lee", "Jin"), Creator::person("Kim", "S.-H.")]);
    assert_eq!(b.item.keywords, ["spin", "sensing"]);
    assert_eq!(b.item.url.as_deref(), None);
}

#[test]
fn an_empty_scopus_search_finds_nothing() {
    let url = "https://api.elsevier.com/content/search/scopus?query=TITLE-ABS-KEY%28zzz%29&start=0&count=2";
    let empty = r#"{"search-results":{"opensearch:totalResults":"0","entry":[{"@_fa":"true","error":"Result set was empty"}]}}"#;
    let (page, _) = search_with_key(&[(url, 200, empty)], Source::Scopus, "zzz", "k");
    let page = page.unwrap();
    assert_eq!((page.hits.len(), page.total), (0, Some(0)));
}

const IEEE_URL: &str = "https://ieeexploreapi.ieee.org/api/v1/search/articles?querytext=quantum+sensing&max_records=2&start_record=1&format=json&apikey=k3";

const IEEE: &str = r#"{"total_records":120,"total_searched":5000000,"articles":[
  {"doi":"10.1109/JSEN.2020.1","title":"Quantum <i>sensing</i> &amp; more","publisher":"IEEE","issn":"1530-437X",
   "authors":{"authors":[{"author_order":1,"full_name":"Ada Lovelace"},{"author_order":2,"full_name":"Alan Turing"}]},
   "access_type":"OPEN_ACCESS","content_type":"Journals","abstract":"Sensors <b>sense</b>.","article_number":"9000001",
   "pdf_url":"https://ieeexplore.ieee.org/stamp/stamp.jsp?arnumber=9000001","publication_title":"IEEE Sensors Journal",
   "publication_year":2020,"volume":"20","issue":"5","start_page":"100","end_page":"110","citing_paper_count":7,
   "index_terms":{"ieee_terms":{"terms":["Sensors","Quantum sensing"]},"author_terms":{"terms":["quantum sensing"]}}},
  {"title":"Standard for things","content_type":"Standards","article_number":"42","access_type":"LOCKED",
   "pdf_url":"https://ieeexplore.ieee.org/stamp/stamp.jsp?arnumber=42","publication_year":"2019"}
]}"#;

#[test]
fn searches_ieee_xplore() {
    let (page, _) = search_with_key(&[(IEEE_URL, 200, IEEE)], Source::Ieee, "quantum sensing", "k3");
    let page = page.unwrap();
    assert_eq!(page.total, Some(120));
    let a = &page.hits[0];
    assert_eq!(a.key, "ieee:9000001");
    assert_eq!(a.item.title, "Quantum sensing & more");
    assert_eq!(a.item.creators[1], Creator::person("Turing", "Alan"));
    assert_eq!(a.item.pages.as_deref(), Some("100-110"));
    assert_eq!(a.item.abstract_note.as_deref(), Some("Sensors sense."));
    assert_eq!(a.item.keywords, ["quantum sensing", "Sensors"]);
    assert_eq!(a.pdf_url.as_deref(), Some("https://ieeexplore.ieee.org/stamp/stamp.jsp?arnumber=9000001"));
    assert_eq!(a.cited_by, Some(7));
    let b = &page.hits[1];
    assert_eq!(b.item.item_type, ItemType::Standard);
    assert_eq!(b.item.year, Some(2019));
    assert_eq!(b.item.url.as_deref(), Some("https://ieeexplore.ieee.org/document/42"));
    assert_eq!(b.pdf_url, None, "a locked PDF is not offered");
}

const SPRINGER_URL: &str = "https://api.springernature.com/meta/v2/json?q=nv+centre&s=1&p=2&api_key=k4";

const SPRINGER: &str = r#"{"apiMessage":"x","result":[{"total":"340","start":"1","pageLength":"2"}],"records":[
  {"contentType":"Article","identifier":"doi:10.1007/s1","language":"en","title":"NV centres in diamond",
   "url":[{"format":"","value":"http://link.springer.com/10.1007/s1"},{"format":"pdf","value":"http://link.springer.com/content/pdf/10.1007/s1.pdf"}],
   "creators":[{"creator":"Doherty, Marcus W."}],"publicationName":"Physics Reports","openaccess":"true",
   "doi":"10.1007/s1","publisher":"Springer","publicationDate":"2013-07-01","issn":"0370-1573","volume":"528",
   "number":"1","startingPage":"1","endingPage":"45","abstract":{"h1":"Abstract","p":["One.","Two."]},
   "keyword":["NV centre","Diamond"]},
  {"contentType":"Chapter","genre":["ConferencePaper"],"identifier":"doi:10.1007/c2","title":"A talk",
   "doi":"10.1007/c2","openaccess":"false","abstract":"<p>Spoken.</p>","printIsbn":"978-3-030-00000-0",
   "url":[{"format":"pdf","value":"http://link.springer.com/content/pdf/10.1007/c2.pdf"}]}
]}"#;

#[test]
fn searches_springer_nature() {
    let (page, _) = search_with_key(&[(SPRINGER_URL, 200, SPRINGER)], Source::Springer, "nv centre", "k4");
    let page = page.unwrap();
    assert_eq!(page.total, Some(340));
    let a = &page.hits[0];
    assert_eq!(a.key, "springer:10.1007/s1");
    assert_eq!(a.item.creators, vec![Creator::person("Doherty", "Marcus W.")]);
    assert_eq!(a.item.pages.as_deref(), Some("1-45"));
    assert_eq!(a.item.abstract_note.as_deref(), Some("One. Two."));
    assert_eq!(a.pdf_url.as_deref(), Some("http://link.springer.com/content/pdf/10.1007/s1.pdf"));
    let b = &page.hits[1];
    assert_eq!(b.item.item_type, ItemType::ConferencePaper);
    assert_eq!(b.item.abstract_note.as_deref(), Some("Spoken."));
    assert_eq!(b.item.isbn.as_deref(), Some("978-3-030-00000-0"));
    assert_eq!(b.pdf_url, None, "only open-access PDFs are offered");
}

const SCIENCEDIRECT_URL: &str = "https://api.elsevier.com/content/search/sciencedirect";

const SCIENCEDIRECT: &str = r#"{"resultsFound":12,"results":[
  {"authors":[{"order":2,"name":"Alan Turing"},{"order":1,"name":"Ada Lovelace"}],"doi":"10.1016/j.x.2020.1",
   "openAccess":false,"pages":{"first":"1","last":"10"},"pii":"S0000000020000001","publicationDate":"2020-03-01",
   "sourceTitle":"Journal of Things","title":"On <i>engines</i>","uri":"https://www.sciencedirect.com/science/article/pii/S0000000020000001",
   "volumeIssue":"Volume 12, Issue 3"}
]}"#;

#[test]
fn searches_sciencedirect() {
    let query = Query { page: 1, per_page: 2, key: Some("k5".into()), ..Query::new("engines") };
    let searcher = Searcher::with_http(FakeHttp::with(&[(SCIENCEDIRECT_URL, 200, SCIENCEDIRECT)]));
    let page = searcher.search(Source::ScienceDirect, &query).unwrap();
    let sent: Value = serde_json::from_slice(&searcher.http.bodies.lock().unwrap()[0]).unwrap();
    assert_eq!(sent["qs"], "engines");
    assert_eq!((sent["display"]["offset"].as_u64(), sent["display"]["show"].as_u64()), (Some(2), Some(2)));
    assert!(searcher.http.headers.lock().unwrap()[0].contains(&("X-ELS-APIKey".into(), "k5".into())));
    assert_eq!(page.total, Some(12));
    let a = &page.hits[0];
    assert_eq!(a.key, "sciencedirect:S0000000020000001");
    assert_eq!(a.item.title, "On engines");
    assert_eq!(a.item.creators, vec![Creator::person("Lovelace", "Ada"), Creator::person("Turing", "Alan")]);
    assert_eq!((a.item.volume.as_deref(), a.item.issue.as_deref()), (Some("12"), Some("3")));
    assert_eq!(a.item.pages.as_deref(), Some("1-10"));
    assert_eq!(a.item.date.as_deref(), Some("2020-03-01"));
}

const LOC_URL: &str = "http://lx2.loc.gov:210/LCDB?version=1.1&operation=searchRetrieve&query=%22machine%22+and+%22learning%22&startRecord=1&maximumRecords=2&recordSchema=marcxml";

const LOC: &str = r#"<?xml version="1.0"?>
<zs:searchRetrieveResponse xmlns:zs="http://www.loc.gov/zing/srw/"><zs:version>1.1</zs:version><zs:numberOfRecords>2</zs:numberOfRecords><zs:records>
<zs:record><zs:recordSchema>marcxml</zs:recordSchema><zs:recordPacking>xml</zs:recordPacking><zs:recordData><record xmlns="http://www.loc.gov/MARC21/slim">
  <leader>01234cam a2200301 a 4500</leader>
  <controlfield tag="001">21000001</controlfield>
  <controlfield tag="008">200101s2020    cc a     b    000 0 chi d</controlfield>
  <datafield tag="010" ind1=" " ind2=" "><subfield code="a">  2020123456</subfield></datafield>
  <datafield tag="020" ind1=" " ind2=" "><subfield code="a">7111111111 (pbk.)</subfield></datafield>
  <datafield tag="020" ind1=" " ind2=" "><subfield code="a">9787111111111</subfield></datafield>
  <datafield tag="024" ind1="7" ind2=" "><subfield code="a">10.1000/XYZ123</subfield><subfield code="2">doi</subfield></datafield>
  <datafield tag="100" ind1="1" ind2=" "><subfield code="6">880-01</subfield><subfield code="a">Zhang, San,</subfield><subfield code="e">author.</subfield></datafield>
  <datafield tag="245" ind1="1" ind2="0"><subfield code="6">880-02</subfield><subfield code="a">Ji qi xue xi :</subfield><subfield code="b">yuan li yu fang fa /</subfield><subfield code="c">Zhang San zhu.</subfield></datafield>
  <datafield tag="264" ind1=" " ind2="4"><subfield code="c">&#xA9;2019</subfield></datafield>
  <datafield tag="264" ind1=" " ind2="1"><subfield code="6">880-03</subfield><subfield code="a">Beijing :</subfield><subfield code="b">Ji xie gong ye chu ban she,</subfield><subfield code="c">2020.</subfield></datafield>
  <datafield tag="502" ind1=" " ind2=" "><subfield code="a">Thesis (Ph.D.)--Tsinghua University, 2019.</subfield></datafield>
  <datafield tag="520" ind1=" " ind2=" "><subfield code="a">An introduction &amp; survey.</subfield></datafield>
  <datafield tag="650" ind1=" " ind2="0"><subfield code="a">Machine learning.</subfield></datafield>
  <datafield tag="650" ind1=" " ind2="7"><subfield code="a">Machine learning</subfield><subfield code="2">fast</subfield></datafield>
  <datafield tag="710" ind1="2" ind2=" "><subfield code="a">Tsinghua University.</subfield><subfield code="b">Department of Computer Science,</subfield><subfield code="e">degree granting institution.</subfield></datafield>
  <datafield tag="880" ind1="1" ind2=" "><subfield code="6">100-01/$1</subfield><subfield code="a">张三,</subfield><subfield code="e">author.</subfield></datafield>
  <datafield tag="880" ind1="1" ind2="0"><subfield code="6">245-02/$1</subfield><subfield code="a">机器学习 :</subfield><subfield code="b">原理与方法 /</subfield><subfield code="c">张三著.</subfield></datafield>
  <datafield tag="880" ind1=" " ind2="1"><subfield code="6">264-03/$1</subfield><subfield code="a">北京 :</subfield><subfield code="b">机械工业出版社,</subfield><subfield code="c">2020.</subfield></datafield>
</record></zs:recordData><zs:recordPosition>1</zs:recordPosition></zs:record>
<zs:record><zs:recordSchema>marcxml</zs:recordSchema><zs:recordPacking>xml</zs:recordPacking><zs:recordData><record xmlns="http://www.loc.gov/MARC21/slim">
  <leader>01234cam a2200301 a 4500</leader>
  <datafield tag="245" ind1="1" ind2="0"><subfield code="a">No control number</subfield></datafield>
</record></zs:recordData><zs:recordPosition>2</zs:recordPosition></zs:record>
</zs:records></zs:searchRetrieveResponse>"#;

#[test]
fn searches_the_library_of_congress() {
    let page = search(&[(LOC_URL, 200, LOC)], Source::LibraryOfCongress, "machine learning").unwrap();
    assert_eq!(page.total, Some(2));
    assert_eq!(page.hits.len(), 1, "a record without a control number is left out");
    let hit = &page.hits[0];
    assert_eq!(hit.key, "loc:21000001");
    let item = &hit.item;
    assert_eq!(item.item_type, ItemType::Thesis);
    assert_eq!(item.title, "机器学习: 原理与方法", "the title in its own script");
    let institution = Creator {
        role: paperloom_model::CreatorRole::Contributor,
        ..Creator::literal("Tsinghua University. Department of Computer Science")
    };
    assert_eq!(item.creators, vec![Creator::literal("张三"), institution]);
    assert_eq!((item.place.as_deref(), item.publisher.as_deref()), (Some("北京"), Some("机械工业出版社")));
    assert_eq!(item.year, Some(2020), "the publication, not the copyright date");
    assert_eq!(item.isbn.as_deref(), Some("9787111111111"));
    assert_eq!(item.doi.as_ref().map(|d| d.as_str()), Some("10.1000/xyz123"));
    assert_eq!(item.abstract_note.as_deref(), Some("An introduction & survey."));
    assert_eq!(item.keywords, ["Machine learning"]);
    assert_eq!(item.language.as_deref(), Some("zh"));
    assert_eq!(item.url.as_deref(), Some("https://lccn.loc.gov/2020123456"));
}

const K10PLUS_URL: &str = "https://sru.k10plus.de/opac-de-627?version=1.1&operation=searchRetrieve&query=pica.all%3D%22grokking%22+and+pica.all%3D%22statistics%22&startRecord=1&maximumRecords=2&recordSchema=marcxml";

const K10PLUS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<zs:searchRetrieveResponse xmlns:zs="http://www.loc.gov/zing/srw/"><zs:version>1.1</zs:version><zs:numberOfRecords>89717</zs:numberOfRecords><zs:records>
<zs:record><zs:recordSchema>marcxml</zs:recordSchema><zs:recordPacking>xml</zs:recordPacking><zs:recordData><record xmlns="http://www.loc.gov/MARC21/slim">
  <leader>     nam a22     2c 4500</leader>
  <controlfield tag="001">1985759950</controlfield>
  <controlfield tag="008">261006s2027    xx |||||o     00| ||eng c</controlfield>
  <datafield tag="020" ind1=" " ind2=" "><subfield code="a">1633434761</subfield><subfield code="q">electronic bk.</subfield></datafield>
  <datafield tag="020" ind1=" " ind2=" "><subfield code="a">9781633434769</subfield></datafield>
  <datafield tag="041" ind1=" " ind2=" "><subfield code="a">eng</subfield></datafield>
  <datafield tag="100" ind1="1" ind2=" "><subfield code="a">Nield, Thomas</subfield><subfield code="c">(Computer programmer),</subfield><subfield code="e">VerfasserIn</subfield><subfield code="4">aut</subfield></datafield>
  <datafield tag="245" ind1="1" ind2="0"><subfield code="a">Grokking statistics</subfield><subfield code="b">a friendly guide for beginners</subfield><subfield code="c">Thomas Nield</subfield></datafield>
  <datafield tag="250" ind1=" " ind2=" "><subfield code="a">2nd edition</subfield></datafield>
  <datafield tag="264" ind1=" " ind2="1"><subfield code="a">Shelter Island, NY</subfield><subfield code="b">Manning Publications</subfield><subfield code="c">[2027]</subfield></datafield>
  <datafield tag="700" ind1="1" ind2=" "><subfield code="a">Doe, Jane</subfield><subfield code="e">HerausgeberIn</subfield><subfield code="4">edt</subfield></datafield>
  <datafield tag="856" ind1="4" ind2="0"><subfield code="u">https://learning.oreilly.com/library/view/-/9781633434769/</subfield></datafield>
</record></zs:recordData></zs:record>
<zs:record><zs:recordSchema>marcxml</zs:recordSchema><zs:recordPacking>xml</zs:recordPacking><zs:recordData><record xmlns="http://www.loc.gov/MARC21/slim">
  <leader>     naa a22     2  4500</leader>
  <controlfield tag="001">1700000001</controlfield>
  <controlfield tag="008">200301s2020    xx |||||o     00| ||ger c</controlfield>
  <datafield tag="024" ind1="7" ind2=" "><subfield code="a">10.1007/s00287-020-01234-5</subfield><subfield code="2">doi</subfield></datafield>
  <datafield tag="245" ind1="1" ind2="0"><subfield code="a">Maschinelles Lernen in der Praxis</subfield></datafield>
  <datafield tag="773" ind1="0" ind2="8"><subfield code="t">Informatik-Spektrum</subfield><subfield code="x">0170-6012</subfield></datafield>
</record></zs:recordData></zs:record>
</zs:records></zs:searchRetrieveResponse>"#;

#[test]
fn searches_k10plus() {
    let page = search(&[(K10PLUS_URL, 200, K10PLUS)], Source::K10plus, "grokking statistics").unwrap();
    assert_eq!(page.total, Some(89717));
    let [book, article] = page.hits.as_slice() else { panic!("two hits: {:?}", page.hits) };
    assert_eq!(book.key, "k10plus:1985759950");
    assert_eq!(book.item.item_type, ItemType::Book);
    assert_eq!(book.item.title, "Grokking statistics: a friendly guide for beginners");
    let editor = Creator { role: paperloom_model::CreatorRole::Editor, ..Creator::person("Doe", "Jane") };
    assert_eq!(book.item.creators, vec![Creator::person("Nield", "Thomas"), editor]);
    assert_eq!(book.item.place.as_deref(), Some("Shelter Island, NY"));
    assert_eq!(book.item.publisher.as_deref(), Some("Manning Publications"));
    assert_eq!(book.item.year, Some(2027));
    assert_eq!(book.item.edition.as_deref(), Some("2nd edition"));
    assert_eq!(book.item.isbn.as_deref(), Some("9781633434769"));
    assert_eq!(book.item.language.as_deref(), Some("en"));
    assert_eq!(book.item.url.as_deref(), Some("https://learning.oreilly.com/library/view/-/9781633434769/"));

    assert_eq!(article.item.item_type, ItemType::JournalArticle);
    assert_eq!(article.item.container_title.as_deref(), Some("Informatik-Spektrum"));
    assert_eq!(article.item.issn.as_deref(), Some("0170-6012"));
    assert_eq!(article.item.year, Some(2020), "from the fixed field");
    assert_eq!(article.item.language.as_deref(), Some("de"));
    assert_eq!(article.item.doi.as_ref().map(|d| d.as_str()), Some("10.1007/s00287-020-01234-5"));
    assert_eq!(article.item.url.as_deref(), Some("https://kxp.k10plus.de/DB=2.1/PPNSET?PPN=1700000001"));
    assert!(article.item.creators.is_empty());
}

#[test]
fn catalogue_diagnostics_are_errors() {
    let refused = r#"<zs:searchRetrieveResponse xmlns:zs="http://www.loc.gov/zing/srw/"><zs:version>1.1</zs:version><zs:diagnostics><diag:diagnostic xmlns:diag="http://www.loc.gov/zing/srw/diagnostic/"><diag:uri>info:srw/diagnostic/1/10</diag:uri><diag:message>Query syntax error</diag:message></diag:diagnostic></zs:diagnostics></zs:searchRetrieveResponse>"#;
    let err = search(&[(K10PLUS_URL, 200, refused)], Source::K10plus, "grokking statistics").unwrap_err();
    assert!(
        matches!(&err, Error::Catalogue { name, detail } if name == "K10plus" && detail == "Query syntax error")
    );
    assert_eq!(err.to_string(), "Catalogue K10plus: Query syntax error");
}

const BNF_URL: &str = "https://catalogue.bnf.fr/api/SRU?version=1.2&operation=searchRetrieve&query=bib.anywhere%3D%22dirty%22+and+bib.anywhere%3D%22data%22&startRecord=1&maximumRecords=2&recordSchema=dublincore";

const BNF: &str = r#"<?xml version="1.0" encoding="UTF-8"?><srw:searchRetrieveResponse xmlns:srw="http://www.loc.gov/zing/srw/">
<srw:version>1.2</srw:version>
<srw:numberOfRecords>991</srw:numberOfRecords>
<srw:records>
<srw:record>
<srw:recordSchema>dc</srw:recordSchema>
<srw:recordPacking>xml</srw:recordPacking>
<srw:recordData>
<oai_dc:dc xmlns:oai_dc="http://www.openarchives.org/OAI/2.0/oai_dc/" xmlns:dc="http://purl.org/dc/elements/1.1/">
  <dc:identifier>http://catalogue.bnf.fr/ark:/12148/cb47476483n</dc:identifier>
  <dc:title>Dirty data processing for machine learning / Zhixin Qi, Hongzhi Wang, Zejiao Dong</dc:title>
  <dc:creator>Qi, Zhixin. Auteur du texte</dc:creator>
  <dc:creator>Wang, Hongzhi (19..-.... ; chercheur en informatique). Auteur du texte</dc:creator>
  <dc:contributor>Martin, Paul. Traducteur</dc:contributor>
  <dc:publisher>Springer (Singapore)</dc:publisher>
  <dc:date>2024</dc:date>
  <dc:subject xml:lang="fre">Apprentissage automatique</dc:subject>
  <dc:identifier>ISBN 9789819976577</dc:identifier>
  <dc:language>eng</dc:language>
  <dc:type xml:lang="fre">texte imprimé</dc:type>
</oai_dc:dc></srw:recordData>
<srw:recordIdentifier>ark:/12148/cb47476483n</srw:recordIdentifier>
<srw:recordPosition>1</srw:recordPosition>
</srw:record>
</srw:records>
</srw:searchRetrieveResponse>"#;

const DNB_URL: &str = "https://services.dnb.de/sru/dnb?version=1.1&operation=searchRetrieve&query=%22workflows%22&startRecord=1&maximumRecords=2&recordSchema=oai_dc";

/// SRU without a prefix, and the record packed as a string.
const DNB: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<searchRetrieveResponse xmlns="http://www.loc.gov/zing/srw/"><version>1.1</version><numberOfRecords>8348</numberOfRecords><records><record><recordSchema>oai_dc</recordSchema><recordPacking>string</recordPacking><recordData>&lt;dc xmlns="http://www.openarchives.org/OAI/2.0/oai_dc/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"&gt;
  &lt;dc:title&gt;10 KI-Workflows : Die dir jede Woche Zeit sparen / Karsten Kr&amp;#228;mer&lt;/dc:title&gt;
  &lt;dc:creator&gt;Kr&amp;#228;mer, Karsten [Verfasser]&lt;/dc:creator&gt;
  &lt;dc:publisher&gt;Berlin : epubli&lt;/dc:publisher&gt;
  &lt;dc:date&gt;2026&lt;/dc:date&gt;
  &lt;dc:language&gt;ger&lt;/dc:language&gt;
  &lt;dc:identifier xsi:type="tel:URN"&gt;urn:nbn:de:101:1-2601301027295&lt;/dc:identifier&gt;
  &lt;dc:identifier xsi:type="tel:URL"&gt;http://nbn-resolving.de/urn:nbn:de:101:1-2601301027295&lt;/dc:identifier&gt;
  &lt;dc:identifier xsi:type="tel:ISBN"&gt;978-3-565-20212-6&lt;/dc:identifier&gt;
  &lt;dc:identifier xsi:type="dnb:IDN"&gt;1388449072&lt;/dc:identifier&gt;
  &lt;dc:subject&gt;650 Management&lt;/dc:subject&gt;
&lt;/dc&gt;</recordData><recordPosition>1</recordPosition></record></records></searchRetrieveResponse>"#;

fn search_catalogue(routes: &[(&str, u16, &str)], catalogue: &Catalogue, text: &str) -> Result<Page> {
    let query = Query { per_page: 2, ..Query::new(text) };
    Searcher::with_http(FakeHttp::with(routes)).search_catalogue(catalogue, &query)
}

fn catalogue(url: &str, index: &str, schema: &str) -> Catalogue {
    Catalogue {
        id: "mine".to_string(),
        name: "My library".to_string(),
        url: url.to_string(),
        index: index.to_string(),
        schema: schema.to_string(),
    }
}

#[test]
fn searches_a_catalogue_in_dublin_core() {
    let bnf = catalogue("https://catalogue.bnf.fr/api/SRU?version=1.2", "bib.anywhere", "dublincore");
    let page = search_catalogue(&[(BNF_URL, 200, BNF)], &bnf, "dirty data").unwrap();
    assert_eq!(page.total, Some(991));
    let hit = &page.hits[0];
    assert_eq!(hit.key, "mine:ark:/12148/cb47476483n");
    assert_eq!(hit.item.item_type, ItemType::Book);
    assert_eq!(hit.item.title, "Dirty data processing for machine learning");
    let translator =
        Creator { role: paperloom_model::CreatorRole::Translator, ..Creator::person("Martin", "Paul") };
    assert_eq!(
        hit.item.creators,
        vec![Creator::person("Qi", "Zhixin"), Creator::person("Wang", "Hongzhi"), translator]
    );
    assert_eq!(hit.item.publisher.as_deref(), Some("Springer (Singapore)"));
    assert_eq!(hit.item.year, Some(2024));
    assert_eq!(hit.item.isbn.as_deref(), Some("9789819976577"));
    assert_eq!(hit.item.language.as_deref(), Some("en"));
    assert_eq!(hit.item.keywords, ["Apprentissage automatique"]);
    assert_eq!(hit.item.url.as_deref(), Some("http://catalogue.bnf.fr/ark:/12148/cb47476483n"));
}

#[test]
fn reads_records_packed_as_strings() {
    // A pasted search address: its own search gives way, its version stays.
    let pasted =
        "https://services.dnb.de/sru/dnb?version=1.1&operation=searchRetrieve&query=old&recordSchema=x#top";
    let dnb = catalogue(pasted, "", "oai_dc");
    let page = search_catalogue(&[(DNB_URL, 200, DNB)], &dnb, "workflows").unwrap();
    let hit = &page.hits[0];
    assert_eq!(
        hit.key, "mine:urn:nbn:de:101:1-2601301027295",
        "the first identifier, with no record identifier"
    );
    assert_eq!(hit.item.title, "10 KI-Workflows: Die dir jede Woche Zeit sparen");
    assert_eq!(hit.item.creators, vec![Creator::person("Krämer", "Karsten")]);
    assert_eq!((hit.item.place.as_deref(), hit.item.publisher.as_deref()), (Some("Berlin"), Some("epubli")));
    assert_eq!(hit.item.isbn.as_deref(), Some("9783565202126"), "not the catalogue's own number");
    assert_eq!(hit.item.url.as_deref(), Some("http://nbn-resolving.de/urn:nbn:de:101:1-2601301027295"));
    assert_eq!(hit.item.keywords, ["Management"]);
    assert_eq!(hit.item.language.as_deref(), Some("de"));
}

#[test]
fn catalogues_need_a_web_address() {
    let err = search_catalogue(&[], &catalogue("file:///etc/passwd", "", ""), "x").unwrap_err();
    assert_eq!(err.to_string(), "Catalogue My library: not a web address");
    let err = search_catalogue(&[], &catalogue("not a url", "", ""), "x").unwrap_err();
    assert!(matches!(err, Error::Catalogue { .. }));
    let page = search_catalogue(&[], &catalogue("https://example.org/sru", "", ""), "  \"  ").unwrap();
    assert!(page.hits.is_empty(), "nothing to look for asks nothing");
    let missing = search_catalogue(&[], &catalogue("https://example.org/sru", "", ""), "x").unwrap_err();
    assert!(matches!(missing, Error::Network(_)), "it asks with version 1.2 and MARCXML: {missing}");
}

#[test]
fn offers_catalogue_presets() {
    let presets = catalogue_presets();
    assert!(presets.len() >= 3);
    for preset in presets {
        assert!(preset.url.starts_with("https://") || preset.url.starts_with("z3950://"), "{}", preset.url);
    }
}

#[test]
fn keyed_sources_need_a_key_that_works() {
    let err = search(&[], Source::Scopus, "nv thermometry").unwrap_err();
    assert!(matches!(err, Error::KeyNeeded("Scopus")), "{err}");
    let (err, _) = search_with_key(&[], Source::Ieee, "x", "  ");
    assert!(matches!(err.unwrap_err(), Error::KeyNeeded("IEEE Xplore")));
    let (err, _) = search_with_key(&[(WOS_URL, 401, "")], Source::WebOfScience, "nv thermometry", "bad");
    assert!(matches!(err.unwrap_err(), Error::KeyRejected("Web of Science")));
    let (err, _) = search_with_key(&[], Source::Springer, "nv centre", "secret-key");
    let message = err.unwrap_err().to_string();
    assert!(!message.contains("secret-key"), "the key stays out of errors: {message}");
    assert!(Source::Ieee.needs_key() && !Source::Crossref.needs_key());
}

#[test]
fn sources_round_trip_their_ids() {
    for source in Source::ALL {
        assert_eq!(Source::parse_id(source.id()), Some(source));
    }
    assert_eq!(Source::parse_id("cnki"), None);
}

#[test]
fn reads_xml() {
    let doc = r#"<a><link href="x&amp;y" title='pdf'/><b>one</b><bb>no</bb><b>two</b></a>"#;
    let bs: Vec<String> = xml::elements(doc, "b").iter().map(|e| xml::text(e.body)).collect();
    assert_eq!(bs, ["one", "two"]);
    let link = &xml::elements(doc, "link")[0];
    assert_eq!(xml::attr(link.attrs, "href").as_deref(), Some("x&y"));
    assert_eq!(xml::attr(link.attrs, "title").as_deref(), Some("pdf"));
    assert_eq!(xml::attr(link.attrs, "rel"), None);
    assert_eq!(xml::text("a &unknown; b &#x4E2D;"), "a &unknown; b 中");
}
