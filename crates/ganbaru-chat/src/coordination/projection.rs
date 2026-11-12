//! Projection of provider runtime events into organizational conversations.

use super::super::agent_runs::{SETTLED_RUN_STATES, unsettled_run_state_sql};
use super::super::events::{
    CanonicalEvent, CanonicalRuntimeEvent, ChangedFileSummary, TurnCompletedEvent,
};
use super::super::models::{
    ChatError, ChatErrorCode, ChatParticipantId, ChatReplyThreadId, ChatResult, ChatTurnId,
    ChatWorkAssignmentId,
};
use super::reports::{
    TeammateUpdate, insert_teammate_update, run_asked_host_question, run_result_report,
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
            if !update_assignment_and_run_state(
                transaction,
                &assignment_id,
                &run_id,
                "waiting_for_approval",
                "waiting",
                None,
                runtime.created_at.as_str(),
            )
            .await?
            {
                return Ok(false);
            }
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
            if !update_assignment_and_run_state(
                transaction,
                &assignment_id,
                &run_id,
                "waiting_for_answer",
                "waiting",
                None,
                runtime.created_at.as_str(),
            )
            .await?
            {
                return Ok(false);
            }
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
            .await?
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
            if awaits_host_answer(transaction, &assignment_id, &run_id).await? {
                settle_run(transaction, &run_id, runtime.created_at.as_str()).await?;
                return Ok(false);
            }
            let changed_files = turn_changed_files(transaction, runtime).await?;
            let reported_result = run_result_report(transaction, &run_id).await?;
            if let Some(body) = reported_result {
                if !update_assignment_and_run_state(
                    transaction,
                    &assignment_id,
                    &run_id,
                    "ready_for_review",
                    "completed",
                    None,
                    runtime.created_at.as_str(),
                )
                .await?
                {
                    return Ok(true);
                }
                if changed_files.is_empty() {
                    false
                } else {
                    insert_projected_teammate_message(
                        transaction,
                        runtime,
                        &target,
                        "review",
                        &body,
                        turn_completion_payload(event, runtime, &changed_files),
                    )
                    .await?
                }
            } else {
                let state = if changed_files.is_empty() {
                    "completed"
                } else {
                    "ready_for_review"
                };
                if !update_assignment_and_run_state(
                    transaction,
                    &assignment_id,
                    &run_id,
                    state,
                    "completed",
                    None,
                    runtime.created_at.as_str(),
                )
                .await?
                {
                    return Ok(true);
                }
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
                    turn_completion_payload(event, runtime, &changed_files),
                )
                .await?
            }
        }
        CanonicalEvent::TurnAborted(event) => {
            if !update_assignment_and_run_state(
                transaction,
                &assignment_id,
                &run_id,
                "failed",
                "failed",
                Some(&event.reason),
                runtime.created_at.as_str(),
            )
            .await?
            {
                return Ok(true);
            }
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
            if !update_assignment_and_run_state(
                transaction,
                &assignment_id,
                &run_id,
                "failed",
                "failed",
                Some(&event.message),
                runtime.created_at.as_str(),
            )
            .await?
            {
                return Ok(true);
            }
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
    if settles_assignment(&runtime.event) {
        // Reached only when the event settled the assignment, after its result or failure
        // message, so a delegated child reports that body to its parent.
        super::delegation::post_delegation_outcome(
            transaction,
            assignment_id.as_str(),
            runtime.created_at.as_str(),
        )
        .await?;
    }
    Ok(projected)
}

/// Whether `event` settles the assignment its run belongs to when it applies.
fn settles_assignment(event: &CanonicalEvent) -> bool {
    match event {
        CanonicalEvent::TurnCompleted(_) | CanonicalEvent::TurnAborted(_) => true,
        CanonicalEvent::RuntimeError(event) => !event.recoverable,
        _ => false,
    }
}

/// The payload of the result or review a completed turn posts.
fn turn_completion_payload(
    event: &TurnCompletedEvent,
    runtime: &CanonicalRuntimeEvent,
    changed_files: &[ChangedFileSummary],
) -> Value {
    json!({
        "stopReason": event.stop_reason,
        "usage": event.usage,
        "changedFiles": changed_files,
        "providerExecutionThreadId": runtime.thread_id,
        "providerExecutionTurnId": runtime.turn_id,
    })
}

/// Whether the assignment still waits for the answer to the host question its run `run_id`
/// asked, so the ending turn settles only the run and the answer starts the next one.
async fn awaits_host_answer(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    run_id: &str,
) -> ChatResult<bool> {
    let waiting: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_work_assignments
           WHERE id = ? AND state = 'waiting_for_answer'
         )",
    )
    .bind(assignment_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if !waiting {
        return Ok(false);
    }
    run_asked_host_question(transaction, run_id).await
}

/// Settles the run `run_id` as completed when it has not settled, leaving its assignment as it is.
async fn settle_run(
    transaction: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    occurred_at: &str,
) -> ChatResult<()> {
    let sql = format!(
        "UPDATE chat_agent_runs
         SET state = 'completed', settled_at = ?, updated_at = ?
         WHERE id = ? AND {}",
        unsettled_run_state_sql("state")
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(occurred_at)
        .bind(occurred_at)
        .bind(run_id)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    Ok(())
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
    let revision_id = format!("organizational-revision:{event_hash}");
    insert_teammate_update(
        transaction,
        &TeammateUpdate {
            item_id: &item_id,
            revision_id: &revision_id,
            reply_thread_id: target.reply_thread_id.as_str(),
            teammate_id: target.teammate_id.as_str(),
            assignment_id: target.assignment_id.as_str(),
            run_id: Some(target.run_id),
            update_kind,
            markdown,
            payload,
            created_at: runtime.created_at.as_str(),
        },
    )
    .await
}

/// Moves the assignment `assignment_id` to `assignment_state` and its run `run_id` to
/// `run_state`, reporting whether the assignment changed. An assignment that already settled
/// keeps its state; its run then records only a settled state, since the provider turn really
/// ended, and never waits or works again. Callers project no message for an assignment that
/// did not change, so a settled assignment shows no late approval, question, or review.
/// A run that already settled keeps its state and settle time.
async fn update_assignment_and_run_state(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    run_id: &str,
    assignment_state: &str,
    run_state: &str,
    reason: Option<&str>,
    occurred_at: &str,
) -> ChatResult<bool> {
    let terminal = matches!(assignment_state, "completed" | "failed" | "cancelled");
    // A settled assignment never leaves its state, so provider events that arrive after a
    // cancel applied from another device cannot reopen it.
    let assignment = sqlx::query(
        "UPDATE chat_work_assignments
         SET state = ?, state_reason = ?, settled_at = CASE WHEN ? THEN ? ELSE NULL END,
             revision = revision + 1, updated_at = ?
         WHERE id = ? AND state NOT IN ('completed', 'failed', 'cancelled')",
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
    let applied = assignment.rows_affected() == 1;
    let run_terminal = SETTLED_RUN_STATES.contains(&run_state);
    if !applied && !run_terminal {
        return Ok(false);
    }
    let run_update = format!(
        "UPDATE chat_agent_runs
         SET state = ?, settled_at = CASE WHEN ? THEN ? ELSE NULL END, updated_at = ?
         WHERE id = ? AND {}",
        unsettled_run_state_sql("state")
    );
    sqlx::query(sqlx::AssertSqlSafe(run_update.as_str()))
        .bind(run_state)
        .bind(run_terminal)
        .bind(occurred_at)
        .bind(occurred_at)
        .bind(run_id)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    Ok(applied)
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
