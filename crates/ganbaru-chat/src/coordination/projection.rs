//! Projection of provider runtime events into organizational conversations.

use super::super::events::{CanonicalEvent, CanonicalRuntimeEvent, ChangedFileSummary};
use super::super::models::{
    ChatError, ChatErrorCode, ChatParticipantId, ChatReplyThreadId, ChatResult, ChatTurnId,
    ChatWorkAssignmentId,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat data could not be saved or read",
        true,
    )
}

fn identifier_error(_error: String) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat identifier is invalid",
        false,
    )
}

/// Run states after which provider events no longer project into organizational Chat.
const SETTLED_RUN_STATES: [&str; 3] = ["completed", "failed", "cancelled"];

/// Projects a provider event into the organizational state of the agent run it belongs to.
///
/// Events are matched by provider turn, or by provider thread while a run is active. A run that
/// has settled (completed, failed, or cancelled by revocation) ignores later events, so a late or
/// replayed provider event can neither reopen it nor overwrite how it ended.
pub async fn project_provider_event_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    runtime: &CanonicalRuntimeEvent,
) -> ChatResult<bool> {
    let run = sqlx::query(
        "SELECT run.id AS run_id, run.state AS run_state, run.assignment_id,
                assignment.reply_thread_id, assignment.teammate_id
         FROM chat_agent_runs run
         JOIN chat_work_assignments assignment ON assignment.id = run.assignment_id
         WHERE (
             (? IS NOT NULL AND run.provider_turn_id = ?)
             OR (? IS NULL AND run.provider_thread_id = ? AND run.state IN ('starting', 'working', 'waiting'))
         )
         ORDER BY run.run_ordinal DESC LIMIT 1",
    )
    .bind(runtime.turn_id.as_ref().map(ChatTurnId::as_str))
    .bind(runtime.turn_id.as_ref().map(ChatTurnId::as_str))
    .bind(runtime.turn_id.as_ref().map(ChatTurnId::as_str))
    .bind(runtime.thread_id.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let Some(run) = run else {
        return Ok(false);
    };
    let run_state: String = run.try_get("run_state").map_err(persistence_error)?;
    if SETTLED_RUN_STATES.contains(&run_state.as_str()) {
        return Ok(false);
    }
    let run_id: String = run.try_get("run_id").map_err(persistence_error)?;
    let assignment_id = ChatWorkAssignmentId::new(
        run.try_get::<String, _>("assignment_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let reply_thread_id = ChatReplyThreadId::new(
        run.try_get::<String, _>("reply_thread_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let teammate_id = ChatParticipantId::new(
        run.try_get::<String, _>("teammate_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let target = ProjectionTarget {
        run_id: &run_id,
        assignment_id: &assignment_id,
        reply_thread_id: &reply_thread_id,
        teammate_id: &teammate_id,
    };
    let projected = match &runtime.event {
        CanonicalEvent::PlanUpdated(event) => {
            insert_projected_teammate_message(
                transaction,
                runtime,
                &target,
                "plan",
                &event.markdown,
                json!({ "steps": event.steps }),
            )
            .await?
        }
        CanonicalEvent::ProposedPlanCompleted(event) => {
            insert_projected_teammate_message(
                transaction,
                runtime,
                &target,
                "plan",
                &event.markdown,
                json!({ "planId": event.plan_id }),
            )
            .await?
        }
        CanonicalEvent::RequestOpened(event) => {
            let markdown = match event.detail.as_deref() {
                Some(detail) if !detail.trim().is_empty() => {
                    format!("{}\n\n{}", event.title, detail)
                }
                _ => event.title.clone(),
            };
            update_assignment_and_run_state(
                transaction,
                &assignment_id,
                &run_id,
                "waiting_for_approval",
                "waiting",
                None,
                runtime.created_at.as_str(),
            )
            .await?;
            insert_projected_teammate_message(
                transaction,
                runtime,
                &target,
                "approval",
                &markdown,
                json!({
                    "requestId": event.request_id,
                    "kind": event.kind,
                    "allowedDecisions": event.allowed_decisions,
                    "safePayload": event.safe_payload,
                }),
            )
            .await?
        }
        CanonicalEvent::UserInputRequested(event) => {
            let markdown = event
                .questions
                .iter()
                .map(|question| question.question.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            update_assignment_and_run_state(
                transaction,
                &assignment_id,
                &run_id,
                "waiting_for_answer",
                "waiting",
                None,
                runtime.created_at.as_str(),
            )
            .await?;
            insert_projected_teammate_message(
                transaction,
                runtime,
                &target,
                "question",
                &markdown,
                json!({
                    "requestId": event.request_id,
                    "questions": event.questions,
                }),
            )
            .await?
        }
        CanonicalEvent::RequestResolved(_) | CanonicalEvent::UserInputResolved(_) => {
            update_assignment_and_run_state(
                transaction,
                &assignment_id,
                &run_id,
                "working",
                "working",
                None,
                runtime.created_at.as_str(),
            )
            .await?;
            true
        }
        CanonicalEvent::TurnCompleted(event) => {
            let markdown: Option<String> = sqlx::query_scalar(
                "SELECT normalized_markdown
                 FROM chat_messages
                 WHERE thread_id = ? AND turn_id = ? AND role = 'assistant'
                 ORDER BY sequence_anchor DESC, created_at DESC LIMIT 1",
            )
            .bind(runtime.thread_id.as_str())
            .bind(runtime.turn_id.as_ref().map(ChatTurnId::as_str))
            .fetch_optional(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            let changed_files = turn_changed_files(transaction, runtime).await?;
            let state = if changed_files.is_empty() {
                "completed"
            } else {
                "ready_for_review"
            };
            update_assignment_and_run_state(
                transaction,
                &assignment_id,
                &run_id,
                state,
                "completed",
                None,
                runtime.created_at.as_str(),
            )
            .await?;
            insert_projected_teammate_message(
                transaction,
                runtime,
                &target,
                if changed_files.is_empty() {
                    "result"
                } else {
                    "review"
                },
                markdown
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("Work completed."),
                json!({
                    "stopReason": event.stop_reason,
                    "usage": event.usage,
                    "changedFiles": changed_files,
                    "providerExecutionThreadId": runtime.thread_id,
                    "providerExecutionTurnId": runtime.turn_id,
                }),
            )
            .await?
        }
        CanonicalEvent::TurnAborted(event) => {
            update_assignment_and_run_state(
                transaction,
                &assignment_id,
                &run_id,
                "failed",
                "failed",
                Some(&event.reason),
                runtime.created_at.as_str(),
            )
            .await?;
            insert_projected_teammate_message(
                transaction,
                runtime,
                &target,
                "failure",
                &event.reason,
                json!({ "recoverable": event.recoverable }),
            )
            .await?
        }
        CanonicalEvent::RuntimeError(event) if !event.recoverable => {
            update_assignment_and_run_state(
                transaction,
                &assignment_id,
                &run_id,
                "failed",
                "failed",
                Some(&event.message),
                runtime.created_at.as_str(),
            )
            .await?;
            insert_projected_teammate_message(
                transaction,
                runtime,
                &target,
                "failure",
                &event.message,
                json!({ "code": event.code, "safeDetails": event.safe_details }),
            )
            .await?
        }
        _ => false,
    };
    Ok(projected)
}

struct ProjectionTarget<'a> {
    run_id: &'a str,
    assignment_id: &'a ChatWorkAssignmentId,
    reply_thread_id: &'a ChatReplyThreadId,
    teammate_id: &'a ChatParticipantId,
}

async fn insert_projected_teammate_message(
    transaction: &mut Transaction<'_, Sqlite>,
    runtime: &CanonicalRuntimeEvent,
    target: &ProjectionTarget<'_>,
    update_kind: &str,
    markdown: &str,
    payload: Value,
) -> ChatResult<bool> {
    let event_hash = sha256_hex(runtime.event_id.as_str().as_bytes());
    let item_id = format!("organizational-item:{event_hash}");
    let exists: i64 =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_conversation_items WHERE id = ?)")
            .bind(&item_id)
            .fetch_one(&mut **transaction)
            .await
            .map_err(persistence_error)?;
    if exists != 0 {
        return Ok(false);
    }
    let conversation_id: String =
        sqlx::query_scalar("SELECT conversation_id FROM chat_reply_threads WHERE id = ?")
            .bind(target.reply_thread_id.as_str())
            .fetch_one(&mut **transaction)
            .await
            .map_err(persistence_error)?;
    let order_key = crate::timeline::next_order_key(
        &mut **transaction,
        &conversation_id,
        Some(target.reply_thread_id.as_str()),
    )
    .await?;
    let revision_id = format!("organizational-revision:{event_hash}");
    sqlx::query(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         VALUES (?, ?, ?, 'message', ?, ?)",
    )
    .bind(&item_id)
    .bind(&conversation_id)
    .bind(target.reply_thread_id.as_str())
    .bind(&order_key)
    .bind(runtime.created_at.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_communication_messages
            (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES (?, ?, (SELECT display_name FROM chat_participants WHERE id = ?), ?)",
    )
    .bind(&item_id)
    .bind(target.teammate_id.as_str())
    .bind(target.teammate_id.as_str())
    .bind(runtime.created_at.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let rich_content = json!({
        "type": "agent_update",
        "updateKind": update_kind,
        "assignmentId": target.assignment_id,
        "agentRunId": target.run_id,
        "payload": payload,
    });
    let bounded = crate::timeline::bounded_projected_revision(markdown, rich_content.clone())?;
    sqlx::query(
        "INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown,
             rich_content_schema_version, rich_content_data, created_at)
         VALUES (?, ?, ?, 1, ?, ?)",
    )
    .bind(&revision_id)
    .bind(&item_id)
    .bind(&bounded.normalized_markdown)
    .bind(&bounded.rich_content_data)
    .bind(runtime.created_at.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query("UPDATE chat_communication_messages SET current_revision_id = ? WHERE item_id = ?")
        .bind(&revision_id)
        .bind(&item_id)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_work_semantic_updates
            (item_id, assignment_id, update_kind, payload_data, created_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&item_id)
    .bind(target.assignment_id.as_str())
    .bind(update_kind)
    .bind(serde_json::to_string(&rich_content).map_err(serialization_error)?)
    .bind(runtime.created_at.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_reply_threads
         SET last_activity_at = ?,
             revision = revision + 1, updated_at = ? WHERE id = ?",
    )
    .bind(runtime.created_at.as_str())
    .bind(runtime.created_at.as_str())
    .bind(target.reply_thread_id.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_conversations
         SET last_activity_at = ?, revision = revision + 1, updated_at = ? WHERE id = ?",
    )
    .bind(runtime.created_at.as_str())
    .bind(runtime.created_at.as_str())
    .bind(&conversation_id)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(true)
}

async fn update_assignment_and_run_state(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    run_id: &str,
    assignment_state: &str,
    run_state: &str,
    reason: Option<&str>,
    occurred_at: &str,
) -> ChatResult<()> {
    let terminal = matches!(assignment_state, "completed" | "failed" | "cancelled");
    sqlx::query(
        "UPDATE chat_work_assignments
         SET state = ?, state_reason = ?, settled_at = CASE WHEN ? THEN ? ELSE NULL END,
             revision = revision + 1, updated_at = ? WHERE id = ?",
    )
    .bind(assignment_state)
    .bind(reason)
    .bind(terminal)
    .bind(occurred_at)
    .bind(occurred_at)
    .bind(assignment_id.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let run_terminal = SETTLED_RUN_STATES.contains(&run_state);
    sqlx::query(
        "UPDATE chat_agent_runs
         SET state = ?, settled_at = CASE WHEN ? THEN ? ELSE NULL END, updated_at = ?
         WHERE id = ?",
    )
    .bind(run_state)
    .bind(run_terminal)
    .bind(occurred_at)
    .bind(occurred_at)
    .bind(run_id)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Reads the files recorded for the completed turn: every diff update, the completion
/// report, and the post-turn checkpoint diff merged by the ingestion path.
async fn turn_changed_files(
    transaction: &mut Transaction<'_, Sqlite>,
    runtime: &CanonicalRuntimeEvent,
) -> ChatResult<Vec<ChangedFileSummary>> {
    let data: Option<String> = sqlx::query_scalar(
        "SELECT changed_file_summary_data FROM chat_turns WHERE id = ? AND thread_id = ?",
    )
    .bind(runtime.turn_id.as_ref().map(ChatTurnId::as_str))
    .bind(runtime.thread_id.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?
    .flatten();
    data.as_deref()
        .map(serde_json::from_str::<Vec<ChangedFileSummary>>)
        .transpose()
        .map_err(serialization_error)
        .map(Option::unwrap_or_default)
}

fn serialization_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat organizational projection data is invalid",
        false,
    )
}
