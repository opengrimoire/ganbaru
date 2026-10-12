//! Theme command adapters. Persistence and validation live in `ganbaru-themes`.

use ganbaru_themes::{DismissalRow, UserThemeRead, UserThemeWrite};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

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
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_themes::insert(&pool, &write).await
}

#[tauri::command]
pub async fn themes_replace_content<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    write: UserThemeWrite,
) -> Result<(), String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_themes::replace_content(&pool, &write).await
}

#[tauri::command]
pub async fn themes_delete<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
) -> Result<(), String> {
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
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_themes::reset_token_to_seed(&pool, &id, &kind, &key).await
}

#[tauri::command]
pub async fn themes_reset_to_seed<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
) -> Result<(), String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_themes::reset_to_seed(&pool, &id).await
}
