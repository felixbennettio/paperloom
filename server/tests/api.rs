//! The API end to end, with the database and blobs in memory and fixed
//! tokens. The same rules hold in PostgreSQL (see tests/postgres.rs).

use std::collections::HashMap;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use paperloom_server::AppState;
use paperloom_server::api::router;
use paperloom_server::auth::{Authenticator, Identity};
use paperloom_server::blobs::Blobs;
use paperloom_server::db::Database;
use paperloom_sync_proto::BlobHash;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

fn identity(sub: &str, scopes: &str) -> Identity {
    Identity { sub: sub.into(), scopes: scopes.split_whitespace().map(str::to_string).collect() }
}

pub fn app(db: Database) -> Router {
    let tokens = HashMap::from([
        ("alice".to_string(), identity("alice-sub", "sync:read sync:write")),
        ("bob".to_string(), identity("bob-sub", "sync:read sync:write")),
        ("alice-reader".to_string(), identity("alice-sub", "sync:read")),
    ]);
    router(AppState::new(db, Authenticator::Fixed(tokens), Blobs::memory().unwrap(), 1024 * 1024))
}

struct Client {
    app: Router,
}

impl Client {
    async fn send(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Body,
    ) -> (StatusCode, Vec<u8>) {
        let mut request = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let request = request.header(header::CONTENT_TYPE, "application/json").body(body).unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        (status, to_bytes(response.into_body(), usize::MAX).await.unwrap().to_vec())
    }

    async fn json(&self, method: Method, uri: &str, token: &str, body: Option<Value>) -> (StatusCode, Value) {
        let body = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
        let (status, bytes) = self.send(method, uri, Some(token), body).await;
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }
}

fn change(device: Uuid, seq: u64, payload: &str) -> Value {
    use base64::Engine;
    json!({ "device": device, "seq": seq, "payload": base64::engine::general_purpose::STANDARD.encode(payload) })
}

/// The same story against any database.
pub async fn syncs_a_library(db: Database) {
    let c = Client { app: app(db) };
    let (status, _) = c.send(Method::GET, "/v1/me", None, Body::empty()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body) = c.json(Method::GET, "/v1/me", "nobody", None).await;
    assert_eq!((status, body["error"].as_str()), (StatusCode::UNAUTHORIZED, Some("unauthorized")));

    // Devices.
    let mac = Uuid::now_v7();
    let phone = Uuid::now_v7();
    let bobs = Uuid::now_v7();
    for (token, id, name) in [("alice", mac, "Mac"), ("alice", phone, "iPhone"), ("bob", bobs, "PC")] {
        let device = json!({ "id": id, "name": name, "platform": "macos" });
        let (status, body) = c.json(Method::POST, "/v1/devices", token, Some(device)).await;
        assert_eq!((status, body["name"].as_str()), (StatusCode::OK, Some(name)));
    }
    let renamed = json!({ "id": mac, "name": "MacBook", "platform": "macos" });
    assert_eq!(c.json(Method::POST, "/v1/devices", "alice", Some(renamed)).await.0, StatusCode::OK);
    let stolen = json!({ "id": mac, "name": "Mine now", "platform": "linux" });
    assert_eq!(c.json(Method::POST, "/v1/devices", "bob", Some(stolen)).await.0, StatusCode::CONFLICT);
    let (_, me) = c.json(Method::GET, "/v1/me", "alice", None).await;
    assert_eq!(me["sub"], "alice-sub");
    assert_eq!(me["plan"], "free");
    let mut names: Vec<&str> =
        me["devices"].as_array().unwrap().iter().map(|d| d["name"].as_str().unwrap()).collect();
    names.sort();
    assert_eq!(names, ["MacBook", "iPhone"]);

    // Libraries.
    let id = Uuid::now_v7();
    let new = json!({ "id": id, "name": "Thesis" });
    let (status, library) = c.json(Method::POST, "/v1/libraries", "alice", Some(new.clone())).await;
    assert_eq!(
        (status, library["role"].as_str(), library["head"].as_u64()),
        (StatusCode::OK, Some("owner"), Some(0))
    );
    assert_eq!(c.json(Method::POST, "/v1/libraries", "alice", Some(new.clone())).await.1, library, "again");
    assert_eq!(c.json(Method::POST, "/v1/libraries", "bob", Some(new)).await.0, StatusCode::CONFLICT);
    assert_eq!(c.json(Method::GET, "/v1/libraries", "alice", None).await.1, json!([library]));
    assert_eq!(c.json(Method::GET, "/v1/libraries", "bob", None).await.1, json!([]));
    let lib = format!("/v1/libraries/{id}");
    assert_eq!(c.json(Method::GET, &lib, "bob", None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(c.json(Method::GET, "/v1/libraries/nope", "alice", None).await.0, StatusCode::BAD_REQUEST);

    // Pushing.
    let changes = format!("{lib}/changes");
    let push = |list: Vec<Value>| Some(json!({ "changes": list }));
    let (status, pushed) =
        c.json(Method::POST, &changes, "alice", push(vec![change(mac, 1, "a"), change(mac, 2, "b")])).await;
    assert_eq!((status, pushed), (StatusCode::OK, json!({ "head": 2, "cursors": [1, 2] })));
    let (_, pushed) =
        c.json(Method::POST, &changes, "alice", push(vec![change(mac, 2, "b"), change(phone, 1, "c")])).await;
    assert_eq!(pushed, json!({ "head": 3, "cursors": [2, 3] }), "a change sent twice is kept once");
    let (status, body) =
        c.json(Method::POST, &changes, "alice", push(vec![change(phone, 2, "d"), change(mac, 5, "e")])).await;
    assert_eq!((status, body["expectedSeq"].as_u64()), (StatusCode::CONFLICT, Some(3)));
    let (_, page) = c.json(Method::GET, &format!("{changes}?after=3"), "alice", None).await;
    assert_eq!(page["changes"], json!([]), "a refused push keeps nothing");
    assert_eq!(
        c.json(Method::POST, &changes, "alice", push(vec![change(bobs, 1, "x")])).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        c.json(Method::POST, &changes, "bob", push(vec![change(bobs, 1, "x")])).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        c.json(Method::POST, &changes, "alice-reader", push(vec![change(phone, 2, "d")])).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        c.json(Method::POST, &changes, "alice", push(vec![change(phone, 0, "z")])).await.0,
        StatusCode::BAD_REQUEST
    );

    // Pulling.
    let (_, page) = c.json(Method::GET, &format!("{changes}?after=0&limit=2"), "alice-reader", None).await;
    assert_eq!((page["head"].as_u64(), page["more"].as_bool()), (Some(3), Some(true)));
    let cursors: Vec<u64> =
        page["changes"].as_array().unwrap().iter().map(|c| c["cursor"].as_u64().unwrap()).collect();
    assert_eq!(cursors, [1, 2]);
    assert_eq!(page["changes"][1]["payload"], "Yg==");
    let (_, page) = c.json(Method::GET, &format!("{changes}?after=2"), "alice", None).await;
    assert_eq!(
        (page["changes"][0]["device"].as_str(), page["more"].as_bool()),
        (Some(phone.to_string().as_str()), Some(false))
    );

    // Blobs.
    let bytes = b"%PDF-1.7 paper".to_vec();
    let hash = BlobHash::of(&bytes);
    let blob = format!("{lib}/blobs/{hash}");
    let wrong = format!("{lib}/blobs/{}", BlobHash::of(b"other"));
    assert_eq!(
        c.send(Method::PUT, &wrong, Some("alice"), Body::from(bytes.clone())).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(c.send(Method::GET, &blob, Some("alice"), Body::empty()).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        c.send(Method::PUT, &blob, Some("alice"), Body::from(bytes.clone())).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        c.send(Method::GET, &blob, Some("alice"), Body::empty()).await,
        (StatusCode::OK, bytes.clone())
    );
    assert_eq!(c.send(Method::GET, &blob, Some("bob"), Body::empty()).await.0, StatusCode::NOT_FOUND);
    let huge = vec![7u8; 1024 * 1024 + 1];
    let huge_blob = format!("{lib}/blobs/{}", BlobHash::of(&huge));
    assert_eq!(
        c.send(Method::PUT, &huge_blob, Some("alice"), Body::from(huge)).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let (_, me) = c.json(Method::GET, "/v1/me", "alice", None).await;
    assert_eq!(me["usage"], json!({ "libraries": 1, "changeBytes": 3, "blobBytes": bytes.len() }));

    // Snapshots fold the log.
    let snapshot = format!("{lib}/snapshot");
    let missing = json!({ "cursor": 2, "blob": BlobHash::of(b"never uploaded") });
    assert_eq!(c.json(Method::POST, &snapshot, "alice", Some(missing)).await.0, StatusCode::BAD_REQUEST);
    let ahead = json!({ "cursor": 9, "blob": hash });
    assert_eq!(c.json(Method::POST, &snapshot, "alice", Some(ahead)).await.0, StatusCode::BAD_REQUEST);
    let good = json!({ "cursor": 2, "blob": hash });
    assert_eq!(c.json(Method::POST, &snapshot, "alice", Some(good.clone())).await.0, StatusCode::NO_CONTENT);
    let (_, page) = c.json(Method::GET, &format!("{changes}?after=0"), "alice", None).await;
    assert_eq!((page["snapshot"].clone(), page["changes"].clone()), (good, json!([])));
    let (_, page) = c.json(Method::GET, &format!("{changes}?after=2"), "alice", None).await;
    assert_eq!(page["changes"].as_array().unwrap().len(), 1);
    assert_eq!(page["snapshot"], Value::Null);
    let (_, pushed) =
        c.json(Method::POST, &changes, "alice", push(vec![change(mac, 1, "a"), change(mac, 3, "f")])).await;
    assert_eq!(pushed, json!({ "head": 4, "cursors": [0, 4] }), "folded changes answer 0");
    let older = json!({ "cursor": 1, "blob": hash });
    assert_eq!(c.json(Method::POST, &snapshot, "alice", Some(older)).await.0, StatusCode::CONFLICT);
}

#[tokio::test]
async fn syncs_a_library_in_memory() {
    syncs_a_library(Database::memory()).await;
}

#[tokio::test]
async fn health_needs_no_token() {
    let c = Client { app: app(Database::memory()) };
    assert_eq!(c.send(Method::GET, "/health", None, Body::empty()).await, (StatusCode::OK, b"ok".to_vec()));
}
