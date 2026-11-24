//! Moving an EndNote library in (desktop): its XML export is picked, then
//! its `.Data` folder when that is not beside the export; the core copies
//! items, PDFs and research notes.

use paperloom_core::EndNoteImport;
use tauri::{AppHandle, Runtime};

use crate::CommandResult;

/// Asks for an EndNote XML export (and, if needed, the library's `.Data`
/// folder for its PDFs) and moves the library in; none if cancelled.
/// Copies files, so off the main thread.
#[tauri::command]
pub(crate) async fn import_endnote<R: Runtime>(app: AppHandle<R>) -> CommandResult<Option<EndNoteImport>> {
    #[cfg(desktop)]
    {
        use tauri::Manager;
        use tauri_plugin_dialog::DialogExt;

        use crate::AppState;
        use crate::vault::VaultState;

        let dialog = app.dialog().file().add_filter("EndNote XML", &["xml"]);
        let Some(file) = crate::answer(move || dialog.blocking_pick_file()).await? else {
            return Ok(None);
        };
        let xml = file.into_path().map_err(|e| e.to_string())?.display().to_string();
        let data = match paperloom_core::find_endnote_data_folder(xml.clone()) {
            Some(found) => Some(found),
            // Without it the items come in, but not their PDFs.
            None => {
                let dialog = app.dialog().file().set_title("EndNote .Data");
                crate::answer(move || dialog.blocking_pick_folder())
                    .await?
                    .and_then(|folder| folder.into_path().ok())
                    .map(|path| path.display().to_string())
            }
        };
        let library = app.state::<AppState>().library.clone();
        let vault = app.state::<VaultState>().get().ok();
        tauri::async_runtime::spawn_blocking(move || library.import_endnote(xml, data, vault))
            .await
            .map_err(|e| e.to_string())?
            .map(Some)
            .map_err(|e| e.to_string())
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err("An EndNote library moves in on a computer.".into())
    }
}
