//! The connector: a small HTTP server on this computer for the Paperloom
//! browser extension and the writing plugins (Word, WPS). The extension
//! reads a page with the same script as the in-app browser and sends what it
//! found; the library adds the works the person picks.
//!
//! Only this computer can reach it (127.0.0.1). The extension's requests
//! from web pages (an `Origin` of `http…`) are refused, so a site cannot
//! add to the library; the extension's own pages send an extension origin,
//! or none. The writing plugins are web pages, so their `/cite/…` requests
//! may come from any origin but need a token the person granted in the app
//! (`POST /cite/pair`); see `docs/writing-plugins.md`.
//!
//! - `GET /ping`: `{"app": "Paperloom", "version": …, "cite": 2}`;
//! - `POST /read`, the script's answer: a [`SearchPage`] of what the page
//!   holds;
//! - `POST /save`, `{"keys": […]}`: adds those works (an [`ImportReport`]);
//! - `POST /cite/pair`, `GET /cite/styles`, `POST /cite/search`,
//!   `POST /cite/format` and `POST /cite/markers` for the writing plugins.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{CitationCluster, CoreError, ImportReport, PaperLibrary, SearchPage};

/// The port the extension looks for the app on.
pub const CONNECTOR_PORT: u16 = 23121;

/// The largest request the connector reads.
const MAX_BODY: usize = 8 * 1024 * 1024;

/// What the app hears from the connector.
#[uniffi::export(with_foreign)]
pub trait ConnectorEvents: Send + Sync {
    /// Works were added from the browser.
    fn added(&self, count: u32);
    /// A writing plugin (`client`, e.g. "Word") on a page from `origin`
    /// (empty when it sent none) asks to read the library and format
    /// citations: true when the person allows it. May wait for them.
    fn allow_writer(&self, client: String, origin: String) -> bool;
}

/// The version of the cite API `GET /ping` reports.
const CITE_API: u32 = 2;

/// A writing plugin the person allowed.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Writer {
    token: String,
    client: String,
    origin: String,
    /// Unix seconds.
    added: i64,
}

/// The writing plugins the person allowed, kept beside the library (in
/// memory for a library kept in memory).
#[derive(Debug, Default)]
pub(crate) struct Writers {
    path: Option<PathBuf>,
    loaded: Option<Vec<Writer>>,
}

impl Writers {
    pub(crate) fn beside(folder: &Path) -> Self {
        Self { path: Some(folder.join("connector-clients.json")), loaded: None }
    }

    fn list(&mut self) -> &mut Vec<Writer> {
        let path = self.path.clone();
        self.loaded.get_or_insert_with(|| {
            path.and_then(|p| std::fs::read(p).ok())
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default()
        })
    }

    fn knows(&mut self, token: &str) -> bool {
        // The whole list is compared, each in full, so timing tells nothing.
        let mut known = false;
        for writer in self.list().iter() {
            known |= same(writer.token.as_bytes(), token.as_bytes());
        }
        known
    }

    fn keep(&mut self, writer: Writer) -> Result<(), CoreError> {
        self.list().push(writer);
        self.save()
    }

    fn save(&mut self) -> Result<(), CoreError> {
        let Some(path) = self.path.clone() else { return Ok(()) };
        let json = serde_json::to_vec_pretty(self.list()).unwrap_or_default();
        std::fs::write(&path, json).map_err(|e| CoreError::Storage { message: e.to_string() })
    }
}

/// Equal bytes, compared without stopping at the first difference.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

/// A writing plugin the person allowed, for Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedWriter {
    pub client: String,
    pub origin: String,
    /// Unix seconds.
    pub added: i64,
}

impl PaperLibrary {
    fn writers(&self) -> Result<std::sync::MutexGuard<'_, Writers>, CoreError> {
        self.writers
            .lock()
            .map_err(|_| CoreError::Storage { message: "The library is unavailable after a crash.".into() })
    }
}

#[uniffi::export]
impl PaperLibrary {
    /// The writing plugins (Word, WPS) allowed to read the library.
    pub fn connected_writers(&self) -> Result<Vec<ConnectedWriter>, CoreError> {
        let mut writers = self.writers()?;
        Ok(writers
            .list()
            .iter()
            .map(|w| ConnectedWriter { client: w.client.clone(), origin: w.origin.clone(), added: w.added })
            .collect())
    }

    /// Takes back every writing plugin's access; each asks again.
    pub fn forget_connected_writers(&self) -> Result<(), CoreError> {
        let mut writers = self.writers()?;
        writers.list().clear();
        writers.save()
    }
}

/// The port the extension looks for the app on.
#[uniffi::export]
pub fn connector_port() -> u16 {
    CONNECTOR_PORT
}

struct Request {
    method: String,
    path: String,
    origin: Option<String>,
    /// The token after `Authorization: Bearer`.
    token: Option<String>,
    /// A preflight asked about reaching this computer from a page
    /// (`Access-Control-Request-Private-Network`).
    private_network: bool,
    body: Vec<u8>,
}

impl Request {
    /// The writing plugins' API, open to web pages that hold a token.
    fn cite(&self) -> bool {
        self.path == "/ping" || self.path.starts_with("/cite/")
    }
}

fn read_request(stream: &TcpStream) -> io::Result<Request> {
    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut length = 0;
    let mut origin = None;
    let mut token = None;
    let mut private_network = false;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            break;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = value.trim().parse().unwrap_or(0),
                "origin" => origin = Some(value.trim().to_string()),
                "authorization" => {
                    token = value.trim().strip_prefix("Bearer ").map(|t| t.trim().to_string());
                }
                "access-control-request-private-network" => private_network = value.trim() == "true",
                _ => {}
            }
        }
    }
    if length > MAX_BODY {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "too large"));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Request { method, path, origin, token, private_network, body })
}

/// Whether a request may come from where it says: an extension, or no page
/// at all.
fn allowed(origin: Option<&str>) -> bool {
    origin.is_none_or(|o| {
        ["chrome-extension://", "moz-extension://", "safari-web-extension://"]
            .iter()
            .any(|p| o.starts_with(p))
    })
}

fn respond(stream: &mut TcpStream, status: u16, body: &str, request: &Request) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Error",
    };
    // The extension's pages, and any page for the writing plugins' API
    // (a token guards it); other pages get no CORS headers.
    let origin = request.origin.as_deref().filter(|o| allowed(Some(o)) || request.cite());
    let mut cors = origin
        .map(|o| {
            format!(
                "Access-Control-Allow-Origin: {o}\r\nVary: Origin\r\nAccess-Control-Allow-Methods: GET, POST\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\n"
            )
        })
        .unwrap_or_default();
    if origin.is_some() && request.private_network {
        cors.push_str("Access-Control-Allow-Private-Network: true\r\n");
    }
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\n{cors}Connection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

fn error_body(message: &str) -> String {
    serde_json::json!({ "error": message }).to_string()
}

#[derive(Deserialize)]
struct Save {
    keys: Vec<String>,
}

#[derive(Deserialize)]
struct Pair {
    client: String,
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    query: String,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Format {
    style_id: String,
    clusters: Vec<CitationCluster>,
    #[serde(default)]
    html: bool,
}

#[derive(Deserialize)]
struct Markers {
    text: String,
}

fn json<T: Serialize>(value: &T) -> (u16, String) {
    (200, serde_json::to_string(value).unwrap_or_default())
}

/// Asks the person whether a writing plugin may read the library; a token
/// when they allow it.
fn pair(library: &PaperLibrary, events: &dyn ConnectorEvents, request: &Request) -> (u16, String) {
    let Ok(pair) = serde_json::from_slice::<Pair>(&request.body) else {
        return (400, error_body("The request does not say which program asks."));
    };
    // A name for the question, not a message from the page.
    let client: String = pair.client.chars().filter(|c| !c.is_control()).take(40).collect();
    if client.trim().is_empty() {
        return (400, error_body("The request does not say which program asks."));
    }
    // One question at a time.
    if library.pairing.swap(true, Ordering::SeqCst) {
        return (409, error_body("Paperloom is already asking about another program."));
    }
    let origin = request.origin.clone().unwrap_or_default();
    let allowed = events.allow_writer(client.clone(), origin.clone());
    library.pairing.store(false, Ordering::SeqCst);
    if !allowed {
        return (403, error_body("Paperloom was not allowed to connect."));
    }
    let mut bytes = [0u8; 32];
    if getrandom::fill(&mut bytes).is_err() {
        return (500, error_body("No random numbers for a token."));
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let token: String = bytes
        .iter()
        .flat_map(|b| [char::from(HEX[usize::from(b >> 4)]), char::from(HEX[usize::from(b & 15)])])
        .collect();
    let added = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let kept = library
        .writers()
        .and_then(|mut writers| writers.keep(Writer { token: token.clone(), client, origin, added }));
    match kept {
        Ok(()) => json(&serde_json::json!({ "token": token })),
        Err(e) => (500, error_body(&e.to_string())),
    }
}

/// The writing plugins' requests that need a token.
fn cite(library: &PaperLibrary, request: &Request) -> (u16, String) {
    let known = request
        .token
        .as_deref()
        .is_some_and(|token| library.writers().is_ok_and(|mut writers| writers.knows(token)));
    if !known {
        return (401, error_body("Connect to Paperloom first."));
    }
    let result = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/cite/styles") => return json(&crate::citation_styles()),
        ("POST", "/cite/search") => match serde_json::from_slice::<Search>(&request.body) {
            Ok(search) => {
                library.cite_candidates(search.query, search.limit.unwrap_or(20).min(200)).map(|c| json(&c))
            }
            Err(_) => return (400, error_body("The request is not a search.")),
        },
        ("POST", "/cite/format") => match serde_json::from_slice::<Format>(&request.body) {
            Ok(format) => library
                .format_citation_clusters(format.style_id, format.clusters, format.html)
                .map(|t| json(&t)),
            Err(_) => return (400, error_body("The request names no citations to format.")),
        },
        ("POST", "/cite/markers") => match serde_json::from_slice::<Markers>(&request.body) {
            Ok(markers) => library.cite_markers(&markers.text).map(|m| json(&m)),
            Err(_) => return (400, error_body("The request holds no text to read citations from.")),
        },
        _ => return (404, error_body("There is nothing here.")),
    };
    result.unwrap_or_else(|e| (400, error_body(&e.to_string())))
}

/// Answers one request.
fn answer(library: &PaperLibrary, events: &dyn ConnectorEvents, request: &Request) -> (u16, String) {
    if !request.cite() && !allowed(request.origin.as_deref()) {
        return (403, error_body("Requests from web pages are not accepted."));
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("OPTIONS", _) => (204, String::new()),
        ("GET", "/ping") => json(
            &serde_json::json!({ "app": "Paperloom", "version": env!("CARGO_PKG_VERSION"), "cite": CITE_API }),
        ),
        ("POST", "/cite/pair") => pair(library, events, request),
        (_, path) if path.starts_with("/cite/") => cite(library, request),
        ("POST", "/read") => {
            let capture = String::from_utf8_lossy(&request.body).into_owned();
            match library.read_captured_page(capture) {
                Ok(page) => (200, serde_json::to_string::<SearchPage>(&page).unwrap_or_default()),
                Err(e) => (400, error_body(&e.to_string())),
            }
        }
        ("POST", "/save") => {
            let Ok(save) = serde_json::from_slice::<Save>(&request.body) else {
                return (400, error_body("The request names no works to add."));
            };
            match library.add_search_results(save.keys, None) {
                Ok(report) => {
                    if report.added > 0 {
                        events.added(report.added);
                    }
                    (200, serde_json::to_string::<ImportReport>(&report).unwrap_or_default())
                }
                Err(e) => (400, error_body(&e.to_string())),
            }
        }
        _ => (404, error_body("There is nothing here.")),
    }
}

impl PaperLibrary {
    /// Answers requests on a listener until the app ends.
    pub(crate) fn serve_connector(self: Arc<Self>, listener: TcpListener, events: Arc<dyn ConnectorEvents>) {
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                // Each on a thread of its own: a pairing question waits for
                // the person, and the rest must not wait with it.
                let (library, events) = (self.clone(), events.clone());
                std::thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                    let Ok(request) = read_request(&stream) else { return };
                    let (status, body) = answer(&library, events.as_ref(), &request);
                    let _ = respond(&mut stream, status, &body, &request);
                });
            }
        });
    }
}

#[uniffi::export]
impl PaperLibrary {
    /// Starts the connector for the browser extension on 127.0.0.1:`port`
    /// (`connector_port()`); false when the port is taken (another copy of
    /// the app has it).
    pub fn start_connector(self: Arc<Self>, port: u16, events: Arc<dyn ConnectorEvents>) -> bool {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                self.serve_connector(listener, events);
                true
            }
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU32;

    use super::*;

    /// Counts what was added and answers pairing questions with `allow`.
    #[derive(Default)]
    struct Counter {
        added: AtomicU32,
        asked: AtomicU32,
        allow: bool,
    }

    impl ConnectorEvents for Counter {
        fn added(&self, count: u32) {
            self.added.fetch_add(count, Ordering::SeqCst);
        }

        fn allow_writer(&self, _client: String, _origin: String) -> bool {
            self.asked.fetch_add(1, Ordering::SeqCst);
            self.allow
        }
    }

    /// Status, headers and body of one request.
    fn send(
        port: u16,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> (u16, String, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let headers: String = headers.iter().map(|(name, value)| format!("{name}: {value}\r\n")).collect();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{headers}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        let status = answer.split_whitespace().nth(1).unwrap().parse().unwrap();
        let (head, body) =
            answer.split_once("\r\n\r\n").map(|(h, b)| (h.to_string(), b.to_string())).unwrap();
        (status, head, body)
    }

    fn call(port: u16, method: &str, path: &str, origin: Option<&str>, body: &str) -> (u16, String) {
        let headers: Vec<(&str, &str)> = origin.map(|o| ("Origin", o)).into_iter().collect();
        let (status, _, body) = send(port, method, path, &headers, body);
        (status, body)
    }

    fn serve(library: &Arc<PaperLibrary>, events: Arc<Counter>) -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        library.clone().serve_connector(listener, events);
        port
    }

    #[test]
    fn adds_what_the_extension_found() {
        let library = PaperLibrary::in_memory().unwrap();
        let counter = Arc::new(Counter::default());
        let port = serve(&library, counter.clone());

        let (status, body) = call(port, "GET", "/ping", Some("chrome-extension://abc"), "");
        assert_eq!(status, 200);
        assert!(body.contains("\"app\":\"Paperloom\"") && body.contains("\"cite\":2"), "{body}");
        let capture = r#"{"url":"https://journal.example.org/paper","title":"X",
            "meta":[["citation_title","Nanometre-scale thermometry"],["citation_author","Kucsko, G."]],
            "coins":[],"jsonLd":[],"records":[]}"#;
        let (status, body) = call(port, "POST", "/read", Some("moz-extension://abc"), capture);
        assert_eq!(status, 200, "{body}");
        let page: SearchPage = serde_json::from_str(&body).unwrap();
        assert_eq!(page.hits.len(), 1);
        assert_eq!(page.hits[0].detail.title, "Nanometre-scale thermometry");
        let keys = serde_json::json!({ "keys": [page.hits[0].key] }).to_string();
        let (status, body) = call(port, "POST", "/save", None, &keys);
        assert_eq!(status, 200, "{body}");
        assert!(body.contains("\"added\":1"), "{body}");
        assert_eq!(counter.added.load(Ordering::SeqCst), 1);
        assert_eq!(library.list_items(crate::ItemFilter::default()).unwrap().len(), 1);

        let (status, _) = call(port, "POST", "/save", Some("https://evil.example"), &keys);
        assert_eq!(status, 403, "web pages cannot add to the library");
        let (status, head, _) = send(port, "OPTIONS", "/save", &[("Origin", "https://evil.example")], "");
        assert_eq!(status, 403);
        assert!(!head.contains("Access-Control-Allow-Origin"), "{head}");
        assert_eq!(call(port, "GET", "/nothing", None, "").0, 404);
        assert_eq!(call(port, "POST", "/save", None, "{}").0, 400);
    }

    #[test]
    fn a_writing_plugin_pairs_then_searches_and_formats() {
        let library = PaperLibrary::in_memory().unwrap();
        let item = library
            .add_item(crate::NewItem {
                title: "Nanometre-scale thermometry in a living cell".into(),
                authors: vec!["Georg Kucsko".into()],
                year: Some(2013),
                ..crate::NewItem::default()
            })
            .unwrap();
        let counter = Arc::new(Counter { allow: true, ..Counter::default() });
        let port = serve(&library, counter.clone());
        let page = ("Origin", "https://addin.example");

        // A page may ask; without a token it learns nothing of the library.
        let (status, head, _) = send(
            port,
            "OPTIONS",
            "/cite/search",
            &[page, ("Access-Control-Request-Private-Network", "true")],
            "",
        );
        assert_eq!(status, 204);
        assert!(head.contains("Access-Control-Allow-Origin: https://addin.example"), "{head}");
        assert!(head.contains("Access-Control-Allow-Private-Network: true"), "{head}");
        assert!(head.contains("Authorization"), "{head}");
        let search = r#"{"query":"kucsko","limit":5}"#;
        assert_eq!(send(port, "POST", "/cite/search", &[page], search).0, 401);
        assert_eq!(
            send(port, "POST", "/cite/search", &[page, ("Authorization", "Bearer nope")], search).0,
            401
        );

        let (status, _, body) = send(port, "POST", "/cite/pair", &[page], r#"{"client":"Word"}"#);
        assert_eq!(status, 200, "{body}");
        assert_eq!(counter.asked.load(Ordering::SeqCst), 1);
        let token: serde_json::Value = serde_json::from_str(&body).unwrap();
        let token = token["token"].as_str().unwrap().to_string();
        assert_eq!(token.len(), 64);
        let bearer = format!("Bearer {token}");
        let auth = ("Authorization", bearer.as_str());

        let (status, _, body) = send(port, "POST", "/cite/search", &[page, auth], search);
        assert_eq!(status, 200, "{body}");
        assert!(body.contains(&item.id) && body.contains("\"cite\":"), "{body}");
        let (status, _, body) = send(port, "GET", "/cite/styles", &[page, auth], "");
        assert_eq!(status, 200);
        assert!(body.contains("gb-7714-2015-numeric"), "{body}");
        let format = serde_json::json!({
            "styleId": "ieee",
            "clusters": [{ "works": [{ "itemId": item.id, "locator": "12" }] }, { "works": [{ "itemId": "gone" }] }],
            "html": true
        })
        .to_string();
        let (status, _, body) = send(port, "POST", "/cite/format", &[page, auth], &format);
        assert_eq!(status, 200, "{body}");
        let text: crate::CitedText = serde_json::from_str(&body).unwrap();
        assert_eq!(text.citations.len(), 2);
        assert!(text.citations[0].contains("[1"), "{:?}", text.citations);
        assert_eq!(text.citations[1], "");
        assert_eq!(text.missing, ["gone"]);
        let key = library.cite_candidates("kucsko".into(), 1).unwrap().remove(0).key;
        let markers = serde_json::json!({ "text": format!("Heat [@{key}] and [@nobody].") }).to_string();
        let (status, _, body) = send(port, "POST", "/cite/markers", &[page, auth], &markers);
        assert_eq!(status, 200, "{body}");
        let markers: Vec<crate::CiteMarker> = serde_json::from_str(&body).unwrap();
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[0].works[0].item_id, item.id);
        assert_eq!(markers[1].missing, ["nobody"]);
        assert_eq!(library.connected_writers().unwrap()[0].client, "Word");

        // Taken back: the token no longer works.
        library.forget_connected_writers().unwrap();
        assert_eq!(send(port, "POST", "/cite/search", &[page, auth], search).0, 401);
    }

    #[test]
    fn a_declined_writing_plugin_gets_no_token() {
        let library = PaperLibrary::in_memory().unwrap();
        let port = serve(&library, Arc::new(Counter::default()));
        let (status, _, body) =
            send(port, "POST", "/cite/pair", &[("Origin", "https://addin.example")], r#"{"client":"Word"}"#);
        assert_eq!(status, 403, "{body}");
        assert!(!body.contains("token"));
        assert_eq!(send(port, "POST", "/cite/pair", &[], r#"{"client":""}"#).0, 400);
        assert!(library.connected_writers().unwrap().is_empty());
    }

    #[test]
    fn keeps_the_writers_beside_the_library() {
        let dir = tempfile::tempdir().unwrap();
        let mut writers = Writers::beside(dir.path());
        writers
            .keep(Writer { token: "a".repeat(64), client: "WPS".into(), origin: String::new(), added: 1 })
            .unwrap();
        let mut again = Writers::beside(dir.path());
        assert!(again.knows(&"a".repeat(64)));
        assert!(!again.knows(&"b".repeat(64)));
        assert!(!again.knows(""));
    }
}
