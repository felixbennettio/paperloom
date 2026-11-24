//! Backing the library up to a `.zip` and putting a backup back (desktop:
//! both pick a file in a dialog). Putting one back finishes when the app
//! starts again.

use paperloom_core::{BackupReport, RestoreReport};
use tauri::{AppHandle, Runtime};

use crate::CommandResult;

/// Asks where to save a backup and writes it; none if cancelled. Reads
/// every file, so off the main thread.
#[tauri::command]
pub(crate) async fn back_up_library<R: Runtime>(
    app: AppHandle<R>,
    name: String,
) -> CommandResult<Option<BackupReport>> {
    #[cfg(desktop)]
    {
        use tauri::Manager;
        use tauri_plugin_dialog::DialogExt;

        use crate::AppState;
        use crate::vault::VaultState;

        let dialog = app.dialog().file().add_filter("ZIP", &["zip"]).set_file_name(&name);
        let Some(path) = crate::answer(move || dialog.blocking_save_file()).await? else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|e| e.to_string())?;
        let library = app.state::<AppState>().library.clone();
        let vault = app.state::<VaultState>().get().ok();
        tauri::async_runtime::spawn_blocking(move || library.back_up(path.display().to_string(), vault))
            .await
            .map_err(|e| e.to_string())?
            .map(Some)
            .map_err(|e| e.to_string())
    }
    #[cfg(mobile)]
    {
        let _ = (app, name);
        Err("Back up on a computer.".into())
    }
}

/// Backs up into `folder` (by default `backups/` beside the library) when
/// the newest automatic backup is older than `every_hours`, keeping the
/// newest `keep`; none if not due. Reads every file, so off the main thread.
#[tauri::command]
pub(crate) async fn back_up_if_due<R: Runtime>(
    app: AppHandle<R>,
    folder: Option<String>,
    every_hours: u32,
    keep: u32,
) -> CommandResult<Option<BackupReport>> {
    use tauri::Manager;

    use crate::AppState;
    use crate::vault::VaultState;

    let library = app.state::<AppState>().library.clone();
    let vault = app.state::<VaultState>().get().ok();
    tauri::async_runtime::spawn_blocking(move || library.back_up_if_due(folder, vault, every_hours, keep))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Asks for a backup and puts it back: the notes now, the library when the
/// app starts again. None if cancelled.
#[tauri::command]
pub(crate) async fn restore_library<R: Runtime>(app: AppHandle<R>) -> CommandResult<Option<RestoreReport>> {
    #[cfg(desktop)]
    {
        use tauri::Manager;
        use tauri_plugin_dialog::DialogExt;

        use crate::AppState;
        use crate::vault::VaultState;

        let dialog = app.dialog().file().add_filter("ZIP", &["zip"]);
        let Some(path) = crate::answer(move || dialog.blocking_pick_file()).await? else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|e| e.to_string())?;
        let library = app.state::<AppState>().library.clone();
        let vault = app.state::<VaultState>().get().ok();
        tauri::async_runtime::spawn_blocking(move || {
            library.restore_backup(path.display().to_string(), vault)
        })
        .await
        .map_err(|e| e.to_string())?
        .map(Some)
        .map_err(|e| e.to_string())
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err("Put a backup back on a computer.".into())
    }
}

/// Starts the app again (to finish putting a backup back).
#[tauri::command]
pub(crate) fn restart_app<R: Runtime>(app: AppHandle<R>) {
    app.restart();
}
