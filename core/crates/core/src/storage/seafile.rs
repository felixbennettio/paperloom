//! This shelf reaches Seafile libraries through the server's Web API v2.

use std::collections::BTreeSet;
use std::sync::Mutex;

use paperloom_http::{BytesResponse, Http, Request};
use paperloom_sync::SyncError;
use serde::Deserialize;
use uuid::Uuid;

use super::{SeafilePlace, Shelf};

pub(crate) struct Seafile<'a> {
    http: &'a dyn Http,
    server: String,
    token: String,
    repo: String,
    folder: String,
    made: Mutex<BTreeSet<String>>,
}

/// RFC 3986 unreserved bytes stay; all other UTF-8 bytes become `%XX`.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn failure(method: &str, path: &str, response: &BytesResponse) -> SyncError {
    SyncError::Transport(format!("Seafile {method} {path} answered HTTP {}.", response.status))
}

fn invalid_answer() -> SyncError {
    SyncError::Transport("The Seafile server returned an invalid answer.".into())
}

fn json<T: serde::de::DeserializeOwned>(response: &BytesResponse) -> Result<T, SyncError> {
    serde_json::from_slice(&response.body).map_err(|_| invalid_answer())
}

fn form(values: &[(&str, &str)]) -> Vec<u8> {
    values
        .iter()
        .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
        .into_bytes()
}

fn form_headers() -> Vec<(String, String)> {
    vec![("Content-Type".into(), "application/x-www-form-urlencoded".into())]
}

/// Transport errors may contain a signed URL or credentials, so their text
/// must not escape this module.
fn send(http: &dyn Http, request: Request) -> Result<BytesResponse, SyncError> {
    let method = request.method;
    let response =
        http.send(request).map_err(|_| SyncError::Transport(format!("Seafile {method} request failed.")))?;
    if response.status == 401 || response.status == 403 {
        return Err(SyncError::Transport(
            "The Seafile server did not accept the credentials or allow access.".into(),
        ));
    }
    Ok(response)
}

#[derive(Deserialize)]
struct Repository {
    #[serde(alias = "repo_id")]
    id: String,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct Entry {
    name: String,
}

impl<'a> Seafile<'a> {
    pub(crate) fn connect(
        http: &'a dyn Http,
        place: &SeafilePlace,
        password: &str,
    ) -> Result<Self, SyncError> {
        let token = if password.len() == 40 && password.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            password.to_string()
        } else {
            #[derive(Deserialize)]
            struct SignedIn {
                token: String,
            }
            let response = send(
                http,
                Request {
                    method: "POST",
                    url: format!("{}/api2/auth-token/", place.server),
                    headers: form_headers(),
                    body: Some(form(&[("username", &place.account), ("password", password)])),
                },
            )?;
            if !response.is_success() {
                return Err(failure("POST", "auth-token", &response));
            }
            let signed: SignedIn = json(&response)?;
            signed.token
        };
        if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(invalid_answer());
        }
        let mut shelf = Self {
            http,
            server: place.server.clone(),
            token,
            repo: String::new(),
            folder: place.folder.clone(),
            made: Mutex::new(BTreeSet::from(["/".to_string()])),
        };
        let response = shelf.api("GET", "repos/", Vec::new(), None)?;
        if !response.is_success() {
            return Err(failure("GET", "repos", &response));
        }
        let repos: Vec<Repository> = json(&response)?;
        let repo = match repos.into_iter().find(|repo| repo.name == place.library) {
            Some(repo) => repo,
            None => {
                let response =
                    shelf.api("POST", "repos/", form_headers(), Some(form(&[("name", &place.library)])))?;
                if !response.is_success() {
                    return Err(failure("POST", "repos", &response));
                }
                json::<Repository>(&response)?
            }
        };
        shelf.repo = Uuid::parse_str(&repo.id).map_err(|_| invalid_answer())?.to_string();
        Ok(shelf)
    }

    fn api(
        &self,
        method: &'static str,
        path: &str,
        mut headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    ) -> Result<BytesResponse, SyncError> {
        headers.push(("Authorization".into(), format!("Token {}", self.token)));
        headers.push(("Accept".into(), "application/json".into()));
        send(self.http, Request { method, url: format!("{}/api2/{path}", self.server), headers, body })
    }

    fn path(&self, path: &str) -> String {
        let parts: Vec<_> =
            [self.folder.as_str(), path].into_iter().filter(|part| !part.is_empty()).collect();
        format!("/{}", parts.join("/"))
    }

    fn endpoint(&self, resource: &str, path: &str) -> String {
        format!("repos/{}/{resource}/?p={}", self.repo, encode(path))
    }

    fn link(&self, resource: &str, path: &str) -> Result<Option<String>, SyncError> {
        let response = self.api("GET", &self.endpoint(resource, path), Vec::new(), None)?;
        if response.status == 404 {
            return Ok(None);
        }
        if !response.is_success() {
            return Err(failure("GET", path, &response));
        }
        let url: String = json(&response)?;
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(invalid_answer());
        }
        Ok(Some(url))
    }

    /// Checks each ancestor before making it and remembers existing folders
    /// for the rest of this sync round.
    fn make_folders(&self, parent: &str) -> Result<(), SyncError> {
        let mut folder = String::new();
        for part in parent.split('/').filter(|part| !part.is_empty()) {
            folder.push('/');
            folder.push_str(part);
            if self.made.lock().map_err(|e| SyncError::Local(e.to_string()))?.contains(&folder) {
                continue;
            }
            let endpoint = self.endpoint("dir", &folder);
            let response = self.api("GET", &endpoint, Vec::new(), None)?;
            if response.status == 404 {
                let response =
                    self.api("POST", &endpoint, form_headers(), Some(form(&[("operation", "mkdir")])))?;
                if !response.is_success() {
                    // Another device may have created this folder meanwhile.
                    if ![400, 409].contains(&response.status)
                        || !self.api("GET", &endpoint, Vec::new(), None)?.is_success()
                    {
                        return Err(failure("POST", &folder, &response));
                    }
                }
            } else if !response.is_success() {
                return Err(failure("GET", &folder, &response));
            }
            self.made.lock().map_err(|e| SyncError::Local(e.to_string()))?.insert(folder.clone());
        }
        Ok(())
    }
}

impl Shelf for Seafile<'_> {
    fn put(&self, path: &str, bytes: Vec<u8>) -> Result<(), SyncError> {
        let path = self.path(path);
        let (parent, name) = path.rsplit_once('/').ok_or_else(invalid_answer)?;
        if name.is_empty() || name.contains(['\r', '\n', '"', '\\']) {
            return Err(SyncError::Transport("Invalid Seafile file name.".into()));
        }
        let parent = if parent.is_empty() { "/" } else { parent };
        self.make_folders(parent)?;
        let url = self
            .link("upload-link", parent)?
            .ok_or_else(|| SyncError::Transport("The Seafile upload folder is missing.".into()))?;
        let boundary = format!("paperloom-{}", Uuid::now_v7());
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"parent_dir\"\r\n\r\n{parent}\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"replace\"\r\n\r\n1\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        ).into_bytes();
        body.extend(bytes);
        body.extend(format!("\r\n--{boundary}--\r\n").bytes());
        // File links carry their own scoped access token, even on another
        // host. The account token is sent only to the API server.
        let response = send(
            self.http,
            Request {
                method: "POST",
                url,
                headers: vec![("Content-Type".into(), format!("multipart/form-data; boundary={boundary}"))],
                body: Some(body),
            },
        )?;
        if !response.is_success() {
            return Err(failure("POST", &path, &response));
        }
        Ok(())
    }

    fn get(&self, path: &str) -> Result<Option<Vec<u8>>, SyncError> {
        let path = self.path(path);
        let Some(url) = self.link("file", &path)? else { return Ok(None) };
        let response = send(self.http, Request { method: "GET", url, headers: Vec::new(), body: None })?;
        match response.status {
            404 => Ok(None),
            _ if response.is_success() => Ok(Some(response.body)),
            _ => Err(failure("GET", &path, &response)),
        }
    }

    fn exists(&self, path: &str) -> Result<bool, SyncError> {
        Ok(self.link("file", &self.path(path))?.is_some())
    }

    fn list(&self, folder: &str) -> Result<Vec<String>, SyncError> {
        let path = self.path(folder);
        let response = self.api("GET", &self.endpoint("dir", &path), Vec::new(), None)?;
        if response.status == 404 {
            return Ok(Vec::new());
        }
        if !response.is_success() {
            return Err(failure("GET", &path, &response));
        }
        let entries: Vec<Entry> = json(&response)?;
        Ok(entries.into_iter().map(|entry| entry.name).collect())
    }
}
