//! Chat adapter: the value checks that mirror the Chat identity and policy table checks, and the
//! priorities of their coupled groups.
//!
//! Agent access at the vault, project, and channel scopes keeps the narrower level among
//! concurrent edits: talk only outranks read only, which outranks ask before commands, which
//! outranks own the folder, and an explicit level outranks inheriting. An edit made after seeing
//! the other still wins by causality.
//!
//! Conversation memberships and AI channel memberships keep the narrower access among concurrent
//! edits: removal first, then demotion. A push approval revocation outranks a concurrent
//! approval, a channel archive outranks a concurrent unarchive, and a message deletion outranks
//! a concurrent restore.
//!
//! Timeline values mirror their table checks too, including the byte bounds that keep one
//! message revision within an operation. A revision's attachments carry their working folder,
//! kind, display name, content type, size, and digest by value, and each non-empty one
//! references its `chat/attachments` file. A scheduled message's content carries its
//! attachments the same way, and its request and mention references stay within one operation.
//!
//! A settled work assignment outranks a concurrent active state, and a review request
//! outranks the other active states.

use ganbaru_sync_contracts::bounds::{MAX_DEVICE_ID_BYTES, MAX_ORDER_KEY_BYTES};
use ganbaru_sync_contracts::chat_agents::is_named_agent_key;
use ganbaru_sync_contracts::chat_provider_status::{
    is_canonical_catalog, is_canonical_limits, provider_status_key,
};
use ganbaru_sync_contracts::chat_push_approvals::{
    MAX_PUSH_APPROVAL_BRANCH_BYTES, MAX_PUSH_APPROVAL_URL_BYTES,
};
use ganbaru_sync_contracts::chat_recurrence;
use ganbaru_sync_contracts::chat_remembered_approvals::{
    SUBJECT_KINDS, subject_key_is_well_formed,
};
use ganbaru_sync_contracts::chat_usage::agent_usage_key;
use ganbaru_sync_contracts::{Cause, Field, GroupId, GroupValue, OrderKey, RowKey, TableId, Value};
use ganbaru_working_folders::UtcTimestamp;

use crate::adapter::{AdapterError, DomainAdapter, OfferedRows, Presentation};
use crate::assets::AssetRef;
use crate::codec;
use crate::domains::distractions::is_usage_date;
use crate::manifest::OwnedTable;
use crate::manifest::vault::chat::{
    ADAPTER, AGENT_USAGE_DAILY_TABLE, AGENTS_TABLE, APP_SETTINGS_TABLE, ASSIGNMENTS_TABLE,
    ATTACHMENT_REFERENCES, ATTACHMENTS_CLASS, CHANNEL_MEMBERSHIPS_TABLE, CHANNEL_NOTES_TABLE,
    CHANNEL_TARGETS_TABLE, CHANNELS_TABLE, CONVERSATION_CURSORS_TABLE, CONVERSATIONS_TABLE,
    ENVIRONMENT_TARGETS_TABLE, FOLDER_TARGETS_TABLE, INPUTS_TABLE, ITEMS_TABLE, MEMBERSHIPS_TABLE,
    MESSAGES_TABLE, PARTICIPANT_TARGETS_TABLE, PARTICIPANTS_TABLE, PATH_TARGETS_TABLE,
    POLICY_REVISIONS_TABLE, PROJECT_SETTINGS_TABLE, PROVIDER_STATUS_TABLE, PUSH_APPROVALS_TABLE,
    RECURRENCES_TABLE, REFERENCES_TABLE, REMEMBERED_APPROVALS_TABLE, REPLY_CURSORS_TABLE,
    REPLY_THREADS_TABLE, REVISIONS_TABLE, SCHEDULED_ATTACHMENT_REFERENCES, SCHEDULED_REFERENCES,
    SCHEDULED_TABLE, SEMANTIC_UPDATES_TABLE, agent_group, agent_usage_group, app_settings_group,
    assignment_group, channel_group, channel_membership_group, channel_note_group,
    conversation_group, input_group, item_group, membership_group, message_group,
    participant_group, policy_revision_group, project_settings_group, provider_status_group,
    push_approval_group, read_cursor_group, recurrence_group, remembered_approval_group,
    reply_thread_group, scheduled_group, semantic_update_group, timeline_group,
};

/// Characters of a display name after trimming.
pub const MAX_DISPLAY_NAME_CHARS: usize = 160;
/// Characters of the agent instructions one scope holds.
pub const MAX_INSTRUCTIONS_CHARS: usize = 16_384;
/// Characters of the instructions a work assignment fixes: the three scopes with their
/// separators.
pub const MAX_RESOLVED_INSTRUCTIONS_CHARS: usize = 3 * MAX_INSTRUCTIONS_CHARS + 1024;
/// Characters of the model label a work assignment records.
pub const MAX_MODEL_LABEL_CHARS: usize = 256;
/// Characters of a provider instance id.
pub const MAX_PROVIDER_INSTANCE_CHARS: usize = 1024;
/// Characters of a stored JSON object: an avatar, a model selection, or provider options.
pub const MAX_JSON_CHARS: usize = 16_384;
/// Characters of the longest enumerated value any Chat replicated column holds.
pub const MAX_ENUM_CHARS: usize = 32;

/// Participant kinds.
pub const PARTICIPANT_KINDS: [&str; 3] = ["local_user", "ai_agent", "human"];
/// Participant kind of the local user.
const LOCAL_USER_KIND: &str = "local_user";
/// The key the baseline seeds for the local user, the only one the kind may hold.
pub const LOCAL_OWNER_PARTICIPANT_KEY: &str = "participant:local-owner";
/// Agent access levels, widest first: own the folder, ask before commands, read only, and talk
/// only.
pub const AGENT_ACCESS_LEVELS: [&str; 4] = ["own", "ask", "read", "talk"];
/// Interaction modes.
pub const INTERACTION_MODES: [&str; 1] = ["build"];
/// Characters of a channel name.
pub const MAX_CHANNEL_NAME_CHARS: usize = 80;
/// Characters of a channel topic.
pub const MAX_CHANNEL_TOPIC_CHARS: usize = 250;
/// Characters of a channel note body after trimming.
pub use ganbaru_sync_contracts::chat_channel_notes::MAX_CHANNEL_NOTE_CHARS;
/// Conversation kinds.
pub const CONVERSATION_KINDS: [&str; 3] = ["channel", "direct_message", "task_discussion"];
/// Conversation membership roles, least restrictive first.
pub const MEMBERSHIP_ROLES: [&str; 2] = ["owner", "member"];
/// Characters of a Chat row id the timeline tables hold, as the tables check it.
pub const MAX_ID_CHARS: usize = 1024;
/// Bytes of a Chat participant id, the Chat organizational identifier limit.
pub const MAX_PARTICIPANT_ID_BYTES: usize = 128;
/// Characters of an execution environment id.
pub const MAX_ENVIRONMENT_ID_CHARS: usize = 2048;
/// Characters of a mention label or its plain text projection.
pub const MAX_LABEL_CHARS: usize = 4096;
/// Characters of a workspace path in a mention.
pub const MAX_RELATIVE_PATH_CHARS: usize = 4096;
/// Bytes of a message revision's normalized Markdown, which also bounds its characters.
pub const MAX_MESSAGE_MARKDOWN_BYTES: usize = 128 * 1024;
/// Bytes of a message revision's rich content, which also bounds its characters.
pub const MAX_RICH_CONTENT_BYTES: usize = 192 * 1024;
/// Largest attachment in bytes.
pub const MAX_ATTACHMENT_BYTES: u32 = 50 * 1024 * 1024;
/// Most attachments on one message revision.
pub const MAX_MESSAGE_ATTACHMENTS: usize = 20;
/// Characters of an attachment id and its working folder id in an attachment reference, kept
/// small so a revision with every attachment still fits one operation.
pub const MAX_ATTACHMENT_ID_CHARS: usize = 128;
/// Characters of an attachment display name.
pub const MAX_ATTACHMENT_NAME_CHARS: usize = 1000;
/// Characters of an attachment content type.
pub const MAX_ATTACHMENT_MIME_CHARS: usize = 64;
/// Characters of a lowercase hex SHA-256 digest.
pub const SHA256_HEX_CHARS: usize = 64;
/// Attachment content types with the attachment kind each one has.
pub const ATTACHMENT_TYPES: [(&str, &str); 5] = [
    ("image/png", "image"),
    ("image/jpeg", "image"),
    ("image/gif", "image"),
    ("image/webp", "image"),
    ("text/plain", "text_snippet"),
];
/// Item kinds of the conversation timeline.
pub const ITEM_KINDS: [&str; 5] = [
    "message",
    "work_update",
    "approval_request",
    "question",
    "review_packet",
];
/// Kinds of message references.
pub const REFERENCE_KINDS: [&str; 5] = [
    "participant",
    "channel",
    "working_folder",
    "workspace_path",
    "execution_environment",
];
/// Kinds of a workspace path in a mention.
pub const PATH_KINDS: [&str; 2] = ["file", "folder"];
/// Bytes of a scheduled message's request, which also bounds its characters.
pub const MAX_SCHEDULED_REQUEST_BYTES: usize = 256 * 1024;
/// Bytes of a scheduled message's request and encoded mention references together, so its
/// content with every attachment fits one operation.
pub const MAX_SCHEDULED_CONTENT_BYTES: usize = 320 * 1024;
/// Most mention references of one scheduled message.
pub const MAX_SCHEDULED_REFERENCES: usize = 256;
/// Characters of a failure or state reason.
pub const MAX_REASON_CHARS: usize = 4000;
/// Bytes of a semantic update payload, which also bounds its characters.
pub const MAX_SEMANTIC_PAYLOAD_BYTES: usize = MAX_RICH_CONTENT_BYTES;
/// The version of a scheduled message request.
pub const SCHEDULED_REQUEST_VERSION: i64 = 1;
/// Work assignment states, active first.
pub const ASSIGNMENT_STATES: [&str; 8] = [
    "queued",
    "working",
    "waiting_for_answer",
    "waiting_for_approval",
    "ready_for_review",
    "completed",
    "failed",
    "cancelled",
];
/// The only assignment state a review outcome may accompany.
pub const REVIEWED_ASSIGNMENT_STATE: &str = "completed";
/// Outcomes of a review of ready-for-review work.
pub const ASSIGNMENT_REVIEW_OUTCOMES: [&str; 2] = ["accepted", "changes_requested"];
/// How a message reached an assignment.
pub const ROUTING_KINDS: [&str; 4] = ["trigger", "steer", "queued_continuation", "follow_up"];
/// Delivery states of an assignment input.
pub const DELIVERY_STATES: [&str; 4] = ["pending", "delivered", "failed", "ignored"];
/// Delivery state of an input that reached a run, the only one that names a delivered revision.
pub const DELIVERED_STATE: &str = "delivered";
/// Kinds of semantic work updates.
pub const SEMANTIC_UPDATE_KINDS: [&str; 10] = [
    "plan",
    "replan",
    "question",
    "approval",
    "failure",
    "result",
    "review",
    "delegation",
    "schedule_proposal",
    "note_proposal",
];
/// Deepest delegation: an agent's own assignment is depth 0, and its children and
/// grandchildren are depths 1 and 2.
pub const MAX_DELEGATION_DEPTH: i64 = 2;
/// Characters of a delegated assignment's objective.
pub const MAX_DELEGATION_OBJECTIVE_CHARS: usize = 4000;
/// Characters of a delegated assignment's expected result.
pub const MAX_DELEGATION_EXPECTED_RESULT_CHARS: usize = 2000;
/// Folder capabilities a delegation ceiling may name: a folder the child may not use at all is
/// left out of the ceiling instead.
pub const DELEGATION_CEILING_CAPABILITIES: [&str; 4] = ["publish", "execute", "edit", "read"];

fn text(field: &Field) -> Result<&str, AdapterError> {
    match field {
        Field::Text(text) => Ok(text),
        _ => Err(AdapterError("chat text field")),
    }
}

fn optional_text(field: &Field) -> Result<Option<&str>, AdapterError> {
    match field {
        Field::Null => Ok(None),
        field => text(field).map(Some),
    }
}

fn integer(field: &Field) -> Result<i64, AdapterError> {
    match field {
        Field::Integer(value) => Ok(*value),
        _ => Err(AdapterError("chat integer field")),
    }
}

fn one_of(field: &Field, allowed: &[&str], what: &'static str) -> Result<(), AdapterError> {
    if allowed.contains(&text(field)?) {
        Ok(())
    } else {
        Err(AdapterError(what))
    }
}

fn optional_one_of(
    field: &Field,
    allowed: &[&str],
    what: &'static str,
) -> Result<(), AdapterError> {
    match field {
        Field::Null => Ok(()),
        field => one_of(field, allowed, what),
    }
}

/// A Chat timestamp every reader of the row can parse. Reads parse these columns as
/// [`UtcTimestamp`], so admitting other text would make the stored row unreadable on every device.
fn timestamp(field: &Field) -> Result<(), AdapterError> {
    UtcTimestamp::new(text(field)?)
        .map(|_| ())
        .map_err(|_| AdapterError("chat timestamp is not RFC 3339 UTC"))
}

fn optional_timestamp(field: &Field) -> Result<(), AdapterError> {
    match field {
        Field::Null => Ok(()),
        field => timestamp(field),
    }
}

/// Text trimmed of spaces, as SQLite `trim` does, holding 1 to `max` characters.
fn trimmed(field: &Field, max: usize, what: &'static str) -> Result<(), AdapterError> {
    let trimmed = text(field)?.trim_matches(' ').chars().count();
    if (1..=max).contains(&trimmed) {
        Ok(())
    } else {
        Err(AdapterError(what))
    }
}

fn bounded(field: &Field, min: usize, max: usize, what: &'static str) -> Result<(), AdapterError> {
    if (min..=max).contains(&text(field)?.chars().count()) {
        Ok(())
    } else {
        Err(AdapterError(what))
    }
}

/// Checks a Chat participant id held outside a row key: nonempty, at most
/// [`MAX_PARTICIPANT_ID_BYTES`], and free of control characters, as Chat validates its ids.
fn participant_id(field: &Field, what: &'static str) -> Result<(), AdapterError> {
    let id = text(field)?;
    if !id.is_empty() && id.len() <= MAX_PARTICIPANT_ID_BYTES && !id.chars().any(char::is_control) {
        Ok(())
    } else {
        Err(AdapterError(what))
    }
}

fn schema_version(field: &Field) -> Result<(), AdapterError> {
    if integer(field)? >= 1 {
        Ok(())
    } else {
        Err(AdapterError("chat schema version"))
    }
}

/// A JSON object within [`MAX_JSON_CHARS`], as the table checks require.
fn json_object(field: &Field, what: &'static str) -> Result<(), AdapterError> {
    let json = text(field)?;
    if json.chars().count() > MAX_JSON_CHARS {
        return Err(AdapterError(what));
    }
    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(json)
        .map(|_| ())
        .map_err(|_| AdapterError(what))
}

fn flag(field: &Field) -> Result<(), AdapterError> {
    if matches!(integer(field)?, 0 | 1) {
        Ok(())
    } else {
        Err(AdapterError("chat flag"))
    }
}

/// Refuses every deletion of a participant, its agent row, or a policy revision but the removal
/// of an agent a person named. A person deletes the participant itself, and its agent row and
/// policy revisions leave only through that deletion, which every one of them names as its
/// cause; the agent row's cause is its own participant.
fn validate_agent_tombstone(
    table: TableId,
    key: &str,
    cause: Option<&Cause>,
) -> Result<(), AdapterError> {
    let named_agent_cause = cause.is_some_and(|cause| {
        cause.table == PARTICIPANTS_TABLE && is_named_agent_key(cause.row.as_str())
    });
    let allowed = if table == PARTICIPANTS_TABLE {
        cause.is_none() && is_named_agent_key(key)
    } else if table == AGENTS_TABLE {
        named_agent_cause && cause.is_some_and(|cause| cause.row.as_str() == key)
    } else if table == POLICY_REVISIONS_TABLE {
        named_agent_cause
    } else {
        true
    };
    if allowed {
        Ok(())
    } else {
        Err(AdapterError("chat agent deletion"))
    }
}

/// Refuses a local user under any key but the seeded one.
fn validate_participant_row(row: &RowKey, groups: &[GroupValue]) -> Result<(), AdapterError> {
    let local_user = groups
        .iter()
        .find(|value| value.group == participant_group::CREATED)
        .and_then(|value| value.value.fields().first())
        .is_some_and(|kind| matches!(kind, Field::Text(kind) if kind == LOCAL_USER_KIND));
    if local_user && row.as_str() != LOCAL_OWNER_PARTICIPANT_KEY {
        return Err(AdapterError("chat local user key"));
    }
    Ok(())
}

fn validate_participant(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (participant_group::CREATED, [kind, created_at]) => {
            one_of(kind, &PARTICIPANT_KINDS, "chat participant kind")?;
            timestamp(created_at)
        }
        (participant_group::PROFILE, [name, avatar_version, avatar]) => {
            trimmed(name, MAX_DISPLAY_NAME_CHARS, "chat participant name")?;
            schema_version(avatar_version)?;
            json_object(avatar, "chat participant avatar")
        }
        (participant_group::LIFECYCLE, [archived_at, hidden_at]) => {
            optional_timestamp(archived_at)?;
            optional_timestamp(hidden_at)?;
            if matches!(archived_at, Field::Null) && !matches!(hidden_at, Field::Null) {
                return Err(AdapterError("chat participant hidden while on"));
            }
            Ok(())
        }
        (participant_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat participant value")),
    }
}

fn validate_agent(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (agent_group::CREATED, [family, created_at]) => {
            trimmed(
                family,
                MAX_PROVIDER_INSTANCE_CHARS,
                "chat agent provider family",
            )?;
            timestamp(created_at)
        }
        (agent_group::POLICY, [_revision]) => Ok(()),
        (agent_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat agent value")),
    }
}

/// An agent access level, or null where the scope inherits.
fn agent_access(field: &Field, inherits: bool) -> Result<(), AdapterError> {
    match field {
        Field::Null if inherits => Ok(()),
        field => one_of(field, &AGENT_ACCESS_LEVELS, "chat agent access"),
    }
}

fn instructions(field: &Field, what: &'static str) -> Result<(), AdapterError> {
    bounded(field, 0, MAX_INSTRUCTIONS_CHARS, what)
}

fn validate_app_settings(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (app_settings_group::ACCESS, [access]) => agent_access(access, false),
        (app_settings_group::INSTRUCTIONS, [text]) => {
            instructions(text, "chat vault agent instructions")
        }
        (app_settings_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat vault agent settings value")),
    }
}

fn validate_project_settings(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (project_settings_group::ACCESS, [access]) => agent_access(access, true),
        (project_settings_group::INSTRUCTIONS, [text]) => {
            instructions(text, "chat project agent instructions")
        }
        (project_settings_group::CREATED | project_settings_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat project agent settings value")),
    }
}

fn validate_policy_revision(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (
            policy_revision_group::CREATED,
            [
                _agent,
                model_version,
                model,
                options_version,
                options,
                mode,
                created_at,
            ],
        ) => {
            schema_version(model_version)?;
            json_object(model, "chat policy model selection")?;
            schema_version(options_version)?;
            json_object(options, "chat policy provider options")?;
            one_of(mode, &INTERACTION_MODES, "chat policy interaction mode")?;
            timestamp(created_at)
        }
        _ => Err(AdapterError("chat policy revision value")),
    }
}

fn validate_conversation(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (conversation_group::CREATED, [_project, kind, created_at]) => {
            one_of(kind, &CONVERSATION_KINDS, "chat conversation kind")?;
            timestamp(created_at)
        }
        (conversation_group::ACTIVITY | conversation_group::UPDATED, [at]) => timestamp(at),
        (conversation_group::LIFECYCLE, [archived_at]) => optional_timestamp(archived_at),
        _ => Err(AdapterError("chat conversation value")),
    }
}

fn validate_membership(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (membership_group::IDENTITY, [_, _]) | (membership_group::EXISTS, []) => Ok(()),
        (membership_group::MEMBERSHIP, [role, removed_at]) => {
            one_of(role, &MEMBERSHIP_ROLES, "chat membership role")?;
            optional_timestamp(removed_at)
        }
        (membership_group::CREATED | membership_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat membership value")),
    }
}

fn validate_channel_membership(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (channel_membership_group::IDENTITY, [_, _]) | (channel_membership_group::EXISTS, []) => {
            Ok(())
        }
        (channel_membership_group::PRESENCE, [removed_at]) => optional_timestamp(removed_at),
        (channel_membership_group::CREATED | channel_membership_group::UPDATED, [at]) => {
            timestamp(at)
        }
        (channel_membership_group::MODEL, [version, data]) => channel_model(version, data),
        _ => Err(AdapterError("chat channel membership value")),
    }
}

fn validate_push_approval(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (
            push_approval_group::IDENTITY,
            [_conversation, _agent, _folder, push_url, destination_branch],
        ) => {
            git_word(
                push_url,
                MAX_PUSH_APPROVAL_URL_BYTES,
                "chat push approval URL",
            )?;
            git_word(
                destination_branch,
                MAX_PUSH_APPROVAL_BRANCH_BYTES,
                "chat push approval branch",
            )
        }
        (push_approval_group::APPROVAL, [_approver, approved_at, revoked_at]) => {
            timestamp(approved_at)?;
            optional_timestamp(revoked_at)
        }
        (push_approval_group::EXISTS, []) => Ok(()),
        _ => Err(AdapterError("chat push approval value")),
    }
}

/// Checks a remembered approval's shape only. Devices derive subject keys with stricter rules,
/// so a well formed key no device would derive never matches a request.
fn validate_remembered_approval(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (remembered_approval_group::IDENTITY, [_conversation, _agent, kind, key]) => {
            one_of(kind, &SUBJECT_KINDS, "chat remembered approval kind")?;
            if subject_key_is_well_formed(text(key)?) {
                Ok(())
            } else {
                Err(AdapterError("chat remembered approval subject"))
            }
        }
        (
            remembered_approval_group::APPROVAL,
            [_approver, approved_at, revoked_at, granted_access],
        ) => {
            timestamp(approved_at)?;
            optional_timestamp(revoked_at)?;
            one_of(
                granted_access,
                &AGENT_ACCESS_LEVELS,
                "chat remembered approval access",
            )
        }
        (remembered_approval_group::EXISTS, []) => Ok(()),
        _ => Err(AdapterError("chat remembered approval value")),
    }
}

/// Text of 1 to `max` bytes without whitespace or control characters, as a push URL or Git
/// branch name is.
fn git_word(field: &Field, max: usize, what: &'static str) -> Result<(), AdapterError> {
    let value = text(field)?;
    if (1..=max).contains(&value.len())
        && !value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        Ok(())
    } else {
        Err(AdapterError(what))
    }
}

fn validate_channel(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (channel_group::CREATED, [_project, _conversation, is_default, created_at]) => {
            flag(is_default)?;
            timestamp(created_at)
        }
        (channel_group::DETAILS, [name, topic]) => {
            bounded(name, 1, MAX_CHANNEL_NAME_CHARS, "chat channel name")?;
            bounded(topic, 0, MAX_CHANNEL_TOPIC_CHARS, "chat channel topic")
        }
        (channel_group::LIFECYCLE, [archived_at]) => optional_timestamp(archived_at),
        (channel_group::UPDATED, [at]) => timestamp(at),
        (channel_group::AGENT_ACCESS, [access]) => agent_access(access, true),
        (channel_group::AGENT_INSTRUCTIONS, [text]) => {
            instructions(text, "chat channel agent instructions")
        }
        _ => Err(AdapterError("chat channel value")),
    }
}

fn validate_channel_note(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (channel_note_group::CREATED, [_channel, created_at]) => timestamp(created_at),
        (channel_note_group::PROVENANCE, [_source_item, _proposed_by]) => Ok(()),
        (channel_note_group::BODY, [body, _edited_by]) => {
            trimmed(body, MAX_CHANNEL_NOTE_CHARS, "chat channel note body")
        }
        (channel_note_group::LIFECYCLE, [deleted_at]) => optional_timestamp(deleted_at),
        (channel_note_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat channel note value")),
    }
}

/// The bucket of a daily agent usage row names a calendar date; the counts are bounded by the
/// manifest.
fn validate_agent_usage(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (agent_usage_group::BUCKET, [_device, _agent, Field::Text(date)]) => {
            if is_usage_date(date) {
                Ok(())
            } else {
                Err(AdapterError("chat agent usage date"))
            }
        }
        (
            agent_usage_group::TOKENS | agent_usage_group::WORKING | agent_usage_group::RUNS,
            [_count],
        ) => Ok(()),
        _ => Err(AdapterError("chat agent usage value")),
    }
}

/// A daily agent usage row that carries its bucket is keyed by that bucket, so one device
/// writes one row per agent and day.
fn validate_agent_usage_row(row: &RowKey, groups: &[GroupValue]) -> Result<(), AdapterError> {
    let Some(bucket) = groups
        .iter()
        .find(|value| value.group == agent_usage_group::BUCKET)
    else {
        return Ok(());
    };
    let [Field::Text(device), Field::Text(agent), Field::Text(date)] = bucket.value.fields() else {
        return Err(AdapterError("chat agent usage bucket"));
    };
    match agent_usage_key(device, agent, date) {
        Ok(expected) if expected == *row => Ok(()),
        _ => Err(AdapterError("chat agent usage key")),
    }
}

/// A provider status row holds its canonical catalog and limits payloads, with limits and their
/// observation time set together.
fn validate_provider_status(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (provider_status_group::IDENTITY, [_device, _family]) => Ok(()),
        (provider_status_group::STATUS, [available, changed_at]) => {
            flag(available)?;
            timestamp(changed_at)
        }
        (provider_status_group::CATALOG, [data, changed_at]) => {
            timestamp(changed_at)?;
            if is_canonical_catalog(text(data)?) {
                Ok(())
            } else {
                Err(AdapterError("chat provider status catalog"))
            }
        }
        (provider_status_group::LIMITS, [Field::Null, Field::Null]) => Ok(()),
        (provider_status_group::LIMITS, [data @ Field::Text(_), observed_at]) => {
            timestamp(observed_at)?;
            if is_canonical_limits(text(data)?) {
                Ok(())
            } else {
                Err(AdapterError("chat provider status limits"))
            }
        }
        _ => Err(AdapterError("chat provider status value")),
    }
}

/// A provider status row that carries its identity is keyed by it, so one device writes one
/// row per provider family.
fn validate_provider_status_row(row: &RowKey, groups: &[GroupValue]) -> Result<(), AdapterError> {
    let Some(identity) = groups
        .iter()
        .find(|value| value.group == provider_status_group::IDENTITY)
    else {
        return Ok(());
    };
    let [Field::Text(device), Field::Text(family)] = identity.value.fields() else {
        return Err(AdapterError("chat provider status identity"));
    };
    match provider_status_key(device, family) {
        Ok(expected) if expected == *row => Ok(()),
        _ => Err(AdapterError("chat provider status key")),
    }
}
/// Text of at most `max` UTF-8 bytes.
fn within_bytes(field: &Field, max: usize, what: &'static str) -> Result<(), AdapterError> {
    if text(field)?.len() <= max {
        Ok(())
    } else {
        Err(AdapterError(what))
    }
}

/// An integer of at least `min`.
fn at_least(field: &Field, min: i64, what: &'static str) -> Result<i64, AdapterError> {
    let value = integer(field)?;
    if value >= min {
        Ok(value)
    } else {
        Err(AdapterError(what))
    }
}

/// A timeline point: an order key and an item id, both `''` before the first item.
fn read_point(order_key: &Field, item_id: &Field) -> Result<(), AdapterError> {
    match (text(order_key)?, text(item_id)?) {
        ("", "") => Ok(()),
        ("", _) | (_, "") => Err(AdapterError("chat read cursor point")),
        _ => {
            bounded(
                order_key,
                1,
                MAX_ORDER_KEY_BYTES,
                "chat read cursor order key",
            )?;
            bounded(item_id, 1, MAX_ID_CHARS, "chat read cursor item id")
        }
    }
}

/// A workspace path relative to its folder, as the table checks it: no leading slash, no
/// parent segment, and no backslash.
fn relative_path(field: &Field) -> Result<(), AdapterError> {
    let path = text(field)?;
    bounded(field, 1, MAX_RELATIVE_PATH_CHARS, "chat workspace path")?;
    let escapes = path.starts_with('/')
        || path.starts_with("../")
        || path.contains("/../")
        || path.ends_with("/..")
        || path.contains('\\');
    if escapes {
        Err(AdapterError("chat workspace path"))
    } else {
        Ok(())
    }
}

fn validate_reply_thread(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (reply_thread_group::IDENTITY, [_]) | (reply_thread_group::EXISTS, []) => Ok(()),
        (reply_thread_group::CREATED, [_conversation, created_at]) => timestamp(created_at),
        (reply_thread_group::ACTIVITY | reply_thread_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat reply thread value")),
    }
}

fn validate_item(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (item_group::CREATED, [_conversation, _thread, kind]) => {
            one_of(kind, &ITEM_KINDS, "chat item kind")
        }
        (item_group::PLACEMENT, [_order_key, created_at]) => timestamp(created_at),
        _ => Err(AdapterError("chat item value")),
    }
}

fn validate_message(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (message_group::CREATED, [label, created_at]) => {
            trimmed(label, MAX_DISPLAY_NAME_CHARS, "chat message author label")?;
            timestamp(created_at)
        }
        (message_group::AUTHOR, [_author]) => Ok(()),
        (message_group::CONTENT, [_revision, edited_at]) => optional_timestamp(edited_at),
        (message_group::LIFECYCLE, [deleted_at]) => optional_timestamp(deleted_at),
        _ => Err(AdapterError("chat message value")),
    }
}

fn validate_revision(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (timeline_group::CREATED, [_message, markdown, version, rich_content, created_at]) => {
            within_bytes(
                markdown,
                MAX_MESSAGE_MARKDOWN_BYTES,
                "chat message Markdown is too large",
            )?;
            schema_version(version)?;
            within_bytes(
                rich_content,
                MAX_RICH_CONTENT_BYTES,
                "chat message rich content is too large",
            )?;
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(text(rich_content)?)
                .map_err(|_| AdapterError("chat message rich content"))?;
            timestamp(created_at)
        }
        (timeline_group::ATTACHMENTS, [blob]) => attachments(blob).map(|_| ()),
        _ => Err(AdapterError("chat message revision value")),
    }
}

/// One attachment a message revision carries by value.
struct Attachment {
    sha256: String,
    mime_type: String,
    byte_size: u32,
}

/// The attachments of a revision's attachments group value, each checked as the table checks
/// it: a lowercase hex digest, a known content type matching the kind, and a size within the
/// asset class bound. Only a text snippet may be empty.
/// Revisions and scheduled messages encode their attachment references alike.
fn attachments(blob: &Field) -> Result<Vec<Attachment>, AdapterError> {
    let Field::Blob(bytes) = blob else {
        return Err(AdapterError("chat attachments value"));
    };
    let rows = codec::decode(&ATTACHMENT_REFERENCES, bytes)
        .map_err(|_| AdapterError("chat attachments encoding"))?;
    rows.into_iter()
        .map(|row| attachment(&row.fields))
        .collect()
}

fn attachment(fields: &[Field]) -> Result<Attachment, AdapterError> {
    let [id, folder, kind, name, mime, size, sha256] = fields else {
        return Err(AdapterError("chat attachment value"));
    };
    bounded(id, 1, MAX_ATTACHMENT_ID_CHARS, "chat attachment id")?;
    bounded(folder, 1, MAX_ATTACHMENT_ID_CHARS, "chat attachment folder")?;
    trimmed(name, MAX_ATTACHMENT_NAME_CHARS, "chat attachment name")?;
    let mime = text(mime)?;
    let expected_kind = ATTACHMENT_TYPES
        .iter()
        .find(|(known, _)| *known == mime)
        .map(|(_, kind)| *kind)
        .ok_or(AdapterError("chat attachment content type"))?;
    if text(kind)? != expected_kind {
        return Err(AdapterError("chat attachment kind"));
    }
    let sha256 = text(sha256)?;
    if sha256.len() != SHA256_HEX_CHARS
        || !sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(AdapterError("chat attachment digest"));
    }
    let byte_size = u32::try_from(integer(size)?)
        .ok()
        .filter(|size| *size <= MAX_ATTACHMENT_BYTES)
        .ok_or(AdapterError("chat attachment size"))?;
    if byte_size == 0 && expected_kind != "text_snippet" {
        return Err(AdapterError("chat attachment size"));
    }
    Ok(Attachment {
        sha256: sha256.to_owned(),
        mime_type: mime.to_owned(),
        byte_size,
    })
}

/// Attachment reference lists at the codec's bounds: none, one empty text snippet, and the
/// most attachments with the longest values and the largest size.
fn attachment_samples() -> Vec<Vec<Vec<Field>>> {
    let reference = |ordinal: usize, kind: &str, mime: &str, size: u32, name: String| {
        let id = format!("{ordinal:0>width$}", width = MAX_ATTACHMENT_ID_CHARS);
        vec![
            Field::Text(id.clone()),
            Field::Text(id),
            Field::Text(kind.to_owned()),
            Field::Text(name),
            Field::Text(mime.to_owned()),
            Field::Integer(i64::from(size)),
            Field::Text(format!("{ordinal:f>width$}", width = SHA256_HEX_CHARS)),
        ]
    };
    let full = (0..MAX_MESSAGE_ATTACHMENTS)
        .map(|ordinal| {
            let (mime, kind) = ATTACHMENT_TYPES[ordinal % ATTACHMENT_TYPES.len()];
            let name = "é".repeat(MAX_ATTACHMENT_NAME_CHARS);
            reference(ordinal, kind, mime, MAX_ATTACHMENT_BYTES, name)
        })
        .collect();
    vec![
        Vec::new(),
        vec![reference(
            0,
            "text_snippet",
            "text/plain",
            0,
            "a".to_owned(),
        )],
        full,
    ]
}

fn validate_reference(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (
            timeline_group::CREATED,
            [
                _revision,
                kind,
                label,
                projection,
                start,
                end,
                ordinal,
                created_at,
            ],
        ) => {
            one_of(kind, &REFERENCE_KINDS, "chat reference kind")?;
            bounded(label, 1, MAX_LABEL_CHARS, "chat reference label")?;
            bounded(projection, 1, MAX_LABEL_CHARS, "chat reference projection")?;
            let start = at_least(start, 0, "chat reference span")?;
            at_least(end, start.saturating_add(1), "chat reference span")?;
            at_least(ordinal, 0, "chat reference ordinal")?;
            timestamp(created_at)
        }
        _ => Err(AdapterError("chat message reference value")),
    }
}

/// A channel reference range: every bound null for an empty range, or well-formed order keys with
/// a lower point whose item may be `''` at or before a high point, and a revision cutoff.
fn validate_channel_target(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (
            timeline_group::CREATED,
            [
                _channel,
                _source,
                _destination,
                lower_key,
                lower_item,
                high_key,
                high_item,
                cutoff,
            ],
        ) => {
            let bounds = [lower_key, lower_item, high_key, high_item, cutoff];
            if bounds.iter().all(|field| matches!(field, Field::Null)) {
                return Ok(());
            }
            if bounds.iter().any(|field| matches!(field, Field::Null)) {
                return Err(AdapterError("chat channel reference range"));
            }
            bounded(lower_key, 1, MAX_ORDER_KEY_BYTES, "chat channel range key")?;
            bounded(lower_item, 0, MAX_ID_CHARS, "chat channel range item")?;
            bounded(high_key, 1, MAX_ORDER_KEY_BYTES, "chat channel range key")?;
            let lower = (text(lower_key)?, text(lower_item)?);
            let high = (text(high_key)?, text(high_item)?);
            for key in [lower.0, high.0] {
                OrderKey::parse(key).map_err(|_| AdapterError("chat channel range key"))?;
            }
            if lower <= high {
                Ok(())
            } else {
                Err(AdapterError("chat channel reference range order"))
            }
        }
        _ => Err(AdapterError("chat channel reference target value")),
    }
}

fn validate_target(table: TableId, group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (timeline_group::CREATED, [environment]) if table == ENVIRONMENT_TARGETS_TABLE => bounded(
            environment,
            1,
            MAX_ENVIRONMENT_ID_CHARS,
            "chat environment reference",
        ),
        (timeline_group::CREATED, [_participant]) if table == PARTICIPANT_TARGETS_TABLE => Ok(()),
        (timeline_group::CREATED, [_folder]) if table == FOLDER_TARGETS_TABLE => Ok(()),
        (timeline_group::CREATED, [_folder, kind, path]) if table == PATH_TARGETS_TABLE => {
            one_of(kind, &PATH_KINDS, "chat workspace path kind")?;
            relative_path(path)
        }
        _ => Err(AdapterError("chat reference target value")),
    }
}

fn validate_read_cursor(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (read_cursor_group::IDENTITY, [_, _]) | (read_cursor_group::EXISTS, []) => Ok(()),
        (read_cursor_group::READ, [order_key, item_id]) => read_point(order_key, item_id),
        (read_cursor_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat read cursor value")),
    }
}

/// A device id: null, or 1 to [`MAX_DEVICE_ID_BYTES`] bytes.
fn executing_device(field: &Field) -> Result<(), AdapterError> {
    match optional_text(field)? {
        None => Ok(()),
        Some(device) if (1..=MAX_DEVICE_ID_BYTES).contains(&device.len()) => Ok(()),
        Some(_) => Err(AdapterError("chat executing device")),
    }
}

/// Optional text of at most `max` characters.
fn optional_bounded(field: &Field, max: usize, what: &'static str) -> Result<(), AdapterError> {
    match field {
        Field::Null => Ok(()),
        field => bounded(field, 0, max, what),
    }
}

/// The Markdown a message revision or a scheduled message holds among `values`, `None` for other
/// rows or a value without it.
fn presented_markdown(table: TableId, values: &[(GroupId, Value)]) -> Option<String> {
    let group_value = |group: GroupId| {
        values
            .iter()
            .find(|(candidate, _)| *candidate == group)
            .map(|(_, value)| value.fields())
    };
    match table {
        REVISIONS_TABLE => match group_value(timeline_group::CREATED)?.get(1)? {
            Field::Text(markdown) => Some(markdown.clone()),
            _ => None,
        },
        CHANNEL_NOTES_TABLE => match group_value(channel_note_group::BODY)?.first()? {
            Field::Text(body) => Some(body.clone()),
            _ => None,
        },
        SCHEDULED_TABLE => {
            let Field::Text(request) = group_value(scheduled_group::CONTENT)?.get(1)? else {
                return None;
            };
            let request =
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(request).ok()?;
            request
                .get("normalizedMarkdown")?
                .as_str()
                .map(str::to_string)
        }
        _ => None,
    }
}

fn validate_scheduled(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (scheduled_group::CREATED, [command, _channel, _thread, device, created_at]) => {
            bounded(command, 1, MAX_ID_CHARS, "chat scheduled command id")?;
            executing_device(device)?;
            timestamp(created_at)
        }
        (
            scheduled_group::CONTENT,
            [
                version,
                request,
                scheduled_for,
                attachment_blob,
                reference_blob,
            ],
        ) => {
            if integer(version)? != SCHEDULED_REQUEST_VERSION {
                return Err(AdapterError("chat scheduled request version"));
            }
            within_bytes(
                request,
                MAX_SCHEDULED_REQUEST_BYTES,
                "chat scheduled request is too large",
            )?;
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(text(request)?)
                .map_err(|_| AdapterError("chat scheduled request"))?;
            timestamp(scheduled_for)?;
            attachments(attachment_blob)?;
            let Field::Blob(references) = reference_blob else {
                return Err(AdapterError("chat scheduled references value"));
            };
            if text(request)?.len() + references.len() > MAX_SCHEDULED_CONTENT_BYTES {
                return Err(AdapterError("chat scheduled message is too large"));
            }
            scheduled_references(references)
        }
        (scheduled_group::FAILURE, [failed_at, error]) => {
            optional_timestamp(failed_at)?;
            optional_bounded(error, MAX_REASON_CHARS, "chat scheduled failure")
        }
        (scheduled_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat scheduled message value")),
    }
}

/// A recurrence rule: an interval rule names only its interval, and a fixed-time rule only its
/// local minute and a non-empty weekday mask, each within the shared bounds. The zone is checked
/// for length only: whether it resolves depends on the device's zone database, so only the
/// executing device decides that, at dispatch.
fn recurrence_rule(
    kind: &Field,
    interval: &Field,
    local_minute: &Field,
    weekdays: &Field,
    time_zone: &Field,
    quiet_policy: &Field,
) -> Result<(), AdapterError> {
    one_of(kind, &chat_recurrence::RULE_KINDS, "chat recurrence kind")?;
    let set = |field: &Field| !matches!(field, Field::Null);
    let interval_rule = text(kind)? == chat_recurrence::INTERVAL_RULE;
    if set(interval) != interval_rule
        || set(local_minute) == interval_rule
        || set(weekdays) == interval_rule
    {
        return Err(AdapterError("chat recurrence shape"));
    }
    let within = |field: &Field, min: i64, max: i64| match field {
        Field::Null => Ok(()),
        field if (min..=max).contains(&integer(field)?) => Ok(()),
        _ => Err(AdapterError("chat recurrence bound")),
    };
    within(
        interval,
        i64::from(chat_recurrence::MIN_INTERVAL_MINUTES),
        i64::from(chat_recurrence::MAX_INTERVAL_MINUTES),
    )?;
    within(
        local_minute,
        0,
        i64::from(chat_recurrence::LAST_LOCAL_MINUTE),
    )?;
    within(weekdays, 1, i64::from(chat_recurrence::ALL_WEEKDAYS_MASK))?;
    trimmed(
        time_zone,
        chat_recurrence::MAX_TIME_ZONE_CHARS,
        "chat recurrence time zone",
    )?;
    one_of(
        quiet_policy,
        &chat_recurrence::QUIET_POLICIES,
        "chat recurrence quiet policy",
    )
}

fn validate_recurrence(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (recurrence_group::IDENTITY, [_]) | (recurrence_group::EXISTS, []) => Ok(()),
        (recurrence_group::CREATED | recurrence_group::UPDATED, [at]) => timestamp(at),
        (
            recurrence_group::RULE,
            [
                kind,
                interval,
                local_minute,
                weekdays,
                time_zone,
                quiet_policy,
            ],
        ) => recurrence_rule(
            kind,
            interval,
            local_minute,
            weekdays,
            time_zone,
            quiet_policy,
        ),
        (recurrence_group::PROGRESS, [next_at, fired]) => {
            timestamp(next_at)?;
            at_least(fired, 0, "chat recurrence fired count").map(|_| ())
        }
        _ => Err(AdapterError("chat scheduled recurrence value")),
    }
}

/// The mention references of a scheduled message: well-formed spans, distinct per message, each
/// naming exactly the target its kind needs.
fn scheduled_references(bytes: &[u8]) -> Result<(), AdapterError> {
    let rows = codec::decode(&SCHEDULED_REFERENCES, bytes)
        .map_err(|_| AdapterError("chat scheduled references encoding"))?;
    let mut spans = std::collections::BTreeSet::new();
    for row in rows {
        let [
            kind,
            label,
            projection,
            start,
            end,
            participant,
            participant_kind,
            channel,
            folder,
            path_kind,
            path,
            environment,
        ] = row.fields.as_slice()
        else {
            return Err(AdapterError("chat scheduled reference value"));
        };
        one_of(kind, &REFERENCE_KINDS, "chat reference kind")?;
        bounded(label, 1, MAX_LABEL_CHARS, "chat reference label")?;
        bounded(projection, 1, MAX_LABEL_CHARS, "chat reference projection")?;
        let start = at_least(start, 0, "chat reference span")?;
        let end = at_least(end, start.saturating_add(1), "chat reference span")?;
        if !spans.insert((start, end)) {
            return Err(AdapterError("chat reference span repeats"));
        }
        let set = |field: &Field| !matches!(field, Field::Null);
        let shape = [
            set(participant),
            set(participant_kind),
            set(channel),
            set(folder),
            set(path_kind),
            set(path),
            set(environment),
        ];
        let expected = match text(kind)? {
            "participant" => [true, true, false, false, false, false, false],
            "channel" => [false, false, true, false, false, false, false],
            "working_folder" => [false, false, false, true, false, false, false],
            "workspace_path" => [false, false, false, true, true, true, false],
            _ => [false, false, false, false, false, false, true],
        };
        if shape != expected {
            return Err(AdapterError("chat scheduled reference target"));
        }
        optional_one_of(
            participant_kind,
            &PARTICIPANT_KINDS,
            "chat reference participant kind",
        )?;
        optional_one_of(path_kind, &PATH_KINDS, "chat workspace path kind")?;
        if set(path) {
            relative_path(path)?;
        }
        if set(environment) {
            bounded(
                environment,
                1,
                MAX_ENVIRONMENT_ID_CHARS,
                "chat environment reference",
            )?;
        }
    }
    Ok(())
}

fn validate_assignment(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (
            assignment_group::CREATED,
            [
                _thread,
                _agent,
                _trigger,
                device,
                created_at,
                model_version,
                model,
                instructions,
            ],
        ) => {
            executing_device(device)?;
            timestamp(created_at)?;
            schema_version(model_version)?;
            json_object(model, "chat assignment model selection")?;
            bounded(
                instructions,
                0,
                MAX_RESOLVED_INSTRUCTIONS_CHARS,
                "chat assignment instructions",
            )
        }
        (assignment_group::MODEL_LABEL, [Field::Null]) => Ok(()),
        (assignment_group::MODEL_LABEL, [label]) => bounded(
            label,
            1,
            MAX_MODEL_LABEL_CHARS,
            "chat assignment model label",
        ),
        (assignment_group::PREVIOUS, [_previous]) => Ok(()),
        (assignment_group::STATE, [state, reason, settled_at, outcome, reviewed_at, reviewer]) => {
            one_of(state, &ASSIGNMENT_STATES, "chat assignment state")?;
            optional_bounded(reason, MAX_REASON_CHARS, "chat assignment state reason")?;
            optional_timestamp(settled_at)?;
            assignment_review(state, outcome, reviewed_at, reviewer)
        }
        (assignment_group::UPDATED, [at]) => timestamp(at),
        (
            assignment_group::DELEGATION,
            [parent, depth, objective, expected_result, folder_ceiling],
        ) => assignment_delegation(parent, depth, objective, expected_result, folder_ceiling),
        _ => Err(AdapterError("chat assignment value")),
    }
}

/// An agent's own assignment has depth 0 and no delegation fields. A delegated child has a
/// depth of 1 to [`MAX_DELEGATION_DEPTH`], its parent, a non-blank objective and expected
/// result, and the folder ceiling resolved when it was delegated, so no later widening of the
/// parent can widen the child. Depth against the parent's depth is checked by the table, which
/// sees the parent row.
fn assignment_delegation(
    parent: &Field,
    depth: &Field,
    objective: &Field,
    expected_result: &Field,
    folder_ceiling: &Field,
) -> Result<(), AdapterError> {
    let depth = integer(depth)?;
    let fields = [parent, objective, expected_result, folder_ceiling];
    let set = fields
        .iter()
        .filter(|field| !matches!(field, Field::Null))
        .count();
    match set {
        0 if depth == 0 => Ok(()),
        _ if set < fields.len() => Err(AdapterError("chat assignment delegation is incomplete")),
        _ if !(1..=MAX_DELEGATION_DEPTH).contains(&depth) => {
            Err(AdapterError("chat assignment delegation depth"))
        }
        _ => {
            trimmed(
                objective,
                MAX_DELEGATION_OBJECTIVE_CHARS,
                "chat assignment delegation objective",
            )?;
            trimmed(
                expected_result,
                MAX_DELEGATION_EXPECTED_RESULT_CHARS,
                "chat assignment delegation expected result",
            )?;
            delegation_folder_ceiling(folder_ceiling)
        }
    }
}

/// A channel's model for an agent is its schema version and selection together, or neither
/// when the agent runs with its default model.
fn channel_model(version: &Field, data: &Field) -> Result<(), AdapterError> {
    match (version, data) {
        (Field::Null, Field::Null) => Ok(()),
        (Field::Null, _) | (_, Field::Null) => {
            Err(AdapterError("chat channel model is incomplete"))
        }
        _ => {
            schema_version(version)?;
            json_object(data, "chat channel model")
        }
    }
}

/// One folder of a delegation ceiling.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CeilingFolder {
    working_folder_id: String,
    capability: String,
}

/// A JSON array within [`MAX_JSON_CHARS`] of folders, each named once with a capability from
/// [`DELEGATION_CEILING_CAPABILITIES`]. An empty array delegates work without folder access.
fn delegation_folder_ceiling(field: &Field) -> Result<(), AdapterError> {
    const WHAT: &str = "chat assignment delegation folder ceiling";
    let json = text(field)?;
    if json.chars().count() > MAX_JSON_CHARS {
        return Err(AdapterError(WHAT));
    }
    let folders =
        serde_json::from_str::<Vec<CeilingFolder>>(json).map_err(|_| AdapterError(WHAT))?;
    let mut seen = std::collections::BTreeSet::new();
    for folder in &folders {
        let id = folder.working_folder_id.as_str();
        let valid_id = !id.trim().is_empty()
            && id.chars().count() <= MAX_ID_CHARS
            && !id.chars().any(char::is_control);
        if !valid_id
            || !DELEGATION_CEILING_CAPABILITIES.contains(&folder.capability.as_str())
            || !seen.insert(id)
        {
            return Err(AdapterError(WHAT));
        }
    }
    Ok(())
}

/// A review is all three fields or none, and only on completed work: the review judges a
/// completion, so a review beside any other state would describe work that did not finish.
fn assignment_review(
    state: &Field,
    outcome: &Field,
    reviewed_at: &Field,
    reviewer: &Field,
) -> Result<(), AdapterError> {
    match (outcome, reviewed_at, reviewer) {
        (Field::Null, Field::Null, Field::Null) => Ok(()),
        (Field::Null, _, _) | (_, Field::Null, _) | (_, _, Field::Null) => {
            Err(AdapterError("chat assignment review is incomplete"))
        }
        _ if text(state)? != REVIEWED_ASSIGNMENT_STATE => {
            Err(AdapterError("chat assignment review needs completed work"))
        }
        _ => {
            one_of(
                outcome,
                &ASSIGNMENT_REVIEW_OUTCOMES,
                "chat assignment review outcome",
            )?;
            timestamp(reviewed_at)?;
            participant_id(reviewer, "chat assignment reviewer")
        }
    }
}

fn validate_input(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (input_group::IDENTITY, [_]) | (input_group::EXISTS, []) => Ok(()),
        (input_group::CREATED, [created_at]) => timestamp(created_at),
        (input_group::ROUTING, [_assignment, kind]) => {
            one_of(kind, &ROUTING_KINDS, "chat assignment input routing")
        }
        (input_group::HOLD, [held]) => flag(held),
        (input_group::ORDER, [ordinal]) => {
            at_least(ordinal, 1, "chat assignment input ordinal").map(|_| ())
        }
        (input_group::DELIVERY, [state, delivered_at, delivered_revision]) => {
            one_of(state, &DELIVERY_STATES, "chat assignment input delivery")?;
            optional_timestamp(delivered_at)?;
            if !matches!(delivered_revision, Field::Null) && text(state)? != DELIVERED_STATE {
                return Err(AdapterError(
                    "chat assignment input revision needs a delivered input",
                ));
            }
            Ok(())
        }
        _ => Err(AdapterError("chat assignment input value")),
    }
}

fn validate_semantic_update(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (semantic_update_group::CREATED, [_assignment, kind, version, payload, created_at]) => {
            one_of(kind, &SEMANTIC_UPDATE_KINDS, "chat semantic update kind")?;
            schema_version(version)?;
            within_bytes(
                payload,
                MAX_SEMANTIC_PAYLOAD_BYTES,
                "chat semantic update payload is too large",
            )?;
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(text(payload)?)
                .map_err(|_| AdapterError("chat semantic update payload"))?;
            timestamp(created_at)
        }
        _ => Err(AdapterError("chat semantic update value")),
    }
}

/// Assignment states by merge priority, the most restrictive ending first.
const ASSIGNMENT_STATE_PRIORITIES: [(&str, u8); 4] = [
    ("cancelled", 5),
    ("failed", 4),
    ("completed", 3),
    ("ready_for_review", 2),
];
/// Priority of the active states other than a review request.
const ACTIVE_ASSIGNMENT_PRIORITY: u8 = 1;

/// The most restrictive ending wins: a cancel outranks a failure, which outranks a completion, so
/// a concurrent cancel beats an accept and carries no review, and a failure is never hidden by a
/// completion recorded elsewhere. Every ending outranks progress a device reported before seeing
/// it, and a review request outranks the other active states.
fn assignment_priority(fields: &[Field]) -> u8 {
    let Some(Field::Text(state)) = fields.first() else {
        return ACTIVE_ASSIGNMENT_PRIORITY;
    };
    ASSIGNMENT_STATE_PRIORITIES
        .iter()
        .find(|(candidate, _)| candidate == state)
        .map_or(ACTIVE_ASSIGNMENT_PRIORITY, |(_, priority)| *priority)
}

/// Input delivery states by merge priority, the furthest along first.
const INPUT_DELIVERY_PRIORITIES: [(&str, u8); 4] = [
    ("delivered", 4),
    ("failed", 3),
    ("ignored", 2),
    ("pending", 1),
];

/// What happened to an input outranks a wish about it: a delivery beats a concurrent cancel,
/// since the run already read the message, and every ending beats pending. No writer returns an
/// input to pending, so the ladder only moves forward.
fn input_delivery_priority(fields: &[Field]) -> u8 {
    let Some(Field::Text(state)) = fields.first() else {
        return 0;
    };
    INPUT_DELIVERY_PRIORITIES
        .iter()
        .find(|(candidate, _)| candidate == state)
        .map_or(0, |(_, priority)| *priority)
}

/// Priority of an ended row: a removal or revocation outranks every live value.
const ENDED: u8 = u8::MAX;

/// Whether the field at `index` is set, which marks a removal or revocation.
fn ended(fields: &[Field], index: usize) -> bool {
    !matches!(fields.get(index), None | Some(Field::Null))
}

/// A membership removal wins, then demotion to member over promotion to owner.
fn membership_priority(fields: &[Field]) -> u8 {
    if ended(fields, 1) {
        ENDED
    } else {
        restrictiveness(fields.first(), &MEMBERSHIP_ROLES)
    }
}

/// A removal from a channel wins over a concurrent re-add.
fn channel_membership_priority(fields: &[Field]) -> u8 {
    if ended(fields, 0) { ENDED } else { 1 }
}

/// The narrower access level wins among concurrent edits of one scope. An inherited level ranks
/// lowest: the scope it inherits from can be as wide as Own the folder.
fn agent_access_priority(fields: &[Field]) -> u8 {
    restrictiveness(fields.first(), &AGENT_ACCESS_LEVELS)
}

/// A revocation wins over a concurrent approval, for push and remembered approvals alike.
fn push_approval_priority(fields: &[Field]) -> u8 {
    if ended(fields, 2) { ENDED } else { 1 }
}

/// An archive wins over a concurrent unarchive.
fn lifecycle_priority(fields: &[Field]) -> u8 {
    if ended(fields, 0) { 2 } else { 1 }
}

/// One plus the position of `field`'s text in `ranked`, least restrictive first, or 0 for null
/// or anything else, so a cleared pointer never outranks a set one.
fn restrictiveness(field: Option<&Field>, ranked: &[&str]) -> u8 {
    let Some(Field::Text(text)) = field else {
        return 0;
    };
    ranked
        .iter()
        .position(|candidate| candidate == text)
        .and_then(|position| u8::try_from(position + 1).ok())
        .unwrap_or(0)
}

/// The attachment references `owned` of `table`: a revision's or a scheduled message's.
fn attachment_table(table: TableId, owned: &str) -> Result<&'static OwnedTable, AdapterError> {
    if table == REVISIONS_TABLE && owned == ATTACHMENT_REFERENCES.table {
        Ok(&ATTACHMENT_REFERENCES)
    } else if table == SCHEDULED_TABLE && owned == SCHEDULED_ATTACHMENT_REFERENCES.table {
        Ok(&SCHEDULED_ATTACHMENT_REFERENCES)
    } else {
        Err(AdapterError("owned table"))
    }
}

/// Adapter of the Chat domain.
#[derive(Debug, Default, Clone, Copy)]
pub struct ChatAdapter;

impl DomainAdapter for ChatAdapter {
    fn name(&self) -> &'static str {
        ADAPTER
    }

    /// Attachment references keep the engine's row encoding and refuse rows the table's checks
    /// would refuse, so every accepted list stores unchanged.
    fn encode_owned(
        &self,
        table: TableId,
        owned: &str,
        rows: Vec<Vec<Field>>,
    ) -> Result<Vec<u8>, AdapterError> {
        let references = attachment_table(table, owned)?;
        for row in &rows {
            attachment(row)?;
        }
        let rows: Vec<_> = rows.into_iter().map(codec::OwnedRow::flat).collect();
        codec::encode(references, &rows).map_err(|_| AdapterError("chat attachments encoding"))
    }

    fn decode_owned(
        &self,
        table: TableId,
        owned: &str,
        bytes: &[u8],
    ) -> Result<Vec<Vec<Field>>, AdapterError> {
        codec::decode(attachment_table(table, owned)?, bytes)
            .map_err(|_| AdapterError("chat attachments encoding"))?
            .into_iter()
            .map(|row| attachment(&row.fields).map(|_| row.fields))
            .collect()
    }

    fn owned_samples(&self, table: TableId, owned: &str) -> Vec<Vec<Vec<Field>>> {
        if attachment_table(table, owned).is_ok() {
            attachment_samples()
        } else {
            Vec::new()
        }
    }

    fn derived(
        &self,
        _table: TableId,
        _group: GroupId,
        _value: &Value,
    ) -> Result<Vec<(&'static str, Field)>, AdapterError> {
        Ok(Vec::new())
    }

    /// The narrower access wins among concurrent edits.
    fn priority(&self, table: TableId, group: GroupId, value: &Value) -> u8 {
        if (table == APP_SETTINGS_TABLE && group == app_settings_group::ACCESS)
            || (table == PROJECT_SETTINGS_TABLE && group == project_settings_group::ACCESS)
            || (table == CHANNELS_TABLE && group == channel_group::AGENT_ACCESS)
        {
            agent_access_priority(value.fields())
        } else if table == MEMBERSHIPS_TABLE && group == membership_group::MEMBERSHIP {
            membership_priority(value.fields())
        } else if table == CHANNEL_MEMBERSHIPS_TABLE && group == channel_membership_group::PRESENCE
        {
            channel_membership_priority(value.fields())
        } else if (table == PUSH_APPROVALS_TABLE && group == push_approval_group::APPROVAL)
            || (table == REMEMBERED_APPROVALS_TABLE && group == remembered_approval_group::APPROVAL)
        {
            push_approval_priority(value.fields())
        } else if (table == CHANNELS_TABLE && group == channel_group::LIFECYCLE)
            || (table == CONVERSATIONS_TABLE && group == conversation_group::LIFECYCLE)
            || (table == MESSAGES_TABLE && group == message_group::LIFECYCLE)
            || (table == CHANNEL_NOTES_TABLE && group == channel_note_group::LIFECYCLE)
        {
            lifecycle_priority(value.fields())
        } else if table == ASSIGNMENTS_TABLE && group == assignment_group::STATE {
            assignment_priority(value.fields())
        } else if table == INPUTS_TABLE && group == input_group::DELIVERY {
            input_delivery_priority(value.fields())
        } else {
            0
        }
    }

    fn validate_tombstone(
        &self,
        table: TableId,
        key: &str,
        cause: Option<&Cause>,
    ) -> Result<(), AdapterError> {
        validate_agent_tombstone(table, key, cause)
    }

    fn validate_row(
        &self,
        table: TableId,
        row: &RowKey,
        groups: &[GroupValue],
    ) -> Result<(), AdapterError> {
        if table == PARTICIPANTS_TABLE {
            validate_participant_row(row, groups)
        } else if table == AGENT_USAGE_DAILY_TABLE {
            validate_agent_usage_row(row, groups)
        } else if table == PROVIDER_STATUS_TABLE {
            validate_provider_status_row(row, groups)
        } else {
            Ok(())
        }
    }

    fn validate(&self, table: TableId, group: GroupId, value: &Value) -> Result<(), AdapterError> {
        let fields = value.fields();
        if table == PARTICIPANTS_TABLE {
            validate_participant(group, fields)
        } else if table == AGENTS_TABLE {
            validate_agent(group, fields)
        } else if table == POLICY_REVISIONS_TABLE {
            validate_policy_revision(group, fields)
        } else if table == APP_SETTINGS_TABLE {
            validate_app_settings(group, fields)
        } else if table == PROJECT_SETTINGS_TABLE {
            validate_project_settings(group, fields)
        } else if table == CONVERSATIONS_TABLE {
            validate_conversation(group, fields)
        } else if table == MEMBERSHIPS_TABLE {
            validate_membership(group, fields)
        } else if table == CHANNEL_MEMBERSHIPS_TABLE {
            validate_channel_membership(group, fields)
        } else if table == PUSH_APPROVALS_TABLE {
            validate_push_approval(group, fields)
        } else if table == REMEMBERED_APPROVALS_TABLE {
            validate_remembered_approval(group, fields)
        } else if table == CHANNELS_TABLE {
            validate_channel(group, fields)
        } else if table == CHANNEL_NOTES_TABLE {
            validate_channel_note(group, fields)
        } else if table == REPLY_THREADS_TABLE {
            validate_reply_thread(group, fields)
        } else if table == ITEMS_TABLE {
            validate_item(group, fields)
        } else if table == MESSAGES_TABLE {
            validate_message(group, fields)
        } else if table == REVISIONS_TABLE {
            validate_revision(group, fields)
        } else if table == REFERENCES_TABLE {
            validate_reference(group, fields)
        } else if table == CHANNEL_TARGETS_TABLE {
            validate_channel_target(group, fields)
        } else if table == ENVIRONMENT_TARGETS_TABLE
            || table == PARTICIPANT_TARGETS_TABLE
            || table == FOLDER_TARGETS_TABLE
            || table == PATH_TARGETS_TABLE
        {
            validate_target(table, group, fields)
        } else if table == CONVERSATION_CURSORS_TABLE || table == REPLY_CURSORS_TABLE {
            validate_read_cursor(group, fields)
        } else if table == SCHEDULED_TABLE {
            validate_scheduled(group, fields)
        } else if table == RECURRENCES_TABLE {
            validate_recurrence(group, fields)
        } else if table == ASSIGNMENTS_TABLE {
            validate_assignment(group, fields)
        } else if table == INPUTS_TABLE {
            validate_input(group, fields)
        } else if table == SEMANTIC_UPDATES_TABLE {
            validate_semantic_update(group, fields)
        } else if table == AGENT_USAGE_DAILY_TABLE {
            validate_agent_usage(group, fields)
        } else if table == PROVIDER_STATUS_TABLE {
            validate_provider_status(group, fields)
        } else {
            Ok(())
        }
    }

    fn asset_groups(&self, table: TableId) -> &'static [GroupId] {
        if table == REVISIONS_TABLE {
            &[timeline_group::ATTACHMENTS]
        } else if table == SCHEDULED_TABLE {
            &[scheduled_group::CONTENT]
        } else {
            &[]
        }
    }

    /// Each non-empty attachment of a revision references its digest named file. An empty
    /// text snippet has no file.
    fn asset_refs(
        &self,
        table: TableId,
        group: GroupId,
        value: &Value,
    ) -> Result<Vec<AssetRef>, AdapterError> {
        let blob = if table == REVISIONS_TABLE && group == timeline_group::ATTACHMENTS {
            value.fields().first()
        } else if table == SCHEDULED_TABLE && group == scheduled_group::CONTENT {
            value.fields().get(3)
        } else {
            return Ok(Vec::new());
        };
        attachments(blob.ok_or(AdapterError("chat attachments value"))?)?
            .into_iter()
            .filter(|attachment| attachment.byte_size > 0)
            .map(|attachment| {
                AssetRef::new(
                    &ATTACHMENTS_CLASS,
                    attachment.sha256,
                    attachment.mime_type,
                    attachment.byte_size,
                )
                .map_err(|_| AdapterError("chat attachment reference"))
            })
            .collect()
    }

    /// A participant presents its display name, an agent its provider, and a channel its name with
    /// its topic as the preview. A message revision, a scheduled message, and a channel note
    /// present their text as the preview, unchanged so it can be copied back. Other rows present
    /// nothing: a message's text lives in its revisions, outside the row's values.
    fn presentation(&self, table: TableId, values: &[(GroupId, Value)]) -> Presentation {
        if let Some(markdown) = presented_markdown(table, values) {
            return Presentation {
                title: String::new(),
                preview: markdown,
            };
        }
        let profile_group = match table {
            PARTICIPANTS_TABLE => participant_group::PROFILE,
            AGENTS_TABLE => agent_group::CREATED,
            CHANNELS_TABLE => channel_group::DETAILS,
            _ => return Presentation::default(),
        };
        let Some((_, value)) = values.iter().find(|(group, _)| *group == profile_group) else {
            return Presentation::default();
        };
        let text = |index: usize| match value.fields().get(index) {
            Some(Field::Text(text)) => text.split_whitespace().collect::<Vec<_>>().join(" "),
            _ => String::new(),
        };
        Presentation {
            title: text(0),
            preview: if table == CHANNELS_TABLE {
                text(1)
            } else {
                String::new()
            },
        }
    }

    /// A message revision is superseded when its message, deleted with it, names another revision
    /// as current or was itself deleted: an offer shows a post's newest text, and never the text
    /// of a post its author deleted.
    fn superseded_in_offer<'o>(
        &self,
        table: TableId,
        key: &str,
        values: &[(GroupId, Value)],
        offered: OfferedRows<'_, 'o>,
    ) -> bool {
        if table != REVISIONS_TABLE {
            return false;
        }
        let Some(Field::Text(message)) =
            group_fields(values, timeline_group::CREATED).and_then(|fields| fields.first())
        else {
            return false;
        };
        let Some(message_values) = offered(MESSAGES_TABLE, message) else {
            return false;
        };
        let deleted = group_fields(message_values, message_group::LIFECYCLE)
            .and_then(|fields| fields.first())
            .is_some_and(|field| *field != Field::Null);
        let replaced = match group_fields(message_values, message_group::CONTENT)
            .and_then(|fields| fields.first())
        {
            Some(Field::Text(current)) => current != key,
            _ => false,
        };
        deleted || replaced
    }
}

/// Fields of `group` among winning group values, `None` when the group has no value.
fn group_fields(values: &[(GroupId, Value)], group: GroupId) -> Option<&[Field]> {
    values
        .iter()
        .find(|(candidate, _)| *candidate == group)
        .map(|(_, value)| value.fields())
}

#[cfg(test)]
mod tests;
