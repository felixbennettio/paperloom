//! Obsidian plugins in the Tauri app. The runtime (`core/plugin-runtime`,
//! shared with the Apple apps) is served from the `plugin:` scheme to a
//! sandboxed frame for each plugin, apart from the app; these commands give
//! the web view the vault's plugins and make the web requests a plugin may.

use paperloom_core::{
    CommunityPlugin, ObsidianPlugin, ObsidianPluginCode, PluginHeader, PluginResponse, ZoteroPlugin,
    ZoteroPluginCode,
};
use tauri::http::{Request, Response, StatusCode, header};
use tauri::{AppHandle, Runtime, State, UriSchemeContext};

use crate::vault::VaultState;
use crate::{AppState, CommandResult};

const HOST: &str = include_str!("../../../core/plugin-runtime/host.html");
const CM6: &str = include_str!("../../../core/plugin-runtime/cm6.js");
const RUNTIME: &str = include_str!("../../../core/plugin-runtime/obsidian.js");
const STYLES: &str = include_str!("../../../core/plugin-runtime/obsidian.css");
const ZOTERO_HOST: &str = include_str!("../../../core/plugin-runtime/zotero.html");
const ZOTERO_RUNTIME: &str = include_str!("../../../core/plugin-runtime/zotero.js");

const TRANSLATE_HOST: &str = include_str!("../../../core/plugin-runtime/translate.html");
const TRANSLATE_RUNTIME: &str = include_str!("../../../core/plugin-runtime/translate.js");

/// Serves the frame page, the runtime and its styles to the `plugin:` scheme.
pub(crate) fn serve<R: Runtime>(
    _context: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let (status, kind, body) = match request.uri().path() {
        "/" | "/host.html" => (StatusCode::OK, "text/html; charset=utf-8", HOST),
        "/cm6.js" => (StatusCode::OK, "text/javascript; charset=utf-8", CM6),
        "/obsidian.js" => (StatusCode::OK, "text/javascript; charset=utf-8", RUNTIME),
        "/obsidian.css" => (StatusCode::OK, "text/css; charset=utf-8", STYLES),
        "/translate.html" => (StatusCode::OK, "text/html; charset=utf-8", TRANSLATE_HOST),
        "/translate.js" => (StatusCode::OK, "text/javascript; charset=utf-8", TRANSLATE_RUNTIME),
        "/zotero.html" => (StatusCode::OK, "text/html; charset=utf-8", ZOTERO_HOST),
        "/zotero.js" => (StatusCode::OK, "text/javascript; charset=utf-8", ZOTERO_RUNTIME),
        _ => (StatusCode::NOT_FOUND, "text/plain", ""),
    };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, kind)
        .body(body.as_bytes().to_vec())
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

#[tauri::command]
pub(crate) fn obsidian_plugins(state: State<'_, VaultState>) -> CommandResult<Vec<ObsidianPlugin>> {
    state.get()?.obsidian_plugins().map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn set_obsidian_plugin_enabled(
    state: State<'_, VaultState>,
    id: String,
    enabled: bool,
) -> CommandResult<()> {
    state.get()?.set_obsidian_plugin_enabled(id, enabled).map_err(|e| e.to_string())
}

/// Every note's metadata, as Obsidian gives it to plugins (JSON).
#[tauri::command]
pub(crate) fn obsidian_metadata(state: State<'_, VaultState>) -> CommandResult<String> {
    state.get()?.obsidian_metadata().map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn obsidian_plugin_code(
    state: State<'_, VaultState>,
    id: String,
) -> CommandResult<ObsidianPluginCode> {
    state.get()?.obsidian_plugin_code(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn save_obsidian_plugin_data(
    state: State<'_, VaultState>,
    id: String,
    data: String,
) -> CommandResult<()> {
    state.get()?.save_obsidian_plugin_data(id, data).map_err(|e| e.to_string())
}

/// The community plugins Obsidian lists.
#[tauri::command]
pub(crate) async fn community_plugins() -> CommandResult<Vec<CommunityPlugin>> {
    tauri::async_runtime::spawn_blocking(paperloom_core::community_plugins)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Downloads a community plugin's latest release into the vault.
#[tauri::command]
pub(crate) async fn install_obsidian_plugin(
    state: State<'_, VaultState>,
    repo: String,
) -> CommandResult<ObsidianPlugin> {
    let vault = state.get()?;
    tauri::async_runtime::spawn_blocking(move || vault.install_obsidian_plugin(repo))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// A plugin's web request, to a site the person allowed in the web view.
#[tauri::command]
pub(crate) async fn plugin_fetch(
    url: String,
    method: String,
    headers: Vec<PluginHeader>,
    body: Option<String>,
) -> CommandResult<PluginResponse> {
    tauri::async_runtime::spawn_blocking(move || paperloom_core::plugin_fetch(url, method, headers, body))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn zotero_plugins(state: State<'_, AppState>) -> CommandResult<Vec<ZoteroPlugin>> {
    state.library.zotero_plugins().map_err(|e| e.to_string())
}

/// Asks for a Zotero plugin file (`.xpi`) and installs it; none if cancelled.
#[tauri::command]
pub(crate) async fn install_zotero_plugin<R: Runtime>(
    app: AppHandle<R>,
) -> CommandResult<Option<ZoteroPlugin>> {
    #[cfg(desktop)]
    {
        use tauri::Manager;
        use tauri_plugin_dialog::DialogExt;

        let dialog = app.dialog().file().add_filter("Zotero plugin", &["xpi"]);
        let Some(file) = crate::answer(move || dialog.blocking_pick_file()).await? else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|e| e.to_string())?.display().to_string();
        let library = app.state::<AppState>().library.clone();
        tauri::async_runtime::spawn_blocking(move || library.install_zotero_plugin(path))
            .await
            .map_err(|e| e.to_string())?
            .map(Some)
            .map_err(|e| e.to_string())
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err("Zotero plugins are installed on a computer.".into())
    }
}

#[tauri::command]
pub(crate) fn set_zotero_plugin_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> CommandResult<()> {
    state.library.set_zotero_plugin_enabled(id, enabled).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn zotero_plugin_code(state: State<'_, AppState>, id: String) -> CommandResult<ZoteroPluginCode> {
    state.library.zotero_plugin_code(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn save_zotero_plugin_prefs(
    state: State<'_, AppState>,
    id: String,
    prefs: String,
) -> CommandResult<()> {
    state.library.save_zotero_plugin_prefs(id, prefs).map_err(|e| e.to_string())
}

/// The library's items as Zotero gives them to plugins (JSON), with the
/// open vault's notes that have a lasting identity.
#[tauri::command]
pub(crate) fn zotero_items(
    state: State<'_, AppState>,
    vaults: State<'_, VaultState>,
) -> CommandResult<String> {
    let parse =
        |json: String| serde_json::from_str::<Vec<serde_json::Value>>(&json).map_err(|e| e.to_string());
    let mut items = parse(state.library.zotero_items().map_err(|e| e.to_string())?)?;
    let vault = vaults.get()?;
    items.extend(parse(vault.zotero_notes().map_err(|e| e.to_string())?)?);
    serde_json::to_string(&items).map_err(|e| e.to_string())
}

/// A note a Zotero plugin makes (HTML), in the open vault; its key.
#[tauri::command]
pub(crate) fn zotero_create_note(
    vaults: State<'_, VaultState>,
    parent: Option<String>,
    html: String,
) -> CommandResult<String> {
    vaults.get()?.zotero_create_note(parent, html).map_err(|e| e.to_string())
}

/// A Zotero plugin's change to a note it made (HTML).
#[tauri::command]
pub(crate) fn zotero_update_note(
    vaults: State<'_, VaultState>,
    key: String,
    html: String,
) -> CommandResult<()> {
    vaults.get()?.zotero_update_note(key, html).map_err(|e| e.to_string())
}

/// A Zotero plugin's change to an item.
#[tauri::command]
pub(crate) fn zotero_update_item(
    state: State<'_, AppState>,
    key: String,
    fields: String,
    tags: Option<Vec<String>>,
) -> CommandResult<()> {
    state.library.zotero_update_item(key, fields, tags).map_err(|e| e.to_string())
}
