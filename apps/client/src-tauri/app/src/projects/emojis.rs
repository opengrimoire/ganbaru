use ganbaru_projects::emojis;
use ganbaru_projects::{ProjectCustomEmojiCreate, ProjectsMutationRows};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;
use crate::vault;

#[tauri::command]
pub async fn projects_create_custom_emoji<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    emoji: ProjectCustomEmojiCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let vault_root = vault::active_writable_vault_path(&app)?;
    emojis::create_custom_emoji(&pool, &vault_root, emoji).await
}

#[tauri::command]
pub async fn projects_delete_custom_emoji<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    emoji_id: String,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    emojis::delete_custom_emoji(&pool, emoji_id).await
}
