use super::events::{apply_projection, read_canonical_events};
use crate::models::{ChatError, ChatErrorCode, ChatResult, ChatThreadId};
use sqlx::SqlitePool;

pub async fn rebuild_thread_projections(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<u64> {
    let events = read_canonical_events(pool, thread_id, 0).await?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let active_turns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chat_turns WHERE thread_id = ?
         AND invalidated_at IS NULL
         AND state IN ('dispatching', 'active', 'waiting_for_approval', 'waiting_for_user_input')",
    )
    .bind(thread_id.as_str())
    .fetch_one(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if active_turns != 0 {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Chat projections cannot be rebuilt during an active turn",
            true,
        ));
    }
    let message_attachment_references: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT r.id, r.attachment_id, r.message_id, r.created_at
         FROM chat_attachment_references r
         JOIN chat_messages m ON m.id = r.message_id
         WHERE m.thread_id = ?",
    )
    .bind(thread_id.as_str())
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    for table in ["chat_pending_requests", "chat_activities", "chat_plans"] {
        let statement = format!("DELETE FROM {table} WHERE thread_id = ?");
        sqlx::query(&statement)
            .bind(thread_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
    }
    sqlx::query("DELETE FROM chat_messages WHERE thread_id = ? AND role != 'user'")
        .bind(thread_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_threads SET message_count = (
                    SELECT COUNT(*) FROM chat_messages
                    WHERE thread_id = chat_threads.id AND (
                        turn_id IS NULL OR EXISTS (
                            SELECT 1 FROM chat_turns
                            WHERE id = chat_messages.turn_id AND invalidated_at IS NULL
                        )
                    )
                ), latest_preview = (
                    SELECT substr(normalized_markdown, -2000) FROM chat_messages
                    WHERE thread_id = chat_threads.id AND (
                        turn_id IS NULL OR EXISTS (
                            SELECT 1 FROM chat_turns
                            WHERE id = chat_messages.turn_id AND invalidated_at IS NULL
                        )
                    )
                    ORDER BY sequence_anchor DESC, id DESC LIMIT 1
                ),
                latest_turn_state = NULL, last_projected_sequence = 0
         WHERE id = ?",
    )
    .bind(thread_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    for event in &events {
        apply_projection(&mut transaction, event.sequence, &event.runtime).await?;
    }
    for (id, attachment_id, message_id, created_at) in message_attachment_references {
        sqlx::query(
            "INSERT OR IGNORE INTO chat_attachment_references
                (id, attachment_id, message_id, created_at)
             SELECT ?, ?, ?, ? WHERE EXISTS (
                SELECT 1 FROM chat_messages WHERE id = ? AND thread_id = ?
             )",
        )
        .bind(id)
        .bind(attachment_id)
        .bind(&message_id)
        .bind(created_at)
        .bind(&message_id)
        .bind(thread_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    let sequence = events.last().map(|event| event.sequence).unwrap_or(0);
    sqlx::query("UPDATE chat_threads SET last_projected_sequence = ? WHERE id = ?")
        .bind(i64::try_from(sequence).map_err(|_| corrupt_data_error())?)
        .bind(thread_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(sequence)
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat projection rebuild failed",
        true,
    )
}

fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat event sequence is invalid",
        false,
    )
}
