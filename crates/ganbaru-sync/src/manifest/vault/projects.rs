//! Projects: table ids 96 to 159.
//!
//! Project groups, projects, working folders, the primary folder of each project, sections,
//! statuses, priorities, tags, tasks, checklist items, dependencies, tag links, custom fields,
//! their options and values, task change events, views, view preferences, and custom emojis
//! replicate. Priorities, tag links, values, and view preferences have a composite primary key
//! and are keyed by `sync_key`; dependencies keep their derived key in `id`. Collapsed flags and
//! revisions are `Local` columns, and the three command receipt tables stay on the device.
//! Ids 115 to 159 are reserved.

use ganbaru_sync_contracts::{GroupId, TableId};

use crate::assets::AssetClass;
use crate::domains::projects::{
    MAX_DATE_CHARS, MAX_ENUM_CHARS, MAX_FOLDER_ID_CHARS, MAX_FOLDER_NAME_CHARS, MAX_ICON_BYTES,
    MAX_ICON_CHARS, MAX_ICON_FILE_BYTES, MAX_ID_CHARS, MAX_LONG_TEXT_CHARS, MAX_NAME_CHARS,
    MAX_PREFERENCE_CHARS, MAX_RELATIVE_PATH_CHARS, MAX_REPOSITORY_IDENTITY_CHARS,
    MAX_SELECTED_OPTIONS, MAX_TIME_CHARS, MAX_TITLE_CHARS,
};
use crate::manifest::AcyclicSpec;
use crate::manifest::{
    ColumnSpec, ColumnType, DerivedKey, DeviceTable, DomainManifest, DomainTrigger, GroupSpec,
    GroupStorage, KeyDerivedColumn, LocalColumn, MergeKind, OnDelete, OwnedBound, OwnedCodec,
    OwnedOrder, OwnedReference, OwnedTable, ReferenceColumn, Resolution, SYNC_KEY_COLUMN,
    TableIdRange, TableSpec, TreeSpec, TriggerPolicy, group_id,
};

/// Domain name for Projects.
pub const DOMAIN_NAME: &str = "projects";

/// Domain adapter name for Projects.
pub const ADAPTER: &str = "projects";

/// Project group and project icon files and custom emoji images, at most 3 MiB each.
pub const ICON_CLASS: AssetClass = AssetClass {
    name: "project-icons",
    max_bytes: MAX_ICON_FILE_BYTES,
};

/// `project_groups`.
pub const GROUPS_TABLE: TableId = TableId(96);
/// `projects`.
pub const PROJECTS_TABLE: TableId = TableId(97);
/// `project_working_folders`.
pub const FOLDERS_TABLE: TableId = TableId(98);
/// `project_primary_working_folders`.
pub const PRIMARY_FOLDERS_TABLE: TableId = TableId(99);
/// `project_sections`.
pub const SECTIONS_TABLE: TableId = TableId(100);
/// `project_statuses`.
pub const STATUSES_TABLE: TableId = TableId(101);
/// `project_priorities`.
pub const PRIORITIES_TABLE: TableId = TableId(102);
/// `project_tags`.
pub const TAGS_TABLE: TableId = TableId(103);
/// `project_tasks`.
pub const TASKS_TABLE: TableId = TableId(104);
/// `project_checklist_items`.
pub const CHECKLIST_TABLE: TableId = TableId(105);
/// `project_task_dependencies`.
pub const DEPENDENCIES_TABLE: TableId = TableId(106);
/// `project_task_tag_links`.
pub const TAG_LINKS_TABLE: TableId = TableId(107);
/// `project_custom_fields`.
pub const FIELDS_TABLE: TableId = TableId(108);
/// `project_custom_field_options`.
pub const OPTIONS_TABLE: TableId = TableId(109);
/// `project_custom_field_values`.
pub const VALUES_TABLE: TableId = TableId(110);
/// `project_task_change_events`.
pub const CHANGE_EVENTS_TABLE: TableId = TableId(111);
/// `project_views`.
pub const VIEWS_TABLE: TableId = TableId(112);
/// `project_view_preferences`.
pub const VIEW_PREFERENCES_TABLE: TableId = TableId(113);
/// `project_custom_emojis`.
pub const EMOJIS_TABLE: TableId = TableId(114);

/// Prefix of priority keys, over the project and priority id.
pub const PRIORITY_KEY_PREFIX: &str = "project-priority";
/// Prefix of dependency keys, over the blocking and blocked task.
pub const DEPENDENCY_KEY_PREFIX: &str = "dependency";
/// Prefix of task tag link keys, over the task and tag.
pub const TASK_TAG_KEY_PREFIX: &str = "task-tag";
/// Prefix of custom field value keys, over the task and field.
pub const CUSTOM_FIELD_VALUE_KEY_PREFIX: &str = "custom-field-value";
/// Prefix of view preference keys, over the view and preference key.
pub const VIEW_PREFERENCE_KEY_PREFIX: &str = "view-preference";

/// Owned selected options of a custom field value.
pub const OPTION_VALUES_OWNED: &str = "project_custom_field_option_values";

/// Project group field groups.
pub mod group_group {
    use super::{GroupId, group_id};

    /// `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `icon` and `icon_byte_size`.
    pub const ICON: GroupId = group_id(2);
    /// `color`.
    pub const COLOR: GroupId = group_id(3);
    /// `order_key`.
    pub const ORDER: GroupId = group_id(4);
    /// `hidden_at` and `archived_at`.
    pub const LIFECYCLE: GroupId = group_id(5);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(6);
}

/// Project field groups.
pub mod project_group {
    use super::{GroupId, group_id};

    /// `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `group_id`.
    pub const GROUP: GroupId = group_id(1);
    /// `name`.
    pub const NAME: GroupId = group_id(2);
    /// `icon` and `icon_byte_size`.
    pub const ICON: GroupId = group_id(3);
    /// `color`.
    pub const COLOR: GroupId = group_id(4);
    /// `order_key`, ordered within the project group.
    pub const ORDER: GroupId = group_id(5);
    /// `status`.
    pub const STATUS: GroupId = group_id(6);
    /// Default Calendar event duration, name, and time mode.
    pub const EVENT_DEFAULTS: GroupId = group_id(7);
    /// Default Focus mode, preset, and custom durations.
    pub const POMODORO_DEFAULTS: GroupId = group_id(8);
    /// Default idle pause source, switch, and threshold.
    pub const IDLE_DEFAULTS: GroupId = group_id(9);
    /// `focus_playlist_id`.
    pub const FOCUS_PLAYLIST: GroupId = group_id(10);
    /// `break_playlist_id`.
    pub const BREAK_PLAYLIST: GroupId = group_id(11);
    /// `work_environment_id`.
    pub const WORK_ENVIRONMENT: GroupId = group_id(12);
    /// `blocker_ruleset_id`.
    pub const BLOCKER_RULESET: GroupId = group_id(13);
    /// Notes open mode and history retention.
    pub const NOTES_SETTINGS: GroupId = group_id(14);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(15);
}

/// Working folder field groups.
pub mod folder_group {
    use super::{GroupId, group_id};

    /// Project, kind, managed path, sort order, and creation time, fixed at creation.
    pub const CREATED: GroupId = group_id(0);
    /// `display_name`.
    pub const DISPLAY_NAME: GroupId = group_id(1);
    /// `repository_kind` and `repository_identity`.
    pub const REPOSITORY: GroupId = group_id(2);
    /// `archived_at`.
    pub const ARCHIVED: GroupId = group_id(3);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(4);
}

/// Primary working folder field groups.
pub mod primary_folder_group {
    use super::{GroupId, group_id};

    /// `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `working_folder_id`.
    pub const FOLDER: GroupId = group_id(1);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(2);
}

/// Section field groups.
pub mod section_group {
    use super::{GroupId, group_id};

    /// `project_id` and `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `order_key`.
    pub const ORDER: GroupId = group_id(2);
    /// `hidden_at` and `archived_at`.
    pub const LIFECYCLE: GroupId = group_id(3);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(4);
}

/// Status field groups.
pub mod status_group {
    use super::{GroupId, group_id};

    /// `project_id` and `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `category`.
    pub const CATEGORY: GroupId = group_id(2);
    /// `terminal`.
    pub const TERMINAL: GroupId = group_id(3);
    /// `color`.
    pub const COLOR: GroupId = group_id(4);
    /// `order_key`.
    pub const ORDER: GroupId = group_id(5);
    /// `deleted_at`.
    pub const DELETED: GroupId = group_id(6);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(7);
}

/// Priority field groups.
pub mod priority_group {
    use super::{GroupId, group_id};

    /// `project_id` and `id`, the primary key.
    pub const IDENTITY: GroupId = group_id(0);
    /// `created_at`.
    pub const CREATED: GroupId = group_id(1);
    /// `name`.
    pub const NAME: GroupId = group_id(2);
    /// `color`.
    pub const COLOR: GroupId = group_id(3);
    /// `order_key`.
    pub const ORDER: GroupId = group_id(4);
    /// `deleted_at`.
    pub const DELETED: GroupId = group_id(5);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(6);
}

/// Tag field groups.
pub mod tag_group {
    use super::{GroupId, group_id};

    /// `project_id` and `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `color`.
    pub const COLOR: GroupId = group_id(2);
    /// `order_key`.
    pub const ORDER: GroupId = group_id(3);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(4);
}

/// Task field groups.
pub mod task_group {
    use super::{GroupId, group_id};

    /// `project_id` and `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `parent_task_id`, the tree parent.
    pub const PARENT: GroupId = group_id(1);
    /// `section_id` and `section_order_key`: one move places the task in its section.
    pub const SECTION_PLACE: GroupId = group_id(2);
    /// `status_id`, `completed_at`, and `status_order_key`: completion follows the status.
    pub const STATUS_PLACE: GroupId = group_id(3);
    /// `title`.
    pub const TITLE: GroupId = group_id(4);
    /// `description`.
    pub const DESCRIPTION: GroupId = group_id(5);
    /// `priority`.
    pub const PRIORITY: GroupId = group_id(6);
    /// `task_type` and `milestone`.
    pub const KIND: GroupId = group_id(7);
    /// `estimate_minutes`.
    pub const ESTIMATE: GroupId = group_id(8);
    /// Start, due, and target end dates with their times and reminders.
    pub const SCHEDULE: GroupId = group_id(9);
    /// `archived_at`.
    pub const ARCHIVED: GroupId = group_id(10);
    /// `blocker_reason`.
    pub const BLOCKER_REASON: GroupId = group_id(11);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(12);
}

/// Checklist item field groups.
pub mod checklist_group {
    use super::{GroupId, group_id};

    /// `task_id` and `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `title`.
    pub const TITLE: GroupId = group_id(1);
    /// `completed_at`.
    pub const COMPLETED: GroupId = group_id(2);
    /// `order_key`.
    pub const ORDER: GroupId = group_id(3);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(4);
}

/// Dependency field groups.
pub mod dependency_group {
    use super::{GroupId, group_id};

    /// `blocking_task_id` and `blocked_task_id`, the key inputs.
    pub const IDENTITY: GroupId = group_id(0);
    /// `dependency_type`.
    pub const KIND: GroupId = group_id(1);
    /// `created_at`.
    pub const CREATED: GroupId = group_id(2);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(3);
}

/// Task tag link field groups.
pub mod tag_link_group {
    use super::{GroupId, group_id};

    /// `task_id` and `tag_id`, the primary key.
    pub const IDENTITY: GroupId = group_id(0);
    /// `created_at`.
    pub const CREATED: GroupId = group_id(1);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(2);
}

/// Custom field field groups.
pub mod field_group {
    use super::{GroupId, group_id};

    /// `project_id`, `field_type`, and `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `icon`.
    pub const ICON: GroupId = group_id(2);
    /// `order_key`.
    pub const ORDER: GroupId = group_id(3);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(4);
}

/// Custom field option field groups.
pub mod option_group {
    use super::{GroupId, group_id};

    /// `field_id` and `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `order_key`.
    pub const ORDER: GroupId = group_id(2);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(3);
}

/// Custom field value field groups.
pub mod value_group {
    use super::{GroupId, group_id};

    /// `task_id` and `field_id`, the primary key.
    pub const IDENTITY: GroupId = group_id(0);
    /// The text, number, date, and checkbox columns and the selected options. Coupled: a value
    /// holds at most one kind, so one write replaces the whole value.
    pub const VALUE: GroupId = group_id(1);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(2);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(3);
}

/// Task change event field groups.
pub mod change_event_group {
    use super::{GroupId, group_id};

    /// Every column; events are append only.
    pub const CREATED: GroupId = group_id(0);
}

/// View field groups.
pub mod view_group {
    use super::{GroupId, group_id};

    /// `project_id`, `view_type`, and `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `order_key`.
    pub const ORDER: GroupId = group_id(2);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(3);
}

/// View preference field groups.
pub mod view_preference_group {
    use super::{GroupId, group_id};

    /// `view_id` and `preference_key`, the primary key.
    pub const IDENTITY: GroupId = group_id(0);
    /// `preference_value`.
    pub const VALUE: GroupId = group_id(1);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(2);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(3);
}

/// Custom emoji field groups.
pub mod emoji_group {
    use super::{GroupId, group_id};

    /// `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `asset_path` and `asset_byte_size`.
    pub const ASSET: GroupId = group_id(2);
    /// `order_key`.
    pub const ORDER: GroupId = group_id(3);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(4);
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

/// `spec`, shown as a conflict when concurrent versions differ and offered in recovery entries
/// when its row is deleted while it changes.
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

/// A `Coupled` group whose reference columns cascade from their rows.
const fn coupled(
    id: GroupId,
    name: &'static str,
    columns: &'static [ColumnSpec],
    references: &'static [ReferenceColumn],
) -> GroupSpec {
    GroupSpec {
        references,
        ..group(id, name, MergeKind::Coupled, columns)
    }
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

/// The `order` group of a table ordered by `order_key`.
const fn order(id: GroupId) -> GroupSpec {
    group(id, "order", MergeKind::Position, &[ORDER_KEY])
}

/// The `updated` group of a table with `updated_at`.
const fn updated(id: GroupId) -> GroupSpec {
    group(id, "updated", MergeKind::Max, &[UPDATED_AT])
}

/// The existence register of a derived-key table.
const fn exists(id: GroupId) -> GroupSpec {
    group(id, "exists", MergeKind::Register, &[])
}

/// A one-column reference group.
const fn reference(
    id: GroupId,
    name: &'static str,
    column: &'static [ColumnSpec],
    table: TableId,
    on_delete: OnDelete,
) -> GroupSpec {
    group(id, name, MergeKind::Reference { table, on_delete }, column)
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

const fn optional_timestamp(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalTimestamp,
    }
}

const fn order_key(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OrderKey,
    }
}

const fn row_key(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalRowKey,
    }
}

/// A Projects palette color index.
const fn color(name: &'static str) -> ColumnSpec {
    integer(name, 0, 31)
}

/// An optional Projects palette color index.
const fn optional_color(name: &'static str) -> ColumnSpec {
    optional_integer(name, 0, 31)
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

const fn gated(name: &'static str) -> DomainTrigger {
    DomainTrigger {
        name,
        policy: TriggerPolicy::Gated,
    }
}

const fn within(columns: &'static [&'static str], group: GroupId) -> Resolution {
    Resolution::WithinGroup { columns, group }
}

/// The local `collapsed` flag of groups and sections; a received row starts expanded.
const COLLAPSED: LocalColumn = local("collapsed", "0");

/// The local `revision` counter of rows edited with an expected revision.
const REVISION: LocalColumn = local("revision", "1");

/// The `created_at` column of a `created` group.
const CREATED_AT: ColumnSpec = timestamp("created_at");

/// The `order_key` column of an `order` group.
const ORDER_KEY: ColumnSpec = order_key("order_key");

/// The `updated_at` column of an `updated` group.
const UPDATED_AT: ColumnSpec = timestamp("updated_at");

/// A table keyed by `id` with no derived columns, hooks, tree, or publication; each table
/// overrides what it declares.
const BASE: TableSpec = TableSpec {
    id: GROUPS_TABLE,
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

/// `project_groups`.
pub const GROUPS: TableSpec = TableSpec {
    id: GROUPS_TABLE,
    name: "project_groups",
    groups: &[
        group(
            group_group::CREATED,
            "created",
            MergeKind::Immutable,
            &[CREATED_AT],
        ),
        register(
            group_group::NAME,
            "name",
            &[trimmed("name", MAX_NAME_CHARS)],
        ),
        register(
            group_group::ICON,
            "icon",
            &[
                trimmed("icon", MAX_ICON_CHARS),
                optional_integer("icon_byte_size", 1, MAX_ICON_BYTES),
            ],
        ),
        register(group_group::COLOR, "color", &[optional_color("color")]),
        order(group_group::ORDER),
        group(
            group_group::LIFECYCLE,
            "lifecycle",
            MergeKind::Coupled,
            &[
                optional_timestamp("hidden_at"),
                optional_timestamp("archived_at"),
            ],
        ),
        updated(group_group::UPDATED),
    ],
    local_columns: &[COLLAPSED],
    resolutions: &[within(&["icon", "icon_byte_size"], group_group::ICON)],
    triggers: &[
        applier_visible("project_groups_icon_size_insert"),
        applier_visible("project_groups_icon_size_update"),
        applier_visible("project_groups_order_key_insert"),
        applier_visible("project_groups_order_key_update"),
    ],
    ..BASE
};

/// `projects`. Playlist, work environment, and blocker ruleset ids are opaque registers: the
/// rows they name live in other domains, and a missing one reads as none.
pub const PROJECTS: TableSpec = TableSpec {
    id: PROJECTS_TABLE,
    name: "projects",
    groups: &[
        group(
            project_group::CREATED,
            "created",
            MergeKind::Immutable,
            &[CREATED_AT],
        ),
        reference(
            project_group::GROUP,
            "group",
            &[row_key("group_id")],
            GROUPS_TABLE,
            OnDelete::Cascade,
        ),
        surfaced(register(
            project_group::NAME,
            "name",
            &[trimmed("name", MAX_NAME_CHARS)],
        )),
        register(
            project_group::ICON,
            "icon",
            &[
                trimmed("icon", MAX_ICON_CHARS),
                optional_integer("icon_byte_size", 1, MAX_ICON_BYTES),
            ],
        ),
        register(project_group::COLOR, "color", &[optional_color("color")]),
        order(project_group::ORDER),
        register(
            project_group::STATUS,
            "status",
            &[text("status", MAX_ENUM_CHARS)],
        ),
        group(
            project_group::EVENT_DEFAULTS,
            "event defaults",
            MergeKind::Coupled,
            &[
                optional_integer("default_event_duration_minutes", 1, 1440),
                optional_text("default_event_name", MAX_NAME_CHARS),
                text("default_event_time_mode", MAX_ENUM_CHARS),
            ],
        ),
        group(
            project_group::POMODORO_DEFAULTS,
            "pomodoro defaults",
            MergeKind::Coupled,
            &[
                text("default_pomodoro_mode", MAX_ENUM_CHARS),
                optional_text("default_pomodoro_preset_key", MAX_ENUM_CHARS),
                optional_integer("default_pomodoro_focus_minutes", 1, 120),
                optional_integer("default_pomodoro_short_break_minutes", 1, 30),
                optional_integer("default_pomodoro_long_break_minutes", 1, 60),
                optional_integer("default_pomodoro_long_break_after_focus_count", 1, 12),
            ],
        ),
        group(
            project_group::IDLE_DEFAULTS,
            "idle defaults",
            MergeKind::Coupled,
            &[
                text("default_idle_settings_source", MAX_ENUM_CHARS),
                flag("default_idle_pause_enabled"),
                integer("default_idle_threshold_minutes", 1, 15),
            ],
        ),
        register(
            project_group::FOCUS_PLAYLIST,
            "focus playlist",
            &[optional_text("focus_playlist_id", MAX_ID_CHARS)],
        ),
        register(
            project_group::BREAK_PLAYLIST,
            "break playlist",
            &[optional_text("break_playlist_id", MAX_ID_CHARS)],
        ),
        register(
            project_group::WORK_ENVIRONMENT,
            "work environment",
            &[optional_text("work_environment_id", MAX_ID_CHARS)],
        ),
        register(
            project_group::BLOCKER_RULESET,
            "blocker ruleset",
            &[optional_text("blocker_ruleset_id", MAX_ID_CHARS)],
        ),
        group(
            project_group::NOTES_SETTINGS,
            "notes settings",
            MergeKind::Coupled,
            &[
                optional_text("notes_default_open_mode", MAX_ENUM_CHARS),
                optional_integer("notes_history_retention_days", 0, 365),
            ],
        ),
        updated(project_group::UPDATED),
    ],
    resolutions: &[
        within(&["icon", "icon_byte_size"], project_group::ICON),
        within(
            &[
                "default_pomodoro_mode",
                "default_pomodoro_preset_key",
                "default_pomodoro_focus_minutes",
                "default_pomodoro_short_break_minutes",
                "default_pomodoro_long_break_minutes",
                "default_pomodoro_long_break_after_focus_count",
            ],
            project_group::POMODORO_DEFAULTS,
        ),
    ],
    triggers: &[
        applier_visible("projects_icon_size_insert"),
        applier_visible("projects_icon_size_update"),
        applier_visible("projects_order_key_insert"),
        applier_visible("projects_order_key_update"),
    ],
    ..BASE
};

/// `project_working_folders`. A managed folder is keyed by its project, created with it, and
/// never archived or removed while the project exists, so its key is never removed and added
/// again. Received folders never touch the filesystem.
pub const FOLDERS: TableSpec = TableSpec {
    id: FOLDERS_TABLE,
    name: "project_working_folders",
    groups: &[
        immutable(
            folder_group::CREATED,
            "created",
            &[
                row_key("project_id"),
                text("kind", MAX_ENUM_CHARS),
                optional_text("managed_relative_path", MAX_RELATIVE_PATH_CHARS),
                integer("sort_order", 0, i64::MAX),
                CREATED_AT,
            ],
            &[cascade("project_id", PROJECTS_TABLE)],
        ),
        bumps(register(
            folder_group::DISPLAY_NAME,
            "display name",
            &[trimmed("display_name", MAX_FOLDER_NAME_CHARS)],
        )),
        bumps(group(
            folder_group::REPOSITORY,
            "repository",
            MergeKind::Coupled,
            &[
                text("repository_kind", MAX_ENUM_CHARS),
                optional_text("repository_identity", MAX_REPOSITORY_IDENTITY_CHARS),
            ],
        )),
        bumps(register(
            folder_group::ARCHIVED,
            "archived",
            &[optional_timestamp("archived_at")],
        )),
        updated(folder_group::UPDATED),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    resolutions: &[
        within(&["kind", "managed_relative_path"], folder_group::CREATED),
        within(
            &["repository_kind", "repository_identity"],
            folder_group::REPOSITORY,
        ),
    ],
    triggers: &[
        applier_visible("project_working_folders_managed_relative_path_shape_insert"),
        applier_visible("project_working_folders_managed_relative_path_shape_update"),
        gated("project_working_folders_protect_managed_insert"),
        gated("project_working_folders_protect_managed_delete"),
        gated("project_working_folders_protect_managed_update"),
    ],
    ..BASE
};

/// `project_primary_working_folders`, keyed by its project and leaving with it. The folder is a
/// plain register: reads resolve the effective primary as the stored folder while it is live,
/// belongs to the project, and is not archived, and the managed folder otherwise.
pub const PRIMARY_FOLDERS: TableSpec = TableSpec {
    id: PRIMARY_FOLDERS_TABLE,
    name: "project_primary_working_folders",
    key_column: "project_id",
    groups: &[
        group(
            primary_folder_group::CREATED,
            "created",
            MergeKind::Immutable,
            &[CREATED_AT],
        ),
        bumps(register(
            primary_folder_group::FOLDER,
            "folder",
            &[text("working_folder_id", MAX_FOLDER_ID_CHARS)],
        )),
        updated(primary_folder_group::UPDATED),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    ..BASE
};

/// `project_sections`.
pub const SECTIONS: TableSpec = TableSpec {
    id: SECTIONS_TABLE,
    name: "project_sections",
    groups: &[
        immutable(
            section_group::CREATED,
            "created",
            &[row_key("project_id"), CREATED_AT],
            &[cascade("project_id", PROJECTS_TABLE)],
        ),
        register(
            section_group::NAME,
            "name",
            &[trimmed("name", MAX_NAME_CHARS)],
        ),
        order(section_group::ORDER),
        group(
            section_group::LIFECYCLE,
            "lifecycle",
            MergeKind::Coupled,
            &[
                optional_timestamp("hidden_at"),
                optional_timestamp("archived_at"),
            ],
        ),
        updated(section_group::UPDATED),
    ],
    local_columns: &[COLLAPSED],
    triggers: &[
        applier_visible("project_sections_order_key_insert"),
        applier_visible("project_sections_order_key_update"),
    ],
    ..BASE
};

/// `project_statuses`. Deletion is soft, so only the project cascade tombstones a status.
pub const STATUSES: TableSpec = TableSpec {
    id: STATUSES_TABLE,
    name: "project_statuses",
    groups: &[
        immutable(
            status_group::CREATED,
            "created",
            &[row_key("project_id"), CREATED_AT],
            &[cascade("project_id", PROJECTS_TABLE)],
        ),
        register(
            status_group::NAME,
            "name",
            &[trimmed("name", MAX_NAME_CHARS)],
        ),
        register(
            status_group::CATEGORY,
            "category",
            &[text("category", MAX_ENUM_CHARS)],
        ),
        register(status_group::TERMINAL, "terminal", &[flag("terminal")]),
        register(status_group::COLOR, "color", &[color("color")]),
        order(status_group::ORDER),
        register(
            status_group::DELETED,
            "deleted",
            &[optional_timestamp("deleted_at")],
        ),
        updated(status_group::UPDATED),
    ],
    triggers: &[
        applier_visible("project_statuses_order_key_insert"),
        applier_visible("project_statuses_order_key_update"),
    ],
    ..BASE
};

/// `project_priorities`, keyed by the derived key of its project and id. Deletion is soft, so a
/// key is never removed and added again and the table has no existence register.
pub const PRIORITIES: TableSpec = TableSpec {
    id: PRIORITIES_TABLE,
    name: "project_priorities",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        immutable(
            priority_group::IDENTITY,
            "identity",
            &[row_key("project_id"), trimmed("id", MAX_ID_CHARS)],
            &[cascade("project_id", PROJECTS_TABLE)],
        ),
        register(priority_group::CREATED, "created", &[CREATED_AT]),
        register(
            priority_group::NAME,
            "name",
            &[trimmed("name", MAX_NAME_CHARS)],
        ),
        register(priority_group::COLOR, "color", &[color("color")]),
        order(priority_group::ORDER),
        register(
            priority_group::DELETED,
            "deleted",
            &[optional_timestamp("deleted_at")],
        ),
        updated(priority_group::UPDATED),
    ],
    triggers: &[
        applier_visible("project_priorities_order_key_insert"),
        applier_visible("project_priorities_order_key_update"),
    ],
    ..BASE
};

/// `project_tags`. Names may repeat: two devices creating one name keep two tags.
pub const TAGS: TableSpec = TableSpec {
    id: TAGS_TABLE,
    name: "project_tags",
    groups: &[
        immutable(
            tag_group::CREATED,
            "created",
            &[row_key("project_id"), CREATED_AT],
            &[cascade("project_id", PROJECTS_TABLE)],
        ),
        register(tag_group::NAME, "name", &[trimmed("name", MAX_NAME_CHARS)]),
        register(tag_group::COLOR, "color", &[optional_color("color")]),
        order(tag_group::ORDER),
        updated(tag_group::UPDATED),
    ],
    triggers: &[
        applier_visible("project_tags_order_key_insert"),
        applier_visible("project_tags_order_key_update"),
    ],
    ..BASE
};

/// The schedule columns of a task, which its date, time, and reminder CHECKs read together.
const SCHEDULE_COLUMNS: [ColumnSpec; 7] = [
    optional_text("start_date", MAX_DATE_CHARS),
    optional_text("start_time", MAX_TIME_CHARS),
    optional_integer("start_reminder_minutes", 0, 40320),
    optional_text("due_date", MAX_DATE_CHARS),
    optional_text("due_time", MAX_TIME_CHARS),
    optional_integer("due_reminder_minutes", 0, 40320),
    optional_text("target_end_date", MAX_DATE_CHARS),
];

/// `project_tasks`. Subtasks nest one level; a merge that nests deeper or forms a cycle is
/// repaired by moving a row to the root.
pub const TASKS: TableSpec = TableSpec {
    id: TASKS_TABLE,
    name: "project_tasks",
    groups: &[
        immutable(
            task_group::CREATED,
            "created",
            &[row_key("project_id"), CREATED_AT],
            &[cascade("project_id", PROJECTS_TABLE)],
        ),
        bumps(reference(
            task_group::PARENT,
            "parent",
            &[row_key("parent_task_id")],
            TASKS_TABLE,
            OnDelete::Cascade,
        )),
        bumps(coupled(
            task_group::SECTION_PLACE,
            "section place",
            &[row_key("section_id"), order_key("section_order_key")],
            &[cascade("section_id", SECTIONS_TABLE)],
        )),
        bumps(coupled(
            task_group::STATUS_PLACE,
            "status place",
            &[
                row_key("status_id"),
                optional_timestamp("completed_at"),
                order_key("status_order_key"),
            ],
            &[cascade("status_id", STATUSES_TABLE)],
        )),
        bumps(surfaced(register(
            task_group::TITLE,
            "title",
            &[trimmed("title", MAX_TITLE_CHARS)],
        ))),
        bumps(surfaced(register(
            task_group::DESCRIPTION,
            "description",
            &[text("description", MAX_LONG_TEXT_CHARS)],
        ))),
        bumps(register(
            task_group::PRIORITY,
            "priority",
            &[trimmed("priority", MAX_ID_CHARS)],
        )),
        bumps(group(
            task_group::KIND,
            "kind",
            MergeKind::Coupled,
            &[text("task_type", MAX_ENUM_CHARS), flag("milestone")],
        )),
        bumps(register(
            task_group::ESTIMATE,
            "estimate",
            &[optional_integer("estimate_minutes", 1, i64::MAX)],
        )),
        bumps(group(
            task_group::SCHEDULE,
            "schedule",
            MergeKind::Coupled,
            &SCHEDULE_COLUMNS,
        )),
        bumps(register(
            task_group::ARCHIVED,
            "archived",
            &[optional_timestamp("archived_at")],
        )),
        bumps(register(
            task_group::BLOCKER_REASON,
            "blocker reason",
            &[optional_text("blocker_reason", MAX_LONG_TEXT_CHARS)],
        )),
        updated(task_group::UPDATED),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    resolutions: &[
        within(&["start_time", "start_date"], task_group::SCHEDULE),
        within(&["due_time", "due_date"], task_group::SCHEDULE),
        within(
            &["start_reminder_minutes", "start_date"],
            task_group::SCHEDULE,
        ),
        within(&["due_reminder_minutes", "due_date"], task_group::SCHEDULE),
        within(&["start_date", "target_end_date"], task_group::SCHEDULE),
    ],
    triggers: &[
        applier_visible("project_tasks_order_key_insert"),
        applier_visible("project_tasks_order_key_update"),
    ],
    tree: Some(TreeSpec {
        parent_group: task_group::PARENT,
        position_group: None,
        max_depth: Some(1),
    }),
    ..BASE
};

/// `project_checklist_items`.
pub const CHECKLIST: TableSpec = TableSpec {
    id: CHECKLIST_TABLE,
    name: "project_checklist_items",
    groups: &[
        immutable(
            checklist_group::CREATED,
            "created",
            &[row_key("task_id"), CREATED_AT],
            &[cascade("task_id", TASKS_TABLE)],
        ),
        GroupSpec {
            surfaced: true,
            ..register(
                checklist_group::TITLE,
                "title",
                &[trimmed("title", MAX_TITLE_CHARS)],
            )
        },
        register(
            checklist_group::COMPLETED,
            "completed",
            &[optional_timestamp("completed_at")],
        ),
        order(checklist_group::ORDER),
        updated(checklist_group::UPDATED),
    ],
    triggers: &[
        applier_visible("project_checklist_items_order_key_insert"),
        applier_visible("project_checklist_items_order_key_update"),
    ],
    ..BASE
};

/// `project_task_dependencies`, keyed by the derived key of its blocking and blocked task, which
/// `id` holds so commands keep addressing a dependency by id; `sync_key` repeats it. Edges never
/// form a cycle: the newest edge of a merged cycle is hidden as a conflict.
pub const DEPENDENCIES: TableSpec = TableSpec {
    id: DEPENDENCIES_TABLE,
    name: "project_task_dependencies",
    groups: &[
        immutable(
            dependency_group::IDENTITY,
            "identity",
            &[row_key("blocking_task_id"), row_key("blocked_task_id")],
            &[
                cascade("blocking_task_id", TASKS_TABLE),
                cascade("blocked_task_id", TASKS_TABLE),
            ],
        ),
        register(
            dependency_group::KIND,
            "kind",
            &[text("dependency_type", MAX_ENUM_CHARS)],
        ),
        register(dependency_group::CREATED, "created", &[CREATED_AT]),
        exists(dependency_group::EXISTS),
    ],
    key_derived_columns: &[KeyDerivedColumn {
        name: "sync_key",
        prefix: "",
    }],
    resolutions: &[within(
        &["blocking_task_id", "blocked_task_id"],
        dependency_group::IDENTITY,
    )],
    derived_key: Some(DerivedKey {
        exists_group: dependency_group::EXISTS,
        prefix: Some(DEPENDENCY_KEY_PREFIX),
        key_inputs: &["blocking_task_id", "blocked_task_id"],
    }),
    acyclic: Some(AcyclicSpec {
        from_column: "blocking_task_id",
        to_column: "blocked_task_id",
    }),
    ..BASE
};

/// `project_task_tag_links`.
pub const TAG_LINKS: TableSpec = TableSpec {
    id: TAG_LINKS_TABLE,
    name: "project_task_tag_links",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        immutable(
            tag_link_group::IDENTITY,
            "identity",
            &[row_key("task_id"), row_key("tag_id")],
            &[
                cascade("task_id", TASKS_TABLE),
                cascade("tag_id", TAGS_TABLE),
            ],
        ),
        register(tag_link_group::CREATED, "created", &[CREATED_AT]),
        exists(tag_link_group::EXISTS),
    ],
    triggers: &[
        applier_visible("project_task_tag_links_sync_key_insert"),
        applier_visible("project_task_tag_links_sync_key_update"),
    ],
    derived_key: Some(DerivedKey {
        exists_group: tag_link_group::EXISTS,
        prefix: Some(TASK_TAG_KEY_PREFIX),
        key_inputs: &["task_id", "tag_id"],
    }),
    ..BASE
};

/// `project_custom_fields`. The type is fixed at creation.
pub const FIELDS: TableSpec = TableSpec {
    id: FIELDS_TABLE,
    name: "project_custom_fields",
    groups: &[
        immutable(
            field_group::CREATED,
            "created",
            &[
                row_key("project_id"),
                text("field_type", MAX_ENUM_CHARS),
                CREATED_AT,
            ],
            &[cascade("project_id", PROJECTS_TABLE)],
        ),
        bumps(register(
            field_group::NAME,
            "name",
            &[trimmed("name", MAX_NAME_CHARS)],
        )),
        bumps(register(
            field_group::ICON,
            "icon",
            &[optional_text("icon", MAX_ICON_CHARS)],
        )),
        order(field_group::ORDER),
        updated(field_group::UPDATED),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    triggers: &[
        applier_visible("project_custom_fields_order_key_insert"),
        applier_visible("project_custom_fields_order_key_update"),
    ],
    ..BASE
};

/// `project_custom_field_options`.
pub const OPTIONS: TableSpec = TableSpec {
    id: OPTIONS_TABLE,
    name: "project_custom_field_options",
    groups: &[
        immutable(
            option_group::CREATED,
            "created",
            &[row_key("field_id"), CREATED_AT],
            &[cascade("field_id", FIELDS_TABLE)],
        ),
        bumps(register(
            option_group::NAME,
            "name",
            &[trimmed("name", MAX_NAME_CHARS)],
        )),
        order(option_group::ORDER),
        updated(option_group::UPDATED),
    ],
    local_columns: &[REVISION],
    revision_column: Some("revision"),
    triggers: &[
        applier_visible("project_custom_field_options_order_key_insert"),
        applier_visible("project_custom_field_options_order_key_update"),
    ],
    ..BASE
};

/// The selected options of a custom field value, in selection order. An option deleted on
/// another device drops out of the materialized list and stays in the value.
pub const OPTION_VALUES: OwnedTable = OwnedTable {
    table: OPTION_VALUES_OWNED,
    parent_column: "value_key",
    order: OwnedOrder::Ordinal("ordinal"),
    columns: &[row_key("option_id")],
    references: &[OwnedReference {
        column: "option_id",
        table: OPTIONS_TABLE,
    }],
    unique: &[],
    max_rows: MAX_SELECTED_OPTIONS,
    codec: OwnedCodec::Generic,
    nested: &[],
    bound: OwnedBound::Static,
};

/// `project_custom_field_values`. Clearing a value removes its row.
pub const VALUES: TableSpec = TableSpec {
    id: VALUES_TABLE,
    name: "project_custom_field_values",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        immutable(
            value_group::IDENTITY,
            "identity",
            &[row_key("task_id"), row_key("field_id")],
            &[
                cascade("task_id", TASKS_TABLE),
                cascade("field_id", FIELDS_TABLE),
            ],
        ),
        GroupSpec {
            id: value_group::VALUE,
            name: "value",
            kind: MergeKind::Coupled,
            storage: GroupStorage::Owned {
                columns: &[
                    optional_text("text_value", MAX_LONG_TEXT_CHARS),
                    optional_integer("number_value", i64::MIN, i64::MAX),
                    optional_text("date_value", MAX_DATE_CHARS),
                    optional_integer("checkbox_value", 0, 1),
                ],
                tables: &[OPTION_VALUES],
            },
            references: &[],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
        updated(value_group::UPDATED),
        exists(value_group::EXISTS),
    ],
    resolutions: &[within(
        &["text_value", "number_value", "date_value", "checkbox_value"],
        value_group::VALUE,
    )],
    derived_key: Some(DerivedKey {
        exists_group: value_group::EXISTS,
        prefix: Some(CUSTOM_FIELD_VALUE_KEY_PREFIX),
        key_inputs: &["task_id", "field_id"],
    }),
    ..BASE
};

/// `project_task_change_events`, append only.
pub const CHANGE_EVENTS: TableSpec = TableSpec {
    id: CHANGE_EVENTS_TABLE,
    name: "project_task_change_events",
    groups: &[immutable(
        change_event_group::CREATED,
        "created",
        &[
            row_key("task_id"),
            text("event_type", MAX_ENUM_CHARS),
            optional_text("field_name", MAX_ENUM_CHARS),
            optional_text("old_value", MAX_LONG_TEXT_CHARS),
            optional_text("new_value", MAX_LONG_TEXT_CHARS),
            optional_text("reason", MAX_LONG_TEXT_CHARS),
            timestamp("occurred_at"),
        ],
        &[cascade("task_id", TASKS_TABLE)],
    )],
    ..BASE
};

/// `project_views`. Views keep plain ids: default views use `view-{project_id}-{type}`, and two
/// devices adding one hosted view concurrently keep two.
pub const VIEWS: TableSpec = TableSpec {
    id: VIEWS_TABLE,
    name: "project_views",
    groups: &[
        immutable(
            view_group::CREATED,
            "created",
            &[
                row_key("project_id"),
                trimmed("view_type", MAX_ENUM_CHARS),
                CREATED_AT,
            ],
            &[cascade("project_id", PROJECTS_TABLE)],
        ),
        register(
            view_group::NAME,
            "name",
            &[optional_text("name", MAX_NAME_CHARS)],
        ),
        order(view_group::ORDER),
        updated(view_group::UPDATED),
    ],
    triggers: &[
        applier_visible("project_views_order_key_insert"),
        applier_visible("project_views_order_key_update"),
    ],
    ..BASE
};

/// `project_view_preferences`.
pub const VIEW_PREFERENCES: TableSpec = TableSpec {
    id: VIEW_PREFERENCES_TABLE,
    name: "project_view_preferences",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        immutable(
            view_preference_group::IDENTITY,
            "identity",
            &[
                row_key("view_id"),
                trimmed("preference_key", MAX_ENUM_CHARS),
            ],
            &[cascade("view_id", VIEWS_TABLE)],
        ),
        register(
            view_preference_group::VALUE,
            "value",
            &[text("preference_value", MAX_PREFERENCE_CHARS)],
        ),
        updated(view_preference_group::UPDATED),
        exists(view_preference_group::EXISTS),
    ],
    triggers: &[
        applier_visible("project_view_preferences_sync_key_insert"),
        applier_visible("project_view_preferences_sync_key_update"),
    ],
    derived_key: Some(DerivedKey {
        exists_group: view_preference_group::EXISTS,
        prefix: Some(VIEW_PREFERENCE_KEY_PREFIX),
        key_inputs: &["view_id", "preference_key"],
    }),
    ..BASE
};

/// `project_custom_emojis`.
pub const EMOJIS: TableSpec = TableSpec {
    id: EMOJIS_TABLE,
    name: "project_custom_emojis",
    groups: &[
        group(
            emoji_group::CREATED,
            "created",
            MergeKind::Immutable,
            &[CREATED_AT],
        ),
        register(
            emoji_group::NAME,
            "name",
            &[trimmed("name", MAX_NAME_CHARS)],
        ),
        register(
            emoji_group::ASSET,
            "asset",
            &[
                trimmed("asset_path", MAX_ICON_CHARS),
                optional_integer("asset_byte_size", 1, MAX_ICON_BYTES),
            ],
        ),
        order(emoji_group::ORDER),
        updated(emoji_group::UPDATED),
    ],
    triggers: &[
        applier_visible("project_custom_emojis_asset_size_insert"),
        applier_visible("project_custom_emojis_asset_size_update"),
        applier_visible("project_custom_emojis_order_key_insert"),
        applier_visible("project_custom_emojis_order_key_update"),
    ],
    ..BASE
};

/// A device table without domain triggers.
const fn device(name: &'static str) -> DeviceTable {
    DeviceTable {
        name,
        triggers: &[],
        assets: &[],
    }
}

/// Projects domain. Replicas write it; commands whose writes reach owner-only tables refuse on
/// replicas before writing.
pub const DOMAIN: DomainManifest = DomainManifest {
    name: DOMAIN_NAME,
    table_ids: TableIdRange {
        first: 96,
        last: 159,
    },
    tables: &[
        GROUPS,
        PROJECTS,
        FOLDERS,
        PRIMARY_FOLDERS,
        SECTIONS,
        STATUSES,
        PRIORITIES,
        TAGS,
        TASKS,
        CHECKLIST,
        DEPENDENCIES,
        TAG_LINKS,
        FIELDS,
        OPTIONS,
        VALUES,
        CHANGE_EVENTS,
        VIEWS,
        VIEW_PREFERENCES,
        EMOJIS,
    ],
    derived: &[],
    local: &[
        device("project_dependency_cascade_receipts"),
        device("project_reorder_receipts"),
        device("project_task_bulk_receipts"),
    ],
    engine: &[],
    replica_writable: true,
    assets: &[ICON_CLASS],
};
