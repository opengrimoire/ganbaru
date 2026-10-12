//! Shared persistence, authorization, and timestamp helpers for interactions.

use crate::chat::workspace::{AuthorizedWorkingFolder, WorkingFolderAuthorizationOperation};
use crate::{chat, db};
use chrono::{SecondsFormat, Utc};
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId, UtcTimestamp, VersionedJson,
};
use sqlx::{Row, SqlitePool};

pub(super) async fn require_workspace(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    working_folder_id: &ProjectWorkingFolderId,
    operation: WorkingFolderAuthorizationOperation,
) -> ChatResult<AuthorizedWorkingFolder> {
    chat::workspace::commands::authorize_working_folder(app, pool, working_folder_id, operation)
        .await
}

pub(super) async fn chat_pool(app: tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app, db_url)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))
}

pub(super) fn now_timestamp() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "create Chat timestamp", false))
}

pub(super) fn versioned_row(
    row: &sqlx::sqlite::SqliteRow,
    version: &str,
    data: &str,
) -> ChatResult<VersionedJson> {
    Ok(VersionedJson {
        schema_version: u32::try_from(row.try_get::<i64, _>(version).map_err(persistence_error)?)
            .map_err(|_| corrupt_data_error())?,
        value: serde_json::from_str(&row.try_get::<String, _>(data).map_err(persistence_error)?)
            .map_err(json_error)?,
    })
}

pub(super) fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat interaction persistence failed",
        true,
    )
}

pub(super) fn json_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat interaction data is invalid",
        false,
    )
}

pub(super) fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat interaction is invalid",
        false,
    )
}
