//! Moving a Zotero library in (desktop): its data folder is picked, and the
//! core copies items, files, annotations and notes.

use paperloom_core::ZoteroImport;
use tauri::{AppHandle, Runtime};

use crate::CommandResult;

/// Asks for Zotero's data folder (starting at `~/Zotero`) and moves its
/// library in, notes into the vault; none if cancelled. Copies files, so
/// off the main thread.
#[tauri::command]
pub(crate) async fn import_zotero<R: Runtime>(app: AppHandle<R>) -> CommandResult<Option<ZoteroImport>> {
    #[cfg(desktop)]
    {
        use tauri::Manager;
        use tauri_plugin_dialog::DialogExt;

        use crate::AppState;
        use crate::vault::VaultState;

        let mut dialog = app.dialog().file().set_title("Zotero");
        if let Some(start) = app.path().home_dir().ok().map(|home| home.join("Zotero")).filter(|p| p.is_dir())
        {
            dialog = dialog.set_directory(start);
        }
        let Some(folder) = crate::answer(move || dialog.blocking_pick_folder()).await? else {
            return Ok(None);
        };
        let folder = folder.into_path().map_err(|e| e.to_string())?;
        let library = app.state::<AppState>().library.clone();
        let vault = app.state::<VaultState>().get().ok();
        tauri::async_runtime::spawn_blocking(move || {
            library.import_zotero(folder.display().to_string(), vault)
        })
        .await
        .map_err(|e| e.to_string())?
        .map(Some)
        .map_err(|e| e.to_string())
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err("A Zotero library moves in on a computer.".into())
    }
}
