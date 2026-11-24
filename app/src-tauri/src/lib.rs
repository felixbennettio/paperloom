//! Paperloom for Windows, Linux and Android: Tauri commands over the shared
//! Rust core, which the Apple apps call through UniFFI.

mod account;
mod ai;
mod backup;
mod browser;
mod diagram;
mod endnote;

mod ocr;
mod organize;
mod pdf;
mod plugins;
mod private;
mod secrets;
mod share;
mod styles;
mod syncing;
mod translators;
mod vault;
mod wordcite;
mod writers;
mod zotero;

use std::sync::Arc;

use paperloom_core::{
    CitationOutput, CitationStyle, CollectionInfo, ImportReport, ItemDetail, ItemFilter, ItemSummary,
    NewItem, PaperLibrary, ReferenceFormat, SearchPage, SearchSource, SruCatalogue,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

pub(crate) struct AppState {
    library: Arc<PaperLibrary>,
    location: String,
}

pub(crate) type CommandResult<T> = Result<T, String>;

/// Waits for a file dialog's answer on a thread of its own: a blocking
/// dialog holds its thread until the person answers, and async commands
/// share one thread per core (two dialogs open on a two-core machine left
/// none for the rest, Azure end to end 27).
pub(crate) async fn answer<T: Send + 'static>(
    dialog: impl FnOnce() -> T + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(dialog).await.map_err(|e| e.to_string())
}

/// Tells the window when the browser extension added works.
#[cfg(desktop)]
struct ConnectorBridge<R: Runtime>(AppHandle<R>);

#[cfg(desktop)]
impl<R: Runtime> paperloom_core::ConnectorEvents for ConnectorBridge<R> {
    fn added(&self, count: u32) {
        use tauri::Emitter;
        let _ = self.0.emit("connector-added", count);
    }

    /// Asks in the window (translated there) and waits for the answer on
    /// the connector's thread; unanswered within two minutes is a no.
    fn allow_writer(&self, client: String, origin: String) -> bool {
        use std::sync::atomic::Ordering;

        use tauri::Emitter;

        let Some(questions) = self.0.try_state::<WriterQuestions>() else { return false };
        let id = questions.next.fetch_add(1, Ordering::SeqCst);
        let (answer, answered) = std::sync::mpsc::channel();
        match questions.waiting.lock() {
            Ok(mut waiting) => waiting.insert(id, answer),
            Err(_) => return false,
        };
        if let Some(window) = self.0.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
        let asked = self.0.emit("connector-writer", WriterQuestion { id, client, origin }).is_ok();
        let allowed = asked && answered.recv_timeout(std::time::Duration::from_secs(120)).unwrap_or(false);
        if let Ok(mut waiting) = questions.waiting.lock() {
            waiting.remove(&id);
        }
        allowed
    }
}

/// Writing plugins' pairing questions waiting for the person, by id.
#[derive(Default)]
struct WriterQuestions {
    next: std::sync::atomic::AtomicU64,
    waiting: std::sync::Mutex<std::collections::HashMap<u64, std::sync::mpsc::Sender<bool>>>,
}

/// May `client` (a Word or WPS add-in, on a page from `origin`) use the library?
#[cfg(desktop)]
#[derive(Clone, Serialize)]
struct WriterQuestion {
    id: u64,
    client: String,
    origin: String,
}

/// The person's answer to a pairing question.
#[tauri::command]
fn connector_writer_answer(questions: State<'_, WriterQuestions>, id: u64, allow: bool) {
    if let Some(answer) = questions.waiting.lock().ok().and_then(|mut waiting| waiting.remove(&id)) {
        let _ = answer.send(allow);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryInfo {
    core_version: String,
    location: String,
}

#[tauri::command]
fn library_info(state: State<'_, AppState>) -> LibraryInfo {
    LibraryInfo { core_version: paperloom_core::core_version(), location: state.location.clone() }
}

/// An error message from a command in the interface's language (the
/// commands answer in English).
#[tauri::command]
fn localize_message(message: String, language: String) -> String {
    paperloom_core::localize(&message, &language)
}

#[tauri::command]
fn list_items(state: State<'_, AppState>, filter: ItemFilter) -> CommandResult<Vec<ItemSummary>> {
    state.library.list_items(filter).map_err(|e| e.to_string())
}

#[tauri::command]
fn add_item(
    state: State<'_, AppState>,
    item: NewItem,
    collection_id: Option<String>,
) -> CommandResult<ItemSummary> {
    let added = state.library.add_item(item).map_err(|e| e.to_string())?;
    if let Some(collection) = collection_id {
        state.library.add_to_collection(collection, added.id.clone()).map_err(|e| e.to_string())?;
    }
    Ok(added)
}

/// Looks the identifier up on the network, off the main thread.
#[tauri::command]
async fn add_by_identifier(
    state: State<'_, AppState>,
    input: String,
    collection_id: Option<String>,
) -> CommandResult<ItemSummary> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.add_by_identifier(input, collection_id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn item_detail(state: State<'_, AppState>, item_id: String) -> CommandResult<Option<ItemDetail>> {
    state.library.item_detail(item_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn recognize_identifier(input: String) -> Option<String> {
    paperloom_core::recognize_identifier(input)
}

#[tauri::command]
fn citation_styles() -> Vec<CitationStyle> {
    paperloom_core::citation_styles()
}

#[tauri::command]
fn format_citations(
    state: State<'_, AppState>,
    item_ids: Vec<String>,
    style_id: String,
    html: bool,
) -> CommandResult<CitationOutput> {
    state.library.format_citations(item_ids, style_id, html).map_err(|e| e.to_string())
}

#[tauri::command]
fn reference_formats() -> Vec<ReferenceFormat> {
    paperloom_core::reference_formats()
}

#[tauri::command]
fn import_references(
    state: State<'_, AppState>,
    text: String,
    collection_id: Option<String>,
) -> CommandResult<ImportReport> {
    state.library.import_references(text, collection_id).map_err(|e| e.to_string())
}

/// Asks where to save, then writes the items there. `None` when cancelled.
#[tauri::command]
async fn export_references_file<R: Runtime>(
    app: AppHandle<R>,
    item_ids: Vec<String>,
    format: String,
) -> CommandResult<Option<String>> {
    let text = app
        .state::<AppState>()
        .library
        .export_references(item_ids, format.clone())
        .map_err(|e| e.to_string())?;
    let extension = paperloom_core::reference_formats()
        .into_iter()
        .find(|f| f.id == format)
        .map_or_else(|| "txt".to_string(), |f| f.file_extension);
    let dialog = app
        .dialog()
        .file()
        .set_file_name(format!("Paperloom.{extension}"))
        .add_filter(&extension, &[extension.as_str()]);
    let Some(path) = answer(move || dialog.blocking_save_file()).await? else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
fn search_sources() -> Vec<SearchSource> {
    paperloom_core::search_sources()
}

/// Items for `[@` completion in the note editor.
#[tauri::command]
fn citation_completions(
    state: State<'_, AppState>,
    query: String,
    limit: u32,
) -> CommandResult<Vec<paperloom_core::CitationCompletion>> {
    state.library.citation_completions(query, limit).map_err(|e| e.to_string())
}

/// The sites the in-app browser reads beyond their embedded metadata.
#[tauri::command]
fn capture_sites() -> Vec<paperloom_core::CaptureSite> {
    paperloom_core::capture_sites()
}

/// Searches an online database, with the person's key for one that needs
/// it; network, so off the main thread.
#[tauri::command]
async fn search_online(
    state: State<'_, AppState>,
    secrets: State<'_, secrets::Secrets>,
    source_id: String,
    text: String,
    page: u32,
) -> CommandResult<SearchPage> {
    let library = state.library.clone();
    let key = secrets.load().database_keys.remove(&source_id);
    tauri::async_runtime::spawn_blocking(move || library.search_online(source_id, text, page, key))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// National libraries' catalogues to start from when adding one.
#[tauri::command]
fn catalogue_presets() -> Vec<SruCatalogue> {
    paperloom_core::catalogue_presets()
}

/// Searches a library catalogue the person added; network, so off the
/// main thread.
#[tauri::command]
async fn search_catalogue(
    state: State<'_, AppState>,
    catalogue: SruCatalogue,
    text: String,
    page: u32,
) -> CommandResult<SearchPage> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.search_catalogue(catalogue, text, page))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Keeps (or, empty, forgets) the person's key for a database.
#[tauri::command]
fn database_key_set(
    secrets: State<'_, secrets::Secrets>,
    source_id: String,
    key: String,
) -> CommandResult<()> {
    let key = key.trim().to_string();
    secrets.update(|kept| {
        if key.is_empty() {
            kept.database_keys.remove(&source_id);
        } else {
            kept.database_keys.insert(source_id, key);
        }
    })
}

/// The databases a key is kept for.
#[tauri::command]
fn database_keys_known(secrets: State<'_, secrets::Secrets>) -> Vec<String> {
    secrets.load().database_keys.into_keys().collect()
}

#[tauri::command]
fn add_search_results(
    state: State<'_, AppState>,
    keys: Vec<String>,
    collection_id: Option<String>,
) -> CommandResult<ImportReport> {
    state.library.add_search_results(keys, collection_id).map_err(|e| e.to_string())
}

/// Opens a web page or PDF link from a search result in the browser.
#[tauri::command]
fn open_url<R: Runtime>(app: AppHandle<R>, url: String) -> CommandResult<()> {
    if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:")) {
        return Err(format!("Not a web address: {url}"));
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn trash_item(state: State<'_, AppState>, item_id: String) -> CommandResult<()> {
    state.library.trash_item(item_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_collections(state: State<'_, AppState>) -> CommandResult<Vec<CollectionInfo>> {
    state.library.collections().map_err(|e| e.to_string())
}

#[tauri::command]
fn create_collection(
    state: State<'_, AppState>,
    name: String,
    parent_id: Option<String>,
) -> CommandResult<CollectionInfo> {
    state.library.create_collection(name, parent_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn normalize_doi(input: String) -> Option<String> {
    paperloom_core::normalize_doi(input)
}

#[tauri::command]
fn item_types() -> Vec<String> {
    paperloom_core::item_types()
}

/// On a screen smaller than the window's first size (a 1366×768 laptop) the
/// bottom of the window would sit under the taskbar: take the whole screen
/// instead.
#[cfg(desktop)]
fn fit_to_screen<R: Runtime>(window: &tauri::WebviewWindow<R>) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()? else { return Ok(()) };
    let area = monitor.work_area().size;
    let size = window.outer_size()?;
    if size.width > area.width || size.height > area.height {
        window.maximize()?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // The installer tests' check of text recognition: reads a picture, quits.
    if let Some(code) = ocr::command_line() {
        std::process::exit(code);
    }
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();
    // First: a second launch (a sign-in callback on Windows and Linux, or
    // files opened with Paperloom) hands its arguments to this instance and
    // quits.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
            // Files opened with Paperloom while it runs.
            share::open_files(app, &argv, std::path::Path::new(&cwd));
        }));
    }
    builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_paperloom_share::init())
        .register_uri_scheme_protocol("plugin", plugins::serve)
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let path = dir.join("library.sqlite");
            // A backup put back last time takes the library's place first.
            if let Err(e) = paperloom_core::finish_restore(path.to_string_lossy().into_owned()) {
                eprintln!("Putting a backup back did not finish: {e}");
            }
            let library = PaperLibrary::new(path.to_string_lossy().into_owned())?;
            app.manage(AppState { library: library.clone(), location: path.display().to_string() });
            // The browser extension saves through this (desktop only); a
            // second copy of the app leaves it to the first.
            app.manage(WriterQuestions::default());
            #[cfg(desktop)]
            {
                let bridge = Arc::new(ConnectorBridge(app.handle().clone()));
                library.clone().start_connector(paperloom_core::connector_port(), bridge);
            }
            app.manage(account::Account::new(&dir));
            app.manage(secrets::Secrets::new(&dir));
            app.manage(pdf::Picked::default());
            app.manage(share::Opened::default());
            // Files Paperloom was opened with (Open With, or a command line).
            #[cfg(desktop)]
            {
                let args: Vec<String> =
                    std::env::args_os().map(|arg| arg.to_string_lossy().into_owned()).collect();
                share::open_files(app.handle(), &args, &std::env::current_dir().unwrap_or_default());
            }
            app.manage(vault::VaultState::open(&dir, library)?);

            // Registers the URL scheme for builds that were not installed
            // (AppImage, portable); installers register it themselves.
            #[cfg(desktop)]
            let _ = app.deep_link().register_all();
            #[cfg(desktop)]
            if let Some(window) = app.get_webview_window("main") {
                let _ = fit_to_screen(&window);
            }
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    account::handle_callback(&handle, url.as_str());
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connector_writer_answer,
            writers::connected_writers,
            writers::forget_connected_writers,
            writers::wps_addin,
            writers::install_wps_addin,
            writers::remove_wps_addin,
            writers::word_addin,
            writers::install_word_addin,
            writers::remove_word_addin,
            library_info,
            localize_message,
            list_items,
            add_item,
            add_by_identifier,
            item_detail,
            recognize_identifier,
            citation_styles,
            styles::search_citation_styles,
            styles::download_citation_style,
            styles::add_citation_style_file,
            styles::remove_citation_style,
            wordcite::cite_word_document,
            format_citations,
            reference_formats,
            import_references,
            export_references_file,
            search_sources,
            search_online,
            catalogue_presets,
            search_catalogue,
            capture_sites,
            citation_completions,
            zotero::import_zotero,
            endnote::import_endnote,
            backup::back_up_library,
            backup::back_up_if_due,
            backup::restore_library,
            backup::restart_app,
            browser::browser_open,
            browser::browser_read,
            browser::browser_snapshot,
            browser::browser_go,
            database_key_set,
            syncing::s3_secret_set,
            syncing::sync_passphrase_set,
            syncing::sync_passphrase_known,
            syncing::s3_secret_known,
            database_keys_known,
            add_search_results,
            share::take_shared,
            share::read_web_page,
            open_url,
            organize::update_item,
            organize::set_starred,
            organize::set_read_status,
            organize::tags,
            organize::set_item_tags,
            organize::set_tag_color,
            organize::rename_tag,
            organize::delete_tag,
            organize::restore_item,
            organize::delete_item,
            organize::empty_trash,
            organize::duplicate_groups,
            organize::merge_items,
            organize::search_fields,
            organize::saved_searches,
            organize::save_search,
            organize::delete_saved_search,
            organize::run_search,
            organize::related_items,
            organize::relate_items,
            organize::unrelate_items,
            organize::feeds,
            organize::add_feed,
            organize::refresh_feeds,
            organize::rename_feed,
            organize::remove_feed,
            organize::feed_items,
            organize::set_feed_item_read,
            organize::mark_feed_read,
            organize::add_feed_item,
            organize::count_citations,
            vault::vault_root,
            vault::vault_choose_folder,
            vault::vault_refresh,
            vault::vault_notes,
            vault::vault_canvases,
            vault::mindmap_layout,
            vault::mindmap_themes,
            vault::mindmap_markers,
            vault::mindmap_edit,
            vault::new_mindmap_text,
            vault::mindmap_svg,
            vault::mindmap_export,
            vault::vault_import_mindmap,
            vault::vault_read_canvas,
            vault::vault_write_canvas,
            vault::vault_create_canvas,
            vault::vault_drawings,
            vault::vault_read_drawing,
            vault::vault_write_drawing,
            vault::drawing_erase,
            vault::vault_create_drawing,
            vault::vault_add_file,
            vault::vault_drawing_svg,
            vault::drawing_export,
            diagram::diagram_view,
            diagram::diagram_change,
            diagram::diagram_shapes,
            diagram::diagram_page_svg,
            diagram::diagram_export,
            diagram::vault_diagrams,
            diagram::vault_read_diagram,
            diagram::vault_write_diagram,
            diagram::vault_create_diagram,
            plugins::obsidian_plugins,
            plugins::set_obsidian_plugin_enabled,
            plugins::obsidian_plugin_code,
            plugins::obsidian_metadata,
            plugins::save_obsidian_plugin_data,
            plugins::plugin_fetch,
            plugins::community_plugins,
            plugins::install_obsidian_plugin,
            translators::download_translators,
            translators::translators_status,
            translators::remove_translators,
            translators::web_translators,
            translators::translator_catalog,
            translators::translator_code,
            translators::read_translated_items,
            translators::translator_request,
            plugins::zotero_plugins,
            plugins::install_zotero_plugin,
            plugins::set_zotero_plugin_enabled,
            plugins::zotero_plugin_code,
            plugins::save_zotero_plugin_prefs,
            plugins::zotero_items,
            plugins::zotero_update_item,
            plugins::zotero_create_note,
            plugins::zotero_update_note,
            vault::vault_folders,
            vault::vault_read,
            vault::vault_write,
            vault::vault_create_note,
            vault::vault_create_folder,
            vault::vault_rename,
            vault::vault_delete,
            vault::vault_outline,
            vault::vault_switcher,
            vault::vault_link_completions,
            vault::vault_tags,
            vault::vault_search,
            vault::vault_render,
            vault::vault_resolve,
            vault::vault_file,
            pdf::annotation_embed,
            pdf::item_link,
            pdf::citation_marker,
            ocr::text_recognition_available,
            ocr::recognize_page_text,
            ocr::recognize_page_lines,
            pdf::add_text_layer,
            ocr::text_recognition_note,
            pdf::annotation_location,
            pdf::resolve_item_reference,
            pdf::attachment_info,
            vault::vault_create_literature_note,
            vault::vault_item_mentions,
            vault::vault_link_suggestions,
            vault::vault_set_properties,
            vault::vault_settings,
            vault::vault_set_settings,
            vault::vault_templates,
            vault::vault_template_text,
            vault::vault_create_from_template,
            vault::vault_daily_note,
            vault::vault_versions,
            vault::vault_version,
            vault::vault_compare_version,
            vault::vault_restore_version,
            vault::vault_graph,
            vault::vault_export_docx,
            vault::vault_export_latex,
            pdf::set_attachment_text,
            pdf::attachment_has_text,
            pdf::search_everything,
            pdf::pick_pdf_files,
            pdf::read_picked_file,
            pdf::import_pdf,
            pdf::attachments,
            pdf::attach_files,
            pdf::find_available_pdf,
            pdf::read_attachment,
            pdf::edit_pdf_pages,
            pdf::pdf_form_fields,
            pdf::fill_pdf_form,
            pdf::export_annotated_pdf,
            pdf::open_attachment,
            pdf::remove_attachment,
            pdf::annotations,
            pdf::add_annotation,
            pdf::update_annotation,
            pdf::remove_annotation,
            trash_item,
            list_collections,
            create_collection,
            normalize_doi,
            item_types,
            account::account_status,
            account::account_available,
            account::account_sign_in,
            account::account_sign_out,
            syncing::sync_status,
            syncing::sync_now,
            syncing::sync_pick_folder,
            syncing::webdav_password_set,
            syncing::webdav_password_known,
            syncing::seafile_password_set,
            syncing::seafile_password_known,
            ai::ai_key_set,
            ai::ai_key_known,
            ai::ai_models,
            ai::ai_semantic_status,
            ai::ai_semantic_index,
            ai::ai_summarize,
            ai::ai_ask,
            ai::ai_transform,
            ai::ai_mindmap,
            ai::ai_ask_library,
            ai::ai_mindmap_item,
        ])
        .run(context())
        .expect("error while running Paperloom");
}

/// The app's configuration, with WebView2's DevTools port opened when
/// PAPERLOOM_WEBVIEW2_DEBUG_PORT names one: the Windows end-to-end test
/// (scripts/ci/windows-e2e.ps1) attaches WebDriver there. WebView2's own
/// ways to add the switch (WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS, its
/// policy) never reached the browser process on the test machine.
fn context() -> tauri::Context<tauri::Wry> {
    #[allow(unused_mut)]
    let mut context = tauri::generate_context!();
    #[cfg(windows)]
    if let Some(port) =
        std::env::var("PAPERLOOM_WEBVIEW2_DEBUG_PORT").ok().filter(|port| port.parse::<u16>().is_ok())
    {
        for window in &mut context.config_mut().app.windows {
            // These replace wry's own switches, so those come first.
            window.additional_browser_args = Some(format!(
                "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
                 --autoplay-policy=no-user-gesture-required --remote-debugging-port={port}"
            ));
        }
    }
    context
}
