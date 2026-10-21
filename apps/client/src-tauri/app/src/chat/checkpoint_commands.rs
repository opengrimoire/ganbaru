//! Durable Chat cleanup command facade.

use super::cleanup_service::run_due_chat_cleanup;
use crate::db;
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult};
use sqlx::SqlitePool;

/// Retries due checkpoint ref and managed attachment cleanup now and returns
/// how many jobs completed.
#[tauri::command]
pub async fn chat_run_checkpoint_cleanup(app: tauri::AppHandle, db_url: String) -> ChatResult<u64> {
    let pool = chat_pool(app.clone(), db_url).await?;
    run_due_chat_cleanup(&app, &pool).await
}

async fn chat_pool(app: tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app, db_url)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))
}
