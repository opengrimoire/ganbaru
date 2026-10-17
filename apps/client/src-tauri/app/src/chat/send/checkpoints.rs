//! Best-effort pre-turn and post-turn checkpoint coordination.

use super::support::{corrupt_data_error, persistence_error};
use crate::chat::workspace::AuthorizedWorkingFolder;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ChatThreadId, ChatTurnId, RepositoryKind, UtcTimestamp,
};
use sqlx::SqlitePool;

pub(super) async fn ensure_pre_turn_checkpoint(
    pool: &SqlitePool,
    workspace: &AuthorizedWorkingFolder,
    thread_id: &ChatThreadId,
    turn_id: &ChatTurnId,
    now: &UtcTimestamp,
) {
    if workspace.repository_kind != RepositoryKind::Git {
        return;
    }
    let ordinal = match turn_ordinal(pool, thread_id, turn_id).await {
        Ok(value) => value,
        Err(error) => {
            record_checkpoint_failure(pool, thread_id, Some(turn_id), "pre_turn", &error, now)
                .await;
            return;
        }
    };
    let existing: Result<Option<String>, _> = sqlx::query_scalar(
        "SELECT id FROM chat_checkpoints
         WHERE thread_id = ? AND turn_count = ? AND status = 'available'
           AND invalidated_at IS NULL",
    )
    .bind(thread_id.as_str())
    .bind(i64::try_from(ordinal).unwrap_or(i64::MAX))
    .fetch_optional(pool)
    .await;
    if let Ok(Some(checkpoint_id)) = existing {
        let _ = sqlx::query(
            "UPDATE chat_turns SET pre_checkpoint_id = ? WHERE id = ? AND thread_id = ?",
        )
        .bind(&checkpoint_id)
        .bind(turn_id.as_str())
        .bind(thread_id.as_str())
        .execute(pool)
        .await;
        update_user_checkpoint_context(pool, turn_id, &checkpoint_id).await;
        return;
    }
    let kind = if ordinal == 0 {
        ganbaru_chat::checkpoints::CheckpointKind::Initial
    } else {
        ganbaru_chat::checkpoints::CheckpointKind::PreTurn
    };
    match ganbaru_chat::checkpoints::capture_and_store(
        pool,
        workspace,
        thread_id,
        Some(turn_id),
        ordinal,
        kind,
        now,
    )
    .await
    {
        Ok(checkpoint) => {
            update_user_checkpoint_context(pool, turn_id, checkpoint.id.as_str()).await
        }
        Err(error) => {
            let failure_kind = if ordinal == 0 { "initial" } else { "pre_turn" };
            record_checkpoint_failure(pool, thread_id, Some(turn_id), failure_kind, &error, now)
                .await;
        }
    }
}

pub(super) async fn ensure_post_turn_checkpoint(
    pool: &SqlitePool,
    workspace: &AuthorizedWorkingFolder,
    thread_id: &ChatThreadId,
    turn_id: &ChatTurnId,
    now: &UtcTimestamp,
) {
    if workspace.repository_kind != RepositoryKind::Git {
        return;
    }
    let ordinal = match turn_ordinal(pool, thread_id, turn_id).await {
        Ok(value) => value,
        Err(error) => {
            record_checkpoint_failure(pool, thread_id, Some(turn_id), "post_turn", &error, now)
                .await;
            return;
        }
    };
    if let Err(error) = ganbaru_chat::checkpoints::capture_and_store(
        pool,
        workspace,
        thread_id,
        Some(turn_id),
        ordinal.saturating_add(1),
        ganbaru_chat::checkpoints::CheckpointKind::PostTurn,
        now,
    )
    .await
    {
        if error.code != ChatErrorCode::Conflict {
            record_checkpoint_failure(pool, thread_id, Some(turn_id), "post_turn", &error, now)
                .await;
        }
    }
}

async fn turn_ordinal(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    turn_id: &ChatTurnId,
) -> ChatResult<u64> {
    let ordinal: i64 =
        sqlx::query_scalar("SELECT ordinal FROM chat_turns WHERE id = ? AND thread_id = ?")
            .bind(turn_id.as_str())
            .bind(thread_id.as_str())
            .fetch_optional(pool)
            .await
            .map_err(persistence_error)?
            .ok_or_else(|| {
                ChatError::new(ChatErrorCode::NotFound, "Chat turn was not found", true)
            })?;
    u64::try_from(ordinal).map_err(|_| corrupt_data_error())
}

async fn update_user_checkpoint_context(
    pool: &SqlitePool,
    turn_id: &ChatTurnId,
    checkpoint_id: &str,
) {
    let _ = sqlx::query(
        "UPDATE chat_messages
         SET content_metadata_data = json_set(content_metadata_data, '$.preCheckpointId', ?)
         WHERE turn_id = ? AND role = 'user'",
    )
    .bind(checkpoint_id)
    .bind(turn_id.as_str())
    .execute(pool)
    .await;
}

async fn record_checkpoint_failure(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    turn_id: Option<&ChatTurnId>,
    kind: &str,
    error: &ChatError,
    now: &UtcTimestamp,
) {
    let id = format!(
        "checkpoint-failure:{}:{}:{}",
        thread_id.as_str(),
        turn_id.map(ChatTurnId::as_str).unwrap_or("initial"),
        kind
    );
    let _ = sqlx::query(
        "INSERT OR REPLACE INTO chat_checkpoint_failures
            (id, thread_id, turn_id, checkpoint_kind, error_code, detail, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(thread_id.as_str())
    .bind(turn_id.map(ChatTurnId::as_str))
    .bind(kind)
    .bind(format!("{:?}", error.code).to_lowercase())
    .bind(&error.message)
    .bind(now.as_str())
    .execute(pool)
    .await;
}
