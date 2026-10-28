//! Notes sync conflict commands: reading a page's conflicts and resolving one.

use ganbaru_notes::conflicts::{NotesConflictError, NotesConflictRead, NotesConflictResolution};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

/// The conflicts of a page: its title, block text, property values, discussion status, and
/// comment bodies, each with the devices that wrote the versions.
#[tauri::command]
pub async fn notes_conflicts<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
) -> Result<NotesConflictRead, NotesConflictError> {
    let vault_id = crate::vault::active_vault_id(&app)?;
    let devices = crate::sync::devices::read(&app)?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_notes::conflicts::conflicts_from_pool(&pool, &vault_id, &devices, &page_id).await
}

/// Reads whether converting a row value stored under another type than its property's drops data.
#[tauri::command]
pub async fn notes_preview_mismatched_value<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    property_id: String,
) -> Result<ganbaru_notes::data_sources::MismatchedValuePreview, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_notes::data_sources::preview_mismatched_value(&pool, &page_id, &property_id).await
}

/// Converts a row value stored under another type than its property's current type, or discards it.
#[tauri::command]
pub async fn notes_settle_mismatched_value<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    property_id: String,
    convert: bool,
) -> Result<(), String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_notes::data_sources::settle_mismatched_value(&pool, &page_id, &property_id, convert)
        .await
}

/// Resolves a field in conflict by keeping the displayed version or using another one.
#[tauri::command]
pub async fn notes_resolve_conflict<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    resolution: NotesConflictResolution,
) -> Result<(), NotesConflictError> {
    let vault_id = crate::vault::active_vault_id(&app)?;
    let now_ms = crate::sync::now_ms()?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_notes::conflicts::resolve_from_pool(&pool, &vault_id, resolution, now_ms).await
}
