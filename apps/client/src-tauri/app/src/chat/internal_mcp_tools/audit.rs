//! Durable invocation budgets and provenance accounting for host tools.

use super::{generic_denial_error, persistence_error};
use crate::chat::internal_mcp::{
    InternalMcpChannelSource, InternalMcpRunScope, generate_opaque_handle,
};
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult};
use serde_json::Value;
use sqlx::SqlitePool;

const MAX_TOOL_CALLS: u32 = 20;
const MAX_SCOPE_RESPONSE_BYTES: usize = 1024 * 1024;

pub(super) async fn reserve_audit(
    pool: &SqlitePool,
    scope: &InternalMcpRunScope,
    tool_name: &str,
    request_hash: &str,
    response_reservation: usize,
) -> ChatResult<String> {
    let id = generate_opaque_handle("host-tool")?;
    let created_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let inserted = sqlx::query(
        "INSERT INTO chat_host_tool_invocations
            (id, authorization_revision_id, tool_name, request_hash,
             decision_state, denial_category, response_bytes, truncated, created_at)
         SELECT ?, ?, ?, ?, 'denied', 'pending', ?, 0, ?
         WHERE EXISTS (
             SELECT 1 FROM chat_assignment_authorization_revisions authorization
             WHERE authorization.id = ?
               AND authorization.decision_state = 'allowed'
               AND authorization.revoked_at IS NULL
         )
           AND (
             SELECT count(*) FROM chat_host_tool_invocations invocation
             WHERE invocation.authorization_revision_id = ?
           ) < ?
           AND ? + coalesce((
             SELECT sum(invocation.response_bytes)
             FROM chat_host_tool_invocations invocation
             WHERE invocation.authorization_revision_id = ?
           ), 0) <= ?",
    )
    .bind(&id)
    .bind(scope.authorization_revision_id.as_str())
    .bind(tool_name)
    .bind(request_hash)
    .bind(i64::try_from(response_reservation).map_err(|_| generic_denial_error())?)
    .bind(created_at)
    .bind(scope.authorization_revision_id.as_str())
    .bind(scope.authorization_revision_id.as_str())
    .bind(i64::from(MAX_TOOL_CALLS))
    .bind(i64::try_from(response_reservation).map_err(|_| generic_denial_error())?)
    .bind(scope.authorization_revision_id.as_str())
    .bind(i64::try_from(MAX_SCOPE_RESPONSE_BYTES).map_err(|_| generic_denial_error())?)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    if inserted.rows_affected() == 1 {
        Ok(id)
    } else {
        Err(generic_denial_error())
    }
}

pub(super) async fn complete_audit_allowed(
    pool: &SqlitePool,
    scope: &InternalMcpRunScope,
    invocation_id: &str,
    response_bytes: usize,
    truncated: bool,
    returned_revisions: &[(String, String)],
    queried_channel_sources: &[&InternalMcpChannelSource],
) -> ChatResult<()> {
    let created_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let response_bytes = i64::try_from(response_bytes).map_err(|_| generic_denial_error())?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let completed = sqlx::query(
        "UPDATE chat_host_tool_invocations
         SET decision_state = 'allowed', denial_category = NULL,
             response_bytes = ?, truncated = ?
         WHERE id = ? AND authorization_revision_id = ?
           AND decision_state = 'denied' AND denial_category = 'pending'
           AND EXISTS (
             SELECT 1 FROM chat_assignment_authorization_revisions authorization
             WHERE authorization.id = ?
               AND authorization.decision_state = 'allowed'
               AND authorization.revoked_at IS NULL
           )
           AND ? + coalesce((
             SELECT sum(other.response_bytes)
             FROM chat_host_tool_invocations other
             WHERE other.authorization_revision_id = ? AND other.id != ?
           ), 0) <= ?",
    )
    .bind(response_bytes)
    .bind(truncated)
    .bind(invocation_id)
    .bind(scope.authorization_revision_id.as_str())
    .bind(scope.authorization_revision_id.as_str())
    .bind(response_bytes)
    .bind(scope.authorization_revision_id.as_str())
    .bind(invocation_id)
    .bind(i64::try_from(MAX_SCOPE_RESPONSE_BYTES).map_err(|_| generic_denial_error())?)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if completed.rows_affected() != 1 {
        sqlx::query(
            "UPDATE chat_host_tool_invocations
             SET denial_category = 'budget_exhausted'
             WHERE id = ? AND authorization_revision_id = ?
               AND decision_state = 'denied' AND denial_category = 'pending'",
        )
        .bind(invocation_id)
        .bind(scope.authorization_revision_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        transaction.commit().await.map_err(persistence_error)?;
        return Err(generic_denial_error());
    }
    for (ordinal, (revision_id, content_sha256)) in returned_revisions.iter().enumerate() {
        sqlx::query(
            "INSERT INTO chat_host_tool_returned_message_revisions
                (invocation_id, message_revision_id, content_sha256, ordinal)
             VALUES (?, ?, ?, ?)",
        )
        .bind(invocation_id)
        .bind(revision_id)
        .bind(content_sha256)
        .bind(i64::try_from(ordinal).map_err(|_| generic_denial_error())?)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    if let Some(scratch_generation_id) = scope.scratch_generation_id.as_deref() {
        for source in queried_channel_sources {
            let recorded = sqlx::query(
                "INSERT INTO chat_scratch_generation_sources
                (scratch_generation_id, conversation_id, lower_ordinal,
                 high_ordinal, audience_revision, created_at)
             SELECT ?, ?, ?, ?, audience.revision, ?
             FROM chat_scratch_generations generation
             JOIN chat_conversation_audience_state audience
               ON audience.conversation_id = ?
             WHERE generation.id = ?
               AND generation.lifecycle_state = 'active'
               AND generation.removed_at IS NULL
             ON CONFLICT(scratch_generation_id, conversation_id) DO UPDATE SET
               lower_ordinal = min(lower_ordinal, excluded.lower_ordinal),
               high_ordinal = max(high_ordinal, excluded.high_ordinal),
               audience_revision = excluded.audience_revision",
            )
            .bind(scratch_generation_id)
            .bind(&source.conversation_id)
            .bind(i64::try_from(source.lower_ordinal).map_err(|_| generic_denial_error())?)
            .bind(i64::try_from(source.high_ordinal).map_err(|_| generic_denial_error())?)
            .bind(&created_at)
            .bind(scope.destination_conversation_id.as_str())
            .bind(scratch_generation_id)
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
            if recorded.rows_affected() != 1 {
                return Err(generic_denial_error());
            }
        }
    }
    transaction.commit().await.map_err(persistence_error)
}

pub(super) async fn complete_audit_denied(
    pool: &SqlitePool,
    scope: &InternalMcpRunScope,
    invocation_id: &str,
    denial_category: &str,
    preserve_response_reservation: bool,
) -> ChatResult<()> {
    let updated = sqlx::query(
        "UPDATE chat_host_tool_invocations
         SET denial_category = ?,
             response_bytes = CASE WHEN ? THEN response_bytes ELSE 0 END
         WHERE id = ? AND authorization_revision_id = ?
           AND decision_state = 'denied' AND denial_category = 'pending'",
    )
    .bind(denial_category)
    .bind(preserve_response_reservation)
    .bind(invocation_id)
    .bind(scope.authorization_revision_id.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() == 1 {
        Ok(())
    } else {
        Err(generic_denial_error())
    }
}

pub(super) fn returned_message_revisions(value: &Value) -> ChatResult<Vec<(String, String)>> {
    let Some(messages) = value.get("messages") else {
        return Ok(Vec::new());
    };
    messages
        .as_array()
        .ok_or_else(generic_denial_error)?
        .iter()
        .map(|message| {
            let revision_id = message
                .get("revisionId")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty() && value.len() <= 1024)
                .ok_or_else(generic_denial_error)?;
            let content_hash = message
                .get("contentHash")
                .and_then(Value::as_str)
                .filter(|value| value.len() == 64)
                .ok_or_else(generic_denial_error)?;
            Ok((revision_id.to_string(), content_hash.to_string()))
        })
        .collect()
}

pub(super) fn denial_category(error: &ChatError) -> &'static str {
    match error.code {
        ChatErrorCode::Conflict | ChatErrorCode::StaleRevision => "stale_scope",
        ChatErrorCode::Validation => "invalid_request",
        ChatErrorCode::NotFound => "unavailable_resource",
        ChatErrorCode::Permission => "permission_denied",
        _ => "runtime_unavailable",
    }
}
