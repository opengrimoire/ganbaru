//! Calendar: table ids 160 to 223.
//!
//! Calendars, preserved iCalendar objects and components, events, deleted-event archives, the
//! iCalendar objects an archive keeps, and project task event links replicate. Every child table
//! of an event, an archive, an object, or a component is owned and keyed by a data column, or by
//! an ordinal fixed at zero for the one-row tables, so writers keep their natural uniqueness and
//! a list with the same rows always encodes the same way. Owned values are bounded by the
//! Calendar sync value bound instead of a static worst case. A calendar's `visible` flag and
//! `last_synced` time are `Local` columns, and edit receipts stay on the device. Archive import
//! objects and task event links have a composite primary key and are keyed by `sync_key`.
//! Ids 167 to 223 are reserved.

use ganbaru_sync_contracts::{GroupId, TableId};

use crate::domains::calendar::{
    MAX_ENUM_CHARS, MAX_FOCUS_STEPS, MAX_GEO_LATITUDE, MAX_GEO_LONGITUDE, MAX_MUSIC_ASSIGNMENTS,
    MAX_OWNED_ROWS, MAX_TEXT_CHARS,
};
use crate::manifest::vault::projects::{PROJECTS_TABLE, TASKS_TABLE};
use crate::manifest::{
    Collation, ColumnSpec, ColumnType, DerivedKey, DeviceTable, DomainManifest, DomainTrigger,
    GroupSpec, GroupStorage, LocalColumn, MergeKind, OnDelete, OwnedBound, OwnedCodec, OwnedOrder,
    OwnedTable, ReferenceColumn, Resolution, SYNC_KEY_COLUMN, TableIdRange, TableSpec,
    TriggerPolicy, group_id,
};

/// Domain name for Calendar.
pub const DOMAIN_NAME: &str = "calendar";

/// Domain adapter name for Calendar.
pub const ADAPTER: &str = "calendar";

/// `calendars`.
pub const CALENDARS_TABLE: TableId = TableId(160);
/// `icalendar_objects`.
pub const OBJECTS_TABLE: TableId = TableId(161);
/// `icalendar_components`.
pub const COMPONENTS_TABLE: TableId = TableId(162);
/// `calendar_events`.
pub const EVENTS_TABLE: TableId = TableId(163);
/// `calendar_event_archives`.
pub const ARCHIVES_TABLE: TableId = TableId(164);
/// `calendar_event_archive_import_objects`.
pub const ARCHIVE_OBJECTS_TABLE: TableId = TableId(165);
/// `project_task_event_links`.
pub const TASK_EVENT_LINKS_TABLE: TableId = TableId(166);

/// Prefix of archive import object keys, over the archive and the object.
pub const ARCHIVE_OBJECT_KEY_PREFIX: &str = "archive-import-object";
/// Prefix of task event link keys, over the task and the event.
pub const TASK_EVENT_KEY_PREFIX: &str = "task-event";

/// Calendar field groups.
pub mod calendar_group {
    use super::{GroupId, group_id};

    /// `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `color`.
    pub const COLOR: GroupId = group_id(2);
    /// `source`, `read_only`, and `source_url`.
    pub const SOURCE: GroupId = group_id(3);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(4);
}

/// iCalendar object field groups.
pub mod object_group {
    use super::{GroupId, group_id};

    /// The import and the source the object was read from.
    pub const SOURCE: GroupId = group_id(0);
    /// Owned diagnostics.
    pub const DIAGNOSTICS: GroupId = group_id(1);
    /// `calendar_id`.
    pub const CALENDAR: GroupId = group_id(2);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(3);
}

/// iCalendar component field groups.
pub mod component_group {
    use super::{GroupId, group_id};

    /// `object_id`.
    pub const OBJECT: GroupId = group_id(0);
    /// `parent_component_id`.
    pub const PARENT: GroupId = group_id(1);
    /// The component's identity and preservation status.
    pub const COMPONENT: GroupId = group_id(2);
    /// Owned warnings, properties, parameters, and value nodes.
    pub const GRAPH: GroupId = group_id(3);
    /// `calendar_id`.
    pub const CALENDAR: GroupId = group_id(4);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(5);
}

/// Event field groups. Id 27 is unused: events are tombstoned, so no existence group exists.
pub mod event_group {
    use super::{GroupId, group_id};

    /// `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `source_uid`.
    pub const SOURCE: GroupId = group_id(1);
    /// `icalendar_component_id`.
    pub const COMPONENT: GroupId = group_id(2);
    /// `title`.
    pub const TITLE: GroupId = group_id(3);
    /// `description`.
    pub const DESCRIPTION: GroupId = group_id(4);
    /// Start, end, zone, recurrence, and the owned excluded dates, added dates, overrides, and
    /// override properties.
    pub const TIMING: GroupId = group_id(5);
    /// `calendar_id`.
    pub const CALENDAR: GroupId = group_id(6);
    /// `project_id`.
    pub const PROJECT: GroupId = group_id(7);
    /// `location`, `geo_lat`, and `geo_lng`.
    pub const PLACE: GroupId = group_id(8);
    /// `color`.
    pub const COLOR: GroupId = group_id(9);
    /// `transparency` and `status`.
    pub const AVAILABILITY: GroupId = group_id(10);
    /// `visibility`.
    pub const PRIVACY: GroupId = group_id(11);
    /// `priority`.
    pub const PRIORITY: GroupId = group_id(12);
    /// `url` and `meeting_enabled`.
    pub const LINK: GroupId = group_id(13);
    /// The three guest permissions.
    pub const GUESTS: GroupId = group_id(14);
    /// `local_rsvp_status`.
    pub const RSVP: GroupId = group_id(15);
    /// `sequence`.
    pub const SEQUENCE: GroupId = group_id(16);
    /// `playlist_id`.
    pub const MUSIC: GroupId = group_id(17);
    /// `environment_id`.
    pub const ENVIRONMENT: GroupId = group_id(18);
    /// Owned alarms.
    pub const ALARMS: GroupId = group_id(19);
    /// Owned attendees.
    pub const ATTENDEES: GroupId = group_id(20);
    /// Owned organizer.
    pub const ORGANIZER: GroupId = group_id(21);
    /// Owned categories.
    pub const CATEGORIES: GroupId = group_id(22);
    /// Owned notifications.
    pub const NOTIFICATIONS: GroupId = group_id(23);
    /// Owned extended properties.
    pub const PROPERTIES: GroupId = group_id(24);
    /// Owned Focus configuration, count rhythm, and sequence steps.
    pub const FOCUS: GroupId = group_id(25);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(26);
}

/// Archive field groups, all written once.
pub mod archive_group {
    use super::{GroupId, group_id};

    /// The deleted event's identity, links, and times.
    pub const EVENT: GroupId = group_id(0);
    /// Title, start, end, zone, and recurrence.
    pub const TIMING: GroupId = group_id(1);
    /// The remaining event details.
    pub const DETAILS: GroupId = group_id(2);
    /// The three guest permissions.
    pub const GUESTS: GroupId = group_id(3);
    /// Owned alarms.
    pub const ALARMS: GroupId = group_id(4);
    /// Owned attendees.
    pub const ATTENDEES: GroupId = group_id(5);
    /// Owned categories.
    pub const CATEGORIES: GroupId = group_id(6);
    /// Owned excluded dates.
    pub const EXDATES: GroupId = group_id(7);
    /// Owned extended properties.
    pub const PROPERTIES: GroupId = group_id(8);
    /// Owned music assignments.
    pub const MUSIC: GroupId = group_id(9);
    /// Owned notifications.
    pub const NOTIFICATIONS: GroupId = group_id(10);
    /// Owned organizer.
    pub const ORGANIZER: GroupId = group_id(11);
    /// Owned overrides and override properties.
    pub const OVERRIDES: GroupId = group_id(12);
    /// Owned Focus configuration, count rhythm, and sequence steps.
    pub const FOCUS: GroupId = group_id(13);
    /// Owned added dates.
    pub const RDATES: GroupId = group_id(14);
    /// Owned task links.
    pub const TASK_LINKS: GroupId = group_id(15);
}

/// Archive import object field groups.
pub mod archive_object_group {
    use super::{GroupId, group_id};

    /// `archive_event_id` and `object_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// Existence.
    pub const EXISTS: GroupId = group_id(1);
}

/// Task event link field groups.
pub mod task_event_link_group {
    use super::{GroupId, group_id};

    /// `task_id` and `event_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `created_at`.
    pub const CREATED: GroupId = group_id(1);
    /// `link_kind`.
    pub const KIND: GroupId = group_id(2);
    /// Existence.
    pub const EXISTS: GroupId = group_id(3);
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

/// `spec`, offered in recovery entries when its row is deleted while it changes.
const fn recoverable(spec: GroupSpec) -> GroupSpec {
    GroupSpec {
        recoverable: true,
        ..spec
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

/// A recoverable `Register` group.
const fn register(id: GroupId, name: &'static str, columns: &'static [ColumnSpec]) -> GroupSpec {
    recoverable(group(id, name, MergeKind::Register, columns))
}

/// A recoverable `Register` group of owned tables.
const fn owned_register(
    id: GroupId,
    name: &'static str,
    tables: &'static [OwnedTable],
) -> GroupSpec {
    recoverable(owned(id, name, MergeKind::Register, tables))
}

/// An `Immutable` group of owned tables.
const fn owned_immutable(
    id: GroupId,
    name: &'static str,
    tables: &'static [OwnedTable],
) -> GroupSpec {
    owned(id, name, MergeKind::Immutable, tables)
}

/// An `Immutable` group whose reference columns cascade from their rows.
const fn immutable(
    id: GroupId,
    name: &'static str,
    columns: &'static [ColumnSpec],
    references: &'static [ReferenceColumn],
) -> GroupSpec {
    GroupSpec {
        references,
        ..group(id, name, MergeKind::Immutable, columns)
    }
}

/// The `updated` group of a table with `updated_at`.
const fn updated(id: GroupId) -> GroupSpec {
    group(id, "updated", MergeKind::Max, &[UPDATED_AT])
}

/// The `created` group of a table with `created_at`.
const fn created(id: GroupId) -> GroupSpec {
    group(id, "created", MergeKind::Immutable, &[CREATED_AT])
}

/// The existence register of a derived-key table.
const fn exists(id: GroupId) -> GroupSpec {
    group(id, "exists", MergeKind::Register, &[])
}

/// A recoverable one-column reference group.
const fn reference(
    id: GroupId,
    name: &'static str,
    column: &'static [ColumnSpec],
    table: TableId,
    on_delete: OnDelete,
) -> GroupSpec {
    recoverable(group(
        id,
        name,
        MergeKind::Reference { table, on_delete },
        column,
    ))
}

const fn cascade(column: &'static str, table: TableId) -> ReferenceColumn {
    ReferenceColumn {
        column,
        table,
        on_delete: OnDelete::Cascade,
    }
}

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

/// Non-blank text.
const fn trimmed(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::TrimmedText {
            min_chars: 1,
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

const fn row_key(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalRowKey,
    }
}

/// A position among an owner's rows, as readers order them.
const fn sort_order() -> ColumnSpec {
    integer("sort_order", 0, i64::MAX)
}

/// An event palette color index.
const fn color() -> ColumnSpec {
    optional_integer("color", 0, 31)
}

/// A count of minutes that is at least one.
const fn minutes(name: &'static str) -> ColumnSpec {
    integer(name, 1, i64::MAX)
}

const fn local(name: &'static str, materialize: &'static str) -> LocalColumn {
    LocalColumn { name, materialize }
}

const fn applier_visible(name: &'static str) -> DomainTrigger {
    DomainTrigger {
        name,
        policy: TriggerPolicy::ApplierVisible,
    }
}

const fn within(columns: &'static [&'static str], group: GroupId) -> Resolution {
    Resolution::WithinGroup { columns, group }
}

/// An owned table of the Calendar codec, bounded by the Calendar sync value bound.
const fn child(
    table: &'static str,
    parent_column: &'static str,
    order: OwnedOrder,
    columns: &'static [ColumnSpec],
    max_rows: usize,
) -> OwnedTable {
    OwnedTable {
        table,
        parent_column,
        order,
        columns,
        references: &[],
        unique: &[],
        max_rows,
        codec: OwnedCodec::Custom,
        nested: &[],
        bound: OwnedBound::Domain,
    }
}

/// An owned table keyed by its `id` column.
const fn by_id(
    table: &'static str,
    parent_column: &'static str,
    columns: &'static [ColumnSpec],
) -> OwnedTable {
    child(
        table,
        parent_column,
        OwnedOrder::Key(&["id"]),
        columns,
        MAX_OWNED_ROWS,
    )
}

/// An owned table holding at most one row, at ordinal zero.
const fn single(
    table: &'static str,
    parent_column: &'static str,
    columns: &'static [ColumnSpec],
) -> OwnedTable {
    child(
        table,
        parent_column,
        OwnedOrder::Ordinal(SINGLE_ORDINAL),
        columns,
        1,
    )
}

/// Ordinal column of the one-row owned tables, always zero.
pub const SINGLE_ORDINAL: &str = "ordinal";

/// The `created_at` column of a `created` group.
const CREATED_AT: ColumnSpec = timestamp("created_at");

/// The `updated_at` column of an `updated` group.
const UPDATED_AT: ColumnSpec = timestamp("updated_at");

/// A table keyed by `id` with no local, derived, or hook columns; each table overrides what it
/// declares.
const BASE: TableSpec = TableSpec {
    id: CALENDARS_TABLE,
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
    holds_deletes: false,
};

/// `calendars`. Removal is a tombstone without redirect.
pub const CALENDARS: TableSpec = TableSpec {
    id: CALENDARS_TABLE,
    name: "calendars",
    groups: &[
        created(calendar_group::CREATED),
        register(calendar_group::NAME, "name", &[trimmed("name")]),
        register(calendar_group::COLOR, "color", &[text("color")]),
        recoverable(group(
            calendar_group::SOURCE,
            "source",
            MergeKind::Coupled,
            &[
                enumerated("source"),
                flag("read_only"),
                optional_text("source_url"),
            ],
        )),
        updated(calendar_group::UPDATED),
    ],
    local_columns: &[local("visible", "1"), local("last_synced", "NULL")],
    ..BASE
};

/// `icalendar_object_diagnostics`.
pub const OBJECT_DIAGNOSTICS: OwnedTable = by_id(
    "icalendar_object_diagnostics",
    "object_id",
    &[trimmed("id"), text("message"), sort_order()],
);

/// `icalendar_objects`. Preserved source data, written once per import.
pub const OBJECTS: TableSpec = TableSpec {
    id: OBJECTS_TABLE,
    name: "icalendar_objects",
    groups: &[
        group(
            object_group::SOURCE,
            "source",
            MergeKind::Immutable,
            &[
                trimmed("import_id"),
                enumerated("source_kind"),
                text("source_name"),
                text("source_fingerprint"),
                optional_text("prodid"),
                optional_text("version"),
                optional_text("method"),
                optional_text("calendar_scale"),
                CREATED_AT,
            ],
        ),
        owned_immutable(
            object_group::DIAGNOSTICS,
            "diagnostics",
            &[OBJECT_DIAGNOSTICS],
        ),
        reference(
            object_group::CALENDAR,
            "calendar",
            &[row_key("calendar_id")],
            CALENDARS_TABLE,
            OnDelete::Cascade,
        ),
        updated(object_group::UPDATED),
    ],
    ..BASE
};

/// `icalendar_component_projection_warnings`.
pub const COMPONENT_WARNINGS: OwnedTable = by_id(
    "icalendar_component_projection_warnings",
    "component_id",
    &[trimmed("id"), text("message"), sort_order()],
);

/// `icalendar_component_properties`.
pub const COMPONENT_PROPERTIES: OwnedTable = by_id(
    "icalendar_component_properties",
    "component_id",
    &[
        trimmed("id"),
        text("name"),
        text("value_type"),
        sort_order(),
    ],
);

/// `icalendar_property_parameters`, flat under the component and linked to their property by
/// `property_id`.
pub const PROPERTY_PARAMETERS: OwnedTable = by_id(
    "icalendar_property_parameters",
    "component_id",
    &[
        trimmed("id"),
        trimmed("property_id"),
        text("name"),
        sort_order(),
    ],
);

/// `icalendar_value_nodes`, flat under the component and linked to their property, parameter,
/// and parent node by id. `number_value` holds the JSON number literal as parsed.
pub const VALUE_NODES: OwnedTable = by_id(
    "icalendar_value_nodes",
    "component_id",
    &[
        trimmed("id"),
        trimmed("property_id"),
        optional_text("parameter_id"),
        optional_text("parent_node_id"),
        sort_order(),
        enumerated("value_kind"),
        optional_text("object_key"),
        optional_text("text_value"),
        optional_text("number_value"),
        optional_integer("boolean_value", i64::MIN, i64::MAX),
    ],
);

/// `icalendar_components`. One component with its property graph is one immutable value;
/// `projected_kind` and `projected_id` are kept by Calendar from the events and overrides that
/// name the component.
pub const COMPONENTS: TableSpec = TableSpec {
    id: COMPONENTS_TABLE,
    name: "icalendar_components",
    groups: &[
        immutable(
            component_group::OBJECT,
            "object",
            &[row_key("object_id")],
            &[cascade("object_id", OBJECTS_TABLE)],
        ),
        immutable(
            component_group::PARENT,
            "parent",
            &[row_key("parent_component_id")],
            &[cascade("parent_component_id", COMPONENTS_TABLE)],
        ),
        group(
            component_group::COMPONENT,
            "component",
            MergeKind::Immutable,
            &[
                text("component_type"),
                optional_text("uid"),
                optional_text("recurrence_id"),
                optional_text("recurrence_id_value_type"),
                optional_integer("sequence", i64::MIN, i64::MAX),
                optional_text("dtstart_key"),
                enumerated("preservation_status"),
                integer("sort_order", i64::MIN, i64::MAX),
                CREATED_AT,
            ],
        ),
        owned_immutable(
            component_group::GRAPH,
            "graph",
            &[
                COMPONENT_WARNINGS,
                COMPONENT_PROPERTIES,
                PROPERTY_PARAMETERS,
                VALUE_NODES,
            ],
        ),
        reference(
            component_group::CALENDAR,
            "calendar",
            &[row_key("calendar_id")],
            CALENDARS_TABLE,
            OnDelete::Cascade,
        ),
        updated(component_group::UPDATED),
    ],
    hook_columns: &["projected_kind", "projected_id"],
    ..BASE
};

/// `calendar_event_exdates`, keyed by the excluded date.
pub const EXDATES: OwnedTable = child(
    "calendar_event_exdates",
    "event_id",
    OwnedOrder::Key(&["occurrence_date"]),
    &[trimmed("occurrence_date"), trimmed("id")],
    MAX_OWNED_ROWS,
);

/// `calendar_event_rdates`, keyed by the added start.
pub const RDATES: OwnedTable = child(
    "calendar_event_rdates",
    "event_id",
    OwnedOrder::Key(&["occurrence_start"]),
    &[trimmed("occurrence_start"), trimmed("id")],
    MAX_OWNED_ROWS,
);

/// The columns an override carries besides its key, shared by events and archives.
macro_rules! override_columns {
    ($($leading:expr),* $(,)?) => {
        &[
            $($leading,)*
            trimmed("recurrence_id"),
            optional_text("title"),
            optional_text("start_time"),
            optional_text("end_time"),
            optional_text("description"),
            optional_text("location"),
            optional_text("url"),
            color(),
            optional_enumerated("status"),
            optional_enumerated("transparency"),
            optional_enumerated("visibility"),
            CREATED_AT,
            UPDATED_AT,
            optional_text("icalendar_component_id"),
            optional_enumerated("recurrence_range"),
        ]
    };
}

/// `calendar_event_overrides`, keyed by the recurrence id.
pub const OVERRIDES: OwnedTable = child(
    "calendar_event_overrides",
    "parent_event_id",
    OwnedOrder::Key(&["recurrence_id"]),
    override_columns!(trimmed("id")),
    MAX_OWNED_ROWS,
);

/// `calendar_event_override_extended_properties`, flat under the event and linked to their
/// override by `override_id`.
pub const OVERRIDE_PROPERTIES: OwnedTable = child(
    "calendar_event_override_extended_properties",
    "event_id",
    OwnedOrder::Key(&["override_id", "property_key"]),
    &[
        trimmed("override_id"),
        trimmed("property_key"),
        trimmed("id"),
        text("property_value"),
        sort_order(),
    ],
    MAX_OWNED_ROWS,
);

/// The alarm columns besides the archive's source id.
macro_rules! alarm_columns {
    ($($leading:expr),* $(,)?) => {
        &[
            trimmed("id"),
            $($leading,)*
            enumerated("action"),
            enumerated("trigger_type"),
            trimmed("trigger_value"),
            optional_text("description"),
            sort_order(),
            optional_text("icalendar_component_id"),
        ]
    };
}

/// The attendee columns besides the archive's source id.
macro_rules! attendee_columns {
    ($($leading:expr),* $(,)?) => {
        &[
            trimmed("id"),
            $($leading,)*
            optional_text("name"),
            trimmed("email"),
            enumerated("role"),
            enumerated("status"),
            flag("rsvp"),
            sort_order(),
            optional_text("icalendar_component_id"),
            optional_integer("icalendar_property_index", 0, i64::MAX),
        ]
    };
}

/// `calendar_event_alarms`.
pub const ALARMS: OwnedTable = by_id("calendar_event_alarms", "event_id", alarm_columns!());

/// `calendar_event_attendees`.
pub const ATTENDEES: OwnedTable =
    by_id("calendar_event_attendees", "event_id", attendee_columns!());

/// The organizer columns.
const ORGANIZER_COLUMNS: &[ColumnSpec] = &[optional_text("name"), trimmed("email")];

/// `calendar_event_organizers`.
pub const ORGANIZERS: OwnedTable =
    single("calendar_event_organizers", "event_id", ORGANIZER_COLUMNS);

/// The category columns.
const CATEGORY_COLUMNS: &[ColumnSpec] = &[trimmed("id"), trimmed("category"), sort_order()];

/// `calendar_event_categories`.
pub const CATEGORIES: OwnedTable = by_id("calendar_event_categories", "event_id", CATEGORY_COLUMNS);

/// The notification columns.
const NOTIFICATION_COLUMNS: &[ColumnSpec] = &[
    trimmed("id"),
    integer("offset_minutes", i64::MIN, i64::MAX),
    sort_order(),
];

/// `calendar_event_notifications`.
pub const NOTIFICATIONS: OwnedTable = by_id(
    "calendar_event_notifications",
    "event_id",
    NOTIFICATION_COLUMNS,
);

/// `calendar_event_extended_properties`, keyed by the property key.
pub const PROPERTIES: OwnedTable = child(
    "calendar_event_extended_properties",
    "event_id",
    OwnedOrder::Key(&["property_key"]),
    &[
        trimmed("property_key"),
        trimmed("id"),
        text("property_value"),
        sort_order(),
    ],
    MAX_OWNED_ROWS,
);

/// The Focus configuration columns.
const FOCUS_CONFIG_COLUMNS: &[ColumnSpec] = &[
    enumerated("rhythm_kind"),
    enumerated("rhythm_source"),
    optional_enumerated("preset_key"),
    optional_integer("idle_timeout_minutes", 1, i64::MAX),
];

/// The count rhythm columns.
const COUNT_RHYTHM_COLUMNS: &[ColumnSpec] = &[
    minutes("focus_duration_minutes"),
    minutes("short_break_minutes"),
    minutes("long_break_minutes"),
    integer("long_break_after_focus_count", 1, 12),
];

/// The sequence step columns.
const SEQUENCE_STEP_COLUMNS: &[ColumnSpec] = &[
    integer("step_index", 0, 11),
    minutes("focus_duration_minutes"),
    enumerated("break_phase"),
    minutes("break_duration_minutes"),
];

/// `calendar_event_pomodoro_configs`.
pub const FOCUS_CONFIGS: OwnedTable = single(
    "calendar_event_pomodoro_configs",
    "event_id",
    FOCUS_CONFIG_COLUMNS,
);

/// `calendar_event_pomodoro_config_count_rhythms`, flat under the event.
pub const COUNT_RHYTHMS: OwnedTable = single(
    "calendar_event_pomodoro_config_count_rhythms",
    "event_id",
    COUNT_RHYTHM_COLUMNS,
);

/// `calendar_event_pomodoro_config_sequence_steps`, flat under the event and keyed by the step.
pub const SEQUENCE_STEPS: OwnedTable = child(
    "calendar_event_pomodoro_config_sequence_steps",
    "event_id",
    OwnedOrder::Key(&["step_index"]),
    SEQUENCE_STEP_COLUMNS,
    MAX_FOCUS_STEPS,
);

/// `calendar_events`. Events are tombstoned, so deleted events reach recovery; a duplicate
/// source uid in one calendar keeps the lowest key.
pub const EVENTS: TableSpec = TableSpec {
    id: EVENTS_TABLE,
    name: "calendar_events",
    groups: &[
        created(event_group::CREATED),
        register(
            event_group::SOURCE,
            "source",
            &[optional_text("source_uid")],
        ),
        reference(
            event_group::COMPONENT,
            "component",
            &[row_key("icalendar_component_id")],
            COMPONENTS_TABLE,
            OnDelete::SetNull,
        ),
        surfaced(group(
            event_group::TITLE,
            "title",
            MergeKind::Register,
            &[text("title")],
        )),
        surfaced(group(
            event_group::DESCRIPTION,
            "description",
            MergeKind::Register,
            &[text("description")],
        )),
        surfaced(GroupSpec {
            storage: GroupStorage::Owned {
                columns: &[
                    trimmed("start_time"),
                    trimmed("end_time"),
                    trimmed("timezone"),
                    flag("all_day"),
                    optional_text("rrule"),
                    optional_text("repeat_until"),
                ],
                tables: &[EXDATES, RDATES, OVERRIDES, OVERRIDE_PROPERTIES],
            },
            ..group(event_group::TIMING, "timing", MergeKind::Coupled, &[])
        }),
        reference(
            event_group::CALENDAR,
            "calendar",
            &[row_key("calendar_id")],
            CALENDARS_TABLE,
            OnDelete::Cascade,
        ),
        reference(
            event_group::PROJECT,
            "project",
            &[row_key("project_id")],
            PROJECTS_TABLE,
            OnDelete::SetNull,
        ),
        recoverable(group(
            event_group::PLACE,
            "place",
            MergeKind::Coupled,
            &[
                text("location"),
                optional_integer("geo_lat", -MAX_GEO_LATITUDE, MAX_GEO_LATITUDE),
                optional_integer("geo_lng", -MAX_GEO_LONGITUDE, MAX_GEO_LONGITUDE),
            ],
        )),
        register(event_group::COLOR, "color", &[color()]),
        register(
            event_group::AVAILABILITY,
            "availability",
            &[enumerated("transparency"), enumerated("status")],
        ),
        register(event_group::PRIVACY, "privacy", &[enumerated("visibility")]),
        register(
            event_group::PRIORITY,
            "priority",
            &[optional_integer("priority", 0, 9)],
        ),
        register(
            event_group::LINK,
            "link",
            &[text("url"), flag("meeting_enabled")],
        ),
        register(
            event_group::GUESTS,
            "guests",
            &[
                flag("guest_can_modify"),
                flag("guest_can_invite_others"),
                flag("guest_can_see_other_guests"),
            ],
        ),
        register(
            event_group::RSVP,
            "rsvp",
            &[optional_enumerated("local_rsvp_status")],
        ),
        group(
            event_group::SEQUENCE,
            "sequence",
            MergeKind::Max,
            &[integer("sequence", 0, i64::MAX)],
        ),
        register(event_group::MUSIC, "music", &[optional_text("playlist_id")]),
        register(
            event_group::ENVIRONMENT,
            "environment",
            &[optional_text("environment_id")],
        ),
        owned_register(event_group::ALARMS, "alarms", &[ALARMS]),
        owned_register(event_group::ATTENDEES, "attendees", &[ATTENDEES]),
        owned_register(event_group::ORGANIZER, "organizer", &[ORGANIZERS]),
        owned_register(event_group::CATEGORIES, "categories", &[CATEGORIES]),
        owned_register(
            event_group::NOTIFICATIONS,
            "notifications",
            &[NOTIFICATIONS],
        ),
        owned_register(event_group::PROPERTIES, "properties", &[PROPERTIES]),
        owned_register(
            event_group::FOCUS,
            "focus",
            &[FOCUS_CONFIGS, COUNT_RHYTHMS, SEQUENCE_STEPS],
        ),
        updated(event_group::UPDATED),
    ],
    resolutions: &[
        Resolution::DuplicateRepair {
            columns: &["calendar_id", "source_uid"],
            collation: Collation::Binary,
        },
        within(&["geo_lat", "geo_lng"], event_group::PLACE),
    ],
    redirects: true,
    holds_deletes: true,
    ..BASE
};

/// `calendar_event_archive_alarms`.
pub const ARCHIVE_ALARMS: OwnedTable = by_id(
    "calendar_event_archive_alarms",
    "archive_event_id",
    alarm_columns!(trimmed("source_alarm_id")),
);

/// `calendar_event_archive_attendees`.
pub const ARCHIVE_ATTENDEES: OwnedTable = by_id(
    "calendar_event_archive_attendees",
    "archive_event_id",
    attendee_columns!(trimmed("source_attendee_id")),
);

/// `calendar_event_archive_categories`.
pub const ARCHIVE_CATEGORIES: OwnedTable = by_id(
    "calendar_event_archive_categories",
    "archive_event_id",
    CATEGORY_COLUMNS,
);

/// `calendar_event_archive_exdates`.
pub const ARCHIVE_EXDATES: OwnedTable = by_id(
    "calendar_event_archive_exdates",
    "archive_event_id",
    &[trimmed("id"), trimmed("occurrence_date"), sort_order()],
);

/// `calendar_event_archive_extended_properties`.
pub const ARCHIVE_PROPERTIES: OwnedTable = by_id(
    "calendar_event_archive_extended_properties",
    "archive_event_id",
    &[
        trimmed("id"),
        trimmed("property_key"),
        text("property_value"),
        sort_order(),
    ],
);

/// `calendar_event_archive_music_assignments`, keyed by owner kind and phase.
pub const ARCHIVE_MUSIC: OwnedTable = child(
    "calendar_event_archive_music_assignments",
    "archive_event_id",
    OwnedOrder::Key(&["owner_kind", "phase"]),
    &[
        enumerated("owner_kind"),
        enumerated("phase"),
        enumerated("behavior"),
        optional_text("playlist_id"),
        enumerated("provenance_kind"),
        optional_text("provenance_id"),
        integer("updated_at_ms", 1, i64::MAX),
        integer("version", 1, i64::MAX),
        enumerated("soundscape_behavior"),
        optional_text("soundscape_preset_id"),
    ],
    MAX_MUSIC_ASSIGNMENTS,
);

/// `calendar_event_archive_notifications`.
pub const ARCHIVE_NOTIFICATIONS: OwnedTable = by_id(
    "calendar_event_archive_notifications",
    "archive_event_id",
    NOTIFICATION_COLUMNS,
);

/// `calendar_event_archive_organizers`.
pub const ARCHIVE_ORGANIZERS: OwnedTable = single(
    "calendar_event_archive_organizers",
    "archive_event_id",
    ORGANIZER_COLUMNS,
);

/// `calendar_event_archive_overrides`, keyed by the archived override's id. The archive keeps no
/// id of its own for an override, so a row fits the value field limit.
pub const ARCHIVE_OVERRIDES: OwnedTable = child(
    "calendar_event_archive_overrides",
    "archive_event_id",
    OwnedOrder::Key(&["source_override_id"]),
    override_columns!(trimmed("source_override_id")),
    MAX_OWNED_ROWS,
);

/// `calendar_event_archive_override_extended_properties`, flat under the archive and linked to
/// their archived override by `source_override_id`.
pub const ARCHIVE_OVERRIDE_PROPERTIES: OwnedTable = child(
    "calendar_event_archive_override_extended_properties",
    "archive_event_id",
    OwnedOrder::Key(&["source_override_id", "property_key"]),
    &[
        trimmed("source_override_id"),
        trimmed("property_key"),
        trimmed("id"),
        text("property_value"),
        sort_order(),
    ],
    MAX_OWNED_ROWS,
);

/// `calendar_event_archive_pomodoro_configs`.
pub const ARCHIVE_FOCUS_CONFIGS: OwnedTable = single(
    "calendar_event_archive_pomodoro_configs",
    "archive_event_id",
    FOCUS_CONFIG_COLUMNS,
);

/// `calendar_event_archive_pomodoro_config_count_rhythms`, flat under the archive.
pub const ARCHIVE_COUNT_RHYTHMS: OwnedTable = single(
    "calendar_event_archive_pomodoro_config_count_rhythms",
    "archive_event_id",
    COUNT_RHYTHM_COLUMNS,
);

/// `calendar_event_archive_pomodoro_config_sequence_steps`, flat under the archive.
pub const ARCHIVE_SEQUENCE_STEPS: OwnedTable = child(
    "calendar_event_archive_pomodoro_config_sequence_steps",
    "archive_event_id",
    OwnedOrder::Key(&["step_index"]),
    SEQUENCE_STEP_COLUMNS,
    MAX_FOCUS_STEPS,
);

/// `calendar_event_archive_rdates`.
pub const ARCHIVE_RDATES: OwnedTable = by_id(
    "calendar_event_archive_rdates",
    "archive_event_id",
    &[trimmed("id"), trimmed("occurrence_start"), sort_order()],
);

/// `calendar_event_archive_task_links`, keyed by the task, which is a plain id.
pub const ARCHIVE_TASK_LINKS: OwnedTable = child(
    "calendar_event_archive_task_links",
    "archive_event_id",
    OwnedOrder::Key(&["task_id"]),
    &[trimmed("task_id"), enumerated("link_kind"), CREATED_AT],
    MAX_OWNED_ROWS,
);

/// `calendar_event_archives`. A deleted event's copy, written once and removed only by Undo;
/// its reference columns are plain text snapshots.
pub const ARCHIVES: TableSpec = TableSpec {
    id: ARCHIVES_TABLE,
    name: "calendar_event_archives",
    groups: &[
        group(
            archive_group::EVENT,
            "event",
            MergeKind::Immutable,
            &[
                trimmed("source_event_id"),
                timestamp("archived_at"),
                optional_text("original_occurrence_id"),
                optional_text("recurrence_date"),
                trimmed("calendar_id"),
                optional_text("project_id"),
                optional_text("playlist_id"),
                optional_text("environment_id"),
                optional_text("source_uid"),
                optional_text("icalendar_component_id"),
                integer("sequence", 0, i64::MAX),
                CREATED_AT,
                UPDATED_AT,
            ],
        ),
        group(
            archive_group::TIMING,
            "timing",
            MergeKind::Immutable,
            &[
                text("title"),
                trimmed("start_time"),
                trimmed("end_time"),
                trimmed("timezone"),
                flag("all_day"),
                optional_text("rrule"),
                optional_text("repeat_until"),
            ],
        ),
        group(
            archive_group::DETAILS,
            "details",
            MergeKind::Immutable,
            &[
                text("description"),
                text("location"),
                text("url"),
                color(),
                enumerated("transparency"),
                enumerated("status"),
                enumerated("visibility"),
                optional_integer("priority", 0, 9),
                optional_integer("geo_lat", -MAX_GEO_LATITUDE, MAX_GEO_LATITUDE),
                optional_integer("geo_lng", -MAX_GEO_LONGITUDE, MAX_GEO_LONGITUDE),
                optional_enumerated("local_rsvp_status"),
                flag("meeting_enabled"),
            ],
        ),
        group(
            archive_group::GUESTS,
            "guests",
            MergeKind::Immutable,
            &[
                flag("guest_can_modify"),
                flag("guest_can_invite_others"),
                flag("guest_can_see_other_guests"),
            ],
        ),
        owned_immutable(archive_group::ALARMS, "alarms", &[ARCHIVE_ALARMS]),
        owned_immutable(archive_group::ATTENDEES, "attendees", &[ARCHIVE_ATTENDEES]),
        owned_immutable(
            archive_group::CATEGORIES,
            "categories",
            &[ARCHIVE_CATEGORIES],
        ),
        owned_immutable(archive_group::EXDATES, "exdates", &[ARCHIVE_EXDATES]),
        owned_immutable(
            archive_group::PROPERTIES,
            "properties",
            &[ARCHIVE_PROPERTIES],
        ),
        owned_immutable(archive_group::MUSIC, "music", &[ARCHIVE_MUSIC]),
        owned_immutable(
            archive_group::NOTIFICATIONS,
            "notifications",
            &[ARCHIVE_NOTIFICATIONS],
        ),
        owned_immutable(archive_group::ORGANIZER, "organizer", &[ARCHIVE_ORGANIZERS]),
        owned_immutable(
            archive_group::OVERRIDES,
            "overrides",
            &[ARCHIVE_OVERRIDES, ARCHIVE_OVERRIDE_PROPERTIES],
        ),
        owned_immutable(
            archive_group::FOCUS,
            "focus",
            &[
                ARCHIVE_FOCUS_CONFIGS,
                ARCHIVE_COUNT_RHYTHMS,
                ARCHIVE_SEQUENCE_STEPS,
            ],
        ),
        owned_immutable(archive_group::RDATES, "rdates", &[ARCHIVE_RDATES]),
        owned_immutable(
            archive_group::TASK_LINKS,
            "task_links",
            &[ARCHIVE_TASK_LINKS],
        ),
    ],
    resolutions: &[within(&["geo_lat", "geo_lng"], archive_group::DETAILS)],
    holds_deletes: true,
    ..BASE
};

/// `calendar_event_archive_import_objects`, keyed by the archive and the object.
pub const ARCHIVE_OBJECTS: TableSpec = TableSpec {
    id: ARCHIVE_OBJECTS_TABLE,
    name: "calendar_event_archive_import_objects",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        immutable(
            archive_object_group::IDENTITY,
            "identity",
            &[row_key("archive_event_id"), row_key("object_id")],
            &[
                cascade("archive_event_id", ARCHIVES_TABLE),
                cascade("object_id", OBJECTS_TABLE),
            ],
        ),
        exists(archive_object_group::EXISTS),
    ],
    triggers: &[
        applier_visible("calendar_event_archive_import_objects_sync_key_insert"),
        applier_visible("calendar_event_archive_import_objects_sync_key_update"),
    ],
    derived_key: Some(DerivedKey {
        exists_group: archive_object_group::EXISTS,
        prefix: Some(ARCHIVE_OBJECT_KEY_PREFIX),
        key_inputs: &["archive_event_id", "object_id"],
    }),
    ..BASE
};

/// `project_task_event_links`, keyed by the task and the event.
pub const TASK_EVENT_LINKS: TableSpec = TableSpec {
    id: TASK_EVENT_LINKS_TABLE,
    name: "project_task_event_links",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        immutable(
            task_event_link_group::IDENTITY,
            "identity",
            &[row_key("task_id"), row_key("event_id")],
            &[
                cascade("task_id", TASKS_TABLE),
                cascade("event_id", EVENTS_TABLE),
            ],
        ),
        group(
            task_event_link_group::CREATED,
            "created",
            MergeKind::Register,
            &[CREATED_AT],
        ),
        register(
            task_event_link_group::KIND,
            "kind",
            &[enumerated("link_kind")],
        ),
        exists(task_event_link_group::EXISTS),
    ],
    triggers: &[
        applier_visible("project_task_event_links_sync_key_insert"),
        applier_visible("project_task_event_links_sync_key_update"),
    ],
    derived_key: Some(DerivedKey {
        exists_group: task_event_link_group::EXISTS,
        prefix: Some(TASK_EVENT_KEY_PREFIX),
        key_inputs: &["task_id", "event_id"],
    }),
    ..BASE
};

/// The Calendar domain manifest. Linked replicas write Calendar rows like the owner.
pub const DOMAIN: DomainManifest = DomainManifest {
    name: DOMAIN_NAME,
    table_ids: TableIdRange {
        first: 160,
        last: 223,
    },
    tables: &[
        CALENDARS,
        OBJECTS,
        COMPONENTS,
        EVENTS,
        ARCHIVES,
        ARCHIVE_OBJECTS,
        TASK_EVENT_LINKS,
    ],
    derived: &[],
    local: &[DeviceTable {
        name: "calendar_edit_receipts",
        seed: None,
        triggers: &[],
        assets: &[],
    }],
    engine: &[],
    unconverted: &[],
    assets: &[],
};
