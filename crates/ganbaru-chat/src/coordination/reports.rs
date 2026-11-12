//! Questions and reports a teammate posts through its host tools.
//!
//! A host question moves working work to `waiting_for_answer` until the person who assigned it
//! replies, and a host report posts a plan, replan, or result in the reply thread. Both are
//! teammate messages with a semantic update whose payload carries [`HOST_TOOL_SOURCE`], so the
//! provider event projection can tell what the teammate reported from what it would infer. Their
//! ids derive from the run and the client request id, so a repeated call writes nothing new.

use super::delegation::MAX_REPORT_BODY_BYTES;
use crate::ids::derived_id;
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use crate::timeline::{bounded_projected_revision, next_order_key};
use serde_json::{Value, json};
use sqlx::SqliteConnection;

/// Longest question a teammate may ask, in characters.
pub const MAX_QUESTION_CHARS: usize = 4000;
/// Most answer choices one question may offer.
pub const MAX_QUESTION_CHOICES: usize = 4;
/// Longest answer choice, in characters.
pub const MAX_QUESTION_CHOICE_CHARS: usize = 200;
/// Payload `source` of every update a host tool posted.
pub const HOST_TOOL_SOURCE: &str = "host_tool";

/// First id part of the rows a host question writes.
const ASK_ID_PART: &str = "ask";
/// First id part of the rows a host report writes.
const REPORT_ID_PART: &str = "report";

/// The kind of a host report, which is also its semantic update kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReportKind {
    Plan,
    Replan,
    Result,
}

impl ReportKind {
    /// Every report kind.
    pub const ALL: [Self; 3] = [Self::Plan, Self::Replan, Self::Result];

    /// The wire name of the kind, equal to its semantic update kind.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Replan => "replan",
            Self::Result => "result",
        }
    }

    /// The kind a wire name names, or `None` for any other name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }
}

/// Why a host report or question was refused. [`HostReportRefusal::code`] is the tool error
/// code, so the teammate can correct its next call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostReportRefusal {
    /// The report body is empty once trimmed.
    EmptyBody,
    /// The report body exceeds [`MAX_REPORT_BODY_BYTES`].
    BodyTooLarge,
    /// The report kind is not a [`ReportKind`].
    UnknownKind,
    /// The client request id was used before with different input.
    RequestConflict,
    /// The assignment is not working, so it cannot ask, or its run already ended.
    AssignmentInactive,
}

impl HostReportRefusal {
    const ALL: [Self; 5] = [
        Self::EmptyBody,
        Self::BodyTooLarge,
        Self::UnknownKind,
        Self::RequestConflict,
        Self::AssignmentInactive,
    ];

    /// The stable tool error code.
    pub const fn code(self) -> &'static str {
        match self {
            Self::EmptyBody => "report_empty_body",
            Self::BodyTooLarge => "report_body_too_large",
            Self::UnknownKind => "report_unknown_kind",
            Self::RequestConflict => "host_request_conflict",
            Self::AssignmentInactive => "assignment_not_working",
        }
    }

    /// The refusal as the tool error the teammate receives.
    pub fn into_error(self) -> ChatError {
        ChatError::new(ChatErrorCode::Permission, self.code(), false)
    }

    /// The refusal an error made by [`HostReportRefusal::into_error`] carries, if any.
    pub fn from_error(error: &ChatError) -> Option<Self> {
        if error.code != ChatErrorCode::Permission {
            return None;
        }
        Self::ALL
            .into_iter()
            .find(|refusal| refusal.code() == error.message)
    }
}

/// Returns the trimmed report body, or the refusal for an empty body or one over
/// [`MAX_REPORT_BODY_BYTES`].
///
/// # Errors
///
/// Returns [`HostReportRefusal::EmptyBody`] or [`HostReportRefusal::BodyTooLarge`].
pub fn validate_report_body(body: &str) -> Result<&str, HostReportRefusal> {
    let body = body.trim();
    if body.is_empty() {
        return Err(HostReportRefusal::EmptyBody);
    }
    if body.len() > MAX_REPORT_BODY_BYTES {
        return Err(HostReportRefusal::BodyTooLarge);
    }
    Ok(body)
}

/// The run that calls a host tool and the request id that makes the call idempotent.
#[derive(Clone, Copy, Debug)]
pub struct HostCall<'a> {
    pub run_id: &'a str,
    pub assignment_id: &'a str,
    pub client_request_id: &'a str,
}

/// Whether a host call wrote its update or found the one an identical earlier call wrote.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostCallOutcome {
    Posted,
    Replayed,
}

/// Records the host question `question` with `choices` that `call` asks the person who
/// assigned the work: the working assignment moves to `waiting_for_answer`, its run keeps
/// working until its turn ends, and a `question` update is posted in the reply thread.
///
/// # Errors
///
/// Returns [`HostReportRefusal::RequestConflict`] when the request id was used with another
/// question, [`HostReportRefusal::AssignmentInactive`] when the assignment is not working or the
/// run ended, both as errors from [`HostReportRefusal::into_error`], and a persistence error
/// when a read or write fails.
pub async fn record_host_question(
    connection: &mut SqliteConnection,
    call: HostCall<'_>,
    question: &str,
    choices: &[String],
    now: &str,
) -> ChatResult<HostCallOutcome> {
    let payload = json!({
        "question": question,
        "choices": choices,
        "source": HOST_TOOL_SOURCE,
    });
    let ids = HostUpdateIds::derive(ASK_ID_PART, call)?;
    if let Some(outcome) = replayed(connection, &ids.item_id, "question", &payload).await? {
        return outcome;
    }
    let target = read_active_target(connection, call, &["working"]).await?;
    sqlx::query(
        "UPDATE chat_work_assignments
         SET state = 'waiting_for_answer', state_reason = NULL,
             revision = revision + 1, updated_at = ?
         WHERE id = ? AND state = 'working'",
    )
    .bind(now)
    .bind(call.assignment_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let markdown = question_markdown(question, choices);
    insert_teammate_update(
        connection,
        &TeammateUpdate {
            item_id: &ids.item_id,
            revision_id: &ids.revision_id,
            reply_thread_id: &target.reply_thread_id,
            teammate_id: &target.teammate_id,
            assignment_id: call.assignment_id,
            run_id: Some(call.run_id),
            update_kind: "question",
            markdown: &markdown,
            payload,
            created_at: now,
        },
    )
    .await?;
    Ok(HostCallOutcome::Posted)
}

/// Records the host report of `kind` with the validated `body` that `call` posts in the reply
/// thread, while its assignment works or waits for an answer.
///
/// # Errors
///
/// Returns [`HostReportRefusal::RequestConflict`] when the request id was used with another
/// report, [`HostReportRefusal::AssignmentInactive`] when the assignment or run ended, both as
/// errors from [`HostReportRefusal::into_error`], and a persistence error when a read or write
/// fails.
pub async fn record_host_report(
    connection: &mut SqliteConnection,
    call: HostCall<'_>,
    kind: ReportKind,
    body: &str,
    now: &str,
) -> ChatResult<HostCallOutcome> {
    let payload = json!({ "kind": kind.as_str(), "source": HOST_TOOL_SOURCE });
    let ids = HostUpdateIds::derive(REPORT_ID_PART, call)?;
    if let Some(outcome) = replayed_report(connection, &ids.item_id, kind, body).await? {
        return outcome;
    }
    let target = read_active_target(connection, call, &["working", "waiting_for_answer"]).await?;
    insert_teammate_update(
        connection,
        &TeammateUpdate {
            item_id: &ids.item_id,
            revision_id: &ids.revision_id,
            reply_thread_id: &target.reply_thread_id,
            teammate_id: &target.teammate_id,
            assignment_id: call.assignment_id,
            run_id: Some(call.run_id),
            update_kind: kind.as_str(),
            markdown: body,
            payload,
            created_at: now,
        },
    )
    .await?;
    Ok(HostCallOutcome::Posted)
}

/// Whether the run `run_id` posted a host question.
///
/// # Errors
///
/// Returns a persistence error when the read fails.
pub async fn run_asked_host_question(
    connection: &mut SqliteConnection,
    run_id: &str,
) -> ChatResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_work_semantic_updates
           WHERE update_kind = 'question'
             AND json_extract(payload_data, '$.agentRunId') = ?
             AND json_extract(payload_data, '$.payload.source') = ?
         )",
    )
    .bind(run_id)
    .bind(HOST_TOOL_SOURCE)
    .fetch_one(connection)
    .await
    .map_err(persistence_error)
}

/// The body of the latest host result report of the run `run_id`, if it posted one.
///
/// # Errors
///
/// Returns a persistence error when the read fails.
pub async fn run_result_report(
    connection: &mut SqliteConnection,
    run_id: &str,
) -> ChatResult<Option<String>> {
    sqlx::query_scalar(
        "SELECT revision.normalized_markdown
         FROM chat_work_semantic_updates semantic
         JOIN chat_conversation_items item ON item.id = semantic.item_id
         JOIN chat_communication_messages message ON message.item_id = semantic.item_id
         JOIN chat_communication_message_revisions revision
           ON revision.id = message.current_revision_id
         WHERE semantic.update_kind = 'result'
           AND json_extract(semantic.payload_data, '$.agentRunId') = ?
           AND json_extract(semantic.payload_data, '$.payload.source') = ?
         ORDER BY item.order_key DESC, item.id DESC LIMIT 1",
    )
    .bind(run_id)
    .bind(HOST_TOOL_SOURCE)
    .fetch_optional(connection)
    .await
    .map_err(persistence_error)
}

/// SQL condition that holds when the input in `input` was posted by a person, so it can answer
/// a host question. A teammate's message or a delegation outcome never answers one.
const PERSON_INPUT_SQL: &str = "EXISTS(
           SELECT 1 FROM chat_communication_messages message
           JOIN chat_participants author ON author.id = message.author_participant_id
           WHERE message.item_id = input.message_item_id
             AND author.participant_kind IN ('local_user', 'human')
         )";

/// Moves `assignment_id` from `waiting_for_answer` back to `working` when its message
/// `message_item_id`, which a person posted, reached a run that still works, as when the person
/// answered before the asking turn ended. Returns whether the assignment changed.
///
/// # Errors
///
/// Returns a persistence error when a read or write fails.
pub async fn resume_answered_question(
    connection: &mut SqliteConnection,
    assignment_id: &str,
    message_item_id: &str,
    now: &str,
) -> ChatResult<bool> {
    let sql = format!(
        "UPDATE chat_work_assignments
         SET state = 'working', state_reason = NULL, revision = revision + 1, updated_at = ?1
         WHERE id = ?2 AND state = 'waiting_for_answer'
           AND EXISTS(
             SELECT 1 FROM chat_agent_runs run
             WHERE run.assignment_id = ?2 AND run.state = 'working'
           )
           AND EXISTS(
             SELECT 1 FROM chat_work_assignment_inputs input
             WHERE input.assignment_id = ?2 AND input.message_item_id = ?3
               AND {PERSON_INPUT_SQL}
           )"
    );
    let updated = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(now)
        .bind(assignment_id)
        .bind(message_item_id)
        .execute(connection)
        .await
        .map_err(persistence_error)?;
    Ok(updated.rows_affected() == 1)
}

/// Whether `assignment_id` waits for an answer that arrived after its asking run ended: it is in
/// `waiting_for_answer` with no live run, its latest run posted a host question, and a person
/// posted a pending input. Such work needs a new run to receive the answer.
///
/// # Errors
///
/// Returns a persistence error when the read fails.
pub async fn answered_question_awaiting_dispatch(
    connection: &mut SqliteConnection,
    assignment_id: &str,
) -> ChatResult<bool> {
    let sql = format!(
        "SELECT EXISTS(
           SELECT 1 FROM chat_work_assignments assignment
           WHERE assignment.id = ?1 AND assignment.state = 'waiting_for_answer'
             AND NOT EXISTS(
               SELECT 1 FROM chat_agent_runs run
               WHERE run.assignment_id = ?1 AND run.state IN ('queued', 'starting', 'working', 'waiting')
             )
             AND EXISTS(
               SELECT 1 FROM chat_work_semantic_updates semantic
               WHERE semantic.assignment_id = ?1 AND semantic.update_kind = 'question'
                 AND json_extract(semantic.payload_data, '$.payload.source') = ?2
                 AND json_extract(semantic.payload_data, '$.agentRunId') = (
                   SELECT latest.id FROM chat_agent_runs latest
                   WHERE latest.assignment_id = ?1
                   ORDER BY latest.run_ordinal DESC LIMIT 1
                 )
             )
             AND EXISTS(
               SELECT 1 FROM chat_work_assignment_inputs input
               WHERE input.assignment_id = ?1
                 AND input.routing_kind IN ('steer', 'queued_continuation')
                 AND input.delivery_state = 'pending'
                 AND {PERSON_INPUT_SQL}
             )
         )"
    );
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(assignment_id)
        .bind(HOST_TOOL_SOURCE)
        .fetch_one(connection)
        .await
        .map_err(persistence_error)
}

/// Queues `assignment_id` again on the device `device_id` when it waits for an answer that
/// arrived after its asking run ended, so a new run receives the answer. The work must run on
/// this device, which holds its settled dispatch job; the job keeps the subject it consented
/// to, so a subject that changed since returns the work to Start here when it is claimed.
/// Returns whether the work was queued; a second call for the same answer queues nothing.
///
/// # Errors
///
/// Returns a persistence error when a read or write fails.
pub async fn requeue_answered_assignment(
    connection: &mut SqliteConnection,
    assignment_id: &str,
    device_id: &str,
    now: &str,
) -> ChatResult<bool> {
    if !answered_question_awaiting_dispatch(connection, assignment_id).await? {
        return Ok(false);
    }
    let queued = sqlx::query(
        "UPDATE chat_work_assignments
         SET state = 'queued', state_reason = NULL, revision = revision + 1, updated_at = ?1
         WHERE id = ?2 AND state = 'waiting_for_answer'
           AND coalesce(executing_device_id, ?3) = ?3
           AND EXISTS(
             SELECT 1 FROM chat_assignment_dispatch_jobs job
             WHERE job.assignment_id = ?2 AND job.state = 'completed'
           )",
    )
    .bind(now)
    .bind(assignment_id)
    .bind(device_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    if queued.rows_affected() != 1 {
        return Ok(false);
    }
    sqlx::query(
        "UPDATE chat_assignment_dispatch_jobs
         SET state = 'queued', available_at = ?1, claimed_at = NULL, claim_token = NULL,
             last_error = NULL, updated_at = ?1
         WHERE assignment_id = ?2",
    )
    .bind(now)
    .bind(assignment_id)
    .execute(connection)
    .await
    .map_err(persistence_error)?;
    Ok(true)
}

/// A message posted in a reply thread, as a run receives it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadMessage {
    pub item_id: String,
    pub revision: u64,
    pub markdown: String,
}

/// A host question whose asking run ended and the answers a person posted after it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnsweredQuestion {
    /// The completed run that asked, the latest run of the assignment.
    pub asking_run_id: String,
    pub question: ThreadMessage,
    /// The pending person inputs that answer it, in posting order.
    pub answers: Vec<ThreadMessage>,
}

/// Reads the host question the latest run of `assignment_id` asked before it completed, with
/// the pending inputs a person posted since, or `None` when that run did not ask, did not
/// complete, or no person answered. An input that names a channel, folder, path, or execution
/// environment is left out, since such references need new work.
///
/// # Errors
///
/// Returns a persistence error when a read fails.
pub async fn read_answered_question(
    connection: &mut SqliteConnection,
    assignment_id: &str,
) -> ChatResult<Option<AnsweredQuestion>> {
    let question: Option<(String, String, i64, String)> = sqlx::query_as(
        "SELECT run.id, semantic.item_id, revision.revision, revision.normalized_markdown
         FROM chat_agent_runs run
         JOIN chat_work_semantic_updates semantic
           ON semantic.assignment_id = run.assignment_id AND semantic.update_kind = 'question'
          AND json_extract(semantic.payload_data, '$.agentRunId') = run.id
          AND json_extract(semantic.payload_data, '$.payload.source') = ?2
         JOIN chat_conversation_items item ON item.id = semantic.item_id
         JOIN chat_communication_messages message ON message.item_id = semantic.item_id
         JOIN chat_communication_message_revisions_numbered revision
           ON revision.id = message.current_revision_id
         WHERE run.assignment_id = ?1 AND run.state = 'completed'
           AND run.run_ordinal = (
             SELECT max(latest.run_ordinal) FROM chat_agent_runs latest
             WHERE latest.assignment_id = ?1
           )
         ORDER BY item.order_key DESC, item.id DESC LIMIT 1",
    )
    .bind(assignment_id)
    .bind(HOST_TOOL_SOURCE)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let Some((asking_run_id, item_id, revision, markdown)) = question else {
        return Ok(None);
    };
    let sql = format!(
        "SELECT input.message_item_id, revision.revision, revision.normalized_markdown
         FROM chat_work_assignment_inputs input
         JOIN chat_communication_messages message ON message.item_id = input.message_item_id
         JOIN chat_communication_message_revisions_numbered revision
           ON revision.id = message.current_revision_id
         WHERE input.assignment_id = ?1
           AND input.routing_kind IN ('steer', 'queued_continuation')
           AND input.delivery_state = 'pending'
           AND {PERSON_INPUT_SQL}
           AND NOT EXISTS(
             SELECT 1 FROM chat_message_references reference
             WHERE reference.message_revision_id = message.current_revision_id
               AND reference.reference_kind IN (
                 'channel', 'working_folder', 'workspace_path', 'execution_environment'
               )
           )
         ORDER BY input.ordinal"
    );
    let answers = sqlx::query_as::<_, (String, i64, String)>(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(assignment_id)
        .fetch_all(connection)
        .await
        .map_err(persistence_error)?
        .into_iter()
        .map(|(item_id, revision, markdown)| thread_message(item_id, revision, markdown))
        .collect::<ChatResult<Vec<_>>>()?;
    if answers.is_empty() {
        return Ok(None);
    }
    Ok(Some(AnsweredQuestion {
        asking_run_id,
        question: thread_message(item_id, revision, markdown)?,
        answers,
    }))
}

fn thread_message(item_id: String, revision: i64, markdown: String) -> ChatResult<ThreadMessage> {
    let revision = u64::try_from(revision).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Persistence,
            "Chat message revision is invalid",
            false,
        )
    })?;
    Ok(ThreadMessage {
        item_id,
        revision,
        markdown,
    })
}

/// The Markdown of a host question: the question, then its choices as a list.
fn question_markdown(question: &str, choices: &[String]) -> String {
    if choices.is_empty() {
        return question.to_owned();
    }
    let list = choices
        .iter()
        .map(|choice| format!("- {choice}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{question}\n\n{list}")
}

/// The item and revision ids of a host update, derived from the call.
pub(super) struct HostUpdateIds {
    pub(super) item_id: String,
    pub(super) revision_id: String,
}

impl HostUpdateIds {
    pub(super) fn derive(part: &str, call: HostCall<'_>) -> ChatResult<Self> {
        let parts = [part, call.run_id, call.client_request_id];
        Ok(Self {
            item_id: derived_id("conversation-item", &parts)?,
            revision_id: derived_id("message-revision", &parts)?,
        })
    }
}

/// The outcome of a call whose update `item_id` exists: a replay when its kind and payload
/// match, else a conflict. `None` when no such update exists.
pub(super) async fn replayed(
    connection: &mut SqliteConnection,
    item_id: &str,
    update_kind: &str,
    payload: &Value,
) -> ChatResult<Option<ChatResult<HostCallOutcome>>> {
    let Some(stored) = stored_update(connection, item_id).await? else {
        return Ok(None);
    };
    let matches = stored.update_kind == update_kind && stored.payload.as_ref() == Some(payload);
    Ok(Some(if matches {
        Ok(HostCallOutcome::Replayed)
    } else {
        Err(HostReportRefusal::RequestConflict.into_error())
    }))
}

/// As [`replayed`] for a report, which also compares the stored body.
async fn replayed_report(
    connection: &mut SqliteConnection,
    item_id: &str,
    kind: ReportKind,
    body: &str,
) -> ChatResult<Option<ChatResult<HostCallOutcome>>> {
    let Some(stored) = stored_update(connection, item_id).await? else {
        return Ok(None);
    };
    let matches = stored.update_kind == kind.as_str() && stored.markdown == body;
    Ok(Some(if matches {
        Ok(HostCallOutcome::Replayed)
    } else {
        Err(HostReportRefusal::RequestConflict.into_error())
    }))
}

/// A stored teammate update, as a replay compares it.
struct StoredUpdate {
    update_kind: String,
    payload: Option<Value>,
    markdown: String,
}

async fn stored_update(
    connection: &mut SqliteConnection,
    item_id: &str,
) -> ChatResult<Option<StoredUpdate>> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT semantic.update_kind, semantic.payload_data, revision.normalized_markdown
         FROM chat_work_semantic_updates semantic
         JOIN chat_communication_messages message ON message.item_id = semantic.item_id
         JOIN chat_communication_message_revisions revision
           ON revision.id = message.current_revision_id
         WHERE semantic.item_id = ?",
    )
    .bind(item_id)
    .fetch_optional(connection)
    .await
    .map_err(persistence_error)?;
    let Some((update_kind, payload_data, markdown)) = row else {
        return Ok(None);
    };
    let payload = serde_json::from_str::<Value>(&payload_data)
        .ok()
        .and_then(|mut value| value.get_mut("payload").map(Value::take));
    Ok(Some(StoredUpdate {
        update_kind,
        payload,
        markdown,
    }))
}

/// The reply thread and teammate of a call's run.
pub(super) struct ActiveTarget {
    pub(super) reply_thread_id: String,
    pub(super) teammate_id: String,
}

/// Reads the reply thread and teammate of `call` when its run belongs to its assignment and has
/// not ended, and the assignment is in one of `assignment_states`.
pub(super) async fn read_active_target(
    connection: &mut SqliteConnection,
    call: HostCall<'_>,
    assignment_states: &[&str],
) -> ChatResult<ActiveTarget> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT assignment.reply_thread_id, assignment.teammate_id, assignment.state
         FROM chat_agent_runs run
         JOIN chat_work_assignments assignment ON assignment.id = run.assignment_id
         WHERE run.id = ? AND run.assignment_id = ?
           AND run.state IN ('starting', 'working', 'waiting')",
    )
    .bind(call.run_id)
    .bind(call.assignment_id)
    .fetch_optional(connection)
    .await
    .map_err(persistence_error)?;
    match row {
        Some((reply_thread_id, teammate_id, state))
            if assignment_states.contains(&state.as_str()) =>
        {
            Ok(ActiveTarget {
                reply_thread_id,
                teammate_id,
            })
        }
        _ => Err(HostReportRefusal::AssignmentInactive.into_error()),
    }
}

/// A teammate message with one semantic update, posted in an assignment's reply thread.
pub(crate) struct TeammateUpdate<'a> {
    pub(crate) item_id: &'a str,
    pub(crate) revision_id: &'a str,
    pub(crate) reply_thread_id: &'a str,
    pub(crate) teammate_id: &'a str,
    pub(crate) assignment_id: &'a str,
    /// The run that produced the update, `None` for an update no run produced.
    pub(crate) run_id: Option<&'a str>,
    pub(crate) update_kind: &'a str,
    pub(crate) markdown: &'a str,
    pub(crate) payload: Value,
    pub(crate) created_at: &'a str,
}

/// Inserts `update` as the next message of its reply thread, authored by its teammate, and
/// bumps the thread and conversation. Returns `false` and writes nothing when the item exists.
///
/// # Errors
///
/// Returns a persistence error when a read or write fails, and a validation error when the rich
/// content cannot fit its bound.
pub(crate) async fn insert_teammate_update(
    connection: &mut SqliteConnection,
    update: &TeammateUpdate<'_>,
) -> ChatResult<bool> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_conversation_items WHERE id = ?)")
            .bind(update.item_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(persistence_error)?;
    if exists {
        return Ok(false);
    }
    let conversation_id: String =
        sqlx::query_scalar("SELECT conversation_id FROM chat_reply_threads WHERE id = ?")
            .bind(update.reply_thread_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(persistence_error)?;
    let order_key =
        next_order_key(connection, &conversation_id, Some(update.reply_thread_id)).await?;
    sqlx::query(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         VALUES (?, ?, ?, 'message', ?, ?)",
    )
    .bind(update.item_id)
    .bind(&conversation_id)
    .bind(update.reply_thread_id)
    .bind(&order_key)
    .bind(update.created_at)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_communication_messages
            (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES (?, ?, (SELECT display_name FROM chat_participants WHERE id = ?), ?)",
    )
    .bind(update.item_id)
    .bind(update.teammate_id)
    .bind(update.teammate_id)
    .bind(update.created_at)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let rich_content = json!({
        "type": "agent_update",
        "updateKind": update.update_kind,
        "assignmentId": update.assignment_id,
        "agentRunId": update.run_id,
        "payload": update.payload,
    });
    let payload_data = serde_json::to_string(&rich_content).map_err(|_| serialization_error())?;
    let bounded = bounded_projected_revision(update.markdown, rich_content)?;
    sqlx::query(
        "INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown,
             rich_content_schema_version, rich_content_data, created_at)
         VALUES (?, ?, ?, 1, ?, ?)",
    )
    .bind(update.revision_id)
    .bind(update.item_id)
    .bind(&bounded.normalized_markdown)
    .bind(&bounded.rich_content_data)
    .bind(update.created_at)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query("UPDATE chat_communication_messages SET current_revision_id = ? WHERE item_id = ?")
        .bind(update.revision_id)
        .bind(update.item_id)
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_work_semantic_updates
            (item_id, assignment_id, update_kind, payload_data, created_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(update.item_id)
    .bind(update.assignment_id)
    .bind(update.update_kind)
    .bind(&payload_data)
    .bind(update.created_at)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_reply_threads
         SET last_activity_at = ?, revision = revision + 1, updated_at = ? WHERE id = ?",
    )
    .bind(update.created_at)
    .bind(update.created_at)
    .bind(update.reply_thread_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_conversations
         SET last_activity_at = ?, revision = revision + 1, updated_at = ? WHERE id = ?",
    )
    .bind(update.created_at)
    .bind(update.created_at)
    .bind(&conversation_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(true)
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat data could not be saved or read",
        true,
    )
}

fn serialization_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat teammate update data is invalid",
        false,
    )
}

#[cfg(test)]
pub(super) mod tests;
