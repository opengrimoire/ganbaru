//! Shared validation, wire conversion, and identifier helpers.

use super::super::channels::{identifier_error, persistence_error};
use ganbaru_chat::coordination::contracts::*;
pub(super) use ganbaru_chat::ids::new_id;
use ganbaru_chat::ids::{REPLY_THREAD_ID_PREFIX, derived_id};
use ganbaru_chat_contracts::models::*;
use serde_json::Value;
#[cfg(desktop)]
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub(super) use ganbaru_chat::timeline::MAX_MESSAGE_MARKDOWN_BYTES as MAX_MESSAGE_BYTES;
use ganbaru_chat::timeline::{MAX_ATTACHMENT_ID_CHARS, MAX_MESSAGE_ATTACHMENTS};

/// Owner of attachment references copied by value from the device's attachment rows.
#[derive(Clone, Copy, Debug)]
pub(super) enum AttachmentReferenceOwner {
    /// A message revision, through its replicated owned references.
    Revision,
    /// A scheduled message, through its device-local references.
    ScheduledMessage,
}

impl AttachmentReferenceOwner {
    fn insert_sql(self) -> &'static str {
        match self {
            Self::Revision => {
                "INSERT INTO chat_communication_attachment_references
                     (message_revision_id, ordinal, attachment_id, working_folder_id, kind,
                      original_display_name, mime_type, byte_size, sha256)
                 SELECT ?, ?, id, working_folder_id, kind, original_display_name, mime_type,
                        byte_size, sha256
                 FROM chat_attachments WHERE id = ?"
            }
            Self::ScheduledMessage => {
                "INSERT INTO chat_scheduled_message_attachment_references
                     (scheduled_message_id, ordinal, attachment_id, working_folder_id, kind,
                      original_display_name, mime_type, byte_size, sha256)
                 SELECT ?, ?, id, working_folder_id, kind, original_display_name, mime_type,
                        byte_size, sha256
                 FROM chat_attachments WHERE id = ?"
            }
        }
    }
}

/// Inserts the references of `owner_id` to `attachment_ids` in order, copying each attachment's
/// working folder, kind, display name, content type, size, and digest from its row on this
/// device so the reference stays readable without that row.
///
/// # Errors
///
/// Returns a validation error when an attachment does not exist on this device.
pub(super) async fn insert_attachment_references(
    connection: &mut sqlx::SqliteConnection,
    owner: AttachmentReferenceOwner,
    owner_id: &str,
    attachment_ids: &[ChatAttachmentId],
) -> ChatResult<()> {
    for (ordinal, attachment_id) in attachment_ids.iter().enumerate() {
        let inserted = sqlx::query(owner.insert_sql())
            .bind(owner_id)
            .bind(i64::try_from(ordinal).unwrap_or(i64::MAX))
            .bind(attachment_id.as_str())
            .execute(&mut *connection)
            .await
            .map_err(persistence_error)?
            .rows_affected();
        if inserted != 1 {
            return Err(ChatError::validation(
                "attachmentIds",
                "Attachment was not found",
            ));
        }
    }
    Ok(())
}

/// Where a posted revision takes its attachment references from.
#[derive(Clone, Copy, Debug)]
pub(super) enum AttachmentSource<'a> {
    /// The device's attachment rows named by the request.
    Device(&'a [ChatAttachmentId]),
    /// The values a scheduled message stored, so a send needs no attachment row on this device.
    Scheduled {
        /// Scheduled message whose references are copied.
        scheduled_message_id: &'a str,
        /// Number of attachments the scheduled request names.
        expected: usize,
    },
}

/// Inserts the attachment references of the revision `revision_id` from `source`.
///
/// # Errors
///
/// Returns a validation error when a device attachment does not exist, or when the scheduled
/// message holds a different number of attachment references than its request names.
pub(super) async fn insert_revision_attachments(
    connection: &mut sqlx::SqliteConnection,
    revision_id: &str,
    source: AttachmentSource<'_>,
) -> ChatResult<()> {
    match source {
        AttachmentSource::Device(attachment_ids) => {
            insert_attachment_references(
                connection,
                AttachmentReferenceOwner::Revision,
                revision_id,
                attachment_ids,
            )
            .await
        }
        AttachmentSource::Scheduled {
            scheduled_message_id,
            expected,
        } => {
            let inserted = sqlx::query(
                "INSERT INTO chat_communication_attachment_references
                     (message_revision_id, ordinal, attachment_id, working_folder_id, kind,
                      original_display_name, mime_type, byte_size, sha256)
                 SELECT ?, ordinal, attachment_id, working_folder_id, kind,
                        original_display_name, mime_type, byte_size, sha256
                 FROM chat_scheduled_message_attachment_references
                 WHERE scheduled_message_id = ?
                 ORDER BY ordinal",
            )
            .bind(revision_id)
            .bind(scheduled_message_id)
            .execute(&mut *connection)
            .await
            .map_err(persistence_error)?
            .rows_affected();
            if usize::try_from(inserted).ok() != Some(expected) {
                return Err(ChatError::validation(
                    "attachmentIds",
                    "The scheduled message attachments are incomplete",
                ));
            }
            Ok(())
        }
    }
}

/// Active folder grants of one AI channel membership, bound to the conversation and teammate.
///
/// Concurrent edits on two devices can each leave a default grant after sync merges them, so
/// only the default grant with the lowest working folder id reads as the default. Execution
/// context selection breaks the tie the same way.
pub(super) const ACTIVE_FOLDER_GRANTS_SQL: &str = "SELECT grant_row.working_folder_id,
        folder.display_name, grant_row.capability, grant_row.capability_inherits_profile,
        (grant_row.is_default <> 0 AND NOT EXISTS (
            SELECT 1 FROM chat_teammate_working_folder_grants other
            WHERE other.conversation_id = grant_row.conversation_id
              AND other.teammate_id = grant_row.teammate_id
              AND other.revoked_at IS NULL AND other.is_default <> 0
              AND other.working_folder_id < grant_row.working_folder_id
        )) AS is_default,
        grant_row.runtime_approval_policy, grant_row.revision, grant_row.revoked_at
 FROM chat_teammate_working_folder_grants grant_row
 JOIN project_working_folders folder ON folder.id = grant_row.working_folder_id
 WHERE grant_row.conversation_id = ? AND grant_row.teammate_id = ?
   AND grant_row.revoked_at IS NULL
 ORDER BY is_default DESC, folder.sort_order, folder.display_name, folder.id";

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
    if json_object(&request.rich_content, "richContent")?.len()
        > ganbaru_chat::timeline::MAX_RICH_CONTENT_BYTES
    {
        return Err(ganbaru_chat::timeline::rich_content_too_large());
    }
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
    if request.attachment_ids.len() > MAX_MESSAGE_ATTACHMENTS {
        return Err(ChatError::validation(
            "attachmentIds",
            "A message can carry at most 20 attachments",
        ));
    }
    if request
        .attachment_ids
        .iter()
        .any(|id| id.as_str().chars().count() > MAX_ATTACHMENT_ID_CHARS)
    {
        return Err(ChatError::validation(
            "attachmentIds",
            "Attachment ID is too long",
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

/// Refuses a teammate name that another teammate, archived ones included, already uses, ignoring
/// case and surrounding spaces.
///
/// The check covers the teammates this device holds. Devices that create or rename teammates
/// concurrently can still end up with equal names, which readers tell apart by id.
#[cfg(desktop)]
pub(super) async fn require_unused_teammate_name(
    connection: &mut sqlx::SqliteConnection,
    display_name: &str,
    teammate_id: &str,
) -> ChatResult<()> {
    let in_use: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_participants
           WHERE participant_kind = 'ai_teammate' AND id != ?
             AND lower(trim(display_name)) = lower(trim(?))
         )",
    )
    .bind(teammate_id)
    .bind(display_name)
    .fetch_one(connection)
    .await
    .map_err(persistence_error)?;
    if in_use {
        return Err(ChatError::validation("displayName", "Name already in use"));
    }
    Ok(())
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

/// A timeline page cursor: a position `(order_key, item_id)` and whether the page includes it.
///
/// Cursors are `before:{order_key}:{item_id}` for pages that end just before the position and
/// `through:{order_key}:{item_id}` for pages that end at it. Order keys never contain `:`, so
/// the first `:` after the prefix ends the key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PageCursor {
    pub(super) order_key: String,
    pub(super) item_id: String,
    pub(super) inclusive: bool,
}

const BEFORE_CURSOR_PREFIX: &str = "before:";
const THROUGH_CURSOR_PREFIX: &str = "through:";

impl PageCursor {
    /// The cursor of a page that ends just before `(order_key, item_id)`.
    pub(super) fn before(order_key: &str, item_id: &str) -> String {
        format!("{BEFORE_CURSOR_PREFIX}{order_key}:{item_id}")
    }

    /// The cursor of a page that ends at `(order_key, item_id)`.
    pub(super) fn through(order_key: &str, item_id: &str) -> String {
        format!("{THROUGH_CURSOR_PREFIX}{order_key}:{item_id}")
    }
}

pub(super) fn parse_cursor(cursor: Option<&str>) -> ChatResult<Option<PageCursor>> {
    cursor
        .map(|value| {
            let (inclusive, position) = if let Some(rest) = value.strip_prefix(BEFORE_CURSOR_PREFIX)
            {
                (false, rest)
            } else if let Some(rest) = value.strip_prefix(THROUGH_CURSOR_PREFIX) {
                (true, rest)
            } else {
                return Err(invalid_cursor());
            };
            let (order_key, item_id) = position.split_once(':').ok_or_else(invalid_cursor)?;
            if ganbaru_sync_contracts::OrderKey::parse(order_key).is_err() || item_id.is_empty() {
                return Err(invalid_cursor());
            }
            Ok(PageCursor {
                order_key: order_key.to_owned(),
                item_id: item_id.to_owned(),
                inclusive,
            })
        })
        .transpose()
}

fn invalid_cursor() -> ChatError {
    ChatError::validation("cursor", "Chat page cursor is invalid")
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
    ChatConversationItemId::new(new_id("conversation-item")?).map_err(identifier_error)
}

pub(super) fn message_revision_id() -> ChatResult<ChatMessageRevisionId> {
    ChatMessageRevisionId::new(new_id("message-revision")?).map_err(identifier_error)
}

/// Item and revision ids of one message a scheduled send posts, derived from the scheduled
/// message so a repeated send names the same rows.
pub(super) struct ScheduledPostIds {
    /// The posted message item.
    pub(super) item_id: ChatConversationItemId,
    /// The posted message's first revision.
    pub(super) revision_id: ChatMessageRevisionId,
}

/// Which message of a scheduled send an id names.
#[derive(Clone, Copy, Debug)]
pub(super) enum ScheduledPostTarget {
    /// The message posted where the scheduled message was written.
    Message,
    /// The channel copy of a reply that is also sent to the channel.
    ChannelCopy,
}

/// Returns the derived ids of a scheduled send's message or channel copy.
pub(super) fn scheduled_post_ids(
    scheduled_message_id: &str,
    target: ScheduledPostTarget,
) -> ChatResult<ScheduledPostIds> {
    let role = match target {
        ScheduledPostTarget::Message => "scheduled",
        ScheduledPostTarget::ChannelCopy => "scheduled-channel-copy",
    };
    Ok(ScheduledPostIds {
        item_id: ChatConversationItemId::new(derived_id(
            "conversation-item",
            &[role, scheduled_message_id],
        )?)
        .map_err(identifier_error)?,
        revision_id: ChatMessageRevisionId::new(derived_id(
            "message-revision",
            &[role, scheduled_message_id],
        )?)
        .map_err(identifier_error)?,
    })
}

/// Returns the reply thread id derived from its root item, so every device that opens the thread
/// for the same root message agrees on its id.
pub(super) fn reply_thread_id(
    root_item_id: &ChatConversationItemId,
) -> ChatResult<ChatReplyThreadId> {
    ChatReplyThreadId::new(derived_id(
        REPLY_THREAD_ID_PREFIX,
        &[root_item_id.as_str()],
    )?)
    .map_err(identifier_error)
}

/// Returns the work assignment input id derived from its message, so one message routes to at
/// most one input whichever device writes it.
pub(super) fn assignment_input_id(message_item_id: &str) -> ChatResult<String> {
    derived_id(
        ganbaru_chat::ids::ASSIGNMENT_INPUT_ID_PREFIX,
        &[message_item_id],
    )
}

/// Returns the work assignment id derived from its triggering message, its teammate, and the
/// assignment it follows, so concurrent Starts of one mention on two devices write one
/// assignment that sync merges instead of two that both run.
pub(super) fn work_assignment_id(
    message_item_id: &ChatConversationItemId,
    teammate_id: &ChatParticipantId,
    previous_assignment_id: Option<&ChatWorkAssignmentId>,
) -> ChatResult<ChatWorkAssignmentId> {
    let mut parts = vec![message_item_id.as_str(), teammate_id.as_str()];
    parts.extend(previous_assignment_id.map(ChatWorkAssignmentId::as_str));
    ChatWorkAssignmentId::new(derived_id(
        ganbaru_chat::ids::WORK_ASSIGNMENT_ID_PREFIX,
        &parts,
    )?)
    .map_err(identifier_error)
}

pub(super) fn parse_participant_kind(value: &str) -> ChatResult<ChatParticipantKind> {
    match value {
        "local_user" => Ok(ChatParticipantKind::LocalUser),
        "ai_teammate" => Ok(ChatParticipantKind::AiTeammate),
        "human" => Ok(ChatParticipantKind::Human),
        _ => Err(stored_value_error("participant kind")),
    }
}

pub(crate) fn parse_approval_policy(value: &str) -> ChatResult<ChatApprovalPolicy> {
    match value {
        "ask_for_approval" => Ok(ChatApprovalPolicy::AskForApproval),
        "approve_for_me" => Ok(ChatApprovalPolicy::ApproveForMe),
        "full_access" => Ok(ChatApprovalPolicy::FullAccess),
        "custom" => Ok(ChatApprovalPolicy::Custom),
        _ => Err(stored_value_error("approval policy")),
    }
}

pub(crate) fn parse_runtime_approval_policy(value: &str) -> ChatResult<ChatRuntimeApprovalPolicy> {
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

/// Order key a `from_grant` history boundary stamps when it is granted now: the first root
/// position after the conversation's last root item, so only later root items are visible.
pub(super) async fn from_grant_lower_order_key(
    connection: &mut sqlx::SqliteConnection,
    conversation_id: &str,
) -> ChatResult<String> {
    let last_key =
        ganbaru_chat::timeline::last_order_key(connection, conversation_id, None).await?;
    ganbaru_chat::timeline::order_key_after(last_key.as_deref())
}

pub(super) fn parse_history_boundary(
    kind: &str,
    lower_order_key: Option<String>,
) -> ChatResult<ChatHistoryBoundary> {
    match kind {
        "entire" if lower_order_key.is_none() => Ok(ChatHistoryBoundary::Entire),
        "from_grant" => Ok(ChatHistoryBoundary::FromGrant { lower_order_key }),
        _ => Err(stored_value_error("history boundary")),
    }
}

/// Channel history one teammate membership may read, resolved from the
/// membership and its access profile revision.
///
/// Positions are root timeline order keys. A reply thread sits at its root's
/// position, because reply order keys count within their thread.
#[cfg(desktop)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TeammateHistoryAccess {
    visible_from: Option<String>,
}

#[cfg(desktop)]
impl TeammateHistoryAccess {
    /// Resolves effective history access. A `from_grant` boundary without a
    /// lower order key admits nothing; entire history starts at the empty key,
    /// which sorts before every order key.
    pub(super) fn resolve(
        read_history: bool,
        from_grant: bool,
        from_order_key: Option<String>,
    ) -> Self {
        let visible_from = match (read_history, from_grant) {
            (false, _) => None,
            (true, true) => from_order_key,
            (true, false) => Some(String::new()),
        };
        Self { visible_from }
    }

    /// Returns the lowest readable order key, or `None` when no history is readable.
    pub(super) fn visible_from(&self) -> Option<&str> {
        self.visible_from.as_deref()
    }

    /// Returns whether the message or reply thread at `order_key` is readable.
    pub(super) fn admits_position(&self, order_key: &str) -> bool {
        self.visible_from
            .as_deref()
            .is_some_and(|lower| lower <= order_key)
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

/// Returns the runtime approval a teammate's approval behavior stands for when no channel,
/// scratch, or folder override applies.
#[cfg(desktop)]
pub(crate) fn teammate_runtime_approval(value: ChatApprovalPolicy) -> ChatRuntimeApprovalPolicy {
    match value {
        ChatApprovalPolicy::AskForApproval => ChatRuntimeApprovalPolicy::Ask,
        ChatApprovalPolicy::ApproveForMe => ChatRuntimeApprovalPolicy::AutoApprove,
        ChatApprovalPolicy::FullAccess => ChatRuntimeApprovalPolicy::Unattended,
        ChatApprovalPolicy::Custom => ChatRuntimeApprovalPolicy::ProviderCustom,
    }
}

/// Orders runtime approval policies from narrowest to broadest. Provider-custom approval has no
/// rank because the provider's own configuration decides it.
#[cfg(desktop)]
pub(crate) fn runtime_approval_rank(policy: ChatRuntimeApprovalPolicy) -> Option<u8> {
    match policy {
        ChatRuntimeApprovalPolicy::Ask => Some(0),
        ChatRuntimeApprovalPolicy::AutoApprove => Some(1),
        ChatRuntimeApprovalPolicy::Unattended => Some(2),
        ChatRuntimeApprovalPolicy::ProviderCustom => None,
    }
}

/// Returns the runtime approval that applies to a teammate's work: the narrower of the runtime
/// approval its approval behavior stands for and the policy resolved from its grants. A Custom
/// teammate cannot be ranked against a grant, so the grant policy alone governs host tools while
/// the provider keeps its own configuration; an unranked grant is returned unchanged so native
/// execution can refuse it.
#[cfg(desktop)]
pub(crate) fn effective_runtime_approval(
    teammate_default: ChatRuntimeApprovalPolicy,
    granted: ChatRuntimeApprovalPolicy,
) -> ChatRuntimeApprovalPolicy {
    match (
        runtime_approval_rank(teammate_default),
        runtime_approval_rank(granted),
    ) {
        (Some(teammate_rank), Some(granted_rank)) if teammate_rank < granted_rank => {
            teammate_default
        }
        _ => granted,
    }
}

/// Returns the provider safety mode a native assignment runs with for a teammate and its frozen
/// effective runtime approval. Only a Custom teammate runs with the provider's own
/// configuration; an unranked grant on a ranked teammate falls back to asking.
#[cfg(desktop)]
pub(crate) fn native_assignment_safety_mode(
    teammate: ChatApprovalPolicy,
    effective: ChatRuntimeApprovalPolicy,
) -> SafetyMode {
    if teammate == ChatApprovalPolicy::Custom {
        return SafetyMode::Custom;
    }
    match effective {
        ChatRuntimeApprovalPolicy::Ask | ChatRuntimeApprovalPolicy::ProviderCustom => {
            SafetyMode::AskForApproval
        }
        ChatRuntimeApprovalPolicy::AutoApprove => SafetyMode::ApproveForMe,
        ChatRuntimeApprovalPolicy::Unattended => SafetyMode::FullAccess,
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

pub(super) fn serialization_error<E>(_error: E) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat JSON data is invalid",
        false,
    )
}

/// The stable id of this device, which decides where assignments and scheduled messages run.
pub(super) fn this_device_id<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> ChatResult<String> {
    crate::sync::devices::stable_device_id(app).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Persistence,
            "This device's identity is unavailable",
            true,
        )
    })
}

/// The devices that can write Chat in the active vault: its owner, and this device when it is a
/// linked replica that Chat lets write. Other linked replicas are not listed, so while Chat is
/// owner-only the owner is the only writer.
pub(super) fn chat_writers<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> ChatResult<super::workflow::ChatWriters> {
    let status = crate::vault::ownership::active_status(app).map_err(|error| {
        ChatError::new(
            ChatErrorCode::Persistence,
            format!("This vault's owner is unavailable: {error}"),
            true,
        )
    })?;
    Ok(chat_writers_from_status(&status))
}

/// The devices that write Chat under the ownership `status`: the vault owner, and this device
/// when it may write the Chat domain.
pub(super) fn chat_writers_from_status(
    status: &crate::vault::ownership::VaultOwnershipStatus,
) -> super::workflow::ChatWriters {
    let mut devices = vec![status.owner_device_id.clone()];
    if status.can_write_domain(ganbaru_sync::manifest::vault::chat::DOMAIN_NAME) {
        devices.push(status.device_id.clone());
    }
    super::workflow::ChatWriters::new(devices)
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
