use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use url::Url;

use paperloom_http::{NetworkError, Response};

use super::*;

const ISSUER: &str = "https://id.example.com/oidc";

/// Answers HTTP calls from a script and records the forms it was sent.
#[derive(Default)]
struct FakeHttp {
    gets: HashMap<String, Response>,
    token_replies: Mutex<Vec<Response>>,
    forms: Mutex<Vec<Vec<(String, String)>>>,
}

impl Http for FakeHttp {
    fn get(&self, url: &str, _accept: &str) -> std::result::Result<Response, NetworkError> {
        self.gets.get(url).cloned().ok_or_else(|| NetworkError(format!("no route to {url}")))
    }

    fn post_form(&self, _url: &str, form: &[(&str, &str)]) -> std::result::Result<Response, NetworkError> {
        self.forms.lock().unwrap().push(form.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
        Ok(self.token_replies.lock().unwrap().remove(0))
    }
}

fn discovery_json() -> String {
    serde_json::json!({
        "issuer": ISSUER,
        "authorization_endpoint": format!("{ISSUER}/auth"),
        "token_endpoint": format!("{ISSUER}/token"),
        "end_session_endpoint": format!("{ISSUER}/session/end"),
        "jwks_uri": format!("{ISSUER}/jwks"),
    })
    .to_string()
}

fn client(token_replies: Vec<Response>) -> Client<FakeHttp> {
    let mut http = FakeHttp::default();
    http.gets
        .insert(format!("{ISSUER}/.well-known/openid-configuration"), Response::new(200, discovery_json()));
    *http.token_replies.lock().unwrap() = token_replies;
    let config = Config { issuer: ISSUER.into(), ..Config::paperloom("app-123") };
    Client::with_http(config, http)
}

fn jwt(claims: serde_json::Value) -> String {
    let encode = |v: &serde_json::Value| URL_SAFE_NO_PAD.encode(v.to_string());
    format!("{}.{}.signature", encode(&serde_json::json!({"alg": "ES384"})), encode(&claims))
}

fn token_reply(id_token: Option<String>) -> Response {
    let mut body = serde_json::json!({
        "access_token": "access-1",
        "refresh_token": "refresh-1",
        "expires_in": 3600,
        "scope": "openid offline_access",
        "token_type": "Bearer",
    });
    if let Some(token) = id_token {
        body["id_token"] = token.into();
    }
    Response::new(200, body.to_string())
}

fn query(url: &str) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for (k, v) in Url::parse(url).unwrap().query_pairs() {
        map.entry(k.into_owned()).or_default().push(v.into_owned());
    }
    map
}

#[test]
fn builds_the_authorization_url() {
    let c = client(vec![]);
    let pending = c.begin_sign_in().unwrap();
    assert!(pending.url.starts_with(&format!("{ISSUER}/auth?")));
    let q = query(&pending.url);
    let one = |k: &str| q[k][0].clone();
    assert_eq!(one("client_id"), "app-123");
    assert_eq!(one("redirect_uri"), "io.felixbennett.paperloom://oauth/callback");
    assert_eq!(one("response_type"), "code");
    assert_eq!(one("code_challenge_method"), "S256");
    assert_eq!(one("code_challenge"), crate::pkce::code_challenge(&pending.code_verifier));
    assert_eq!(one("state"), pending.state);
    assert_eq!(one("nonce"), pending.nonce);
    assert_eq!(one("prompt"), "consent");
    assert!(one("scope").split(' ').any(|s| s == "offline_access"));
    assert_eq!(q["resource"], vec!["https://sync.paperloom.routeforge.zone".to_string()]);
}

#[test]
fn signs_in() {
    let c = client(vec![]);
    let pending = c.begin_sign_in().unwrap();
    let id_token = jwt(serde_json::json!({
        "iss": ISSUER, "sub": "user-1", "aud": "app-123", "exp": now() + 600,
        "nonce": pending.nonce, "name": "Marie Curie", "email": "marie@example.com",
    }));
    *c.http.token_replies.lock().unwrap() = vec![token_reply(Some(id_token))];
    let callback = format!("io.felixbennett.paperloom://oauth/callback?code=abc&state={}", pending.state);

    let session = c.finish_sign_in(&pending, &callback).unwrap();
    assert_eq!(session.user.subject, "user-1");
    assert_eq!(session.user.display_name(), "Marie Curie");
    assert_eq!(session.tokens.refresh_token.as_deref(), Some("refresh-1"));
    assert!(!session.tokens.expires_within(60));

    let form = &c.http.forms.lock().unwrap()[0];
    let get = |k: &str| form.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
    assert_eq!(get("grant_type"), Some("authorization_code"));
    assert_eq!(get("code"), Some("abc"));
    assert_eq!(get("code_verifier"), Some(pending.code_verifier.as_str()));
    assert_eq!(get("client_secret"), None);
}

#[test]
fn rejects_bad_callbacks() {
    let c = client(vec![]);
    let pending = c.begin_sign_in().unwrap();
    let base = "io.felixbennett.paperloom://oauth/callback";
    type Expect = fn(&crate::Error) -> bool;
    let cases: [(String, Expect); 5] = [
        (format!("{base}?code=abc&state=other"), |e| matches!(e, crate::Error::StateMismatch)),
        (format!("{base}?error=access_denied&state={}", pending.state), |e| {
            matches!(e, crate::Error::Cancelled)
        }),
        (
            format!("{base}?error=server_error&error_description=down"),
            |e| matches!(e, crate::Error::Provider { error, description } if error == "server_error" && description.as_deref() == Some("down")),
        ),
        (format!("{base}?state={}", pending.state), |e| matches!(e, crate::Error::InvalidCallback(_))),
        ("https://evil.example.com/?code=abc".to_string(), |e| matches!(e, crate::Error::InvalidCallback(_))),
    ];
    for (url, expected) in cases {
        let err = c.finish_sign_in(&pending, &url).unwrap_err();
        assert!(expected(&err), "{url}: {err}");
    }
}

#[test]
fn checks_the_id_token() {
    let pending_for = |c: &Client<FakeHttp>| c.begin_sign_in().unwrap();
    let cases = [
        serde_json::json!({"iss": "https://other", "sub": "u", "aud": "app-123", "exp": now() + 600}),
        serde_json::json!({"iss": ISSUER, "sub": "u", "aud": ["someone-else"], "exp": now() + 600}),
        serde_json::json!({"iss": ISSUER, "sub": "u", "aud": "app-123", "exp": now() - 3600}),
        serde_json::json!({"iss": ISSUER, "sub": "u", "aud": "app-123", "exp": now() + 600, "nonce": "wrong"}),
    ];
    for claims in cases {
        let c = client(vec![token_reply(Some(jwt(claims.clone())))]);
        let pending = pending_for(&c);
        let callback = format!("io.felixbennett.paperloom://oauth/callback?code=abc&state={}", pending.state);
        let err = c.finish_sign_in(&pending, &callback).unwrap_err();
        assert!(matches!(err, crate::Error::InvalidToken(_)), "{claims}: {err}");
    }
}

#[test]
fn refreshes_for_the_sync_service() {
    let c = client(vec![Response::new(
        200,
        r#"{"access_token":"jwt-for-sync","expires_in":600,"token_type":"Bearer"}"#,
    )]);
    let tokens = c.refresh("refresh-1", Some(crate::SYNC_RESOURCE)).unwrap();
    assert_eq!(tokens.access_token, "jwt-for-sync");
    assert_eq!(tokens.refresh_token.as_deref(), Some("refresh-1"), "keeps the old one without rotation");
    let form = &c.http.forms.lock().unwrap()[0];
    assert!(form.contains(&("resource".into(), crate::SYNC_RESOURCE.into())));
    assert!(form.contains(&("grant_type".into(), "refresh_token".into())));
}

#[test]
fn reports_provider_errors() {
    let c =
        client(vec![Response::new(400, r#"{"error":"invalid_grant","error_description":"grant expired"}"#)]);
    let err = c.refresh("old", None).unwrap_err();
    assert_eq!(err.to_string(), "the account service refused: invalid_grant (grant expired)");
}

#[test]
fn builds_the_sign_out_url() {
    let c = client(vec![]);
    let q = query(&c.sign_out_url(Some("id-token")).unwrap());
    assert_eq!(q["post_logout_redirect_uri"][0], "io.felixbennett.paperloom://oauth/signed-out");
    assert_eq!(q["id_token_hint"][0], "id-token");
}

#[test]
fn rejects_a_discovery_for_another_issuer() {
    let mut http = FakeHttp::default();
    http.gets.insert(
        format!("{ISSUER}/.well-known/openid-configuration"),
        Response::new(200, discovery_json().replace(ISSUER, "https://attacker.example.com")),
    );
    let c = Client::with_http(Config { issuer: ISSUER.into(), ..Config::paperloom("app") }, http);
    assert!(c.discovery().is_err());
}
