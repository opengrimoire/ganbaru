//! Aggregate interaction-state reads and event projections.

use super::followups::read_queued_followup;
use super::support::{chat_pool, corrupt_data_error, json_error, persistence_error, versioned_row};
use crate::chat::interaction_commands::{ChatInteractionStateRead, ChatPendingRequestRead};
use ganbaru_chat::runtime::ChatRuntimeRegistry;
use ganbaru_chat_contracts::events::{
    AccountStatusEvent, CanonicalEvent, RateLimitStatusEvent, ThreadUsageUpdatedEvent,
};
use ganbaru_chat_contracts::models::{ChatResult, ChatThreadId, UtcTimestamp};
use sqlx::{Row, SqlitePool};
use tauri::Manager;

pub(crate) async fn read_interaction_state(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
) -> ChatResult<ChatInteractionStateRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let owner = app
        .state::<ChatRuntimeRegistry>()
        .owner(thread_id.clone())?;
    let snapshot = owner.snapshot()?;
    Ok(ChatInteractionStateRead {
        session_id: snapshot.session_id.map(|value| value.into_inner()),
        session_state: snapshot.session_state,
        active_turn_id: snapshot.active_turn_id.map(|value| value.into_inner()),
        capabilities: snapshot.capabilities,
        pending_request: read_pending_request(&pool, &thread_id).await?,
        queued_followup: read_queued_followup(&pool, &thread_id).await?,
        usage: read_latest_usage(&pool, &thread_id).await?,
        account_status: read_latest_account_status(&pool, &thread_id).await?,
        rate_limit_status: read_latest_rate_limit_status(&pool, &thread_id).await?,
        automatic_compaction_reported: automatic_compaction_reported(&pool, &thread_id).await?,
    })
}

async fn read_pending_request(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<Option<ChatPendingRequestRead>> {
    let row = sqlx::query(
        "SELECT id, turn_id, provider_request_id, request_kind,
                safe_display_schema_version, safe_display_data,
                allowed_decisions_schema_version, allowed_decisions_data, opened_at
         FROM chat_pending_requests
         WHERE thread_id = ? AND resolution_state = 'open'
         ORDER BY opened_sequence DESC, id DESC LIMIT 1",
    )
    .bind(thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    row.map(|row| {
        Ok(ChatPendingRequestRead {
            id: row.try_get("id").map_err(persistence_error)?,
            turn_id: row.try_get("turn_id").map_err(persistence_error)?,
            provider_request_id: row
                .try_get("provider_request_id")
                .map_err(persistence_error)?,
            request_kind: row.try_get("request_kind").map_err(persistence_error)?,
            safe_display: versioned_row(&row, "safe_display_schema_version", "safe_display_data")?,
            allowed_decisions: versioned_row(
                &row,
                "allowed_decisions_schema_version",
                "allowed_decisions_data",
            )?,
            opened_at: UtcTimestamp::new(
                row.try_get::<String, _>("opened_at")
                    .map_err(persistence_error)?,
            )
            .map_err(|_| corrupt_data_error())?,
        })
    })
    .transpose()
}

async fn read_latest_event(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    event_type: &str,
) -> ChatResult<Option<CanonicalEvent>> {
    let payload: Option<String> = sqlx::query_scalar(
        "SELECT payload_data FROM chat_events
         WHERE thread_id = ? AND event_type = ?
         ORDER BY sequence DESC LIMIT 1",
    )
    .bind(thread_id.as_str())
    .bind(event_type)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    payload
        .map(|value| serde_json::from_str(&value).map_err(json_error))
        .transpose()
}

async fn read_latest_usage(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<Option<ThreadUsageUpdatedEvent>> {
    match read_latest_event(pool, thread_id, "thread_usage_updated").await? {
        Some(CanonicalEvent::ThreadUsageUpdated(value)) => Ok(Some(value)),
        None => Ok(None),
        _ => Err(corrupt_data_error()),
    }
}

async fn read_latest_account_status(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<Option<AccountStatusEvent>> {
    match read_latest_event(pool, thread_id, "account_status").await? {
        Some(CanonicalEvent::AccountStatus(value)) => Ok(Some(value)),
        None => Ok(None),
        _ => Err(corrupt_data_error()),
    }
}

async fn read_latest_rate_limit_status(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<Option<RateLimitStatusEvent>> {
    match read_latest_event(pool, thread_id, "rate_limit_status").await? {
        Some(CanonicalEvent::RateLimitStatus(value)) => Ok(Some(value)),
        None => Ok(None),
        _ => Err(corrupt_data_error()),
    }
}

async fn automatic_compaction_reported(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM chat_events
            WHERE thread_id = ?
              AND event_type IN ('item_started', 'item_updated', 'item_completed')
              AND json_extract(payload_data, '$.payload.kind') = 'context_compaction'
         )",
    )
    .bind(thread_id.as_str())
    .fetch_one(pool)
    .await
    .map_err(persistence_error)
}
