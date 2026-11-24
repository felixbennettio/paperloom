//! Citations in a Word document: a copy of it with them formatted.

use paperloom_core::WordCitations;
use serde::Serialize;
use tauri::{AppHandle, Runtime};

use crate::CommandResult;

/// Where the copy went, and what became of its citations.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CitedDocument {
    path: String,
    #[serde(flatten)]
    citations: WordCitations,
}

/// Asks for a Word (or OpenDocument) document, then where to save a copy
/// of it with its `[@citations]` formatted in a style and the reference
/// list added, and writes the copy; none if cancelled. `copy_name` names
/// the copy, with `{name}` for the document's own name.
#[tauri::command]
pub(crate) async fn cite_word_document<R: Runtime>(
    app: AppHandle<R>,
    style_id: String,
    copy_name: String,
) -> CommandResult<Option<CitedDocument>> {
    #[cfg(desktop)]
    {
        use tauri::Manager;
        use tauri_plugin_dialog::DialogExt;

        let dialog = app.dialog().file().add_filter("Word, OpenDocument", &["docx", "odt"]);
        let Some(source) = crate::answer(move || dialog.blocking_pick_file()).await? else {
            return Ok(None);
        };
        let source = source.into_path().map_err(|e| e.to_string())?;
        let name = source.file_stem().and_then(|s| s.to_str()).unwrap_or("Document");
        // The copy is the same kind of document as the original.
        let extension = match source.extension().and_then(|e| e.to_str()) {
            Some(e) if e.eq_ignore_ascii_case("odt") => "odt",
            _ => "docx",
        };
        let mut dialog = app
            .dialog()
            .file()
            .add_filter(if extension == "odt" { "OpenDocument" } else { "Word" }, &[extension])
            .set_file_name(format!("{}.{extension}", copy_name.replace("{name}", name)));
        if let Some(folder) = source.parent() {
            dialog = dialog.set_directory(folder);
        }
        let Some(target) = crate::answer(move || dialog.blocking_save_file()).await? else { return Ok(None) };
        let target = target.into_path().map_err(|e| e.to_string())?.display().to_string();
        let library = app.state::<crate::AppState>().library.clone();
        let (source, written) = (source.display().to_string(), target.clone());
        let citations = tauri::async_runtime::spawn_blocking(move || {
            library.cite_word_document(source, written, style_id)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        Ok(Some(CitedDocument { path: target, citations }))
    }
    #[cfg(mobile)]
    {
        let _ = (app, style_id, copy_name);
        Err("Word documents are made on a computer.".into())
    }
}
