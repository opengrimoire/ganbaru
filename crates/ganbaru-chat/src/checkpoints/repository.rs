//! SQLite persistence for checkpoint metadata and turn associations.

use super::{CapturedCheckpoint, CheckpointKind, StoredCheckpoint};
use crate::models::{
    ChatCheckpointId, ChatError, ChatErrorCode, ChatResult, ChatThreadId, ChatTurnId, UtcTimestamp,
};
use sqlx::{Row, SqlitePool};

pub(super) async fn available_checkpoint_exists(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    turn_count: u64,
) -> ChatResult<bool> {
    sqlx::query(
        "SELECT id FROM chat_checkpoints
         WHERE thread_id = ? AND turn_count = ? AND status = 'available'",
    )
    .bind(thread_id.as_str())
    .bind(i64_value(turn_count)?)
    .fetch_optional(pool)
    .await
    .map(|row| row.is_some())
    .map_err(persistence_error)
}

pub(super) async fn latest_checkpoint_oid(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<Option<String>> {
    sqlx::query_scalar(
        "SELECT git_object_id FROM chat_checkpoints
         WHERE thread_id = ? AND status = 'available' AND invalidated_at IS NULL
         ORDER BY turn_count DESC LIMIT 1",
    )
    .bind(thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)
}

/// Inputs for storing a checkpoint row and its turn association in one transaction.
pub(super) struct StoreCheckpointRequest<'a> {
    pub captured: &'a CapturedCheckpoint,
    pub thread_id: &'a ChatThreadId,
    pub turn_id: Option<&'a ChatTurnId>,
    pub turn_count: u64,
    pub repository_identity: &'a str,
    pub kind: CheckpointKind,
    pub now: &'a UtcTimestamp,
}

pub(super) async fn insert_checkpoint_and_associate_turn(
    pool: &SqlitePool,
    request: StoreCheckpointRequest<'_>,
) -> ChatResult<()> {
    let StoreCheckpointRequest {
        captured,
        thread_id,
        turn_id,
        turn_count,
        repository_identity,
        kind,
        now,
    } = request;
    let changed_files_data =
        serde_json::to_string(&captured.changed_files).map_err(serialization_error)?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_checkpoints
            (id, thread_id, turn_count, repository_identity, hidden_ref_name, git_object_id,
             status, changed_files_data, created_at, checkpoint_kind, turn_id,
             index_commit_oid, index_tree_oid, worktree_tree_oid, head_oid, head_ref,
             index_fingerprint)
         VALUES (?, ?, ?, ?, ?, ?, 'available', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(captured.id.as_str())
    .bind(thread_id.as_str())
    .bind(i64_value(turn_count)?)
    .bind(repository_identity)
    .bind(&captured.hidden_ref_name)
    .bind(&captured.git_object_id)
    .bind(changed_files_data)
    .bind(now.as_str())
    .bind(kind.as_str())
    .bind(turn_id.map(ChatTurnId::as_str))
    .bind(&captured.index_commit_oid)
    .bind(&captured.index_tree_oid)
    .bind(&captured.worktree_tree_oid)
    .bind(captured.head_oid.as_deref())
    .bind(captured.head_ref.as_deref())
    .bind(&captured.index_fingerprint)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if let Some(turn_id) = turn_id {
        let column = if matches!(kind, CheckpointKind::Initial | CheckpointKind::PreTurn) {
            "pre_checkpoint_id"
        } else {
            "post_checkpoint_id"
        };
        let statement =
            format!("UPDATE chat_turns SET {column} = ? WHERE id = ? AND thread_id = ?");
        let updated = sqlx::query(&statement)
            .bind(captured.id.as_str())
            .bind(turn_id.as_str())
            .bind(thread_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        if updated.rows_affected() != 1 {
            return Err(ChatError::new(
                ChatErrorCode::Persistence,
                "Chat checkpoint turn association failed",
                true,
            ));
        }
    }
    transaction.commit().await.map_err(persistence_error)
}

pub async fn read_stored_checkpoint(
    pool: &SqlitePool,
    checkpoint_id: &ChatCheckpointId,
) -> ChatResult<StoredCheckpoint> {
    let row = sqlx::query(
        "SELECT id, thread_id, turn_count, repository_identity, hidden_ref_name,
                git_object_id, index_tree_oid, worktree_tree_oid, head_oid, head_ref
         FROM chat_checkpoints
         WHERE id = ? AND status = 'available' AND invalidated_at IS NULL",
    )
    .bind(checkpoint_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Chat checkpoint is unavailable",
            true,
        )
    })?;
    Ok(StoredCheckpoint {
        id: parse_id(row.try_get("id").map_err(persistence_error)?)?,
        thread_id: parse_thread_id(row.try_get("thread_id").map_err(persistence_error)?)?,
        turn_count: u64_column(&row, "turn_count")?,
        repository_identity: row
            .try_get("repository_identity")
            .map_err(persistence_error)?,
        hidden_ref_name: row.try_get("hidden_ref_name").map_err(persistence_error)?,
        git_object_id: row.try_get("git_object_id").map_err(persistence_error)?,
        index_tree_oid: required_string(&row, "index_tree_oid")?,
        worktree_tree_oid: required_string(&row, "worktree_tree_oid")?,
        head_oid: row.try_get("head_oid").map_err(persistence_error)?,
        head_ref: row.try_get("head_ref").map_err(persistence_error)?,
    })
}

fn required_string(row: &sqlx::sqlite::SqliteRow, column: &str) -> ChatResult<String> {
    row.try_get::<Option<String>, _>(column)
        .map_err(persistence_error)?
        .ok_or_else(|| super::checkpoint_error("Stored Chat checkpoint is incomplete"))
}

fn u64_column(row: &sqlx::sqlite::SqliteRow, column: &str) -> ChatResult<u64> {
    u64::try_from(row.try_get::<i64, _>(column).map_err(persistence_error)?)
        .map_err(super::checkpoint_error)
}

fn i64_value(value: u64) -> ChatResult<i64> {
    i64::try_from(value).map_err(|_| ChatError::validation("number", "Value is too large"))
}

fn parse_id(value: String) -> ChatResult<ChatCheckpointId> {
    ChatCheckpointId::new(value).map_err(super::checkpoint_error)
}

fn parse_thread_id(value: String) -> ChatResult<ChatThreadId> {
    ChatThreadId::new(value).map_err(super::checkpoint_error)
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat checkpoint persistence failed",
        true,
    )
}

fn serialization_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat checkpoint metadata could not be encoded",
        false,
    )
}
