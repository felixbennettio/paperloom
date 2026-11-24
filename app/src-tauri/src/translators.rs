//! The optional translator installation and the translation frame's requests.

use paperloom_core::{PluginHeader, SearchPage, TranslatorResponse, TranslatorStatus, WebTranslator};
use tauri::State;

use crate::{AppState, CommandResult};

#[tauri::command]
pub(crate) async fn download_translators(state: State<'_, AppState>) -> CommandResult<u32> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.download_translators())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn translators_status(state: State<'_, AppState>) -> CommandResult<TranslatorStatus> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.translators_status())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn remove_translators(state: State<'_, AppState>) -> CommandResult<()> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.remove_translators())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn web_translators(state: State<'_, AppState>) -> CommandResult<Vec<WebTranslator>> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.web_translators())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn translator_catalog(state: State<'_, AppState>) -> CommandResult<Vec<WebTranslator>> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.translator_catalog())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn translator_code(state: State<'_, AppState>, id: String) -> CommandResult<String> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.translator_code(id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn read_translated_items(
    state: State<'_, AppState>,
    json: String,
    page_url: String,
) -> CommandResult<SearchPage> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.read_translated_items(json, page_url))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn translator_request(
    url: String,
    method: String,
    headers: Vec<PluginHeader>,
    body: Option<String>,
) -> CommandResult<TranslatorResponse> {
    tauri::async_runtime::spawn_blocking(move || {
        paperloom_core::translator_request(url, method, headers, body)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
