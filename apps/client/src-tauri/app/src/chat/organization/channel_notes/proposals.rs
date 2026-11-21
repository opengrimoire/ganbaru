//! Channel note proposals an agent posts and a person settles.
//!
//! An agent only proposes a note through its report tool; a person accepts it, with its text
//! or an edited one, or dismisses it. Both write the note row whose id derives from the proposal
//! item and whose creation time is the proposal's, so devices that settle one proposal at the
//! same time write one row. A dismissal is that row already deleted, so it wins over a
//! concurrent accept.

use super::{
    LOCAL_PARTICIPANT_ID, NoteAction, normalized_note_body, read_note, record_receipt,
    replayed_note, require_room_for_a_note, require_writable_member,
};
use crate::chat::channels::{identifier_error, now_timestamp, persistence_error};
use ganbaru_chat::ids::derived_id;
use ganbaru_chat_contracts::models::{
    ChatChannelId, ChatChannelNoteId, ChatChannelNoteRead, ChatCommandId, ChatConversationItemId,
    ChatError, ChatErrorCode, ChatResult,
};
use ganbaru_sync_contracts::chat_channel_notes::{
    CHANNEL_NOTE_PROPOSAL_KEY_PREFIX, MAX_CHANNEL_NOTE_CHARS,
};
use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};

/// Semantic update kind of an agent's note proposal.
const NOTE_PROPOSAL_KIND: &str = "note_proposal";

/// A person saves an agent's proposal as a channel note, with `body` replacing its text when
/// given.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptChatNoteProposalCommand {
    pub client_command_id: ChatCommandId,
    pub item_id: ChatConversationItemId,
    #[serde(default)]
    pub body: Option<String>,
}

/// A person declines an agent's proposal, so no device saves it.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DismissChatNoteProposalCommand {
    pub client_command_id: ChatCommandId,
    pub item_id: ChatConversationItemId,
}

/// A note proposal as the settling commands read it.
struct Proposal {
    item_id: ChatConversationItemId,
    note_id: ChatChannelNoteId,
    channel_id: ChatChannelId,
    proposed_by: String,
    body: String,
    created_at: String,
}

/// Whether the note a proposal settles into exists, and whether it is deleted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettledNote {
    Absent,
    Saved,
    Deleted,
}

/// Saves `request`'s proposal as a note by the local person. Accepting a proposal another
/// device already saved returns that note.
///
/// # Errors
///
/// Returns a not found error on `itemId` for an unknown proposal, a validation error for a
/// blank or oversize body or a channel at the note limit, a permission error when the local
/// person is not a member, a conflict when the proposal was dismissed or its note deleted, the
/// channel is archived, or the command id named another command, and a persistence error when a
/// read or write fails.
pub(in crate::chat::organization) async fn accept_note_proposal(
    pool: &SqlitePool,
    request: &AcceptChatNoteProposalCommand,
) -> ChatResult<ChatChannelNoteRead> {
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    if let Some(note_id) = replayed_note(
        &mut transaction,
        &request.client_command_id,
        NoteAction::Accept,
    )
    .await?
    {
        return read_note(&mut transaction, &note_id).await;
    }
    let proposal = read_proposal(&mut transaction, &request.item_id).await?;
    require_writable_member(&mut transaction, &proposal.channel_id).await?;
    match settled_note(&mut transaction, &proposal.note_id).await? {
        SettledNote::Saved => {}
        SettledNote::Deleted => {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "This suggestion was dismissed or its note was deleted",
                false,
            ));
        }
        SettledNote::Absent => {
            let body = normalized_note_body(request.body.as_deref().unwrap_or(&proposal.body))?;
            require_room_for_a_note(&mut transaction, &proposal.channel_id).await?;
            let now = now_timestamp()?;
            insert_settled_note(&mut transaction, &proposal, &body, None, now.as_str()).await?;
        }
    }
    record_receipt(
        &mut transaction,
        &request.client_command_id,
        NoteAction::Accept,
        &proposal.note_id,
    )
    .await?;
    let note = read_note(&mut transaction, &proposal.note_id).await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(note)
}

/// Declines `request`'s proposal by writing its note row already deleted. Dismissing a proposal
/// that is already dismissed succeeds.
///
/// # Errors
///
/// Returns a not found error on `itemId` for an unknown proposal, a permission error when the
/// local person is not a member, a conflict when the proposal was already saved as a note, the
/// channel is archived, or the command id named another command, and a persistence error when a
/// read or write fails.
pub(in crate::chat::organization) async fn dismiss_note_proposal(
    pool: &SqlitePool,
    request: &DismissChatNoteProposalCommand,
) -> ChatResult<()> {
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    if replayed_note(
        &mut transaction,
        &request.client_command_id,
        NoteAction::Dismiss,
    )
    .await?
    .is_some()
    {
        return Ok(());
    }
    let proposal = read_proposal(&mut transaction, &request.item_id).await?;
    require_writable_member(&mut transaction, &proposal.channel_id).await?;
    match settled_note(&mut transaction, &proposal.note_id).await? {
        SettledNote::Deleted => {}
        SettledNote::Saved => {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "This suggestion is already a note, so delete the note instead",
                true,
            ));
        }
        SettledNote::Absent => {
            let body = dismissed_body(&proposal.body)?;
            let now = now_timestamp()?;
            insert_settled_note(
                &mut transaction,
                &proposal,
                &body,
                Some(now.as_str()),
                now.as_str(),
            )
            .await?;
        }
    }
    record_receipt(
        &mut transaction,
        &request.client_command_id,
        NoteAction::Dismiss,
        &proposal.note_id,
    )
    .await?;
    transaction.commit().await.map_err(persistence_error)
}

/// Most note proposals one state read may name.
pub(in crate::chat::organization) const MAX_NOTE_PROPOSAL_STATE_READS: usize = 200;

/// Where an agent's note proposal stands on every linked device.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatNoteProposalState {
    /// Nobody saved or dismissed it.
    Pending,
    /// A person saved it, and the note is live.
    Saved,
    /// A person dismissed it, or deleted the note it became.
    Dismissed,
}

/// The state of one note proposal.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatNoteProposalStateRead {
    pub proposal_item_id: ChatConversationItemId,
    pub state: ChatNoteProposalState,
}

/// Reads where each proposal in `proposal_item_ids` stands, from the note row its id derives.
///
/// # Errors
///
/// Returns a validation error on `proposalItemIds` when more than
/// [`MAX_NOTE_PROPOSAL_STATE_READS`] are named, and a persistence error when a read fails.
pub(in crate::chat::organization) async fn read_note_proposal_states(
    pool: &SqlitePool,
    proposal_item_ids: Vec<ChatConversationItemId>,
) -> ChatResult<Vec<ChatNoteProposalStateRead>> {
    if proposal_item_ids.len() > MAX_NOTE_PROPOSAL_STATE_READS {
        return Err(ChatError::validation(
            "proposalItemIds",
            format!("Read at most {MAX_NOTE_PROPOSAL_STATE_READS} suggested notes at once"),
        ));
    }
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    let mut states = Vec::with_capacity(proposal_item_ids.len());
    for proposal_item_id in proposal_item_ids {
        let note_id = proposal_note_id(&proposal_item_id)?;
        let state = match settled_note(&mut connection, &note_id).await? {
            SettledNote::Absent => ChatNoteProposalState::Pending,
            SettledNote::Saved => ChatNoteProposalState::Saved,
            SettledNote::Deleted => ChatNoteProposalState::Dismissed,
        };
        states.push(ChatNoteProposalStateRead {
            proposal_item_id,
            state,
        });
    }
    Ok(states)
}
/// The id of the note that settles the proposal `item_id`.
///
/// # Errors
///
/// Returns a validation error when the id cannot be derived.
pub(super) fn proposal_note_id(item_id: &ChatConversationItemId) -> ChatResult<ChatChannelNoteId> {
    let id = derived_id(CHANNEL_NOTE_PROPOSAL_KEY_PREFIX, &[item_id.as_str()])?;
    ChatChannelNoteId::new(id).map_err(identifier_error)
}

/// Reads the note proposal `item_id` with its channel and proposing agent.
async fn read_proposal(
    connection: &mut SqliteConnection,
    item_id: &ChatConversationItemId,
) -> ChatResult<Proposal> {
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT channel.id, message.author_participant_id, revision.normalized_markdown,
                item.created_at
         FROM chat_work_semantic_updates semantic
         JOIN chat_conversation_items item ON item.id = semantic.item_id
         JOIN chat_communication_messages message ON message.item_id = semantic.item_id
         JOIN chat_communication_message_revisions revision
           ON revision.id = message.current_revision_id
         JOIN chat_channels channel ON channel.conversation_id = item.conversation_id
         WHERE semantic.item_id = ? AND semantic.update_kind = ?",
    )
    .bind(item_id.as_str())
    .bind(NOTE_PROPOSAL_KIND)
    .fetch_optional(connection)
    .await
    .map_err(persistence_error)?;
    let (channel_id, proposed_by, body, created_at) = row.ok_or_else(proposal_not_found)?;
    Ok(Proposal {
        item_id: item_id.clone(),
        note_id: proposal_note_id(item_id)?,
        channel_id: ChatChannelId::new(channel_id).map_err(identifier_error)?,
        proposed_by,
        body,
        created_at,
    })
}

/// The refusal for an item that is not a note proposal, naming the `itemId` field.
fn proposal_not_found() -> ChatError {
    ChatError {
        field: Some("itemId".to_owned()),
        ..ChatError::new(
            ChatErrorCode::NotFound,
            "The suggested note was not found",
            true,
        )
    }
}

async fn settled_note(
    connection: &mut SqliteConnection,
    note_id: &ChatChannelNoteId,
) -> ChatResult<SettledNote> {
    let deleted: Option<bool> =
        sqlx::query_scalar("SELECT deleted_at IS NOT NULL FROM chat_channel_notes WHERE id = ?")
            .bind(note_id.as_str())
            .fetch_optional(connection)
            .await
            .map_err(persistence_error)?;
    Ok(match deleted {
        None => SettledNote::Absent,
        Some(false) => SettledNote::Saved,
        Some(true) => SettledNote::Deleted,
    })
}

/// Inserts the note settling `proposal` with `body`, deleted at `deleted_at` when given.
async fn insert_settled_note(
    connection: &mut SqliteConnection,
    proposal: &Proposal,
    body: &str,
    deleted_at: Option<&str>,
    now: &str,
) -> ChatResult<()> {
    sqlx::query(
        "INSERT INTO chat_channel_notes
            (id, channel_id, source_item_id, proposed_by_participant_id, body,
             edited_by_participant_id, deleted_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(proposal.note_id.as_str())
    .bind(proposal.channel_id.as_str())
    .bind(proposal.item_id.as_str())
    .bind(&proposal.proposed_by)
    .bind(body)
    .bind(LOCAL_PARTICIPANT_ID)
    .bind(deleted_at)
    .bind(&proposal.created_at)
    .bind(now)
    .execute(connection)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// The stored text of a dismissed proposal: its plain text cut to the note bound, since the
/// deleted row still has to satisfy the note's shape.
///
/// # Errors
///
/// Returns a validation error on `itemId` when no text remains.
fn dismissed_body(body: &str) -> ChatResult<String> {
    let plain: String = body
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .take(MAX_CHANNEL_NOTE_CHARS)
        .collect();
    let plain = plain.trim();
    if plain.is_empty() {
        return Err(ChatError::validation(
            "itemId",
            "The suggested note has no text",
        ));
    }
    Ok(plain.to_owned())
}

#[cfg(test)]
mod tests;
