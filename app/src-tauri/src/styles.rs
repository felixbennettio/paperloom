//! Citation styles beyond the built-in ones: searched for in and downloaded
//! from the CSL style repository, or added from a `.csl` file.

use paperloom_core::{CitationStyle, StyleListing};
use tauri::{AppHandle, Runtime, State};

use crate::{AppState, CommandResult};

/// Searches the style repository; network, so off the main thread.
#[tauri::command]
pub(crate) async fn search_citation_styles(
    state: State<'_, AppState>,
    query: String,
    limit: u32,
) -> CommandResult<Vec<StyleListing>> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.search_citation_styles(query, limit))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn download_citation_style(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<CitationStyle> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.download_citation_style(id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Asks for a `.csl` file and adds its style; none if cancelled.
#[tauri::command]
pub(crate) async fn add_citation_style_file<R: Runtime>(
    app: AppHandle<R>,
) -> CommandResult<Option<CitationStyle>> {
    #[cfg(desktop)]
    {
        use tauri::Manager;
        use tauri_plugin_dialog::DialogExt;

        let dialog = app.dialog().file().add_filter("CSL", &["csl"]);
        let Some(file) = crate::answer(move || dialog.blocking_pick_file()).await? else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        let library = app.state::<AppState>().library.clone();
        tauri::async_runtime::spawn_blocking(move || {
            library.add_citation_style_file(path.display().to_string())
        })
        .await
        .map_err(|e| e.to_string())?
        .map(Some)
        .map_err(|e| e.to_string())
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err("Style files are added on a computer; download styles instead.".into())
    }
}

#[tauri::command]
pub(crate) fn remove_citation_style(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    state.library.remove_citation_style(id).map_err(|e| e.to_string())
}
