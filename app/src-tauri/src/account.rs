//! Signing in on Windows, Linux and Android.
//!
//! `account_sign_in` opens the account service's page in the system browser;
//! the browser comes back through the `io.felixbennett.paperloom://` deep
//! link, which `handle_callback` finishes. The session (refresh token, ID
//! token and the user's name) is kept in `account.json` in the app's private
//! data folder, sealed for this user (see `private`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use paperloom_core::{AccountService, AccountSession, AccountUser, CoreError, SignInRequest};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_opener::OpenerExt;

use crate::private::{read_private, write_private};

/// The app's ID in the account service (a public, native client).
const CLIENT_ID: &str = "v5laws0wyscuf6877geh5";
const CALLBACK: &str = "io.felixbennett.paperloom://oauth/callback";
/// Emitted with the signed-in user, or `null` after signing out.
pub const CHANGED_EVENT: &str = "account-changed";
/// Emitted with a message when signing in fails.
pub const ERROR_EVENT: &str = "account-error";

pub struct Account {
    service: Option<Arc<AccountService>>,
    pending: Mutex<Option<SignInRequest>>,
    file: PathBuf,
    /// One token refresh at a time: the service rotates refresh tokens, and
    /// a second refresh with the same one would fail.
    refreshing: Mutex<()>,
}

/// What is kept between launches.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSession {
    user: AccountUser,
    refresh_token: Option<String>,
    id_token: Option<String>,
}

impl Account {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            service: AccountService::new(CLIENT_ID.to_string()).ok(),
            pending: Mutex::new(None),
            file: data_dir.join("account.json"),
            refreshing: Mutex::new(()),
        }
    }

    fn load(&self) -> Option<StoredSession> {
        serde_json::from_str(&read_private(&self.file)?).ok()
    }

    /// An access token for the sync service, from the kept session. The
    /// refresh token that comes back with it replaces the kept one.
    pub(crate) fn sync_token(&self) -> Result<String, String> {
        let service = self.service.clone().ok_or("Signing in is not set up in this build.")?;
        let _one_at_a_time = self.refreshing.lock().map_err(|e| e.to_string())?;
        let mut stored = self.load().ok_or("Sign in to sync your library.")?;
        let refresh_token = stored.refresh_token.clone().ok_or("Sign in again to sync your library.")?;
        let tokens = service.refresh(refresh_token, true).map_err(message)?;
        if let Some(rotated) = tokens.refresh_token {
            stored.refresh_token = Some(rotated);
            let text = serde_json::to_string(&stored).map_err(|e| e.to_string())?;
            write_private(&self.file, &text).map_err(|e| e.to_string())?;
        }
        Ok(tokens.access_token)
    }

    fn save(&self, session: &AccountSession) -> Result<(), String> {
        let stored = StoredSession {
            user: session.user.clone(),
            refresh_token: session.tokens.refresh_token.clone(),
            id_token: session.tokens.id_token.clone(),
        };
        let text = serde_json::to_string(&stored).map_err(|e| e.to_string())?;
        write_private(&self.file, &text).map_err(|e| e.to_string())
    }
}

fn message(err: CoreError) -> String {
    err.to_string()
}

/// The signed-in user, if any.
#[tauri::command]
pub fn account_status(account: tauri::State<'_, Account>) -> Option<AccountUser> {
    account.load().map(|s| s.user)
}

/// False when this build cannot sign in.
#[tauri::command]
pub fn account_available(account: tauri::State<'_, Account>) -> bool {
    account.service.is_some()
}

/// Opens the sign-in page in the system browser. The result arrives as an
/// `account-changed` or `account-error` event.
#[tauri::command]
pub async fn account_sign_in<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let account = app.state::<Account>();
    let service = account.service.clone().ok_or("Signing in is not set up in this build.")?;
    let request = tauri::async_runtime::spawn_blocking(move || service.begin_sign_in())
        .await
        .map_err(|e| e.to_string())?
        .map_err(message)?;
    let url = request.url.clone();
    *account.pending.lock().map_err(|e| e.to_string())? = Some(request);
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn account_sign_out<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let account = app.state::<Account>();
    match std::fs::remove_file(&account.file) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    app.emit(CHANGED_EVENT, Option::<AccountUser>::None).map_err(|e| e.to_string())
}

/// Finishes a sign-in when the browser comes back through the deep link.
pub fn handle_callback<R: Runtime>(app: &AppHandle<R>, url: &str) {
    if !url.starts_with(CALLBACK) {
        return;
    }
    let account = app.state::<Account>();
    let (Some(service), Some(request)) =
        (account.service.clone(), account.pending.lock().ok().and_then(|mut p| p.take()))
    else {
        return;
    };
    let app = app.clone();
    let url = url.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let result = service.finish_sign_in(request, url).map_err(message).and_then(|session| {
            app.state::<Account>().save(&session)?;
            Ok(session.user)
        });
        let _ = match result {
            Ok(user) => app.emit(CHANGED_EVENT, Some(user)),
            Err(e) => app.emit(ERROR_EVENT, e),
        };
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_focus();
        }
    });
}
