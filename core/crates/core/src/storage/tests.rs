use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use paperloom_http::NetworkError;

use super::*;
use crate::NewItem;

fn library(dir: &tempfile::TempDir) -> Arc<PaperLibrary> {
    PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap()
}

fn vault(dir: &tempfile::TempDir) -> Arc<PaperVault> {
    PaperVault::open(dir.path().join("Vault").display().to_string()).unwrap()
}

fn paper(title: &str) -> NewItem {
    NewItem {
        item_type: String::new(),
        title: title.into(),
        authors: vec!["Marie Curie".into()],
        container_title: None,
        year: Some(1903),
        doi: None,
    }
}

#[test]
fn two_libraries_sync_through_a_shared_folder() {
    let shared = tempfile::tempdir().unwrap();
    let folder = shared.path().join("Paperloom").display().to_string();
    let (mac_dir, pc_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mac, pc) = (library(&mac_dir), library(&pc_dir));
    let (mac_notes, pc_notes) = (vault(&mac_dir), vault(&pc_dir));

    let item = mac.add_item(paper("Radioactive substances")).unwrap();
    let pdf = mac_dir.path().join("curie.pdf");
    fs::write(&pdf, b"%PDF-1.4 thesis").unwrap();
    mac.attach_file(item.id.clone(), pdf.display().to_string()).unwrap();
    mac_notes.write_note("Curie.md".into(), "Polonium, then radium.\n".into()).unwrap();

    let report = mac.sync_with_folder(folder.clone(), Some(mac_notes.clone())).unwrap();
    assert_eq!(report.sent, 1);
    let devices: Vec<_> = fs::read_dir(shared.path().join("Paperloom/changes")).unwrap().collect();
    assert_eq!(devices.len(), 1, "one folder per device");

    let report = pc.sync_with_folder(folder.clone(), Some(pc_notes.clone())).unwrap();
    assert_eq!((report.received, report.downloaded, report.notes), (1, 1, 1));
    let files = pc.attachments(item.id.clone()).unwrap();
    assert_eq!(fs::read(&files[0].file_path).unwrap(), b"%PDF-1.4 thesis");
    assert_eq!(pc_notes.read_note("Curie.md".into()).unwrap(), "Polonium, then radium.\n");

    pc.set_starred(item.id.clone(), true).unwrap();
    pc.sync_with_folder(folder.clone(), Some(pc_notes)).unwrap();
    assert_eq!(mac.sync_with_folder(folder.clone(), Some(mac_notes.clone())).unwrap().received, 1);
    assert!(mac.item_detail(item.id.clone()).unwrap().unwrap().starred);
    let status = mac.sync_status().unwrap();
    assert_eq!(status.service, Some(format!("folder:{folder}")));
    assert!(mac.sync_with_folder("  ".into(), None).is_err());
}

/// Every file under a folder, with its bytes.
fn files_in(folder: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    for entry in fs::read_dir(folder).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_in(&path));
        } else {
            found.push((path.clone(), fs::read(&path).unwrap()));
        }
    }
    found
}

#[test]
fn sealed_libraries_share_only_ciphertext() {
    let cheap = paperloom_sync::seal::Cost { memory_kib: 64, passes: 1 };
    let shared = tempfile::tempdir().unwrap();
    let folder = shared.path().join("Paperloom").display().to_string();
    let (mac_dir, pc_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mac, pc) = (library(&mac_dir), library(&pc_dir));
    let (mac_notes, pc_notes) = (vault(&mac_dir), vault(&pc_dir));
    mac.seal_with(Some("correct horse".into()), cheap).unwrap();
    pc.seal_with(Some("correct horse".into()), cheap).unwrap();
    assert!(mac.sync_sealed() && pc.sync_sealed());

    let item = mac.add_item(paper("Radioactive substances")).unwrap();
    let pdf = mac_dir.path().join("curie.pdf");
    fs::write(&pdf, b"%PDF-1.4 thesis").unwrap();
    mac.attach_file(item.id.clone(), pdf.display().to_string()).unwrap();
    mac_notes.write_note("Curie.md".into(), "Polonium, then radium.\n".into()).unwrap();
    assert_eq!(mac.sync_with_folder(folder.clone(), Some(mac_notes.clone())).unwrap().sent, 1);

    let kept = files_in(shared.path());
    assert!(kept.len() >= 2, "a change and a file: {kept:?}");
    for (path, bytes) in &kept {
        assert!(paperloom_sync::seal::is_sealed(bytes), "{} is sealed", path.display());
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains("Radioactive") && !text.contains("PDF") && !text.contains("Polonium"));
    }

    let report = pc.sync_with_folder(folder.clone(), Some(pc_notes.clone())).unwrap();
    assert_eq!((report.received, report.downloaded, report.notes), (1, 1, 1));
    let files = pc.attachments(item.id.clone()).unwrap();
    assert_eq!(fs::read(&files[0].file_path).unwrap(), b"%PDF-1.4 thesis");
    assert_eq!(pc_notes.read_note("Curie.md".into()).unwrap(), "Polonium, then radium.\n");

    let other_dir = tempfile::tempdir().unwrap();
    let stranger = library(&other_dir);
    stranger.add_item(paper("Plain")).unwrap();
    let locked = stranger.sync_with_folder(folder.clone(), None);
    assert!(matches!(locked, Err(CoreError::InvalidInput { .. })), "{locked:?}");
    stranger.seal_with(Some("battery staple".into()), cheap).unwrap();
    assert!(stranger.sync_with_folder(folder, None).is_err(), "another passphrase opens nothing");
    assert_eq!(files_in(shared.path()).len(), kept.len(), "and sends nothing");
}

/// A WebDAV server in memory, for `me` with password `secret`.
#[derive(Default)]
struct FakeDav {
    folders: Mutex<BTreeSet<String>>,
    files: Mutex<BTreeMap<String, Vec<u8>>>,
}

const ROOT: &str = "https://dav.test";

impl FakeDav {
    fn new() -> Self {
        let dav = Self::default();
        dav.folders.lock().unwrap().insert("/dav".into());
        dav
    }

    fn status(status: u16) -> Result<BytesResponse, NetworkError> {
        Ok(BytesResponse { status, body: Vec::new() })
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

impl Http for FakeDav {
    fn get(&self, _url: &str, _accept: &str) -> Result<paperloom_http::Response, NetworkError> {
        Err(NetworkError("not used".into()))
    }

    fn post_form(
        &self,
        _url: &str,
        _form: &[(&str, &str)],
    ) -> Result<paperloom_http::Response, NetworkError> {
        Err(NetworkError("not used".into()))
    }

    fn send(&self, request: Request) -> Result<BytesResponse, NetworkError> {
        let expected = format!("Basic {}", STANDARD.encode("me:secret"));
        if !request.headers.iter().any(|(k, v)| k == "Authorization" && *v == expected) {
            return Self::status(401);
        }
        let path = request.url.strip_prefix(ROOT).expect("the server's address");
        let path = path.trim_end_matches('/').to_string();
        let mut folders = self.folders.lock().unwrap();
        let mut files = self.files.lock().unwrap();
        match request.method {
            "MKCOL" if folders.contains(&path) => Self::status(405),
            "MKCOL" if !folders.contains(parent(&path)) => Self::status(409),
            "MKCOL" => {
                folders.insert(path);
                Self::status(201)
            }
            "PUT" if !folders.contains(parent(&path)) => Self::status(409),
            "PUT" => {
                files.insert(path, request.body.unwrap_or_default());
                Self::status(201)
            }
            "GET" => match files.get(&path) {
                Some(bytes) => Ok(BytesResponse { status: 200, body: bytes.clone() }),
                None => Self::status(404),
            },
            "HEAD" => Self::status(if files.contains_key(&path) { 200 } else { 404 }),
            "PROPFIND" => {
                assert!(request.headers.iter().any(|(k, v)| k == "Depth" && v == "1"));
                if !folders.contains(&path) {
                    return Self::status(404);
                }
                // As Nextcloud answers: the folder itself first, prefixed tags.
                let mut xml = format!(
                    r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:"><d:response><d:href>{path}/</d:href></d:response>"#
                );
                for folder in folders.iter().filter(|f| parent(f) == path) {
                    xml.push_str(&format!("<d:response><d:href>{folder}/</d:href></d:response>"));
                }
                for file in files.keys().filter(|f| parent(f) == path) {
                    xml.push_str(&format!("<d:response><d:href>{ROOT}{file}</d:href></d:response>"));
                }
                xml.push_str("</d:multistatus>");
                Ok(BytesResponse { status: 207, body: xml.into_bytes() })
            }
            other => panic!("unexpected {other} {path}"),
        }
    }
}

#[test]
fn two_libraries_sync_through_webdav() {
    let dav = FakeDav::new();
    let address = format!("{ROOT}/dav/Paperloom/");
    let (mac_dir, phone_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mac, phone) = (library(&mac_dir), library(&phone_dir));
    let item = mac.add_item(paper("Recherches sur les substances radioactives")).unwrap();
    let pdf = mac_dir.path().join("these.pdf");
    fs::write(&pdf, b"%PDF-1.4 these").unwrap();
    mac.attach_file(item.id.clone(), pdf.display().to_string()).unwrap();

    let wrong = mac.sync_webdav_through(&dav, &address, "me", "guess", None).unwrap_err();
    assert!(wrong.to_string().contains("password"), "{wrong}");
    assert_eq!(mac.sync_webdav_through(&dav, &address, "me", "secret", None).unwrap().sent, 1);
    assert!(dav.folders.lock().unwrap().contains("/dav/Paperloom/changes"));
    assert_eq!(dav.files.lock().unwrap().keys().filter(|f| f.contains("/blobs/")).count(), 1);

    let report = phone.sync_webdav_through(&dav, &address, "me", "secret", None).unwrap();
    assert_eq!((report.received, report.downloaded), (1, 1));
    assert_eq!(
        phone.item_detail(item.id.clone()).unwrap().unwrap().title,
        "Recherches sur les substances radioactives"
    );
    assert_eq!(phone.sync_webdav_through(&dav, &address, "me", "secret", None).unwrap().received, 0);
    assert!(phone.sync_webdav_through(&dav, "ftp://dav.test", "me", "secret", None).is_err());
}

#[test]
fn reads_webdav_listings_from_different_servers() {
    // 坚果云 answers without a prefix and with absolute addresses; others
    // use D: or d:, and escape names.
    let jianguoyun = r#"<multistatus xmlns="DAV:"><response><href>/dav/Paperloom/changes/</href></response><response><href>/dav/Paperloom/changes/0192f0aa-0000-7000-8000-000000000001/</href></response></multistatus>"#;
    let names: Vec<String> = hrefs(jianguoyun).iter().map(|h| url_path(h)).collect();
    assert_eq!(
        names,
        ["/dav/Paperloom/changes", "/dav/Paperloom/changes/0192f0aa-0000-7000-8000-000000000001"]
    );
    let synology = r#"<D:multistatus xmlns:D="DAV:"><D:response><D:href xmlns:D="DAV:">https://nas.local:5006/Paperloom/blobs/ab/</D:href></D:response></D:multistatus>"#;
    assert_eq!(hrefs(synology), ["https://nas.local:5006/Paperloom/blobs/ab/"]);
    assert_eq!(url_path("https://nas.local:5006/Paperloom/blobs/ab/"), "/Paperloom/blobs/ab");
    assert_eq!(percent_decode("/dav/%E7%BB%87%E5%8D%B7/a%20b%zz"), "/dav/织卷/a b%zz");
}

/// Objects in one bucket, answered the way S3 does (path-style).
#[derive(Default)]
struct FakeS3 {
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl Http for FakeS3 {
    fn get(&self, _url: &str, _accept: &str) -> Result<paperloom_http::Response, NetworkError> {
        Err(NetworkError("not used".into()))
    }

    fn post_form(
        &self,
        _url: &str,
        _form: &[(&str, &str)],
    ) -> Result<paperloom_http::Response, NetworkError> {
        Err(NetworkError("not used".into()))
    }

    fn send(&self, request: Request) -> Result<BytesResponse, NetworkError> {
        let header = |name: &str| request.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
        let signed = header("Authorization").unwrap_or_default();
        let scope = signed.split("Credential=").nth(1).and_then(|c| c.split(',').next()).unwrap_or_default();
        if !scope.starts_with("AKID/") || !scope.ends_with("/eu-west-1/s3/aws4_request") {
            return Ok(BytesResponse {
                status: 403,
                body: b"<Error><Code>SignatureDoesNotMatch</Code></Error>".to_vec(),
            });
        }
        assert!(header("x-amz-date").is_some() && header("x-amz-content-sha256").is_some());
        let rest = request.url.strip_prefix("https://minio.test/library/").expect("path-style address");
        let (key, query) = rest.split_once('?').unwrap_or((rest, ""));
        let mut objects = self.objects.lock().unwrap();
        match request.method {
            "PUT" => {
                objects.insert(key.to_string(), request.body.unwrap_or_default());
                Ok(BytesResponse { status: 200, body: Vec::new() })
            }
            "GET" if key.is_empty() => {
                let prefix = query
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("prefix="))
                    .map(|p| p.replace("%2F", "/"))
                    .unwrap_or_default();
                let mut xml = String::from("<ListBucketResult><IsTruncated>false</IsTruncated>");
                let mut folders = BTreeSet::new();
                for name in objects.keys().filter_map(|k| k.strip_prefix(&prefix)) {
                    match name.split_once('/') {
                        Some((folder, _)) => {
                            folders.insert(format!("{prefix}{folder}/"));
                        }
                        None => xml.push_str(&format!("<Contents><Key>{prefix}{name}</Key></Contents>")),
                    }
                }
                for folder in folders {
                    xml.push_str(&format!("<CommonPrefixes><Prefix>{folder}</Prefix></CommonPrefixes>"));
                }
                xml.push_str("</ListBucketResult>");
                Ok(BytesResponse { status: 200, body: xml.into_bytes() })
            }
            "GET" => Ok(match objects.get(key) {
                Some(bytes) => BytesResponse { status: 200, body: bytes.clone() },
                None => BytesResponse { status: 404, body: Vec::new() },
            }),
            "HEAD" => Ok(BytesResponse {
                status: if objects.contains_key(key) { 200 } else { 404 },
                body: Vec::new(),
            }),
            other => panic!("unexpected {other} {key}"),
        }
    }
}

#[test]
fn two_libraries_sync_through_s3() {
    let s3 = FakeS3::default();
    let place = S3Place {
        endpoint: "https://minio.test/".into(),
        region: "eu-west-1".into(),
        bucket: "library".into(),
        prefix: "/paperloom/".into(),
        access_key_id: "AKID".into(),
    };
    let (mac_dir, phone_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mac, phone) = (library(&mac_dir), library(&phone_dir));
    let item = mac.add_item(paper("Recherches sur les substances radioactives")).unwrap();
    let pdf = mac_dir.path().join("these.pdf");
    fs::write(&pdf, b"%PDF-1.4 these").unwrap();
    mac.attach_file(item.id.clone(), pdf.display().to_string()).unwrap();

    let elsewhere = S3Place { region: "us-east-1".into(), ..place.clone() };
    let wrong = mac.sync_s3_through(&s3, &elsewhere, "secret", None).unwrap_err();
    assert!(wrong.to_string().contains("did not accept the keys"), "{wrong}");
    assert!(mac.sync_s3_through(&s3, &place, " ", None).is_err(), "a secret is needed");

    assert_eq!(mac.sync_s3_through(&s3, &place, "secret", None).unwrap().sent, 1);
    let keys: Vec<String> = s3.objects.lock().unwrap().keys().cloned().collect();
    assert!(keys.iter().all(|k| k.starts_with("paperloom/")), "{keys:?}");
    assert_eq!(keys.iter().filter(|k| k.starts_with("paperloom/blobs/")).count(), 1);

    let report = phone.sync_s3_through(&s3, &place, "secret", None).unwrap();
    assert_eq!((report.received, report.downloaded), (1, 1));
    assert_eq!(
        phone.item_detail(item.id.clone()).unwrap().unwrap().title,
        "Recherches sur les substances radioactives"
    );
    assert_eq!(phone.sync_s3_through(&s3, &place, "secret", None).unwrap().received, 0);
}

/// A Seafile API and its separate file server answer requests in memory.
#[derive(Default)]
struct FakeSeafile {
    library_name: Mutex<Option<String>>,
    folders: Mutex<BTreeSet<String>>,
    files: Mutex<BTreeMap<String, Vec<u8>>>,
    requests: Mutex<Vec<Request>>,
    download_missing: bool,
    file_network_failure: bool,
    failure: Option<BytesResponse>,
    network_failure: bool,
}

const SEAFILE_SERVER: &str = "https://seafile.test";
const SEAFILE_FILES: &str = "https://files.seafile.test";
const SEAFILE_REPO: &str = "0192f0aa-0000-7000-8000-000000000001";
const SEAFILE_TOKEN: &str = "0123456789abcdef0123456789abcdef01234567";
const SEAFILE_PASSWORD: &str = "p&ss+ word";

fn seafile_place() -> SeafilePlace {
    SeafilePlace {
        server: SEAFILE_SERVER.into(),
        account: "me+中文@example.com".into(),
        library: "中文资料库".into(),
        folder: "中文路径".into(),
    }
}

fn form_value(body: &[u8], key: &str) -> String {
    String::from_utf8_lossy(body)
        .split('&')
        .find_map(|pair| pair.strip_prefix(&format!("{key}=")))
        .map(percent_decode)
        .unwrap_or_default()
}

impl FakeSeafile {
    fn answer(value: serde_json::Value) -> Result<BytesResponse, NetworkError> {
        Ok(BytesResponse { status: 200, body: serde_json::to_vec(&value).unwrap() })
    }
}

impl Http for FakeSeafile {
    fn get(&self, _url: &str, _accept: &str) -> Result<paperloom_http::Response, NetworkError> {
        Err(NetworkError("not used".into()))
    }

    fn post_form(
        &self,
        _url: &str,
        _form: &[(&str, &str)],
    ) -> Result<paperloom_http::Response, NetworkError> {
        Err(NetworkError("not used".into()))
    }

    fn send(&self, request: Request) -> Result<BytesResponse, NetworkError> {
        self.requests.lock().unwrap().push(request.clone());
        if self.network_failure {
            return Err(NetworkError(format!("{} {SEAFILE_PASSWORD} {SEAFILE_TOKEN}", request.url)));
        }
        if let Some(response) = &self.failure {
            return Ok(response.clone());
        }
        if let Some(rest) = request.url.strip_prefix(SEAFILE_FILES) {
            assert!(!request.headers.iter().any(|(key, _)| key == "Authorization"));
            if self.file_network_failure {
                return Err(NetworkError(format!("{} {SEAFILE_TOKEN}", request.url)));
            }
            if rest.starts_with("/download?p=") {
                let path = percent_decode(rest.strip_prefix("/download?p=").unwrap());
                return if self.download_missing {
                    FakeDav::status(404)
                } else {
                    Ok(BytesResponse {
                        status: 200,
                        body: self.files.lock().unwrap().get(&path).unwrap().clone(),
                    })
                };
            }
            assert_eq!(request.method, "POST");
            let content_type = &request.headers.iter().find(|(key, _)| key == "Content-Type").unwrap().1;
            let boundary = content_type.strip_prefix("multipart/form-data; boundary=").unwrap();
            let bytes = request.body.unwrap();
            let text = String::from_utf8_lossy(&bytes);
            let parent =
                text.split("name=\"parent_dir\"\r\n\r\n").nth(1).unwrap().split("\r\n").next().unwrap();
            assert_eq!(parent, percent_decode(rest.strip_prefix("/upload?p=").unwrap()));
            assert!(self.folders.lock().unwrap().contains(parent) || parent == "/");
            assert!(text.contains("name=\"replace\"\r\n\r\n1\r\n"));
            let name = text.split("filename=\"").nth(1).unwrap().split('"').next().unwrap();
            let marker = b"Content-Type: application/octet-stream\r\n\r\n";
            let start = bytes.windows(marker.len()).position(|part| part == marker).unwrap() + marker.len();
            let end = bytes.len() - format!("\r\n--{boundary}--\r\n").len();
            let path = format!("{}/{name}", parent.trim_end_matches('/'));
            self.files.lock().unwrap().insert(path, bytes[start..end].to_vec());
            return FakeDav::status(200);
        }
        let rest = request.url.strip_prefix(&format!("{SEAFILE_SERVER}/api2/")).unwrap();
        if rest == "auth-token/" {
            assert_eq!(request.method, "POST");
            let body = request.body.unwrap();
            assert!(
                String::from_utf8_lossy(&body).contains("username=me%2B%E4%B8%AD%E6%96%87%40example.com")
            );
            return if form_value(&body, "password") == SEAFILE_PASSWORD {
                Self::answer(serde_json::json!({"token": SEAFILE_TOKEN}))
            } else {
                FakeDav::status(401)
            };
        }
        assert!(request.headers.contains(&("Authorization".into(), format!("Token {SEAFILE_TOKEN}"))));
        if rest == "repos/" {
            let mut name = self.library_name.lock().unwrap();
            if request.method == "POST" {
                assert!(name.is_none());
                *name = Some(form_value(request.body.as_ref().unwrap(), "name"));
                return Self::answer(serde_json::json!({"repo_id": SEAFILE_REPO}));
            }
            return Self::answer(match name.as_ref() {
                Some(name) => serde_json::json!([
                    {"id": "0192f0aa-0000-7000-8000-000000000002", "name": "Another library"},
                    {"id": SEAFILE_REPO, "name": name}
                ]),
                None => serde_json::json!([]),
            });
        }
        let rest = rest.strip_prefix(&format!("repos/{SEAFILE_REPO}/")).unwrap();
        let (resource, path) = rest.split_once("/?p=").unwrap();
        let path = percent_decode(path);
        match (request.method, resource) {
            ("GET", "file") => {
                if !self.files.lock().unwrap().contains_key(&path) {
                    return FakeDav::status(404);
                }
                Self::answer(serde_json::json!(format!(
                    "{SEAFILE_FILES}/download?p={}",
                    rest.split_once("?p=").unwrap().1
                )))
            }
            ("GET", "upload-link") => Self::answer(serde_json::json!(format!(
                "{SEAFILE_FILES}/upload?p={}",
                rest.split_once("?p=").unwrap().1
            ))),
            ("POST", "dir") => {
                assert_eq!(form_value(request.body.as_ref().unwrap(), "operation"), "mkdir");
                let mut folders = self.folders.lock().unwrap();
                assert!(parent(&path).is_empty() || folders.contains(parent(&path)));
                assert!(folders.insert(path), "existing folders must not be made again");
                FakeDav::status(201)
            }
            ("GET", "dir") => {
                let folders = self.folders.lock().unwrap();
                if path != "/" && !folders.contains(&path) {
                    return FakeDav::status(404);
                }
                let files = self.files.lock().unwrap();
                let entries: Vec<_> = folders
                    .iter()
                    .chain(files.keys())
                    .filter(|entry| parent(entry) == path || (path == "/" && parent(entry).is_empty()))
                    .map(|entry| serde_json::json!({"name": entry.rsplit('/').next().unwrap()}))
                    .collect();
                Self::answer(serde_json::json!(entries))
            }
            other => panic!("unexpected Seafile request: {other:?}"),
        }
    }
}

#[test]
fn seafile_signs_in_creates_a_library_and_uses_account_tokens() {
    let http = FakeSeafile::default();
    let place = seafile_place();
    seafile::Seafile::connect(&http, &place, SEAFILE_PASSWORD).unwrap();
    assert_eq!(http.library_name.lock().unwrap().as_deref(), Some(place.library.as_str()));
    assert_eq!(http.requests.lock().unwrap().len(), 3);
    http.requests.lock().unwrap().clear();
    seafile::Seafile::connect(&http, &place, SEAFILE_TOKEN).unwrap();
    let requests = http.requests.lock().unwrap();
    assert_eq!(requests.len(), 1, "a token skips sign-in and an existing library skips creation");
    assert_eq!(requests[0].method, "GET");
}

#[test]
fn seafile_put_get_exists_and_list_encode_paths_and_handle_missing_files() {
    let http = FakeSeafile::default();
    let shelf = seafile::Seafile::connect(&http, &seafile_place(), SEAFILE_PASSWORD).unwrap();
    assert!(shelf.get("missing").unwrap().is_none());
    assert!(!shelf.exists("missing").unwrap());
    assert!(shelf.list("missing").unwrap().is_empty());
    let path = "子目录/a b+%?#.change";
    let bytes = vec![0, 255, 13, 10, 42];
    shelf.put(path, bytes.clone()).unwrap();
    assert_eq!(shelf.get(path).unwrap(), Some(bytes));
    assert!(shelf.exists(path).unwrap());
    assert_eq!(shelf.list("").unwrap(), ["子目录"]);
    assert_eq!(shelf.list("子目录").unwrap(), ["a b+%?#.change"]);
    shelf.put(path, b"replacement".to_vec()).unwrap();
    assert_eq!(shelf.get(path).unwrap(), Some(b"replacement".to_vec()));
    let requests = http.requests.lock().unwrap();
    assert_eq!(requests.iter().filter(|r| r.method == "POST" && r.url.contains("/dir/")).count(), 2);
    assert!(requests.iter().any(|r| r.url.ends_with(
        "file/?p=%2F%E4%B8%AD%E6%96%87%E8%B7%AF%E5%BE%84%2F%E5%AD%90%E7%9B%AE%E5%BD%95%2Fa%20b%2B%25%3F%23.change"
    )));
    drop(requests);
    let http = FakeSeafile { download_missing: true, ..http };
    let shelf = seafile::Seafile::connect(&http, &seafile_place(), SEAFILE_TOKEN).unwrap();
    assert!(shelf.get(path).unwrap().is_none(), "a file can disappear after its link is fetched");
}

#[test]
fn seafile_uses_the_library_top_without_creating_it() {
    let http = FakeSeafile::default();
    let place = SeafilePlace { folder: String::new(), ..seafile_place() };
    let shelf = seafile::Seafile::connect(&http, &place, SEAFILE_PASSWORD).unwrap();
    shelf.put("top.txt", b"at the top".to_vec()).unwrap();
    assert_eq!(shelf.get("top.txt").unwrap(), Some(b"at the top".to_vec()));
    assert!(shelf.exists("top.txt").unwrap());
    assert_eq!(shelf.list("").unwrap(), ["top.txt"]);
    assert!(http.folders.lock().unwrap().is_empty());
    assert!(http.requests.lock().unwrap().iter().any(|r| r.url.ends_with("upload-link/?p=%2F")));
}

#[test]
fn seafile_errors_never_expose_credentials_or_signed_urls() {
    for status in [400, 401, 403, 500] {
        let http = FakeSeafile {
            failure: Some(BytesResponse {
                status,
                body: format!("{SEAFILE_PASSWORD} {SEAFILE_TOKEN}").into_bytes(),
            }),
            ..FakeSeafile::default()
        };
        let error =
            seafile::Seafile::connect(&http, &seafile_place(), SEAFILE_PASSWORD).err().unwrap().to_string();
        assert!(!error.contains(SEAFILE_PASSWORD) && !error.contains(SEAFILE_TOKEN));
        assert!(error.contains("Seafile"));
    }
    let http = FakeSeafile { network_failure: true, ..FakeSeafile::default() };
    let error =
        seafile::Seafile::connect(&http, &seafile_place(), SEAFILE_PASSWORD).err().unwrap().to_string();
    assert_eq!(error, "Seafile POST request failed.");
    let http = FakeSeafile {
        failure: Some(BytesResponse { status: 200, body: SEAFILE_TOKEN.as_bytes().to_vec() }),
        ..FakeSeafile::default()
    };
    let error =
        seafile::Seafile::connect(&http, &seafile_place(), SEAFILE_PASSWORD).err().unwrap().to_string();
    assert!(!error.contains(SEAFILE_TOKEN));

    let http = FakeSeafile::default();
    let shelf = seafile::Seafile::connect(&http, &seafile_place(), SEAFILE_PASSWORD).unwrap();
    shelf.put("existing.txt", b"contents".to_vec()).unwrap();
    let http = FakeSeafile { file_network_failure: true, ..http };
    let shelf = seafile::Seafile::connect(&http, &seafile_place(), SEAFILE_TOKEN).unwrap();
    assert_eq!(shelf.get("existing.txt").unwrap_err().to_string(), "Seafile GET request failed.");
    assert_eq!(
        shelf.put("existing.txt", b"replacement".to_vec()).unwrap_err().to_string(),
        "Seafile POST request failed."
    );
}

#[test]
fn two_libraries_sync_through_seafile() {
    let http = FakeSeafile::default();
    let place = seafile_place();
    let (mac_dir, phone_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mac, phone) = (library(&mac_dir), library(&phone_dir));
    let (mac_notes, phone_notes) = (vault(&mac_dir), vault(&phone_dir));
    let item = mac.add_item(paper("Radioactive substances")).unwrap();
    let pdf = mac_dir.path().join("curie.pdf");
    fs::write(&pdf, b"%PDF-1.4 thesis").unwrap();
    mac.attach_file(item.id.clone(), pdf.display().to_string()).unwrap();
    mac_notes.write_note("Curie.md".into(), "Polonium, then radium.\n".into()).unwrap();
    assert!(mac.sync_seafile_through(&http, &place, "wrong", None).is_err());
    let invalid = SeafilePlace { server: "ftp://seafile.test".into(), ..place.clone() };
    assert!(mac.sync_seafile_through(&http, &invalid, SEAFILE_PASSWORD, None).is_err());
    let invalid = SeafilePlace { folder: "../outside".into(), ..place.clone() };
    assert!(mac.sync_seafile_through(&http, &invalid, SEAFILE_PASSWORD, None).is_err());
    assert!(mac.sync_seafile_through(&http, &place, "", None).is_err());
    assert_eq!(mac.sync_seafile_through(&http, &place, SEAFILE_PASSWORD, Some(&mac_notes)).unwrap().sent, 1);
    let report = phone.sync_seafile_through(&http, &place, SEAFILE_TOKEN, Some(&phone_notes)).unwrap();
    assert_eq!((report.received, report.downloaded, report.notes), (1, 1, 1));
    assert_eq!(phone.item_detail(item.id.clone()).unwrap().unwrap().title, "Radioactive substances");
    let attachments = phone.attachments(item.id).unwrap();
    assert_eq!(fs::read(&attachments[0].file_path).unwrap(), b"%PDF-1.4 thesis");
    assert_eq!(phone_notes.read_note("Curie.md".into()).unwrap(), "Polonium, then radium.\n");
    assert_eq!(
        phone.sync_seafile_through(&http, &place, SEAFILE_TOKEN, Some(&phone_notes)).unwrap().received,
        0
    );
    assert_eq!(
        phone.sync_status().unwrap().service,
        Some(format!("seafile:{SEAFILE_SERVER}/中文资料库/中文路径"))
    );
}
