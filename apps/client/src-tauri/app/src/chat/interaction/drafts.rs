//! Interactive user-input draft persistence.

use super::support::{
    chat_pool, corrupt_data_error, json_error, now_timestamp, persistence_error, versioned_row,
};
use crate::chat::interaction_commands::ChatUserInputDraftRead;
use ganbaru_chat_contracts::models::{ChatError, ChatResult, UtcTimestamp, VersionedJson};
use sqlx::{Row, SqlitePool};

pub(crate) async fn read_user_input_draft(
    app: tauri::AppHandle,
    db_url: String,
    request_id: String,
) -> ChatResult<Option<ChatUserInputDraftRead>> {
    read_user_input_draft_from_pool(&chat_pool(app, db_url).await?, &request_id).await
}

async fn read_user_input_draft_from_pool(
    pool: &SqlitePool,
    request_id: &str,
) -> ChatResult<Option<ChatUserInputDraftRead>> {
    let row = sqlx::query(
        "SELECT request_id, answers_schema_version, answers_data, updated_at
         FROM chat_user_input_drafts WHERE request_id = ?",
    )
    .bind(request_id)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    row.map(|row| {
        Ok(ChatUserInputDraftRead {
            request_id: row.try_get("request_id").map_err(persistence_error)?,
            answers: versioned_row(&row, "answers_schema_version", "answers_data")?,
            updated_at: UtcTimestamp::new(
                row.try_get::<String, _>("updated_at")
                    .map_err(persistence_error)?,
            )
            .map_err(|_| corrupt_data_error())?,
        })
    })
    .transpose()
}

pub(crate) async fn save_user_input_draft(
    app: tauri::AppHandle,
    db_url: String,
    request_id: String,
    answers: VersionedJson,
) -> ChatResult<ChatUserInputDraftRead> {
    if !answers.value.is_array() {
        return Err(ChatError::validation(
            "answers",
            "Chat answer draft must be an array",
        ));
    }
    let pool = chat_pool(app, db_url).await?;
    let now = now_timestamp()?;
    sqlx::query(
        "INSERT INTO chat_user_input_drafts
            (request_id, answers_schema_version, answers_data, updated_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(request_id) DO UPDATE SET
            answers_schema_version = excluded.answers_schema_version,
            answers_data = excluded.answers_data, updated_at = excluded.updated_at",
    )
    .bind(&request_id)
    .bind(i64::from(answers.schema_version))
    .bind(serde_json::to_string(&answers.value).map_err(json_error)?)
    .bind(now.as_str())
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    read_user_input_draft_from_pool(&pool, &request_id)
        .await?
        .ok_or_else(corrupt_data_error)
}
