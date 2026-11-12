//! Schedules a teammate proposes for a person to confirm.
//!
//! A proposal is a teammate message in its reply thread with a `schedule_proposal` semantic
//! update. It schedules nothing: only a person who confirms it creates a scheduled message, so
//! every scheduled message stays a person's. Its ids derive from the run and the client request
//! id, so a repeated call writes nothing new.

use super::reports::{
    HOST_TOOL_SOURCE, HostCall, HostCallOutcome, HostUpdateIds, TeammateUpdate,
    insert_teammate_update, read_active_target, replayed,
};
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use crate::scheduling::RecurrenceSpec;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqliteConnection;

/// Semantic update kind of a schedule proposal.
pub const SCHEDULE_PROPOSAL_UPDATE_KIND: &str = "schedule_proposal";
/// Longest message a teammate may propose, in characters.
pub const MAX_PROPOSED_MESSAGE_CHARS: usize = 4000;
/// Longest reason a teammate may give for a proposal, in characters.
pub const MAX_PROPOSAL_REASON_CHARS: usize = 500;
/// Id part of a schedule proposal update derived from its host call.
const PROPOSAL_ID_PART: &str = "schedule-proposal";

/// A schedule a teammate proposes: the message to send, when it first sends, how it repeats,
/// and why. It is also the semantic update payload, with [`HOST_TOOL_SOURCE`] beside it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleProposal {
    /// Message text, without the mention of the proposing teammate a confirmation adds.
    pub message: String,
    /// RFC 3339 UTC time of the first send.
    pub first_at: String,
    /// Repeat rule, or `None` to send once.
    #[serde(default)]
    pub recurrence: Option<RecurrenceSpec>,
    /// Why the teammate proposes it.
    pub reason: String,
}

/// Why a schedule proposal was refused. [`ScheduleProposalRefusal::code`] is the tool error
/// code, so the teammate can correct its next call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduleProposalRefusal {
    /// The first send time is unparsable, too soon, or too far ahead.
    TimeInvalid,
    /// The repeat rule breaks a bound or names an unknown zone.
    RecurrenceInvalid,
    /// The message is empty, or the message or reason is too long.
    TooLarge,
}

impl ScheduleProposalRefusal {
    const ALL: [Self; 3] = [Self::TimeInvalid, Self::RecurrenceInvalid, Self::TooLarge];

    /// The stable tool error code.
    pub const fn code(self) -> &'static str {
        match self {
            Self::TimeInvalid => "proposal_time_invalid",
            Self::RecurrenceInvalid => "proposal_recurrence_invalid",
            Self::TooLarge => "proposal_too_large",
        }
    }

    /// The refusal as the tool error the teammate receives.
    pub fn into_error(self) -> ChatError {
        ChatError::new(ChatErrorCode::Permission, self.code(), false)
    }

    /// The refusal an error made by [`ScheduleProposalRefusal::into_error`] carries, if any.
    pub fn from_error(error: &ChatError) -> Option<Self> {
        if error.code != ChatErrorCode::Permission {
            return None;
        }
        Self::ALL
            .into_iter()
            .find(|refusal| refusal.code() == error.message)
    }
}

/// Returns `proposal` with its message and reason trimmed, or the refusal for an empty message
/// or a message or reason over its bound.
///
/// # Errors
///
/// Returns [`ScheduleProposalRefusal::TooLarge`].
pub fn normalize_proposal_text(
    mut proposal: ScheduleProposal,
) -> Result<ScheduleProposal, ScheduleProposalRefusal> {
    proposal.message = proposal.message.trim().to_string();
    proposal.reason = proposal.reason.trim().to_string();
    if proposal.message.is_empty()
        || proposal.message.chars().count() > MAX_PROPOSED_MESSAGE_CHARS
        || proposal.reason.chars().count() > MAX_PROPOSAL_REASON_CHARS
    {
        return Err(ScheduleProposalRefusal::TooLarge);
    }
    Ok(proposal)
}

/// Records the schedule `proposal` that `call` posts in its reply thread while its assignment
/// works or waits for an answer. An identical earlier call replays before `validate` runs, so a
/// replay after the proposed time has passed still succeeds; a new call runs `validate` first.
///
/// # Errors
///
/// Returns [`super::reports::HostReportRefusal::RequestConflict`] when the request id was used
/// with another proposal, [`super::reports::HostReportRefusal::AssignmentInactive`] when the
/// assignment or run ended, the error `validate` returns, and a persistence error when a read
/// or write fails.
pub async fn record_host_schedule_proposal(
    connection: &mut SqliteConnection,
    call: HostCall<'_>,
    proposal: &ScheduleProposal,
    validate: impl FnOnce(&ScheduleProposal) -> ChatResult<()>,
    now: &str,
) -> ChatResult<HostCallOutcome> {
    let payload = proposal_payload(proposal)?;
    let ids = HostUpdateIds::derive(PROPOSAL_ID_PART, call)?;
    if let Some(outcome) = replayed(
        connection,
        &ids.item_id,
        SCHEDULE_PROPOSAL_UPDATE_KIND,
        &payload,
    )
    .await?
    {
        return outcome;
    }
    validate(proposal)?;
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
            update_kind: SCHEDULE_PROPOSAL_UPDATE_KIND,
            markdown: &proposal.message,
            payload,
            created_at: now,
        },
    )
    .await?;
    Ok(HostCallOutcome::Posted)
}

fn proposal_payload(proposal: &ScheduleProposal) -> ChatResult<Value> {
    let mut payload = serde_json::to_value(proposal).map_err(|_| invalid_proposal())?;
    let Value::Object(fields) = &mut payload else {
        return Err(invalid_proposal());
    };
    fields.insert("source".to_string(), Value::from(HOST_TOOL_SOURCE));
    Ok(payload)
}

/// A stored schedule proposal with what a confirmation needs: where it was posted and which
/// teammate proposed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredScheduleProposal {
    pub proposal: ScheduleProposal,
    pub channel_id: String,
    pub reply_thread_id: String,
    pub teammate_id: String,
    /// The teammate's current name, `None` when it was removed or archived.
    pub teammate_name: Option<String>,
}

/// Reads the schedule proposal posted as the conversation item `item_id`. The payload replicates
/// without per-kind validation, so it is parsed here and a malformed one is refused.
///
/// # Errors
///
/// Returns a not found error when no live schedule proposal has that id, a validation error
/// when its payload is malformed, and a persistence error when the read fails.
pub async fn read_schedule_proposal(
    connection: &mut SqliteConnection,
    item_id: &str,
) -> ChatResult<StoredScheduleProposal> {
    type Row = (
        String,
        Option<String>,
        String,
        String,
        Option<String>,
        Option<String>,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT semantic.payload_data, item.reply_thread_id, channel.id, assignment.teammate_id,
                participant.display_name, participant.participant_kind
         FROM chat_work_semantic_updates semantic
         JOIN chat_conversation_items item ON item.id = semantic.item_id
         JOIN chat_communication_messages message ON message.item_id = semantic.item_id
         JOIN chat_work_assignments assignment ON assignment.id = semantic.assignment_id
         JOIN chat_channels channel ON channel.conversation_id = item.conversation_id
         LEFT JOIN chat_participants participant
           ON participant.id = assignment.teammate_id AND participant.archived_at IS NULL
         WHERE semantic.item_id = ? AND semantic.update_kind = ?
           AND message.deleted_at IS NULL",
    )
    .bind(item_id)
    .bind(SCHEDULE_PROPOSAL_UPDATE_KIND)
    .fetch_optional(connection)
    .await
    .map_err(persistence_error)?;
    let Some((payload_data, reply_thread_id, channel_id, teammate_id, name, kind)) = row else {
        return Err(ChatError::new(
            ChatErrorCode::NotFound,
            "Schedule proposal was not found",
            false,
        ));
    };
    let reply_thread_id = reply_thread_id.ok_or_else(malformed_proposal)?;
    let proposal = parse_stored_payload(&payload_data)?;
    Ok(StoredScheduleProposal {
        proposal,
        channel_id,
        reply_thread_id,
        teammate_id,
        teammate_name: name.filter(|_| kind.as_deref() == Some("ai_teammate")),
    })
}

fn parse_stored_payload(payload_data: &str) -> ChatResult<ScheduleProposal> {
    let mut value =
        serde_json::from_str::<Value>(payload_data).map_err(|_| malformed_proposal())?;
    let payload = value
        .get_mut("payload")
        .map(Value::take)
        .ok_or_else(malformed_proposal)?;
    let proposal =
        serde_json::from_value::<ScheduleProposal>(payload).map_err(|_| malformed_proposal())?;
    normalize_proposal_text(proposal).map_err(|_| malformed_proposal())
}

fn malformed_proposal() -> ChatError {
    ChatError::validation("proposalItemId", "Schedule proposal data is invalid")
}

fn invalid_proposal() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Schedule proposal data is invalid",
        false,
    )
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat data could not be saved or read",
        true,
    )
}

#[cfg(test)]
mod tests;
