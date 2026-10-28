//! Chat adapter: the value checks that mirror the Chat identity and policy table checks, and the
//! priorities of their coupled groups.
//!
//! A teammate's current policy and an access profile's current revision are coupled with a copy
//! of the revision's restrictiveness. Concurrent edits keep the more restrictive one: asking
//! for approval outranks approving for the user, which outranks full access, which outranks a
//! custom mode that defers to the provider, and a lower maximum folder capability outranks a
//! higher one. An edit made after
//! seeing the other still wins by causality.
//!
//! Conversation memberships, AI channel memberships, and working folder grants keep the narrower
//! access among concurrent edits: removal and revocation first, then the stricter approval
//! override, folder capability, and channel capabilities. A channel archive outranks a
//! concurrent unarchive, and a message deletion outranks a concurrent restore.
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
use ganbaru_sync_contracts::{Field, GroupId, OrderKey, TableId, Value};

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::assets::AssetRef;
use crate::codec;
use crate::manifest::OwnedTable;
use crate::manifest::vault::chat::{
    ADAPTER, ASSIGNMENTS_TABLE, ATTACHMENT_REFERENCES, ATTACHMENTS_CLASS,
    CHANNEL_MEMBERSHIPS_TABLE, CHANNEL_TARGETS_TABLE, CHANNELS_TABLE, CONVERSATION_CURSORS_TABLE,
    CONVERSATIONS_TABLE, ENVIRONMENT_TARGETS_TABLE, FOLDER_GRANTS_TABLE, FOLDER_TARGETS_TABLE,
    INPUTS_TABLE, ITEMS_TABLE, MEMBERSHIPS_TABLE, MESSAGES_TABLE, PARTICIPANT_TARGETS_TABLE,
    PARTICIPANTS_TABLE, PATH_TARGETS_TABLE, POLICY_REVISIONS_TABLE, PROFILE_REVISIONS_TABLE,
    PROFILES_TABLE, REFERENCES_TABLE, REPLY_CURSORS_TABLE, REPLY_THREADS_TABLE, REVISIONS_TABLE,
    SCHEDULED_ATTACHMENT_REFERENCES, SCHEDULED_REFERENCES, SCHEDULED_TABLE, SEMANTIC_UPDATES_TABLE,
    TEAMMATES_TABLE, assignment_group, channel_group, channel_membership_group, conversation_group,
    folder_grant_group, input_group, item_group, membership_group, message_group,
    participant_group, policy_revision_group, profile_group, profile_revision_group,
    read_cursor_group, reply_thread_group, scheduled_group, semantic_update_group, teammate_group,
    timeline_group,
};

/// Characters of a display name after trimming.
pub const MAX_DISPLAY_NAME_CHARS: usize = 160;
/// Characters of a teammate role after trimming.
pub const MAX_ROLE_CHARS: usize = 1000;
/// Characters of teammate instructions.
pub const MAX_INSTRUCTIONS_CHARS: usize = 65_536;
/// Characters of a provider instance id.
pub const MAX_PROVIDER_INSTANCE_CHARS: usize = 1024;
/// Characters of a stored JSON object: an avatar, a model selection, or provider options.
pub const MAX_JSON_CHARS: usize = 16_384;
/// Characters of the longest enumerated value any Chat replicated column holds.
pub const MAX_ENUM_CHARS: usize = 32;
/// Fewest characters of a stored timestamp, as the tables check it.
pub const MIN_TIMESTAMP_CHARS: usize = 20;

/// Participant kinds.
pub const PARTICIPANT_KINDS: [&str; 3] = ["local_user", "ai_teammate", "human"];
/// Teammate safety modes, least restrictive first. Custom defers to the
/// provider's own configuration, which can grant anything, so it ranks lowest.
pub const SAFETY_MODES: [&str; 4] = [
    "custom",
    "full_access",
    "approve_for_me",
    "ask_for_approval",
];
/// Reasoning effort levels.
pub const EFFORTS: [&str; 6] = ["none", "minimal", "low", "medium", "high", "xhigh"];
/// Response speeds.
pub const SPEEDS: [&str; 2] = ["standard", "fast"];
/// Interaction modes.
pub const INTERACTION_MODES: [&str; 1] = ["build"];
/// Built-in access profile keys.
pub const BUILTIN_PROFILE_KEYS: [&str; 5] = [
    "conversation_only",
    "read_only",
    "edit_files",
    "build_and_test",
    "publish_changes",
];
/// History boundaries of an access profile revision.
pub const HISTORY_BOUNDARIES: [&str; 2] = ["entire", "from_grant"];
/// Maximum folder capabilities, least restrictive first.
pub const FOLDER_CAPABILITIES: [&str; 5] = ["publish", "execute", "edit", "read", "none"];
/// Characters of a channel name.
pub const MAX_CHANNEL_NAME_CHARS: usize = 80;
/// Characters of a channel topic.
pub const MAX_CHANNEL_TOPIC_CHARS: usize = 250;
/// Conversation kinds.
pub const CONVERSATION_KINDS: [&str; 3] = ["channel", "direct_message", "task_discussion"];
/// Conversation membership roles, least restrictive first.
pub const MEMBERSHIP_ROLES: [&str; 2] = ["owner", "member"];
/// Runtime approval overrides, least restrictive first. Provider custom defers to the
/// provider's own configuration, mirroring the custom safety mode.
pub const APPROVAL_POLICIES: [&str; 4] = ["provider_custom", "unattended", "auto_approve", "ask"];
/// Characters of a Chat row id the timeline tables hold, as the tables check it.
pub const MAX_ID_CHARS: usize = 1024;
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
/// Settled work assignment states.
pub const SETTLED_ASSIGNMENT_STATES: [&str; 3] = ["completed", "failed", "cancelled"];
/// How a message reached an assignment.
pub const ROUTING_KINDS: [&str; 4] = ["trigger", "steer", "queued_continuation", "follow_up"];
/// Delivery states of an assignment input.
pub const DELIVERY_STATES: [&str; 4] = ["pending", "delivered", "failed", "ignored"];
/// Kinds of semantic work updates.
pub const SEMANTIC_UPDATE_KINDS: [&str; 7] = [
    "plan", "replan", "question", "approval", "failure", "result", "review",
];
/// Rank of no approval override, the least restrictive.
const INHERITED_APPROVAL_RANK: u8 = 1;

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

fn timestamp(field: &Field) -> Result<(), AdapterError> {
    if text(field)?.chars().count() >= MIN_TIMESTAMP_CHARS {
        Ok(())
    } else {
        Err(AdapterError("chat timestamp is too short"))
    }
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

/// A current revision pointer and its copied restrictiveness: both null, or a revision id with
/// a value from `allowed`.
fn current_pair(
    id: &Field,
    copied: &Field,
    allowed: &[&str],
    what: &'static str,
) -> Result<(), AdapterError> {
    match (optional_text(id)?, copied) {
        (None, Field::Null) => Ok(()),
        (Some(_), copied) => one_of(copied, allowed, what),
        (None, _) => Err(AdapterError(what)),
    }
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
        (participant_group::LIFECYCLE, [archived_at]) => optional_timestamp(archived_at),
        (participant_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat participant value")),
    }
}

fn validate_teammate(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (teammate_group::CREATED | teammate_group::UPDATED, [at]) => timestamp(at),
        (teammate_group::PROFILE, [role, instructions]) => {
            trimmed(role, MAX_ROLE_CHARS, "chat teammate role")?;
            bounded(
                instructions,
                0,
                MAX_INSTRUCTIONS_CHARS,
                "chat teammate instructions",
            )
        }
        (teammate_group::POLICY, [id, safety]) => {
            current_pair(id, safety, &SAFETY_MODES, "chat teammate current policy")
        }
        (teammate_group::HOME, [device]) => match optional_text(device)? {
            Some("") => Err(AdapterError("chat teammate home device")),
            _ => Ok(()),
        },
        _ => Err(AdapterError("chat teammate value")),
    }
}

fn validate_policy_revision(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (
            policy_revision_group::CREATED,
            [
                _teammate,
                provider,
                model_version,
                model,
                effort,
                speed,
                options_version,
                options,
                mode,
                created_at,
                safety,
            ],
        ) => {
            bounded(
                provider,
                1,
                MAX_PROVIDER_INSTANCE_CHARS,
                "chat policy provider instance",
            )?;
            schema_version(model_version)?;
            json_object(model, "chat policy model selection")?;
            optional_one_of(effort, &EFFORTS, "chat policy effort")?;
            optional_one_of(speed, &SPEEDS, "chat policy speed")?;
            schema_version(options_version)?;
            json_object(options, "chat policy provider options")?;
            one_of(mode, &INTERACTION_MODES, "chat policy interaction mode")?;
            timestamp(created_at)?;
            one_of(safety, &SAFETY_MODES, "chat policy safety mode")
        }
        _ => Err(AdapterError("chat policy revision value")),
    }
}

fn validate_profile(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (profile_group::CREATED, [builtin, created_at]) => {
            optional_one_of(builtin, &BUILTIN_PROFILE_KEYS, "chat built-in profile key")?;
            timestamp(created_at)
        }
        (profile_group::NAME, [name]) => {
            trimmed(name, MAX_DISPLAY_NAME_CHARS, "chat access profile name")
        }
        (profile_group::CURRENT, [id, capability]) => current_pair(
            id,
            capability,
            &FOLDER_CAPABILITIES,
            "chat access profile current revision",
        ),
        (profile_group::LIFECYCLE, [archived_at]) => optional_timestamp(archived_at),
        (profile_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat access profile value")),
    }
}

fn validate_profile_revision(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (
            profile_revision_group::CREATED,
            [
                _profile,
                read_history,
                participate,
                boundary,
                capability,
                created_at,
            ],
        ) => {
            flag(read_history)?;
            flag(participate)?;
            one_of(
                boundary,
                &HISTORY_BOUNDARIES,
                "chat profile history boundary",
            )?;
            one_of(
                capability,
                &FOLDER_CAPABILITIES,
                "chat profile folder capability",
            )?;
            timestamp(created_at)
        }
        _ => Err(AdapterError("chat access profile revision value")),
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

/// A history boundary and its order key: `entire` with none, or `from_grant` with a non-empty
/// key.
fn history_pair(boundary: &Field, order_key: &Field) -> Result<(), AdapterError> {
    one_of(boundary, &HISTORY_BOUNDARIES, "chat history boundary")?;
    match (text(boundary)?, order_key) {
        ("entire", Field::Null) => Ok(()),
        ("from_grant", key @ Field::Text(_)) => bounded(
            key,
            1,
            MAX_ORDER_KEY_BYTES,
            "chat history boundary order key",
        ),
        _ => Err(AdapterError("chat history boundary order key")),
    }
}

fn validate_channel_membership(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (channel_membership_group::IDENTITY, [_, _]) | (channel_membership_group::EXISTS, []) => {
            Ok(())
        }
        (
            channel_membership_group::ACCESS,
            [
                _profile,
                read_history,
                read_history_inherits,
                participate,
                participate_inherits,
                boundary,
                boundary_inherits,
                history_from,
                approval,
                scratch_approval,
                removed_at,
            ],
        ) => {
            for value in [
                read_history,
                read_history_inherits,
                participate,
                participate_inherits,
                boundary_inherits,
            ] {
                flag(value)?;
            }
            history_pair(boundary, history_from)?;
            optional_one_of(approval, &APPROVAL_POLICIES, "chat approval policy")?;
            optional_one_of(scratch_approval, &APPROVAL_POLICIES, "chat approval policy")?;
            optional_timestamp(removed_at)
        }
        (channel_membership_group::CREATED | channel_membership_group::UPDATED, [at]) => {
            timestamp(at)
        }
        _ => Err(AdapterError("chat channel membership value")),
    }
}

fn validate_folder_grant(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (folder_grant_group::IDENTITY, [_, _, _]) | (folder_grant_group::EXISTS, []) => Ok(()),
        (folder_grant_group::GRANT, [capability, inherits, is_default, approval, revoked_at]) => {
            one_of(
                capability,
                &FOLDER_CAPABILITIES,
                "chat folder grant capability",
            )?;
            flag(inherits)?;
            flag(is_default)?;
            optional_one_of(approval, &APPROVAL_POLICIES, "chat approval policy")?;
            optional_timestamp(revoked_at)
        }
        (folder_grant_group::CREATED, [_project, created_at]) => timestamp(created_at),
        _ => Err(AdapterError("chat folder grant value")),
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
        _ => Err(AdapterError("chat channel value")),
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
        (item_group::CREATED, [_conversation, _thread, kind, _order_key, created_at]) => {
            one_of(kind, &ITEM_KINDS, "chat item kind")?;
            timestamp(created_at)
        }
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
        (timeline_group::CREATED, [_target])
            if table == PARTICIPANT_TARGETS_TABLE || table == FOLDER_TARGETS_TABLE =>
        {
            Ok(())
        }
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
        (assignment_group::CREATED, [_thread, _teammate, _trigger, device, created_at]) => {
            executing_device(device)?;
            timestamp(created_at)
        }
        (assignment_group::PREVIOUS, [_previous]) => Ok(()),
        (assignment_group::STATE, [state, reason, settled_at]) => {
            one_of(state, &ASSIGNMENT_STATES, "chat assignment state")?;
            optional_bounded(reason, MAX_REASON_CHARS, "chat assignment state reason")?;
            optional_timestamp(settled_at)
        }
        (assignment_group::UPDATED, [at]) => timestamp(at),
        _ => Err(AdapterError("chat assignment value")),
    }
}

fn validate_input(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (input_group::IDENTITY, [_]) | (input_group::EXISTS, []) => Ok(()),
        (input_group::CREATED, [created_at]) => timestamp(created_at),
        (input_group::ROUTING, [_assignment, ordinal, kind]) => {
            at_least(ordinal, 1, "chat assignment input ordinal")?;
            one_of(kind, &ROUTING_KINDS, "chat assignment input routing")
        }
        (input_group::DELIVERY, [state, delivered_at]) => {
            one_of(state, &DELIVERY_STATES, "chat assignment input delivery")?;
            optional_timestamp(delivered_at)
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

/// A settled assignment outranks a concurrent active one, so a cancel, failure, or completion is
/// never undone by progress a device reported before seeing it; a review request outranks the
/// other active states.
fn assignment_priority(fields: &[Field]) -> u8 {
    match fields.first() {
        Some(Field::Text(state)) if SETTLED_ASSIGNMENT_STATES.contains(&state.as_str()) => 3,
        Some(Field::Text(state)) if state == "ready_for_review" => 2,
        _ => 1,
    }
}

/// Priority of an ended row: a removal or revocation outranks every live value.
const ENDED: u8 = u8::MAX;

/// Rank of an approval override, 1 for the least restrictive.
///
/// No override inherits: a folder grant's or scratch override falls back to the channel
/// override and a channel override to the teammate's default, which an explicit override can
/// only narrow (the runtime keeps the narrower of the default and the override). The default
/// can be unattended or defer to the provider, so inheriting ranks lowest, below the explicit
/// policies in [`APPROVAL_POLICIES`] order. A folder or scratch override replaces the channel
/// override instead of narrowing it, so when the channel override is stricter than a concurrent
/// explicit folder or scratch override, inheriting was the stricter intent; a static rank cannot
/// see the channel row, and the loser's value is not available to Chat after the merge.
fn approval_rank(field: Option<&Field>) -> u8 {
    match field {
        None | Some(Field::Null) => INHERITED_APPROVAL_RANK,
        field => INHERITED_APPROVAL_RANK + restrictiveness(field, &APPROVAL_POLICIES),
    }
}

/// Number of approval override ranks.
const APPROVAL_RANKS: u8 = INHERITED_APPROVAL_RANK + APPROVAL_POLICIES.len() as u8;

/// Whether the field at `index` is set, which marks a removal or revocation.
fn ended(fields: &[Field], index: usize) -> bool {
    !matches!(fields.get(index), None | Some(Field::Null))
}

/// Whether the inherit flag at `index` is set, so the value beside it comes from the profile.
fn inherits(fields: &[Field], index: usize) -> bool {
    matches!(fields.get(index), Some(Field::Integer(1)))
}

/// A membership removal wins, then demotion to member over promotion to owner.
fn membership_priority(fields: &[Field]) -> u8 {
    if ended(fields, 1) {
        ENDED
    } else {
        restrictiveness(fields.first(), &MEMBERSHIP_ROLES)
    }
}

/// A channel removal wins, then the value that clears more of history reading, participation,
/// and full history, then the stricter channel approval override, then the stricter scratch
/// approval override.
///
/// A capability counts as cleared only when it is set explicitly: an inherited one follows the
/// profile, which may grant it, and an explicit grant is still capped by the profile, so only
/// an explicit clear narrows. The access profile itself carries no rank, so concurrent edits
/// that differ only in the profile keep the engine's tie order.
fn channel_membership_priority(fields: &[Field]) -> u8 {
    if ended(fields, 10) {
        return ENDED;
    }
    let cleared = [
        !inherits(fields, 2) && matches!(fields.get(1), Some(Field::Integer(0))),
        !inherits(fields, 4) && matches!(fields.get(3), Some(Field::Integer(0))),
        !inherits(fields, 6)
            && matches!(fields.get(5), Some(Field::Text(text)) if text == "from_grant"),
    ]
    .into_iter()
    .filter(|cleared| *cleared)
    .count();
    let cleared = u8::try_from(cleared).unwrap_or(0);
    let approvals =
        (approval_rank(fields.get(8)) - 1) * APPROVAL_RANKS + (approval_rank(fields.get(9)) - 1);
    1 + cleared * APPROVAL_RANKS * APPROVAL_RANKS + approvals
}

/// A revocation wins, then the lower folder capability, then the stricter approval override.
/// An inherited capability follows the profile's maximum, which may be publishing, so it ranks
/// as the least restrictive capability.
fn folder_grant_priority(fields: &[Field]) -> u8 {
    if ended(fields, 4) {
        return ENDED;
    }
    let capability = if inherits(fields, 1) {
        1
    } else {
        restrictiveness(fields.first(), &FOLDER_CAPABILITIES).max(1)
    };
    1 + (capability - 1) * APPROVAL_RANKS + (approval_rank(fields.get(3)) - 1)
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

    /// The more restrictive current policy or profile revision wins among concurrent edits.
    fn priority(&self, table: TableId, group: GroupId, value: &Value) -> u8 {
        let copied = value.fields().get(1);
        if table == TEAMMATES_TABLE && group == teammate_group::POLICY {
            restrictiveness(copied, &SAFETY_MODES)
        } else if table == PROFILES_TABLE && group == profile_group::CURRENT {
            restrictiveness(copied, &FOLDER_CAPABILITIES)
        } else if table == MEMBERSHIPS_TABLE && group == membership_group::MEMBERSHIP {
            membership_priority(value.fields())
        } else if table == CHANNEL_MEMBERSHIPS_TABLE && group == channel_membership_group::ACCESS {
            channel_membership_priority(value.fields())
        } else if table == FOLDER_GRANTS_TABLE && group == folder_grant_group::GRANT {
            folder_grant_priority(value.fields())
        } else if (table == CHANNELS_TABLE && group == channel_group::LIFECYCLE)
            || (table == CONVERSATIONS_TABLE && group == conversation_group::LIFECYCLE)
            || (table == MESSAGES_TABLE && group == message_group::LIFECYCLE)
        {
            lifecycle_priority(value.fields())
        } else if table == ASSIGNMENTS_TABLE && group == assignment_group::STATE {
            assignment_priority(value.fields())
        } else {
            0
        }
    }

    fn validate(&self, table: TableId, group: GroupId, value: &Value) -> Result<(), AdapterError> {
        let fields = value.fields();
        if table == PARTICIPANTS_TABLE {
            validate_participant(group, fields)
        } else if table == TEAMMATES_TABLE {
            validate_teammate(group, fields)
        } else if table == POLICY_REVISIONS_TABLE {
            validate_policy_revision(group, fields)
        } else if table == PROFILES_TABLE {
            validate_profile(group, fields)
        } else if table == PROFILE_REVISIONS_TABLE {
            validate_profile_revision(group, fields)
        } else if table == CONVERSATIONS_TABLE {
            validate_conversation(group, fields)
        } else if table == MEMBERSHIPS_TABLE {
            validate_membership(group, fields)
        } else if table == CHANNEL_MEMBERSHIPS_TABLE {
            validate_channel_membership(group, fields)
        } else if table == FOLDER_GRANTS_TABLE {
            validate_folder_grant(group, fields)
        } else if table == CHANNELS_TABLE {
            validate_channel(group, fields)
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
        } else if table == ASSIGNMENTS_TABLE {
            validate_assignment(group, fields)
        } else if table == INPUTS_TABLE {
            validate_input(group, fields)
        } else if table == SEMANTIC_UPDATES_TABLE {
            validate_semantic_update(group, fields)
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

    /// Chat is owner-only until its presentation lands, so rows present nothing.
    fn presentation(&self, _table: TableId, _values: &[(GroupId, Value)]) -> Presentation {
        Presentation::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::OwnedRow;

    const AT: &str = "2026-10-24T10:51:41.000Z";

    fn value(fields: Vec<Field>) -> Value {
        Value::new(fields).unwrap()
    }

    fn t(text: &str) -> Field {
        Field::Text(text.to_owned())
    }

    fn policy(safety: Option<&str>) -> Value {
        match safety {
            Some(safety) => value(vec![t("policy-1"), t(safety)]),
            None => value(vec![Field::Null, Field::Null]),
        }
    }

    fn current(capability: Option<&str>) -> Value {
        match capability {
            Some(capability) => value(vec![t("revision-1"), t(capability)]),
            None => value(vec![Field::Null, Field::Null]),
        }
    }

    fn policy_revision(model: &str, safety: &str) -> Value {
        value(vec![
            t("teammate-1"),
            t("codex"),
            Field::Integer(1),
            t(model),
            t("high"),
            Field::Null,
            Field::Integer(1),
            t("{}"),
            t("build"),
            t(AT),
            t(safety),
        ])
    }

    #[test]
    fn chat_priorities_prefer_the_more_restrictive_revision() {
        let adapter = ChatAdapter;
        let safety =
            |mode| adapter.priority(TEAMMATES_TABLE, teammate_group::POLICY, &policy(mode));
        assert!(safety(Some("ask_for_approval")) > safety(Some("approve_for_me")));
        assert!(safety(Some("approve_for_me")) > safety(Some("full_access")));
        assert!(safety(Some("full_access")) > safety(Some("custom")));
        assert!(safety(Some("custom")) > safety(None));

        let capability =
            |level| adapter.priority(PROFILES_TABLE, profile_group::CURRENT, &current(level));
        assert!(capability(Some("none")) > capability(Some("read")));
        assert!(capability(Some("read")) > capability(Some("edit")));
        assert!(capability(Some("edit")) > capability(Some("execute")));
        assert!(capability(Some("execute")) > capability(Some("publish")));
        assert!(capability(Some("publish")) > capability(None));

        let name = value(vec![t("Profile")]);
        assert_eq!(
            adapter.priority(PROFILES_TABLE, profile_group::NAME, &name),
            0
        );
    }

    #[test]
    fn chat_validation_mirrors_the_table_checks() {
        let adapter = ChatAdapter;
        let check = |table, group, fields| adapter.validate(table, group, &value(fields));

        assert!(
            check(
                PARTICIPANTS_TABLE,
                participant_group::CREATED,
                vec![t("ai_teammate"), t(AT)]
            )
            .is_ok()
        );
        assert!(
            check(
                PARTICIPANTS_TABLE,
                participant_group::CREATED,
                vec![t("robot"), t(AT)]
            )
            .is_err()
        );
        let avatar = |json: &str| {
            check(
                PARTICIPANTS_TABLE,
                participant_group::PROFILE,
                vec![t(" Ada "), Field::Integer(1), t(json)],
            )
        };
        assert!(avatar("{\"kind\":\"initials\"}").is_ok());
        assert!(avatar("[]").is_err());
        assert!(avatar("{").is_err());
        let oversized = format!("{{\"k\":\"{}\"}}", "a".repeat(MAX_JSON_CHARS));
        assert!(avatar(&oversized).is_err());
        assert!(
            check(
                PARTICIPANTS_TABLE,
                participant_group::PROFILE,
                vec![t("   "), Field::Integer(1), t("{}")],
            )
            .is_err()
        );

        assert!(
            adapter
                .validate(TEAMMATES_TABLE, teammate_group::POLICY, &policy(None))
                .is_ok()
        );
        assert!(
            check(
                TEAMMATES_TABLE,
                teammate_group::POLICY,
                vec![Field::Null, t("custom")]
            )
            .is_err()
        );
        assert!(
            check(
                TEAMMATES_TABLE,
                teammate_group::POLICY,
                vec![t("policy-1"), Field::Null]
            )
            .is_err()
        );
        assert!(
            check(
                TEAMMATES_TABLE,
                teammate_group::POLICY,
                vec![t("policy-1"), t("reckless")]
            )
            .is_err()
        );
        assert!(check(TEAMMATES_TABLE, teammate_group::HOME, vec![Field::Null]).is_ok());
        assert!(check(TEAMMATES_TABLE, teammate_group::HOME, vec![t("")]).is_err());
        let instructions = "a".repeat(MAX_INSTRUCTIONS_CHARS + 1);
        assert!(
            check(
                TEAMMATES_TABLE,
                teammate_group::PROFILE,
                vec![t("Reviewer"), t(&instructions)]
            )
            .is_err()
        );

        let revision = |model, safety| {
            adapter.validate(
                POLICY_REVISIONS_TABLE,
                policy_revision_group::CREATED,
                &policy_revision(model, safety),
            )
        };
        assert!(revision("{\"model\":\"gpt\"}", "custom").is_ok());
        assert!(revision("\"gpt\"", "custom").is_err());
        assert!(revision("{}", "reckless").is_err());

        assert!(
            check(
                PROFILES_TABLE,
                profile_group::CREATED,
                vec![t("read_only"), t(AT)]
            )
            .is_ok()
        );
        assert!(
            check(
                PROFILES_TABLE,
                profile_group::CREATED,
                vec![Field::Null, t(AT)]
            )
            .is_ok()
        );
        assert!(
            check(
                PROFILES_TABLE,
                profile_group::CREATED,
                vec![t("root"), t(AT)]
            )
            .is_err()
        );
        assert!(
            adapter
                .validate(
                    PROFILES_TABLE,
                    profile_group::CURRENT,
                    &current(Some("edit"))
                )
                .is_ok()
        );

        let profile_revision = |flag_value, capability| {
            check(
                PROFILE_REVISIONS_TABLE,
                profile_revision_group::CREATED,
                vec![
                    t("profile-1"),
                    Field::Integer(flag_value),
                    Field::Integer(0),
                    t("from_grant"),
                    t(capability),
                    t(AT),
                ],
            )
        };
        assert!(profile_revision(1, "execute").is_ok());
        assert!(profile_revision(2, "execute").is_err());
        assert!(profile_revision(1, "admin").is_err());
    }

    fn optional(text: Option<&str>) -> Field {
        text.map_or(Field::Null, t)
    }

    fn membership(role: &str, removed: bool) -> Value {
        value(vec![t(role), optional(removed.then_some(AT))])
    }

    /// An AI channel membership access value with each capability set or cleared.
    fn channel_access(
        read_history: bool,
        participate: bool,
        from_grant: bool,
        approval: Option<&str>,
        scratch_approval: Option<&str>,
        removed: bool,
    ) -> Value {
        value(vec![
            t("profile-1"),
            Field::Integer(i64::from(read_history)),
            Field::Integer(0),
            Field::Integer(i64::from(participate)),
            Field::Integer(0),
            t(if from_grant { "from_grant" } else { "entire" }),
            Field::Integer(0),
            if from_grant { t("c001") } else { Field::Null },
            optional(approval),
            optional(scratch_approval),
            optional(removed.then_some(AT)),
        ])
    }

    fn grant(capability: &str, approval: Option<&str>, revoked: bool) -> Value {
        value(vec![
            t(capability),
            Field::Integer(0),
            Field::Integer(1),
            optional(approval),
            optional(revoked.then_some(AT)),
        ])
    }

    #[test]
    fn chat_membership_priorities_prefer_removal_then_demotion() {
        let adapter = ChatAdapter;
        let priority = |value: &Value| {
            adapter.priority(MEMBERSHIPS_TABLE, membership_group::MEMBERSHIP, value)
        };
        assert!(priority(&membership("owner", true)) > priority(&membership("member", false)));
        assert_eq!(
            priority(&membership("owner", true)),
            priority(&membership("member", true))
        );
        assert!(priority(&membership("member", false)) > priority(&membership("owner", false)));
        assert_eq!(
            adapter.priority(
                MEMBERSHIPS_TABLE,
                membership_group::CREATED,
                &value(vec![t(AT)])
            ),
            0
        );
    }

    #[test]
    fn chat_channel_membership_priorities_prefer_removal_then_fewer_capabilities_then_stricter_approval()
     {
        let adapter = ChatAdapter;
        let priority = |value: &Value| {
            adapter.priority(
                CHANNEL_MEMBERSHIPS_TABLE,
                channel_membership_group::ACCESS,
                value,
            )
        };
        let open = channel_access(true, true, false, Some("provider_custom"), None, false);
        let removed = channel_access(true, true, false, Some("provider_custom"), None, true);
        let strictest_live = channel_access(false, false, true, Some("ask"), Some("ask"), false);
        assert!(priority(&removed) > priority(&strictest_live));
        assert!(priority(&open) >= 1);
        assert!(priority(&strictest_live) < u8::MAX);

        // Clearing more capabilities wins over any approval override.
        let cleared = [
            channel_access(true, true, false, Some("ask"), Some("ask"), false),
            channel_access(false, true, false, None, None, false),
            channel_access(true, false, false, None, None, false),
        ];
        assert!(priority(&cleared[1]) > priority(&cleared[0]));
        assert!(priority(&cleared[2]) > priority(&cleared[0]));
        let more_cleared = [
            channel_access(true, true, false, Some("ask"), Some("ask"), false),
            channel_access(false, true, false, Some("ask"), Some("ask"), false),
            channel_access(false, false, false, Some("ask"), Some("ask"), false),
            channel_access(false, false, true, Some("ask"), Some("ask"), false),
        ];
        for (looser, stricter) in more_cleared.iter().zip(&more_cleared[1..]) {
            assert!(priority(stricter) > priority(looser));
        }
        assert!(
            priority(&channel_access(false, false, true, None, None, false))
                > priority(&channel_access(
                    false,
                    false,
                    false,
                    Some("ask"),
                    Some("ask"),
                    false
                ))
        );

        // With equal capabilities, the stricter approval override wins, and no override, which
        // inherits the teammate default, is the least strict.
        let approvals = [
            None,
            Some("provider_custom"),
            Some("unattended"),
            Some("auto_approve"),
            Some("ask"),
        ];
        for pair in approvals.windows(2) {
            let looser = channel_access(true, true, false, pair[0], Some("ask"), false);
            let stricter = channel_access(true, true, false, pair[1], None, false);
            assert!(priority(&stricter) > priority(&looser), "{pair:?}");
            let looser = channel_access(true, true, false, None, pair[0], false);
            let stricter = channel_access(true, true, false, None, pair[1], false);
            assert!(priority(&stricter) > priority(&looser), "scratch {pair:?}");
        }
    }

    #[test]
    fn chat_channel_membership_inherited_capabilities_rank_as_granted() {
        let adapter = ChatAdapter;
        let priority = |value: &Value| {
            adapter.priority(
                CHANNEL_MEMBERSHIPS_TABLE,
                channel_membership_group::ACCESS,
                value,
            )
        };
        let explicit = channel_access(false, false, true, None, None, false);
        let granted = channel_access(true, true, false, None, None, false);
        for inherit in [2, 4, 6] {
            let mut fields = explicit.fields().to_vec();
            fields[inherit] = Field::Integer(1);
            let inherited = value(fields);
            assert!(priority(&explicit) > priority(&inherited), "{inherit}");
            assert!(priority(&inherited) > priority(&granted), "{inherit}");
        }
        let mut fields = explicit.fields().to_vec();
        for inherit in [2, 4, 6] {
            fields[inherit] = Field::Integer(1);
        }
        assert_eq!(priority(&value(fields)), priority(&granted));
    }

    #[test]
    fn chat_folder_grant_priorities_prefer_revocation_then_lower_capability_then_stricter_approval()
    {
        let adapter = ChatAdapter;
        let priority =
            |value: &Value| adapter.priority(FOLDER_GRANTS_TABLE, folder_grant_group::GRANT, value);
        assert!(
            priority(&grant("publish", None, true)) > priority(&grant("none", Some("ask"), false))
        );
        for pair in FOLDER_CAPABILITIES.windows(2) {
            assert!(
                priority(&grant(pair[1], Some("provider_custom"), false))
                    > priority(&grant(pair[0], Some("ask"), false)),
                "{pair:?}"
            );
        }
        let approvals = [
            None,
            Some("provider_custom"),
            Some("unattended"),
            Some("auto_approve"),
            Some("ask"),
        ];
        for pair in approvals.windows(2) {
            assert!(
                priority(&grant("edit", pair[1], false)) > priority(&grant("edit", pair[0], false)),
                "{pair:?}"
            );
        }
        assert!(priority(&grant("none", Some("ask"), false)) < u8::MAX);

        // An inherited capability follows the profile's maximum, so it ranks as publishing.
        let mut inherited = grant("none", Some("ask"), false).fields().to_vec();
        inherited[1] = Field::Integer(1);
        assert_eq!(
            priority(&value(inherited)),
            priority(&grant("publish", Some("ask"), false))
        );
    }

    #[test]
    fn chat_channel_and_conversation_archive_outranks_unarchive() {
        let adapter = ChatAdapter;
        for (table, group) in [
            (CHANNELS_TABLE, channel_group::LIFECYCLE),
            (CONVERSATIONS_TABLE, conversation_group::LIFECYCLE),
        ] {
            let priority = |archived: Option<&str>| {
                adapter.priority(table, group, &value(vec![optional(archived)]))
            };
            assert!(priority(Some(AT)) > priority(None));
        }
        assert_eq!(
            adapter.priority(
                CHANNELS_TABLE,
                channel_group::DETAILS,
                &value(vec![t("general"), t("")])
            ),
            0
        );
    }

    #[test]
    fn chat_conversation_access_validation_mirrors_the_table_checks() {
        let adapter = ChatAdapter;
        let check = |table, group, fields| adapter.validate(table, group, &value(fields));

        assert!(
            check(
                CONVERSATIONS_TABLE,
                conversation_group::CREATED,
                vec![t("project-1"), t("channel"), t(AT)]
            )
            .is_ok()
        );
        assert!(
            check(
                CONVERSATIONS_TABLE,
                conversation_group::CREATED,
                vec![t("project-1"), t("thread"), t(AT)]
            )
            .is_err()
        );
        assert!(
            adapter
                .validate(
                    MEMBERSHIPS_TABLE,
                    membership_group::MEMBERSHIP,
                    &membership("member", true)
                )
                .is_ok()
        );
        assert!(
            adapter
                .validate(
                    MEMBERSHIPS_TABLE,
                    membership_group::MEMBERSHIP,
                    &membership("admin", false)
                )
                .is_err()
        );

        let access = |boundary: &str, history_from: Field, approval: &str| {
            let mut fields = channel_access(true, true, false, None, None, false)
                .fields()
                .to_vec();
            fields[5] = t(boundary);
            fields[7] = history_from;
            fields[8] = t(approval);
            check(
                CHANNEL_MEMBERSHIPS_TABLE,
                channel_membership_group::ACCESS,
                fields,
            )
        };
        assert!(access("entire", Field::Null, "ask").is_ok());
        assert!(access("from_grant", t("c001"), "unattended").is_ok());
        assert!(access("from_grant", Field::Null, "ask").is_err());
        assert!(access("from_grant", t(""), "ask").is_err());
        assert!(access("from_grant", Field::Integer(1), "ask").is_err());
        assert!(access("entire", t("c001"), "ask").is_err());
        assert!(access("entire", Field::Null, "always").is_err());

        assert!(
            adapter
                .validate(
                    FOLDER_GRANTS_TABLE,
                    folder_grant_group::GRANT,
                    &grant("execute", Some("auto_approve"), true)
                )
                .is_ok()
        );
        assert!(
            adapter
                .validate(
                    FOLDER_GRANTS_TABLE,
                    folder_grant_group::GRANT,
                    &grant("admin", None, false)
                )
                .is_err()
        );

        let details = |name: &str, topic: &str| {
            check(
                CHANNELS_TABLE,
                channel_group::DETAILS,
                vec![t(name), t(topic)],
            )
        };
        assert!(details("general", "").is_ok());
        assert!(details("", "").is_err());
        assert!(details(&"n".repeat(MAX_CHANNEL_NAME_CHARS + 1), "").is_err());
        assert!(details("general", &"t".repeat(MAX_CHANNEL_TOPIC_CHARS + 1)).is_err());
    }

    #[test]
    fn chat_channel_reference_ranges_need_ordered_well_formed_keys() {
        let adapter = ChatAdapter;
        let range = |lower: (&str, &str), high: (&str, &str)| {
            adapter.validate(
                CHANNEL_TARGETS_TABLE,
                timeline_group::CREATED,
                &value(vec![
                    t("reference-1"),
                    t("channel-1"),
                    t("channel-2"),
                    t(lower.0),
                    t(lower.1),
                    t(high.0),
                    t(high.1),
                    t("revision-1"),
                ]),
            )
        };
        assert!(range(("a0", ""), ("a0", "item-1")).is_ok());
        assert!(range(("a0", "item-1"), ("a0", "item-1")).is_ok());
        assert!(range(("a0", "item-1"), ("a1", "")).is_ok());
        assert!(range(("a1", ""), ("a0", "item-1")).is_err());
        assert!(range(("a0", "item-2"), ("a0", "item-1")).is_err());
        assert!(range(("!", ""), ("a0", "item-1")).is_err());
        assert!(range(("a0", ""), ("a", "item-1")).is_err());

        let empty = adapter.validate(
            CHANNEL_TARGETS_TABLE,
            timeline_group::CREATED,
            &value(vec![
                t("reference-1"),
                t("channel-1"),
                t("channel-2"),
                Field::Null,
                Field::Null,
                Field::Null,
                Field::Null,
                Field::Null,
            ]),
        );
        assert!(empty.is_ok());
    }

    fn attachment_row(kind: &str, mime: &str, size: i64) -> OwnedRow {
        OwnedRow::flat(vec![
            t("attachment-1"),
            t("working-folder:project-1"),
            t(kind),
            t("screen.png"),
            t(mime),
            Field::Integer(size),
            t(&"a".repeat(64)),
        ])
    }

    fn attachments_value(rows: &[OwnedRow]) -> Value {
        value(vec![Field::Blob(
            codec::encode(&ATTACHMENT_REFERENCES, rows).unwrap(),
        )])
    }

    #[test]
    fn chat_attachments_round_trip_and_mirror_the_reference_checks() {
        let adapter = ChatAdapter;
        let rows = vec![
            attachment_row("image", "image/png", 8),
            attachment_row("text_snippet", "text/plain", 0),
        ];
        let encoded = codec::encode(&ATTACHMENT_REFERENCES, &rows).unwrap();
        assert_eq!(
            codec::decode(&ATTACHMENT_REFERENCES, &encoded).unwrap(),
            rows
        );

        let check = |rows: &[OwnedRow]| {
            adapter.validate(
                REVISIONS_TABLE,
                timeline_group::ATTACHMENTS,
                &attachments_value(rows),
            )
        };
        assert_eq!(check(&rows), Ok(()));
        assert_eq!(check(&[]), Ok(()));
        let limit = i64::from(MAX_ATTACHMENT_BYTES);
        assert_eq!(
            check(&[attachment_row("image", "image/gif", limit)]),
            Ok(())
        );
        let oversized = attachment_row("image", "image/png", limit + 1);
        assert!(codec::encode(&ATTACHMENT_REFERENCES, &[oversized]).is_err());
        assert!(check(&[attachment_row("image", "image/png", 0)]).is_err());
        assert!(check(&[attachment_row("text_snippet", "image/png", 8)]).is_err());
        assert!(check(&[attachment_row("image", "application/pdf", 8)]).is_err());

        let mut upper_digest = attachment_row("image", "image/png", 8);
        upper_digest.fields[6] = t(&"A".repeat(64));
        assert!(check(&[upper_digest]).is_err());
        let mut long_id = attachment_row("image", "image/png", 8);
        long_id.fields[0] = t(&"x".repeat(MAX_ATTACHMENT_ID_CHARS + 1));
        assert!(codec::encode(&ATTACHMENT_REFERENCES, &[long_id]).is_err());
        let too_many = vec![attachment_row("image", "image/png", 8); MAX_MESSAGE_ATTACHMENTS + 1];
        assert!(codec::encode(&ATTACHMENT_REFERENCES, &too_many).is_err());
    }

    #[test]
    fn chat_attachment_asset_refs_skip_empty_snippets() {
        let adapter = ChatAdapter;
        assert_eq!(
            adapter.asset_groups(REVISIONS_TABLE),
            &[timeline_group::ATTACHMENTS]
        );
        let refs = adapter
            .asset_refs(
                REVISIONS_TABLE,
                timeline_group::ATTACHMENTS,
                &attachments_value(&[
                    attachment_row("image", "image/png", 8),
                    attachment_row("text_snippet", "text/plain", 0),
                ]),
            )
            .unwrap();
        assert_eq!(refs.len(), 1, "an empty text snippet has no file");
        assert_eq!(refs[0].class().name, "chat/attachments");
        assert_eq!(refs[0].content_type(), "image/png");
        assert_eq!(refs[0].byte_size(), 8);
        assert_eq!(refs[0].file_name(), format!("{}.png", "a".repeat(64)));
        assert!(
            adapter
                .asset_refs(
                    REVISIONS_TABLE,
                    timeline_group::CREATED,
                    &value(vec![t("body")])
                )
                .unwrap()
                .is_empty()
        );
    }
}
