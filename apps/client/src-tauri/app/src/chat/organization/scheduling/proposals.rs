//! Schedule proposals an agent posts and a person confirms or dismisses.
//!
//! A confirmation schedules a message authored by the local person that mentions the proposing
//! agent in the proposal's reply thread. Its scheduled message and command ids derive from the
//! proposal, so confirming one proposal on two devices yields one scheduled message. Dismissing
//! a proposal is device-local.

use super::super::common::scheduled_post_ids;
use super::super::{
    ScheduledPostTarget, chat_pool, identifier_error, now_timestamp, persistence_error,
};
use super::schedule_message;
#[cfg(desktop)]
use super::{series, validate_scheduled_for};
use crate::chat::device_state::{
    read_active_device_scope, record_dismissed_schedule_proposal, update_active_device_scope,
};
use ganbaru_chat::coordination::contracts::*;
use ganbaru_chat::coordination::schedule_proposals::{
    MAX_PROPOSED_MESSAGE_CHARS, StoredScheduleProposal, read_schedule_proposal,
};
#[cfg(desktop)]
use ganbaru_chat::coordination::schedule_proposals::{ScheduleProposal, ScheduleProposalRefusal};
use ganbaru_chat::ids::derived_id;
#[cfg(desktop)]
use ganbaru_chat::scheduling::validate_recurrence;
use ganbaru_chat_contracts::models::*;
use serde_json::json;
use sqlx::{SqliteConnection, SqlitePool};
use std::collections::BTreeMap;

/// Id part every id a confirmation derives starts with, followed by the proposal item id.
const PROPOSAL_ID_PART: &str = "schedule-proposal";
/// Most proposals one state read may name.
const MAX_PROPOSAL_STATE_READS: usize = 200;

/// Checks an agent's proposal: its first send time is a valid delivery time from `now`, and
/// its repeat rule is valid and resolves a first occurrence.
///
/// # Errors
///
/// Returns [`ScheduleProposalRefusal::TimeInvalid`] or
/// [`ScheduleProposalRefusal::RecurrenceInvalid`] as errors from
/// [`ScheduleProposalRefusal::into_error`].
#[cfg(desktop)]
pub(crate) fn validate_proposed_schedule(
    proposal: &ScheduleProposal,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let first_at = UtcTimestamp::new(proposal.first_at.clone())
        .map_err(|_| ScheduleProposalRefusal::TimeInvalid.into_error())?;
    validate_scheduled_for(&first_at, now)
        .map_err(|_| ScheduleProposalRefusal::TimeInvalid.into_error())?;
    if let Some(spec) = proposal.recurrence.as_ref() {
        validate_recurrence(spec)
            .map_err(|_| ScheduleProposalRefusal::RecurrenceInvalid.into_error())?;
        series::first_occurrence_at(spec, &first_at)
            .map_err(|_| ScheduleProposalRefusal::RecurrenceInvalid.into_error())?;
    }
    Ok(())
}

/// Schedules the message a person confirmed from an agent's proposal. `schedule_message`
/// validates it like any scheduled message and records this device's consent.
pub(in crate::chat::organization) async fn confirm_schedule_proposal(
    app: tauri::AppHandle,
    db_url: String,
    request: ConfirmScheduleProposalCommand,
) -> ChatResult<ChatScheduledMessageRead> {
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    let command = confirmation_command(&mut connection, request).await?;
    drop(connection);
    schedule_message(app, db_url, command).await
}

/// Builds the scheduled message command that confirms the proposal `request` names: the
/// person's text after a mention of the proposing agent, in the proposal's reply thread,
/// under ids derived from the proposal.
///
/// # Errors
///
/// Returns a not found error for an unknown proposal, a validation error for a malformed
/// proposal or text, and a conflict when the proposing agent was removed.
pub(super) async fn confirmation_command(
    connection: &mut SqliteConnection,
    request: ConfirmScheduleProposalCommand,
) -> ChatResult<ScheduleChatMessageCommand> {
    let item_id = request.proposal_item_id.as_str();
    let StoredScheduleProposal {
        proposal,
        channel_id,
        reply_thread_id,
        agent_id,
        agent_name,
    } = read_schedule_proposal(connection, item_id).await?;
    let agent_name = agent_name.ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::Conflict,
            "The agent that proposed this schedule was removed",
            false,
        )
    })?;
    let text = confirmed_text(request.normalized_markdown, proposal.message)?;
    let mention = format!("@{agent_name}");
    let normalized_markdown = format!("{mention} {text}");
    let derived = |prefix: &str| derived_id(prefix, &[PROPOSAL_ID_PART, item_id]);
    let reference = ChatMessageReference::Participant {
        metadata: ChatReferenceMetadata {
            reference_id: ChatMessageReferenceId::new(derived("message-reference")?)
                .map_err(identifier_error)?,
            label_snapshot: agent_name,
            start_offset: 0,
            end_offset: u64::try_from(mention.len()).map_err(|_| text_too_long())?,
            plain_text_projection: mention,
        },
        participant_id: ChatParticipantId::new(agent_id).map_err(identifier_error)?,
        participant_kind: ChatParticipantKind::AiAgent,
    };
    Ok(ScheduleChatMessageCommand {
        scheduled_message_id: ChatScheduledMessageId::new(derived("scheduled-message")?)
            .map_err(identifier_error)?,
        scheduled_for: request.scheduled_for,
        message: PostChatMessageCommand {
            client_command_id: ChatCommandId::new(derived("command")?).map_err(identifier_error)?,
            channel_id: ChatChannelId::new(channel_id).map_err(identifier_error)?,
            reply_thread_id: Some(
                ChatReplyThreadId::new(reply_thread_id).map_err(identifier_error)?,
            ),
            normalized_markdown,
            rich_content: VersionedJson {
                schema_version: 1,
                value: json!({ "type": "document", "children": [] }),
            },
            attachment_ids: Vec::new(),
            references: vec![reference],
            execution_target: None,
            also_send_to_channel: false,
            followup_routing: None,
        },
        recurrence: request.recurrence,
    })
}

/// The text a confirmation sends: the person's edited text, or the proposed text, trimmed and
/// bounded like a proposal.
fn confirmed_text(edited: Option<String>, proposed: String) -> ChatResult<String> {
    let text = edited.unwrap_or(proposed).trim().to_string();
    if text.is_empty() {
        return Err(ChatError::validation(
            "normalizedMarkdown",
            "Message content is required",
        ));
    }
    if text.chars().count() > MAX_PROPOSED_MESSAGE_CHARS {
        return Err(text_too_long());
    }
    Ok(text)
}

fn device_state_error(_error: String) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Dismissed schedule proposals could not be read or saved",
        true,
    )
}

fn text_too_long() -> ChatError {
    ChatError::validation(
        "normalizedMarkdown",
        format!("A confirmed proposal holds at most {MAX_PROPOSED_MESSAGE_CHARS} characters"),
    )
}

/// The scheduled message id a confirmation of the proposal `proposal_item_id` uses.
fn confirmed_scheduled_message_id(proposal_item_id: &str) -> ChatResult<String> {
    derived_id("scheduled-message", &[PROPOSAL_ID_PART, proposal_item_id])
}

/// Records that this device dismissed the proposal `proposal_item_id`. It never syncs.
pub(in crate::chat::organization) fn dismiss_schedule_proposal(
    app: &tauri::AppHandle,
    proposal_item_id: &ChatConversationItemId,
) -> ChatResult<()> {
    let now = now_timestamp()?;
    update_active_device_scope(app, |scope| {
        record_dismissed_schedule_proposal(scope, proposal_item_id.as_str().to_string(), now);
        Ok(())
    })
    .map_err(device_state_error)
}

/// Reads where each proposal in `proposal_item_ids` stands on this device.
pub(in crate::chat::organization) async fn read_schedule_proposal_states(
    app: tauri::AppHandle,
    db_url: String,
    proposal_item_ids: Vec<ChatConversationItemId>,
) -> ChatResult<Vec<ChatScheduleProposalStateRead>> {
    if proposal_item_ids.len() > MAX_PROPOSAL_STATE_READS {
        return Err(ChatError::validation(
            "proposalItemIds",
            format!("Read at most {MAX_PROPOSAL_STATE_READS} proposals at once"),
        ));
    }
    let dismissed = read_active_device_scope(&app)
        .map_err(device_state_error)?
        .dismissed_schedule_proposals;
    let pool = chat_pool(app, db_url).await?;
    proposal_states(&pool, proposal_item_ids, &dismissed).await
}

/// Where each proposal stands: confirmed when its derived scheduled message exists or its
/// one-time post was sent, else dismissed when this device dismissed it, else pending. A
/// cancelled confirmation leaves the proposal pending again until it expires.
pub(super) async fn proposal_states(
    pool: &SqlitePool,
    proposal_item_ids: Vec<ChatConversationItemId>,
    dismissed: &BTreeMap<String, UtcTimestamp>,
) -> ChatResult<Vec<ChatScheduleProposalStateRead>> {
    let mut states = Vec::with_capacity(proposal_item_ids.len());
    for proposal_item_id in proposal_item_ids {
        let scheduled_message_id = confirmed_scheduled_message_id(proposal_item_id.as_str())?;
        let posted_item_id =
            scheduled_post_ids(&scheduled_message_id, ScheduledPostTarget::Message, None)?.item_id;
        let confirmed: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM chat_scheduled_messages WHERE id = ?)
                 OR EXISTS(SELECT 1 FROM chat_conversation_items WHERE id = ?)",
        )
        .bind(&scheduled_message_id)
        .bind(posted_item_id.as_str())
        .fetch_one(pool)
        .await
        .map_err(persistence_error)?;
        let state = if confirmed {
            ChatScheduleProposalState::Confirmed
        } else if dismissed.contains_key(proposal_item_id.as_str()) {
            ChatScheduleProposalState::Dismissed
        } else {
            ChatScheduleProposalState::Pending
        };
        states.push(ChatScheduleProposalStateRead {
            proposal_item_id,
            state,
        });
    }
    Ok(states)
}

#[cfg(test)]
mod tests;
