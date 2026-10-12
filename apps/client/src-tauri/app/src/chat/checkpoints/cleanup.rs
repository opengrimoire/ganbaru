//! Durable cleanup service for exact checkpoint refs.

use crate::chat::workspace::WorkingFolderAuthorizationOperation;
use ganbaru_chat::checkpoints::delete_exact_ref;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId, UtcTimestamp,
};
use sqlx::{Row, SqlitePool};

pub(crate) async fn run_checkpoint_cleanup(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    now: &UtcTimestamp,
) -> ChatResult<u64> {
    let rows = sqlx::query(
        "SELECT id, working_folder_id, exact_target, repository_identity, expected_object_id
         FROM chat_cleanup_jobs
         WHERE cleanup_kind = 'checkpoint_ref' AND state IN ('pending', 'failed')
           AND available_at <= ?
         ORDER BY available_at, id LIMIT 100",
    )
    .bind(now.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut completed = 0;
    for row in rows {
        let cleanup_id: String = row.try_get("id").map_err(persistence_error)?;
        let working_folder_id: Option<String> = row
            .try_get("working_folder_id")
            .map_err(persistence_error)?;
        let reference: String = row.try_get("exact_target").map_err(persistence_error)?;
        let repository_identity: Option<String> = row
            .try_get("repository_identity")
            .map_err(persistence_error)?;
        let expected_object_id: Option<String> = row
            .try_get("expected_object_id")
            .map_err(persistence_error)?;
        sqlx::query(
            "UPDATE chat_cleanup_jobs
             SET state = 'running', attempt_count = attempt_count + 1, updated_at = ? WHERE id = ?",
        )
        .bind(now.as_str())
        .bind(&cleanup_id)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
        let result = match (working_folder_id, repository_identity, expected_object_id) {
            (Some(working_folder_id), Some(repository_identity), Some(expected_object_id)) => {
                let working_folder_id = ProjectWorkingFolderId::new(working_folder_id)
                    .map_err(|_| corrupt_data_error())?;
                let authorized = super::super::workspace::commands::authorize_working_folder(
                    app,
                    pool,
                    &working_folder_id,
                    WorkingFolderAuthorizationOperation::Restore,
                )
                .await?;
                if authorized.repository_identity.as_deref() != Some(&repository_identity) {
                    Err(ChatError::new(
                        ChatErrorCode::ConfigurationInvalid,
                        "Checkpoint cleanup repository identity changed",
                        true,
                    ))
                } else {
                    let root = authorized.canonical_path;
                    tauri::async_runtime::spawn_blocking(move || {
                        delete_exact_ref(&root, &reference, &expected_object_id)
                    })
                    .await
                    .map_err(|_| checkpoint_command_error())?
                }
            }
            _ => Err(corrupt_data_error()),
        };
        match result {
            Ok(()) => {
                sqlx::query(
                    "UPDATE chat_cleanup_jobs
                     SET state = 'completed', last_error_code = NULL, updated_at = ? WHERE id = ?",
                )
                .bind(now.as_str())
                .bind(&cleanup_id)
                .execute(pool)
                .await
                .map_err(persistence_error)?;
                completed += 1;
            }
            Err(error) => {
                sqlx::query(
                    "UPDATE chat_cleanup_jobs
                     SET state = 'failed', last_error_code = ?, updated_at = ? WHERE id = ?",
                )
                .bind(format!("{:?}", error.code).to_lowercase())
                .bind(now.as_str())
                .bind(&cleanup_id)
                .execute(pool)
                .await
                .map_err(persistence_error)?;
            }
        }
    }
    Ok(completed)
}

fn checkpoint_command_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Checkpoint cleanup worker stopped",
        true,
    )
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat checkpoint cleanup failed",
        true,
    )
}

fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat checkpoint cleanup data is invalid",
        false,
    )
}
