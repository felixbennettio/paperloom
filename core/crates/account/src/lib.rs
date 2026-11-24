//! Signing in to Paperloom.
//!
//! OpenID Connect authorization code flow with PKCE (RFC 7636), as native
//! apps should do it (RFC 8252): no client secret in the app, the system
//! browser shows the sign-in page, and the account service redirects back to
//! `io.felixbennett.paperloom://oauth/callback`. The identity provider is
//! Logto; nothing here is Logto-specific beyond its `resource` parameter
//! (RFC 8707), which asks for an access token for the sync service.
//!
//! The apps own the platform parts: opening the browser, receiving the
//! callback and keeping the tokens in the keychain. Everything else is here.

mod client;
mod pkce;

pub use client::{Client, Config, Discovery, PendingSignIn, Session, Tokens, UserInfo};
pub use paperloom_http::{Http, NetworkError, Response, UreqHttp};

/// The account service's issuer.
pub const ISSUER: &str = "https://api.sso.routeforge.zone/oidc";
/// The API resource of the official sync service, the audience of its tokens.
pub const SYNC_RESOURCE: &str = "https://sync.paperloom.routeforge.zone";
/// Where the account service sends the browser after signing in.
pub const REDIRECT_URI: &str = "io.felixbennett.paperloom://oauth/callback";
/// Where it sends the browser after signing out.
pub const POST_LOGOUT_REDIRECT_URI: &str = "io.felixbennett.paperloom://oauth/signed-out";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(String),
    #[error("the account service answered {status}: {body}")]
    Http { status: u16, body: String },
    #[error("the account service refused: {error}{}", .description.as_deref().map(|d| format!(" ({d})")).unwrap_or_default())]
    Provider { error: String, description: Option<String> },
    #[error("sign-in was cancelled")]
    Cancelled,
    #[error("the sign-in reply does not belong to this sign-in")]
    StateMismatch,
    #[error("unexpected sign-in reply: {0}")]
    InvalidCallback(String),
    #[error("invalid token: {0}")]
    InvalidToken(String),
    #[error("unreadable reply from the account service: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<NetworkError> for Error {
    fn from(err: NetworkError) -> Self {
        Error::Network(err.0)
    }
}
