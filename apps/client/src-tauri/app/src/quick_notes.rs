//! Quick notes command adapters. Persistence, ordering, tags, and conflicts live in
//! `ganbaru-quick-notes`; these commands authorize the active vault and the device's write
//! access before calling it.

use ganbaru_quick_notes::{
    QuickNoteConflictError, QuickNoteConflictRead, QuickNoteConflictResolution,
    QuickNoteConflictResolved, QuickNotePinRequest, QuickNoteRead, QuickNoteReorderRequest,
    QuickNoteRevisionRequest, QuickNoteTagError, QuickNoteTagRead, QuickNoteTagWrite,
    QuickNoteUpdate, QuickNoteWrite, QuickNoteWriteError, QuickNotesListRequest,
    QuickNotesTrashPurge, QuickNotesWindow, tags,
};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

#[tauri::command]
pub async fn quick_notes_list<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: QuickNotesListRequest,
) -> Result<QuickNotesWindow, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_quick_notes::list_window_from_pool(&pool, request).await
}

#[tauri::command]
pub async fn quick_notes_load<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
) -> Result<QuickNoteRead, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_quick_notes::load_note_from_pool(&pool, &id).await
}

/// Deletes trash past its retention. Devices that cannot write Quick notes, neither as owner
/// nor as a linked replica, skip the pass and report no deadline.
#[tauri::command]
pub async fn quick_notes_purge_expired_trash<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<QuickNotesTrashPurge, String> {
    let status = crate::vault::ownership::active_status(&app)?;
    if !status.can_write && !status.replicated_writes {
        return Ok(QuickNotesTrashPurge::skipped());
    }
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_quick_notes::purge_expired_trash(&pool).await
}

#[tauri::command]
pub async fn quick_notes_create<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    note: QuickNoteWrite,
) -> Result<QuickNoteRead, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_quick_notes::create_from_pool(&pool, note).await
}

#[tauri::command]
pub async fn quick_notes_update<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    note: QuickNoteUpdate,
) -> Result<QuickNoteRead, QuickNoteWriteError> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_quick_notes::update_from_pool(&pool, note).await
}

#[tauri::command]
pub async fn quick_notes_set_pinned<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: QuickNotePinRequest,
) -> Result<QuickNoteRead, QuickNoteWriteError> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_quick_notes::set_pinned_from_pool(&pool, request).await
}

#[tauri::command]
pub async fn quick_notes_reorder<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: QuickNoteReorderRequest,
) -> Result<(), String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_quick_notes::reorder_from_pool(&pool, request).await
}

#[tauri::command]
pub async fn quick_notes_archive<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: QuickNoteRevisionRequest,
) -> Result<QuickNoteRead, QuickNoteWriteError> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_quick_notes::archive_from_pool(&pool, &request).await
}

#[tauri::command]
pub async fn quick_notes_unarchive<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: QuickNoteRevisionRequest,
) -> Result<QuickNoteRead, QuickNoteWriteError> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_quick_notes::unarchive_from_pool(&pool, &request).await
}

#[tauri::command]
pub async fn quick_notes_trash<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: QuickNoteRevisionRequest,
) -> Result<QuickNoteRead, QuickNoteWriteError> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_quick_notes::trash_from_pool(&pool, &request).await
}

#[tauri::command]
pub async fn quick_notes_restore<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: QuickNoteRevisionRequest,
) -> Result<QuickNoteRead, QuickNoteWriteError> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_quick_notes::restore_from_pool(&pool, &request).await
}

#[tauri::command]
pub async fn quick_notes_delete_permanently<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
) -> Result<(), String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_quick_notes::delete_permanently_from_pool(&pool, &id).await
}

#[tauri::command]
pub async fn quick_notes_empty_trash<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<u64, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_quick_notes::empty_trash_from_pool(&pool).await
}

#[tauri::command]
pub async fn quick_notes_list_tags<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<Vec<QuickNoteTagRead>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    tags::list_from_pool(&pool).await
}

#[tauri::command]
pub async fn quick_notes_create_tag<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    tag: QuickNoteTagWrite,
) -> Result<QuickNoteTagRead, QuickNoteTagError> {
    let pool = connect_sqlite(app, db_url).await?;
    tags::create_from_pool(&pool, tag).await
}

#[tauri::command]
pub async fn quick_notes_rename_tag<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    tag: QuickNoteTagWrite,
) -> Result<QuickNoteTagRead, QuickNoteTagError> {
    let pool = connect_sqlite(app, db_url).await?;
    tags::rename_from_pool(&pool, tag).await
}

#[tauri::command]
pub async fn quick_notes_delete_tag<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
) -> Result<(), QuickNoteTagError> {
    let pool = connect_sqlite(app, db_url).await?;
    tags::delete_from_pool(&pool, &id).await
}

/// The conflicts of a note, with every version and the device that wrote it.
#[tauri::command]
pub async fn quick_notes_conflict<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
) -> Result<QuickNoteConflictRead, QuickNoteConflictError> {
    let vault_id = crate::vault::active_vault_id(&app)?;
    let devices = crate::sync::devices::read(&app)?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_quick_notes::conflict_from_pool(&pool, &vault_id, &devices, &id).await
}

/// Resolves a title or body conflict by keeping the displayed version, using another one, or
/// keeping both in separate notes.
#[tauri::command]
pub async fn quick_notes_resolve_conflict<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    resolution: QuickNoteConflictResolution,
) -> Result<QuickNoteConflictResolved, QuickNoteConflictError> {
    let vault_id = crate::vault::active_vault_id(&app)?;
    let now_ms = crate::sync::now_ms()?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_quick_notes::resolve_from_pool(&pool, &vault_id, resolution, now_ms).await
}
