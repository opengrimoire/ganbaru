//! Chat: table ids 352 to 447.
//!
//! The identity and policy tables replicate: participants, AI agents, and agent policy revisions
//! (tables 352 to 354; 355 and 356 are retired). An agent is a provider: it names its provider
//! family and points at its default model revision through a plain register. Revisions are
//! immutable rows; display numbers come from `(created_at, id)` order.
//!
//! Agent configuration replicates at three scopes: the vault settings singleton (table 357), the
//! settings of each project (table 358), and the columns each channel carries. Each holds an
//! access level and instructions, and concurrent access edits keep the narrower level.
//!
//! Conversations, their memberships, AI channel memberships, and channels replicate too (tables
//! 360 to 362 and 364; 363 is retired). The two membership tables use a `sync_key` derived from
//! their key columns. Concurrent edits of one membership keep the removal, and an AI channel
//! membership carries only its presence and an optional model for the channel.
//!
//! The timeline replicates as well (tables 370 to 381): reply threads, conversation items,
//! communication messages, their immutable revisions, message references and their five target
//! tables, and both read cursors. Items order by `(order_key, id)` within their conversation root
//! or reply thread, and history boundaries, channel reference ranges, and read cursors hold
//! `(order_key, item_id)` points of that order. Reply thread ids derive from their root item and
//! cursors use a derived `sync_key`. Revisions are bounded so one fits an operation.
//!
//! A revision owns its attachment references (`chat_communication_attachment_references`), each
//! carrying the attachment by value with its digest, so the referenced files of the
//! `chat/attachments` asset class travel through the asset store. The device's attachment and
//! resource rows stay `Local`; the Chat hook creates them for applied references, and the
//! attachment rows keep their files alive while queued or unsent work names them.
//!
//! Scheduled messages, their recurrences, work assignments, assignment inputs, and semantic
//! updates replicate (tables 390, 401, and 395 to 397). A scheduled message owns its attachment
//! and mention references and names the device that sends it, and a repeating one keeps its rule
//! and progress in a recurrence row deleted with it. An assignment names the device that
//! executes it and fixes the model and instructions it starts with. Input ids derive from their
//! message, so concurrent routing of one message converges, and a settled assignment state wins
//! over a concurrent active one.
//!
//! Standing push approvals replicate (table 365) with a key derived from the channel, agent,
//! folder, push URL, and destination branch. Only people approve, and a revocation wins over a
//! concurrent approval.
//!
//! Channel notes replicate (table 400). People write them, an agent only proposes one, and a
//! deletion wins over a concurrent edit, so a dismissed proposal stays dismissed.
//!
//! Daily agent usage summaries replicate (table 399): one row per device, agent, and local
//! day that only the device it names writes and whose counts only grow.
//!
//! Execution, drafts, review history, provider state, receipts, dispatch claims, cleanup jobs,
//! and the audience counters stay on the device as `Local` tables, and message search is
//! `Derived`. Replicas write Chat like the owner; device-local state never leaves the device.
//! Ids 359, 366 to 369, 382 to 389, 391 to 394, 398, and 402 to 447 are reserved.

use ganbaru_sync_contracts::bounds::MAX_DEVICE_ID_BYTES;
use ganbaru_sync_contracts::chat_push_approvals::{
    MAX_PUSH_APPROVAL_BRANCH_BYTES, MAX_PUSH_APPROVAL_URL_BYTES, PUSH_APPROVAL_KEY_PREFIX,
};
use ganbaru_sync_contracts::chat_recurrence;
use ganbaru_sync_contracts::chat_usage::{
    AGENT_USAGE_OWNER_PREFIX, LOCAL_DATE_CHARS, MAX_DAILY_COUNTED_TOKENS, MAX_DAILY_RUNS,
    MAX_DAILY_WORKING_SECONDS,
};
use ganbaru_sync_contracts::{GroupId, TableId};

use ganbaru_sync_contracts::bounds::MAX_ORDER_KEY_BYTES;

use crate::assets::{AssetClass, LocalAssetColumn, LocalDigest};
use crate::domains::chat::{
    MAX_ATTACHMENT_BYTES, MAX_ATTACHMENT_ID_CHARS, MAX_ATTACHMENT_MIME_CHARS,
    MAX_ATTACHMENT_NAME_CHARS, MAX_CHANNEL_NAME_CHARS, MAX_CHANNEL_NOTE_CHARS,
    MAX_CHANNEL_TOPIC_CHARS, MAX_DELEGATION_DEPTH, MAX_DELEGATION_EXPECTED_RESULT_CHARS,
    MAX_DELEGATION_OBJECTIVE_CHARS, MAX_DISPLAY_NAME_CHARS, MAX_ENUM_CHARS,
    MAX_ENVIRONMENT_ID_CHARS, MAX_ID_CHARS, MAX_INSTRUCTIONS_CHARS, MAX_JSON_CHARS,
    MAX_LABEL_CHARS, MAX_MESSAGE_ATTACHMENTS, MAX_MESSAGE_MARKDOWN_BYTES, MAX_MODEL_LABEL_CHARS,
    MAX_PROVIDER_INSTANCE_CHARS, MAX_REASON_CHARS, MAX_RELATIVE_PATH_CHARS,
    MAX_RESOLVED_INSTRUCTIONS_CHARS, MAX_RICH_CONTENT_BYTES, MAX_SCHEDULED_REFERENCES,
    MAX_SCHEDULED_REQUEST_BYTES, MAX_SEMANTIC_PAYLOAD_BYTES, SHA256_HEX_CHARS,
};
use crate::manifest::vault::projects::{FOLDERS_TABLE, PROJECTS_TABLE};
use crate::manifest::{
    Collation, ColumnSpec, ColumnType, Deletion, DerivedKey, DeviceTable, DomainManifest,
    DomainTrigger, GroupSpec, GroupStorage, LocalColumn, MergeKind, OnDelete, OwnedBound,
    OwnedCodec, OwnedOrder, OwnedReference, OwnedTable, ReferenceColumn, Resolution, RowKeyRule,
    RowOwner, SYNC_KEY_COLUMN, TableIdRange, TableSpec, TriggerPolicy, group_id,
};

/// Domain name for Chat.
pub const DOMAIN_NAME: &str = "chat";

/// Domain adapter name for Chat.
pub const ADAPTER: &str = "chat";

/// `chat_participants`.
pub const PARTICIPANTS_TABLE: TableId = TableId(352);
/// `chat_agents`.
pub const AGENTS_TABLE: TableId = TableId(353);
/// `chat_agent_policy_revisions`.
pub const POLICY_REVISIONS_TABLE: TableId = TableId(354);
/// `chat_agent_app_settings`.
pub const APP_SETTINGS_TABLE: TableId = TableId(357);
/// `chat_project_agent_settings`.
pub const PROJECT_SETTINGS_TABLE: TableId = TableId(358);
/// `chat_conversations`.
pub const CONVERSATIONS_TABLE: TableId = TableId(360);
/// `chat_conversation_memberships`.
pub const MEMBERSHIPS_TABLE: TableId = TableId(361);
/// `chat_agent_channel_memberships`.
pub const CHANNEL_MEMBERSHIPS_TABLE: TableId = TableId(362);
/// `chat_channels`.
pub const CHANNELS_TABLE: TableId = TableId(364);
/// `chat_push_approvals`.
pub const PUSH_APPROVALS_TABLE: TableId = TableId(365);
/// `chat_channel_notes`.
pub const CHANNEL_NOTES_TABLE: TableId = TableId(400);
/// `chat_reply_threads`.
pub const REPLY_THREADS_TABLE: TableId = TableId(370);
/// `chat_conversation_items`.
pub const ITEMS_TABLE: TableId = TableId(371);
/// `chat_communication_messages`.
pub const MESSAGES_TABLE: TableId = TableId(372);
/// `chat_communication_message_revisions`.
pub const REVISIONS_TABLE: TableId = TableId(373);
/// `chat_message_references`.
pub const REFERENCES_TABLE: TableId = TableId(374);
/// `chat_channel_reference_targets`.
pub const CHANNEL_TARGETS_TABLE: TableId = TableId(375);
/// `chat_execution_environment_reference_targets`.
pub const ENVIRONMENT_TARGETS_TABLE: TableId = TableId(376);
/// `chat_participant_reference_targets`.
pub const PARTICIPANT_TARGETS_TABLE: TableId = TableId(377);
/// `chat_working_folder_reference_targets`.
pub const FOLDER_TARGETS_TABLE: TableId = TableId(378);
/// `chat_workspace_path_reference_targets`.
pub const PATH_TARGETS_TABLE: TableId = TableId(379);
/// `chat_conversation_read_cursors`.
pub const CONVERSATION_CURSORS_TABLE: TableId = TableId(380);
/// `chat_reply_thread_read_cursors`.
pub const REPLY_CURSORS_TABLE: TableId = TableId(381);
/// `chat_scheduled_messages`.
pub const SCHEDULED_TABLE: TableId = TableId(390);
/// `chat_scheduled_message_recurrences`.
pub const RECURRENCES_TABLE: TableId = TableId(401);
/// `chat_work_assignments`.
pub const ASSIGNMENTS_TABLE: TableId = TableId(395);
/// `chat_work_assignment_inputs`.
pub const INPUTS_TABLE: TableId = TableId(396);
/// `chat_work_semantic_updates`.
pub const SEMANTIC_UPDATES_TABLE: TableId = TableId(397);
/// `chat_agent_usage_daily`.
pub const AGENT_USAGE_DAILY_TABLE: TableId = TableId(399);

/// Prefix of conversation membership keys, over the conversation and participant.
pub const MEMBERSHIP_KEY_PREFIX: &str = "conversation-membership";
/// Prefix of AI channel membership keys, over the conversation and agent.
pub const CHANNEL_MEMBERSHIP_KEY_PREFIX: &str = "ai-channel-membership";
/// Key of the vault's single agent settings row.
pub const APP_SETTINGS_KEY: &str = "vault";
/// Prefix of reply thread ids, over the root item.
pub const REPLY_THREAD_KEY_PREFIX: &str = "reply-thread";
/// Prefix of conversation read cursor keys, over the conversation and participant.
pub const CONVERSATION_CURSOR_KEY_PREFIX: &str = "conversation-read-cursor";
/// Prefix of reply thread read cursor keys, over the reply thread and participant.
pub const REPLY_CURSOR_KEY_PREFIX: &str = "reply-thread-read-cursor";
/// Prefix of assignment input ids, over the routed message.
pub const ASSIGNMENT_INPUT_KEY_PREFIX: &str = "assignment-input";
/// Prefix of scheduled message recurrence ids, over the scheduled message.
pub const RECURRENCE_KEY_PREFIX: &str = chat_recurrence::RECURRENCE_KEY_PREFIX;

/// Participant field groups.
pub mod participant_group {
    use super::{GroupId, group_id};

    /// `participant_kind` and `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `display_name`, `avatar_schema_version`, and `avatar_data`.
    pub const PROFILE: GroupId = group_id(1);
    /// `archived_at`.
    pub const LIFECYCLE: GroupId = group_id(2);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(3);
}

/// Agent field groups.
pub mod agent_group {
    use super::{GroupId, group_id};

    /// `provider_family_id` and `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `current_policy_revision_id`, the default model.
    pub const POLICY: GroupId = group_id(1);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(2);
}

/// Vault agent settings field groups.
pub mod app_settings_group {
    use super::{GroupId, group_id};

    /// `agent_access`.
    pub const ACCESS: GroupId = group_id(0);
    /// `agent_instructions`.
    pub const INSTRUCTIONS: GroupId = group_id(1);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(2);
}

/// Project agent settings field groups.
pub mod project_settings_group {
    use super::{GroupId, group_id};

    /// `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `agent_access`, null when the project inherits the vault's.
    pub const ACCESS: GroupId = group_id(1);
    /// `agent_instructions`.
    pub const INSTRUCTIONS: GroupId = group_id(2);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(3);
}

/// Agent policy revision field groups.
pub mod policy_revision_group {
    use super::{GroupId, group_id};

    /// Every column, fixed when the revision is saved.
    pub const CREATED: GroupId = group_id(0);
}

/// Conversation field groups.
pub mod conversation_group {
    use super::{GroupId, group_id};

    /// `project_id`, `conversation_kind`, and `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `last_activity_at`.
    pub const ACTIVITY: GroupId = group_id(1);
    /// `archived_at`.
    pub const LIFECYCLE: GroupId = group_id(2);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(3);
}

/// Conversation membership field groups.
pub mod membership_group {
    use super::{GroupId, group_id};

    /// `conversation_id` and `participant_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `membership_role` and `removed_at`.
    pub const MEMBERSHIP: GroupId = group_id(1);
    /// `created_at`.
    pub const CREATED: GroupId = group_id(2);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(3);
    /// Whether the row exists.
    pub const EXISTS: GroupId = group_id(4);
}

/// AI channel membership field groups.
pub mod channel_membership_group {
    use super::{GroupId, group_id};

    /// `conversation_id` and `agent_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `removed_at`.
    pub const PRESENCE: GroupId = group_id(1);
    /// `created_at`.
    pub const CREATED: GroupId = group_id(2);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(3);
    /// Whether the row exists.
    pub const EXISTS: GroupId = group_id(4);
    /// `model_selection_schema_version` and `model_selection_data`, the channel's model for the
    /// agent, or neither for the agent's default.
    pub const MODEL: GroupId = group_id(5);
}

/// Push approval field groups.
pub mod push_approval_group {
    use super::{GroupId, group_id};

    /// `conversation_id`, `agent_id`, `working_folder_id`, `push_url`, and
    /// `destination_branch`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `approved_by_participant_id`, `approved_at`, and `revoked_at`.
    pub const APPROVAL: GroupId = group_id(1);
    /// Whether the row exists.
    pub const EXISTS: GroupId = group_id(2);
}

/// Channel field groups.
pub mod channel_group {
    use super::{GroupId, group_id};

    /// `project_id`, `conversation_id`, `is_default`, and `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `name` and `topic`.
    pub const DETAILS: GroupId = group_id(1);
    /// `archived_at`.
    pub const LIFECYCLE: GroupId = group_id(2);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(3);
    /// `agent_access`, null when the channel inherits its project's.
    pub const AGENT_ACCESS: GroupId = group_id(4);
    /// `agent_instructions`.
    pub const AGENT_INSTRUCTIONS: GroupId = group_id(5);
}

/// Channel note field groups.
pub mod channel_note_group {
    use super::{GroupId, group_id};

    /// `channel_id` and `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `source_item_id` and `proposed_by_participant_id`, set when a note comes from an agent
    /// proposal.
    pub const PROVENANCE: GroupId = group_id(1);
    /// `body` and `edited_by_participant_id`, the person who wrote the current wording.
    pub const BODY: GroupId = group_id(2);
    /// `deleted_at`.
    pub const LIFECYCLE: GroupId = group_id(3);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(4);
}

/// Reply thread field groups.
pub mod reply_thread_group {
    use super::{GroupId, group_id};

    /// `root_item_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `conversation_id` and `created_at`.
    pub const CREATED: GroupId = group_id(1);
    /// `last_activity_at`.
    pub const ACTIVITY: GroupId = group_id(2);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(3);
    /// Whether the row exists.
    pub const EXISTS: GroupId = group_id(4);
}

/// Conversation item field groups.
pub mod item_group {
    use super::{GroupId, group_id};

    /// `conversation_id`, `reply_thread_id`, and `item_kind`, fixed when the item is posted.
    pub const CREATED: GroupId = group_id(0);
    /// `order_key` and `created_at`: where the item sits in its scope and when it entered it. The
    /// executing device moves a reply that waited for running work to the end of its thread when
    /// the work receives it.
    pub const PLACEMENT: GroupId = group_id(1);
}

/// Communication message field groups.
pub mod message_group {
    use super::{GroupId, group_id};

    /// `author_label_snapshot` and `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `author_participant_id`.
    pub const AUTHOR: GroupId = group_id(1);
    /// `current_revision_id` and `edited_at`.
    pub const CONTENT: GroupId = group_id(2);
    /// `deleted_at`.
    pub const LIFECYCLE: GroupId = group_id(3);
}

/// Groups of the immutable timeline tables: message revisions, message references, and the five
/// reference target tables.
pub mod timeline_group {
    use super::{GroupId, group_id};

    /// Every column, fixed when the row is written.
    pub const CREATED: GroupId = group_id(0);
    /// The attachments of a message revision, as owned rows. Other timeline tables have none.
    pub const ATTACHMENTS: GroupId = group_id(1);
}

/// Read cursor field groups, shared by conversation and reply thread cursors.
pub mod read_cursor_group {
    use super::{GroupId, group_id};

    /// The scope and the participant.
    pub const IDENTITY: GroupId = group_id(0);
    /// `last_read_order_key` and `last_read_item_id`.
    pub const READ: GroupId = group_id(1);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(2);
    /// Whether the row exists.
    pub const EXISTS: GroupId = group_id(3);
}

/// Scheduled message field groups.
pub mod scheduled_group {
    use super::{GroupId, group_id};

    /// `client_command_id`, `channel_id`, `reply_thread_id`, `executing_device_id`, and
    /// `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `request_schema_version`, `request_data`, and `scheduled_for`, with the attachment and
    /// mention references as owned rows.
    pub const CONTENT: GroupId = group_id(1);
    /// `failed_at` and `last_error`.
    pub const FAILURE: GroupId = group_id(2);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(3);
}

/// Scheduled message recurrence field groups.
pub mod recurrence_group {
    use super::{GroupId, group_id};

    /// `scheduled_message_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `created_at`.
    pub const CREATED: GroupId = group_id(1);
    /// `rule_kind`, `interval_minutes`, `local_minute`, `weekdays_mask`, `time_zone`, and
    /// `quiet_policy`.
    pub const RULE: GroupId = group_id(2);
    /// `next_occurrence_at` and `fired_count`, written by the executing device.
    pub const PROGRESS: GroupId = group_id(3);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(4);
    /// Whether the row exists.
    pub const EXISTS: GroupId = group_id(5);
}

/// Work assignment field groups.
pub mod assignment_group {
    use super::{GroupId, group_id};

    /// `reply_thread_id`, `agent_id`, `triggering_message_item_id`, `executing_device_id`,
    /// `created_at`, `model_selection_schema_version`, `model_selection_data`, and
    /// `instructions`, so the model and instructions are fixed with the request.
    pub const CREATED: GroupId = group_id(0);
    /// `previous_assignment_id`.
    pub const PREVIOUS: GroupId = group_id(1);
    /// `state`, `state_reason`, `settled_at`, `review_outcome`, `reviewed_at`, and
    /// `reviewed_by_participant_id`, so a review always travels with the completion it judges.
    pub const STATE: GroupId = group_id(2);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(3);
    /// `parent_assignment_id`, `delegation_depth`, `delegation_objective`,
    /// `delegation_expected_result`, and `delegation_folder_ceiling`, fixed when an agent
    /// delegates the assignment and null with depth 0 otherwise.
    pub const DELEGATION: GroupId = group_id(4);
    /// `model_label`, the model that ran, written by the executing device.
    pub const MODEL_LABEL: GroupId = group_id(5);
}

/// Assignment input field groups.
pub mod input_group {
    use super::{GroupId, group_id};

    /// `message_item_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `created_at`.
    pub const CREATED: GroupId = group_id(1);
    /// `assignment_id` and `routing_kind`.
    pub const ROUTING: GroupId = group_id(2);
    /// `delivery_state`, `delivered_at`, and `delivered_revision_id`.
    pub const DELIVERY: GroupId = group_id(3);
    /// Whether the row exists.
    pub const EXISTS: GroupId = group_id(4);
    /// `held`, set while a person keeps a waiting reply from delivery.
    pub const HOLD: GroupId = group_id(5);
    /// `ordinal`, apart from the routing so a reorder never undoes a concurrent move of the
    /// reply onto another assignment.
    pub const ORDER: GroupId = group_id(6);
}

/// Semantic update field groups.
pub mod semantic_update_group {
    use super::{GroupId, group_id};

    /// Every column, fixed when the update is posted.
    pub const CREATED: GroupId = group_id(0);
}

/// Daily agent usage field groups.
pub mod agent_usage_group {
    use super::{GroupId, group_id};

    /// `device_id`, `agent_id`, and `local_date`, the key inputs.
    pub const BUCKET: GroupId = group_id(0);
    /// `counted_tokens`.
    pub const TOKENS: GroupId = group_id(1);
    /// `working_seconds`.
    pub const WORKING: GroupId = group_id(2);
    /// `run_count`.
    pub const RUNS: GroupId = group_id(3);
}

/// A hidden, non-recoverable group without references.
const fn group(
    id: GroupId,
    name: &'static str,
    kind: MergeKind,
    columns: &'static [ColumnSpec],
) -> GroupSpec {
    GroupSpec {
        id,
        name,
        kind,
        storage: GroupStorage::Columns(columns),
        references: &[],
        surfaced: false,
        recoverable: false,
        bumps_revision: false,
    }
}

/// `spec`, shown as a conflict when concurrent versions differ and offered in recovery entries.
const fn surfaced(spec: GroupSpec) -> GroupSpec {
    GroupSpec {
        surfaced: true,
        recoverable: true,
        ..spec
    }
}

/// `spec`, bumping the table's local revision when it materializes.
const fn bumps(spec: GroupSpec) -> GroupSpec {
    GroupSpec {
        bumps_revision: true,
        ..spec
    }
}

/// A `Register` group.
const fn register(id: GroupId, name: &'static str, columns: &'static [ColumnSpec]) -> GroupSpec {
    group(id, name, MergeKind::Register, columns)
}

/// A silent `Coupled` group whose priority comes from the adapter.
const fn coupled(id: GroupId, name: &'static str, columns: &'static [ColumnSpec]) -> GroupSpec {
    group(id, name, MergeKind::Coupled, columns)
}

/// An `Immutable` group whose reference columns cascade from their rows.
const fn immutable(
    id: GroupId,
    columns: &'static [ColumnSpec],
    references: &'static [ReferenceColumn],
) -> GroupSpec {
    GroupSpec {
        references,
        ..group(id, "created", MergeKind::Immutable, columns)
    }
}

/// The `Immutable` identity group of a composite-key table: its key columns, each cascading from
/// its row.
const fn identity(
    id: GroupId,
    columns: &'static [ColumnSpec],
    references: &'static [ReferenceColumn],
) -> GroupSpec {
    GroupSpec {
        references,
        ..group(id, "identity", MergeKind::Immutable, columns)
    }
}

/// The columnless existence register of a derived-key table.
const fn exists(id: GroupId) -> GroupSpec {
    group(id, "exists", MergeKind::Register, &[])
}

/// `spec` with reference columns.
const fn referencing(spec: GroupSpec, references: &'static [ReferenceColumn]) -> GroupSpec {
    GroupSpec { references, ..spec }
}

/// The columns of an `updated` group.
const UPDATED_COLUMNS: &[ColumnSpec] = &[timestamp("updated_at")];

/// The `updated` group of a table with `updated_at`.
const fn updated(id: GroupId) -> GroupSpec {
    group(id, "updated", MergeKind::Max, UPDATED_COLUMNS)
}

const fn cascade(column: &'static str, table: TableId) -> ReferenceColumn {
    ReferenceColumn {
        column,
        table,
        on_delete: OnDelete::Cascade,
    }
}

const fn set_null(column: &'static str, table: TableId) -> ReferenceColumn {
    ReferenceColumn {
        column,
        table,
        on_delete: OnDelete::SetNull,
    }
}

/// An integer of at least `min`.
const fn integer(name: &'static str, min: i64) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Integer { min, max: i64::MAX },
    }
}

const fn order_key(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OrderKey,
    }
}

const fn text(name: &'static str, max_chars: usize) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Text { max_chars },
    }
}

const fn optional_text(name: &'static str, max_chars: usize) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalText { max_chars },
    }
}

const fn trimmed(name: &'static str, max_chars: usize) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::TrimmedText {
            min_chars: 1,
            max_chars,
        },
    }
}

/// A positive schema version.
const fn version(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Integer {
            min: 1,
            max: i64::MAX,
        },
    }
}

/// Null or a positive schema version.
const fn optional_version(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalInteger {
            min: 1,
            max: i64::MAX,
        },
    }
}

const fn flag(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Flag,
    }
}

const fn timestamp(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Timestamp,
    }
}

const fn optional_timestamp(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalTimestamp,
    }
}

const fn row_key(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::RowKey,
    }
}

const fn optional_row_key(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalRowKey,
    }
}

const fn applier_visible(name: &'static str) -> DomainTrigger {
    DomainTrigger {
        name,
        policy: TriggerPolicy::ApplierVisible,
    }
}

/// A trigger that writes only the device-local search index and keeps firing during apply.
const fn derived_index(name: &'static str) -> DomainTrigger {
    DomainTrigger {
        name,
        policy: TriggerPolicy::DerivedIndex,
    }
}

const fn gated(name: &'static str) -> DomainTrigger {
    DomainTrigger {
        name,
        policy: TriggerPolicy::Gated,
    }
}

const fn same_row(name: &'static str) -> DomainTrigger {
    DomainTrigger {
        name,
        policy: TriggerPolicy::SameRow,
    }
}

/// A multi-column CHECK whose columns share one group.
const fn within(columns: &'static [&'static str], group: GroupId) -> Resolution {
    Resolution::WithinGroup { columns, group }
}

/// The local `revision` counter of rows edited with an expected revision.
const REVISION: LocalColumn = LocalColumn {
    name: "revision",
    materialize: "1",
};

const BASE: TableSpec = TableSpec {
    id: PARTICIPANTS_TABLE,
    name: "",
    key_column: "id",
    groups: &[],
    local_columns: &[],
    derived_columns: &[],
    hook_columns: &[],
    key_derived_columns: &[],
    revision_column: None,
    resolutions: &[],
    redirects: false,
    adapter: Some(ADAPTER),
    triggers: &[],
    derived_key: None,
    tree: None,
    acyclic: None,
    publish_when: None,
    published_with: None,
    key_parent: None,
    owner: None,
    holds_deletes: false,
    key_rule: RowKeyRule::Any,
    deletion: Deletion::Ordinary,
    retained: None,
};

/// `chat_participants`, archive-only. The local owner keeps the id the baseline seeds, which an
/// insert trigger enforces because the key is outside every group.
pub const PARTICIPANTS: TableSpec = TableSpec {
    id: PARTICIPANTS_TABLE,
    name: "chat_participants",
    groups: &[
        immutable(
            participant_group::CREATED,
            &[
                text("participant_kind", MAX_ENUM_CHARS),
                timestamp("created_at"),
            ],
            &[],
        ),
        surfaced(bumps(register(
            participant_group::PROFILE,
            "profile",
            &[
                trimmed("display_name", MAX_DISPLAY_NAME_CHARS),
                version("avatar_schema_version"),
                text("avatar_data", MAX_JSON_CHARS),
            ],
        ))),
        bumps(register(
            participant_group::LIFECYCLE,
            "lifecycle",
            &[optional_timestamp("archived_at")],
        )),
        updated(participant_group::UPDATED),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    triggers: &[
        gated("chat_participant_push_approvals_archive"),
        applier_visible("chat_participants_local_user_identity"),
    ],
    deletion: Deletion::Never,
    ..BASE
};

/// `chat_agents`, keyed by the participant id. An agent for a provider takes an id derived from
/// the provider family, so devices that detect one provider concurrently create one agent. The
/// default model is a plain register over the revision id: reads use the named revision while
/// it belongs to the agent, else the agent's newest.
pub const AGENTS: TableSpec = TableSpec {
    id: AGENTS_TABLE,
    name: "chat_agents",
    key_column: "participant_id",
    key_parent: Some(PARTICIPANTS_TABLE),
    groups: &[
        immutable(
            agent_group::CREATED,
            &[
                trimmed("provider_family_id", MAX_PROVIDER_INSTANCE_CHARS),
                timestamp("created_at"),
            ],
            &[],
        ),
        register(
            agent_group::POLICY,
            "policy",
            &[optional_row_key("current_policy_revision_id")],
        ),
        updated(agent_group::UPDATED),
    ],
    deletion: Deletion::Never,
    ..BASE
};

/// `chat_agent_app_settings`, the single row of vault-wide agent access and instructions that
/// every project and channel inherits. Concurrent access edits keep the narrower level.
pub const APP_SETTINGS: TableSpec = TableSpec {
    id: APP_SETTINGS_TABLE,
    name: "chat_agent_app_settings",
    groups: &[
        bumps(coupled(
            app_settings_group::ACCESS,
            "access",
            &[text("agent_access", MAX_ENUM_CHARS)],
        )),
        surfaced(bumps(register(
            app_settings_group::INSTRUCTIONS,
            "instructions",
            &[text("agent_instructions", MAX_INSTRUCTIONS_CHARS)],
        ))),
        updated(app_settings_group::UPDATED),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    key_rule: RowKeyRule::Fixed(APP_SETTINGS_KEY),
    deletion: Deletion::Never,
    ..BASE
};

/// `chat_project_agent_settings`, keyed by its project and leaving with it. A null access
/// inherits the vault's, and concurrent access edits keep the narrower level, with an inherited
/// level counting as the widest.
pub const PROJECT_SETTINGS: TableSpec = TableSpec {
    id: PROJECT_SETTINGS_TABLE,
    name: "chat_project_agent_settings",
    key_column: "project_id",
    key_parent: Some(PROJECTS_TABLE),
    groups: &[
        immutable(
            project_settings_group::CREATED,
            &[timestamp("created_at")],
            &[],
        ),
        bumps(coupled(
            project_settings_group::ACCESS,
            "access",
            &[optional_text("agent_access", MAX_ENUM_CHARS)],
        )),
        surfaced(bumps(register(
            project_settings_group::INSTRUCTIONS,
            "instructions",
            &[text("agent_instructions", MAX_INSTRUCTIONS_CHARS)],
        ))),
        updated(project_settings_group::UPDATED),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    ..BASE
};

/// `chat_agent_policy_revisions`, an agent's default model and options, immutable and never
/// deleted.
pub const POLICY_REVISIONS: TableSpec = TableSpec {
    id: POLICY_REVISIONS_TABLE,
    name: "chat_agent_policy_revisions",
    groups: &[immutable(
        policy_revision_group::CREATED,
        &[
            row_key("agent_id"),
            version("model_selection_schema_version"),
            text("model_selection_data", MAX_JSON_CHARS),
            version("provider_options_schema_version"),
            text("provider_options_data", MAX_JSON_CHARS),
            text("interaction_mode", MAX_ENUM_CHARS),
            timestamp("created_at"),
        ],
        &[cascade("agent_id", AGENTS_TABLE)],
    )],
    triggers: &[
        applier_visible("chat_agent_policy_identity_insert"),
        applier_visible("chat_agent_policy_immutable_delete"),
        applier_visible("chat_agent_policy_immutable_update"),
    ],
    deletion: Deletion::Never,
    ..BASE
};

/// `chat_conversations`, archive-only and deleted only with their project. An archive wins over
/// a concurrent unarchive.
pub const CONVERSATIONS: TableSpec = TableSpec {
    id: CONVERSATIONS_TABLE,
    name: "chat_conversations",
    groups: &[
        immutable(
            conversation_group::CREATED,
            &[
                row_key("project_id"),
                text("conversation_kind", MAX_ENUM_CHARS),
                timestamp("created_at"),
            ],
            &[cascade("project_id", PROJECTS_TABLE)],
        ),
        group(
            conversation_group::ACTIVITY,
            "activity",
            MergeKind::Max,
            &[timestamp("last_activity_at")],
        ),
        bumps(coupled(
            conversation_group::LIFECYCLE,
            "lifecycle",
            &[optional_timestamp("archived_at")],
        )),
        updated(conversation_group::UPDATED),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    triggers: &[gated("chat_conversation_create_audience_state")],
    ..BASE
};

/// `chat_conversation_memberships`, removed by `removed_at`. A concurrent removal or demotion
/// wins over a re-add or promotion.
pub const MEMBERSHIPS: TableSpec = TableSpec {
    id: MEMBERSHIPS_TABLE,
    name: "chat_conversation_memberships",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        identity(
            membership_group::IDENTITY,
            &[row_key("conversation_id"), row_key("participant_id")],
            &[
                cascade("conversation_id", CONVERSATIONS_TABLE),
                cascade("participant_id", PARTICIPANTS_TABLE),
            ],
        ),
        bumps(coupled(
            membership_group::MEMBERSHIP,
            "membership",
            &[
                text("membership_role", MAX_ENUM_CHARS),
                optional_timestamp("removed_at"),
            ],
        )),
        register(
            membership_group::CREATED,
            "created",
            &[timestamp("created_at")],
        ),
        updated(membership_group::UPDATED),
        exists(membership_group::EXISTS),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    triggers: &[
        gated("chat_generic_membership_audience_delete"),
        gated("chat_generic_membership_audience_insert"),
        gated("chat_generic_membership_audience_update"),
    ],
    derived_key: Some(DerivedKey {
        exists_group: membership_group::EXISTS,
        prefix: Some(MEMBERSHIP_KEY_PREFIX),
        key_inputs: &["conversation_id", "participant_id"],
    }),
    ..BASE
};

/// `chat_agent_channel_memberships`, an agent's presence in a channel, removed by `removed_at`.
/// A concurrent removal wins over a re-add. The channel's model for the agent is a register
/// whose two columns travel together.
pub const CHANNEL_MEMBERSHIPS: TableSpec = TableSpec {
    id: CHANNEL_MEMBERSHIPS_TABLE,
    name: "chat_agent_channel_memberships",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        identity(
            channel_membership_group::IDENTITY,
            &[row_key("conversation_id"), row_key("agent_id")],
            &[
                cascade("conversation_id", CONVERSATIONS_TABLE),
                cascade("agent_id", AGENTS_TABLE),
            ],
        ),
        coupled(
            channel_membership_group::PRESENCE,
            "presence",
            &[optional_timestamp("removed_at")],
        ),
        register(
            channel_membership_group::CREATED,
            "created",
            &[timestamp("created_at")],
        ),
        updated(channel_membership_group::UPDATED),
        exists(channel_membership_group::EXISTS),
        register(
            channel_membership_group::MODEL,
            "model",
            &[
                optional_version("model_selection_schema_version"),
                optional_text("model_selection_data", MAX_JSON_CHARS),
            ],
        ),
    ],
    resolutions: &[within(
        &["model_selection_schema_version", "model_selection_data"],
        channel_membership_group::MODEL,
    )],
    triggers: &[
        gated("chat_agent_channel_membership_audience_delete"),
        gated("chat_agent_channel_membership_push_approvals_delete"),
        gated("chat_agent_channel_membership_push_approvals_update"),
        gated("chat_agent_channel_membership_audience_insert"),
        gated("chat_agent_channel_membership_audience_update"),
        applier_visible("chat_agent_channel_membership_identity_insert"),
        gated("chat_agent_channel_membership_requires_membership_insert"),
    ],
    derived_key: Some(DerivedKey {
        exists_group: channel_membership_group::EXISTS,
        prefix: Some(CHANNEL_MEMBERSHIP_KEY_PREFIX),
        key_inputs: &["conversation_id", "agent_id"],
    }),
    ..BASE
};

/// `chat_push_approvals`, revoked by `revoked_at`. Only a person approves, which local triggers
/// check. Removing the agent from the channel or archiving it revokes its approvals through
/// local triggers, and the Chat services revoke them when the channel's effective access falls
/// below Ask before commands. A revocation wins over a concurrent approval.
pub const PUSH_APPROVALS: TableSpec = TableSpec {
    id: PUSH_APPROVALS_TABLE,
    name: "chat_push_approvals",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        identity(
            push_approval_group::IDENTITY,
            &[
                row_key("conversation_id"),
                row_key("agent_id"),
                row_key("working_folder_id"),
                text("push_url", MAX_PUSH_APPROVAL_URL_BYTES),
                text("destination_branch", MAX_PUSH_APPROVAL_BRANCH_BYTES),
            ],
            &[
                cascade("conversation_id", CONVERSATIONS_TABLE),
                cascade("agent_id", AGENTS_TABLE),
                cascade("working_folder_id", FOLDERS_TABLE),
            ],
        ),
        bumps(referencing(
            coupled(
                push_approval_group::APPROVAL,
                "approval",
                &[
                    optional_row_key("approved_by_participant_id"),
                    timestamp("approved_at"),
                    optional_timestamp("revoked_at"),
                ],
            ),
            &[set_null("approved_by_participant_id", PARTICIPANTS_TABLE)],
        )),
        exists(push_approval_group::EXISTS),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    triggers: &[
        gated("chat_push_approvals_approver_is_person_insert"),
        gated("chat_push_approvals_approver_is_person_update"),
    ],
    derived_key: Some(DerivedKey {
        exists_group: push_approval_group::EXISTS,
        prefix: Some(PUSH_APPROVAL_KEY_PREFIX),
        key_inputs: &[
            "conversation_id",
            "agent_id",
            "working_folder_id",
            "push_url",
            "destination_branch",
        ],
    }),
    ..BASE
};

/// `chat_channels`, archive-only. A default channel's ids derive from its project, so devices
/// that create it concurrently create one row, and every other channel creates its own
/// conversation, so one channel per conversation needs only a local insert trigger. An archive
/// wins over a concurrent unarchive. The channel's agent access is null while it inherits its
/// project's, and concurrent access edits keep the narrower level.
pub const CHANNELS: TableSpec = TableSpec {
    id: CHANNELS_TABLE,
    name: "chat_channels",
    groups: &[
        immutable(
            channel_group::CREATED,
            &[
                row_key("project_id"),
                row_key("conversation_id"),
                flag("is_default"),
                timestamp("created_at"),
            ],
            &[
                cascade("project_id", PROJECTS_TABLE),
                cascade("conversation_id", CONVERSATIONS_TABLE),
            ],
        ),
        surfaced(bumps(register(
            channel_group::DETAILS,
            "details",
            &[
                text("name", MAX_CHANNEL_NAME_CHARS),
                text("topic", MAX_CHANNEL_TOPIC_CHARS),
            ],
        ))),
        bumps(coupled(
            channel_group::LIFECYCLE,
            "lifecycle",
            &[optional_timestamp("archived_at")],
        )),
        updated(channel_group::UPDATED),
        bumps(coupled(
            channel_group::AGENT_ACCESS,
            "agent_access",
            &[optional_text("agent_access", MAX_ENUM_CHARS)],
        )),
        surfaced(bumps(register(
            channel_group::AGENT_INSTRUCTIONS,
            "agent_instructions",
            &[text("agent_instructions", MAX_INSTRUCTIONS_CHARS)],
        ))),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    triggers: &[
        applier_visible("chat_channels_conversation_kind_insert"),
        gated("chat_channels_default_name_insert"),
        gated("chat_channels_default_protected_update"),
        gated("chat_channels_single_conversation_insert"),
        gated("chat_channels_single_default_insert"),
    ],
    ..BASE
};

/// `chat_channel_notes`. Only a person writes a note, which local triggers check; a note from a
/// agent proposal takes an id derived from the proposal item, so devices that accept or
/// dismiss one proposal concurrently write one row. Concurrent wording edits surface a
/// conflict, and a deletion wins over a concurrent edit.
pub const CHANNEL_NOTES: TableSpec = TableSpec {
    id: CHANNEL_NOTES_TABLE,
    name: "chat_channel_notes",
    groups: &[
        immutable(
            channel_note_group::CREATED,
            &[row_key("channel_id"), timestamp("created_at")],
            &[cascade("channel_id", CHANNELS_TABLE)],
        ),
        referencing(
            register(
                channel_note_group::PROVENANCE,
                "provenance",
                &[
                    optional_row_key("source_item_id"),
                    optional_row_key("proposed_by_participant_id"),
                ],
            ),
            &[
                set_null("source_item_id", ITEMS_TABLE),
                set_null("proposed_by_participant_id", PARTICIPANTS_TABLE),
            ],
        ),
        surfaced(bumps(referencing(
            register(
                channel_note_group::BODY,
                "body",
                &[
                    trimmed("body", MAX_CHANNEL_NOTE_CHARS),
                    optional_row_key("edited_by_participant_id"),
                ],
            ),
            &[set_null("edited_by_participant_id", PARTICIPANTS_TABLE)],
        ))),
        bumps(coupled(
            channel_note_group::LIFECYCLE,
            "lifecycle",
            &[optional_timestamp("deleted_at")],
        )),
        updated(channel_note_group::UPDATED),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    triggers: &[
        gated("chat_channel_notes_editor_is_person_insert"),
        gated("chat_channel_notes_editor_is_person_update"),
    ],
    ..BASE
};

/// `chat_reply_threads`. The id derives from the root item, so devices that open a thread on one
/// message concurrently create one row. Activity keeps the latest time.
pub const REPLY_THREADS: TableSpec = TableSpec {
    id: REPLY_THREADS_TABLE,
    name: "chat_reply_threads",
    groups: &[
        identity(
            reply_thread_group::IDENTITY,
            &[row_key("root_item_id")],
            &[],
        ),
        referencing(
            register(
                reply_thread_group::CREATED,
                "created",
                &[row_key("conversation_id"), timestamp("created_at")],
            ),
            &[cascade("conversation_id", CONVERSATIONS_TABLE)],
        ),
        bumps(group(
            reply_thread_group::ACTIVITY,
            "activity",
            MergeKind::Max,
            &[timestamp("last_activity_at")],
        )),
        updated(reply_thread_group::UPDATED),
        exists(reply_thread_group::EXISTS),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    triggers: &[gated("chat_reply_threads_root_item_insert")],
    derived_key: Some(DerivedKey {
        exists_group: reply_thread_group::EXISTS,
        prefix: Some(REPLY_THREAD_KEY_PREFIX),
        key_inputs: &["root_item_id"],
    }),
    ..BASE
};

/// `chat_conversation_items`. Items order by `(order_key, id)` within their scope, so concurrent
/// posts that take one key order by id on every device. Only the placement changes after the
/// post, when a waiting reply reaches its work.
pub const ITEMS: TableSpec = TableSpec {
    id: ITEMS_TABLE,
    name: "chat_conversation_items",
    groups: &[
        immutable(
            item_group::CREATED,
            &[
                row_key("conversation_id"),
                optional_row_key("reply_thread_id"),
                text("item_kind", MAX_ENUM_CHARS),
            ],
            &[
                cascade("conversation_id", CONVERSATIONS_TABLE),
                cascade("reply_thread_id", REPLY_THREADS_TABLE),
            ],
        ),
        register(
            item_group::PLACEMENT,
            "placement",
            &[order_key("order_key"), timestamp("created_at")],
        ),
    ],
    triggers: &[applier_visible(
        "chat_conversation_items_reply_thread_insert",
    )],
    ..BASE
};

/// `chat_communication_messages`. An edit points at a new immutable revision, a concurrent
/// delete wins over an edit, and a removed author leaves the label snapshot. Search rows are
/// rebuilt by the Chat sync hook for applied rows.
pub const MESSAGES: TableSpec = TableSpec {
    id: MESSAGES_TABLE,
    name: "chat_communication_messages",
    key_column: "item_id",
    key_parent: Some(ITEMS_TABLE),
    groups: &[
        immutable(
            message_group::CREATED,
            &[
                trimmed("author_label_snapshot", MAX_DISPLAY_NAME_CHARS),
                timestamp("created_at"),
            ],
            &[],
        ),
        referencing(
            register(
                message_group::AUTHOR,
                "author",
                &[optional_row_key("author_participant_id")],
            ),
            &[set_null("author_participant_id", PARTICIPANTS_TABLE)],
        ),
        surfaced(register(
            message_group::CONTENT,
            "content",
            &[
                optional_row_key("current_revision_id"),
                optional_timestamp("edited_at"),
            ],
        )),
        coupled(
            message_group::LIFECYCLE,
            "lifecycle",
            &[optional_timestamp("deleted_at")],
        ),
    ],
    triggers: &[
        gated("chat_communication_message_current_revision_update"),
        derived_index("chat_communication_search_delete"),
        gated("chat_communication_search_insert"),
    ],
    ..BASE
};

/// Managed Chat attachment files under `assets/chat/attachments`.
pub const ATTACHMENTS_CLASS: AssetClass = AssetClass {
    name: "chat/attachments",
    max_bytes: MAX_ATTACHMENT_BYTES,
};

/// `chat_communication_attachment_references`, the attachments of one revision in order. Each
/// row carries the attachment by value, so a replica can show and fetch it without the
/// device's attachment row.
pub const ATTACHMENT_REFERENCES: OwnedTable = OwnedTable {
    table: "chat_communication_attachment_references",
    parent_column: "message_revision_id",
    order: OwnedOrder::Ordinal("ordinal"),
    columns: &[
        text("attachment_id", MAX_ATTACHMENT_ID_CHARS),
        text("working_folder_id", MAX_ATTACHMENT_ID_CHARS),
        text("kind", MAX_ENUM_CHARS),
        trimmed("original_display_name", MAX_ATTACHMENT_NAME_CHARS),
        text("mime_type", MAX_ATTACHMENT_MIME_CHARS),
        ColumnSpec {
            name: "byte_size",
            ty: ColumnType::Integer {
                min: 0,
                max: MAX_ATTACHMENT_BYTES as i64,
            },
        },
        text("sha256", SHA256_HEX_CHARS),
    ],
    references: &[],
    unique: &[],
    max_rows: MAX_MESSAGE_ATTACHMENTS,
    codec: OwnedCodec::Custom,
    nested: &[],
    bound: OwnedBound::Static,
};

/// `chat_communication_message_revisions`, immutable and bounded so one revision fits an
/// operation. Display numbers come from `(created_at, id)` order within the message.
pub const REVISIONS: TableSpec = TableSpec {
    id: REVISIONS_TABLE,
    name: "chat_communication_message_revisions",
    groups: &[
        immutable(
            timeline_group::CREATED,
            &[
                row_key("message_item_id"),
                text("normalized_markdown", MAX_MESSAGE_MARKDOWN_BYTES),
                version("rich_content_schema_version"),
                text("rich_content_data", MAX_RICH_CONTENT_BYTES),
                timestamp("created_at"),
            ],
            &[cascade("message_item_id", MESSAGES_TABLE)],
        ),
        GroupSpec {
            storage: GroupStorage::Owned {
                columns: &[],
                tables: &[ATTACHMENT_REFERENCES],
            },
            ..group(
                timeline_group::ATTACHMENTS,
                "attachments",
                MergeKind::Immutable,
                &[],
            )
        },
    ],
    triggers: &[applier_visible(
        "chat_communication_message_revision_immutable_update",
    )],
    ..BASE
};

/// `chat_message_references`, immutable mention spans of one revision.
pub const REFERENCES: TableSpec = TableSpec {
    id: REFERENCES_TABLE,
    name: "chat_message_references",
    groups: &[immutable(
        timeline_group::CREATED,
        &[
            row_key("message_revision_id"),
            text("reference_kind", MAX_ENUM_CHARS),
            text("label_snapshot", MAX_LABEL_CHARS),
            text("plain_text_projection", MAX_LABEL_CHARS),
            integer("start_offset", 0),
            integer("end_offset", 1),
            integer("ordinal", 0),
            timestamp("created_at"),
        ],
        &[cascade("message_revision_id", REVISIONS_TABLE)],
    )],
    resolutions: &[within(
        &["start_offset", "end_offset"],
        timeline_group::CREATED,
    )],
    ..BASE
};

/// The base of the immutable mention target tables, keyed by their reference. Targets hold
/// values without foreign keys; a missing target renders the label snapshot.
const TARGET: TableSpec = TableSpec {
    key_column: "reference_id",
    key_parent: Some(REFERENCES_TABLE),
    ..BASE
};

/// `chat_channel_reference_targets`. A channel range holds `(order_key, item_id)` bounds and a
/// revision cutoff, all null for an empty range.
pub const CHANNEL_TARGETS: TableSpec = TableSpec {
    id: CHANNEL_TARGETS_TABLE,
    name: "chat_channel_reference_targets",
    groups: &[immutable(
        timeline_group::CREATED,
        &[
            row_key("channel_id"),
            row_key("source_conversation_id"),
            row_key("destination_conversation_id"),
            optional_text("source_lower_order_key", MAX_ORDER_KEY_BYTES),
            optional_text("source_lower_item_id", MAX_ID_CHARS),
            optional_text("source_high_order_key", MAX_ORDER_KEY_BYTES),
            optional_row_key("source_high_item_id"),
            optional_row_key("source_revision_cutoff_id"),
        ],
        &[],
    )],
    resolutions: &[within(
        &[
            "source_lower_order_key",
            "source_lower_item_id",
            "source_high_order_key",
            "source_high_item_id",
            "source_revision_cutoff_id",
        ],
        timeline_group::CREATED,
    )],
    ..TARGET
};

/// `chat_execution_environment_reference_targets`.
pub const ENVIRONMENT_TARGETS: TableSpec = TableSpec {
    id: ENVIRONMENT_TARGETS_TABLE,
    name: "chat_execution_environment_reference_targets",
    groups: &[immutable(
        timeline_group::CREATED,
        &[text("execution_environment_id", MAX_ENVIRONMENT_ID_CHARS)],
        &[],
    )],
    ..TARGET
};

/// `chat_participant_reference_targets`.
pub const PARTICIPANT_TARGETS: TableSpec = TableSpec {
    id: PARTICIPANT_TARGETS_TABLE,
    name: "chat_participant_reference_targets",
    groups: &[immutable(
        timeline_group::CREATED,
        &[row_key("participant_id")],
        &[],
    )],
    ..TARGET
};

/// `chat_working_folder_reference_targets`.
pub const FOLDER_TARGETS: TableSpec = TableSpec {
    id: FOLDER_TARGETS_TABLE,
    name: "chat_working_folder_reference_targets",
    groups: &[immutable(
        timeline_group::CREATED,
        &[row_key("working_folder_id")],
        &[],
    )],
    ..TARGET
};

/// `chat_workspace_path_reference_targets`.
pub const PATH_TARGETS: TableSpec = TableSpec {
    id: PATH_TARGETS_TABLE,
    name: "chat_workspace_path_reference_targets",
    groups: &[immutable(
        timeline_group::CREATED,
        &[
            row_key("working_folder_id"),
            text("path_kind", MAX_ENUM_CHARS),
            text("relative_path", MAX_RELATIVE_PATH_CHARS),
        ],
        &[],
    )],
    ..TARGET
};

/// The columns of a read cursor's `read` group: `''` for both when nothing is read.
const READ_COLUMNS: &[ColumnSpec] = &[
    text("last_read_order_key", MAX_ORDER_KEY_BYTES),
    text("last_read_item_id", MAX_ID_CHARS),
];

/// `chat_conversation_read_cursors`. Concurrent reads keep the later item; marking a channel
/// unread replicates when it follows the read it clears.
pub const CONVERSATION_CURSORS: TableSpec = TableSpec {
    id: CONVERSATION_CURSORS_TABLE,
    name: "chat_conversation_read_cursors",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        identity(
            read_cursor_group::IDENTITY,
            &[row_key("conversation_id"), row_key("participant_id")],
            &[
                cascade("conversation_id", CONVERSATIONS_TABLE),
                cascade("participant_id", PARTICIPANTS_TABLE),
            ],
        ),
        group(
            read_cursor_group::READ,
            "read",
            MergeKind::Max,
            READ_COLUMNS,
        ),
        updated(read_cursor_group::UPDATED),
        exists(read_cursor_group::EXISTS),
    ],
    derived_key: Some(DerivedKey {
        exists_group: read_cursor_group::EXISTS,
        prefix: Some(CONVERSATION_CURSOR_KEY_PREFIX),
        key_inputs: &["conversation_id", "participant_id"],
    }),
    ..BASE
};

/// `chat_reply_thread_read_cursors`. Concurrent reads keep the later item.
pub const REPLY_CURSORS: TableSpec = TableSpec {
    id: REPLY_CURSORS_TABLE,
    name: "chat_reply_thread_read_cursors",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        identity(
            read_cursor_group::IDENTITY,
            &[row_key("reply_thread_id"), row_key("participant_id")],
            &[
                cascade("reply_thread_id", REPLY_THREADS_TABLE),
                cascade("participant_id", PARTICIPANTS_TABLE),
            ],
        ),
        group(
            read_cursor_group::READ,
            "read",
            MergeKind::Max,
            READ_COLUMNS,
        ),
        updated(read_cursor_group::UPDATED),
        exists(read_cursor_group::EXISTS),
    ],
    derived_key: Some(DerivedKey {
        exists_group: read_cursor_group::EXISTS,
        prefix: Some(REPLY_CURSOR_KEY_PREFIX),
        key_inputs: &["reply_thread_id", "participant_id"],
    }),
    ..BASE
};

/// `chat_scheduled_message_attachment_references`, the attachments of one scheduled message in
/// order, carried by value like a revision's attachment references so the send succeeds after
/// the device's attachment row is gone.
pub const SCHEDULED_ATTACHMENT_REFERENCES: OwnedTable = OwnedTable {
    table: "chat_scheduled_message_attachment_references",
    parent_column: "scheduled_message_id",
    ..ATTACHMENT_REFERENCES
};

/// `chat_scheduled_message_references`, the mention spans of one scheduled message in order.
/// A row whose participant, channel, or working folder is gone is not materialized.
pub const SCHEDULED_REFERENCES: OwnedTable = OwnedTable {
    table: "chat_scheduled_message_references",
    parent_column: "scheduled_message_id",
    order: OwnedOrder::Ordinal("ordinal"),
    columns: &[
        text("reference_kind", MAX_ENUM_CHARS),
        text("label_snapshot", MAX_LABEL_CHARS),
        text("plain_text_projection", MAX_LABEL_CHARS),
        integer("start_offset", 0),
        integer("end_offset", 1),
        optional_row_key("participant_id"),
        optional_text("participant_kind", MAX_ENUM_CHARS),
        optional_row_key("channel_id"),
        optional_row_key("working_folder_id"),
        optional_text("path_kind", MAX_ENUM_CHARS),
        optional_text("relative_path", MAX_RELATIVE_PATH_CHARS),
        optional_text("execution_environment_id", MAX_ENVIRONMENT_ID_CHARS),
    ],
    references: &[
        OwnedReference {
            column: "participant_id",
            table: PARTICIPANTS_TABLE,
        },
        OwnedReference {
            column: "channel_id",
            table: CHANNELS_TABLE,
        },
        OwnedReference {
            column: "working_folder_id",
            table: FOLDERS_TABLE,
        },
    ],
    unique: &[],
    max_rows: MAX_SCHEDULED_REFERENCES,
    codec: OwnedCodec::Generic,
    nested: &[],
    bound: OwnedBound::Domain,
};

/// `chat_scheduled_messages`. The executing device sends the message, posting it under ids
/// derived from the scheduled message and deleting the row in one transaction. Claims and
/// retries stay in the device's `chat_scheduled_message_dispatch` row; only the executing device
/// writes a failure.
pub const SCHEDULED: TableSpec = TableSpec {
    id: SCHEDULED_TABLE,
    name: "chat_scheduled_messages",
    groups: &[
        immutable(
            scheduled_group::CREATED,
            &[
                text("client_command_id", MAX_ID_CHARS),
                row_key("channel_id"),
                optional_row_key("reply_thread_id"),
                optional_text("executing_device_id", MAX_DEVICE_ID_BYTES),
                timestamp("created_at"),
            ],
            &[
                cascade("channel_id", CHANNELS_TABLE),
                cascade("reply_thread_id", REPLY_THREADS_TABLE),
            ],
        ),
        GroupSpec {
            storage: GroupStorage::Owned {
                columns: &[
                    version("request_schema_version"),
                    text("request_data", MAX_SCHEDULED_REQUEST_BYTES),
                    timestamp("scheduled_for"),
                ],
                tables: &[SCHEDULED_ATTACHMENT_REFERENCES, SCHEDULED_REFERENCES],
            },
            ..register(scheduled_group::CONTENT, "content", &[])
        },
        register(
            scheduled_group::FAILURE,
            "failure",
            &[
                optional_timestamp("failed_at"),
                optional_text("last_error", MAX_REASON_CHARS),
            ],
        ),
        updated(scheduled_group::UPDATED),
    ],
    resolutions: &[Resolution::DuplicateRepair {
        columns: &["client_command_id"],
        collation: Collation::Binary,
    }],
    redirects: true,
    triggers: &[applier_visible(
        "chat_scheduled_messages_reply_thread_insert",
    )],
    ..BASE
};

/// An optional integer from `min` to `max`.
const fn optional_bounded(name: &'static str, min: i64, max: i64) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalInteger { min, max },
    }
}

/// `chat_scheduled_message_recurrences`, one per repeating scheduled message under an id derived
/// from it, and deleted with it, so a cancel wins over a concurrent rule edit or advance. The
/// scheduled message's executing device advances the progress, and an edit that changes the
/// rule or time on any device moves the next occurrence, so an edit concurrent with an advance
/// resolves to one whole progress value and can repeat or drop that one occurrence. Rules are
/// checked for shape on admission; the executing device resolves the zone.
pub const RECURRENCES: TableSpec = TableSpec {
    id: RECURRENCES_TABLE,
    name: "chat_scheduled_message_recurrences",
    groups: &[
        identity(
            recurrence_group::IDENTITY,
            &[row_key("scheduled_message_id")],
            &[cascade("scheduled_message_id", SCHEDULED_TABLE)],
        ),
        register(
            recurrence_group::CREATED,
            "created",
            &[timestamp("created_at")],
        ),
        register(
            recurrence_group::RULE,
            "rule",
            &[
                text("rule_kind", MAX_ENUM_CHARS),
                optional_bounded(
                    "interval_minutes",
                    chat_recurrence::MIN_INTERVAL_MINUTES as i64,
                    chat_recurrence::MAX_INTERVAL_MINUTES as i64,
                ),
                optional_bounded("local_minute", 0, chat_recurrence::LAST_LOCAL_MINUTE as i64),
                optional_bounded(
                    "weekdays_mask",
                    1,
                    chat_recurrence::ALL_WEEKDAYS_MASK as i64,
                ),
                trimmed("time_zone", chat_recurrence::MAX_TIME_ZONE_CHARS),
                text("quiet_policy", MAX_ENUM_CHARS),
            ],
        ),
        register(
            recurrence_group::PROGRESS,
            "progress",
            &[timestamp("next_occurrence_at"), integer("fired_count", 0)],
        ),
        updated(recurrence_group::UPDATED),
        exists(recurrence_group::EXISTS),
    ],
    resolutions: &[
        within(&["rule_kind", "interval_minutes"], recurrence_group::RULE),
        within(
            &["rule_kind", "local_minute", "weekdays_mask"],
            recurrence_group::RULE,
        ),
    ],
    derived_key: Some(DerivedKey {
        exists_group: recurrence_group::EXISTS,
        prefix: Some(RECURRENCE_KEY_PREFIX),
        key_inputs: &["scheduled_message_id"],
    }),
    ..BASE
};

/// `chat_work_assignments`. Only the executing device dispatches an assignment and writes its
/// active states; any device may settle it. Concurrent states resolve to the most restrictive
/// ending: cancelled, then failed, then completed, then a review request, then the other active
/// states. A delegated child names its parent and is deleted with it; an applier-visible
/// trigger keeps the child exactly one level deeper than its parent.
pub const ASSIGNMENTS: TableSpec = TableSpec {
    id: ASSIGNMENTS_TABLE,
    name: "chat_work_assignments",
    groups: &[
        immutable(
            assignment_group::CREATED,
            &[
                row_key("reply_thread_id"),
                row_key("agent_id"),
                row_key("triggering_message_item_id"),
                optional_text("executing_device_id", MAX_DEVICE_ID_BYTES),
                timestamp("created_at"),
                version("model_selection_schema_version"),
                text("model_selection_data", MAX_JSON_CHARS),
                text("instructions", MAX_RESOLVED_INSTRUCTIONS_CHARS),
            ],
            &[
                cascade("reply_thread_id", REPLY_THREADS_TABLE),
                cascade("agent_id", AGENTS_TABLE),
                cascade("triggering_message_item_id", MESSAGES_TABLE),
            ],
        ),
        referencing(
            register(
                assignment_group::PREVIOUS,
                "previous",
                &[optional_row_key("previous_assignment_id")],
            ),
            &[set_null("previous_assignment_id", ASSIGNMENTS_TABLE)],
        ),
        bumps(coupled(
            assignment_group::STATE,
            "state",
            &[
                text("state", MAX_ENUM_CHARS),
                optional_text("state_reason", MAX_REASON_CHARS),
                optional_timestamp("settled_at"),
                optional_text("review_outcome", MAX_ENUM_CHARS),
                optional_timestamp("reviewed_at"),
                optional_row_key("reviewed_by_participant_id"),
            ],
        )),
        updated(assignment_group::UPDATED),
        GroupSpec {
            references: &[cascade("parent_assignment_id", ASSIGNMENTS_TABLE)],
            ..group(
                assignment_group::DELEGATION,
                "delegation",
                MergeKind::Immutable,
                &[
                    optional_row_key("parent_assignment_id"),
                    ColumnSpec {
                        name: "delegation_depth",
                        ty: ColumnType::Integer {
                            min: 0,
                            max: MAX_DELEGATION_DEPTH,
                        },
                    },
                    optional_text("delegation_objective", MAX_DELEGATION_OBJECTIVE_CHARS),
                    optional_text(
                        "delegation_expected_result",
                        MAX_DELEGATION_EXPECTED_RESULT_CHARS,
                    ),
                    optional_text("delegation_folder_ceiling", MAX_JSON_CHARS),
                ],
            )
        },
        register(
            assignment_group::MODEL_LABEL,
            "model_label",
            &[optional_text("model_label", MAX_MODEL_LABEL_CHARS)],
        ),
    ],
    triggers: &[
        applier_visible("chat_work_assignment_delegation_insert"),
        gated("chat_work_assignment_delegation_depth_insert"),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    resolutions: &[within(
        &[
            "state",
            "review_outcome",
            "reviewed_at",
            "reviewed_by_participant_id",
        ],
        assignment_group::STATE,
    )],
    ..BASE
};

/// `chat_work_assignment_inputs`. The id derives from the routed message, so devices that route
/// one message concurrently create one row; rerouting moves it to another assignment.
pub const INPUTS: TableSpec = TableSpec {
    id: INPUTS_TABLE,
    name: "chat_work_assignment_inputs",
    groups: &[
        identity(
            input_group::IDENTITY,
            &[row_key("message_item_id")],
            &[cascade("message_item_id", MESSAGES_TABLE)],
        ),
        register(input_group::CREATED, "created", &[timestamp("created_at")]),
        referencing(
            register(
                input_group::ROUTING,
                "routing",
                &[
                    row_key("assignment_id"),
                    text("routing_kind", MAX_ENUM_CHARS),
                ],
            ),
            &[cascade("assignment_id", ASSIGNMENTS_TABLE)],
        ),
        coupled(
            input_group::DELIVERY,
            "delivery",
            &[
                text("delivery_state", MAX_ENUM_CHARS),
                optional_timestamp("delivered_at"),
                optional_row_key("delivered_revision_id"),
            ],
        ),
        exists(input_group::EXISTS),
        register(input_group::HOLD, "hold", &[flag("held")]),
        register(input_group::ORDER, "order", &[integer("ordinal", 1)]),
    ],
    derived_key: Some(DerivedKey {
        exists_group: input_group::EXISTS,
        prefix: Some(ASSIGNMENT_INPUT_KEY_PREFIX),
        key_inputs: &["message_item_id"],
    }),
    ..BASE
};

/// `chat_work_semantic_updates`, keyed by their conversation item and immutable.
pub const SEMANTIC_UPDATES: TableSpec = TableSpec {
    id: SEMANTIC_UPDATES_TABLE,
    name: "chat_work_semantic_updates",
    key_column: "item_id",
    key_parent: Some(ITEMS_TABLE),
    groups: &[immutable(
        semantic_update_group::CREATED,
        &[
            row_key("assignment_id"),
            text("update_kind", MAX_ENUM_CHARS),
            version("payload_schema_version"),
            text("payload_data", MAX_SEMANTIC_PAYLOAD_BYTES),
            timestamp("created_at"),
        ],
        &[cascade("assignment_id", ASSIGNMENTS_TABLE)],
    )],
    ..BASE
};

/// A daily usage count that only grows.
const fn daily_count(name: &'static str, max: i64) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Integer { min: 0, max },
    }
}

/// `chat_agent_usage_daily`, the usage one device's runs of one agent recorded during one
/// local day, keyed by the owner tag of the device followed by a digest of those three values,
/// under [`AGENT_USAGE_OWNER_PREFIX`]. Only the device the row names may write it, which every
/// replica enforces from the key, and its counts only grow, so `Max` converges. Rows are never
/// deleted while their agent exists, so the table declares no existence group and the
/// retention window is a read filter.
pub const AGENT_USAGE_DAILY: TableSpec = TableSpec {
    id: AGENT_USAGE_DAILY_TABLE,
    name: "chat_agent_usage_daily",
    owner: Some(RowOwner::KeyTag {
        prefix: AGENT_USAGE_OWNER_PREFIX,
        device_column: "device_id",
    }),
    groups: &[
        GroupSpec {
            references: &[cascade("agent_id", AGENTS_TABLE)],
            ..group(
                agent_usage_group::BUCKET,
                "bucket",
                MergeKind::Immutable,
                &[
                    trimmed("device_id", MAX_DEVICE_ID_BYTES),
                    row_key("agent_id"),
                    text("local_date", LOCAL_DATE_CHARS),
                ],
            )
        },
        group(
            agent_usage_group::TOKENS,
            "tokens",
            MergeKind::Max,
            &[daily_count("counted_tokens", MAX_DAILY_COUNTED_TOKENS)],
        ),
        group(
            agent_usage_group::WORKING,
            "working",
            MergeKind::Max,
            &[daily_count("working_seconds", MAX_DAILY_WORKING_SECONDS)],
        ),
        group(
            agent_usage_group::RUNS,
            "runs",
            MergeKind::Max,
            &[daily_count("run_count", MAX_DAILY_RUNS)],
        ),
    ],
    ..BASE
};

/// A device table with its triggers.
const fn device(name: &'static str, triggers: &'static [DomainTrigger]) -> DeviceTable {
    DeviceTable {
        name,
        seed: None,
        triggers,
        assets: &[],
    }
}

/// Chat domain. Replicas write it; a concurrent removal or revocation wins over a widening.
pub const DOMAIN: DomainManifest = DomainManifest {
    name: DOMAIN_NAME,
    table_ids: TableIdRange {
        first: 352,
        last: 447,
    },
    tables: &[
        PARTICIPANTS,
        AGENTS,
        POLICY_REVISIONS,
        APP_SETTINGS,
        PROJECT_SETTINGS,
        CONVERSATIONS,
        MEMBERSHIPS,
        CHANNEL_MEMBERSHIPS,
        PUSH_APPROVALS,
        CHANNELS,
        REPLY_THREADS,
        ITEMS,
        CHANNEL_NOTES,
        MESSAGES,
        REVISIONS,
        REFERENCES,
        CHANNEL_TARGETS,
        ENVIRONMENT_TARGETS,
        PARTICIPANT_TARGETS,
        FOLDER_TARGETS,
        PATH_TARGETS,
        CONVERSATION_CURSORS,
        REPLY_CURSORS,
        SCHEDULED,
        RECURRENCES,
        ASSIGNMENTS,
        INPUTS,
        SEMANTIC_UPDATES,
        AGENT_USAGE_DAILY,
    ],
    derived: &["chat_communication_search_fts"],
    local: &[
        device("chat_access_revocation_jobs", &[]),
        device(
            "chat_activities",
            &[
                gated("chat_activities_timeline_change_insert"),
                gated("chat_activities_timeline_change_update"),
                gated("chat_activities_timeline_change_delete"),
            ],
        ),
        device("chat_agent_run_delivered_sources", &[]),
        device("chat_agent_run_usage", &[]),
        device(
            "chat_agent_runs",
            &[
                applier_visible("chat_agent_run_attempt_immutable"),
                applier_visible("chat_agent_run_authority_immutable"),
                gated("chat_agent_run_authorization_insert"),
                gated("chat_agent_run_authorization_target_insert"),
                gated("chat_agent_run_execution_target_insert"),
                applier_visible("chat_agent_run_id_immutable"),
                gated("chat_agent_run_usage_working_insert"),
                gated("chat_agent_run_usage_working_update"),
            ],
        ),
        device(
            "chat_assignment_authorized_channel_sources",
            &[
                gated("chat_authorized_channel_source_immutable_delete"),
                applier_visible("chat_authorized_channel_source_immutable_update"),
            ],
        ),
        device(
            "chat_assignment_authorized_folder_sources",
            &[
                gated("chat_authorized_folder_source_immutable_delete"),
                applier_visible("chat_authorized_folder_source_immutable_update"),
                gated("chat_authorized_folder_source_requires_approval"),
            ],
        ),
        device("chat_assignment_context_sources", &[]),
        device("chat_assignment_context_packages", &[]),
        device("chat_assignment_dispatch_jobs", &[]),
        device("chat_assignment_input_requested_targets", &[]),
        device("chat_assignment_requested_targets", &[]),
        device(
            "chat_attachment_references",
            &[gated("chat_attachment_messages_expose_resource")],
        ),
        DeviceTable {
            name: "chat_attachments",
            seed: None,
            triggers: &[
                gated("chat_attachments_create_resource"),
                applier_visible("chat_attachments_managed_relative_path_shape_insert"),
                applier_visible("chat_attachments_managed_relative_path_shape_update"),
                gated("chat_attachments_update_resource_integrity"),
            ],
            assets: &[LocalAssetColumn {
                column: "sha256",
                json_path: None,
                digest: LocalDigest::Sha256 {
                    class: ATTACHMENTS_CLASS.name,
                },
            }],
        },
        device("chat_browser_artifacts", &[]),
        device("chat_checkpoint_failures", &[]),
        device("chat_cleanup_jobs", &[]),
        device("chat_command_receipts", &[]),
        device(
            "chat_conversation_audience_state",
            &[applier_visible(
                "chat_conversation_audience_revision_monotonic",
            )],
        ),
        device("chat_drafts", &[]),
        device("chat_events", &[gated("chat_events_require_next_sequence")]),
        device("chat_host_tool_returned_message_revisions", &[]),
        device("chat_host_tool_invocations", &[]),
        device(
            "chat_assignment_authorization_revisions",
            &[
                gated("chat_assignment_authorization_delete_denied"),
                applier_visible("chat_assignment_authorization_id_immutable"),
                applier_visible("chat_assignment_authorization_reason_immutable"),
                applier_visible("chat_assignment_authorization_revocation_insert"),
                applier_visible("chat_assignment_authorization_revocation_update"),
                applier_visible("chat_assignment_authorization_scope_immutable"),
                applier_visible("chat_assignment_authorization_state_transition"),
                gated("chat_assignment_authorization_target_required_insert"),
                applier_visible("chat_assignment_authorization_terminal_immutable"),
            ],
        ),
        device("chat_organizational_command_receipts", &[]),
        device(
            "chat_plans",
            &[
                gated("chat_plans_timeline_change_insert"),
                gated("chat_plans_timeline_change_update"),
                gated("chat_plans_timeline_change_delete"),
            ],
        ),
        device("chat_preview_tabs", &[]),
        device("chat_preview_tool_invocations", &[]),
        device("chat_provider_cleanup_jobs", &[]),
        device("chat_push_request_claims", &[]),
        device("chat_queued_attachment_references", &[]),
        device("chat_queued_followups", &[]),
        device("chat_resource_thread_references", &[]),
        device(
            "chat_resources",
            &[
                applier_visible("chat_resources_managed_relative_path_shape_insert"),
                applier_visible("chat_resources_managed_relative_path_shape_update"),
            ],
        ),
        device(
            "chat_messages",
            &[
                gated("chat_messages_timeline_change_insert"),
                gated("chat_messages_timeline_change_update"),
                gated("chat_messages_timeline_change_delete"),
            ],
        ),
        device("chat_restore_operations", &[]),
        device("chat_restore_previews", &[]),
        device(
            "chat_review_comments",
            &[
                applier_visible("chat_review_comments_relative_path_shape_insert"),
                applier_visible("chat_review_comments_relative_path_shape_update"),
            ],
        ),
        device("chat_scheduled_message_dispatch", &[]),
        device("chat_scratch_cleanup_jobs", &[]),
        device(
            "chat_scratch_generation_sources",
            &[
                gated("chat_scratch_generation_source_delete_denied"),
                applier_visible("chat_scratch_generation_source_monotonic"),
            ],
        ),
        device(
            "chat_scratch_promotions",
            &[
                applier_visible("chat_scratch_attachment_promotion_completed"),
                gated("chat_scratch_promotion_audit_delete_denied"),
                applier_visible("chat_scratch_promotion_authority_immutable"),
                applier_visible("chat_scratch_promotion_completion_provenance"),
                gated("chat_scratch_promotion_request_digest_required"),
                applier_visible("chat_scratch_promotion_terminal_immutable"),
            ],
        ),
        device("chat_secret_requests", &[]),
        device("chat_terminal_attachment_contexts", &[]),
        device("chat_terminal_layouts", &[]),
        device("chat_thread_relations", &[]),
        device("chat_timeline_change_clocks", &[]),
        device("chat_timeline_removals", &[]),
        device("chat_user_input_drafts", &[]),
        device("chat_pending_requests", &[]),
        device("chat_worktrees", &[]),
        device("chat_checkpoints", &[]),
        device(
            "chat_turns",
            &[
                gated("chat_turns_timeline_change_insert"),
                gated("chat_turns_timeline_change_update"),
            ],
        ),
        device(
            "chat_threads",
            &[
                same_row("chat_threads_assign_current_environment"),
                gated("chat_threads_execution_target_insert"),
                applier_visible("chat_threads_execution_target_update"),
                gated("chat_threads_project_matches_working_folder_insert"),
                applier_visible("chat_threads_project_matches_working_folder_update"),
            ],
        ),
        device(
            "chat_execution_environments",
            &[applier_visible(
                "chat_execution_environment_authority_immutable",
            )],
        ),
        device(
            "chat_scratch_generations",
            &[applier_visible(
                "chat_scratch_generation_identity_immutable",
            )],
        ),
        device(
            "chat_scratch_scopes",
            &[applier_visible("chat_scratch_scope_identity_immutable")],
        ),
    ],
    engine: &[],
    unconverted: &[],
    assets: &[ATTACHMENTS_CLASS],
};
