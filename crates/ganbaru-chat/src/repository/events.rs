use crate::events::{
    CANONICAL_EVENT_SCHEMA_VERSION, CanonicalEvent, CanonicalRuntimeEvent, CanonicalStoredEvent,
    ChangedFileSummary,
};
use crate::models::{
    ActivityStatus, ChatChangeNotification, ChatError, ChatErrorCode, ChatResult, ChatThreadId,
    UtcTimestamp, VersionedJson,
};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

mod codec;
mod projections;

use codec::*;
pub(super) use projections::apply_projection;

#[derive(Clone, Debug)]
pub struct AppendCanonicalEventRequest {
    pub runtime: CanonicalRuntimeEvent,
    pub ingested_at: UtcTimestamp,
    pub diagnostic_expires_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppendCanonicalEventResult {
    pub event: CanonicalStoredEvent,
    pub notification: ChatChangeNotification,
}

pub async fn append_canonical_event(
    pool: &SqlitePool,
    request: AppendCanonicalEventRequest,
) -> ChatResult<AppendCanonicalEventResult> {
    if request.runtime.schema_version != CANONICAL_EVENT_SCHEMA_VERSION {
        return Err(ChatError::validation(
            "event.schemaVersion",
            "Canonical event schema is unsupported",
        ));
    }
    if request.runtime.redacted_diagnostic.is_some() != request.diagnostic_expires_at.is_some() {
        return Err(ChatError::validation(
            "event.redactedDiagnostic",
            "Redacted diagnostics require an explicit retention deadline",
        ));
    }
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let thread = sqlx::query("SELECT revision, last_event_sequence FROM chat_threads WHERE id = ?")
        .bind(request.runtime.thread_id.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(persistence_error)?
        .ok_or_else(|| {
            ChatError::new(ChatErrorCode::NotFound, "Chat thread was not found", true)
        })?;
    let revision = u64_column(&thread, "revision")?;
    let sequence = u64_column(&thread, "last_event_sequence")?
        .checked_add(1)
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Internal,
                "Chat event sequence overflow",
                false,
            )
        })?;
    let payload = serde_json::to_string(&request.runtime.event).map_err(serialization_error)?;
    let provider_reference = versioned_parts(request.runtime.provider_reference.as_ref())?;
    let diagnostic = versioned_parts(request.runtime.redacted_diagnostic.as_ref())?;

    sqlx::query(
        "INSERT INTO chat_events
            (id, thread_id, sequence, event_schema_version, turn_id, provider_turn_id,
             provider_item_id, provider_request_id, provider_task_id, provider_family_id,
             provider_instance_id, event_type, payload_schema_version, payload_data,
             provider_reference_schema_version, provider_reference_data, created_at,
             ingested_at, redacted_diagnostic_schema_version, redacted_diagnostic_data,
             diagnostic_expires_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(request.runtime.event_id.as_str())
    .bind(request.runtime.thread_id.as_str())
    .bind(i64_value(sequence)?)
    .bind(i64::from(request.runtime.schema_version))
    .bind(request.runtime.turn_id.as_ref().map(|value| value.as_str()))
    .bind(
        request
            .runtime
            .provider_turn_id
            .as_ref()
            .map(|value| value.as_str()),
    )
    .bind(
        request
            .runtime
            .provider_item_id
            .as_ref()
            .map(|value| value.as_str()),
    )
    .bind(
        request
            .runtime
            .provider_request_id
            .as_ref()
            .map(|value| value.as_str()),
    )
    .bind(
        request
            .runtime
            .provider_task_id
            .as_ref()
            .map(|value| value.as_str()),
    )
    .bind(request.runtime.provider_family_id.as_str())
    .bind(request.runtime.provider_instance_id.as_str())
    .bind(event_type(&request.runtime.event)?)
    .bind(i64::from(request.runtime.schema_version))
    .bind(payload)
    .bind(provider_reference.0)
    .bind(provider_reference.1)
    .bind(request.runtime.created_at.as_str())
    .bind(request.ingested_at.as_str())
    .bind(diagnostic.0)
    .bind(diagnostic.1)
    .bind(
        request
            .diagnostic_expires_at
            .as_ref()
            .map(UtcTimestamp::as_str),
    )
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;

    let mut changed_projection_keys =
        apply_projection(&mut transaction, sequence, &request.runtime).await?;
    let next_revision = revision.checked_add(1).ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::Internal,
            "Chat thread revision overflow",
            false,
        )
    })?;
    let updated = sqlx::query(
        "UPDATE chat_threads
         SET last_event_sequence = ?, last_projected_sequence = ?, revision = ?,
             last_activity_at = ?, updated_at = ?
         WHERE id = ? AND revision = ? AND last_event_sequence = ?",
    )
    .bind(i64_value(sequence)?)
    .bind(i64_value(sequence)?)
    .bind(i64_value(next_revision)?)
    .bind(request.runtime.created_at.as_str())
    .bind(request.ingested_at.as_str())
    .bind(request.runtime.thread_id.as_str())
    .bind(i64_value(revision)?)
    .bind(i64_value(sequence - 1)?)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "Chat thread changed while the event was being appended",
            true,
        ));
    }
    if crate::coordination::projection::project_provider_event_in_transaction(
        &mut transaction,
        &request.runtime,
    )
    .await?
    {
        changed_projection_keys.push("organizational".to_string());
    }
    transaction.commit().await.map_err(persistence_error)?;
    let notification_thread_id = request.runtime.thread_id.clone();

    Ok(AppendCanonicalEventResult {
        event: CanonicalStoredEvent {
            sequence,
            ingested_at: request.ingested_at,
            runtime: request.runtime,
        },
        notification: ChatChangeNotification {
            thread_id: notification_thread_id,
            sequence,
            revision: next_revision,
            changed_projection_keys,
        },
    })
}

pub async fn read_canonical_events(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    after_sequence: u64,
) -> ChatResult<Vec<CanonicalStoredEvent>> {
    let rows = sqlx::query(
        "SELECT id, thread_id, sequence, event_schema_version, turn_id, provider_turn_id,
                provider_item_id, provider_request_id, provider_task_id,
                provider_family_id, provider_instance_id, payload_data,
                provider_reference_schema_version, provider_reference_data,
                created_at, ingested_at, redacted_diagnostic_schema_version,
                redacted_diagnostic_data
         FROM chat_events
         WHERE thread_id = ? AND sequence > ? AND invalidated_at IS NULL
         ORDER BY sequence ASC, id ASC",
    )
    .bind(thread_id.as_str())
    .bind(i64_value(after_sequence)?)
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    rows.into_iter().map(row_to_event).collect()
}
