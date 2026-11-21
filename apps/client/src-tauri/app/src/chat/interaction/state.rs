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
    let snapshot = app.state::<ChatRuntimeRegistry>().snapshot(&thread_id)?;
    let session_state = snapshot.session_state();
    let active_turn_id = snapshot
        .active_turn_id()
        .map(|value| value.as_str().to_string());
    Ok(ChatInteractionStateRead {
        session_id: snapshot.session_id.map(|value| value.into_inner()),
        session_state,
        active_turn_id,
        capabilities: snapshot.capabilities,
        pending_requests: read_pending_requests(&pool, &thread_id).await?,
        queued_followup: read_queued_followup(&pool, &thread_id).await?,
        usage: read_latest_usage(&pool, &thread_id).await?,
        account_status: read_latest_account_status(&pool, &thread_id).await?,
        rate_limit_status: read_latest_rate_limit_status(&pool, &thread_id).await?,
        automatic_compaction_reported: automatic_compaction_reported(&pool, &thread_id).await?,
    })
}

/// Reads every open request of `thread_id`, oldest first, so a provider that keeps several
/// requests open at once shows each of them.
async fn read_pending_requests(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<Vec<ChatPendingRequestRead>> {
    let rows = sqlx::query(
        "SELECT id, turn_id, provider_request_id, request_kind,
                safe_display_schema_version, safe_display_data,
                allowed_decisions_schema_version, allowed_decisions_data, opened_at
         FROM chat_pending_requests
         WHERE thread_id = ? AND resolution_state = 'open'
         ORDER BY opened_sequence ASC, id ASC",
    )
    .bind(thread_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut requests = Vec::with_capacity(rows.len());
    for row in rows {
        requests.push(pending_request_from_row(pool, thread_id, &row).await?);
    }
    Ok(requests)
}

/// Builds the read of one open request row, with the rule an approval may offer to remember.
async fn pending_request_from_row(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    row: &sqlx::sqlite::SqliteRow,
) -> ChatResult<ChatPendingRequestRead> {
    let request_kind: String = row.try_get("request_kind").map_err(persistence_error)?;
    let rememberable = if request_kind == "approval" {
        crate::chat::send::offered_rule(
            pool,
            thread_id,
            &row.try_get::<String, _>("safe_display_data")
                .map_err(persistence_error)?,
            &row.try_get::<String, _>("allowed_decisions_data")
                .map_err(persistence_error)?,
        )
        .await?
    } else {
        None
    };
    Ok(ChatPendingRequestRead {
        id: row.try_get("id").map_err(persistence_error)?,
        turn_id: row.try_get("turn_id").map_err(persistence_error)?,
        provider_request_id: row
            .try_get("provider_request_id")
            .map_err(persistence_error)?,
        request_kind,
        safe_display: versioned_row(row, "safe_display_schema_version", "safe_display_data")?,
        allowed_decisions: versioned_row(
            row,
            "allowed_decisions_schema_version",
            "allowed_decisions_data",
        )?,
        opened_at: UtcTimestamp::new(
            row.try_get::<String, _>("opened_at")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        rememberable,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::tests::repository::pool_with_thread;

    #[test]
    fn every_open_request_is_read_oldest_first_and_settled_ones_are_left_out() {
        tauri::async_runtime::block_on(async {
            let pool = pool_with_thread().await;
            for (id, kind, sequence, state) in [
                ("request-b", "user_input", 2, "open"),
                ("request-a", "approval", 1, "open"),
                ("request-c", "approval", 3, "open"),
                ("request-settled", "approval", 4, "resolved"),
            ] {
                sqlx::query(
                    "INSERT INTO chat_pending_requests
                        (id, thread_id, provider_request_id, request_kind, safe_display_data,
                         allowed_decisions_data, opened_sequence, resolution_state, resolved_at,
                         opened_at)
                     VALUES (?, 'thread-1', ?, ?, '{}', '[]', ?, ?,
                             CASE WHEN ? = 'open' THEN NULL ELSE '2026-11-20T12:05:00.000Z' END,
                             '2026-11-20T12:00:00.000Z')",
                )
                .bind(id)
                .bind(format!("provider-{id}"))
                .bind(kind)
                .bind(sequence)
                .bind(state)
                .bind(state)
                .execute(&pool)
                .await
                .unwrap();
            }

            let thread = ChatThreadId::new("thread-1").unwrap();
            let requests = read_pending_requests(&pool, &thread).await.unwrap();

            assert_eq!(
                requests
                    .iter()
                    .map(|request| (request.id.as_str(), request.provider_request_id.as_str()))
                    .collect::<Vec<_>>(),
                [
                    ("request-a", "provider-request-a"),
                    ("request-b", "provider-request-b"),
                    ("request-c", "provider-request-c"),
                ]
            );
            assert_eq!(requests[1].request_kind, "user_input");
            let other = ChatThreadId::new("thread-other").unwrap();
            assert!(
                read_pending_requests(&pool, &other)
                    .await
                    .unwrap()
                    .is_empty()
            );
        });
    }
}
