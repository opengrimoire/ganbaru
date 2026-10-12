use super::events::{AppendCanonicalEventRequest, append_canonical_event};
use crate::events::{CanonicalEvent, CanonicalRuntimeEvent, TurnAbortedEvent};
use crate::models::{
    ChatError, ChatErrorCode, ChatEventId, ChatResult, ChatThreadId, ChatTurnId, ChatTurnState,
    ProviderFamilyId, ProviderInstanceId, UtcTimestamp,
};
use sqlx::{Row, SqlitePool};
use std::collections::HashSet;

pub async fn recover_orphaned_turns(
    pool: &SqlitePool,
    resumable_threads: &HashSet<ChatThreadId>,
    now: &UtcTimestamp,
) -> ChatResult<u64> {
    let rows = sqlx::query(
        "SELECT t.id AS turn_id, t.thread_id, th.provider_family_id,
                th.provider_instance_id, th.last_event_sequence
         FROM chat_turns t JOIN chat_threads th ON th.id = t.thread_id
         WHERE t.state IN ('dispatching', 'active', 'waiting_for_approval', 'waiting_for_user_input')
         ORDER BY t.thread_id, t.ordinal, t.id",
    )
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut recovered = 0;
    for row in rows {
        let thread_id = id::<ChatThreadId>(&row, "thread_id")?;
        if resumable_threads.contains(&thread_id) {
            continue;
        }
        let turn_id = id::<ChatTurnId>(&row, "turn_id")?;
        let last_sequence: i64 = row
            .try_get("last_event_sequence")
            .map_err(persistence_error)?;
        let event_id = ChatEventId::new(format!(
            "recovery:{}:{}:{}",
            thread_id.as_str(),
            turn_id.as_str(),
            last_sequence + 1
        ))
        .map_err(|_| corrupt_data_error())?;
        append_canonical_event(
            pool,
            AppendCanonicalEventRequest {
                runtime: CanonicalRuntimeEvent {
                    schema_version: 1,
                    event_id,
                    provider_family_id: id::<ProviderFamilyId>(&row, "provider_family_id")?,
                    provider_instance_id: id::<ProviderInstanceId>(&row, "provider_instance_id")?,
                    thread_id,
                    created_at: now.clone(),
                    turn_id: Some(turn_id),
                    provider_turn_id: None,
                    provider_item_id: None,
                    provider_request_id: None,
                    provider_task_id: None,
                    provider_reference: None,
                    event: CanonicalEvent::TurnAborted(TurnAbortedEvent {
                        state: ChatTurnState::Interrupted,
                        reason: "Application restarted before the provider settled the turn"
                            .to_string(),
                        recoverable: true,
                    }),
                    redacted_diagnostic: None,
                },
                ingested_at: now.clone(),
                diagnostic_expires_at: None,
            },
        )
        .await?;
        recovered += 1;
    }
    sqlx::query(
        "UPDATE chat_pending_requests
         SET resolution_state = 'interrupted', resolved_at = ?
         WHERE resolution_state = 'open' AND (
             turn_id IS NULL OR NOT EXISTS (
                 SELECT 1 FROM chat_turns
                 WHERE chat_turns.id = chat_pending_requests.turn_id
                   AND chat_turns.thread_id = chat_pending_requests.thread_id
                   AND chat_turns.invalidated_at IS NULL
                   AND chat_turns.state IN (
                       'dispatching', 'active', 'waiting_for_approval', 'waiting_for_user_input'
                   )
             )
         )",
    )
    .bind(now.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(recovered)
}

trait StoredIdentifier: Sized {
    fn parse(value: String) -> Result<Self, String>;
}
macro_rules! stored_identifier {
    ($type:ty) => {
        impl StoredIdentifier for $type {
            fn parse(value: String) -> Result<Self, String> {
                <$type>::new(value)
            }
        }
    };
}
stored_identifier!(ChatThreadId);
stored_identifier!(ChatTurnId);
stored_identifier!(ProviderFamilyId);
stored_identifier!(ProviderInstanceId);

fn id<T: StoredIdentifier>(row: &sqlx::sqlite::SqliteRow, column: &str) -> ChatResult<T> {
    T::parse(row.try_get(column).map_err(persistence_error)?).map_err(|_| corrupt_data_error())
}
fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat startup recovery persistence failed",
        true,
    )
}
fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat recovery data is invalid",
        false,
    )
}
