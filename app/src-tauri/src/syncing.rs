//! The library syncs with Paperloom Cloud, through a folder other devices
//! reach, or through WebDAV, Seafile or an S3 bucket. Credentials are kept
//! with the app's other secrets.

use paperloom_core::{S3Place, SeafilePlace, SyncReport, SyncStatus};
use serde::Deserialize;
use tauri::{AppHandle, Manager, Runtime, State};

use crate::account::Account;
use crate::secrets::Secrets;
use crate::vault::VaultState;
use crate::{AppState, CommandResult};

/// Where to sync.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum SyncTarget {
    Cloud {
        service: String,
    },
    Folder {
        path: String,
    },
    #[serde(rename_all = "camelCase")]
    Webdav {
        address: String,
        username: String,
    },
    #[serde(rename_all = "camelCase")]
    S3 {
        endpoint: String,
        region: String,
        bucket: String,
        prefix: String,
        access_key_id: String,
    },
    Seafile {
        server: String,
        account: String,
        library: String,
        folder: String,
    },
}

/// Keeps the Seafile password or API token; an empty one forgets it.
#[tauri::command]
pub(crate) fn seafile_password_set(secrets: State<'_, Secrets>, password: String) -> CommandResult<()> {
    secrets.update(|kept| kept.seafile_password = Some(password).filter(|p| !p.is_empty()))
}

/// Returns whether a Seafile password or API token is kept.
#[tauri::command]
pub(crate) fn seafile_password_known(secrets: State<'_, Secrets>) -> bool {
    secrets.load().seafile_password.is_some()
}

/// Keeps the S3 secret key; an empty one forgets it.
#[tauri::command]
pub(crate) fn s3_secret_set(secrets: State<'_, Secrets>, secret: String) -> CommandResult<()> {
    secrets.update(|kept| kept.s3_secret = Some(secret.trim().to_string()).filter(|s| !s.is_empty()))
}

/// Whether an S3 secret key is kept.
#[tauri::command]
pub(crate) fn s3_secret_known(secrets: State<'_, Secrets>) -> bool {
    secrets.load().s3_secret.is_some()
}

/// Keeps the WebDAV password; an empty one forgets it.
#[tauri::command]
pub(crate) fn webdav_password_set(secrets: State<'_, Secrets>, password: String) -> CommandResult<()> {
    secrets.update(|kept| kept.webdav_password = Some(password).filter(|p| !p.is_empty()))
}

/// Whether a WebDAV password is kept.
#[tauri::command]
pub(crate) fn webdav_password_known(secrets: State<'_, Secrets>) -> bool {
    secrets.load().webdav_password.is_some()
}

/// Asks for the folder to sync through; none if cancelled. Desktop only:
/// phones sync with Paperloom Cloud or WebDAV.
#[tauri::command]
pub(crate) async fn sync_pick_folder<R: Runtime>(app: AppHandle<R>) -> CommandResult<Option<String>> {
    pick_folder(&app).await
}

#[cfg(desktop)]
async fn pick_folder<R: Runtime>(app: &AppHandle<R>) -> CommandResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog().file();
    let Some(folder) = crate::answer(move || dialog.blocking_pick_folder()).await? else { return Ok(None) };
    Ok(Some(folder.into_path().map_err(|e| e.to_string())?.display().to_string()))
}

#[cfg(mobile)]
async fn pick_folder<R: Runtime>(_app: &AppHandle<R>) -> CommandResult<Option<String>> {
    Err("On this device, sync with Paperloom Cloud or WebDAV.".into())
}

/// This device's name in the account's device list.
fn device_name() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| std::env::consts::OS.to_string())
}

/// Keeps the passphrase that seals sync end to end (an empty one stops
/// sealing) and makes its key, off the main thread.
#[tauri::command]
pub(crate) async fn sync_passphrase_set(
    state: State<'_, AppState>,
    secrets: State<'_, Secrets>,
    passphrase: String,
) -> CommandResult<()> {
    let passphrase = Some(passphrase).filter(|p| !p.is_empty());
    let library = state.library.clone();
    let wanted = passphrase.clone();
    tauri::async_runtime::spawn_blocking(move || library.set_sync_passphrase(wanted))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    secrets.update(|kept| kept.sync_passphrase = passphrase)
}

/// Whether sync seals what it sends end to end.
#[tauri::command]
pub(crate) fn sync_passphrase_known(secrets: State<'_, Secrets>) -> bool {
    secrets.load().sync_passphrase.is_some()
}

/// Whether the library syncs, when it last did, and what waits to go.
#[tauri::command]
pub(crate) fn sync_status(state: State<'_, AppState>) -> CommandResult<SyncStatus> {
    state.library.sync_status().map_err(|e| e.to_string())
}

/// One sync of the library and the vault's notes with `target`; for
/// Paperloom Cloud, with a fresh token for it.
#[tauri::command]
pub(crate) async fn sync_now<R: Runtime>(app: AppHandle<R>, target: SyncTarget) -> CommandResult<SyncReport> {
    let library = app.state::<AppState>().library.clone();
    let vault = app.state::<VaultState>().get().ok();
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // The key is made the first time it is needed after starting.
        let passphrase = handle.state::<Secrets>().load().sync_passphrase;
        if passphrase.is_some() != library.sync_sealed() {
            library.set_sync_passphrase(passphrase).map_err(|e| e.to_string())?;
        }
        let report = match target {
            SyncTarget::Cloud { service } => {
                let token = handle.state::<Account>().sync_token()?;
                library.sync_with_cloud(
                    service,
                    token,
                    device_name(),
                    std::env::consts::OS.to_string(),
                    vault,
                )
            }
            SyncTarget::Folder { path } => library.sync_with_folder(path, vault),
            SyncTarget::Webdav { address, username } => {
                let password = handle
                    .state::<Secrets>()
                    .load()
                    .webdav_password
                    .ok_or("Enter the WebDAV password in Settings.")?;
                library.sync_with_webdav(address, username, password, vault)
            }
            SyncTarget::S3 { endpoint, region, bucket, prefix, access_key_id } => {
                let secret = handle
                    .state::<Secrets>()
                    .load()
                    .s3_secret
                    .ok_or("Enter the S3 secret key in Settings.")?;
                let place = S3Place { endpoint, region, bucket, prefix, access_key_id };
                library.sync_with_s3(place, secret, vault)
            }
            SyncTarget::Seafile { server, account, library: name, folder } => {
                let password = handle
                    .state::<Secrets>()
                    .load()
                    .seafile_password
                    .ok_or("Enter the Seafile password or API token in Settings.")?;
                let place = SeafilePlace { server, account, library: name, folder };
                library.sync_with_seafile(place, password, vault)
            }
        };
        report.map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
