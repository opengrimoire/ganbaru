//! Shared validation, wire conversion, and identifier helpers.

use super::super::channels::{identifier_error, persistence_error, u64_value};
use ganbaru_chat::coordination::contracts::*;
use ganbaru_chat_contracts::models::*;
use serde_json::Value;
#[cfg(desktop)]
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) const MAX_MESSAGE_BYTES: usize = 128 * 1024;

pub(super) fn has_thread_eligible_mention(request: &PostChatMessageCommand) -> bool {
    request.references.iter().any(|reference| {
        matches!(
            reference,
            ChatMessageReference::Participant {
                participant_kind: ChatParticipantKind::AiTeammate,
                ..
            }
        )
    })
}

#[cfg(desktop)]
pub(super) fn validate_policy(
    app: &tauri::AppHandle,
    policy: &ChatTeammatePolicyInput,
) -> ChatResult<()> {
    if policy.provider_managed_model == policy.model_id.is_some() {
        return Err(ChatError::validation(
            "modelId",
            "Choose either provider-managed model selection or one explicit model",
        ));
    }
    if let Some(effort) = policy.effort.as_deref() {
        if !is_valid_teammate_effort(effort) {
            return Err(ChatError::validation(
                "effort",
                "Teammate effort is invalid",
            ));
        }
    }
    if let Some(speed) = policy.speed.as_deref() {
        if !matches!(speed, "standard" | "fast") {
            return Err(ChatError::validation("speed", "Teammate speed is invalid"));
        }
    }
    super::super::settings_commands::read_provider(app, &policy.provider_instance_id)?;
    Ok(())
}

#[cfg(desktop)]
pub(super) fn is_valid_teammate_effort(effort: &str) -> bool {
    matches!(
        effort,
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
    )
}

pub(super) fn validate_message_request(request: &PostChatMessageCommand) -> ChatResult<()> {
    let markdown = request.normalized_markdown.trim();
    if markdown.is_empty() && request.attachment_ids.is_empty() && request.references.is_empty() {
        return Err(ChatError::validation(
            "normalizedMarkdown",
            "Message content is required",
        ));
    }
    if request.normalized_markdown.len() > MAX_MESSAGE_BYTES {
        return Err(ChatError::validation(
            "normalizedMarkdown",
            "Message content exceeds the 128 KiB limit",
        ));
    }
    json_object(&request.rich_content, "richContent")?;
    let mut ranges = Vec::with_capacity(request.references.len());
    let mut reference_ids = BTreeSet::new();
    let mut semantic_targets = BTreeSet::new();
    for reference in &request.references {
        let metadata = reference.metadata();
        let start = usize::try_from(metadata.start_offset)
            .map_err(|_| ChatError::validation("references", "Reference ranges are invalid"))?;
        let end = usize::try_from(metadata.end_offset)
            .map_err(|_| ChatError::validation("references", "Reference ranges are invalid"))?;
        if start >= end
            || end > request.normalized_markdown.len()
            || !request.normalized_markdown.is_char_boundary(start)
            || !request.normalized_markdown.is_char_boundary(end)
            || metadata.label_snapshot.trim().is_empty()
            || metadata.label_snapshot.chars().count() > 4_096
            || metadata.label_snapshot.chars().any(char::is_control)
            || metadata.plain_text_projection.is_empty()
            || metadata.plain_text_projection.chars().count() > 4_096
            || metadata.plain_text_projection.chars().any(char::is_control)
            || request.normalized_markdown.get(start..end)
                != Some(metadata.plain_text_projection.as_str())
            || !reference_ids.insert(metadata.reference_id.as_str())
            || !semantic_targets.insert(reference_semantic_key(reference))
        {
            return Err(ChatError::validation(
                "references",
                "Reference IDs, labels, projections, or UTF-8 byte ranges are invalid",
            ));
        }
        ranges.push((metadata.start_offset, metadata.end_offset));
        if let ChatMessageReference::WorkspacePath { relative_path, .. } = reference {
            if !safe_relative_path(relative_path) {
                return Err(ChatError::validation(
                    "references",
                    "A workspace path reference is invalid",
                ));
            }
        }
    }
    ranges.sort_unstable();
    if ranges.windows(2).any(|pair| pair[1].0 < pair[0].1) {
        return Err(ChatError::validation(
            "references",
            "Inline reference ranges must not overlap",
        ));
    }
    let mut attachments = BTreeSet::new();
    if request
        .attachment_ids
        .iter()
        .any(|id| !attachments.insert(id.as_str()))
    {
        return Err(ChatError::validation(
            "attachmentIds",
            "Attachment IDs must be distinct",
        ));
    }
    Ok(())
}

fn reference_semantic_key(reference: &ChatMessageReference) -> String {
    match reference {
        ChatMessageReference::Participant { participant_id, .. } => {
            format!("participant:{}", participant_id.as_str())
        }
        ChatMessageReference::Channel { channel_id, .. } => {
            format!("channel:{}", channel_id.as_str())
        }
        ChatMessageReference::WorkingFolder {
            working_folder_id, ..
        } => format!("working_folder:{}", working_folder_id.as_str()),
        ChatMessageReference::WorkspacePath {
            working_folder_id,
            path_kind,
            relative_path,
            ..
        } => format!(
            "workspace_path:{}:{path_kind:?}:{relative_path}",
            working_folder_id.as_str()
        ),
        ChatMessageReference::ExecutionEnvironment {
            execution_environment_id,
            ..
        } => format!(
            "execution_environment:{}",
            execution_environment_id.as_str()
        ),
    }
}

#[cfg(desktop)]
pub(super) fn validate_display_name(value: &str) -> ChatResult<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 160 || value.chars().any(char::is_control) {
        return Err(ChatError::validation(
            "displayName",
            "Teammate name must contain 1 to 160 characters",
        ));
    }
    Ok(value.to_string())
}

#[cfg(desktop)]
pub(super) fn validate_teammate_role(value: &str) -> ChatResult<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 1_000 || value.chars().any(char::is_control) {
        return Err(ChatError::validation(
            "role",
            "Teammate role must contain 1 to 1000 characters",
        ));
    }
    Ok(value.to_string())
}

#[cfg(desktop)]
pub(super) fn validate_profile_text(value: &str, maximum: usize, field: &str) -> ChatResult<()> {
    if value.len() > maximum {
        return Err(ChatError::validation(
            field,
            "Teammate profile text is too long",
        ));
    }
    Ok(())
}

#[cfg(desktop)]
pub(super) fn map_teammate_write_error(error: sqlx::Error) -> ChatError {
    let detail = error.to_string();
    if detail.contains("idx_chat_participants_teammate_display_name")
        || detail.contains("chat_participants.display_name")
    {
        ChatError::validation("displayName", "Name already in use")
    } else {
        persistence_error(error)
    }
}

pub(super) fn json_object(value: &VersionedJson, field: &str) -> ChatResult<String> {
    if !value.value.is_object() {
        return Err(ChatError::validation(field, "Value must be a JSON object"));
    }
    serde_json::to_string(&value.value).map_err(serialization_error)
}

pub(super) fn parse_json(data: String) -> ChatResult<Value> {
    serde_json::from_str(&data).map_err(serialization_error)
}

pub(super) fn parse_cursor(cursor: Option<&str>) -> ChatResult<Option<i64>> {
    cursor
        .map(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| ChatError::validation("cursor", "Chat page cursor is invalid"))
        })
        .transpose()
}

pub(super) fn normalized_fts_query(query: &str) -> ChatResult<String> {
    let terms = query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .take(20)
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    if terms.is_empty() || query.len() > 500 {
        return Err(ChatError::validation(
            "query",
            "Message search query is invalid",
        ));
    }
    Ok(terms.join(" AND "))
}

fn safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4_096
        && !path.starts_with('/')
        && !path.starts_with("../")
        && !path.contains("/../")
        && !path.ends_with("/..")
        && !path.contains('\\')
}

pub(super) fn conversation_item_id() -> ChatResult<ChatConversationItemId> {
    ChatConversationItemId::new(new_id("conversation-item")).map_err(identifier_error)
}

pub(super) fn message_revision_id() -> ChatResult<ChatMessageRevisionId> {
    ChatMessageRevisionId::new(new_id("message-revision")).map_err(identifier_error)
}

pub(super) fn reply_thread_id() -> ChatResult<ChatReplyThreadId> {
    ChatReplyThreadId::new(new_id("reply-thread")).map_err(identifier_error)
}

#[cfg(desktop)]
pub(super) fn work_assignment_id() -> ChatResult<ChatWorkAssignmentId> {
    ChatWorkAssignmentId::new(new_id("assignment")).map_err(identifier_error)
}

pub(super) fn parse_participant_kind(value: &str) -> ChatResult<ChatParticipantKind> {
    match value {
        "local_user" => Ok(ChatParticipantKind::LocalUser),
        "ai_teammate" => Ok(ChatParticipantKind::AiTeammate),
        "human" => Ok(ChatParticipantKind::Human),
        _ => Err(stored_value_error("participant kind")),
    }
}

pub(super) fn parse_approval_policy(value: &str) -> ChatResult<ChatApprovalPolicy> {
    match value {
        "ask_for_approval" => Ok(ChatApprovalPolicy::AskForApproval),
        "approve_for_me" => Ok(ChatApprovalPolicy::ApproveForMe),
        "full_access" => Ok(ChatApprovalPolicy::FullAccess),
        "custom" => Ok(ChatApprovalPolicy::Custom),
        _ => Err(stored_value_error("approval policy")),
    }
}

pub(super) fn parse_runtime_approval_policy(value: &str) -> ChatResult<ChatRuntimeApprovalPolicy> {
    match value {
        "ask" => Ok(ChatRuntimeApprovalPolicy::Ask),
        "auto_approve" => Ok(ChatRuntimeApprovalPolicy::AutoApprove),
        "unattended" => Ok(ChatRuntimeApprovalPolicy::Unattended),
        "provider_custom" => Ok(ChatRuntimeApprovalPolicy::ProviderCustom),
        _ => Err(stored_value_error("runtime approval policy")),
    }
}

#[cfg(desktop)]
pub(super) fn wire_runtime_approval_policy(value: ChatRuntimeApprovalPolicy) -> &'static str {
    match value {
        ChatRuntimeApprovalPolicy::Ask => "ask",
        ChatRuntimeApprovalPolicy::AutoApprove => "auto_approve",
        ChatRuntimeApprovalPolicy::Unattended => "unattended",
        ChatRuntimeApprovalPolicy::ProviderCustom => "provider_custom",
    }
}

pub(super) fn parse_folder_capability(value: &str) -> ChatResult<ChatFolderCapability> {
    match value {
        "none" => Ok(ChatFolderCapability::None),
        "read" => Ok(ChatFolderCapability::Read),
        "edit" => Ok(ChatFolderCapability::Edit),
        "execute" => Ok(ChatFolderCapability::Execute),
        "publish" => Ok(ChatFolderCapability::Publish),
        _ => Err(stored_value_error("folder capability")),
    }
}

#[cfg(desktop)]
pub(super) fn wire_folder_capability(value: ChatFolderCapability) -> &'static str {
    match value {
        ChatFolderCapability::None => "none",
        ChatFolderCapability::Read => "read",
        ChatFolderCapability::Edit => "edit",
        ChatFolderCapability::Execute => "execute",
        ChatFolderCapability::Publish => "publish",
    }
}

pub(super) fn parse_history_boundary(
    kind: &str,
    lower_ordinal: Option<i64>,
) -> ChatResult<ChatHistoryBoundary> {
    match kind {
        "entire" if lower_ordinal.is_none() => Ok(ChatHistoryBoundary::Entire),
        "from_grant" => Ok(ChatHistoryBoundary::FromGrant {
            lower_ordinal: lower_ordinal.map(u64_value).transpose()?,
        }),
        _ => Err(stored_value_error("history boundary")),
    }
}

#[cfg(desktop)]
pub(super) fn wire_history_boundary(value: &ChatHistoryBoundary) -> &'static str {
    match value {
        ChatHistoryBoundary::Entire => "entire",
        ChatHistoryBoundary::FromGrant { .. } => "from_grant",
    }
}

pub(super) fn parse_access_profile_builtin_key(
    value: Option<&str>,
) -> ChatResult<Option<ChatAccessProfileBuiltinKey>> {
    value
        .map(|value| match value {
            "conversation_only" => Ok(ChatAccessProfileBuiltinKey::ConversationOnly),
            "read_only" => Ok(ChatAccessProfileBuiltinKey::ReadOnly),
            "edit_files" => Ok(ChatAccessProfileBuiltinKey::EditFiles),
            "build_and_test" => Ok(ChatAccessProfileBuiltinKey::BuildAndTest),
            "publish_changes" => Ok(ChatAccessProfileBuiltinKey::PublishChanges),
            _ => Err(stored_value_error("access profile built-in key")),
        })
        .transpose()
}

pub(super) fn parse_work_state(value: &str) -> ChatResult<ChatWorkAssignmentState> {
    match value {
        "queued" => Ok(ChatWorkAssignmentState::Queued),
        "working" => Ok(ChatWorkAssignmentState::Working),
        "waiting_for_answer" => Ok(ChatWorkAssignmentState::WaitingForAnswer),
        "waiting_for_approval" => Ok(ChatWorkAssignmentState::WaitingForApproval),
        "ready_for_review" => Ok(ChatWorkAssignmentState::ReadyForReview),
        "completed" => Ok(ChatWorkAssignmentState::Completed),
        "failed" => Ok(ChatWorkAssignmentState::Failed),
        "cancelled" => Ok(ChatWorkAssignmentState::Cancelled),
        _ => Err(stored_value_error("assignment state")),
    }
}

pub(super) fn wire_participant_kind(value: ChatParticipantKind) -> &'static str {
    match value {
        ChatParticipantKind::LocalUser => "local_user",
        ChatParticipantKind::AiTeammate => "ai_teammate",
        ChatParticipantKind::Human => "human",
    }
}

#[cfg(desktop)]
pub(super) fn wire_approval_policy(value: ChatApprovalPolicy) -> &'static str {
    match value {
        ChatApprovalPolicy::AskForApproval => "ask_for_approval",
        ChatApprovalPolicy::ApproveForMe => "approve_for_me",
        ChatApprovalPolicy::FullAccess => "full_access",
        ChatApprovalPolicy::Custom => "custom",
    }
}

#[cfg(desktop)]
pub(super) fn wire_work_state(value: ChatWorkAssignmentState) -> &'static str {
    match value {
        ChatWorkAssignmentState::Queued => "queued",
        ChatWorkAssignmentState::Working => "working",
        ChatWorkAssignmentState::WaitingForAnswer => "waiting_for_answer",
        ChatWorkAssignmentState::WaitingForApproval => "waiting_for_approval",
        ChatWorkAssignmentState::ReadyForReview => "ready_for_review",
        ChatWorkAssignmentState::Completed => "completed",
        ChatWorkAssignmentState::Failed => "failed",
        ChatWorkAssignmentState::Cancelled => "cancelled",
    }
}

pub(super) fn u32_value(value: i64) -> ChatResult<u32> {
    u32::try_from(value).map_err(|_| stored_value_error("schema version"))
}

#[cfg(desktop)]
pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(desktop)]
pub(super) fn truncate_utf8(value: &str, maximum: usize) -> &str {
    if value.len() <= maximum {
        return value;
    }
    let mut boundary = maximum;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &value[..boundary]
}

fn stored_value_error(label: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        format!("Stored Chat {label} is invalid"),
        false,
    )
}

pub(super) fn new_id(prefix: &str) -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}:{nanos:032x}{sequence:016x}")
}

pub(super) fn serialization_error<E>(_error: E) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat JSON data is invalid",
        false,
    )
}

pub(super) fn map_command_receipt_error(error: sqlx::Error) -> ChatError {
    if error
        .to_string()
        .contains("chat_organizational_command_receipts.client_command_id")
    {
        ChatError::new(
            ChatErrorCode::Conflict,
            "This message command was already accepted",
            true,
        )
    } else {
        persistence_error(error)
    }
}
