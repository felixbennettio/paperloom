//! AI with the person's own key: listing a provider's models, an item's
//! summary, questions about a PDF, help with a passage and mind maps
//! drafted from a note or a paper. The key is kept
//! with the app's other secrets; the rest of the settings come from the UI.

use paperloom_core::{AiAnswer, AiLibraryAnswer, AiMindMap, AiModel, AiProvider, AiSemanticStatus};
use serde::Deserialize;
use tauri::{AppHandle, Manager, Runtime, State};

use crate::secrets::Secrets;
use crate::{AppState, CommandResult};

/// The provider settings the UI keeps.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AiSettings {
    protocol: String,
    base_url: String,
    model: String,
    #[serde(default)]
    embedding_model: String,
}

fn provider<R: Runtime>(app: &AppHandle<R>, settings: AiSettings) -> AiProvider {
    let api_key = app.state::<Secrets>().load().ai_key.unwrap_or_default();
    AiProvider {
        api: settings.protocol,
        base_url: settings.base_url,
        api_key,
        model: settings.model,
        embedding_model: settings.embedding_model,
    }
}

/// Runs a blocking AI call off the async runtime.
async fn blocking<T: Send + 'static>(
    call: impl FnOnce() -> Result<T, paperloom_core::CoreError> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(move || call().map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/// Keeps the AI provider's key; an empty one forgets it.
#[tauri::command]
pub(crate) fn ai_key_set(secrets: State<'_, Secrets>, key: String) -> CommandResult<()> {
    secrets.update(|kept| kept.ai_key = Some(key.trim().to_string()).filter(|k| !k.is_empty()))
}

#[tauri::command]
pub(crate) fn ai_key_known(secrets: State<'_, Secrets>) -> bool {
    secrets.load().ai_key.is_some()
}

#[tauri::command]
pub(crate) async fn ai_models<R: Runtime>(
    app: AppHandle<R>,
    settings: AiSettings,
) -> CommandResult<Vec<AiModel>> {
    let provider = provider(&app, settings);
    blocking(move || paperloom_core::ai_models(provider)).await
}

#[tauri::command]
pub(crate) async fn ai_semantic_status<R: Runtime>(
    app: AppHandle<R>,
    settings: AiSettings,
) -> CommandResult<AiSemanticStatus> {
    let provider = provider(&app, settings);
    let library = app.state::<AppState>().library.clone();
    blocking(move || library.ai_semantic_status(provider)).await
}

#[tauri::command]
pub(crate) async fn ai_semantic_index<R: Runtime>(
    app: AppHandle<R>,
    settings: AiSettings,
    limit: u32,
) -> CommandResult<AiSemanticStatus> {
    let provider = provider(&app, settings);
    let library = app.state::<AppState>().library.clone();
    blocking(move || library.ai_semantic_index(provider, limit)).await
}

#[tauri::command]
pub(crate) async fn ai_summarize<R: Runtime>(
    app: AppHandle<R>,
    settings: AiSettings,
    item_id: String,
    language: String,
) -> CommandResult<AiAnswer> {
    let provider = provider(&app, settings);
    let library = app.state::<AppState>().library.clone();
    blocking(move || library.ai_summarize(provider, item_id, language)).await
}

#[tauri::command]
pub(crate) async fn ai_ask<R: Runtime>(
    app: AppHandle<R>,
    settings: AiSettings,
    attachment_id: String,
    question: String,
    language: String,
) -> CommandResult<AiAnswer> {
    let provider = provider(&app, settings);
    let library = app.state::<AppState>().library.clone();
    blocking(move || library.ai_ask(provider, attachment_id, question, language)).await
}

/// An answer from the whole library, citing its sources.
#[tauri::command]
pub(crate) async fn ai_ask_library<R: Runtime>(
    app: AppHandle<R>,
    settings: AiSettings,
    question: String,
    language: String,
) -> CommandResult<AiLibraryAnswer> {
    let provider = provider(&app, settings);
    let library = app.state::<AppState>().library.clone();
    blocking(move || library.ai_ask_library(provider, question, language)).await
}

/// A mind map drafted from a note: the text of a new map note.
#[tauri::command]
pub(crate) async fn ai_mindmap<R: Runtime>(
    app: AppHandle<R>,
    settings: AiSettings,
    title: String,
    text: String,
    language: String,
) -> CommandResult<String> {
    let provider = provider(&app, settings);
    blocking(move || paperloom_core::ai_mindmap(provider, title, text, language)).await
}

/// A mind map drafted from a paper.
#[tauri::command]
pub(crate) async fn ai_mindmap_item<R: Runtime>(
    app: AppHandle<R>,
    settings: AiSettings,
    item_id: String,
    language: String,
) -> CommandResult<AiMindMap> {
    let provider = provider(&app, settings);
    let library = app.state::<AppState>().library.clone();
    blocking(move || library.ai_mindmap_item(provider, item_id, language)).await
}

#[tauri::command]
pub(crate) async fn ai_transform<R: Runtime>(
    app: AppHandle<R>,
    settings: AiSettings,
    task: String,
    text: String,
    language: String,
) -> CommandResult<AiAnswer> {
    let provider = provider(&app, settings);
    blocking(move || paperloom_core::ai_transform(provider, task, text, language)).await
}
