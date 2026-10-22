//! Theme command adapters. Persistence, validation, and conflicts live in `ganbaru-themes`;
//! these commands admit writes from the owner and from linked replicas, since user themes
//! replicate concurrently.

use ganbaru_sync::manifest::vault::themes::DOMAIN_NAME as THEMES_DOMAIN;
use ganbaru_themes::{
    DismissalRow, ThemeConflictError, ThemeConflictRead, ThemeConflictResolution, UserThemeRead,
    UserThemeWrite,
};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;
use crate::vault::ownership::{READ_ONLY_ERROR, active_status};

/// Admits the owner and linked replicas that may write themes.
fn require_writable<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if active_status(app)?.can_write_domain(THEMES_DOMAIN) {
        Ok(())
    } else {
        Err(READ_ONLY_ERROR.to_string())
    }
}

#[tauri::command]
pub async fn themes_load_all<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<Vec<UserThemeRead>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_themes::load_all(&pool).await
}

#[tauri::command]
pub async fn themes_insert<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    write: UserThemeWrite,
) -> Result<(), String> {
    require_writable(&app)?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_themes::insert(&pool, &write).await
}

#[tauri::command]
pub async fn themes_replace_content<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    write: UserThemeWrite,
) -> Result<(), String> {
    require_writable(&app)?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_themes::replace_content(&pool, &write).await
}

#[tauri::command]
pub async fn themes_delete<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
) -> Result<(), String> {
    require_writable(&app)?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_themes::delete(&pool, &id).await
}

#[tauri::command]
pub async fn themes_record_dismissal<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
    engine_version: i64,
) -> Result<(), String> {
    require_writable(&app)?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_themes::record_dismissal(&pool, &id, engine_version).await
}

#[tauri::command]
pub async fn themes_load_dismissals<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<Vec<DismissalRow>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_themes::load_dismissals(&pool).await
}

#[tauri::command]
pub async fn themes_rename<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
    display_name: String,
) -> Result<(), String> {
    require_writable(&app)?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_themes::rename(&pool, &id, &display_name).await
}

#[tauri::command]
pub async fn themes_reset_token_to_seed<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
    kind: String,
    key: String,
) -> Result<(), String> {
    require_writable(&app)?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_themes::reset_token_to_seed(&pool, &id, &kind, &key).await
}

#[tauri::command]
pub async fn themes_reset_to_seed<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
) -> Result<(), String> {
    require_writable(&app)?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_themes::reset_to_seed(&pool, &id).await
}

/// The content versions of a theme left by concurrent saves on linked devices, with the device
/// that wrote each one.
#[tauri::command]
pub async fn themes_conflict<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
) -> Result<ThemeConflictRead, ThemeConflictError> {
    let vault_id = crate::vault::active_vault_id(&app)?;
    let devices = crate::sync::devices::read(&app)?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_themes::conflict_from_pool(&pool, &vault_id, &devices, &id).await
}

/// Resolves a theme content conflict by keeping the displayed version or using another one.
#[tauri::command]
pub async fn themes_resolve_conflict<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    resolution: ThemeConflictResolution,
) -> Result<UserThemeRead, ThemeConflictError> {
    require_writable(&app)?;
    let vault_id = crate::vault::active_vault_id(&app)?;
    let now_ms = crate::sync::now_ms()?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_themes::resolve_from_pool(&pool, &vault_id, resolution, now_ms).await
}
