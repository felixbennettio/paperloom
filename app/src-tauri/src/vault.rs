//! The Markdown vault. It opens at `<app data>/Vault`, or at a folder the
//! person picks (an existing Obsidian vault works), remembered in
//! `vault-folder.txt`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use paperloom_core::{
    CanvasBoard, DiffLine, DrawingPage, LinkCompletion, LinkSuggestion, LocalTime, NoteGraph, NoteInfo,
    NoteMention, NoteOutline, NoteProperty, NoteVersion, PaperLibrary, PaperVault, VaultSettings, VaultTag,
};
use tauri::ipc::Response;
use tauri::{AppHandle, Manager, Runtime, State};
#[cfg(desktop)]
use tauri_plugin_dialog::DialogExt;

use crate::CommandResult;

pub(crate) struct VaultState {
    vault: Mutex<Arc<PaperVault>>,
    /// Where the chosen folder is remembered.
    choice_file: PathBuf,
    /// Whose full-text index holds the notes.
    library: Arc<PaperLibrary>,
}

impl VaultState {
    /// The remembered folder, else `Vault` in the app's data folder.
    pub(crate) fn open(data_dir: &Path, library: Arc<PaperLibrary>) -> Result<Self, String> {
        let choice_file = data_dir.join("vault-folder.txt");
        let chosen = std::fs::read_to_string(&choice_file)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let vault = chosen
            .and_then(|path| PaperVault::open(path).ok())
            .map_or_else(|| PaperVault::open(data_dir.join("Vault").display().to_string()), Ok)
            .map_err(|e| e.to_string())?;
        // Indexing the notes can take a moment in a big vault.
        let (connecting, index) = (vault.clone(), library.clone());
        std::thread::spawn(move || connecting.connect_library(index));
        Ok(Self { vault: Mutex::new(vault), choice_file, library })
    }

    pub(crate) fn get(&self) -> CommandResult<Arc<PaperVault>> {
        self.vault.lock().map(|v| v.clone()).map_err(|e| e.to_string())
    }
}

fn run<T>(
    state: &State<'_, VaultState>,
    f: impl FnOnce(&PaperVault) -> Result<T, paperloom_core::CoreError>,
) -> CommandResult<T> {
    let vault = state.get()?;
    f(&vault).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn vault_root(state: State<'_, VaultState>) -> CommandResult<String> {
    run(&state, |v| v.root())
}

/// Asks for a folder and makes it the vault; its path, or none if cancelled.
#[tauri::command]
pub(crate) async fn vault_choose_folder<R: Runtime>(app: AppHandle<R>) -> CommandResult<Option<String>> {
    let Some(path) = pick_folder(&app).await? else { return Ok(None) };
    let vault = PaperVault::open(path.clone()).map_err(|e| e.to_string())?;
    let state = app.state::<VaultState>();
    vault.connect_library(state.library.clone());
    *state.vault.lock().map_err(|e| e.to_string())? = vault;
    std::fs::write(&state.choice_file, &path).map_err(|e| e.to_string())?;
    Ok(Some(path))
}

#[cfg(desktop)]
async fn pick_folder<R: Runtime>(app: &AppHandle<R>) -> CommandResult<Option<String>> {
    let dialog = app.dialog().file();
    let Some(folder) = crate::answer(move || dialog.blocking_pick_folder()).await? else { return Ok(None) };
    Ok(Some(folder.into_path().map_err(|e| e.to_string())?.display().to_string()))
}

/// Phones keep the vault in the app's own folder.
#[cfg(mobile)]
async fn pick_folder<R: Runtime>(_app: &AppHandle<R>) -> CommandResult<Option<String>> {
    Err("On this device, notes are kept in the app's own folder.".into())
}

#[tauri::command]
pub(crate) fn vault_refresh(state: State<'_, VaultState>) -> CommandResult<bool> {
    run(&state, |v| v.refresh())
}

#[tauri::command]
pub(crate) fn vault_notes(state: State<'_, VaultState>) -> CommandResult<Vec<NoteInfo>> {
    run(&state, |v| v.notes())
}

#[tauri::command]
pub(crate) fn vault_folders(state: State<'_, VaultState>) -> CommandResult<Vec<String>> {
    run(&state, |v| v.folders())
}

#[tauri::command]
pub(crate) fn vault_read(state: State<'_, VaultState>, path: String) -> CommandResult<String> {
    run(&state, |v| v.read_note(path))
}

#[tauri::command]
pub(crate) fn vault_write(
    state: State<'_, VaultState>,
    path: String,
    text: String,
) -> CommandResult<NoteInfo> {
    run(&state, |v| v.write_note(path, text))
}

#[tauri::command]
pub(crate) fn vault_create_note(
    state: State<'_, VaultState>,
    folder: String,
    title: String,
) -> CommandResult<NoteInfo> {
    run(&state, |v| v.create_note(folder, title))
}

#[tauri::command]
pub(crate) fn vault_create_folder(state: State<'_, VaultState>, path: String) -> CommandResult<()> {
    run(&state, |v| v.create_folder(path))
}

#[tauri::command]
pub(crate) fn vault_rename(
    state: State<'_, VaultState>,
    path: String,
    new_path: String,
) -> CommandResult<u32> {
    run(&state, |v| v.rename_note(path, new_path))
}

#[tauri::command]
pub(crate) fn vault_delete(state: State<'_, VaultState>, path: String) -> CommandResult<()> {
    run(&state, |v| v.delete_note(path))
}

/// A note's outline laid out as a mind map.
#[tauri::command]
pub(crate) fn mindmap_layout(
    text: String,
    title: String,
    style: String,
    folded: Vec<String>,
) -> paperloom_core::MindMapLayout {
    paperloom_core::mindmap_layout(text, title, style, folded)
}

/// Changes a mind map; the note's new text and the topic to show next.
#[tauri::command]
pub(crate) fn mindmap_edit(
    text: String,
    title: String,
    edit: paperloom_core::MindMapEdit,
) -> CommandResult<paperloom_core::MindMapChange> {
    paperloom_core::mindmap_edit(text, title, edit).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn new_mindmap_text(title: String) -> String {
    paperloom_core::new_mindmap_text(title)
}

/// The themes a mind map can take.
#[tauri::command]
pub(crate) fn mindmap_themes() -> Vec<paperloom_core::MindMapTheme> {
    paperloom_core::mindmap_themes()
}

/// The markers mind map topics can carry.
#[tauri::command]
pub(crate) fn mindmap_markers() -> Vec<paperloom_core::MindMarker> {
    paperloom_core::mindmap_markers()
}

/// A mind map note as an SVG picture.
#[tauri::command]
pub(crate) fn mindmap_svg(text: String, title: String, style: String, folded: Vec<String>) -> String {
    paperloom_core::mindmap_svg(text, title, style, folded)
}

/// Asks where to save a mind map as `format` (`xmind`, `opml`, `mm`, `svg`,
/// or `png` with the picture the web view drew, in base64) and writes it;
/// none if cancelled.
#[tauri::command]
pub(crate) async fn mindmap_export<R: Runtime>(
    app: AppHandle<R>,
    text: String,
    title: String,
    format: String,
    png: Option<String>,
) -> CommandResult<Option<String>> {
    #[cfg(desktop)]
    {
        use base64::Engine as _;

        let kind = match format.as_str() {
            "xmind" => "XMind",
            "opml" => "OPML",
            "mm" => "FreeMind",
            "svg" => "SVG",
            "png" => "PNG",
            "pdf" => "PDF",
            _ => return Err(format!("Mind maps can't be saved as .{format} files")),
        };
        let dialog = app
            .dialog()
            .file()
            .add_filter(kind, &[format.as_str()])
            .set_file_name(format!("{title}.{format}"));
        let Some(target) = crate::answer(move || dialog.blocking_save_file()).await? else { return Ok(None) };
        let target = target.into_path().map_err(|e| e.to_string())?.display().to_string();
        match png.filter(|_| format == "png" || format == "pdf") {
            Some(picture) => {
                let bytes =
                    base64::engine::general_purpose::STANDARD.decode(picture).map_err(|e| e.to_string())?;
                let bytes = if format == "pdf" {
                    paperloom_core::picture_pdf(bytes).map_err(|e| e.to_string())?
                } else {
                    bytes
                };
                std::fs::write(&target, bytes).map_err(|e| e.to_string())?;
            }
            None => {
                let written = target.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    paperloom_core::export_mindmap(text, title, written)
                })
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            }
        }
        Ok(Some(target))
    }
    #[cfg(mobile)]
    {
        let _ = (app, text, title, format, png);
        Err("Mind map files are made on a computer.".into())
    }
}

/// Asks where to save a drawing as an SVG picture, or a PNG one (`png`, the
/// picture the web view drew, in base64), and writes it; none if cancelled.
#[tauri::command]
pub(crate) async fn drawing_export<R: Runtime>(
    app: AppHandle<R>,
    path: String,
    format: String,
    png: Option<String>,
) -> CommandResult<Option<String>> {
    #[cfg(desktop)]
    {
        use base64::Engine as _;

        let (kind, bytes) = match (format.as_str(), png) {
            ("svg", _) => {
                let vault = app.state::<VaultState>().get()?;
                ("SVG", vault.drawing_svg(path.clone()).map_err(|e| e.to_string())?.into_bytes())
            }
            ("png", Some(png)) => {
                ("PNG", base64::engine::general_purpose::STANDARD.decode(png).map_err(|e| e.to_string())?)
            }
            ("pdf", Some(picture)) => {
                let jpeg =
                    base64::engine::general_purpose::STANDARD.decode(picture).map_err(|e| e.to_string())?;
                ("PDF", paperloom_core::picture_pdf(jpeg).map_err(|e| e.to_string())?)
            }
            _ => return Err(format!("Drawings can't be saved as .{format} files")),
        };
        let name = path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".excalidraw").to_string();
        let dialog = app
            .dialog()
            .file()
            .add_filter(kind, &[format.as_str()])
            .set_file_name(format!("{name}.{format}"));
        let Some(target) = crate::answer(move || dialog.blocking_save_file()).await? else { return Ok(None) };
        let target = target.into_path().map_err(|e| e.to_string())?;
        std::fs::write(&target, bytes).map_err(|e| e.to_string())?;
        Ok(Some(target.display().to_string()))
    }
    #[cfg(mobile)]
    {
        let _ = (app, path, format, png);
        Err("Pictures are saved on a computer.".into())
    }
}

/// Asks for an XMind, OPML or FreeMind file and makes a mind map note of
/// it in `folder`; none if cancelled.
#[tauri::command]
pub(crate) async fn vault_import_mindmap<R: Runtime>(
    app: AppHandle<R>,
    folder: String,
) -> CommandResult<Option<NoteInfo>> {
    #[cfg(desktop)]
    {
        let dialog = app.dialog().file().add_filter("XMind, OPML, FreeMind", &["xmind", "opml", "mm"]);
        let Some(file) = crate::answer(move || dialog.blocking_pick_file()).await? else { return Ok(None) };
        let source = file.into_path().map_err(|e| e.to_string())?.display().to_string();
        let vault = app.state::<VaultState>().get()?;
        tauri::async_runtime::spawn_blocking(move || vault.import_mindmap(folder, source))
            .await
            .map_err(|e| e.to_string())?
            .map(Some)
            .map_err(|e| e.to_string())
    }
    #[cfg(mobile)]
    {
        let _ = (app, folder);
        Err("Mind map files are opened on a computer.".into())
    }
}

/// The vault's canvases, by path.
#[tauri::command]
pub(crate) fn vault_canvases(state: State<'_, VaultState>) -> CommandResult<Vec<String>> {
    run(&state, |v| v.canvases())
}

#[tauri::command]
pub(crate) fn vault_read_canvas(state: State<'_, VaultState>, path: String) -> CommandResult<CanvasBoard> {
    run(&state, |v| v.read_canvas(path))
}

#[tauri::command]
pub(crate) fn vault_write_canvas(
    state: State<'_, VaultState>,
    path: String,
    board: CanvasBoard,
) -> CommandResult<()> {
    run(&state, |v| v.write_canvas(path, board))
}

/// Makes an empty canvas in a folder; its path.
#[tauri::command]
pub(crate) fn vault_create_canvas(
    state: State<'_, VaultState>,
    folder: String,
    name: String,
) -> CommandResult<String> {
    run(&state, |v| v.create_canvas(folder, name))
}

/// The vault's drawings (Excalidraw scenes), by path.
#[tauri::command]
pub(crate) fn vault_drawings(state: State<'_, VaultState>) -> CommandResult<Vec<String>> {
    run(&state, |v| v.drawings())
}

#[tauri::command]
pub(crate) fn vault_read_drawing(state: State<'_, VaultState>, path: String) -> CommandResult<DrawingPage> {
    run(&state, |v| v.read_drawing(path))
}

#[tauri::command]
pub(crate) fn vault_write_drawing(
    state: State<'_, VaultState>,
    path: String,
    page: DrawingPage,
) -> CommandResult<()> {
    run(&state, |v| v.write_drawing(path, page))
}

/// What rubbing along a path leaves of a drawing's strokes.
#[tauri::command]
pub(crate) fn drawing_erase(
    items: Vec<paperloom_core::DrawingItem>,
    path: Vec<paperloom_core::DrawingPoint>,
    radius: f64,
) -> Vec<paperloom_core::DrawingItem> {
    paperloom_core::drawing_erase(items, path, radius)
}

/// Makes an empty drawing on `paper` in a folder; its path.
#[tauri::command]
pub(crate) fn vault_create_drawing(
    state: State<'_, VaultState>,
    folder: String,
    name: String,
    paper: String,
) -> CommandResult<String> {
    run(&state, |v| v.create_drawing(folder, name, paper))
}

/// Keeps a file the app made (a recording, in base64) in a folder of the
/// vault; its path.
#[tauri::command]
pub(crate) fn vault_add_file(
    state: State<'_, VaultState>,
    folder: String,
    name: String,
    data: String,
) -> CommandResult<String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| e.to_string())?;
    run(&state, |v| v.add_file(folder, name, bytes))
}

/// A drawing as an SVG picture.
#[tauri::command]
pub(crate) fn vault_drawing_svg(state: State<'_, VaultState>, path: String) -> CommandResult<String> {
    run(&state, |v| v.drawing_svg(path))
}

#[tauri::command]
pub(crate) fn vault_outline(state: State<'_, VaultState>, path: String) -> CommandResult<NoteOutline> {
    run(&state, |v| v.outline(path))
}

#[tauri::command]
pub(crate) fn vault_switcher(
    state: State<'_, VaultState>,
    query: String,
    limit: u32,
) -> CommandResult<Vec<NoteInfo>> {
    run(&state, |v| v.switcher(query, limit))
}

#[tauri::command]
pub(crate) fn vault_link_completions(
    state: State<'_, VaultState>,
    from: String,
    query: String,
    limit: u32,
) -> CommandResult<Vec<LinkCompletion>> {
    run(&state, |v| v.link_completions(from, query, limit))
}

#[tauri::command]
pub(crate) fn vault_tags(state: State<'_, VaultState>) -> CommandResult<Vec<VaultTag>> {
    run(&state, |v| v.tags())
}

#[tauri::command]
pub(crate) fn vault_search(
    state: State<'_, VaultState>,
    query: String,
    limit: u32,
) -> CommandResult<Vec<NoteMention>> {
    run(&state, |v| v.search(query, limit))
}

#[tauri::command]
pub(crate) fn vault_render(
    state: State<'_, VaultState>,
    path: String,
    text: String,
) -> CommandResult<String> {
    run(&state, |v| v.render(path, text))
}

#[tauri::command]
pub(crate) fn vault_resolve(
    state: State<'_, VaultState>,
    target: String,
    from: String,
) -> CommandResult<Option<String>> {
    run(&state, |v| v.resolve(target, from))
}

/// A file in the vault (an embedded image), as bytes.
#[tauri::command]
pub(crate) fn vault_file(state: State<'_, VaultState>, path: String) -> CommandResult<Response> {
    let full = run(&state, |v| v.file_path(path))?;
    std::fs::read(&full).map(Response::new).map_err(|e| format!("{full}: {e}"))
}

/// Lines of notes that link to an item or embed its highlights.
#[tauri::command]
pub(crate) fn vault_item_mentions(
    state: State<'_, VaultState>,
    item_id: String,
) -> CommandResult<Vec<NoteMention>> {
    run(&state, |v| v.item_mentions(item_id))
}

/// Notes and items a note is about too but does not link to yet.
#[tauri::command]
pub(crate) fn vault_link_suggestions(
    state: State<'_, VaultState>,
    path: String,
    limit: u32,
) -> CommandResult<Vec<LinkSuggestion>> {
    run(&state, |v| v.link_suggestions(path, limit))
}

/// Makes (once) a literature note for an item and returns it.
#[tauri::command]
pub(crate) fn vault_create_literature_note(
    state: State<'_, VaultState>,
    item_id: String,
) -> CommandResult<NoteInfo> {
    run(&state, |v| v.create_literature_note(item_id))
}

/// Writes a note's properties (front matter), keeping the rest.
#[tauri::command]
pub(crate) fn vault_set_properties(
    state: State<'_, VaultState>,
    path: String,
    properties: Vec<NoteProperty>,
) -> CommandResult<NoteInfo> {
    run(&state, |v| v.set_properties(path, properties))
}

#[tauri::command]
pub(crate) fn vault_settings(state: State<'_, VaultState>) -> CommandResult<VaultSettings> {
    run(&state, |v| v.settings())
}

#[tauri::command]
pub(crate) fn vault_set_settings(state: State<'_, VaultState>, settings: VaultSettings) -> CommandResult<()> {
    run(&state, |v| v.set_settings(settings))
}

#[tauri::command]
pub(crate) fn vault_templates(state: State<'_, VaultState>) -> CommandResult<Vec<NoteInfo>> {
    run(&state, |v| v.templates())
}

/// A template filled in, for putting into the note being written.
#[tauri::command]
pub(crate) fn vault_template_text(
    state: State<'_, VaultState>,
    template: String,
    title: String,
    now: LocalTime,
) -> CommandResult<String> {
    run(&state, |v| v.template_text(template, title, now))
}

#[tauri::command]
pub(crate) fn vault_create_from_template(
    state: State<'_, VaultState>,
    folder: String,
    title: String,
    template: Option<String>,
    now: LocalTime,
) -> CommandResult<NoteInfo> {
    run(&state, |v| v.create_note_from_template(folder, title, template, now))
}

/// Today's daily note, made the first time.
#[tauri::command]
pub(crate) fn vault_daily_note(state: State<'_, VaultState>, now: LocalTime) -> CommandResult<NoteInfo> {
    run(&state, |v| v.daily_note(now))
}

#[tauri::command]
pub(crate) fn vault_versions(state: State<'_, VaultState>, path: String) -> CommandResult<Vec<NoteVersion>> {
    run(&state, |v| v.note_versions(path))
}

#[tauri::command]
pub(crate) fn vault_version(state: State<'_, VaultState>, path: String, time: i64) -> CommandResult<String> {
    run(&state, |v| v.note_version(path, time))
}

#[tauri::command]
pub(crate) fn vault_compare_version(
    state: State<'_, VaultState>,
    path: String,
    time: i64,
) -> CommandResult<Vec<DiffLine>> {
    run(&state, |v| v.compare_version(path, time))
}

#[tauri::command]
pub(crate) fn vault_restore_version(
    state: State<'_, VaultState>,
    path: String,
    time: i64,
) -> CommandResult<NoteInfo> {
    run(&state, |v| v.restore_version(path, time))
}

/// The graph of notes; the whole vault's, or around a note. Laying out a
/// big vault takes a moment, so off the main thread.
#[tauri::command]
pub(crate) async fn vault_graph(
    state: State<'_, VaultState>,
    center: Option<String>,
    depth: u32,
) -> CommandResult<NoteGraph> {
    let vault = state.get()?;
    tauri::async_runtime::spawn_blocking(move || vault.graph(center, depth))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Asks where to save a note as a Word document, with its citations and
/// reference list, and writes it; none if cancelled.
#[tauri::command]
pub(crate) async fn vault_export_docx<R: Runtime>(
    app: AppHandle<R>,
    path: String,
) -> CommandResult<Option<String>> {
    #[cfg(desktop)]
    {
        let title = path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".md").to_string();
        let dialog = app.dialog().file().add_filter("Word", &["docx"]).set_file_name(format!("{title}.docx"));
        let Some(target) = crate::answer(move || dialog.blocking_save_file()).await? else { return Ok(None) };
        let target = target.into_path().map_err(|e| e.to_string())?.display().to_string();
        let vault = app.state::<VaultState>().get()?;
        let written = target.clone();
        tauri::async_runtime::spawn_blocking(move || vault.export_docx(path, written))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        Ok(Some(target))
    }
    #[cfg(mobile)]
    {
        let _ = (app, path);
        Err("Word documents are made on a computer.".into())
    }
}

/// Asks where to save a note as a LaTeX document, with its citations and
/// their BibTeX inside, and writes it; none if cancelled.
#[tauri::command]
pub(crate) async fn vault_export_latex<R: Runtime>(
    app: AppHandle<R>,
    path: String,
) -> CommandResult<Option<String>> {
    #[cfg(desktop)]
    {
        let title = path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".md").to_string();
        let dialog = app.dialog().file().add_filter("LaTeX", &["tex"]).set_file_name(format!("{title}.tex"));
        let Some(target) = crate::answer(move || dialog.blocking_save_file()).await? else { return Ok(None) };
        let target = target.into_path().map_err(|e| e.to_string())?.display().to_string();
        let vault = app.state::<VaultState>().get()?;
        let written = target.clone();
        tauri::async_runtime::spawn_blocking(move || vault.export_latex(path, written))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        Ok(Some(target))
    }
    #[cfg(mobile)]
    {
        let _ = (app, path);
        Err("LaTeX documents are made on a computer.".into())
    }
}
