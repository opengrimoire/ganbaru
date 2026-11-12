//! Outcome delivery of delegated work to the assignment that delegated it.
//!
//! When a delegated child settles, the parent's reply thread receives one teammate message
//! authored by the child teammate with a `delegation` update. Its ids derive from the child, so
//! the device that settled the child and the parent's executing device, which writes it after
//! sync applied the settled child, produce the same rows and sync merges them into one message.

use crate::ids::{ASSIGNMENT_INPUT_ID_PREFIX, derived_id};
use crate::models::{
    ChatError, ChatErrorCode, ChatResult, ChatWorkAssignmentId, ChatWorkAssignmentState,
};
use crate::timeline::{bounded_projected_revision, next_order_key, truncate_at_char_boundary};
use serde_json::json;
use sqlx::{Row, SqliteConnection};

/// Most bytes of a teammate's report body, and of the child result an outcome carries.
pub const MAX_REPORT_BODY_BYTES: usize = 16 * 1024;

/// First id part of every row an outcome writes, followed by the child assignment id.
const OUTCOME_ID_PART: &str = "delegation-outcome";

/// Deepest delegation the schema allows, which bounds every walk down the delegated work of an
/// assignment.
const DELEGATION_DEPTH_LIMIT: i64 = 2;

/// Child states that post an outcome.
const OUTCOME_STATES: [&str; 4] = ["completed", "ready_for_review", "failed", "cancelled"];

/// The outcome message one settled child posted in its parent's reply thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DelegationOutcome {
    pub parent_assignment_id: ChatWorkAssignmentId,
    pub message_item_id: String,
    /// Whether the message became a queued continuation input of the parent.
    pub input_queued: bool,
}

/// Returns the conversation item id of the outcome of `child_assignment_id`.
///
/// # Errors
///
/// Returns a validation error when the id cannot be derived from `child_assignment_id`.
pub fn outcome_item_id(child_assignment_id: &str) -> ChatResult<String> {
    derived_id("conversation-item", &[OUTCOME_ID_PART, child_assignment_id])
}

/// Posts the outcome of the delegated child `child_assignment_id` in its parent's reply thread
/// when the child reached `completed`, `ready_for_review`, `failed`, or `cancelled`.
///
/// The message is authored by the child teammate and carries the child's last result, review,
/// or failure body, else its state reason, bounded to [`MAX_REPORT_BODY_BYTES`]. When the parent
/// still accepts continuation, the message also becomes a `queued_continuation` input of the
/// parent; otherwise it stays visible only. An undelegated or unsettled child, a child already
/// reported, and a cancelled child whose work moved to a successor write nothing, so repeating
/// the call leaves every revision as it was. Call it after the child's settle and its result or
/// failure message were written in the same transaction.
///
/// # Errors
///
/// Returns a persistence error when a read or write fails.
pub async fn post_delegation_outcome(
    connection: &mut SqliteConnection,
    child_assignment_id: &str,
    now: &str,
) -> ChatResult<Option<DelegationOutcome>> {
    let Some(child) = read_settled_child(connection, child_assignment_id).await? else {
        return Ok(None);
    };
    let item_id = outcome_item_id(child_assignment_id)?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_conversation_items WHERE id = ?)")
            .bind(&item_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(persistence_error)?;
    if exists {
        return Ok(None);
    }
    let body = child_result_body(connection, child_assignment_id).await?;
    let body = body
        .or_else(|| child.state_reason.clone())
        .filter(|body| !body.trim().is_empty())
        .unwrap_or_else(|| fallback_body(&child.state).to_owned());
    let (body, _) = truncate_at_char_boundary(&body, MAX_REPORT_BODY_BYTES);
    let revision_id = derived_id("message-revision", &[OUTCOME_ID_PART, child_assignment_id])?;
    let order_key = next_order_key(
        connection,
        &child.conversation_id,
        Some(&child.parent_reply_thread_id),
    )
    .await?;
    sqlx::query(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         VALUES (?, ?, ?, 'message', ?, ?)",
    )
    .bind(&item_id)
    .bind(&child.conversation_id)
    .bind(&child.parent_reply_thread_id)
    .bind(&order_key)
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_communication_messages
            (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&item_id)
    .bind(&child.teammate_id)
    .bind(&child.teammate_label)
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let rich_content = json!({
        "type": "agent_update",
        "updateKind": "delegation",
        "assignmentId": child.parent_assignment_id,
        "agentRunId": null,
        "payload": {
            "phase": "settled",
            "childAssignmentId": child_assignment_id,
            "parentAssignmentId": child.parent_assignment_id,
            "state": child.state,
        },
    });
    let bounded = bounded_projected_revision(body, rich_content)?;
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
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query("UPDATE chat_communication_messages SET current_revision_id = ? WHERE item_id = ?")
        .bind(&revision_id)
        .bind(&item_id)
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_work_semantic_updates
            (item_id, assignment_id, update_kind, payload_data, created_at)
         VALUES (?, ?, 'delegation', ?, ?)",
    )
    .bind(&item_id)
    .bind(&child.parent_assignment_id)
    .bind(&bounded.rich_content_data)
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let input_queued = if child.parent_state.accepts_continuation() {
        append_continuation_input(connection, &child.parent_assignment_id, &item_id, now).await?;
        true
    } else {
        false
    };
    sqlx::query(
        "UPDATE chat_reply_threads SET last_activity_at = ?, revision = revision + 1,
             updated_at = ? WHERE id = ?",
    )
    .bind(now)
    .bind(now)
    .bind(&child.parent_reply_thread_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_conversations SET last_activity_at = ?, revision = revision + 1,
             updated_at = ? WHERE id = ?",
    )
    .bind(now)
    .bind(now)
    .bind(&child.conversation_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(Some(DelegationOutcome {
        parent_assignment_id: ChatWorkAssignmentId::new(child.parent_assignment_id)
            .map_err(|_| stored_value_error())?,
        message_item_id: item_id,
        input_queued,
    }))
}

/// State reason of delegated work cancelled because a person cancelled an assignment it was
/// delegated from.
pub const PARENT_CANCELLED_REASON: &str = "parent_cancelled";

/// Cancels the open work delegated from `assignment_id`, down to the delegation depth limit,
/// after a person cancelled it, and returns the cancelled ids.
///
/// Each open descendant moves to `cancelled` with [`PARENT_CANCELLED_REASON`], its allowed
/// authorizations are revoked with their access revocation jobs, as a revoked parent revokes
/// them, and its outcome is posted in its own parent's reply thread. The outcomes are posted
/// after every descendant settled, so a cancelled parent receives them without an input. The
/// cancelled state replicates, so a descendant executing on another device stops there when
/// sync applies it. Settled descendants keep their state, so repeating the call cancels
/// nothing more.
///
/// # Errors
///
/// Returns a persistence error when a read or write fails.
pub async fn cancel_delegated_descendants(
    connection: &mut SqliteConnection,
    assignment_id: &str,
    now: &str,
) -> ChatResult<Vec<String>> {
    let open: Vec<String> = sqlx::query_scalar(
        "WITH RECURSIVE descendants(id, depth) AS (
           SELECT id, 1 FROM chat_work_assignments WHERE parent_assignment_id = ?1
           UNION
           SELECT child.id, descendants.depth + 1
           FROM chat_work_assignments child
           JOIN descendants ON child.parent_assignment_id = descendants.id
           WHERE descendants.depth < ?2
         )
         SELECT assignment.id FROM chat_work_assignments assignment
         JOIN descendants ON descendants.id = assignment.id
         WHERE assignment.state NOT IN ('completed', 'ready_for_review', 'failed', 'cancelled')
         ORDER BY descendants.depth, assignment.id",
    )
    .bind(assignment_id)
    .bind(DELEGATION_DEPTH_LIMIT)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    for descendant_id in &open {
        sqlx::query(
            "UPDATE chat_work_assignments
             SET state = 'cancelled', state_reason = ?, settled_at = ?,
                 revision = revision + 1, updated_at = ?
             WHERE id = ? AND state NOT IN ('completed', 'ready_for_review', 'failed', 'cancelled')",
        )
        .bind(PARENT_CANCELLED_REASON)
        .bind(now)
        .bind(now)
        .bind(descendant_id)
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?;
    }
    super::revocation::revoke_delegated_authorizations(connection, assignment_id, now).await?;
    for descendant_id in &open {
        post_delegation_outcome(connection, descendant_id, now).await?;
    }
    Ok(open)
}

/// A settled delegated child and the parent thread its outcome goes to.
struct SettledChild {
    state: String,
    state_reason: Option<String>,
    teammate_id: String,
    teammate_label: String,
    parent_assignment_id: String,
    parent_state: ChatWorkAssignmentState,
    parent_reply_thread_id: String,
    conversation_id: String,
}

/// Reads `child_assignment_id` when it is a delegated child in an outcome state. A child
/// cancelled because its work moved to a successor is not settled work, so it is skipped.
async fn read_settled_child(
    connection: &mut SqliteConnection,
    child_assignment_id: &str,
) -> ChatResult<Option<SettledChild>> {
    let row = sqlx::query(
        "SELECT child.state, child.state_reason, child.teammate_id,
                participant.display_name AS teammate_label,
                parent.id AS parent_assignment_id, parent.state AS parent_state,
                parent.reply_thread_id AS parent_reply_thread_id, thread.conversation_id,
                EXISTS(
                  SELECT 1 FROM chat_work_assignments successor
                  WHERE successor.previous_assignment_id = child.id
                ) AS has_successor
         FROM chat_work_assignments child
         JOIN chat_work_assignments parent ON parent.id = child.parent_assignment_id
         JOIN chat_reply_threads thread ON thread.id = parent.reply_thread_id
         JOIN chat_participants participant ON participant.id = child.teammate_id
         WHERE child.id = ?",
    )
    .bind(child_assignment_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let state: String = row.try_get("state").map_err(persistence_error)?;
    let has_successor: bool = row.try_get("has_successor").map_err(persistence_error)?;
    if !OUTCOME_STATES.contains(&state.as_str()) || (state == "cancelled" && has_successor) {
        return Ok(None);
    }
    let parent_state: String = row.try_get("parent_state").map_err(persistence_error)?;
    Ok(Some(SettledChild {
        state,
        state_reason: row.try_get("state_reason").map_err(persistence_error)?,
        teammate_id: row.try_get("teammate_id").map_err(persistence_error)?,
        teammate_label: row.try_get("teammate_label").map_err(persistence_error)?,
        parent_assignment_id: row
            .try_get("parent_assignment_id")
            .map_err(persistence_error)?,
        parent_state: serde_json::from_value(serde_json::Value::String(parent_state))
            .map_err(|_| stored_value_error())?,
        parent_reply_thread_id: row
            .try_get("parent_reply_thread_id")
            .map_err(persistence_error)?,
        conversation_id: row.try_get("conversation_id").map_err(persistence_error)?,
    }))
}

/// The Markdown of the child's latest result, review, or failure update. These replicate with
/// the child, so every device reads the same body.
async fn child_result_body(
    connection: &mut SqliteConnection,
    child_assignment_id: &str,
) -> ChatResult<Option<String>> {
    sqlx::query_scalar(
        "SELECT revision.normalized_markdown
         FROM chat_work_semantic_updates semantic
         JOIN chat_conversation_items item ON item.id = semantic.item_id
         JOIN chat_communication_messages message ON message.item_id = semantic.item_id
         JOIN chat_communication_message_revisions revision
           ON revision.id = message.current_revision_id
         WHERE semantic.assignment_id = ?
           AND semantic.update_kind IN ('result', 'review', 'failure')
         ORDER BY item.order_key DESC, item.id DESC LIMIT 1",
    )
    .bind(child_assignment_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)
}

/// The body of an outcome whose child left no result and no reason.
fn fallback_body(state: &str) -> &'static str {
    match state {
        "completed" => "Delegated work completed.",
        "ready_for_review" => "Delegated work is ready for review.",
        "failed" => "Delegated work failed.",
        _ => "Delegated work was cancelled.",
    }
}

/// Appends the outcome message as the next queued continuation input of the parent, under the
/// input id derived from the message.
async fn append_continuation_input(
    connection: &mut SqliteConnection,
    parent_assignment_id: &str,
    message_item_id: &str,
    now: &str,
) -> ChatResult<()> {
    let ordinal: i64 = sqlx::query_scalar(
        "SELECT coalesce(max(ordinal), 0) + 1
         FROM chat_work_assignment_inputs WHERE assignment_id = ?",
    )
    .bind(parent_assignment_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_work_assignment_inputs
            (id, assignment_id, message_item_id, ordinal, routing_kind, created_at)
         VALUES (?, ?, ?, ?, 'queued_continuation', ?)",
    )
    .bind(derived_id(ASSIGNMENT_INPUT_ID_PREFIX, &[message_item_id])?)
    .bind(parent_assignment_id)
    .bind(message_item_id)
    .bind(ordinal)
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat data could not be saved or read",
        true,
    )
}

fn stored_value_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat delegation data is invalid",
        false,
    )
}

#[cfg(test)]
mod tests;
