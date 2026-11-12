//! Channel notes: short facts a person saves in a channel for the people and teammates in it.
//!
//! Only a person writes a note, and only a current member of the channel's conversation; a
//! teammate can only propose one for a person to settle. Every
//! write records an organizational command receipt keyed by its client command id in the same
//! transaction, so repeating a command returns its first result instead of writing again.

use super::LOCAL_PARTICIPANT_ID;
use super::common::map_command_receipt_error;
use crate::chat::channels::{
    i64_value, identifier_error, now_timestamp, persistence_error, timestamp, u64_value,
};
use ganbaru_chat::ids::new_id;
use ganbaru_chat_contracts::models::{
    ChatChannelId, ChatChannelNoteId, ChatChannelNoteRead, ChatCommandId, ChatConversationItemId,
    ChatError, ChatErrorCode, ChatParticipantId, ChatResult,
};
use ganbaru_sync_contracts::chat_channel_notes::MAX_CHANNEL_NOTE_CHARS;
use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, Row, SqliteConnection, SqlitePool};

mod proposals;

pub use proposals::{
    AcceptChatNoteProposalCommand, ChatNoteProposalStateRead, DismissChatNoteProposalCommand,
};
pub(super) use proposals::{
    accept_note_proposal, dismiss_note_proposal, read_note_proposal_states,
};

/// Most live notes a person may keep in one channel. Merges from other devices may exceed it.
pub const MAX_ACTIVE_CHANNEL_NOTES: i64 = 50;

/// Id prefix of a note a person writes.
const CHANNEL_NOTE_ID_PREFIX: &str = "channel-note";

/// Receipt kind of every channel note command.
const CHANNEL_NOTE_RECEIPT_KIND: &str = "channel_note";

/// Schema version of a channel note receipt's result.
const RECEIPT_RESULT_SCHEMA_VERSION: i64 = 1;

/// A person saves a new note in a channel.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateChatChannelNoteCommand {
    pub client_command_id: ChatCommandId,
    pub channel_id: ChatChannelId,
    pub body: String,
}

/// A person rewords a note at the revision they saw.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateChatChannelNoteCommand {
    pub client_command_id: ChatCommandId,
    pub note_id: ChatChannelNoteId,
    pub expected_revision: u64,
    pub body: String,
}

/// A person deletes a note at the revision they saw.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteChatChannelNoteCommand {
    pub client_command_id: ChatCommandId,
    pub note_id: ChatChannelNoteId,
    pub expected_revision: u64,
}

/// The channel note command a receipt records, so a repeated id replays only the same command.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum NoteAction {
    Create,
    Update,
    Delete,
    Accept,
    Dismiss,
}

/// The stored result of a channel note command.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct NoteReceipt {
    action: NoteAction,
    note_id: String,
}

/// Lists the live notes of `channel_id`, newest updated first, for a member of its conversation.
///
/// # Errors
///
/// Returns a not found error for an unknown channel, a permission error when the local person
/// is not a member, and a persistence error when a read fails.
pub(super) async fn list_channel_notes(
    pool: &SqlitePool,
    channel_id: &ChatChannelId,
) -> ChatResult<Vec<ChatChannelNoteRead>> {
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    require_member(&mut connection, channel_id).await?;
    let rows = sqlx::query(AssertSqlSafe(format!(
        "{NOTE_COLUMNS} WHERE channel_id = ? AND deleted_at IS NULL
         ORDER BY updated_at DESC, id DESC"
    )))
    .bind(channel_id.as_str())
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    rows.iter().map(note_from_row).collect()
}

/// Saves `request`'s note in its channel as written by the local person.
///
/// # Errors
///
/// Returns a validation error for a blank or oversize body or a channel at
/// [`MAX_ACTIVE_CHANNEL_NOTES`], a permission error when the local person is not a member, a
/// conflict when the channel is archived or the command id named another command, and a
/// persistence error when a read or write fails.
pub(super) async fn create_channel_note(
    pool: &SqlitePool,
    request: &CreateChatChannelNoteCommand,
) -> ChatResult<ChatChannelNoteRead> {
    let body = normalized_note_body(&request.body)?;
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    if let Some(note_id) = replayed_note(
        &mut transaction,
        &request.client_command_id,
        NoteAction::Create,
    )
    .await?
    {
        return read_note(&mut transaction, &note_id).await;
    }
    require_writable_member(&mut transaction, &request.channel_id).await?;
    require_room_for_a_note(&mut transaction, &request.channel_id).await?;
    let note_id =
        ChatChannelNoteId::new(new_id(CHANNEL_NOTE_ID_PREFIX)?).map_err(identifier_error)?;
    let now = now_timestamp()?;
    sqlx::query(
        "INSERT INTO chat_channel_notes
            (id, channel_id, body, edited_by_participant_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(note_id.as_str())
    .bind(request.channel_id.as_str())
    .bind(&body)
    .bind(LOCAL_PARTICIPANT_ID)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    record_receipt(
        &mut transaction,
        &request.client_command_id,
        NoteAction::Create,
        &note_id,
    )
    .await?;
    let note = read_note(&mut transaction, &note_id).await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(note)
}

/// Rewords `request`'s note when it is still at the revision the person saw.
///
/// # Errors
///
/// Returns a validation error for a blank or oversize body, a not found error for a missing or
/// deleted note, a stale revision error when the note changed, a permission error when the
/// local person is not a member, a conflict when the channel is archived or the command id
/// named another command, and a persistence error when a read or write fails.
pub(super) async fn update_channel_note(
    pool: &SqlitePool,
    request: &UpdateChatChannelNoteCommand,
) -> ChatResult<ChatChannelNoteRead> {
    let body = normalized_note_body(&request.body)?;
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    if let Some(note_id) = replayed_note(
        &mut transaction,
        &request.client_command_id,
        NoteAction::Update,
    )
    .await?
    {
        return read_note(&mut transaction, &note_id).await;
    }
    let channel_id = live_note_channel(&mut transaction, &request.note_id).await?;
    require_writable_member(&mut transaction, &channel_id).await?;
    let now = now_timestamp()?;
    let updated = sqlx::query(
        "UPDATE chat_channel_notes
         SET body = ?, edited_by_participant_id = ?, revision = revision + 1, updated_at = ?
         WHERE id = ? AND revision = ? AND deleted_at IS NULL",
    )
    .bind(&body)
    .bind(LOCAL_PARTICIPANT_ID)
    .bind(now.as_str())
    .bind(request.note_id.as_str())
    .bind(i64_value(request.expected_revision)?)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    require_one_row(updated.rows_affected())?;
    record_receipt(
        &mut transaction,
        &request.client_command_id,
        NoteAction::Update,
        &request.note_id,
    )
    .await?;
    let note = read_note(&mut transaction, &request.note_id).await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(note)
}

/// Deletes `request`'s note when it is still at the revision the person saw. The row stays as a
/// deletion so the delete wins over an edit another device makes at the same time.
///
/// # Errors
///
/// Returns a not found error for a missing or deleted note, a stale revision error when the
/// note changed, a permission error when the local person is not a member, a conflict when the
/// channel is archived or the command id named another command, and a persistence error when a
/// read or write fails.
pub(super) async fn delete_channel_note(
    pool: &SqlitePool,
    request: &DeleteChatChannelNoteCommand,
) -> ChatResult<()> {
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    if replayed_note(
        &mut transaction,
        &request.client_command_id,
        NoteAction::Delete,
    )
    .await?
    .is_some()
    {
        return Ok(());
    }
    let channel_id = live_note_channel(&mut transaction, &request.note_id).await?;
    require_writable_member(&mut transaction, &channel_id).await?;
    let now = now_timestamp()?;
    let deleted = sqlx::query(
        "UPDATE chat_channel_notes
         SET deleted_at = ?, edited_by_participant_id = ?, revision = revision + 1,
             updated_at = ?
         WHERE id = ? AND revision = ? AND deleted_at IS NULL",
    )
    .bind(now.as_str())
    .bind(LOCAL_PARTICIPANT_ID)
    .bind(now.as_str())
    .bind(request.note_id.as_str())
    .bind(i64_value(request.expected_revision)?)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    require_one_row(deleted.rows_affected())?;
    record_receipt(
        &mut transaction,
        &request.client_command_id,
        NoteAction::Delete,
        &request.note_id,
    )
    .await?;
    transaction.commit().await.map_err(persistence_error)
}

/// The trimmed note body, refused when blank, over [`MAX_CHANNEL_NOTE_CHARS`] characters, or
/// holding a control character other than a line break or tab.
///
/// # Errors
///
/// Returns a validation error on `body`.
pub(super) fn normalized_note_body(body: &str) -> ChatResult<String> {
    let body = body.trim();
    if body.is_empty() {
        return Err(ChatError::validation(
            "body",
            "Write the note before saving it",
        ));
    }
    if body.chars().count() > MAX_CHANNEL_NOTE_CHARS {
        return Err(ChatError::validation(
            "body",
            format!("A channel note cannot exceed {MAX_CHANNEL_NOTE_CHARS} characters"),
        ));
    }
    if body
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
    {
        return Err(ChatError::validation(
            "body",
            "A channel note cannot contain control characters",
        ));
    }
    Ok(body.to_owned())
}

/// Columns of a note read, ahead of its `WHERE` clause.
pub(super) const NOTE_COLUMNS: &str = "SELECT id, channel_id, body, source_item_id,
        proposed_by_participant_id, edited_by_participant_id, revision, created_at, updated_at
     FROM chat_channel_notes";

/// Reads note `note_id`, deleted or not.
///
/// # Errors
///
/// Returns a not found error when the row is absent and a persistence error when the read
/// fails.
pub(super) async fn read_note(
    connection: &mut SqliteConnection,
    note_id: &ChatChannelNoteId,
) -> ChatResult<ChatChannelNoteRead> {
    let row = sqlx::query(AssertSqlSafe(format!("{NOTE_COLUMNS} WHERE id = ?")))
        .bind(note_id.as_str())
        .fetch_optional(connection)
        .await
        .map_err(persistence_error)?
        .ok_or_else(note_not_found)?;
    note_from_row(&row)
}

fn note_from_row(row: &sqlx::sqlite::SqliteRow) -> ChatResult<ChatChannelNoteRead> {
    let participant = |column: &str| -> ChatResult<Option<ChatParticipantId>> {
        row.try_get::<Option<String>, _>(column)
            .map_err(persistence_error)?
            .map(ChatParticipantId::new)
            .transpose()
            .map_err(identifier_error)
    };
    Ok(ChatChannelNoteRead {
        id: ChatChannelNoteId::new(row.try_get::<String, _>("id").map_err(persistence_error)?)
            .map_err(identifier_error)?,
        channel_id: ChatChannelId::new(
            row.try_get::<String, _>("channel_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        body: row.try_get("body").map_err(persistence_error)?,
        source_item_id: row
            .try_get::<Option<String>, _>("source_item_id")
            .map_err(persistence_error)?
            .map(ChatConversationItemId::new)
            .transpose()
            .map_err(identifier_error)?,
        proposed_by: participant("proposed_by_participant_id")?,
        edited_by: participant("edited_by_participant_id")?,
        revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
        created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
        updated_at: timestamp(row.try_get("updated_at").map_err(persistence_error)?)?,
    })
}

/// The channel of live note `note_id`.
async fn live_note_channel(
    connection: &mut SqliteConnection,
    note_id: &ChatChannelNoteId,
) -> ChatResult<ChatChannelId> {
    let channel_id: Option<String> = sqlx::query_scalar(
        "SELECT channel_id FROM chat_channel_notes WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(note_id.as_str())
    .fetch_optional(connection)
    .await
    .map_err(persistence_error)?;
    ChatChannelId::new(channel_id.ok_or_else(note_not_found)?).map_err(identifier_error)
}

/// Refuses unless `channel_id` exists and the local person is a current member of its
/// conversation. Returns whether the channel is archived.
async fn require_member(
    connection: &mut SqliteConnection,
    channel_id: &ChatChannelId,
) -> ChatResult<bool> {
    let row: Option<(bool, bool)> = sqlx::query_as(
        "SELECT channel.archived_at IS NOT NULL,
                EXISTS(
                  SELECT 1 FROM chat_conversation_memberships membership
                  WHERE membership.conversation_id = channel.conversation_id
                    AND membership.participant_id = ? AND membership.removed_at IS NULL
                )
         FROM chat_channels channel WHERE channel.id = ?",
    )
    .bind(LOCAL_PARTICIPANT_ID)
    .bind(channel_id.as_str())
    .fetch_optional(connection)
    .await
    .map_err(persistence_error)?;
    match row {
        None => Err(ChatError::new(
            ChatErrorCode::NotFound,
            "The Chat channel was not found",
            true,
        )),
        Some((_, false)) => Err(ChatError::new(
            ChatErrorCode::Permission,
            "Join the channel to see or change its notes",
            false,
        )),
        Some((archived, true)) => Ok(archived),
    }
}

/// As [`require_member`], and refuses an archived channel, whose notes stay readable only.
pub(super) async fn require_writable_member(
    connection: &mut SqliteConnection,
    channel_id: &ChatChannelId,
) -> ChatResult<()> {
    if require_member(connection, channel_id).await? {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Restore the channel before changing its notes",
            true,
        ));
    }
    Ok(())
}

/// Refuses when `channel_id` already holds [`MAX_ACTIVE_CHANNEL_NOTES`] live notes.
pub(super) async fn require_room_for_a_note(
    connection: &mut SqliteConnection,
    channel_id: &ChatChannelId,
) -> ChatResult<()> {
    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_channel_notes WHERE channel_id = ? AND deleted_at IS NULL",
    )
    .bind(channel_id.as_str())
    .fetch_one(connection)
    .await
    .map_err(persistence_error)?;
    if live >= MAX_ACTIVE_CHANNEL_NOTES {
        return Err(ChatError::validation(
            "channelId",
            format!(
                "This channel has {MAX_ACTIVE_CHANNEL_NOTES} notes, so remove one before adding \
                 another"
            ),
        ));
    }
    Ok(())
}

/// The note a completed receipt of `client_command_id` names when it recorded `action`, `None`
/// when no receipt exists.
///
/// # Errors
///
/// Returns a conflict when the id named another command, and a persistence error when the
/// receipt cannot be read.
pub(super) async fn replayed_note(
    connection: &mut SqliteConnection,
    client_command_id: &ChatCommandId,
    action: NoteAction,
) -> ChatResult<Option<ChatChannelNoteId>> {
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT command_kind, result_data FROM chat_organizational_command_receipts
         WHERE client_command_id = ?",
    )
    .bind(client_command_id.as_str())
    .fetch_optional(connection)
    .await
    .map_err(persistence_error)?;
    let Some((kind, data)) = row else {
        return Ok(None);
    };
    let receipt = (kind == CHANNEL_NOTE_RECEIPT_KIND)
        .then(|| data.and_then(|data| serde_json::from_str::<NoteReceipt>(&data).ok()))
        .flatten()
        .filter(|receipt| receipt.action == action)
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Conflict,
                "This command id was already used for another change",
                false,
            )
        })?;
    ChatChannelNoteId::new(receipt.note_id)
        .map(Some)
        .map_err(identifier_error)
}

/// Records the completed receipt of `action` on `note_id` under `client_command_id`.
///
/// # Errors
///
/// Returns a conflict when the id is taken and a persistence error when the write fails.
pub(super) async fn record_receipt(
    connection: &mut SqliteConnection,
    client_command_id: &ChatCommandId,
    action: NoteAction,
    note_id: &ChatChannelNoteId,
) -> ChatResult<()> {
    let result = serde_json::to_string(&NoteReceipt {
        action,
        note_id: note_id.as_str().to_owned(),
    })
    .map_err(|_| {
        ChatError::new(
            ChatErrorCode::Persistence,
            "Chat channel note receipt could not be saved",
            false,
        )
    })?;
    let now = now_timestamp()?;
    sqlx::query(
        "INSERT INTO chat_organizational_command_receipts
            (client_command_id, command_kind, state, result_schema_version, result_data,
             created_at, updated_at)
         VALUES (?, ?, 'completed', ?, ?, ?, ?)",
    )
    .bind(client_command_id.as_str())
    .bind(CHANNEL_NOTE_RECEIPT_KIND)
    .bind(RECEIPT_RESULT_SCHEMA_VERSION)
    .bind(result)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(connection)
    .await
    .map_err(map_command_receipt_error)?;
    Ok(())
}

fn require_one_row(rows_affected: u64) -> ChatResult<()> {
    if rows_affected == 1 {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The note changed before the update",
            true,
        ))
    }
}

fn note_not_found() -> ChatError {
    ChatError::new(
        ChatErrorCode::NotFound,
        "The channel note was not found",
        true,
    )
}

#[cfg(test)]
pub(super) mod tests;
