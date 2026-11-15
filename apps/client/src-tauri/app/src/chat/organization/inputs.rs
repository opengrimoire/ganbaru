//! Replies routed to an assignment after the message that started it: listing them with their
//! delivery, editing one before delivery, and cancelling one that has not reached a run.
//!
//! A `follow_up` input is the reply that started a follow-up assignment. The executing device
//! freezes its text into that assignment's context package, so another device cannot tell
//! whether an edit would still reach the run: it is listed but never edited or cancelled here,
//! and stopping the assignment it started is what keeps it from running.

use super::common::{insert_revision_copied_attachments, json_object, message_revision_id};
use super::{
    LOCAL_PARTICIPANT_ID, chat_pool, identifier_error, optional_timestamp, persistence_error,
    u64_value,
};
use ganbaru_chat::timeline::{
    MAX_MESSAGE_MARKDOWN_BYTES, MAX_RICH_CONTENT_BYTES, rich_content_too_large,
};
use ganbaru_chat_contracts::models::*;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

/// Characters of an input's message shown in the pending list; the rest stays in the thread.
pub(super) const INPUT_PREVIEW_CHARS: usize = 280;

/// Error detail reason of an edit or cancel refused because the input already left pending.
pub(super) const INPUT_ALREADY_DELIVERED: &str = "input_already_delivered";

/// Error detail reason of an edit or cancel refused because the input started its own work,
/// whose context package carries its text.
pub(super) const INPUT_STARTS_WORK: &str = "input_starts_work";

/// Routing kind of the reply that started a follow-up assignment.
const FOLLOW_UP_ROUTING_KIND: &str = "follow_up";

/// SQL condition, over a conversation item aliased `item`, that holds while the item is a
/// person's reply waiting for running work, or one a person cancelled before it reached a run.
/// Such a reply lives only in the queue of its work: the thread, its reply count, and search
/// leave it out until the work receives it, and a cancelled one never returns. A reply still
/// pending on failed or cancelled work shows where it was posted, since no run will take it
/// until Retry. Replies an agent posted, such as delegated outcomes, always show.
pub(super) const AWAITING_DELIVERY_SQL: &str = "EXISTS (
    SELECT 1 FROM chat_work_assignment_inputs waiting
    JOIN chat_work_assignments waiting_work ON waiting_work.id = waiting.assignment_id
    JOIN chat_communication_messages waiting_message
      ON waiting_message.item_id = waiting.message_item_id
    WHERE waiting.message_item_id = item.id
      AND waiting.routing_kind IN ('steer', 'queued_continuation')
      AND (waiting.delivery_state = 'ignored'
           OR (waiting.delivery_state = 'pending'
               AND waiting_work.state NOT IN ('failed', 'cancelled')))
      AND NOT EXISTS (
        SELECT 1 FROM chat_participants waiting_author
        WHERE waiting_author.id = waiting_message.author_participant_id
          AND waiting_author.participant_kind = 'ai_teammate'))";

/// Moves the reply `message_item_id`, which waited for running work and has just reached it, to
/// the end of its thread at `entered_at`, so the thread shows it after everything that came
/// before that moment. Only the device that delivers the reply writes the placement, and it
/// replicates as one register, so every device shows the reply at that place. A reply an agent
/// posted keeps its place, since it never waited out of view.
pub(super) async fn place_delivered_reply(
    connection: &mut sqlx::SqliteConnection,
    message_item_id: &str,
    entered_at: &str,
) -> ChatResult<()> {
    let agent_authored: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM chat_communication_messages message
            JOIN chat_participants author ON author.id = message.author_participant_id
            WHERE message.item_id = ? AND author.participant_kind = 'ai_teammate')",
    )
    .bind(message_item_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(persistence_error)?;
    if !agent_authored {
        ganbaru_chat::timeline::place_item_at_scope_end(connection, message_item_id, entered_at)
            .await?;
    }
    Ok(())
}

/// Columns of one input row with its message's current revision, shared by every read here.
const INPUT_ROW_SELECT: &str = "SELECT input.id, input.message_item_id, input.ordinal,
        input.routing_kind, input.delivery_state, input.held, input.delivered_at,
        input.delivered_revision_id, message.current_revision_id,
        message.author_participant_id, revision.normalized_markdown
     FROM chat_work_assignment_inputs input
     JOIN chat_communication_messages message ON message.item_id = input.message_item_id
     LEFT JOIN chat_communication_message_revisions revision
       ON revision.id = message.current_revision_id";

/// Lists the replies routed to `assignment_id` in routing order, leaving out the message that
/// started it, whose text the assignment already carries.
pub(super) async fn list_assignment_inputs(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<Vec<ChatAssignmentInputRead>> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "{INPUT_ROW_SELECT}
         WHERE input.assignment_id = ? AND input.routing_kind <> 'trigger'
         ORDER BY input.ordinal, input.id"
    )))
    .bind(assignment_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?
    .iter()
    .map(input_read)
    .collect()
}

/// Replaces the text of the pending input `input_id` with a new revision of its message, so the
/// run receives the edit when the input is delivered. Only the local person's own replies can
/// be edited, and never a `follow_up` input, whose text its assignment's package carries. The
/// new revision keeps the message's attachments and carries no references: a reference names a
/// range of the old text.
pub(super) async fn edit_assignment_input(
    pool: &SqlitePool,
    input_id: &ChatAssignmentInputId,
    edit: &ChatAssignmentInputEditRequest,
    now: &UtcTimestamp,
) -> ChatResult<ChatAssignmentInputRead> {
    let markdown = edit.normalized_markdown.trim();
    if markdown.len() > MAX_MESSAGE_MARKDOWN_BYTES {
        return Err(ChatError::validation(
            "normalizedMarkdown",
            "Message content exceeds the 128 KiB limit",
        ));
    }
    let rich_content = json_object(&edit.rich_content, "richContent")?;
    if rich_content.len() > MAX_RICH_CONTENT_BYTES {
        return Err(rich_content_too_large());
    }
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let row = sqlx::query(
        "SELECT input.message_item_id, input.routing_kind, input.delivery_state,
                message.author_participant_id,
                message.current_revision_id,
                (SELECT count(*) FROM chat_communication_attachment_references attachment
                 WHERE attachment.message_revision_id = message.current_revision_id)
                    AS attachment_count
         FROM chat_work_assignment_inputs input
         JOIN chat_communication_messages message ON message.item_id = input.message_item_id
         WHERE input.id = ? AND input.routing_kind <> 'trigger' AND message.deleted_at IS NULL",
    )
    .bind(input_id.as_str())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(persistence_error)?
    .ok_or_else(input_not_found)?;
    let message_item_id: String = row.try_get("message_item_id").map_err(persistence_error)?;
    let routing_kind: String = row.try_get("routing_kind").map_err(persistence_error)?;
    let delivery_state: String = row.try_get("delivery_state").map_err(persistence_error)?;
    let author: Option<String> = row
        .try_get("author_participant_id")
        .map_err(persistence_error)?;
    let previous_revision_id: Option<String> = row
        .try_get("current_revision_id")
        .map_err(persistence_error)?;
    let attachment_count: i64 = row.try_get("attachment_count").map_err(persistence_error)?;
    if author.as_deref() != Some(LOCAL_PARTICIPANT_ID) {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Only the author of a reply can edit it",
            false,
        ));
    }
    if delivery_state != "pending" {
        return Err(already_delivered());
    }
    if routing_kind == FOLLOW_UP_ROUTING_KIND {
        return Err(starts_work());
    }
    if markdown.is_empty() && attachment_count == 0 {
        return Err(ChatError::validation(
            "normalizedMarkdown",
            "Message content is required",
        ));
    }
    let revision_id = message_revision_id()?;
    sqlx::query(
        "INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown,
             rich_content_schema_version, rich_content_data, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(revision_id.as_str())
    .bind(&message_item_id)
    .bind(markdown)
    .bind(i64::from(edit.rich_content.schema_version))
    .bind(&rich_content)
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if let Some(previous_revision_id) = &previous_revision_id {
        insert_revision_copied_attachments(
            &mut transaction,
            previous_revision_id,
            revision_id.as_str(),
        )
        .await?;
    }
    sqlx::query(
        "UPDATE chat_communication_messages SET current_revision_id = ?, edited_at = ?
         WHERE item_id = ?",
    )
    .bind(revision_id.as_str())
    .bind(now.as_str())
    .bind(&message_item_id)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let read = read_input(&mut transaction, input_id).await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(read)
}

/// Cancels the pending input `input_id` so no run receives it. A person's cancelled reply
/// leaves the thread with it, as [`AWAITING_DELIVERY_SQL`] reads.
/// An input that already reached a run, failed, or was cancelled is refused, and so is a
/// `follow_up` input, which only stopping the work it started keeps from the run.
pub(super) async fn cancel_assignment_input(
    pool: &SqlitePool,
    input_id: &ChatAssignmentInputId,
) -> ChatResult<ChatAssignmentInputRead> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let cancelled = sqlx::query(
        "UPDATE chat_work_assignment_inputs SET delivery_state = 'ignored'
         WHERE id = ? AND routing_kind NOT IN ('trigger', ?) AND delivery_state = 'pending'",
    )
    .bind(input_id.as_str())
    .bind(FOLLOW_UP_ROUTING_KIND)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    let read = read_input(&mut transaction, input_id).await?;
    if cancelled == 0 {
        return Err(refusal_of_settled(&read));
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(read)
}

/// Holds the pending reply `input_id` back from delivery, or resumes it when `held` is false. A
/// held reply keeps its place in the queue, and every delivery path skips it until it resumes.
/// A `follow_up` input is refused, since the work it started already carries its text.
pub(super) async fn set_assignment_input_held(
    pool: &SqlitePool,
    input_id: &ChatAssignmentInputId,
    held: bool,
) -> ChatResult<ChatAssignmentInputRead> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let changed = sqlx::query(
        "UPDATE chat_work_assignment_inputs SET held = ?
         WHERE id = ? AND routing_kind IN ('steer', 'queued_continuation')
           AND delivery_state = 'pending'",
    )
    .bind(i64::from(held))
    .bind(input_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    let read = read_input(&mut transaction, input_id).await?;
    if changed == 0 {
        return Err(refusal_of_settled(&read));
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(read)
}

/// Routes the pending reply `input_id` into the current turn of its work, ahead of the queue,
/// and resumes it when it was held. Work with no running turn receives it when its next turn
/// starts, as any steer.
pub(super) async fn steer_assignment_input(
    pool: &SqlitePool,
    input_id: &ChatAssignmentInputId,
) -> ChatResult<ChatAssignmentInputRead> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let changed = sqlx::query(
        "UPDATE chat_work_assignment_inputs SET routing_kind = 'steer', held = 0
         WHERE id = ? AND routing_kind IN ('steer', 'queued_continuation')
           AND delivery_state = 'pending'",
    )
    .bind(input_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    let read = read_input(&mut transaction, input_id).await?;
    if changed == 0 {
        return Err(refusal_of_settled(&read));
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(read)
}

/// Puts the pending replies of `assignment_id` in the order of `ordered_input_ids`, which must
/// name exactly the replies still waiting there. They take the ordinal slots they already held,
/// raised where needed to stay strictly increasing, so the inputs around them keep their place
/// and the order stays a total one on every device.
pub(super) async fn reorder_assignment_inputs(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    ordered_input_ids: &[ChatAssignmentInputId],
) -> ChatResult<Vec<ChatAssignmentInputRead>> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let waiting = sqlx::query(
        "SELECT id, ordinal FROM chat_work_assignment_inputs
         WHERE assignment_id = ? AND routing_kind IN ('steer', 'queued_continuation')
           AND delivery_state = 'pending'
         ORDER BY ordinal, id",
    )
    .bind(assignment_id.as_str())
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let mut slots = Vec::with_capacity(waiting.len());
    let mut waiting_ids = Vec::with_capacity(waiting.len());
    for row in &waiting {
        slots.push(
            row.try_get::<i64, _>("ordinal")
                .map_err(persistence_error)?,
        );
        waiting_ids.push(row.try_get::<String, _>("id").map_err(persistence_error)?);
    }
    let mut requested = ordered_input_ids
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect::<Vec<_>>();
    let mut current = waiting_ids.clone();
    requested.sort_unstable();
    current.sort_unstable();
    if requested != current {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The waiting replies changed before the reorder",
            true,
        ));
    }
    for (input_id, ordinal) in ordered_input_ids.iter().zip(reorder_ordinals(&slots)?) {
        sqlx::query(
            "UPDATE chat_work_assignment_inputs SET ordinal = ?
             WHERE id = ? AND ordinal IS NOT ?",
        )
        .bind(ordinal)
        .bind(input_id.as_str())
        .bind(ordinal)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    list_assignment_inputs(pool, assignment_id).await
}

/// Strictly increasing ordinals for reordered inputs that held the ascending `slots`: each takes
/// its slot, or one past the previous ordinal when that slot would tie or go backwards.
pub(super) fn reorder_ordinals(slots: &[i64]) -> ChatResult<Vec<i64>> {
    let mut ordinals = Vec::with_capacity(slots.len());
    let mut previous: Option<i64> = None;
    for &slot in slots {
        let next = match previous {
            Some(previous) => slot.max(previous.checked_add(1).ok_or_else(|| {
                ChatError::new(ChatErrorCode::Validation, "Too many waiting replies", true)
            })?),
            None => slot,
        };
        ordinals.push(next);
        previous = Some(next);
    }
    Ok(ordinals)
}

pub(super) async fn list_assignment_inputs_command(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: ChatWorkAssignmentId,
) -> ChatResult<Vec<ChatAssignmentInputRead>> {
    let pool = chat_pool(app, db_url).await?;
    list_assignment_inputs(&pool, &assignment_id).await
}

pub(super) async fn edit_assignment_input_command(
    app: tauri::AppHandle,
    db_url: String,
    input_id: ChatAssignmentInputId,
    edit: ChatAssignmentInputEditRequest,
) -> ChatResult<ChatAssignmentInputRead> {
    let pool = chat_pool(app, db_url).await?;
    edit_assignment_input(&pool, &input_id, &edit, &super::now_timestamp()?).await
}

pub(super) async fn cancel_assignment_input_command(
    app: tauri::AppHandle,
    db_url: String,
    input_id: ChatAssignmentInputId,
) -> ChatResult<ChatAssignmentInputRead> {
    let pool = chat_pool(app, db_url).await?;
    cancel_assignment_input(&pool, &input_id).await
}

pub(super) async fn set_assignment_input_held_command(
    app: tauri::AppHandle,
    db_url: String,
    input_id: ChatAssignmentInputId,
    held: bool,
) -> ChatResult<ChatAssignmentInputRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let read = set_assignment_input_held(&pool, &input_id, held).await?;
    if !held {
        continue_released_input(&app, &pool, &input_id).await?;
    }
    Ok(read)
}

pub(super) async fn steer_assignment_input_command(
    app: tauri::AppHandle,
    db_url: String,
    input_id: ChatAssignmentInputId,
) -> ChatResult<ChatAssignmentInputRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let read = steer_assignment_input(&pool, &input_id).await?;
    continue_released_input(&app, &pool, &input_id).await?;
    Ok(read)
}

pub(super) async fn reorder_assignment_inputs_command(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: ChatWorkAssignmentId,
    ordered_input_ids: Vec<ChatAssignmentInputId>,
) -> ChatResult<Vec<ChatAssignmentInputRead>> {
    let pool = chat_pool(app, db_url).await?;
    reorder_assignment_inputs(&pool, &assignment_id, &ordered_input_ids).await
}

/// Hands a reply that just became deliverable to the path sync takes for routed replies: a
/// steer reaches the run this device holds, and a reply left behind a turn that already ended
/// continues the work in a follow-up. Work another device executes picks the change up when it
/// replicates there.
async fn continue_released_input(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    input_id: &ChatAssignmentInputId,
) -> ChatResult<()> {
    let assignment_id: Option<String> =
        sqlx::query_scalar("SELECT assignment_id FROM chat_work_assignment_inputs WHERE id = ?")
            .bind(input_id.as_str())
            .fetch_optional(pool)
            .await
            .map_err(persistence_error)?;
    #[cfg(desktop)]
    if let Some(assignment_id) = assignment_id {
        super::assignments::start_synced_assignment_work(
            app,
            crate::chat::sync::SyncedAssignmentWork {
                routed: std::collections::BTreeSet::from([assignment_id]),
                ..Default::default()
            },
        );
    }
    #[cfg(mobile)]
    let _ = (app, assignment_id);
    Ok(())
}

async fn read_input(
    connection: &mut sqlx::SqliteConnection,
    input_id: &ChatAssignmentInputId,
) -> ChatResult<ChatAssignmentInputRead> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{INPUT_ROW_SELECT} WHERE input.id = ? AND input.routing_kind <> 'trigger'"
    )))
    .bind(input_id.as_str())
    .fetch_optional(connection)
    .await
    .map_err(persistence_error)?
    .ok_or_else(input_not_found)?;
    input_read(&row)
}

fn input_read(row: &SqliteRow) -> ChatResult<ChatAssignmentInputRead> {
    let markdown: Option<String> = row
        .try_get("normalized_markdown")
        .map_err(persistence_error)?;
    let (preview, preview_truncated) = preview(markdown.as_deref().unwrap_or_default());
    Ok(ChatAssignmentInputRead {
        input_id: ChatAssignmentInputId::new(
            row.try_get::<String, _>("id").map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        message_item_id: ChatConversationItemId::new(
            row.try_get::<String, _>("message_item_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        ordinal: u64_value(row.try_get("ordinal").map_err(persistence_error)?)?,
        routing_kind: row.try_get("routing_kind").map_err(persistence_error)?,
        delivery_state: row.try_get("delivery_state").map_err(persistence_error)?,
        held: row.try_get::<i64, _>("held").map_err(persistence_error)? != 0,
        delivered_at: optional_timestamp(row.try_get("delivered_at").map_err(persistence_error)?)?,
        delivered_revision_id: optional_revision_id(
            row.try_get("delivered_revision_id")
                .map_err(persistence_error)?,
        )?,
        current_revision_id: optional_revision_id(
            row.try_get("current_revision_id")
                .map_err(persistence_error)?,
        )?,
        preview,
        preview_truncated,
        author_participant_id: row
            .try_get::<Option<String>, _>("author_participant_id")
            .map_err(persistence_error)?
            .map(ChatParticipantId::new)
            .transpose()
            .map_err(identifier_error)?,
    })
}

fn optional_revision_id(value: Option<String>) -> ChatResult<Option<ChatMessageRevisionId>> {
    value
        .map(ChatMessageRevisionId::new)
        .transpose()
        .map_err(identifier_error)
}

/// The first `INPUT_PREVIEW_CHARS` characters of `markdown`, and whether any were left out.
fn preview(markdown: &str) -> (String, bool) {
    match markdown.char_indices().nth(INPUT_PREVIEW_CHARS) {
        Some((end, _)) => (markdown[..end].to_string(), true),
        None => (markdown.to_string(), false),
    }
}

/// Why a change to the waiting reply `read` was refused: it started its own work, or it already
/// left pending.
fn refusal_of_settled(read: &ChatAssignmentInputRead) -> ChatError {
    if read.routing_kind == FOLLOW_UP_ROUTING_KIND && read.delivery_state == "pending" {
        starts_work()
    } else {
        already_delivered()
    }
}

fn input_not_found() -> ChatError {
    ChatError::new(ChatErrorCode::NotFound, "Reply input was not found", false)
}

/// A `Conflict` for an input that already left pending, which the client shows as "Already
/// delivered".
fn already_delivered() -> ChatError {
    let mut error = ChatError::new(
        ChatErrorCode::Conflict,
        "This reply already reached the teammate or was cancelled",
        false,
    );
    error.details = Some(Box::new(
        serde_json::json!({ "reason": INPUT_ALREADY_DELIVERED }),
    ));
    error
}

/// A `Conflict` for a `follow_up` input, whose text the work it started already carries, so
/// the client stops that work instead.
fn starts_work() -> ChatError {
    let mut error = ChatError::new(
        ChatErrorCode::Conflict,
        "This reply started its own work; stop that work instead",
        false,
    );
    error.details = Some(Box::new(serde_json::json!({ "reason": INPUT_STARTS_WORK })));
    error
}

#[cfg(test)]
mod tests;
