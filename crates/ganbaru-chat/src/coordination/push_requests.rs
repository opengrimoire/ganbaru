//! Pushes a teammate asks a person to confirm.
//!
//! A teammate never pushes. It asks: a push request is a host question in its reply thread whose
//! payload records the commit, the remote, the destination branch, and the push URL a push would
//! reach, read by Ganbaru from the folder, with the digest of the folder's whole effective Git
//! configuration. A request may also carry a commit message: Ganbaru then commits the working tree
//! on that commit at confirmation, and records the tree, author, signing, and changed paths it
//! read. The assignment waits for an answer like any question. Only a person who confirms the
//! request pushes, and only while a fresh read still names the same target and tree, so a
//! teammate cannot change what reaches the remote after the person looked.

use super::contracts::ChatPushRequestState;
use super::reports::{
    HOST_TOOL_SOURCE, HostCall, HostCallOutcome, HostUpdateIds, record_question_update,
};
use crate::git::{
    GitPushTarget, GitWorktreeCommit, is_object_id, is_supported_push_url,
    normalize_commit_message, validate_branch_name,
};
use crate::ids::derived_id;
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;

/// Longest summary a teammate may give for a push request, in characters.
pub const MAX_PUSH_SUMMARY_CHARS: usize = 1_000;
/// Longest remote name or ID a push request may record, in bytes.
const MAX_PUSH_FIELD_BYTES: usize = 1_024;
/// Id part of a push request update derived from its host call.
const PUSH_REQUEST_ID_PART: &str = "push-request";
/// Id part every id a confirmation derives starts with, followed by the request item id.
const PUSH_CONFIRMATION_ID_PART: &str = "push-confirmation";
/// Prefix of the id of the teammate mention a confirmation reply carries; its presence marks
/// the request pushed.
pub const PUSH_CONFIRMATION_REFERENCE_PREFIX: &str = "message-reference";

/// What a teammate asks to push, as Ganbaru read it from the folder.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushRequest {
    /// The working folder whose repository pushes.
    pub working_folder_id: String,
    /// The execution environment the target was read in, `None` for the folder itself.
    pub execution_environment_id: Option<String>,
    /// The commit, branches, remote, and push URL.
    #[serde(flatten)]
    pub target: GitPushTarget,
    /// Hexadecimal digest of the folder's whole effective Git configuration when the target was
    /// read.
    pub git_config_digest: String,
    /// The commit Ganbaru makes from the working tree on the target commit when the person
    /// confirms, `None` to push the target commit itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<PushRequestCommit>,
}

/// A commit a push request asks Ganbaru to make at confirmation: the teammate's message on the
/// working tree Ganbaru read, whose parent is the target commit.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushRequestCommit {
    /// The commit message.
    pub message: String,
    /// The tree, author, signing, and changed paths Ganbaru read.
    #[serde(flatten)]
    pub worktree: GitWorktreeCommit,
}

impl PushRequest {
    /// Whether every field has the shape Ganbaru writes. A request replicates from other
    /// devices without per-kind validation, so a stored one is checked before it is shown or
    /// confirmed.
    pub fn is_well_formed(&self) -> bool {
        let field = |value: &str| {
            !value.is_empty()
                && value.len() <= MAX_PUSH_FIELD_BYTES
                && !value.starts_with('-')
                && !value
                    .chars()
                    .any(|character| character.is_control() || character.is_whitespace())
        };
        field(&self.working_folder_id)
            && self.execution_environment_id.as_deref().is_none_or(field)
            && field(&self.target.remote)
            && validate_branch_name(&self.target.local_branch).is_ok()
            && validate_branch_name(&self.target.destination_branch).is_ok()
            && is_supported_push_url(&self.target.push_url)
            && is_object_id(&self.target.object_id)
            && self.git_config_digest.len() == 64
            && self
                .git_config_digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && self.commit.as_ref().is_none_or(|commit| {
                normalize_commit_message(&commit.message) == Some(commit.message.as_str())
                    && commit.worktree.is_well_formed()
            })
    }
}

/// Why a push request was refused, to the teammate asking or the person confirming.
/// [`PushRequestRefusal::code`] is the tool error code or the `reason` detail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PushRequestRefusal {
    /// The summary is empty or longer than [`MAX_PUSH_SUMMARY_CHARS`].
    SummaryInvalid,
    /// No branch is checked out.
    Detached,
    /// The branch has no push or upstream remote and none was named, or the named remote is
    /// not configured.
    RemoteUnavailable,
    /// The remote has several push URLs, the push URL carries a password or names an
    /// unsupported transport, or the destination is not a plain branch.
    TargetUnsupported,
    /// Repository configuration chooses how Git connects or authenticates (a credential
    /// helper, an SSH or proxy command, a proxy, or TLS trust).
    RepositoryConnection,
    /// The asking run is still working, so the request cannot be confirmed yet.
    Pending,
    /// The request was answered, superseded, or its run did not complete.
    Closed,
    /// The commit message is empty, longer than [`crate::git::MAX_COMMIT_MESSAGE_CHARS`], or
    /// carries control characters.
    CommitMessageInvalid,
    /// A commit was asked for, but the working tree has no change to commit.
    NothingToCommit,
    /// A commit was asked for, but the person's Git configuration names no author.
    CommitIdentityMissing,
    /// The folder no longer names the recorded target, working tree, or Git configuration.
    TargetChanged,
    /// Another confirmation or a standing push of the request is pushing it now.
    InProgress,
    /// The request was pushed; only its confirmation reply may still be on its way.
    AlreadyPushed,
}

impl PushRequestRefusal {
    const ALL: [Self; 13] = [
        Self::SummaryInvalid,
        Self::Detached,
        Self::RemoteUnavailable,
        Self::TargetUnsupported,
        Self::RepositoryConnection,
        Self::CommitMessageInvalid,
        Self::NothingToCommit,
        Self::CommitIdentityMissing,
        Self::Pending,
        Self::Closed,
        Self::TargetChanged,
        Self::InProgress,
        Self::AlreadyPushed,
    ];

    /// The stable error code.
    pub const fn code(self) -> &'static str {
        match self {
            Self::SummaryInvalid => "push_summary_invalid",
            Self::Detached => "push_branch_detached",
            Self::RemoteUnavailable => "push_remote_unavailable",
            Self::TargetUnsupported => "push_target_unsupported",
            Self::RepositoryConnection => "push_repository_connection",
            Self::CommitMessageInvalid => "push_commit_message_invalid",
            Self::NothingToCommit => "push_nothing_to_commit",
            Self::CommitIdentityMissing => "push_commit_identity_missing",
            Self::Pending => "push_request_pending",
            Self::Closed => "push_request_closed",
            Self::TargetChanged => "push_target_changed",
            Self::InProgress => "push_request_in_progress",
            Self::AlreadyPushed => "push_request_already_pushed",
        }
    }

    /// The refusal as an error whose message and `reason` detail are the code.
    pub fn into_error(self) -> ChatError {
        let mut error = ChatError::new(ChatErrorCode::Permission, self.code(), false);
        error.details = Some(Box::new(json!({ "reason": self.code() })));
        error
    }

    /// The refusal an error made by [`PushRequestRefusal::into_error`] carries, if any.
    pub fn from_error(error: &ChatError) -> Option<Self> {
        if error.code != ChatErrorCode::Permission {
            return None;
        }
        Self::ALL
            .into_iter()
            .find(|refusal| refusal.code() == error.message)
    }
}

/// Returns the trimmed `summary`, or the refusal for an empty one or one over
/// [`MAX_PUSH_SUMMARY_CHARS`].
///
/// # Errors
///
/// Returns [`PushRequestRefusal::SummaryInvalid`].
pub fn normalize_push_summary(summary: &str) -> Result<&str, PushRequestRefusal> {
    let summary = summary.trim();
    if summary.is_empty() || summary.chars().count() > MAX_PUSH_SUMMARY_CHARS {
        return Err(PushRequestRefusal::SummaryInvalid);
    }
    Ok(summary)
}

/// Records the push `request` that `call` asks with the normalized `summary`: like a host
/// question, the working assignment moves to `waiting_for_answer` and a `question` update with
/// the request is posted in the reply thread.
///
/// # Errors
///
/// Returns the refusals of [`super::reports::record_host_question`], a validation error for a
/// malformed request, and a persistence error when a read or write fails.
pub async fn record_host_push_request(
    connection: &mut SqliteConnection,
    call: HostCall<'_>,
    summary: &str,
    request: &PushRequest,
    now: &str,
) -> ChatResult<HostCallOutcome> {
    if !request.is_well_formed() {
        return Err(malformed_request());
    }
    let payload = json!({
        "question": summary,
        "choices": [],
        "source": HOST_TOOL_SOURCE,
        "push": request,
    });
    let ids = HostUpdateIds::derive(PUSH_REQUEST_ID_PART, call)?;
    record_question_update(connection, call, &ids, payload, summary, now).await
}

/// Whether a person can confirm a stored push request now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PushRequestState {
    /// The asking run completed, its assignment waits for an answer, the request is its newest
    /// question, and no person replied after it.
    Open,
    /// As [`PushRequestState::Open`], except that the asking run is still working.
    Pending,
    /// Anything else: answered, superseded, failed, or stopped.
    Closed,
}

impl PushRequestState {
    /// `Ok` when open, else the refusal a confirmation returns.
    ///
    /// # Errors
    ///
    /// Returns [`PushRequestRefusal::Pending`] or [`PushRequestRefusal::Closed`].
    pub fn require_open(self) -> Result<(), PushRequestRefusal> {
        match self {
            Self::Open => Ok(()),
            Self::Pending => Err(PushRequestRefusal::Pending),
            Self::Closed => Err(PushRequestRefusal::Closed),
        }
    }
}

/// A stored push request with what a confirmation needs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredPushRequest {
    pub request: PushRequest,
    pub summary: String,
    pub state: PushRequestState,
    pub assignment_id: String,
    /// The authorization revision of the asking run, whose folder grant a confirmation checks;
    /// `None` when the run is gone.
    pub authorization_revision_id: Option<String>,
    pub channel_id: String,
    pub reply_thread_id: String,
    pub teammate_id: String,
    /// The teammate's current name, `None` when it was removed or archived.
    pub teammate_name: Option<String>,
}

/// The row a stored push request is read from.
#[derive(sqlx::FromRow)]
struct PushRequestRow {
    payload_data: String,
    assignment_id: String,
    assignment_state: String,
    run_state: Option<String>,
    authorization_revision_id: Option<String>,
    run_is_latest: bool,
    is_newest_question: bool,
    person_replied_after: bool,
    reply_thread_id: Option<String>,
    channel_id: String,
    teammate_id: String,
    teammate_name: Option<String>,
    teammate_kind: Option<String>,
}

/// Reads the push request posted as the conversation item `item_id` and whether it is open.
///
/// # Errors
///
/// Returns a not found error when no live push request has that id, a validation error when its
/// payload is malformed, and a persistence error when the read fails.
pub async fn read_push_request(
    connection: &mut SqliteConnection,
    item_id: &str,
) -> ChatResult<StoredPushRequest> {
    let row: Option<PushRequestRow> = sqlx::query_as(
        "SELECT semantic.payload_data, semantic.assignment_id,
                assignment.state AS assignment_state, run.state AS run_state,
                run.authorization_revision_id,
                coalesce(run.run_ordinal = (
                  SELECT max(latest.run_ordinal) FROM chat_agent_runs latest
                  WHERE latest.assignment_id = semantic.assignment_id
                ), 0) AS run_is_latest,
                NOT EXISTS(
                  SELECT 1 FROM chat_work_semantic_updates later
                  JOIN chat_conversation_items later_item ON later_item.id = later.item_id
                  WHERE later.assignment_id = semantic.assignment_id
                    AND later.update_kind = 'question'
                    AND (later_item.order_key > item.order_key
                      OR (later_item.order_key = item.order_key AND later_item.id > item.id))
                ) AS is_newest_question,
                EXISTS(
                  SELECT 1 FROM chat_conversation_items reply
                  JOIN chat_communication_messages reply_message ON reply_message.item_id = reply.id
                  JOIN chat_participants author
                    ON author.id = reply_message.author_participant_id
                  WHERE reply.reply_thread_id = item.reply_thread_id
                    AND author.participant_kind IN ('local_user', 'human')
                    AND (reply.order_key > item.order_key
                      OR (reply.order_key = item.order_key AND reply.id > item.id))
                ) AS person_replied_after,
                item.reply_thread_id, channel.id AS channel_id, assignment.teammate_id,
                participant.display_name AS teammate_name,
                participant.participant_kind AS teammate_kind
         FROM chat_work_semantic_updates semantic
         JOIN chat_conversation_items item ON item.id = semantic.item_id
         JOIN chat_communication_messages message ON message.item_id = semantic.item_id
         JOIN chat_work_assignments assignment ON assignment.id = semantic.assignment_id
         JOIN chat_channels channel ON channel.conversation_id = item.conversation_id
         LEFT JOIN chat_agent_runs run
           ON run.id = json_extract(semantic.payload_data, '$.agentRunId')
          AND run.assignment_id = semantic.assignment_id
         LEFT JOIN chat_participants participant
           ON participant.id = assignment.teammate_id AND participant.archived_at IS NULL
         WHERE semantic.item_id = ? AND semantic.update_kind = 'question'
           AND json_extract(semantic.payload_data, '$.payload.source') = ?
           AND json_type(semantic.payload_data, '$.payload.push') = 'object'
           AND message.deleted_at IS NULL",
    )
    .bind(item_id)
    .bind(HOST_TOOL_SOURCE)
    .fetch_optional(connection)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Err(ChatError::new(
            ChatErrorCode::NotFound,
            "Push request was not found",
            false,
        ));
    };
    let (summary, request) = parse_stored_payload(&row.payload_data)?;
    let reply_thread_id = row.reply_thread_id.ok_or_else(malformed_request)?;
    let waiting = row.assignment_state == "waiting_for_answer"
        && row.run_is_latest
        && row.is_newest_question
        && !row.person_replied_after;
    let state = match row.run_state.as_deref() {
        Some("completed") if waiting => PushRequestState::Open,
        Some("starting" | "working" | "waiting") if waiting => PushRequestState::Pending,
        _ => PushRequestState::Closed,
    };
    Ok(StoredPushRequest {
        request,
        summary,
        state,
        assignment_id: row.assignment_id,
        authorization_revision_id: row.authorization_revision_id,
        channel_id: row.channel_id,
        reply_thread_id,
        teammate_id: row.teammate_id,
        teammate_name: row
            .teammate_name
            .filter(|_| row.teammate_kind.as_deref() == Some("ai_teammate")),
    })
}

/// The id with `prefix` that the confirmation of the push request `item_id` derives, so
/// confirming one request on two devices posts one reply.
///
/// # Errors
///
/// Returns a validation error for an unusable prefix or item id.
pub fn push_confirmation_id(prefix: &str, item_id: &str) -> ChatResult<String> {
    derived_id(prefix, &[PUSH_CONFIRMATION_ID_PART, item_id])
}

/// Reads where the push request `item_id` stands: pushed when its confirmation reply exists,
/// else open, waiting, or closed from [`read_push_request`]. An unknown or malformed request is
/// closed.
///
/// # Errors
///
/// Returns a persistence error when a read fails.
pub async fn read_push_request_state(
    connection: &mut SqliteConnection,
    item_id: &str,
) -> ChatResult<ChatPushRequestState> {
    let reference_id = push_confirmation_id(PUSH_CONFIRMATION_REFERENCE_PREFIX, item_id)?;
    let pushed: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_message_references WHERE id = ?)")
            .bind(&reference_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(persistence_error)?;
    if pushed {
        return Ok(ChatPushRequestState::Pushed);
    }
    match read_push_request(connection, item_id).await {
        Ok(stored) => Ok(match stored.state {
            PushRequestState::Open => ChatPushRequestState::Open,
            PushRequestState::Pending => ChatPushRequestState::Waiting,
            PushRequestState::Closed => ChatPushRequestState::Closed,
        }),
        Err(error)
            if matches!(
                error.code,
                ChatErrorCode::NotFound | ChatErrorCode::Validation
            ) =>
        {
            Ok(ChatPushRequestState::Closed)
        }
        Err(error) => Err(error),
    }
}

fn parse_stored_payload(payload_data: &str) -> ChatResult<(String, PushRequest)> {
    let mut value = serde_json::from_str::<Value>(payload_data).map_err(|_| malformed_request())?;
    let mut payload = value
        .get_mut("payload")
        .map(Value::take)
        .ok_or_else(malformed_request)?;
    let summary = payload
        .get("question")
        .and_then(Value::as_str)
        .and_then(|summary| normalize_push_summary(summary).ok())
        .ok_or_else(malformed_request)?
        .to_string();
    let request = payload
        .get_mut("push")
        .map(Value::take)
        .and_then(|push| serde_json::from_value::<PushRequest>(push).ok())
        .filter(PushRequest::is_well_formed)
        .ok_or_else(malformed_request)?;
    Ok((summary, request))
}

fn malformed_request() -> ChatError {
    ChatError::validation("requestItemId", "Push request data is invalid")
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat data could not be saved or read",
        true,
    )
}

pub mod claims;

#[cfg(test)]
mod tests;
