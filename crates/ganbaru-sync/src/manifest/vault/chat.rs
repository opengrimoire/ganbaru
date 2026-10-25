//! Chat: table ids 352 to 399.
//!
//! The identity and policy tables replicate: participants, AI teammates, teammate policy
//! revisions, access profiles, and access profile revisions (tables 352 to 356). Teammates and
//! profiles point at their effective revision through a silent coupled register that carries a
//! copy of the revision's restrictiveness, so concurrent policy edits settle on the stricter one.
//! Revisions are immutable rows; display numbers come from `(created_at, id)` order.
//!
//! Execution, drafts, review history, provider state, receipts, cleanup jobs, and the access
//! and audience counters stay on the device as `Local` tables, and message search is `Derived`.
//! The remaining organizational tables stay unconverted until later steps convert them. Chat is
//! owner-only: replicas refuse Chat writes until its presentation lands. Ids 357 to 399 are
//! reserved.

use ganbaru_sync_contracts::bounds::MAX_DEVICE_ID_BYTES;
use ganbaru_sync_contracts::{GroupId, TableId};

use crate::domains::chat::{
    MAX_DISPLAY_NAME_CHARS, MAX_ENUM_CHARS, MAX_INSTRUCTIONS_CHARS, MAX_JSON_CHARS,
    MAX_PROVIDER_INSTANCE_CHARS, MAX_ROLE_CHARS,
};
use crate::manifest::{
    Collation, ColumnSpec, ColumnType, DeviceTable, DomainManifest, DomainTrigger, GroupSpec,
    GroupStorage, LocalColumn, MergeKind, OnDelete, ReferenceColumn, Resolution, TableIdRange,
    TableSpec, TriggerPolicy, group_id,
};

/// Domain name for Chat.
pub const DOMAIN_NAME: &str = "chat";

/// Domain adapter name for Chat.
pub const ADAPTER: &str = "chat";

/// `chat_participants`.
pub const PARTICIPANTS_TABLE: TableId = TableId(352);
/// `chat_teammates`.
pub const TEAMMATES_TABLE: TableId = TableId(353);
/// `chat_teammate_policy_revisions`.
pub const POLICY_REVISIONS_TABLE: TableId = TableId(354);
/// `chat_access_profiles`.
pub const PROFILES_TABLE: TableId = TableId(355);
/// `chat_access_profile_revisions`.
pub const PROFILE_REVISIONS_TABLE: TableId = TableId(356);

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

/// Teammate field groups.
pub mod teammate_group {
    use super::{GroupId, group_id};

    /// `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `role` and `instructions`.
    pub const PROFILE: GroupId = group_id(1);
    /// `current_policy_revision_id` and the copied `current_policy_safety_mode`.
    pub const POLICY: GroupId = group_id(2);
    /// `home_device_id`.
    pub const HOME: GroupId = group_id(3);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(4);
}

/// Teammate policy revision field groups.
pub mod policy_revision_group {
    use super::{GroupId, group_id};

    /// Every column, fixed when the revision is saved.
    pub const CREATED: GroupId = group_id(0);
}

/// Access profile field groups.
pub mod profile_group {
    use super::{GroupId, group_id};

    /// `builtin_key` and `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `display_name`.
    pub const NAME: GroupId = group_id(1);
    /// `current_revision_id` and the copied `current_maximum_folder_capability`.
    pub const CURRENT: GroupId = group_id(2);
    /// `archived_at`.
    pub const LIFECYCLE: GroupId = group_id(3);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(4);
}

/// Access profile revision field groups.
pub mod profile_revision_group {
    use super::{GroupId, group_id};

    /// Every column, fixed when the revision is saved.
    pub const CREATED: GroupId = group_id(0);
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
        ty: ColumnType::OptionalRowKey,
    }
}

const fn applier_visible(name: &'static str) -> DomainTrigger {
    DomainTrigger {
        name,
        policy: TriggerPolicy::ApplierVisible,
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
    holds_deletes: false,
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
    triggers: &[applier_visible("chat_participants_local_user_identity")],
    ..BASE
};

/// `chat_teammates`, keyed by the participant id. Concurrent policy edits keep the stricter
/// safety mode, and the home device names where its assignments run.
pub const TEAMMATES: TableSpec = TableSpec {
    id: TEAMMATES_TABLE,
    name: "chat_teammates",
    key_column: "participant_id",
    groups: &[
        immutable(teammate_group::CREATED, &[timestamp("created_at")], &[]),
        surfaced(register(
            teammate_group::PROFILE,
            "profile",
            &[
                trimmed("role", MAX_ROLE_CHARS),
                text("instructions", MAX_INSTRUCTIONS_CHARS),
            ],
        )),
        coupled(
            teammate_group::POLICY,
            "policy",
            &[
                row_key("current_policy_revision_id"),
                optional_text("current_policy_safety_mode", MAX_ENUM_CHARS),
            ],
        ),
        register(
            teammate_group::HOME,
            "home",
            &[optional_text("home_device_id", MAX_DEVICE_ID_BYTES)],
        ),
        updated(teammate_group::UPDATED),
    ],
    resolutions: &[within(
        &["current_policy_revision_id", "current_policy_safety_mode"],
        teammate_group::POLICY,
    )],
    triggers: &[gated("chat_teammate_create_access_state")],
    ..BASE
};

/// `chat_teammate_policy_revisions`, immutable and never deleted.
pub const POLICY_REVISIONS: TableSpec = TableSpec {
    id: POLICY_REVISIONS_TABLE,
    name: "chat_teammate_policy_revisions",
    groups: &[immutable(
        policy_revision_group::CREATED,
        &[
            row_key("teammate_id"),
            text("provider_instance_id", MAX_PROVIDER_INSTANCE_CHARS),
            version("model_selection_schema_version"),
            text("model_selection_data", MAX_JSON_CHARS),
            optional_text("effort", MAX_ENUM_CHARS),
            optional_text("speed", MAX_ENUM_CHARS),
            version("provider_options_schema_version"),
            text("provider_options_data", MAX_JSON_CHARS),
            text("interaction_mode", MAX_ENUM_CHARS),
            timestamp("created_at"),
            text("safety_mode", MAX_ENUM_CHARS),
        ],
        &[cascade("teammate_id", TEAMMATES_TABLE)],
    )],
    triggers: &[
        applier_visible("chat_teammate_policy_identity_insert"),
        applier_visible("chat_teammate_policy_immutable_delete"),
        applier_visible("chat_teammate_policy_immutable_update"),
    ],
    ..BASE
};

/// `chat_access_profiles`, archive-only. Built-in profiles are seeded with fixed ids, so the
/// built-in key never collides across devices; concurrent edits of a custom profile keep the
/// revision with the lower folder capability.
pub const PROFILES: TableSpec = TableSpec {
    id: PROFILES_TABLE,
    name: "chat_access_profiles",
    groups: &[
        immutable(
            profile_group::CREATED,
            &[
                optional_text("builtin_key", MAX_ENUM_CHARS),
                timestamp("created_at"),
            ],
            &[],
        ),
        bumps(register(
            profile_group::NAME,
            "name",
            &[trimmed("display_name", MAX_DISPLAY_NAME_CHARS)],
        )),
        bumps(coupled(
            profile_group::CURRENT,
            "current",
            &[
                row_key("current_revision_id"),
                optional_text("current_maximum_folder_capability", MAX_ENUM_CHARS),
            ],
        )),
        bumps(register(
            profile_group::LIFECYCLE,
            "lifecycle",
            &[optional_timestamp("archived_at")],
        )),
        updated(profile_group::UPDATED),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    resolutions: &[
        Resolution::DuplicateRepair {
            columns: &["builtin_key"],
            collation: Collation::Binary,
        },
        within(
            &["current_revision_id", "current_maximum_folder_capability"],
            profile_group::CURRENT,
        ),
    ],
    redirects: true,
    triggers: &[
        applier_visible("chat_access_profile_builtin_protected"),
        gated("chat_access_profile_revision_counter_monotonic"),
    ],
    ..BASE
};

/// `chat_access_profile_revisions`, immutable and never deleted.
pub const PROFILE_REVISIONS: TableSpec = TableSpec {
    id: PROFILE_REVISIONS_TABLE,
    name: "chat_access_profile_revisions",
    groups: &[immutable(
        profile_revision_group::CREATED,
        &[
            row_key("access_profile_id"),
            flag("default_read_history"),
            flag("default_participate"),
            text("default_history_boundary", MAX_ENUM_CHARS),
            text("maximum_folder_capability", MAX_ENUM_CHARS),
            timestamp("created_at"),
        ],
        &[cascade("access_profile_id", PROFILES_TABLE)],
    )],
    triggers: &[
        applier_visible("chat_access_profile_builtin_revision_protected"),
        applier_visible("chat_access_profile_revision_immutable_delete"),
        applier_visible("chat_access_profile_revision_immutable_update"),
        gated("chat_access_profile_audience_publish"),
    ],
    ..BASE
};

/// A device table with its triggers.
const fn device(name: &'static str, triggers: &'static [DomainTrigger]) -> DeviceTable {
    DeviceTable {
        name,
        triggers,
        assets: &[],
    }
}

/// Chat domain. Chat writes stay owner-only until replicas can present its conflicts.
pub const DOMAIN: DomainManifest = DomainManifest {
    name: DOMAIN_NAME,
    table_ids: TableIdRange {
        first: 352,
        last: 399,
    },
    tables: &[
        PARTICIPANTS,
        TEAMMATES,
        POLICY_REVISIONS,
        PROFILES,
        PROFILE_REVISIONS,
    ],
    derived: &["chat_communication_search_fts"],
    local: &[
        device("chat_access_revocation_jobs", &[]),
        device("chat_activities", &[]),
        device(
            "chat_agent_runs",
            &[
                applier_visible("chat_agent_run_authority_immutable"),
                gated("chat_agent_run_authorization_insert"),
                gated("chat_agent_run_authorization_target_insert"),
                gated("chat_agent_run_execution_target_insert"),
                applier_visible("chat_agent_run_id_immutable"),
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
        device(
            "chat_attachment_references",
            &[gated("chat_attachment_messages_expose_resource")],
        ),
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
        device("chat_plans", &[]),
        device("chat_preview_tabs", &[]),
        device("chat_preview_tool_invocations", &[]),
        device("chat_provider_cleanup_jobs", &[]),
        device("chat_queued_attachment_references", &[]),
        device("chat_queued_followups", &[]),
        device("chat_resource_thread_references", &[]),
        device("chat_messages", &[]),
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
        device(
            "chat_teammate_access_state",
            &[applier_visible("chat_teammate_access_revision_monotonic")],
        ),
        device("chat_terminal_attachment_contexts", &[]),
        device("chat_terminal_layouts", &[]),
        device("chat_thread_relations", &[]),
        device("chat_user_input_drafts", &[]),
        device("chat_pending_requests", &[]),
        device("chat_worktrees", &[]),
        device("chat_checkpoints", &[]),
        device("chat_turns", &[]),
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
    replica_writable: false,
    assets: &[],
};
