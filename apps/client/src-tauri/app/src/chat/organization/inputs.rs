//! Replies routed to an assignment after the message that started it: listing them with their
//! delivery, editing one before delivery, and cancelling one that has not reached a run.

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

/// Columns of one input row with its message's current revision, shared by every read here.
const INPUT_ROW_SELECT: &str = "SELECT input.id, input.message_item_id, input.ordinal,
        input.routing_kind, input.delivery_state, input.delivered_at,
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
/// be edited. The new revision keeps the message's attachments and carries no references: a
/// reference names a range of the old text.
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
        "SELECT input.message_item_id, input.delivery_state, message.author_participant_id,
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

/// Cancels the pending input `input_id` so no run receives it. Its reply stays in the thread.
/// An input that already reached a run, failed, or was cancelled is refused.
pub(super) async fn cancel_assignment_input(
    pool: &SqlitePool,
    input_id: &ChatAssignmentInputId,
) -> ChatResult<ChatAssignmentInputRead> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let cancelled = sqlx::query(
        "UPDATE chat_work_assignment_inputs SET delivery_state = 'ignored'
         WHERE id = ? AND routing_kind <> 'trigger' AND delivery_state = 'pending'",
    )
    .bind(input_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    let read = read_input(&mut transaction, input_id).await?;
    if cancelled == 0 {
        return Err(already_delivered());
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(read)
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

#[cfg(test)]
mod tests;
