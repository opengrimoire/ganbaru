//! Focus history and activity: table ids 224 to 271.
//!
//! A run replicates once it is closed, with its rhythm, segments, pauses, and run events, so a
//! remote run always arrives closed and an open run, its heartbeats, and its live phase changes
//! stay on the device that runs it. Every device writes one activity row naming what it is
//! focusing on, which other devices read to warn before a second start and to skip an occurrence
//! during automatic admission. Execution state, command receipts, and the adaptive model stay on
//! the device. Ids 228 to 271 are reserved.

use ganbaru_sync_contracts::bounds::MAX_ROW_KEY_BYTES;
use ganbaru_sync_contracts::{GroupId, TableId};

use crate::domains::focus::{MAX_ENUM_CHARS, MAX_RHYTHM_STEPS, MAX_SEGMENT_PAUSES, MAX_TEXT_CHARS};
use crate::manifest::vault::calendar::{ARCHIVES_TABLE, EVENTS_TABLE};
use crate::manifest::{
    ColumnSpec, ColumnType, DeviceTable, DomainManifest, DomainTrigger, GroupSpec, GroupStorage,
    MergeKind, OnDelete, OwnedBound, OwnedCodec, OwnedOrder, OwnedTable, PublishedWith,
    ReferenceColumn, TableIdRange, TableSpec, TriggerPolicy, group_id,
};

/// Domain name for Focus.
pub const DOMAIN_NAME: &str = "focus";

/// Domain adapter name for Focus.
pub const ADAPTER: &str = "focus";

/// `pomodoro_runs`.
pub const RUNS_TABLE: TableId = TableId(224);
/// `pomodoro_segments`.
pub const SEGMENTS_TABLE: TableId = TableId(225);
/// `pomodoro_run_events`.
pub const RUN_EVENTS_TABLE: TableId = TableId(226);
/// `pomodoro_device_activity`.
pub const DEVICE_ACTIVITY_TABLE: TableId = TableId(227);

/// Publication predicate of a run: closed runs replicate and are never reopened.
pub const RUN_PUBLISHED: &str = "ended_at IS NOT NULL";

/// Ordinal column of the one-row count rhythm table, always zero.
pub const RHYTHM_ORDINAL: &str = "ordinal";

/// Ordinal column of a segment's pauses, their zero-based position in start order.
pub const PAUSE_ORDINAL: &str = "ordinal";

/// Run field groups.
pub mod run_group {
    use super::{GroupId, group_id};

    /// The event, schedule, rhythm choice, and start trigger the run started with.
    pub const PLAN: GroupId = group_id(0);
    /// `started_at`, `ended_at`, `end_reason`, and `last_heartbeat`.
    pub const OUTCOME: GroupId = group_id(1);
    /// Focus and rhythm progress inherited from a previous run.
    pub const INHERITANCE: GroupId = group_id(2);
    /// `inherited_from_run_id`.
    pub const INHERITED_FROM: GroupId = group_id(3);
    /// Owned count rhythm and sequence steps.
    pub const RHYTHM: GroupId = group_id(4);
    /// The Calendar event, occurrence, and archive the run currently belongs to.
    pub const CALENDAR_LINK: GroupId = group_id(5);
}

/// Segment field groups.
pub mod segment_group {
    use super::{GroupId, group_id};

    /// `run_id`.
    pub const RUN: GroupId = group_id(0);
    /// The phase, its schedule, and its outcome.
    pub const PHASE: GroupId = group_id(1);
    /// `event_id`.
    pub const EVENT: GroupId = group_id(2);
    /// Owned pauses.
    pub const PAUSES: GroupId = group_id(3);
}

/// Run event field groups.
pub mod run_event_group {
    use super::{GroupId, group_id};

    /// `run_id`.
    pub const RUN: GroupId = group_id(0);
    /// `segment_id`.
    pub const SEGMENT: GroupId = group_id(1);
    /// The event type, time, and payload.
    pub const EVENT: GroupId = group_id(2);
}

/// Device activity field groups.
pub mod activity_group {
    use super::{GroupId, group_id};

    /// The device's state, run, occurrence, phase times, and freshness.
    pub const ACTIVITY: GroupId = group_id(0);
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

/// A group of owned tables with no columns of its own.
const fn owned(
    id: GroupId,
    name: &'static str,
    kind: MergeKind,
    tables: &'static [OwnedTable],
) -> GroupSpec {
    GroupSpec {
        storage: GroupStorage::Owned {
            columns: &[],
            tables,
        },
        ..group(id, name, kind, &[])
    }
}

/// The `run_id` column of a row published with its run.
const RUN_ID: &[ColumnSpec] = &[row_key("run_id")];

/// The reference to the run a child row is published with, written once and cascading.
const fn run_reference(id: GroupId) -> GroupSpec {
    GroupSpec {
        references: &[ReferenceColumn {
            column: "run_id",
            table: RUNS_TABLE,
            on_delete: OnDelete::Cascade,
        }],
        ..group(id, "run", MergeKind::Immutable, RUN_ID)
    }
}

/// A one-column reference group that clears when its row is deleted.
const fn clearing_reference(
    id: GroupId,
    name: &'static str,
    column: &'static [ColumnSpec],
    table: TableId,
) -> GroupSpec {
    group(
        id,
        name,
        MergeKind::Reference {
            table,
            on_delete: OnDelete::SetNull,
        },
        column,
    )
}

/// Unconstrained text.
const fn text(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Text {
            max_chars: MAX_TEXT_CHARS,
        },
    }
}

const fn optional_text(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalText {
            max_chars: MAX_TEXT_CHARS,
        },
    }
}

/// An enumerated value; the adapter checks the literal.
const fn enumerated(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Text {
            max_chars: MAX_ENUM_CHARS,
        },
    }
}

/// An optional enumerated value; the adapter checks the literal.
const fn optional_enumerated(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalText {
            max_chars: MAX_ENUM_CHARS,
        },
    }
}

const fn integer(name: &'static str, min: i64, max: i64) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Integer { min, max },
    }
}

const fn optional_integer(name: &'static str, min: i64, max: i64) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalInteger { min, max },
    }
}

/// An integer the schema does not constrain.
const fn any_integer(name: &'static str) -> ColumnSpec {
    integer(name, i64::MIN, i64::MAX)
}

/// A count of minutes that is at least one.
const fn minutes(name: &'static str) -> ColumnSpec {
    integer(name, 1, i64::MAX)
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

const fn gated(name: &'static str) -> DomainTrigger {
    DomainTrigger {
        name,
        policy: TriggerPolicy::Gated,
    }
}

/// An owned table of the generic codec.
const fn child(
    table: &'static str,
    parent_column: &'static str,
    ordinal: &'static str,
    columns: &'static [ColumnSpec],
    max_rows: usize,
    bound: OwnedBound,
) -> OwnedTable {
    OwnedTable {
        table,
        parent_column,
        order: OwnedOrder::Ordinal(ordinal),
        columns,
        references: &[],
        unique: &[],
        max_rows,
        codec: OwnedCodec::Generic,
        nested: &[],
        bound,
    }
}

/// `pomodoro_run_count_rhythms`, the one count rhythm of a count run at ordinal zero.
pub const COUNT_RHYTHMS: OwnedTable = child(
    "pomodoro_run_count_rhythms",
    "run_id",
    RHYTHM_ORDINAL,
    &[
        minutes("focus_duration_minutes"),
        minutes("short_break_minutes"),
        minutes("long_break_minutes"),
        integer("long_break_after_focus_count", 1, 12),
    ],
    1,
    OwnedBound::Static,
);

/// `pomodoro_run_sequence_steps`, the steps of a sequence run in order.
pub const SEQUENCE_STEPS: OwnedTable = child(
    "pomodoro_run_sequence_steps",
    "run_id",
    "step_index",
    &[
        minutes("focus_duration_minutes"),
        enumerated("break_phase"),
        minutes("break_duration_minutes"),
    ],
    MAX_RHYTHM_STEPS,
    OwnedBound::Static,
);

/// `pomodoro_pauses`, a segment's pauses in start order. Focus keeps at most
/// [`MAX_SEGMENT_PAUSES`] per segment, and its timestamps are short, so the domain keeps the
/// value within one blob field.
pub const PAUSES: OwnedTable = child(
    "pomodoro_pauses",
    "segment_id",
    PAUSE_ORDINAL,
    &[
        timestamp("started_at"),
        optional_timestamp("ended_at"),
        enumerated("reason"),
        optional_timestamp("detected_at"),
        timestamp("created_at"),
    ],
    MAX_SEGMENT_PAUSES,
    OwnedBound::Domain,
);

/// A table keyed by `id` with no derived columns, hooks, tree, or publication; each table
/// overrides what it declares.
const BASE: TableSpec = TableSpec {
    id: RUNS_TABLE,
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

/// `pomodoro_runs`, published once closed. The plan, outcome, and inheritance groups change only
/// while the run is open, so they are registers; once published, only Calendar writes the
/// Calendar link. `inherited_from_run_id` is a plain id because nothing may reference a
/// predicate table, and readers treat an unknown id as no predecessor. Gated triggers keep at
/// most one open run.
pub const RUNS: TableSpec = TableSpec {
    id: RUNS_TABLE,
    name: "pomodoro_runs",
    groups: &[
        group(
            run_group::PLAN,
            "plan",
            MergeKind::Register,
            &[
                text("original_event_id"),
                text("event_date"),
                timestamp("planned_start"),
                timestamp("planned_end"),
                enumerated("rhythm_kind"),
                enumerated("rhythm_source"),
                optional_enumerated("preset_key"),
                optional_integer("idle_timeout_minutes", i64::MIN, i64::MAX),
                optional_text("event_title_snapshot"),
                enumerated("start_trigger"),
                timestamp("created_at"),
            ],
        ),
        group(
            run_group::OUTCOME,
            "outcome",
            MergeKind::Register,
            &[
                timestamp("started_at"),
                optional_timestamp("ended_at"),
                optional_enumerated("end_reason"),
                timestamp("last_heartbeat"),
            ],
        ),
        group(
            run_group::INHERITANCE,
            "inheritance",
            MergeKind::Register,
            &[
                any_integer("inherited_focus_minutes"),
                any_integer("inherited_rhythm_position"),
                optional_integer("inherited_focus_milliseconds", 0, i64::MAX),
                optional_integer("inherited_phase_milliseconds", 0, i64::MAX),
            ],
        ),
        group(
            run_group::INHERITED_FROM,
            "inherited_from",
            MergeKind::Immutable,
            &[ColumnSpec {
                name: "inherited_from_run_id",
                ty: ColumnType::OptionalText {
                    max_chars: MAX_ROW_KEY_BYTES,
                },
            }],
        ),
        owned(
            run_group::RHYTHM,
            "rhythm",
            MergeKind::Immutable,
            &[COUNT_RHYTHMS, SEQUENCE_STEPS],
        ),
        GroupSpec {
            references: &[
                ReferenceColumn {
                    column: "event_id",
                    table: EVENTS_TABLE,
                    on_delete: OnDelete::SetNull,
                },
                ReferenceColumn {
                    column: "calendar_archive_id",
                    table: ARCHIVES_TABLE,
                    on_delete: OnDelete::SetNull,
                },
            ],
            ..group(
                run_group::CALENDAR_LINK,
                "calendar_link",
                MergeKind::Coupled,
                &[
                    row_key("event_id"),
                    optional_text("current_occurrence_id"),
                    optional_text("current_event_date"),
                    optional_text("current_event_title"),
                    row_key("calendar_archive_id"),
                ],
            )
        },
    ],
    triggers: &[
        gated("pomodoro_runs_single_open_insert"),
        gated("pomodoro_runs_single_open_update"),
    ],
    publish_when: Some(RUN_PUBLISHED),
    ..BASE
};

/// `pomodoro_segments`, published with their run. Segments stay rows because run events,
/// block events, and adaptive rows reference them. Gated triggers keep at most one active
/// segment and one open pause per segment.
pub const SEGMENTS: TableSpec = TableSpec {
    id: SEGMENTS_TABLE,
    name: "pomodoro_segments",
    groups: &[
        run_reference(segment_group::RUN),
        group(
            segment_group::PHASE,
            "phase",
            MergeKind::Register,
            &[
                integer("rhythm_position", 1, i64::MAX),
                enumerated("phase"),
                timestamp("planned_start"),
                timestamp("planned_end"),
                timestamp("actual_start"),
                optional_timestamp("actual_end"),
                enumerated("status"),
                optional_enumerated("end_reason"),
                text("event_date"),
                optional_integer("chosen_duration_ms", 1, i64::MAX),
                timestamp("created_at"),
            ],
        ),
        clearing_reference(
            segment_group::EVENT,
            "event",
            &[row_key("event_id")],
            EVENTS_TABLE,
        ),
        owned(
            segment_group::PAUSES,
            "pauses",
            MergeKind::Register,
            &[PAUSES],
        ),
    ],
    triggers: &[
        gated("pomodoro_segments_single_active_insert"),
        gated("pomodoro_segments_single_active_update"),
        gated("pomodoro_pauses_single_open_insert"),
        gated("pomodoro_pauses_single_open_update"),
    ],
    published_with: Some(PublishedWith {
        column: "run_id",
        parent: RUNS_TABLE,
        null_publishes: false,
    }),
    ..BASE
};

/// `pomodoro_run_events`, published with their run and written once.
pub const RUN_EVENTS: TableSpec = TableSpec {
    id: RUN_EVENTS_TABLE,
    name: "pomodoro_run_events",
    groups: &[
        run_reference(run_event_group::RUN),
        clearing_reference(
            run_event_group::SEGMENT,
            "segment",
            &[row_key("segment_id")],
            SEGMENTS_TABLE,
        ),
        group(
            run_event_group::EVENT,
            "event",
            MergeKind::Immutable,
            &[
                enumerated("event_type"),
                timestamp("occurred_at"),
                optional_enumerated("phase"),
                optional_text("reason"),
                optional_integer("duration_seconds", i64::MIN, i64::MAX),
                timestamp("created_at"),
            ],
        ),
    ],
    published_with: Some(PublishedWith {
        column: "run_id",
        parent: RUNS_TABLE,
        null_publishes: false,
    }),
    ..BASE
};

/// `pomodoro_device_activity`, keyed by the stable device id. Only the device a row names writes
/// it, at each run transition and never on heartbeat; a row past `expires_at_ms` means the
/// device was last seen then. `run_id` is plain text because the run it names is still open and
/// unpublished.
pub const DEVICE_ACTIVITY: TableSpec = TableSpec {
    id: DEVICE_ACTIVITY_TABLE,
    name: "pomodoro_device_activity",
    key_column: "device_id",
    groups: &[group(
        activity_group::ACTIVITY,
        "activity",
        MergeKind::Coupled,
        &[
            enumerated("state"),
            ColumnSpec {
                name: "run_id",
                ty: ColumnType::OptionalText {
                    max_chars: MAX_ROW_KEY_BYTES,
                },
            },
            optional_text("occurrence_id"),
            optional_text("event_title"),
            optional_integer("phase_started_at_ms", 0, i64::MAX),
            optional_integer("phase_ends_at_ms", 0, i64::MAX),
            integer("expires_at_ms", 0, i64::MAX),
            integer("updated_at_ms", 0, i64::MAX),
        ],
    )],
    ..BASE
};

/// A device table without domain triggers.
const fn device(name: &'static str) -> DeviceTable {
    DeviceTable {
        name,
        triggers: &[],
    }
}

/// The Focus history and activity domain. Its device tables are the execution state, command
/// receipts, and every adaptive table, which each device learns from its own runs.
pub const DOMAIN: DomainManifest = DomainManifest {
    name: DOMAIN_NAME,
    table_ids: TableIdRange {
        first: 224,
        last: 271,
    },
    tables: &[RUNS, SEGMENTS, RUN_EVENTS, DEVICE_ACTIVITY],
    derived: &[],
    local: &[
        device("pomodoro_execution_state"),
        device("pomodoro_execution_receipts"),
        device("pomodoro_run_adaptive_snapshots"),
        device("pomodoro_adaptive_planned_blocks"),
        device("pomodoro_adaptive_policies"),
        device("pomodoro_adaptive_policy_bounds"),
        device("pomodoro_adaptive_context_states"),
        device("pomodoro_adaptive_context_state_history"),
        device("pomodoro_adaptive_context_snapshots"),
        device("pomodoro_adaptive_context_snapshot_features"),
        device("pomodoro_adaptive_data_quality_flags"),
        device("pomodoro_adaptive_decisions"),
        device("pomodoro_adaptive_decision_reasons"),
        device("pomodoro_adaptive_decision_state_scores"),
        device("pomodoro_adaptive_decision_values"),
        device("pomodoro_adaptive_experiments"),
        device("pomodoro_adaptive_experiment_variants"),
        device("pomodoro_adaptive_assignments"),
        device("pomodoro_adaptive_outcomes"),
    ],
    engine: &[],
    replica_writable: true,
    assets: &[],
};
