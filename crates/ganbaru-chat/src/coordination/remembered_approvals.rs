//! Remembered approvals: the rules an "Allow and don't ask again" choice records for an agent in
//! a channel, and the reads and writes that keep them.
//!
//! A rule is derived only here, from the request Ganbaru persisted, so the offer a person sees,
//! the rule their choice records, and the match that later answers a request for them agree. A
//! rule names one web origin, one exact command, or one tool. Requests that name a path, a
//! search pattern, or a query, that hide a blocked path, or whose subject was shortened for
//! display never get one.

use crate::coordination::access::effective_access_level_sql;
use crate::ids::derived_id;
use crate::models::{
    ChatAgentAccess, ChatChannelId, ChatChannelRememberedApprovalRead, ChatError, ChatErrorCode,
    ChatParticipantId, ChatRememberableApprovalRead, ChatRememberedApprovalKind, ChatResult,
    UtcTimestamp,
};
use crate::sync_effects::lapsed_remembered_approval_sql;
use ganbaru_sync_contracts::chat_remembered_approvals::{
    MAX_REMEMBERED_APPROVAL_SUBJECT_BYTES, REMEMBERED_APPROVAL_KEY_PREFIX,
};
use serde_json::Value;
use sqlx::SqliteConnection;
use url::{Host, Url};

/// Characters that let one command line run more than one program or expand into other text, so
/// a command holding any of them is never remembered.
const SHELL_OPERATORS: [char; 11] = [';', '&', '|', '<', '>', '$', '`', '(', ')', '{', '}'];

/// Host names that name this device rather than a public site.
const PRIVATE_HOSTS: [&str; 1] = ["localhost"];
/// Host suffixes that name this device or a private network rather than a public site.
const PRIVATE_HOST_SUFFIXES: [&str; 5] =
    [".localhost", ".local", ".lan", ".internal", ".home.arpa"];

/// The rule an "Allow and don't ask again" choice records for the persisted approval request
/// whose `safe_display_data` is `safe_display`, or `None` when the request cannot be
/// remembered.
#[must_use]
pub fn rememberable_approval(safe_display: &Value) -> Option<ChatRememberableApprovalRead> {
    let hides_blocked_path = safe_display
        .pointer("/payload/value/hasBlockedPath")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if hides_blocked_path {
        return None;
    }
    let subject = safe_display.get("subject")?;
    if subject.get("truncated").and_then(Value::as_bool) != Some(false) {
        return None;
    }
    let text = subject.get("text")?.as_str()?;
    let (kind, key) = match subject.get("kind")?.as_str()? {
        "url" => (ChatRememberedApprovalKind::UrlOrigin, url_origin_key(text)?),
        "command" => (ChatRememberedApprovalKind::Command, command_key(text)?),
        "tool" => (ChatRememberedApprovalKind::Tool, tool_key(text)?),
        _ => return None,
    };
    Some(ChatRememberableApprovalRead { kind, key })
}

/// The origin of a public web address, `scheme://host[:port]`, or `None` for an address that is
/// not http or https, names an IP address, or names this device or a private network.
fn url_origin_key(text: &str) -> Option<String> {
    let url = Url::parse(text).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let Some(Host::Domain(host)) = url.host() else {
        return None;
    };
    let host = host.to_ascii_lowercase();
    let public = host.contains('.')
        && !host.ends_with('.')
        && !PRIVATE_HOSTS.contains(&host.as_str())
        && !PRIVATE_HOST_SUFFIXES
            .iter()
            .any(|suffix| host.ends_with(suffix));
    if !public {
        return None;
    }
    let origin = url.origin().ascii_serialization();
    bounded(&origin).then_some(origin)
}

/// The exact text of a single command without shell operators, or `None`.
fn command_key(text: &str) -> Option<String> {
    let single = !text.trim().is_empty() && !text.contains(SHELL_OPERATORS);
    (single && bounded(text)).then(|| text.to_string())
}

/// The exact name of a tool, or `None` for a blank or unbounded one.
fn tool_key(text: &str) -> Option<String> {
    let named = !text.is_empty() && !text.chars().any(char::is_whitespace);
    (named && bounded(text)).then(|| text.to_string())
}

/// Whether a subject key fits the stored bound and holds no control character.
fn bounded(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_REMEMBERED_APPROVAL_SUBJECT_BYTES
        && !key.chars().any(char::is_control)
}

/// The channel and agent a provider thread works for, with the channel's current effective
/// access: what a remembered approval of one of its requests names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RememberedApprovalScope {
    pub conversation_id: String,
    pub agent_id: String,
    pub access: ChatAgentAccess,
}

/// The scope of the newest run of the provider thread `thread_id` when that run works in a
/// channel with a folder, a scratch space, or an environment, or `None` for a thread that is no
/// channel run or whose newest run only takes part in the conversation.
///
/// # Errors
///
/// Returns a persistence error when the read fails.
pub async fn channel_run_scope(
    connection: &mut SqliteConnection,
    thread_id: &str,
) -> ChatResult<Option<RememberedApprovalScope>> {
    let row: Option<(String, String, String, bool)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT reply.conversation_id, assignment.agent_id, {access},
                    run.working_folder_id IS NULL AND run.scratch_generation_id IS NULL
                        AND run.execution_environment_id IS NULL
             FROM chat_agent_runs run
             JOIN chat_work_assignments assignment ON assignment.id = run.assignment_id
             JOIN chat_reply_threads reply ON reply.id = assignment.reply_thread_id
             JOIN chat_channels channel ON channel.conversation_id = reply.conversation_id
             WHERE run.provider_thread_id = ?
             ORDER BY run.created_at DESC, run.run_ordinal DESC, run.id DESC
             LIMIT 1",
        access = effective_access_level_sql("reply.conversation_id"),
    )))
    .bind(thread_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let Some((conversation_id, agent_id, access, conversation_only)) = row else {
        return Ok(None);
    };
    if conversation_only {
        return Ok(None);
    }
    let access = ChatAgentAccess::parse(&access).ok_or_else(corrupt_data_error)?;
    Ok(Some(RememberedApprovalScope {
        conversation_id,
        agent_id,
        access,
    }))
}

/// Records that the agent of `scope` may repeat requests that `rule` matches in its channel
/// without asking, given by the person `approver` at `now` under the channel's current access.
/// Approving again clears an earlier revocation.
///
/// # Errors
///
/// Returns a validation error when the key cannot be derived and a persistence error when the
/// write fails, including when `approver` is not a person.
pub async fn remember_approval(
    connection: &mut SqliteConnection,
    scope: &RememberedApprovalScope,
    rule: &ChatRememberableApprovalRead,
    approver: &str,
    now: &str,
) -> ChatResult<()> {
    let kind = rule.kind.as_str();
    let sync_key = derived_id(
        REMEMBERED_APPROVAL_KEY_PREFIX,
        &[&scope.conversation_id, &scope.agent_id, kind, &rule.key],
    )?;
    sqlx::query(
        "INSERT INTO chat_remembered_approvals
            (conversation_id, agent_id, subject_kind, subject_key, sync_key,
             approved_by_participant_id, approved_at, granted_access)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (conversation_id, agent_id, subject_kind, subject_key)
         DO UPDATE SET
             approved_by_participant_id = excluded.approved_by_participant_id,
             approved_at = excluded.approved_at, granted_access = excluded.granted_access,
             revoked_at = NULL, revision = revision + 1",
    )
    .bind(&scope.conversation_id)
    .bind(&scope.agent_id)
    .bind(kind)
    .bind(&rule.key)
    .bind(&sync_key)
    .bind(approver)
    .bind(now)
    .bind(scope.access.as_str())
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// Whether an unrevoked, unlapsed approval lets the agent of `scope` repeat requests that `rule`
/// matches: the channel's effective access still ranks at or above the access it was granted
/// under, and the agent is present and unarchived.
///
/// # Errors
///
/// Returns a persistence error when the read fails.
pub async fn approval_is_live(
    connection: &mut SqliteConnection,
    scope: &RememberedApprovalScope,
    rule: &ChatRememberableApprovalRead,
) -> ChatResult<bool> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT EXISTS(
             SELECT 1 FROM chat_remembered_approvals approval
             WHERE approval.conversation_id = ? AND approval.agent_id = ?
               AND approval.subject_kind = ? AND approval.subject_key = ?
               AND approval.revoked_at IS NULL
               AND NOT {lapsed}
         )",
        lapsed = lapsed_remembered_approval_sql("approval"),
    )))
    .bind(&scope.conversation_id)
    .bind(&scope.agent_id)
    .bind(rule.kind.as_str())
    .bind(&rule.key)
    .fetch_one(&mut *connection)
    .await
    .map_err(persistence_error)
}

/// Returns the unrevoked remembered approvals of a conversation, newest first. The approval id
/// is the row's sync key, which names it on every device.
///
/// # Errors
///
/// Returns a persistence error when the read fails or a stored row is invalid.
pub async fn read_channel_remembered_approvals(
    connection: &mut SqliteConnection,
    conversation_id: &str,
) -> ChatResult<Vec<ChatChannelRememberedApprovalRead>> {
    type Row = (String, String, String, String, String, String);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT approval.sync_key, channel.id, approval.agent_id, approval.subject_kind,
                approval.subject_key, approval.approved_at
         FROM chat_remembered_approvals approval
         JOIN chat_channels channel ON channel.conversation_id = approval.conversation_id
         WHERE approval.conversation_id = ? AND approval.revoked_at IS NULL
         ORDER BY approval.approved_at DESC, approval.sync_key",
    )
    .bind(conversation_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    rows.into_iter()
        .map(
            |(approval_id, channel_id, agent_id, kind, key, approved_at)| {
                Ok(ChatChannelRememberedApprovalRead {
                    approval_id,
                    channel_id: ChatChannelId::new(channel_id).map_err(|_| corrupt_data_error())?,
                    agent_id: ChatParticipantId::new(agent_id).map_err(|_| corrupt_data_error())?,
                    kind: ChatRememberedApprovalKind::parse(&kind)
                        .ok_or_else(corrupt_data_error)?,
                    key,
                    approved_at: UtcTimestamp::new(approved_at)
                        .map_err(|_| corrupt_data_error())?,
                })
            },
        )
        .collect()
}

/// Revokes the approval whose id is `approval_id` at `now`. Revoking one that is already revoked
/// or gone changes nothing.
///
/// # Errors
///
/// Returns a persistence error when the write fails.
pub async fn revoke_remembered_approval(
    connection: &mut SqliteConnection,
    approval_id: &str,
    now: &str,
) -> ChatResult<()> {
    sqlx::query(
        "UPDATE chat_remembered_approvals SET revoked_at = ?, revision = revision + 1
         WHERE sync_key = ? AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(approval_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat remembered approval persistence failed",
        true,
    )
}

fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat remembered approvals are invalid",
        false,
    )
}

#[cfg(test)]
mod tests;
