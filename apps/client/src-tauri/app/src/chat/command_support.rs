//! Shared infrastructure for thin Chat command facades.

use crate::db;
use chrono::{SecondsFormat, Utc};
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult, UtcTimestamp};
use sqlx::SqlitePool;

pub(crate) async fn chat_pool(app: tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app, db_url)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))
}

pub(crate) fn now_timestamp() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "create Chat timestamp", false))
}
