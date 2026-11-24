//! The sign-in flow.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use paperloom_http::{Http, UreqHttp};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{Error, POST_LOGOUT_REDIRECT_URI, REDIRECT_URI, Result, SYNC_RESOURCE, pkce};

/// Seconds of clock difference tolerated when checking token expiry.
const CLOCK_SKEW: i64 = 120;

/// How the app signs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub issuer: String,
    /// The app's client ID in the account service (a public, native client).
    pub client_id: String,
    pub redirect_uri: String,
    pub post_logout_redirect_uri: String,
    pub scopes: Vec<String>,
    /// API resources the sign-in asks consent for (RFC 8707).
    pub resources: Vec<String>,
}

impl Config {
    /// Paperloom's settings for the given client ID.
    pub fn paperloom(client_id: impl Into<String>) -> Self {
        Self {
            issuer: crate::ISSUER.to_string(),
            client_id: client_id.into(),
            redirect_uri: REDIRECT_URI.to_string(),
            post_logout_redirect_uri: POST_LOGOUT_REDIRECT_URI.to_string(),
            scopes: ["openid", "offline_access", "profile", "email", "sync:read", "sync:write"]
                .map(String::from)
                .to_vec(),
            resources: vec![SYNC_RESOURCE.to_string()],
        }
    }
}

/// The endpoints the issuer publishes at `.well-known/openid-configuration`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub end_session_endpoint: Option<String>,
    #[serde(default)]
    pub userinfo_endpoint: Option<String>,
    pub jwks_uri: String,
}

/// A sign-in in progress: open `url` in the browser, keep the rest until
/// the callback arrives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingSignIn {
    pub url: String,
    pub state: String,
    pub nonce: String,
    pub code_verifier: String,
}

/// Tokens from the token endpoint. `expires_at` is in Unix seconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub expires_at: i64,
    pub scope: Option<String>,
}

impl Tokens {
    /// True if the access token expires within `margin` seconds.
    pub fn expires_within(&self, margin: i64) -> bool {
        self.expires_at - margin <= now()
    }
}

/// Who signed in, from the ID token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserInfo {
    pub subject: String,
    pub name: Option<String>,
    pub username: Option<String>,
    pub email: Option<String>,
    pub picture: Option<String>,
}

impl UserInfo {
    /// The best name to show: name, user name, email, or the subject.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().or(self.username.as_deref()).or(self.email.as_deref()).unwrap_or(&self.subject)
    }
}

/// A completed sign-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub user: UserInfo,
    pub tokens: Tokens,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct IdClaims {
    iss: String,
    sub: String,
    aud: Audience,
    exp: i64,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    picture: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Audience {
    fn contains(&self, value: &str) -> bool {
        match self {
            Audience::One(a) => a == value,
            Audience::Many(list) => list.iter().any(|a| a == value),
        }
    }
}

/// Talks to the account service. Cheap to share; discovery is fetched once.
pub struct Client<H: Http = UreqHttp> {
    config: Config,
    http: H,
    discovery: Mutex<Option<Discovery>>,
}

impl Client<UreqHttp> {
    pub fn new(config: Config) -> Self {
        Self::with_http(config, UreqHttp::new())
    }
}

impl<H: Http> Client<H> {
    pub fn with_http(config: Config, http: H) -> Self {
        Self { config, http, discovery: Mutex::new(None) }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The issuer's endpoints, fetched on first use.
    pub fn discovery(&self) -> Result<Discovery> {
        let mut cached = self.discovery.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(found) = cached.as_ref() {
            return Ok(found.clone());
        }
        let url = format!("{}/.well-known/openid-configuration", self.config.issuer.trim_end_matches('/'));
        let response = self.http.get(&url, "application/json")?;
        if response.status != 200 {
            return Err(Error::Http { status: response.status, body: response.body });
        }
        let found: Discovery = serde_json::from_str(&response.body)?;
        if found.issuer.trim_end_matches('/') != self.config.issuer.trim_end_matches('/') {
            return Err(Error::InvalidToken(format!("discovery names issuer {}", found.issuer)));
        }
        *cached = Some(found.clone());
        Ok(found)
    }

    /// Starts a sign-in: the URL to open and what to keep for the callback.
    pub fn begin_sign_in(&self) -> Result<PendingSignIn> {
        let discovery = self.discovery()?;
        let state = pkce::random_token(24);
        let nonce = pkce::random_token(24);
        let code_verifier = pkce::code_verifier();
        let mut url = Url::parse(&discovery.authorization_endpoint)
            .map_err(|e| Error::InvalidCallback(format!("authorization endpoint: {e}")))?;
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("client_id", &self.config.client_id)
                .append_pair("redirect_uri", &self.config.redirect_uri)
                .append_pair("response_type", "code")
                .append_pair("scope", &self.config.scopes.join(" "))
                .append_pair("state", &state)
                .append_pair("nonce", &nonce)
                .append_pair("code_challenge", &pkce::code_challenge(&code_verifier))
                .append_pair("code_challenge_method", "S256")
                // Refresh tokens (offline_access) need the consent prompt.
                .append_pair("prompt", "consent");
            for resource in &self.config.resources {
                query.append_pair("resource", resource);
            }
        }
        Ok(PendingSignIn { url: url.into(), state, nonce, code_verifier })
    }

    /// Finishes a sign-in with the URL the browser was redirected to.
    pub fn finish_sign_in(&self, pending: &PendingSignIn, callback_url: &str) -> Result<Session> {
        let code = parse_callback(callback_url, &self.config.redirect_uri, &pending.state)?;
        let discovery = self.discovery()?;
        let tokens = self.token_request(
            &discovery,
            &[
                ("grant_type", "authorization_code"),
                ("code", code.as_str()),
                ("redirect_uri", self.config.redirect_uri.as_str()),
                ("client_id", self.config.client_id.as_str()),
                ("code_verifier", pending.code_verifier.as_str()),
            ],
        )?;
        let id_token = tokens
            .id_token
            .as_deref()
            .ok_or_else(|| Error::InvalidToken("the reply has no ID token".into()))?;
        let user = self.verify_id_token(&discovery, id_token, Some(&pending.nonce))?;
        Ok(Session { user, tokens })
    }

    /// New tokens from a refresh token; with `resource`, an access token for
    /// that API (the sync service).
    pub fn refresh(&self, refresh_token: &str, resource: Option<&str>) -> Result<Tokens> {
        let discovery = self.discovery()?;
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", self.config.client_id.as_str()),
        ];
        if let Some(resource) = resource {
            form.push(("resource", resource));
        }
        let mut tokens = self.token_request(&discovery, &form)?;
        // Without rotation the service keeps the old refresh token valid.
        if tokens.refresh_token.is_none() {
            tokens.refresh_token = Some(refresh_token.to_string());
        }
        Ok(tokens)
    }

    /// The URL that ends the session in the browser.
    pub fn sign_out_url(&self, id_token: Option<&str>) -> Result<String> {
        let discovery = self.discovery()?;
        let endpoint = discovery
            .end_session_endpoint
            .ok_or_else(|| Error::InvalidCallback("the account service has no sign-out endpoint".into()))?;
        let mut url =
            Url::parse(&endpoint).map_err(|e| Error::InvalidCallback(format!("sign-out endpoint: {e}")))?;
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("client_id", &self.config.client_id)
                .append_pair("post_logout_redirect_uri", &self.config.post_logout_redirect_uri);
            if let Some(token) = id_token {
                query.append_pair("id_token_hint", token);
            }
        }
        Ok(url.into())
    }

    fn token_request(&self, discovery: &Discovery, form: &[(&str, &str)]) -> Result<Tokens> {
        let response = self.http.post_form(&discovery.token_endpoint, form)?;
        if response.status != 200 {
            return Err(match serde_json::from_str::<ErrorResponse>(&response.body) {
                Ok(e) => Error::Provider { error: e.error, description: e.error_description },
                Err(_) => Error::Http { status: response.status, body: response.body },
            });
        }
        let reply: TokenResponse = serde_json::from_str(&response.body)?;
        Ok(Tokens {
            access_token: reply.access_token,
            refresh_token: reply.refresh_token,
            id_token: reply.id_token,
            expires_at: now() + reply.expires_in.unwrap_or(3600),
            scope: reply.scope,
        })
    }

    /// Checks an ID token received straight from the token endpoint over TLS.
    /// OIDC Core §3.1.3.7 lets TLS stand in for the signature check then;
    /// issuer, audience, expiry and nonce are still verified.
    fn verify_id_token(&self, discovery: &Discovery, token: &str, nonce: Option<&str>) -> Result<UserInfo> {
        let payload = token.split('.').nth(1).ok_or_else(|| Error::InvalidToken("not a JWT".into()))?;
        let bytes = URL_SAFE_NO_PAD
            .decode(payload.trim_end_matches('='))
            .map_err(|e| Error::InvalidToken(format!("payload: {e}")))?;
        let claims: IdClaims = serde_json::from_slice(&bytes)?;
        if claims.iss != discovery.issuer {
            return Err(Error::InvalidToken(format!("issued by {}", claims.iss)));
        }
        if !claims.aud.contains(&self.config.client_id) {
            return Err(Error::InvalidToken("issued for another app".into()));
        }
        if claims.exp + CLOCK_SKEW < now() {
            return Err(Error::InvalidToken("expired".into()));
        }
        if nonce.is_some() && claims.nonce.as_deref() != nonce {
            return Err(Error::InvalidToken("nonce does not match".into()));
        }
        Ok(UserInfo {
            subject: claims.sub,
            name: claims.name,
            username: claims.username,
            email: claims.email,
            picture: claims.picture,
        })
    }
}

/// The authorization code from a callback URL, after checking it belongs to
/// this sign-in.
pub(crate) fn parse_callback(callback_url: &str, redirect_uri: &str, state: &str) -> Result<String> {
    if !callback_url.starts_with(redirect_uri) {
        return Err(Error::InvalidCallback(format!("not a reply to {redirect_uri}")));
    }
    let url = Url::parse(callback_url).map_err(|e| Error::InvalidCallback(e.to_string()))?;
    let get = |name: &str| url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned());
    if let Some(error) = get("error") {
        if error == "access_denied" {
            return Err(Error::Cancelled);
        }
        return Err(Error::Provider { error, description: get("error_description") });
    }
    if get("state").as_deref() != Some(state) {
        return Err(Error::StateMismatch);
    }
    get("code").filter(|c| !c.is_empty()).ok_or_else(|| Error::InvalidCallback("no code".into()))
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests;
