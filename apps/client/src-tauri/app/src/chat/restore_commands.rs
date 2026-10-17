//! Coordinated checkpoint restore preview and execution.

use super::workspace::{AuthorizedWorkingFolder, WorkingFolderAuthorizationOperation};
use crate::db;
use chrono::{SecondsFormat, Utc};
use ganbaru_chat::checkpoints::{
    ChatChangedFileRead, CurrentGitSnapshot, StoredCheckpoint, current_git_snapshot, diff_files,
    read_stored_checkpoint, restore_git_snapshot, verify_checkpoint,
};
use ganbaru_chat::repository::receipts::{
    CommandReceiptClaim, CommandReceiptRead, CommandReceiptState, claim_command_receipt,
    complete_command_receipt,
};
use ganbaru_chat::runtime::ChatRuntimeRegistry;
use ganbaru_chat_contracts::events::{CanonicalEvent, ThreadRevertedEvent};
use ganbaru_chat_contracts::models::{
    ChatCheckpointId, ChatCommandContext, ChatError, ChatErrorCode, ChatResult, ChatThreadId,
    ChatTurnId, InterruptTurnRequest, ProjectWorkingFolderId, ProviderCapability, RollbackRequest,
    UtcTimestamp, VersionedJson,
};
use ganbaru_chat_providers::{DriverCancellation, DriverOperationContext};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::time::{Duration, Instant};
use tauri::Manager;

mod persistence;
mod preview;
mod provider_history;
mod saga;

use persistence::{
    mark_preview_state, persist_restore, read_preview, record_restore_operation,
    settle_executing_preview_failure,
};
use preview::{
    StoredRestorePreview, add_duration, preview_id, require_head_context, transient_checkpoint,
};
use provider_history::{stored_provider_rollback_cursor, target_turn_id};
use saga::{RestoreFailurePhase, execute_restore};

const RESTORE_PREVIEW_LIFETIME: chrono::Duration = chrono::Duration::minutes(15);

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewChatRestoreRequest {
    pub thread_id: ChatThreadId,
    pub checkpoint_id: ChatCheckpointId,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteChatRestoreRequest {
    pub command: ChatCommandContext,
    pub thread_id: ChatThreadId,
    pub preview_id: String,
    pub confirmed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRestorePreviewRead {
    pub preview_id: String,
    pub checkpoint_id: ChatCheckpointId,
    pub expires_at: UtcTimestamp,
    pub files: Vec<ChatChangedFileRead>,
    pub staged_changes: bool,
    pub provider_rollback: String,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRestoreResultRead {
    pub checkpoint_id: ChatCheckpointId,
    pub reverted_turn_ids: Vec<ChatTurnId>,
    pub provider_history_action: String,
    pub recovery_state: String,
    pub thread_revision: u64,
}

#[tauri::command]
pub async fn chat_preview_checkpoint_restore(
    app: tauri::AppHandle,
    db_url: String,
    request: PreviewChatRestoreRequest,
) -> ChatResult<ChatRestorePreviewRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let (working_folder_id, authorized, revision) =
        authorize_thread(&app, &pool, &request.thread_id).await?;
    let target = read_stored_checkpoint(&pool, &request.checkpoint_id).await?;
    if target.thread_id != request.thread_id
        || authorized.repository_identity.as_deref() != Some(&target.repository_identity)
    {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Checkpoint belongs to another thread or repository",
            false,
        ));
    }
    let authorized_for_git = authorized.clone();
    let target_for_git = target.clone();
    let (current, files) = tauri::async_runtime::spawn_blocking(move || {
        verify_checkpoint(&authorized_for_git, &target_for_git)?;
        let current = current_git_snapshot(&authorized_for_git.canonical_path)?;
        require_head_context(&current, &target_for_git)?;
        let current_checkpoint = transient_checkpoint(&target_for_git, &current);
        let files = diff_files(
            &authorized_for_git.canonical_path,
            &current_checkpoint,
            &target_for_git,
        )?;
        Ok::<_, ChatError>((current, files))
    })
    .await
    .map_err(|_| restore_worker_error())??;
    let now = now_timestamp()?;
    let expires_at = add_duration(&now, RESTORE_PREVIEW_LIFETIME)?;
    let preview_id = preview_id(&request.thread_id, &request.checkpoint_id, &current, &now);
    let files_data = serde_json::to_string(&files).map_err(json_error)?;
    sqlx::query(
        "INSERT INTO chat_restore_previews
            (id, thread_id, checkpoint_id, expected_thread_revision, repository_identity,
             head_oid, head_ref, current_worktree_tree_oid, current_index_tree_oid,
             current_index_fingerprint, affected_files_data, state, expires_at, created_at,
             updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'ready', ?, ?, ?)",
    )
    .bind(&preview_id)
    .bind(request.thread_id.as_str())
    .bind(request.checkpoint_id.as_str())
    .bind(i64_value(revision)?)
    .bind(target.repository_identity)
    .bind(current.head_oid.as_deref())
    .bind(current.head_ref.as_deref())
    .bind(&current.worktree_tree_oid)
    .bind(&current.index_tree_oid)
    .bind(&current.index_fingerprint)
    .bind(files_data)
    .bind(expires_at.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    let capabilities = app
        .state::<ChatRuntimeRegistry>()
        .owner(request.thread_id)?
        .snapshot()?
        .capabilities;
    let staged_changes = current.index_tree_oid != target.index_tree_oid;
    let mut warnings = Vec::new();
    if staged_changes {
        warnings.push("The real staging state will be restored to the checkpoint.".to_string());
    }
    if !capabilities.supports(ProviderCapability::NativeRollback) {
        warnings.push(
            "The provider cannot align native history, so the restored conversation will require a fork."
                .to_string(),
        );
    }
    if working_folder_id != authorized.working_folder_id {
        return Err(restore_worker_error());
    }
    Ok(ChatRestorePreviewRead {
        preview_id,
        checkpoint_id: request.checkpoint_id,
        expires_at,
        files,
        staged_changes,
        provider_rollback: if capabilities.supports(ProviderCapability::NativeRollback) {
            "supported"
        } else {
            "unsupported"
        }
        .to_string(),
        warnings,
    })
}

#[tauri::command]
pub async fn chat_execute_checkpoint_restore(
    app: tauri::AppHandle,
    db_url: String,
    request: ExecuteChatRestoreRequest,
) -> ChatResult<ChatRestoreResultRead> {
    if !request.confirmed {
        return Err(ChatError::validation(
            "confirmed",
            "Checkpoint restore requires explicit confirmation",
        ));
    }
    let pool = chat_pool(app.clone(), db_url).await?;
    let now = now_timestamp()?;
    match claim_command_receipt(
        &pool,
        &request.command.client_command_id,
        &request.thread_id,
        "restore_checkpoint",
        request.command.expected_thread_revision,
        &now,
    )
    .await?
    {
        CommandReceiptClaim::Replay(receipt) => return replay_restore(receipt),
        CommandReceiptClaim::Claimed(_) => {}
    }
    let result = execute_restore(&app, &pool, &request, &now).await;
    match result {
        Ok(result) => {
            let value = versioned_value(&result)?;
            complete_command_receipt(
                &pool,
                &request.command.client_command_id,
                CommandReceiptState::Completed,
                Some(&value),
                None,
                &now_timestamp()?,
            )
            .await?;
            Ok(result)
        }
        Err(error) => {
            let value = versioned_error(&error)?;
            complete_command_receipt(
                &pool,
                &request.command.client_command_id,
                CommandReceiptState::Failed,
                None,
                Some(&value),
                &now_timestamp()?,
            )
            .await?;
            Err(error)
        }
    }
}

async fn authorize_thread(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<(ProjectWorkingFolderId, AuthorizedWorkingFolder, u64)> {
    let (working_folder_id, _, authorized, revision) =
        super::execution_environment::authorize_thread_environment(
            app,
            pool,
            thread_id,
            WorkingFolderAuthorizationOperation::Restore,
        )
        .await?;
    Ok((working_folder_id, authorized, revision))
}

fn replay_restore(receipt: CommandReceiptRead) -> ChatResult<ChatRestoreResultRead> {
    match receipt.state {
        CommandReceiptState::Completed => {
            let result = receipt.result.ok_or_else(corrupt_data_error)?;
            serde_json::from_value(serde_json::to_value(result.value).map_err(json_error)?)
                .map_err(json_error)
        }
        CommandReceiptState::Failed => Err(ChatError::new(
            ChatErrorCode::Conflict,
            "This checkpoint restore previously failed",
            true,
        )),
        CommandReceiptState::Accepted => Err(ChatError::new(
            ChatErrorCode::Busy,
            "This checkpoint restore is already running",
            true,
        )),
    }
}

fn versioned_value<T: Serialize>(value: &T) -> ChatResult<VersionedJson> {
    Ok(VersionedJson {
        schema_version: 1,
        value: serde_json::to_value(value).map_err(json_error)?,
    })
}

fn versioned_error(error: &ChatError) -> ChatResult<VersionedJson> {
    versioned_value(&json!({
        "code": format!("{:?}", error.code).to_lowercase(),
        "message": error.message,
        "recoverable": error.recoverable,
    }))
}

fn operation_context(operation_id: &str, timeout: Duration) -> DriverOperationContext {
    DriverOperationContext {
        operation_id: operation_id.to_string(),
        deadline: Instant::now() + timeout,
        cancellation: DriverCancellation::default(),
    }
}

fn now_timestamp() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "create Chat timestamp", false))
}

async fn chat_pool(app: tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app, db_url)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))
}

fn i64_value(value: u64) -> ChatResult<i64> {
    i64::try_from(value).map_err(|_| ChatError::validation("number", "Value is too large"))
}

fn restore_worker_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Checkpoint restore worker stopped",
        true,
    )
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Checkpoint restore persistence failed",
        true,
    )
}

fn json_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Checkpoint restore metadata is invalid",
        false,
    )
}

fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored checkpoint restore data is invalid",
        false,
    )
}
