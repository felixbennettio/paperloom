//! PDFs and other attached files, and the marks made on them.
//!
//! The web UI reads a PDF's first pages with pdf.js and hands the text to
//! the core, which recognises the paper. Files come only from the system
//! file dialog: the UI can read a file back only if the person picked it.
//! On Android the dialog gives `content://` addresses, read through the fs
//! plugin and copied to a temporary file for the core.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use paperloom_core::{
    AnnotationInfo, AnnotationLocation, AttachmentInfo, FoundPdf, FullTextHit, NewAnnotation, PageEdit,
    PdfFieldValue, PdfFormField, PdfImport, RecognizedPage,
};
use tauri::ipc::Response;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_dialog::{DialogExt, FilePath};
use tauri_plugin_fs::FsExt;
use tauri_plugin_opener::OpenerExt;

use crate::{AppState, CommandResult};

/// Files picked in a file dialog during this session, by the name the UI
/// knows them by (a path, or a `content://` address on Android).
#[derive(Default)]
pub(crate) struct Picked(Mutex<HashMap<String, FilePath>>);

impl Picked {
    pub(crate) fn remember(&self, files: Vec<FilePath>) -> CommandResult<Vec<String>> {
        let mut known = self.0.lock().map_err(|e| e.to_string())?;
        Ok(files
            .into_iter()
            .map(|file| {
                let key = file.to_string();
                known.insert(key.clone(), file);
                key
            })
            .collect())
    }

    fn get(&self, key: &str) -> CommandResult<FilePath> {
        self.0
            .lock()
            .map_err(|e| e.to_string())?
            .get(key)
            .cloned()
            .ok_or_else(|| format!("{key} was not chosen in a file dialog."))
    }
}

/// A picked file as a path on disk: itself, or (for a `content://`
/// address) a temporary copy, which the caller deletes.
fn local_copy<R: Runtime>(app: &AppHandle<R>, file: &FilePath) -> CommandResult<(PathBuf, bool)> {
    match file {
        FilePath::Path(path) => Ok((path.clone(), false)),
        FilePath::Url(url) => {
            let bytes = app.fs().read(file.clone()).map_err(|e| format!("{url}: {e}"))?;
            let name = url
                .path_segments()
                .and_then(|mut parts| parts.next_back())
                .map(decode)
                .filter(|n| !n.is_empty() && !n.contains(['/', '\\', ':']))
                .unwrap_or_else(|| "document".to_string());
            let name = if name.to_lowercase().ends_with(".pdf") { name } else { format!("{name}.pdf") };
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
            let folder =
                app.path().app_cache_dir().map_err(|e| e.to_string())?.join("picked").join(stamp.to_string());
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            let path = folder.join(name);
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            Ok((path, true))
        }
    }
}

/// `%20` → ` ` in a URL path segment.
fn decode(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&segment[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Asks for PDFs to add; how the UI should name them, or none when
/// cancelled.
#[tauri::command]
pub(crate) async fn pick_pdf_files<R: Runtime>(app: AppHandle<R>) -> CommandResult<Vec<String>> {
    let dialog = app.dialog().file().add_filter("PDF", &["pdf", "PDF"]);
    let Some(files) = crate::answer(move || dialog.blocking_pick_files()).await? else {
        return Ok(Vec::new());
    };
    app.state::<Picked>().remember(files)
}

/// The bytes of a file picked in a dialog, for pdf.js.
#[tauri::command]
pub(crate) fn read_picked_file<R: Runtime>(app: AppHandle<R>, path: String) -> CommandResult<Response> {
    let file = app.state::<Picked>().get(&path)?;
    app.fs().read(file).map(Response::new).map_err(|e| format!("{path}: {e}"))
}

/// Adds a picked PDF; `text` is its first pages' text, `title` the title
/// stored in it. Network, so off the main thread.
#[tauri::command]
pub(crate) async fn import_pdf<R: Runtime>(
    app: AppHandle<R>,
    path: String,
    text: String,
    title: Option<String>,
    collection_id: Option<String>,
) -> CommandResult<PdfImport> {
    let file = app.state::<Picked>().get(&path)?;
    let (local, temporary) = local_copy(&app, &file)?;
    let library = app.state::<AppState>().library.clone();
    let source = local.display().to_string();
    let result =
        tauri::async_runtime::spawn_blocking(move || library.import_pdf(source, text, title, collection_id))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string());
    if temporary && let Some(folder) = local.parent() {
        let _ = std::fs::remove_dir_all(folder);
    }
    result
}

#[tauri::command]
pub(crate) fn attachments(state: State<'_, AppState>, item_id: String) -> CommandResult<Vec<AttachmentInfo>> {
    state.library.attachments(item_id).map_err(|e| e.to_string())
}

/// Asks for files and attaches them to an item.
#[tauri::command]
pub(crate) async fn attach_files<R: Runtime>(
    app: AppHandle<R>,
    item_id: String,
) -> CommandResult<Vec<AttachmentInfo>> {
    let dialog = app.dialog().file();
    let Some(files) = crate::answer(move || dialog.blocking_pick_files()).await? else {
        return Ok(Vec::new());
    };
    let library = app.state::<AppState>().library.clone();
    let mut attached = Vec::new();
    for file in files {
        let (local, temporary) = local_copy(&app, &file)?;
        let result =
            library.attach_file(item_id.clone(), local.display().to_string()).map_err(|e| e.to_string());
        if temporary && let Some(folder) = local.parent() {
            let _ = std::fs::remove_dir_all(folder);
        }
        attached.push(result?);
    }
    Ok(attached)
}

/// Looks for an open-access PDF of an item and attaches it, off the main
/// thread; None when there is none.
#[tauri::command]
pub(crate) async fn find_available_pdf(
    state: State<'_, AppState>,
    item_id: String,
) -> CommandResult<Option<FoundPdf>> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.find_available_pdf(item_id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

fn attachment(state: &AppState, attachment_id: String) -> CommandResult<AttachmentInfo> {
    state
        .library
        .attachment(attachment_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "The file is no longer attached.".to_string())
}

/// An attachment's bytes, for the PDF reader.
#[tauri::command]
pub(crate) fn read_attachment(state: State<'_, AppState>, attachment_id: String) -> CommandResult<Response> {
    let file = attachment(&state, attachment_id)?;
    std::fs::read(&file.file_path).map(Response::new).map_err(|e| format!("{}: {e}", file.file_path))
}

/// Changes a PDF's pages (the file is rewritten, so off the main thread).
#[tauri::command]
pub(crate) async fn edit_pdf_pages(
    state: State<'_, AppState>,
    attachment_id: String,
    edit: PageEdit,
) -> CommandResult<AttachmentInfo> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.edit_pdf_pages(attachment_id, edit))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Writes recognised lines into a PDF as invisible text over its scanned
/// pages (the file is rewritten, so off the main thread).
#[tauri::command]
pub(crate) async fn add_text_layer(
    state: State<'_, AppState>,
    attachment_id: String,
    pages: Vec<RecognizedPage>,
) -> CommandResult<AttachmentInfo> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.add_text_layer(attachment_id, pages))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// The fields of a PDF's form.
#[tauri::command]
pub(crate) async fn pdf_form_fields(
    state: State<'_, AppState>,
    attachment_id: String,
) -> CommandResult<Vec<PdfFormField>> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.pdf_form_fields(attachment_id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Fills in a PDF's form (the file is rewritten, so off the main thread).
#[tauri::command]
pub(crate) async fn fill_pdf_form(
    state: State<'_, AppState>,
    attachment_id: String,
    values: Vec<PdfFieldValue>,
) -> CommandResult<AttachmentInfo> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.fill_pdf_form(attachment_id, values))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Asks where to save a copy of a PDF with its marks written into it, and
/// writes it; the path, or none if cancelled.
#[tauri::command]
pub(crate) async fn export_annotated_pdf<R: Runtime>(
    app: AppHandle<R>,
    attachment_id: String,
) -> CommandResult<Option<String>> {
    #[cfg(desktop)]
    {
        let file = attachment(&app.state::<AppState>(), attachment_id.clone())?;
        let stem = file.title.trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
        let dialog =
            app.dialog().file().add_filter("PDF", &["pdf"]).set_file_name(format!("{stem} (annotated).pdf"));
        let Some(target) = crate::answer(move || dialog.blocking_save_file()).await? else {
            return Ok(None);
        };
        let target = target.into_path().map_err(|e| e.to_string())?.display().to_string();
        let library = app.state::<AppState>().library.clone();
        let written = target.clone();
        tauri::async_runtime::spawn_blocking(move || library.export_annotated_pdf(attachment_id, written))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        Ok(Some(target))
    }
    #[cfg(mobile)]
    {
        let _ = (app, attachment_id);
        Err("Annotated copies are saved on a computer.".into())
    }
}

/// Opens an attachment in the app the system uses for its type.
#[tauri::command]
pub(crate) fn open_attachment<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    attachment_id: String,
) -> CommandResult<()> {
    let file = attachment(&state, attachment_id)?;
    app.opener().open_path(file.file_path, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn remove_attachment(state: State<'_, AppState>, attachment_id: String) -> CommandResult<()> {
    state.library.remove_attachment(attachment_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn annotations(
    state: State<'_, AppState>,
    attachment_id: String,
) -> CommandResult<Vec<AnnotationInfo>> {
    state.library.annotations(attachment_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn add_annotation(
    state: State<'_, AppState>,
    annotation: NewAnnotation,
) -> CommandResult<AnnotationInfo> {
    state.library.add_annotation(annotation).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn update_annotation(
    state: State<'_, AppState>,
    annotation_id: String,
    color: String,
    comment: Option<String>,
) -> CommandResult<AnnotationInfo> {
    state.library.update_annotation(annotation_id, color, comment).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn remove_annotation(state: State<'_, AppState>, annotation_id: String) -> CommandResult<()> {
    state.library.remove_annotation(annotation_id).map_err(|e| e.to_string())
}

/// Keeps a PDF's page text (read by pdf.js) for full-text search.
#[tauri::command]
pub(crate) async fn set_attachment_text(
    state: State<'_, AppState>,
    attachment_id: String,
    pages: Vec<String>,
) -> CommandResult<()> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.set_attachment_text(attachment_id, pages))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn attachment_has_text(state: State<'_, AppState>, attachment_id: String) -> CommandResult<bool> {
    state.library.attachment_has_text(attachment_id).map_err(|e| e.to_string())
}

/// Searches items, annotations, PDF text and notes.
#[tauri::command]
pub(crate) fn search_everything(
    state: State<'_, AppState>,
    query: String,
    kinds: Vec<String>,
    limit: u32,
) -> CommandResult<Vec<FullTextHit>> {
    state.library.search_everything(query, kinds, limit).map_err(|e| e.to_string())
}

/// Markdown that embeds a highlight in a note.
#[tauri::command]
pub(crate) fn annotation_embed(state: State<'_, AppState>, annotation_id: String) -> CommandResult<String> {
    state.library.annotation_embed(annotation_id).map_err(|e| e.to_string())
}

/// Markdown that links a note to an item.
#[tauri::command]
pub(crate) fn item_link(state: State<'_, AppState>, item_id: String) -> CommandResult<String> {
    state.library.item_link(item_id).map_err(|e| e.to_string())
}

/// The `[@key]` citation of some items, for notes and Word documents.
#[tauri::command]
pub(crate) fn citation_marker(state: State<'_, AppState>, item_ids: Vec<String>) -> CommandResult<String> {
    state.library.citation_marker(item_ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn annotation_location(
    state: State<'_, AppState>,
    annotation_id: String,
) -> CommandResult<Option<AnnotationLocation>> {
    state.library.annotation_location(annotation_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn resolve_item_reference(
    state: State<'_, AppState>,
    reference: String,
) -> CommandResult<Option<String>> {
    state.library.resolve_item_reference(reference).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn attachment_info(
    state: State<'_, AppState>,
    attachment_id: String,
) -> CommandResult<Option<AttachmentInfo>> {
    state.library.attachment(attachment_id).map_err(|e| e.to_string())
}
