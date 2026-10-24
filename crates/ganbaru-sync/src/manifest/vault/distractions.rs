//! Distraction rules and usage: table ids 272 to 287.
//!
//! Vault settings, built-in category toggles, custom categories, host and app rules, and usage
//! limits replicate. Rule edits are low-stakes toggles, so no group is surfaced and every mutable
//! group is recoverable. Host and app rules are keyed by their content, so a rule removed on one
//! device can be added again on another under the same key.
//!
//! Daily usage replicates as one total per device, source, and local day, which only its device
//! writes and which only grows. Usage samples stay on the device as its accounting journal; block
//! events stay owner-only. Ids 279 to 287 are reserved.

use ganbaru_sync_contracts::bounds::MAX_DEVICE_ID_BYTES;
use ganbaru_sync_contracts::{GroupId, TableId};

use crate::domains::distractions::{
    MAX_APP_IDENTITY_CHARS, MAX_APP_NAME_CHARS, MAX_CATEGORY_NAME_CHARS, MAX_COLOR, MAX_ENUM_CHARS,
    MAX_HOST_CHARS, MAX_ID_CHARS, MAX_LIMIT_ENTRIES, MAX_LIMIT_NAME_CHARS,
    MAX_MATCH_NAMES_JSON_CHARS, MAX_MINUTES_PER_DAY, MAX_MINUTES_PER_WEEK, MAX_PACKAGE_CHARS,
    MAX_USAGE_DAY_SECONDS, MAX_USAGE_LABEL_CHARS, MAX_USAGE_SOURCE_KEY_CHARS, USAGE_DATE_CHARS,
};
use crate::manifest::{
    ColumnSpec, ColumnType, DerivedKey, DeviceTable, DomainManifest, GroupSpec, GroupStorage,
    MergeKind, OnDelete, OwnedBound, OwnedCodec, OwnedOrder, OwnedTable, TableIdRange, TableSpec,
    group_id,
};

/// Domain name for distraction rules.
pub const DOMAIN_NAME: &str = "distractions";

/// Domain adapter name for distraction rules.
pub const ADAPTER: &str = "distractions";

/// `distractions_settings`.
pub const SETTINGS_TABLE: TableId = TableId(272);
/// `distractions_category_rules`.
pub const CATEGORY_RULES_TABLE: TableId = TableId(273);
/// `distractions_custom_categories`.
pub const CUSTOM_CATEGORIES_TABLE: TableId = TableId(274);
/// `distractions_host_rules`.
pub const HOST_RULES_TABLE: TableId = TableId(275);
/// `distractions_app_rules`.
pub const APP_RULES_TABLE: TableId = TableId(276);
/// `distractions_usage_limits`.
pub const USAGE_LIMITS_TABLE: TableId = TableId(277);
/// `distractions_usage_totals`.
pub const USAGE_TOTALS_TABLE: TableId = TableId(278);

/// The device's journal of usage samples, which never replicates.
pub const USAGE_SAMPLES_LOCAL: &str = "distractions_usage_samples";

/// Owned entries of a usage limit, in display order.
pub const LIMIT_ENTRIES_OWNED: &str = "distractions_usage_limit_entries";

/// Row key of the seeded settings row.
pub const SETTINGS_KEY: &str = "vault";

/// Key prefix of a host rule, derived from its list kind, scope, and host.
pub const HOST_RULE_KEY_PREFIX: &str = "host";

/// Key prefix of an app rule, derived from its platform and identity.
pub const APP_RULE_KEY_PREFIX: &str = "app";

/// Key prefix of a usage total, derived from its device, source type, source key, and local
/// date.
pub const USAGE_TOTAL_KEY_PREFIX: &str = "usage";

/// Settings field groups.
pub mod settings_group {
    use super::{GroupId, group_id};

    /// `mode`.
    pub const MODE: GroupId = group_id(0);
    /// `enabled`.
    pub const ENABLED: GroupId = group_id(1);
    /// The four Focus phase flags.
    pub const PHASES: GroupId = group_id(2);
    /// `desktop_enabled` and the desktop phase flags.
    pub const DESKTOP: GroupId = group_id(3);
    /// `mobile_enabled` and the mobile phase flags.
    pub const MOBILE: GroupId = group_id(4);
    /// `limits_enabled`.
    pub const LIMITS: GroupId = group_id(5);
}

/// Built-in category rule field groups.
pub mod category_group {
    use super::{GroupId, group_id};

    /// `enabled`.
    pub const ENABLED: GroupId = group_id(0);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(1);
}

/// Custom category field groups.
pub mod custom_category_group {
    use super::{GroupId, group_id};

    /// `created_at_ms`.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `enabled`.
    pub const ENABLED: GroupId = group_id(2);
}

/// Host rule field groups.
pub mod host_group {
    use super::{GroupId, group_id};

    /// `list_kind`, `scope`, and `host`, the key inputs.
    pub const RULE: GroupId = group_id(0);
    /// `category_id`, the custom category of a category host.
    pub const CATEGORY: GroupId = group_id(1);
    /// `enabled`.
    pub const ENABLED: GroupId = group_id(2);
    /// `created_at_ms`.
    pub const CREATED: GroupId = group_id(3);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(4);
}

/// App rule field groups.
pub mod app_group {
    use super::{GroupId, group_id};

    /// `platform` and `identity`, the key inputs.
    pub const RULE: GroupId = group_id(0);
    /// `display_name` and `match_names_json`.
    pub const LABEL: GroupId = group_id(1);
    /// `enabled`.
    pub const ENABLED: GroupId = group_id(2);
    /// `created_at_ms`.
    pub const CREATED: GroupId = group_id(3);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(4);
}

/// Usage limit field groups.
pub mod limit_group {
    use super::{GroupId, group_id};

    /// `created_at_ms`.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `enabled`.
    pub const ENABLED: GroupId = group_id(2);
    /// `minutes_per_day` and `minutes_per_week`.
    pub const BUDGET: GroupId = group_id(3);
    /// Owned entries.
    pub const ENTRIES: GroupId = group_id(4);
}

/// Usage total field groups.
pub mod usage_total_group {
    use super::{GroupId, group_id};

    /// `device_id`, `source_type`, `source_key`, and `local_date`, the key inputs.
    pub const BUCKET: GroupId = group_id(0);
    /// `elapsed_seconds`.
    pub const ELAPSED: GroupId = group_id(1);
    /// `display_name`.
    pub const LABEL: GroupId = group_id(2);
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

/// A recoverable register.
const fn register(id: GroupId, name: &'static str, columns: &'static [ColumnSpec]) -> GroupSpec {
    GroupSpec {
        recoverable: true,
        ..group(id, name, MergeKind::Register, columns)
    }
}

/// A recoverable coupled group.
const fn coupled(id: GroupId, name: &'static str, columns: &'static [ColumnSpec]) -> GroupSpec {
    GroupSpec {
        recoverable: true,
        ..group(id, name, MergeKind::Coupled, columns)
    }
}

/// The existence register of a derived-key table.
const fn exists(id: GroupId) -> GroupSpec {
    group(id, "exists", MergeKind::Register, &[])
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

const fn optional_integer(name: &'static str, min: i64, max: i64) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalInteger { min, max },
    }
}

/// A positive millisecond time, the form the schema checks with `> 0`.
const fn positive_ms(name: &'static str) -> ColumnSpec {
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

/// A table keyed by `id` with no derived columns, hooks, tree, or publication; each table
/// overrides what it declares.
const BASE: TableSpec = TableSpec {
    id: SETTINGS_TABLE,
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

/// `distractions_settings`, the seeded singleton row [`SETTINGS_KEY`]. It is never deleted.
pub const SETTINGS: TableSpec = TableSpec {
    id: SETTINGS_TABLE,
    name: "distractions_settings",
    groups: &[
        register(
            settings_group::MODE,
            "mode",
            &[text("mode", MAX_ENUM_CHARS)],
        ),
        register(settings_group::ENABLED, "enabled", &[flag("enabled")]),
        coupled(
            settings_group::PHASES,
            "phases",
            &[
                flag("block_during_focus"),
                flag("block_during_short_breaks"),
                flag("block_during_long_breaks"),
                flag("pause_during_focus_pause"),
            ],
        ),
        coupled(
            settings_group::DESKTOP,
            "desktop",
            &[
                flag("desktop_enabled"),
                flag("desktop_block_during_focus"),
                flag("desktop_block_during_short_breaks"),
                flag("desktop_block_during_long_breaks"),
                flag("desktop_pause_during_focus_pause"),
            ],
        ),
        coupled(
            settings_group::MOBILE,
            "mobile",
            &[
                flag("mobile_enabled"),
                flag("mobile_block_during_focus"),
                flag("mobile_block_during_short_breaks"),
                flag("mobile_block_during_long_breaks"),
                flag("mobile_pause_during_focus_pause"),
            ],
        ),
        register(settings_group::LIMITS, "limits", &[flag("limits_enabled")]),
    ],
    ..BASE
};

/// `distractions_category_rules`, keyed by the built-in category id. A missing row means the
/// category's default.
pub const CATEGORY_RULES: TableSpec = TableSpec {
    id: CATEGORY_RULES_TABLE,
    name: "distractions_category_rules",
    groups: &[
        register(category_group::ENABLED, "enabled", &[flag("enabled")]),
        exists(category_group::EXISTS),
    ],
    derived_key: Some(DerivedKey {
        exists_group: category_group::EXISTS,
        prefix: None,
        key_inputs: &[],
    }),
    ..BASE
};

/// `distractions_custom_categories`.
pub const CUSTOM_CATEGORIES: TableSpec = TableSpec {
    id: CUSTOM_CATEGORIES_TABLE,
    name: "distractions_custom_categories",
    groups: &[
        group(
            custom_category_group::CREATED,
            "created",
            MergeKind::Immutable,
            &[positive_ms("created_at_ms")],
        ),
        register(
            custom_category_group::NAME,
            "name",
            &[trimmed("name", MAX_CATEGORY_NAME_CHARS)],
        ),
        register(
            custom_category_group::ENABLED,
            "enabled",
            &[flag("enabled")],
        ),
    ],
    ..BASE
};

/// `distractions_host_rules`. `scope` is empty for the blocked, exception, and allowed lists
/// and the custom category id for a category host, which `category_id` also references so the
/// host goes with its category.
pub const HOST_RULES: TableSpec = TableSpec {
    id: HOST_RULES_TABLE,
    name: "distractions_host_rules",
    groups: &[
        group(
            host_group::RULE,
            "rule",
            MergeKind::Immutable,
            &[
                text("list_kind", MAX_ENUM_CHARS),
                text("scope", MAX_ID_CHARS),
                trimmed("host", MAX_HOST_CHARS),
            ],
        ),
        group(
            host_group::CATEGORY,
            "category",
            MergeKind::Reference {
                table: CUSTOM_CATEGORIES_TABLE,
                on_delete: OnDelete::Cascade,
            },
            &[ColumnSpec {
                name: "category_id",
                ty: ColumnType::OptionalRowKey,
            }],
        ),
        register(host_group::ENABLED, "enabled", &[flag("enabled")]),
        group(
            host_group::CREATED,
            "created",
            MergeKind::Register,
            &[positive_ms("created_at_ms")],
        ),
        exists(host_group::EXISTS),
    ],
    derived_key: Some(DerivedKey {
        exists_group: host_group::EXISTS,
        prefix: Some(HOST_RULE_KEY_PREFIX),
        key_inputs: &["list_kind", "scope", "host"],
    }),
    ..BASE
};

/// `distractions_app_rules`. The identity is the package name on mobile and the lowercase
/// normalized name on desktop.
pub const APP_RULES: TableSpec = TableSpec {
    id: APP_RULES_TABLE,
    name: "distractions_app_rules",
    groups: &[
        group(
            app_group::RULE,
            "rule",
            MergeKind::Immutable,
            &[
                text("platform", MAX_ENUM_CHARS),
                trimmed("identity", MAX_APP_IDENTITY_CHARS),
            ],
        ),
        coupled(
            app_group::LABEL,
            "label",
            &[
                trimmed("display_name", MAX_APP_NAME_CHARS),
                text("match_names_json", MAX_MATCH_NAMES_JSON_CHARS),
            ],
        ),
        register(app_group::ENABLED, "enabled", &[flag("enabled")]),
        group(
            app_group::CREATED,
            "created",
            MergeKind::Register,
            &[positive_ms("created_at_ms")],
        ),
        exists(app_group::EXISTS),
    ],
    derived_key: Some(DerivedKey {
        exists_group: app_group::EXISTS,
        prefix: Some(APP_RULE_KEY_PREFIX),
        key_inputs: &["platform", "identity"],
    }),
    ..BASE
};

/// The entries of a usage limit in display order. The rules domain keeps every list within one
/// blob field.
pub const LIMIT_ENTRIES: OwnedTable = OwnedTable {
    table: LIMIT_ENTRIES_OWNED,
    parent_column: "limit_id",
    order: OwnedOrder::Ordinal("ordinal"),
    columns: &[
        trimmed("id", MAX_ID_CHARS),
        optional_text("name", MAX_LIMIT_NAME_CHARS),
        optional_integer("color", 0, MAX_COLOR),
        optional_text("website_host", MAX_HOST_CHARS),
        optional_text("mobile_app_name", MAX_APP_NAME_CHARS),
        optional_text("mobile_app_package", MAX_PACKAGE_CHARS),
        optional_text("desktop_app_name", MAX_APP_NAME_CHARS),
        text("desktop_app_match_names_json", MAX_MATCH_NAMES_JSON_CHARS),
    ],
    references: &[],
    unique: &[],
    max_rows: MAX_LIMIT_ENTRIES,
    codec: OwnedCodec::Generic,
    nested: &[],
    bound: OwnedBound::Domain,
};

/// `distractions_usage_limits`.
pub const USAGE_LIMITS: TableSpec = TableSpec {
    id: USAGE_LIMITS_TABLE,
    name: "distractions_usage_limits",
    groups: &[
        group(
            limit_group::CREATED,
            "created",
            MergeKind::Immutable,
            &[positive_ms("created_at_ms")],
        ),
        register(
            limit_group::NAME,
            "name",
            &[trimmed("name", MAX_LIMIT_NAME_CHARS)],
        ),
        register(limit_group::ENABLED, "enabled", &[flag("enabled")]),
        coupled(
            limit_group::BUDGET,
            "budget",
            &[
                optional_integer("minutes_per_day", 1, MAX_MINUTES_PER_DAY),
                optional_integer("minutes_per_week", 1, MAX_MINUTES_PER_WEEK),
            ],
        ),
        GroupSpec {
            id: limit_group::ENTRIES,
            name: "entries",
            kind: MergeKind::Register,
            storage: GroupStorage::Owned {
                columns: &[],
                tables: &[LIMIT_ENTRIES],
            },
            references: &[],
            surfaced: false,
            recoverable: true,
            bumps_revision: false,
        },
    ],
    ..BASE
};

/// `distractions_usage_totals`, the seconds one device spent on one source during one local day,
/// keyed by those four values under [`USAGE_TOTAL_KEY_PREFIX`]. Only the device the row names
/// writes it and its total only grows, so `Max` converges. Rows are never deleted, so the table
/// declares no existence group and the retention window is a read filter.
pub const USAGE_TOTALS: TableSpec = TableSpec {
    id: USAGE_TOTALS_TABLE,
    name: "distractions_usage_totals",
    groups: &[
        group(
            usage_total_group::BUCKET,
            "bucket",
            MergeKind::Immutable,
            &[
                trimmed("device_id", MAX_DEVICE_ID_BYTES),
                text("source_type", MAX_ENUM_CHARS),
                trimmed("source_key", MAX_USAGE_SOURCE_KEY_CHARS),
                text("local_date", USAGE_DATE_CHARS),
            ],
        ),
        group(
            usage_total_group::ELAPSED,
            "elapsed",
            MergeKind::Max,
            &[ColumnSpec {
                name: "elapsed_seconds",
                ty: ColumnType::Integer {
                    min: 1,
                    max: MAX_USAGE_DAY_SECONDS,
                },
            }],
        ),
        group(
            usage_total_group::LABEL,
            "label",
            MergeKind::Register,
            &[optional_text("display_name", MAX_USAGE_LABEL_CHARS)],
        ),
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

/// The distraction rules and usage domain.
pub const DOMAIN: DomainManifest = DomainManifest {
    name: DOMAIN_NAME,
    table_ids: TableIdRange {
        first: 272,
        last: 287,
    },
    tables: &[
        SETTINGS,
        CATEGORY_RULES,
        CUSTOM_CATEGORIES,
        HOST_RULES,
        APP_RULES,
        USAGE_LIMITS,
        USAGE_TOTALS,
    ],
    derived: &[],
    local: &[device(USAGE_SAMPLES_LOCAL)],
    engine: &[],
    replica_writable: true,
    assets: &[],
};
