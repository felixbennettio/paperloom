//! The HTTP API (v1). See docs/sync-protocol.md.

use std::convert::Infallible;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, JsonRejection, PathRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use paperloom_sync_proto::{
    BlobHash, ChangesPage, Device, HeadEvent, Library, MAX_BATCH, Me, NewDevice, NewLibrary, Push, Pushed,
    SCOPE_READ, SCOPE_WRITE, Snapshot,
};
use serde::Deserialize;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use uuid::Uuid;

use crate::AppState;
use crate::auth::Identity;
use crate::db::UserId;
use crate::error::ApiError;

/// The largest change.
pub const MAX_CHANGE_BYTES: usize = 8 * 1024 * 1024;
/// The largest push body (base64 makes bytes a third bigger).
const MAX_PUSH_BODY: usize = 48 * 1024 * 1024;

type Shared = Arc<AppState>;

pub fn router(state: Shared) -> Router {
    let blob_limit = state.max_blob_bytes;
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/me", get(me))
        .route("/v1/devices", post(register_device))
        .route("/v1/libraries", get(libraries).post(create_library))
        .route("/v1/libraries/{id}", get(library))
        .route(
            "/v1/libraries/{id}/changes",
            get(changes).post(push).layer(DefaultBodyLimit::max(MAX_PUSH_BODY)),
        )
        .route("/v1/libraries/{id}/events", get(events))
        .route("/v1/libraries/{id}/snapshot", post(snapshot))
        .route(
            "/v1/libraries/{id}/blobs/{hash}",
            put(put_blob).get(get_blob).layer(DefaultBodyLimit::max(blob_limit)),
        )
        .with_state(state)
}

/// The account calling, from its bearer token.
pub struct Caller {
    pub user: UserId,
    pub identity: Identity,
}

impl Caller {
    fn need(&self, scope: &str) -> Result<(), ApiError> {
        if self.identity.has(scope) {
            Ok(())
        } else {
            Err(ApiError::Forbidden(format!("The token lacks the {scope} scope.")))
        }
    }
}

impl FromRequestParts<Shared> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, ApiError> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| ApiError::Unauthorized("Sign in: no bearer token.".into()))?;
        let identity = state.auth.verify(token).await?;
        let user = state.db.user(&identity.sub).await?;
        Ok(Self { user, identity })
    }
}

/// A library the caller is a member of; for writing, one they may change.
async fn member(state: &AppState, caller: &Caller, id: Uuid, write: bool) -> Result<Library, ApiError> {
    let library = state
        .db
        .library(caller.user, id)
        .await?
        .ok_or_else(|| ApiError::NotFound("No such library".into()))?;
    if write && !library.role.can_write() {
        return Err(ApiError::Forbidden("This library is read-only for you.".into()));
    }
    Ok(library)
}

async fn me(State(state): State<Shared>, caller: Caller) -> Result<Json<Me>, ApiError> {
    caller.need(SCOPE_READ)?;
    let devices = state.db.devices(caller.user).await?;
    let usage = state.db.usage(caller.user).await?;
    Ok(Json(Me { sub: caller.identity.sub, plan: "free".into(), devices, usage }))
}

fn check_name(what: &str, name: &str, longest: usize) -> Result<(), ApiError> {
    if name.trim().is_empty() || name.chars().count() > longest {
        return Err(ApiError::BadRequest(format!("The {what} must have 1 to {longest} characters.")));
    }
    Ok(())
}

async fn register_device(
    State(state): State<Shared>,
    caller: Caller,
    body: Result<Json<NewDevice>, JsonRejection>,
) -> Result<Json<Device>, ApiError> {
    caller.need(SCOPE_WRITE)?;
    let Json(device) = body?;
    check_name("device name", &device.name, 200)?;
    check_name("platform", &device.platform, 40)?;
    Ok(Json(state.db.register_device(caller.user, device).await?))
}

async fn libraries(State(state): State<Shared>, caller: Caller) -> Result<Json<Vec<Library>>, ApiError> {
    caller.need(SCOPE_READ)?;
    Ok(Json(state.db.libraries(caller.user).await?))
}

async fn create_library(
    State(state): State<Shared>,
    caller: Caller,
    body: Result<Json<NewLibrary>, JsonRejection>,
) -> Result<Json<Library>, ApiError> {
    caller.need(SCOPE_WRITE)?;
    let Json(library) = body?;
    check_name("library name", &library.name, 200)?;
    Ok(Json(state.db.create_library(caller.user, library).await?))
}

async fn library(
    State(state): State<Shared>,
    caller: Caller,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Library>, ApiError> {
    caller.need(SCOPE_READ)?;
    let Path(id) = path?;
    Ok(Json(member(&state, &caller, id, false).await?))
}

async fn push(
    State(state): State<Shared>,
    caller: Caller,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<Push>, JsonRejection>,
) -> Result<Json<Pushed>, ApiError> {
    caller.need(SCOPE_WRITE)?;
    let Path(id) = path?;
    let Json(push) = body?;
    if push.changes.len() > MAX_BATCH {
        return Err(ApiError::TooLarge(format!("Push at most {MAX_BATCH} changes at once.")));
    }
    if push.changes.iter().any(|c| c.payload.len() > MAX_CHANGE_BYTES) {
        return Err(ApiError::TooLarge("A change is larger than 8 MiB; keep large data in blobs.".into()));
    }
    if push.changes.iter().any(|c| c.seq == 0 || c.seq > i64::MAX as u64) {
        return Err(ApiError::BadRequest("Changes are numbered from 1.".into()));
    }
    let before = member(&state, &caller, id, true).await?.head;
    let pushed = state.db.push(caller.user, id, push.changes).await?;
    if pushed.head > before {
        state.events.publish(id, pushed.head);
    }
    Ok(Json(pushed))
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    after: u64,
    limit: Option<usize>,
}

async fn changes(
    State(state): State<Shared>,
    caller: Caller,
    path: Result<Path<Uuid>, PathRejection>,
    page: Result<Query<Page>, QueryRejection>,
) -> Result<Json<ChangesPage>, ApiError> {
    caller.need(SCOPE_READ)?;
    let Path(id) = path?;
    let Query(page) = page?;
    member(&state, &caller, id, false).await?;
    let limit = page.limit.unwrap_or(MAX_BATCH).clamp(1, MAX_BATCH);
    Ok(Json(state.db.changes(id, page.after, limit).await?))
}

/// Server-sent `head` events: the log's head now, then whenever it grows.
async fn events(
    State(state): State<Shared>,
    caller: Caller,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    caller.need(SCOPE_READ)?;
    let Path(id) = path?;
    let receiver = state.events.subscribe(id);
    let head = member(&state, &caller, id, false).await?.head;
    let grown = BroadcastStream::new(receiver).filter_map(Result::ok);
    let stream = tokio_stream::once(head)
        .chain(grown)
        .map(|head| Ok(Event::default().event("head").json_data(HeadEvent { head }).unwrap_or_default()));
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn snapshot(
    State(state): State<Shared>,
    caller: Caller,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<Snapshot>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    caller.need(SCOPE_WRITE)?;
    let Path(id) = path?;
    let Json(snapshot) = body?;
    member(&state, &caller, id, true).await?;
    if !state.db.has_blob(id, &snapshot.blob).await? {
        return Err(ApiError::BadRequest("Upload the snapshot's blob first.".into()));
    }
    state.db.set_snapshot(id, snapshot).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn blob_hash(text: &str) -> Result<BlobHash, ApiError> {
    text.parse().map_err(|e| ApiError::BadRequest(format!("{e}")))
}

async fn put_blob(
    State(state): State<Shared>,
    caller: Caller,
    path: Result<Path<(Uuid, String)>, PathRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Result<StatusCode, ApiError> {
    caller.need(SCOPE_WRITE)?;
    let Path((id, hash)) = path?;
    let hash = blob_hash(&hash)?;
    let body = body?;
    member(&state, &caller, id, true).await?;
    let bytes = body.clone();
    let actual = tokio::task::spawn_blocking(move || BlobHash::of(&bytes))
        .await
        .map_err(|e| ApiError::Internal(format!("hashing: {e}")))?;
    if actual != hash {
        return Err(ApiError::BadRequest("The bytes do not match the hash in the address.".into()));
    }
    if !state.db.has_blob(id, &hash).await? {
        let size = body.len() as u64;
        state.blobs.put(id, &hash, body).await?;
        state.db.add_blob(id, &hash, size).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn get_blob(
    State(state): State<Shared>,
    caller: Caller,
    path: Result<Path<(Uuid, String)>, PathRejection>,
) -> Result<Response, ApiError> {
    caller.need(SCOPE_READ)?;
    let Path((id, hash)) = path?;
    let hash = blob_hash(&hash)?;
    member(&state, &caller, id, false).await?;
    if !state.db.has_blob(id, &hash).await? {
        return Err(ApiError::NotFound("No such blob".into()));
    }
    let bytes = state.blobs.get(id, &hash).await?;
    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response())
}
