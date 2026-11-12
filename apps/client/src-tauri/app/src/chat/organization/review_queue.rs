//! The review queue: assignments in every channel that wait for the person to answer, approve,
//! review, or look at a recent failure, read from replicated rows so every device can show it.

use super::common::this_device_id;
use crate::chat::channels::{
    ASSIGNMENT_ATTENTION_PREDICATE_SQL, ASSIGNMENT_ATTENTION_PRIORITY_SQL,
    PERSON_ATTENTION_STATES_SQL, chat_pool, current_failure_window_start, identifier_error,
    persistence_error, timestamp, u64_value, validate_project_id,
};
use ganbaru_chat_contracts::models::*;
use serde::Deserialize;
use sqlx::{AssertSqlSafe, Row, SqlitePool};
use std::collections::{BTreeMap, BTreeSet};

/// Most items one review queue read returns.
pub(crate) const MAX_REVIEW_QUEUE_ITEMS: u32 = 200;

/// Most characters of a state reason an item carries before it is clipped.
pub(crate) const MAX_REVIEW_QUEUE_REASON_CHARS: usize = 240;

/// Joins and filters shared by the item read and the count. It binds the failure window start
/// and then the optional project id twice.
fn review_queue_from_sql() -> String {
    format!(
        "FROM chat_work_assignments assignment
         JOIN chat_reply_threads thread ON thread.id = assignment.reply_thread_id
         JOIN chat_channels channel ON channel.conversation_id = thread.conversation_id
         JOIN projects project ON project.id = channel.project_id
         JOIN chat_participants participant ON participant.id = assignment.teammate_id
         WHERE assignment.state IN ({PERSON_ATTENTION_STATES_SQL})
           AND {ASSIGNMENT_ATTENTION_PREDICATE_SQL}
           AND channel.archived_at IS NULL
           AND project.status <> 'archived'
           AND (? IS NULL OR channel.project_id = ?)"
    )
}

/// Reads the review queue on this device, across every project or within `project_id`.
pub(super) async fn list_review_queue(
    app: tauri::AppHandle,
    db_url: String,
    project_id: Option<String>,
) -> ChatResult<ChatReviewQueueRead> {
    if let Some(project_id) = project_id.as_deref() {
        validate_project_id(project_id)?;
    }
    let device_id = this_device_id(&app)?;
    let pool = chat_pool(app, db_url).await?;
    read_review_queue(
        &pool,
        &device_id,
        project_id.as_deref(),
        &current_failure_window_start(),
    )
    .await
}

/// Reads the review queue as `device_id` sees it, with failures settled before
/// `failure_window_start` left out.
pub(crate) async fn read_review_queue(
    pool: &SqlitePool,
    device_id: &str,
    project_id: Option<&str>,
    failure_window_start: &str,
) -> ChatResult<ChatReviewQueueRead> {
    let from = review_queue_from_sql();
    let count_sql = format!("SELECT count(*) {from}");
    let total: i64 = sqlx::query_scalar(AssertSqlSafe(count_sql))
        .bind(failure_window_start)
        .bind(project_id)
        .bind(project_id)
        .fetch_one(pool)
        .await
        .map_err(persistence_error)?;
    let items_sql = format!(
        "SELECT assignment.id, assignment.revision, assignment.state, assignment.state_reason,
                assignment.executing_device_id, assignment.updated_at,
                assignment.reply_thread_id, assignment.teammate_id,
                participant.display_name AS teammate_display_name,
                channel.id AS channel_id, channel.name AS channel_name,
                project.id AS project_id, project.name AS project_name
         {from}
         ORDER BY {ASSIGNMENT_ATTENTION_PRIORITY_SQL}, assignment.updated_at DESC, assignment.id
         LIMIT ?"
    );
    let rows = sqlx::query(AssertSqlSafe(items_sql))
        .bind(failure_window_start)
        .bind(project_id)
        .bind(project_id)
        .bind(i64::from(MAX_REVIEW_QUEUE_ITEMS))
        .fetch_all(pool)
        .await
        .map_err(persistence_error)?;
    let assignment_ids = rows
        .iter()
        .map(|row| row.try_get::<String, _>("id"))
        .collect::<Result<Vec<_>, _>>()
        .map_err(persistence_error)?;
    let changed_files = changed_file_counts(pool, &assignment_ids).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(item_from_row(&row, device_id, &changed_files)?);
    }
    let total = u32::try_from(total).unwrap_or(u32::MAX);
    let read = u32::try_from(items.len()).unwrap_or(u32::MAX);
    Ok(ChatReviewQueueRead {
        items,
        total,
        truncated: total > read,
    })
}

fn item_from_row(
    row: &sqlx::sqlite::SqliteRow,
    device_id: &str,
    changed_files: &BTreeMap<String, u32>,
) -> ChatResult<ChatReviewQueueItemRead> {
    let assignment_id: String = row.try_get("id").map_err(persistence_error)?;
    let state: String = row.try_get("state").map_err(persistence_error)?;
    let kind = queue_kind(&state)?;
    let executing_device_id: Option<String> = row
        .try_get("executing_device_id")
        .map_err(persistence_error)?;
    let state_reason: Option<String> = row.try_get("state_reason").map_err(persistence_error)?;
    Ok(ChatReviewQueueItemRead {
        changed_file_count: changed_files.get(&assignment_id).copied().unwrap_or(0),
        assignment_id: ChatWorkAssignmentId::new(assignment_id).map_err(identifier_error)?,
        revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
        kind,
        project_id: row.try_get("project_id").map_err(persistence_error)?,
        project_name: row.try_get("project_name").map_err(persistence_error)?,
        channel_id: ChatChannelId::new(
            row.try_get::<String, _>("channel_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        channel_name: row.try_get("channel_name").map_err(persistence_error)?,
        reply_thread_id: ChatReplyThreadId::new(
            row.try_get::<String, _>("reply_thread_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        teammate_participant_id: ChatParticipantId::new(
            row.try_get::<String, _>("teammate_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        teammate_display_name: row
            .try_get("teammate_display_name")
            .map_err(persistence_error)?,
        state_reason: state_reason.map(|reason| clip_reason(&reason)),
        executed_here: executing_device_id.as_deref() == Some(device_id),
        updated_at: timestamp(row.try_get("updated_at").map_err(persistence_error)?)?,
    })
}

fn queue_kind(state: &str) -> ChatResult<ChatReviewQueueKind> {
    match state {
        "waiting_for_answer" => Ok(ChatReviewQueueKind::Answer),
        "waiting_for_approval" => Ok(ChatReviewQueueKind::Approval),
        "ready_for_review" => Ok(ChatReviewQueueKind::Review),
        "failed" => Ok(ChatReviewQueueKind::Failure),
        _ => Err(ChatError::new(
            ChatErrorCode::Persistence,
            "A review queue item has an unexpected state",
            false,
        )),
    }
}

/// `reason` cut to [`MAX_REVIEW_QUEUE_REASON_CHARS`] characters, with `…` after a cut.
fn clip_reason(reason: &str) -> String {
    match reason.char_indices().nth(MAX_REVIEW_QUEUE_REASON_CHARS) {
        Some((end, _)) => format!("{}…", &reason[..end]),
        None => reason.to_owned(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewPayload {
    #[serde(default)]
    changed_files: Vec<ChangedFilePath>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangedFilePath {
    relative_path: String,
}

/// Distinct changed file paths across the `review` semantic updates of each assignment in
/// `assignment_ids`, read in one query.
async fn changed_file_counts(
    pool: &SqlitePool,
    assignment_ids: &[String],
) -> ChatResult<BTreeMap<String, u32>> {
    if assignment_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let placeholders = vec!["?"; assignment_ids.len()].join(", ");
    let sql = format!(
        "SELECT assignment_id, payload_data FROM chat_work_semantic_updates
         WHERE update_kind = 'review' AND assignment_id IN ({placeholders})"
    );
    let mut query = sqlx::query(AssertSqlSafe(sql));
    for assignment_id in assignment_ids {
        query = query.bind(assignment_id);
    }
    let rows = query.fetch_all(pool).await.map_err(persistence_error)?;
    let mut paths = BTreeMap::<String, BTreeSet<String>>::new();
    for row in rows {
        let assignment_id: String = row.try_get("assignment_id").map_err(persistence_error)?;
        let payload: String = row.try_get("payload_data").map_err(persistence_error)?;
        // Review payloads are validated when written, so an unreadable one came from a newer or
        // damaged writer. It adds no paths rather than hiding the whole queue.
        let Ok(payload) = serde_json::from_str::<ReviewPayload>(&payload) else {
            continue;
        };
        paths.entry(assignment_id).or_default().extend(
            payload
                .changed_files
                .into_iter()
                .map(|file| file.relative_path),
        );
    }
    Ok(paths
        .into_iter()
        .map(|(assignment_id, paths)| {
            (
                assignment_id,
                u32::try_from(paths.len()).unwrap_or(u32::MAX),
            )
        })
        .collect())
}

#[cfg(all(test, desktop))]
mod tests;
