//! Signing in, for the apps. The apps open `SignInRequest::url` in the
//! system browser (ASWebAuthenticationSession on Apple platforms), hand the
//! callback URL to `finish_sign_in`, and keep the tokens in the platform's
//! secure storage. Every call here talks to the network and blocks: call it
//! off the main thread.

use std::sync::Arc;

use paperloom_account as account;
use serde::{Deserialize, Serialize};

use crate::CoreError;

impl From<account::Error> for CoreError {
    fn from(err: account::Error) -> Self {
        let message = err.to_string();
        match err {
            account::Error::Cancelled => CoreError::Cancelled { message },
            account::Error::Network(_) => CoreError::Network { message },
            _ => CoreError::Account { message },
        }
    }
}

/// A sign-in in progress: open `url`, keep the rest for the callback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SignInRequest {
    pub url: String,
    pub state: String,
    pub nonce: String,
    pub code_verifier: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct AccountTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    /// Unix seconds.
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct AccountUser {
    pub subject: String,
    pub display_name: String,
    pub email: Option<String>,
    pub picture: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct AccountSession {
    pub user: AccountUser,
    pub tokens: AccountTokens,
}

impl From<account::Tokens> for AccountTokens {
    fn from(t: account::Tokens) -> Self {
        Self {
            access_token: t.access_token,
            refresh_token: t.refresh_token,
            id_token: t.id_token,
            expires_at: t.expires_at,
        }
    }
}

impl From<account::Session> for AccountSession {
    fn from(s: account::Session) -> Self {
        Self {
            user: AccountUser {
                display_name: s.user.display_name().to_string(),
                subject: s.user.subject,
                email: s.user.email,
                picture: s.user.picture,
            },
            tokens: s.tokens.into(),
        }
    }
}

/// The Paperloom account service (OpenID Connect with PKCE).
#[derive(uniffi::Object)]
pub struct AccountService {
    client: account::Client,
}

#[uniffi::export]
impl AccountService {
    /// `client_id` is the app's ID in the account service; empty means sign-in
    /// is not set up in this build.
    #[uniffi::constructor]
    pub fn new(client_id: String) -> Result<Arc<Self>, CoreError> {
        if client_id.trim().is_empty() {
            return Err(CoreError::Account { message: "Signing in is not set up in this build.".into() });
        }
        Ok(Arc::new(Self { client: account::Client::new(account::Config::paperloom(client_id.trim())) }))
    }

    pub fn begin_sign_in(&self) -> Result<SignInRequest, CoreError> {
        let p = self.client.begin_sign_in()?;
        Ok(SignInRequest { url: p.url, state: p.state, nonce: p.nonce, code_verifier: p.code_verifier })
    }

    pub fn finish_sign_in(
        &self,
        request: SignInRequest,
        callback_url: String,
    ) -> Result<AccountSession, CoreError> {
        let pending = account::PendingSignIn {
            url: request.url,
            state: request.state,
            nonce: request.nonce,
            code_verifier: request.code_verifier,
        };
        Ok(self.client.finish_sign_in(&pending, &callback_url)?.into())
    }

    /// Fresh tokens; with `for_sync`, an access token for the sync service.
    pub fn refresh(&self, refresh_token: String, for_sync: bool) -> Result<AccountTokens, CoreError> {
        let resource = for_sync.then_some(account::SYNC_RESOURCE);
        Ok(self.client.refresh(&refresh_token, resource)?.into())
    }

    /// The page that ends the session in the browser.
    pub fn sign_out_url(&self, id_token: Option<String>) -> Result<String, CoreError> {
        Ok(self.client.sign_out_url(id_token.as_deref())?)
    }
}

/// The URL scheme sign-in callbacks arrive on.
#[uniffi::export]
pub fn account_callback_scheme() -> String {
    account::REDIRECT_URI.split("://").next().unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_a_client_id() {
        assert!(matches!(AccountService::new("  ".into()), Err(CoreError::Account { .. })));
        assert!(AccountService::new("app".into()).is_ok());
    }

    #[test]
    fn maps_errors() {
        assert!(matches!(CoreError::from(account::Error::Cancelled), CoreError::Cancelled { .. }));
        assert!(matches!(CoreError::from(account::Error::Network("down".into())), CoreError::Network { .. }));
        assert!(matches!(CoreError::from(account::Error::StateMismatch), CoreError::Account { .. }));
    }

    #[test]
    fn callback_scheme() {
        assert_eq!(account_callback_scheme(), "io.felixbennett.paperloom");
    }
}
