//! The HTTP client the core's network features share: sign-in, metadata
//! lookup, online search. A trait, so tests answer the calls themselves.

use std::io::Read as _;
use std::time::Duration;

/// The request never got an answer: no network, DNS, TLS, timeout.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct NetworkError(pub String);

/// An answer: status, content type and body text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Response {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: String,
}

impl Response {
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self { status, content_type: None, body: body.into() }
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// A bounded page response, with its final address when the client knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageResponse {
    pub response: Response,
    pub url: Option<String>,
}

/// Bounds the decoded body too, after any HTTP decompression.
fn page_body(reader: impl std::io::Read, limit: u64) -> Result<String, NetworkError> {
    let mut body = Vec::new();
    reader.take(limit.saturating_add(1)).read_to_end(&mut body).map_err(|e| NetworkError(e.to_string()))?;
    if body.len() as u64 > limit {
        return Err(NetworkError("The page is too large to read.".into()));
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// A request with any method, headers and body: for APIs beyond reading
/// pages and posting forms (the sync service).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// An answer as bytes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BytesResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl BytesResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

pub trait Http: Send + Sync {
    /// GET with the given `Accept` header; redirects are followed.
    fn get(&self, url: &str, accept: &str) -> Result<Response, NetworkError>;
    /// GET a page up to `limit` bytes. Clients should bound the read itself;
    /// the default checks the body after `get` for existing clients.
    fn get_page(&self, url: &str, accept: &str, limit: u64) -> Result<PageResponse, NetworkError> {
        let response = self.get(url, accept)?;
        if response.body.len() as u64 > limit {
            return Err(NetworkError("The page is too large to read.".into()));
        }
        Ok(PageResponse { response, url: None })
    }
    /// POST an `application/x-www-form-urlencoded` body.
    fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<Response, NetworkError>;
    /// Any request, answered as bytes. Clients that only read pages (test
    /// doubles) can leave it out.
    fn send(&self, request: Request) -> Result<BytesResponse, NetworkError> {
        Err(NetworkError(format!("{} {} cannot be sent by this client", request.method, request.url)))
    }
}

/// The real client: ureq with rustls and the bundled Mozilla roots.
pub struct UreqHttp {
    agent: ureq::Agent,
}

impl UreqHttp {
    pub fn new() -> Self {
        Self::with_timeout(Duration::from_secs(30))
    }

    /// A client whose requests may take up to `timeout` (a language model
    /// writing a long answer takes minutes).
    pub fn with_timeout(timeout: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .user_agent(concat!("Paperloom/", env!("CARGO_PKG_VERSION"), " (research literature workbench)"))
            .build()
            .into();
        Self { agent }
    }

    fn read(result: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<Response, NetworkError> {
        let mut response = result.map_err(|e| NetworkError(e.to_string()))?;
        let status = response.status().as_u16();
        let content_type =
            response.headers().get("content-type").and_then(|v| v.to_str().ok()).map(str::to_string);
        let body = response
            .body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| NetworkError(e.to_string()))?;
        Ok(Response { status, content_type, body })
    }
}

impl Default for UreqHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqHttp {
    fn read_bytes(
        result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
        limit: u64,
    ) -> Result<(BytesResponse, String), NetworkError> {
        use ureq::ResponseExt;
        let mut response = result.map_err(|e| NetworkError(e.to_string()))?;
        let status = response.status().as_u16();
        let url = response.get_uri().to_string();
        let body = response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|e| NetworkError(e.to_string()))?;
        Ok((BytesResponse { status, body }, url))
    }
}

impl Http for UreqHttp {
    fn get(&self, url: &str, accept: &str) -> Result<Response, NetworkError> {
        Self::read(self.agent.get(url).header("Accept", accept).call())
    }

    fn get_page(&self, url: &str, accept: &str, limit: u64) -> Result<PageResponse, NetworkError> {
        use ureq::ResponseExt;
        let mut response =
            self.agent.get(url).header("Accept", accept).call().map_err(|e| NetworkError(e.to_string()))?;
        let status = response.status().as_u16();
        let url = Some(response.get_uri().to_string());
        let content_type =
            response.headers().get("content-type").and_then(|v| v.to_str().ok()).map(str::to_string);
        let reader = response.body_mut().with_config().limit(limit.saturating_add(1)).reader();
        let body = page_body(reader, limit)?;
        Ok(PageResponse { response: Response { status, content_type, body }, url })
    }

    fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<Response, NetworkError> {
        Self::read(self.agent.post(url).header("Accept", "application/json").send_form(form.iter().copied()))
    }

    fn send(&self, request: Request) -> Result<BytesResponse, NetworkError> {
        self.send_with_limit(request, 1024 * 1024 * 1024).map(|(response, _)| response)
    }
}

impl UreqHttp {
    /// A bounded response and its final address after redirects.
    pub fn send_with_limit(
        &self,
        request: Request,
        limit: u64,
    ) -> Result<(BytesResponse, String), NetworkError> {
        let mut builder = ureq::http::Request::builder().method(request.method).uri(request.url.as_str());
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let invalid = |e: ureq::http::Error| NetworkError(e.to_string());
        match request.body {
            Some(body) => Self::read_bytes(self.agent.run(builder.body(body).map_err(invalid)?), limit),
            None => Self::read_bytes(self.agent.run(builder.body(()).map_err(invalid)?), limit),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Web;

    impl Http for Web {
        fn get(&self, url: &str, accept: &str) -> Result<Response, NetworkError> {
            assert_eq!(url, "https://example.org/");
            assert_eq!(accept, "text/html");
            Ok(Response { status: 200, content_type: Some("text/html".into()), body: "page".into() })
        }

        fn post_form(&self, _url: &str, _form: &[(&str, &str)]) -> Result<Response, NetworkError> {
            panic!("not used")
        }
    }

    #[test]
    fn existing_clients_can_read_bounded_pages() {
        let page = Web.get_page("https://example.org/", "text/html", 4).unwrap();
        assert_eq!(page.response.status, 200);
        assert_eq!(page.response.content_type.as_deref(), Some("text/html"));
        assert_eq!(page.response.body, "page");
        assert!(page.url.is_none());
        assert!(Web.get_page("https://example.org/", "text/html", 3).is_err());
    }

    #[test]
    fn bounds_decoded_bytes_and_accepts_pdf_binary() {
        assert_eq!(page_body(&b"page"[..], 4).unwrap(), "page");
        assert!(page_body(&b"large"[..], 4).is_err());
        assert_eq!(page_body(&b""[..], 0).unwrap(), "");
        assert!(page_body(&b"x"[..], 0).is_err());
        assert!(page_body(&b"%PDF-1.7\xff"[..], 20).unwrap().starts_with("%PDF-"));
        let mut reader = std::io::Cursor::new(vec![b'x'; 100]);
        assert!(page_body(&mut reader, 4).is_err());
        assert_eq!(reader.position(), 5);
    }
}
