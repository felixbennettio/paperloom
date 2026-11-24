//! Editing items, stars, reading status, tags and the trash.

use paperloom_core::{FeedInfo, FeedItem, ItemDetail, ItemSummary, SavedSearch, SearchField, TagInfo};
use tauri::State;

use crate::{AppState, CommandResult};

#[tauri::command]
pub(crate) fn update_item(state: State<'_, AppState>, detail: ItemDetail) -> CommandResult<ItemDetail> {
    state.library.update_item(detail).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn set_starred(state: State<'_, AppState>, item_id: String, starred: bool) -> CommandResult<()> {
    state.library.set_starred(item_id, starred).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn set_read_status(
    state: State<'_, AppState>,
    item_id: String,
    status: String,
) -> CommandResult<()> {
    state.library.set_read_status(item_id, status).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn tags(state: State<'_, AppState>) -> CommandResult<Vec<TagInfo>> {
    state.library.tags().map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn set_item_tags(
    state: State<'_, AppState>,
    item_id: String,
    tags: Vec<String>,
) -> CommandResult<()> {
    state.library.set_item_tags(item_id, tags).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn set_tag_color(
    state: State<'_, AppState>,
    name: String,
    color: Option<String>,
) -> CommandResult<()> {
    state.library.set_tag_color(name, color).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn rename_tag(state: State<'_, AppState>, name: String, new_name: String) -> CommandResult<()> {
    state.library.rename_tag(name, new_name).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn delete_tag(state: State<'_, AppState>, name: String) -> CommandResult<()> {
    state.library.delete_tag(name).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn restore_item(state: State<'_, AppState>, item_id: String) -> CommandResult<()> {
    state.library.restore_item(item_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn delete_item(state: State<'_, AppState>, item_id: String) -> CommandResult<()> {
    state.library.delete_item(item_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn empty_trash(state: State<'_, AppState>) -> CommandResult<u32> {
    state.library.empty_trash().map_err(|e| e.to_string())
}

/// Items that look like the same work.
#[tauri::command]
pub(crate) fn duplicate_groups(
    state: State<'_, AppState>,
) -> CommandResult<Vec<paperloom_core::DuplicateGroup>> {
    state.library.duplicate_groups().map_err(|e| e.to_string())
}

/// Merges items into one; the merged item.
#[tauri::command]
pub(crate) fn merge_items(
    state: State<'_, AppState>,
    master_id: String,
    other_ids: Vec<String>,
) -> CommandResult<paperloom_core::ItemDetail> {
    state.library.merge_items(master_id, other_ids).map_err(|e| e.to_string())
}

/// The fields saved-search conditions can test.
#[tauri::command]
pub(crate) fn search_fields() -> Vec<SearchField> {
    paperloom_core::search_fields()
}

#[tauri::command]
pub(crate) fn saved_searches(state: State<'_, AppState>) -> CommandResult<Vec<SavedSearch>> {
    state.library.saved_searches().map_err(|e| e.to_string())
}

/// Saves a search (a new one when its ID is empty); the search as saved.
#[tauri::command]
pub(crate) fn save_search(state: State<'_, AppState>, search: SavedSearch) -> CommandResult<SavedSearch> {
    state.library.save_search(search).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn delete_saved_search(state: State<'_, AppState>, search_id: String) -> CommandResult<()> {
    state.library.delete_saved_search(search_id).map_err(|e| e.to_string())
}

/// The items a search finds, saved or being edited.
#[tauri::command]
pub(crate) fn run_search(state: State<'_, AppState>, search: SavedSearch) -> CommandResult<Vec<ItemSummary>> {
    state.library.run_search(search).map_err(|e| e.to_string())
}

/// The items related to an item, not counting the trash.
#[tauri::command]
pub(crate) fn related_items(state: State<'_, AppState>, item_id: String) -> CommandResult<Vec<ItemSummary>> {
    state.library.related_items(item_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn relate_items(
    state: State<'_, AppState>,
    item_id: String,
    other_id: String,
) -> CommandResult<()> {
    state.library.relate_items(item_id, other_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn unrelate_items(
    state: State<'_, AppState>,
    item_id: String,
    other_id: String,
) -> CommandResult<()> {
    state.library.unrelate_items(item_id, other_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn feeds(state: State<'_, AppState>) -> CommandResult<Vec<FeedInfo>> {
    state.library.feeds().map_err(|e| e.to_string())
}

/// Follows a feed and reads it, off the main thread.
#[tauri::command]
pub(crate) async fn add_feed(state: State<'_, AppState>, url: String) -> CommandResult<FeedInfo> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.add_feed(url))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Reads every feed again, off the main thread.
#[tauri::command]
pub(crate) async fn refresh_feeds(state: State<'_, AppState>) -> CommandResult<Vec<FeedInfo>> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.refresh_feeds())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn rename_feed(state: State<'_, AppState>, feed_id: String, title: String) -> CommandResult<()> {
    state.library.rename_feed(feed_id, title).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn remove_feed(state: State<'_, AppState>, feed_id: String) -> CommandResult<()> {
    state.library.remove_feed(feed_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn feed_items(
    state: State<'_, AppState>,
    feed_id: Option<String>,
) -> CommandResult<Vec<FeedItem>> {
    state.library.feed_items(feed_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn set_feed_item_read(
    state: State<'_, AppState>,
    item_id: String,
    read: bool,
) -> CommandResult<()> {
    state.library.set_feed_item_read(item_id, read).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn mark_feed_read(state: State<'_, AppState>, feed_id: Option<String>) -> CommandResult<()> {
    state.library.mark_feed_read(feed_id).map_err(|e| e.to_string())
}

/// Adds a feed entry to the library, looked up by DOI when it has one, off
/// the main thread.
#[tauri::command]
pub(crate) async fn add_feed_item(
    state: State<'_, AppState>,
    item_id: String,
    collection_id: Option<String>,
) -> CommandResult<ItemSummary> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.add_feed_item(item_id, collection_id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Looks up how many works cite some items (all, when none are named) and
/// keeps the counts, off the main thread; how many were counted.
#[tauri::command]
pub(crate) async fn count_citations(state: State<'_, AppState>, item_ids: Vec<String>) -> CommandResult<u32> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.count_citations(item_ids))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
