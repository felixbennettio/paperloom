//! Who is calling: Logto access tokens (JWT), checked against the issuer's
//! published keys (JWKS): signature, issuer, audience (the sync API
//! resource) and expiry. Accounts are known by the token's `sub`; the
//! service keeps no passwords.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use paperloom_http::Http;
use serde::Deserialize;

use crate::error::ApiError;

/// The account behind a token, and what the token allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub sub: String,
    pub scopes: Vec<String>,
}

impl Identity {
    pub fn has(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }
}

pub enum Authenticator {
    Jwks(Jwks),
    /// Fixed tokens, for tests.
    Fixed(HashMap<String, Identity>),
}

impl Authenticator {
    pub async fn verify(&self, token: &str) -> Result<Identity, ApiError> {
        match self {
            Self::Jwks(jwks) => jwks.verify(token).await,
            Self::Fixed(tokens) => {
                tokens.get(token).cloned().ok_or_else(|| ApiError::Unauthorized("Unknown token".into()))
            }
        }
    }
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    #[serde(default)]
    scope: String,
}

/// How long fetched keys are trusted before fetching them again.
const KEYS_TTL: Duration = Duration::from_secs(3600);
/// The least time between fetches for a token signed with an unknown key.
const REFETCH_GAP: Duration = Duration::from_secs(60);

pub struct Jwks {
    issuer: String,
    audience: String,
    http: Arc<dyn Http>,
    keys: Mutex<Option<(JwkSet, Instant)>>,
}

impl Jwks {
    pub fn new(issuer: impl Into<String>, audience: impl Into<String>, http: Arc<dyn Http>) -> Self {
        Self { issuer: issuer.into(), audience: audience.into(), http, keys: Mutex::new(None) }
    }

    /// Starts with these keys (fetching again only for an unknown key).
    pub fn with_keys(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        http: Arc<dyn Http>,
        keys: JwkSet,
    ) -> Self {
        let jwks = Self::new(issuer, audience, http);
        *jwks.keys.lock().unwrap_or_else(|e| e.into_inner()) = Some((keys, Instant::now()));
        jwks
    }

    pub async fn verify(&self, token: &str) -> Result<Identity, ApiError> {
        let unauthorized = |text: &str| ApiError::Unauthorized(text.to_string());
        let header = decode_header(token).map_err(|_| unauthorized("The token is not a JWT."))?;
        // Only keys the issuer publishes: never a shared secret.
        if matches!(header.alg, Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512) {
            return Err(unauthorized("The token is not signed with a published key."));
        }
        let kid = header.kid.ok_or_else(|| unauthorized("The token does not name its key."))?;
        let jwk = match self.cached(false).and_then(|keys| keys.find(&kid).cloned()) {
            Some(jwk) => jwk,
            None => {
                let keys = self.fetch().await.map_err(|e| {
                    if self.cached(true).is_some() { unauthorized("The token's key is unknown.") } else { e }
                })?;
                keys.find(&kid).cloned().ok_or_else(|| unauthorized("The token's key is unknown."))?
            }
        };
        let key = DecodingKey::from_jwk(&jwk).map_err(|_| unauthorized("The token's key cannot be used."))?;
        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.leeway = 30;
        let data = decode::<Claims>(token, &key, &validation)
            .map_err(|e| ApiError::Unauthorized(format!("The token is not valid: {e}")))?;
        Ok(Identity {
            sub: data.claims.sub,
            scopes: data.claims.scope.split_whitespace().map(str::to_string).collect(),
        })
    }

    /// The keys held, if still fresh (or at all, with `stale`).
    fn cached(&self, stale: bool) -> Option<JwkSet> {
        let keys = self.keys.lock().unwrap_or_else(|e| e.into_inner());
        keys.as_ref().filter(|(_, at)| stale || at.elapsed() < KEYS_TTL).map(|(set, _)| set.clone())
    }

    /// Fetches the issuer's keys, at most once a minute.
    async fn fetch(&self) -> Result<JwkSet, ApiError> {
        if let Some((set, at)) = self.keys.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
            && at.elapsed() < REFETCH_GAP
        {
            return Ok(set.clone());
        }
        let http = self.http.clone();
        let issuer = self.issuer.trim_end_matches('/').to_string();
        let set = tokio::task::spawn_blocking(move || fetch_keys(http.as_ref(), &issuer))
            .await
            .map_err(|e| ApiError::Internal(format!("fetching keys: {e}")))??;
        *self.keys.lock().unwrap_or_else(|e| e.into_inner()) = Some((set.clone(), Instant::now()));
        Ok(set)
    }
}

/// OpenID discovery, then the JWKS it points to.
fn fetch_keys(http: &dyn Http, issuer: &str) -> Result<JwkSet, ApiError> {
    let fail = |what: &str, detail: String| ApiError::Internal(format!("{what} from {issuer}: {detail}"));
    let discovery = http
        .get(&format!("{issuer}/.well-known/openid-configuration"), "application/json")
        .map_err(|e| fail("discovery", e.to_string()))?;
    if !discovery.is_success() {
        return Err(fail("discovery", format!("HTTP {}", discovery.status)));
    }
    let discovery: serde_json::Value =
        serde_json::from_str(&discovery.body).map_err(|e| fail("discovery", e.to_string()))?;
    let uri = discovery["jwks_uri"].as_str().ok_or_else(|| fail("discovery", "no jwks_uri".into()))?;
    let keys = http.get(uri, "application/json").map_err(|e| fail("keys", e.to_string()))?;
    if !keys.is_success() {
        return Err(fail("keys", format!("HTTP {}", keys.status)));
    }
    serde_json::from_str(&keys.body).map_err(|e| fail("keys", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use paperloom_http::{NetworkError, Response};
    use ring::rand::SystemRandom;
    use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const ISSUER: &str = "https://id.example/oidc";
    const AUDIENCE: &str = "https://sync.example";

    /// Answers discovery and keys from fixed text; counts the calls.
    struct FakeIdp {
        keys: Option<String>,
        calls: AtomicUsize,
    }

    impl Http for FakeIdp {
        fn get(&self, url: &str, _accept: &str) -> Result<Response, NetworkError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let keys = self.keys.clone().ok_or_else(|| NetworkError("offline".into()))?;
            if url.ends_with("/.well-known/openid-configuration") {
                Ok(Response::new(200, json!({ "jwks_uri": format!("{ISSUER}/jwks") }).to_string()))
            } else {
                Ok(Response::new(200, keys))
            }
        }

        fn post_form(&self, _url: &str, _form: &[(&str, &str)]) -> Result<Response, NetworkError> {
            Err(NetworkError("no".into()))
        }
    }

    /// A new signing key, and its public half as a JWKS.
    fn key(kid: &str) -> (EncodingKey, serde_json::Value) {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng).unwrap();
        let point = pair.public_key().as_ref();
        let jwks = json!({ "keys": [{
            "kty": "EC", "crv": "P-256", "alg": "ES256", "use": "sig", "kid": kid,
            "x": URL_SAFE_NO_PAD.encode(&point[1..33]),
            "y": URL_SAFE_NO_PAD.encode(&point[33..65]),
        }]});
        (EncodingKey::from_ec_der(pkcs8.as_ref()), jwks)
    }

    fn token(key: &EncodingKey, kid: &str, audience: &str) -> String {
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(kid.into());
        let exp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 600;
        let claims = json!({ "sub": "user-1", "iss": ISSUER, "aud": audience, "exp": exp, "scope": "sync:read sync:write" });
        encode(&header, &claims, key).unwrap()
    }

    #[tokio::test]
    async fn accepts_tokens_for_the_sync_api_only() {
        let (signing, jwks) = key("k1");
        let idp = Arc::new(FakeIdp { keys: Some(jwks.to_string()), calls: AtomicUsize::new(0) });
        let verifier = Jwks::new(ISSUER, AUDIENCE, idp.clone());

        let identity = verifier.verify(&token(&signing, "k1", AUDIENCE)).await.unwrap();
        assert_eq!(identity.sub, "user-1");
        assert!(identity.has("sync:write") && !identity.has("admin"));
        assert_eq!(idp.calls.load(Ordering::SeqCst), 2, "discovery and keys, once");
        verifier.verify(&token(&signing, "k1", AUDIENCE)).await.unwrap();
        assert_eq!(idp.calls.load(Ordering::SeqCst), 2, "keys kept");

        let other_api = verifier.verify(&token(&signing, "k1", "https://other.example")).await;
        assert!(matches!(other_api, Err(ApiError::Unauthorized(_))));
        let (stranger, _) = key("k1");
        assert!(verifier.verify(&token(&stranger, "k1", AUDIENCE)).await.is_err(), "wrong signature");
        assert!(verifier.verify("not a token").await.is_err());

        let shared_secret = encode(
            &Header::new(Algorithm::HS256),
            &json!({ "sub": "x", "iss": ISSUER, "aud": AUDIENCE, "exp": 9_999_999_999u64 }),
            &EncodingKey::from_secret(b"secret"),
        )
        .unwrap();
        assert!(verifier.verify(&shared_secret).await.is_err());
    }

    #[tokio::test]
    async fn unknown_keys_are_fetched_again_but_not_too_often() {
        let (signing, jwks) = key("k2");
        let idp =
            Arc::new(FakeIdp { keys: Some(json!({ "keys": [] }).to_string()), calls: AtomicUsize::new(0) });
        let set: JwkSet = serde_json::from_value(jwks).unwrap();
        let verifier = Jwks::with_keys(ISSUER, AUDIENCE, idp.clone(), set);
        verifier.verify(&token(&signing, "k2", AUDIENCE)).await.unwrap();
        assert_eq!(idp.calls.load(Ordering::SeqCst), 0);
        // A key it was never given: held keys are recent, so no fetch.
        assert!(verifier.verify(&token(&signing, "k3", AUDIENCE)).await.is_err());
        assert_eq!(idp.calls.load(Ordering::SeqCst), 0);

        let offline =
            Jwks::new(ISSUER, AUDIENCE, Arc::new(FakeIdp { keys: None, calls: AtomicUsize::new(0) }));
        assert!(matches!(offline.verify(&token(&signing, "k2", AUDIENCE)).await, Err(ApiError::Internal(_))));
    }
}
