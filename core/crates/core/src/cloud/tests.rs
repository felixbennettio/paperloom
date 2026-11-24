use std::collections::HashMap;
use std::sync::Mutex;

use paperloom_http::NetworkError;
use paperloom_sync_proto::{Device, LoggedChange, Role};
use serde_json::json;

use super::*;
use crate::{NewAnnotation, NewItem};

const SERVICE: &str = "https://cloud.test";

/// Paperloom Cloud in memory: one account, its libraries, their logs and
/// blobs; requests are checked for the token.
#[derive(Default)]
struct FakeCloud {
    state: Mutex<Fake>,
}

#[derive(Default)]
struct Fake {
    devices: Vec<Uuid>,
    libraries: Vec<CloudLibrary>,
    log: Vec<LoggedChange>,
    blobs: HashMap<String, Vec<u8>>,
}

fn answer(status: u16, value: Value) -> Result<BytesResponse, NetworkError> {
    Ok(BytesResponse { status, body: value.to_string().into_bytes() })
}

impl Http for FakeCloud {
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
        if !request.headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer token") {
            return answer(401, json!({ "error": "unauthorized", "message": "Sign in." }));
        }
        let path = request.url.strip_prefix(SERVICE).expect("the service's address").to_string();
        let body = request.body.unwrap_or_default();
        let mut fake = self.state.lock().unwrap();
        match (request.method, path.as_str()) {
            ("POST", "/v1/devices") => {
                let device: NewDevice = serde_json::from_slice(&body).unwrap();
                fake.devices.push(device.id);
                let device =
                    Device { id: device.id, name: device.name, platform: device.platform, created_at: 0 };
                answer(200, serde_json::to_value(device).unwrap())
            }
            ("GET", "/v1/libraries") => answer(200, serde_json::to_value(&fake.libraries).unwrap()),
            ("POST", "/v1/libraries") => {
                let new: NewLibrary = serde_json::from_slice(&body).unwrap();
                let library =
                    CloudLibrary { id: new.id, name: new.name, role: Role::Owner, head: 0, created_at: 0 };
                fake.libraries.push(library.clone());
                answer(200, serde_json::to_value(library).unwrap())
            }
            (method, path) => {
                let id = fake.libraries.first().map(|l| l.id.to_string()).unwrap_or_default();
                let rest = path.strip_prefix(&format!("/v1/libraries/{id}")).expect("the library's address");
                match (method, rest) {
                    ("POST", "/changes") => {
                        let push: Push = serde_json::from_slice(&body).unwrap();
                        let mut cursors = Vec::new();
                        for change in push.changes {
                            let cursor = fake.log.len() as u64 + 1;
                            fake.log.push(LoggedChange {
                                cursor,
                                device: change.device,
                                seq: change.seq,
                                payload: change.payload,
                            });
                            cursors.push(cursor);
                        }
                        let pushed = Pushed { head: fake.log.len() as u64, cursors };
                        answer(200, serde_json::to_value(pushed).unwrap())
                    }
                    ("GET", rest) if rest.starts_with("/changes?after=") => {
                        let after: u64 = rest["/changes?after=".len()..].parse().unwrap();
                        let changes: Vec<LoggedChange> =
                            fake.log.iter().filter(|c| c.cursor > after).cloned().collect();
                        let page =
                            ChangesPage { changes, head: fake.log.len() as u64, more: false, snapshot: None };
                        answer(200, serde_json::to_value(page).unwrap())
                    }
                    ("PUT", rest) if rest.starts_with("/blobs/") => {
                        let hash = &rest["/blobs/".len()..];
                        assert_eq!(BlobHash::of(&body).as_str(), hash, "a blob is named by its hash");
                        fake.blobs.insert(hash.to_string(), body);
                        Ok(BytesResponse { status: 204, body: Vec::new() })
                    }
                    ("GET", rest) if rest.starts_with("/blobs/") => {
                        match fake.blobs.get(&rest["/blobs/".len()..]) {
                            Some(bytes) => Ok(BytesResponse { status: 200, body: bytes.clone() }),
                            None => answer(404, json!({ "error": "not_found", "message": "No such blob" })),
                        }
                    }
                    other => panic!("unexpected request {other:?}"),
                }
            }
        }
    }
}

fn library(dir: &tempfile::TempDir) -> std::sync::Arc<PaperLibrary> {
    PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap()
}

fn sync(library: &PaperLibrary, cloud: &FakeCloud, device: &str) -> SyncReport {
    let this = ThisDevice { name: device, platform: "macos" };
    library.sync_through(cloud, &format!("{SERVICE}/"), "token", this, None).unwrap()
}

fn sync_notes(library: &PaperLibrary, cloud: &FakeCloud, device: &str, vault: &PaperVault) -> SyncReport {
    let this = ThisDevice { name: device, platform: "macos" };
    library.sync_through(cloud, &format!("{SERVICE}/"), "token", this, Some(vault)).unwrap()
}

#[test]
fn two_libraries_sync_through_the_service() {
    let cloud = FakeCloud::default();
    let (mac_dir, phone_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mac, phone) = (library(&mac_dir), library(&phone_dir));
    assert_eq!(mac.sync_status().unwrap().service, None);

    let item = mac
        .add_item(NewItem {
            item_type: String::new(),
            title: "Quantum thermometry in living cells".into(),
            authors: vec!["Georg Kucsko".into()],
            container_title: Some("Nature".into()),
            year: Some(2013),
            doi: Some("10.1038/nature12373".into()),
        })
        .unwrap();
    let pdf = mac_dir.path().join("Kucsko 2013.pdf");
    fs::write(&pdf, b"%PDF-1.7 the paper").unwrap();
    let file = mac.attach_file(item.id.clone(), pdf.display().to_string()).unwrap();
    mac.add_annotation(NewAnnotation {
        attachment_id: file.id.clone(),
        kind: "highlight".into(),
        color: "#ffd400".into(),
        page_index: 0,
        rects: vec![crate::AnnotationRect { x: 1.0, y: 2.0, width: 3.0, height: 4.0 }],
        text: Some("nanometre-scale thermometry".into()),
        comment: None,
        strokes: Vec::new(),
        font_size: None,
    })
    .unwrap();

    let report = sync(&mac, &cloud, "Mac");
    assert_eq!((report.sent, report.received), (1, 0));
    assert_eq!(cloud.state.lock().unwrap().blobs.len(), 1, "the PDF went up");
    let status = mac.sync_status().unwrap();
    assert_eq!((status.service.as_deref(), status.waiting), (Some(SERVICE), 0));
    assert!(status.last_synced.is_some());

    let report = sync(&phone, &cloud, "iPhone");
    assert_eq!((report.received, report.downloaded), (1, 1));
    assert_eq!(cloud.state.lock().unwrap().libraries.len(), 1, "the phone joined the Mac's library");
    let files = phone.attachments(item.id.clone()).unwrap();
    assert_eq!(fs::read(&files[0].file_path).unwrap(), b"%PDF-1.7 the paper");
    assert_eq!(phone.annotations(file.id.clone()).unwrap().len(), 1);
    let hits = phone.search_everything("thermometry".into(), vec![], 10).unwrap();
    assert!(hits.iter().any(|h| h.kind == "item"), "{hits:?}");
    assert!(hits.iter().any(|h| h.kind == "annotation"), "{hits:?}");

    // Changes go the other way too, and nothing is sent twice.
    phone.set_starred(item.id.clone(), true).unwrap();
    assert_eq!(phone.sync_status().unwrap().waiting, 1);
    assert_eq!(sync(&phone, &cloud, "iPhone").sent, 1);
    assert_eq!(sync(&mac, &cloud, "Mac").received, 1);
    assert!(mac.item_detail(item.id.clone()).unwrap().unwrap().starred);
    assert_eq!(sync(&mac, &cloud, "Mac").sent, 0);

    // A file deleted for good goes from the other device too.
    mac.remove_attachment(file.id.clone()).unwrap();
    sync(&mac, &cloud, "Mac");
    sync(&phone, &cloud, "iPhone");
    assert!(phone.attachments(item.id.clone()).unwrap().is_empty());
    assert!(!std::path::Path::new(&files[0].file_path).exists());
    assert_eq!(cloud.state.lock().unwrap().devices.len(), 2, "each device registered once");
}

#[test]
fn a_refused_token_asks_to_sign_in_again() {
    let cloud = FakeCloud::default();
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let mac = || ThisDevice { name: "Mac", platform: "macos" };
    let err = library.sync_through(&cloud, SERVICE, "expired", mac(), None).unwrap_err();
    assert!(matches!(err, CoreError::Account { .. }), "{err:?}");
    let memory = PaperLibrary::in_memory().unwrap();
    assert!(memory.sync_through(&cloud, SERVICE, "token", mac(), None).is_err());
}

#[test]
fn the_state_file_is_checked() {
    let dir = tempfile::tempdir().unwrap();
    let state = CloudState::new(SERVICE);
    let replica = Replica::new(state.sync.peer).unwrap();
    write_state(dir.path(), &state, &replica).unwrap();
    let (read, document) = read_state(dir.path()).unwrap().unwrap();
    assert_eq!((read.service, read.library), (state.service, state.library));
    assert!(Replica::load(&document, state.sync.peer).is_ok());
    assert_ne!(state.sync.peer, 0);
    fs::write(dir.path().join(STATE_FILE), b"PAPERLOOM-SYNC-1\n\xff").unwrap();
    assert!(matches!(read_state(dir.path()), Err(SyncError::Corrupt(_))));
    assert!(read_state(&dir.path().join("none")).unwrap().is_none());
}

#[test]
fn notes_sync_with_the_library() {
    let cloud = FakeCloud::default();
    let (mac_dir, phone_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mac, phone) = (library(&mac_dir), library(&phone_dir));
    let mac_vault = PaperVault::open(mac_dir.path().join("Vault").display().to_string()).unwrap();
    let phone_vault = PaperVault::open(phone_dir.path().join("Vault").display().to_string()).unwrap();
    mac_vault.write_note("Ideas/Thermometry.md".into(), "# Thermometry\n\nFirst idea.\n".into()).unwrap();

    assert_eq!(sync_notes(&mac, &cloud, "Mac", &mac_vault).sent, 1);
    let report = sync_notes(&phone, &cloud, "iPhone", &phone_vault);
    assert_eq!(report.notes, 1);
    assert_eq!(
        phone_vault.read_note("Ideas/Thermometry.md".into()).unwrap(),
        "# Thermometry\n\nFirst idea.\n"
    );

    // Edited on both at once (one of them outside the app): both edits stay.
    mac_vault
        .write_note("Ideas/Thermometry.md".into(), "# Thermometry\n\nFirst idea.\nFrom the Mac.\n".into())
        .unwrap();
    fs::write(phone_dir.path().join("Vault/Ideas/Thermometry.md"), "# Thermometry (phone)\n\nFirst idea.\n")
        .unwrap();
    sync_notes(&mac, &cloud, "Mac", &mac_vault);
    sync_notes(&phone, &cloud, "iPhone", &phone_vault);
    sync_notes(&mac, &cloud, "Mac", &mac_vault);
    let merged = "# Thermometry (phone)\n\nFirst idea.\nFrom the Mac.\n";
    assert_eq!(mac_vault.read_note("Ideas/Thermometry.md".into()).unwrap(), merged);
    assert_eq!(phone_vault.read_note("Ideas/Thermometry.md".into()).unwrap(), merged);
    assert_eq!(sync_notes(&mac, &cloud, "Mac", &mac_vault).sent, 0, "nothing new");

    // Deleted on the phone: moved to the Mac vault's trash too.
    phone_vault.delete_note("Ideas/Thermometry.md".into()).unwrap();
    sync_notes(&phone, &cloud, "iPhone", &phone_vault);
    sync_notes(&mac, &cloud, "Mac", &mac_vault);
    assert!(mac_vault.read_note("Ideas/Thermometry.md".into()).is_err());
}
