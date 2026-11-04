//! Per-turn budget and audit rows for model preview tools in direct sessions.
//!
//! Preview tools act on pages the user may be signed into, so every admitted call leaves an
//! app-owned row with the tool, a hash of its arguments, its outcome, and its result size,
//! independent of what the provider reports. A per-turn call and byte budget stops a looping
//! model from flooding its context; the next turn starts a new budget.

use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult, ChatTurnId};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

/// Preview tool calls one turn may make. An iterative UI check uses a handful per step
/// (navigate, wait, snapshot, click, snapshot), so this leaves room for long test loops.
pub(super) const MAX_PREVIEW_TOOL_CALLS_PER_TURN: u32 = 300;
/// Largest encoded result of one preview tool call: twice the 2 MiB browser result bound,
/// which leaves room for JSON escaping of a full snapshot.
pub(super) const MAX_PREVIEW_TOOL_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
/// Result bytes one turn may receive from preview tools. Each admitted call reserves
/// `MAX_PREVIEW_TOOL_RESPONSE_BYTES` until it settles to its actual size.
pub(super) const MAX_PREVIEW_TOOL_BYTES_PER_TURN: usize = 64 * 1024 * 1024;

/// How an admitted preview tool call ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PreviewToolOutcome {
    Succeeded,
    Failed,
    /// The tool ran but its result exceeded `MAX_PREVIEW_TOOL_RESPONSE_BYTES`.
    Refused,
}

impl PreviewToolOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Refused => "refused",
        }
    }
}

/// An admitted call whose row is pending until `complete` settles it.
#[derive(Debug)]
pub(super) struct PreviewToolInvocation {
    id: String,
}

/// Admits one preview tool call against the turn's budget and records it as pending. The
/// budget check and the insert are one statement, so concurrent calls cannot overrun it.
pub(super) async fn reserve(
    pool: &SqlitePool,
    turn_id: &ChatTurnId,
    tool_name: &str,
    arguments: &Map<String, Value>,
) -> ChatResult<PreviewToolInvocation> {
    let id = super::generate_opaque_handle("preview-tool")?;
    let request_hash = request_hash(tool_name, arguments)?;
    let created_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let reservation = byte_count(MAX_PREVIEW_TOOL_RESPONSE_BYTES)?;
    let inserted = sqlx::query(
        "INSERT INTO chat_preview_tool_invocations
            (id, turn_id, tool_name, request_hash, outcome, response_bytes, created_at)
         SELECT ?, ?, ?, ?, 'pending', ?, ?
         WHERE (
             SELECT count(*) FROM chat_preview_tool_invocations invocation
             WHERE invocation.turn_id = ?
           ) < ?
           AND ? + coalesce((
             SELECT sum(invocation.response_bytes) FROM chat_preview_tool_invocations invocation
             WHERE invocation.turn_id = ?
           ), 0) <= ?",
    )
    .bind(&id)
    .bind(turn_id.as_str())
    .bind(tool_name)
    .bind(&request_hash)
    .bind(reservation)
    .bind(&created_at)
    .bind(turn_id.as_str())
    .bind(i64::from(MAX_PREVIEW_TOOL_CALLS_PER_TURN))
    .bind(reservation)
    .bind(turn_id.as_str())
    .bind(byte_count(MAX_PREVIEW_TOOL_BYTES_PER_TURN)?)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    if inserted.rows_affected() == 1 {
        Ok(PreviewToolInvocation { id })
    } else {
        Err(budget_exhausted_error())
    }
}

/// Settles a pending call with its outcome and actual result size, releasing the rest of its
/// byte reservation.
pub(super) async fn complete(
    pool: &SqlitePool,
    invocation: &PreviewToolInvocation,
    outcome: PreviewToolOutcome,
    response_bytes: usize,
) -> ChatResult<()> {
    let updated = sqlx::query(
        "UPDATE chat_preview_tool_invocations SET outcome = ?, response_bytes = ?
         WHERE id = ? AND outcome = 'pending'",
    )
    .bind(outcome.as_str())
    .bind(byte_count(response_bytes)?)
    .bind(&invocation.id)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() == 1 {
        Ok(())
    } else {
        Err(persistence_error(()))
    }
}

/// Classifies a finished call and refuses results over `MAX_PREVIEW_TOOL_RESPONSE_BYTES`.
pub(super) fn settle(result: ChatResult<Value>) -> (PreviewToolOutcome, usize, ChatResult<Value>) {
    let value = match result {
        Ok(value) => value,
        Err(error) => return (PreviewToolOutcome::Failed, 0, Err(error)),
    };
    let encoded_bytes = match serde_json::to_vec(&value) {
        Ok(encoded) => encoded.len(),
        Err(_) => {
            return (
                PreviewToolOutcome::Failed,
                0,
                Err(ChatError::new(
                    ChatErrorCode::Internal,
                    "The preview tool result could not be encoded",
                    false,
                )),
            );
        }
    };
    if encoded_bytes > MAX_PREVIEW_TOOL_RESPONSE_BYTES {
        return (
            PreviewToolOutcome::Refused,
            0,
            Err(ChatError::new(
                ChatErrorCode::Validation,
                format!(
                    "The preview tool result exceeds {} MiB. Narrow the request, for example with a smaller page or viewport",
                    MAX_PREVIEW_TOOL_RESPONSE_BYTES / (1024 * 1024)
                ),
                false,
            )),
        );
    }
    (PreviewToolOutcome::Succeeded, encoded_bytes, Ok(value))
}

fn budget_exhausted_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Busy,
        format!(
            "This turn reached its preview tool budget of {MAX_PREVIEW_TOOL_CALLS_PER_TURN} calls or {} MiB of results. The user can continue in a new message",
            MAX_PREVIEW_TOOL_BYTES_PER_TURN / (1024 * 1024)
        ),
        false,
    )
}

fn request_hash(tool_name: &str, arguments: &Map<String, Value>) -> ChatResult<String> {
    let encoded = serde_json::to_vec(&(tool_name, arguments)).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Internal,
            "The preview tool request could not be encoded",
            false,
        )
    })?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

fn byte_count(bytes: usize) -> ChatResult<i64> {
    i64::try_from(bytes).map_err(|_| persistence_error(()))
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "The preview tool call could not be recorded",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-10-20T12:00:00.000Z";

    async fn pool_with_turn(turn_id: &str) -> SqlitePool {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        sqlx::query(
            "INSERT INTO chat_turns
                (id, thread_id, ordinal, state, safety_mode, interaction_mode, created_at, updated_at)
             VALUES (?, 'thread-1', 0, 'active', 'ask_for_approval', 'build', ?, ?)",
        )
        .bind(turn_id)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn rows(pool: &SqlitePool) -> Vec<(String, String, i64)> {
        sqlx::query_as(
            "SELECT tool_name, outcome, response_bytes FROM chat_preview_tool_invocations
             ORDER BY created_at, id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[test]
    fn preview_tool_calls_are_recorded_and_stop_at_the_turn_call_budget() {
        tauri::async_runtime::block_on(async {
            let pool = pool_with_turn("turn-1").await;
            let turn_id = ChatTurnId::new("turn-1").unwrap();
            let arguments = Map::new();
            let invocation = reserve(&pool, &turn_id, "preview_snapshot", &arguments)
                .await
                .unwrap();
            assert_eq!(
                rows(&pool).await,
                vec![(
                    "preview_snapshot".to_string(),
                    "pending".to_string(),
                    byte_count(MAX_PREVIEW_TOOL_RESPONSE_BYTES).unwrap()
                )]
            );
            complete(&pool, &invocation, PreviewToolOutcome::Succeeded, 42)
                .await
                .unwrap();
            assert_eq!(
                rows(&pool).await,
                vec![("preview_snapshot".to_string(), "succeeded".to_string(), 42)]
            );
            assert!(
                complete(&pool, &invocation, PreviewToolOutcome::Failed, 0)
                    .await
                    .is_err(),
                "a settled call cannot be rewritten"
            );

            for _ in 1..MAX_PREVIEW_TOOL_CALLS_PER_TURN {
                let invocation = reserve(&pool, &turn_id, "preview_status", &arguments)
                    .await
                    .unwrap();
                complete(&pool, &invocation, PreviewToolOutcome::Succeeded, 1)
                    .await
                    .unwrap();
            }
            let refused = reserve(&pool, &turn_id, "preview_status", &arguments)
                .await
                .unwrap_err();
            assert_eq!(refused.code, ChatErrorCode::Busy);
            assert_eq!(
                rows(&pool).await.len(),
                usize::try_from(MAX_PREVIEW_TOOL_CALLS_PER_TURN).unwrap()
            );
        });
    }

    #[test]
    fn preview_tool_byte_reservations_stop_at_the_turn_byte_budget() {
        tauri::async_runtime::block_on(async {
            let pool = pool_with_turn("turn-1").await;
            let turn_id = ChatTurnId::new("turn-1").unwrap();
            let arguments = Map::new();
            let in_flight = MAX_PREVIEW_TOOL_BYTES_PER_TURN / MAX_PREVIEW_TOOL_RESPONSE_BYTES;
            let mut pending = Vec::new();
            for _ in 0..in_flight {
                pending.push(
                    reserve(&pool, &turn_id, "preview_snapshot", &arguments)
                        .await
                        .unwrap(),
                );
            }
            let refused = reserve(&pool, &turn_id, "preview_snapshot", &arguments)
                .await
                .unwrap_err();
            assert_eq!(refused.code, ChatErrorCode::Busy);

            complete(&pool, &pending[0], PreviewToolOutcome::Failed, 0)
                .await
                .unwrap();
            reserve(&pool, &turn_id, "preview_snapshot", &arguments)
                .await
                .expect("a settled reservation frees its unused bytes");
        });
    }

    #[test]
    fn a_new_turn_starts_with_a_fresh_budget_and_deleting_a_turn_removes_its_rows() {
        tauri::async_runtime::block_on(async {
            let pool = pool_with_turn("turn-1").await;
            sqlx::query(
                "INSERT INTO chat_turns
                    (id, thread_id, ordinal, state, safety_mode, interaction_mode, created_at, updated_at)
                 VALUES ('turn-2', 'thread-1', 1, 'active', 'ask_for_approval', 'build', ?, ?)",
            )
            .bind(NOW)
            .bind(NOW)
            .execute(&pool)
            .await
            .unwrap();
            let arguments = Map::new();
            let first = ChatTurnId::new("turn-1").unwrap();
            for _ in 0..MAX_PREVIEW_TOOL_CALLS_PER_TURN {
                let invocation = reserve(&pool, &first, "preview_status", &arguments)
                    .await
                    .unwrap();
                complete(&pool, &invocation, PreviewToolOutcome::Succeeded, 1)
                    .await
                    .unwrap();
            }
            assert!(
                reserve(&pool, &first, "preview_status", &arguments)
                    .await
                    .is_err()
            );
            reserve(
                &pool,
                &ChatTurnId::new("turn-2").unwrap(),
                "preview_status",
                &arguments,
            )
            .await
            .expect("the next turn has its own budget");

            sqlx::query("DELETE FROM chat_turns WHERE id = 'turn-1'")
                .execute(&pool)
                .await
                .unwrap();
            assert_eq!(rows(&pool).await.len(), 1);
        });
    }

    #[test]
    fn settling_refuses_oversized_results_and_keeps_tool_errors() {
        let (outcome, bytes, result) = settle(Ok(serde_json::json!({ "accepted": true })));
        assert_eq!(outcome, PreviewToolOutcome::Succeeded);
        assert_eq!(bytes, r#"{"accepted":true}"#.len());
        assert!(result.is_ok());

        let oversized = "x".repeat(MAX_PREVIEW_TOOL_RESPONSE_BYTES);
        let (outcome, bytes, result) = settle(Ok(serde_json::json!({ "snapshot": oversized })));
        assert_eq!(outcome, PreviewToolOutcome::Refused);
        assert_eq!(bytes, 0);
        assert_eq!(result.unwrap_err().code, ChatErrorCode::Validation);

        let (outcome, bytes, result) = settle(Err(ChatError::new(
            ChatErrorCode::Timeout,
            "Browser wait condition timed out",
            true,
        )));
        assert_eq!(outcome, PreviewToolOutcome::Failed);
        assert_eq!(bytes, 0);
        assert_eq!(result.unwrap_err().code, ChatErrorCode::Timeout);
    }
}
