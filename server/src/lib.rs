//! Paperloom Cloud, the official sync service: accounts from Logto tokens,
//! each library's change log in PostgreSQL, attachments in object storage.
//! It never reads what devices send it. See docs/sync-protocol.md.

pub mod api;
pub mod auth;
pub mod blobs;
pub mod config;
pub mod db;
pub mod error;
pub mod events;

use std::sync::Arc;

use auth::{Authenticator, Jwks};
use blobs::Blobs;
use config::Config;
use db::{Database, Postgres};
use events::Events;

pub struct AppState {
    pub db: Database,
    pub auth: Authenticator,
    pub blobs: Blobs,
    pub events: Events,
    pub max_blob_bytes: usize,
}

impl AppState {
    pub fn new(db: Database, auth: Authenticator, blobs: Blobs, max_blob_bytes: usize) -> Arc<Self> {
        Arc::new(Self { db, auth, blobs, events: Events::default(), max_blob_bytes })
    }
}

/// Serves until SIGINT or SIGTERM.
pub async fn run(config: Config) -> Result<(), String> {
    let db = Postgres::connect(&config.database_url).await.map_err(|e| format!("database: {e}"))?;
    let blobs = Blobs::open(&config.blobs).map_err(|e| format!("blob storage: {e}"))?;
    let http = Arc::new(paperloom_http::UreqHttp::new());
    let auth = Authenticator::Jwks(Jwks::new(config.issuer, config.audience, http));
    let state = AppState::new(Database::Postgres(db), auth, blobs, config.max_blob_bytes);
    let listener =
        tokio::net::TcpListener::bind(config.bind).await.map_err(|e| format!("{}: {e}", config.bind))?;
    tracing::info!("Paperloom Cloud listening on {}", config.bind);
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(stopped())
        .await
        .map_err(|e| e.to_string())
}

async fn stopped() {
    let interrupt = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut terminate = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(_) => {
                let _ = interrupt.await;
                return;
            }
        };
        tokio::select! {
            _ = interrupt => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = interrupt.await;
    }
    tracing::info!("Stopping");
}
