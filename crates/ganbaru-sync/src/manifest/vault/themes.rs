//! User themes: `themes` (table 17), whose live and seed tokens and event palettes are owned
//! child tables.

use ganbaru_sync_contracts::{GroupId, TableId};

use crate::domains::themes::{
    MAX_CALENDAR_MODE_CHARS, MAX_COLOR_CHARS, MAX_DISPLAY_NAME_CHARS, MAX_ICON_LABEL_CHARS,
    MAX_TOKEN_KEY_CHARS, MAX_TOKEN_KIND_CHARS, MAX_TOKENS, PALETTE_SIZE,
};
use crate::manifest::{
    ColumnSpec, ColumnType, DomainManifest, GroupSpec, GroupStorage, MergeKind, OwnedBound,
    OwnedCodec, OwnedOrder, OwnedTable, TableIdRange, TableSpec, group_id,
};

/// Domain name for user themes.
pub const DOMAIN_NAME: &str = "themes";

/// Domain adapter name for user themes.
pub const ADAPTER: &str = "themes";

/// `themes`.
pub const THEMES_TABLE: TableId = TableId(17);
/// Owned live tokens of a theme.
pub const TOKENS_OWNED: &str = "theme_tokens";
/// Owned live event palette of a theme.
pub const PALETTE_OWNED: &str = "theme_event_palette";
/// Owned seed tokens of a theme, the reset target fixed at creation.
pub const SEED_TOKENS_OWNED: &str = "theme_seed_tokens";
/// Owned seed event palette of a theme, the reset target fixed at creation.
pub const SEED_PALETTE_OWNED: &str = "theme_seed_event_palette";

/// Theme field groups.
pub mod group {
    use super::{GroupId, group_id};

    /// `created_at_ms`.
    pub const CREATED: GroupId = group_id(0);
    /// The seed columns and owned seed tokens and palette, fixed when the theme is created.
    pub const SEED: GroupId = group_id(1);
    /// `display_name`.
    pub const NAME: GroupId = group_id(2);
    /// `icon_label`.
    pub const ICON: GroupId = group_id(3);
    /// Blend canvas, derivation engine version, Calendar default, and the owned live tokens
    /// and palette, which the editor saves together.
    pub const CONTENT: GroupId = group_id(4);
    /// `upgrade_dismissed_engine_version`.
    pub const DISMISSAL: GroupId = group_id(5);
    /// `updated_at_ms`.
    pub const UPDATED: GroupId = group_id(6);
}

const fn group(
    id: GroupId,
    name: &'static str,
    kind: MergeKind,
    storage: GroupStorage,
    surfaced: bool,
    recoverable: bool,
) -> GroupSpec {
    GroupSpec {
        id,
        name,
        kind,
        storage,
        references: &[],
        surfaced,
        recoverable,
        bumps_revision: false,
    }
}

const fn text(name: &'static str, max_chars: usize) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Text { max_chars },
    }
}

const fn non_negative(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Integer {
            min: 0,
            max: i64::MAX,
        },
    }
}

const TOKEN_COLUMNS: [ColumnSpec; 4] = [
    text("kind", MAX_TOKEN_KIND_CHARS),
    text("key", MAX_TOKEN_KEY_CHARS),
    text("value", MAX_COLOR_CHARS),
    ColumnSpec {
        name: "isolated",
        ty: ColumnType::Flag,
    },
];

const PALETTE_COLUMNS: [ColumnSpec; 2] = [
    ColumnSpec {
        name: "slot",
        ty: ColumnType::Integer {
            min: 0,
            max: PALETTE_SIZE as i64 - 1,
        },
    },
    text("value", MAX_COLOR_CHARS),
];

const fn tokens(table: &'static str) -> OwnedTable {
    OwnedTable {
        table,
        parent_column: "theme_id",
        order: OwnedOrder::Key(&["kind", "key"]),
        columns: &TOKEN_COLUMNS,
        references: &[],
        unique: &[],
        max_rows: MAX_TOKENS,
        codec: OwnedCodec::Custom,
        nested: &[],
        bound: OwnedBound::Static,
    }
}

const fn palette(table: &'static str) -> OwnedTable {
    OwnedTable {
        table,
        parent_column: "theme_id",
        order: OwnedOrder::Key(&["slot"]),
        columns: &PALETTE_COLUMNS,
        references: &[],
        unique: &[],
        max_rows: PALETTE_SIZE,
        codec: OwnedCodec::Custom,
        nested: &[],
        bound: OwnedBound::Static,
    }
}

const GROUPS: [GroupSpec; 7] = [
    group(
        group::CREATED,
        "created",
        MergeKind::Immutable,
        GroupStorage::Columns(&[non_negative("created_at_ms")]),
        false,
        false,
    ),
    group(
        group::SEED,
        "seed",
        MergeKind::Immutable,
        GroupStorage::Owned {
            columns: &[
                text("seed_blend_canvas", MAX_COLOR_CHARS),
                text("seed_icon_label", MAX_ICON_LABEL_CHARS),
                text("seed_calendar_default_mode", MAX_CALENDAR_MODE_CHARS),
                text("seed_calendar_default_custom", MAX_COLOR_CHARS),
            ],
            tables: &[tokens(SEED_TOKENS_OWNED), palette(SEED_PALETTE_OWNED)],
        },
        false,
        false,
    ),
    group(
        group::NAME,
        "name",
        MergeKind::Register,
        GroupStorage::Columns(&[ColumnSpec {
            name: "display_name",
            ty: ColumnType::TrimmedText {
                min_chars: 1,
                max_chars: MAX_DISPLAY_NAME_CHARS,
            },
        }]),
        false,
        true,
    ),
    group(
        group::ICON,
        "icon",
        MergeKind::Register,
        GroupStorage::Columns(&[text("icon_label", MAX_ICON_LABEL_CHARS)]),
        false,
        true,
    ),
    group(
        group::CONTENT,
        "content",
        MergeKind::Register,
        GroupStorage::Owned {
            columns: &[
                text("blend_canvas", MAX_COLOR_CHARS),
                non_negative("derivation_engine_version"),
                text("calendar_default_mode", MAX_CALENDAR_MODE_CHARS),
                text("calendar_default_custom", MAX_COLOR_CHARS),
            ],
            tables: &[tokens(TOKENS_OWNED), palette(PALETTE_OWNED)],
        },
        true,
        true,
    ),
    group(
        group::DISMISSAL,
        "dismissal",
        MergeKind::Max,
        GroupStorage::Columns(&[non_negative("upgrade_dismissed_engine_version")]),
        false,
        false,
    ),
    group(
        group::UPDATED,
        "updated",
        MergeKind::Max,
        GroupStorage::Columns(&[non_negative("updated_at_ms")]),
        false,
        false,
    ),
];

/// `themes`. A theme's seed is written once at creation and never changes, so concurrent saves
/// on two devices conflict only in the content group, which a person resolves by picking one
/// version.
pub const THEMES: TableSpec = TableSpec {
    id: THEMES_TABLE,
    name: "themes",
    key_column: "id",
    groups: &GROUPS,
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

/// The user themes domain, the rest of the 16 to 31 range after portable preferences.
pub const DOMAIN: DomainManifest = DomainManifest {
    name: DOMAIN_NAME,
    table_ids: TableIdRange {
        first: 17,
        last: 31,
    },
    tables: &[THEMES],
    derived: &[],
    local: &[],
    engine: &[],
    replica_writable: true,
    unconverted: &[],
    assets: &[],
};
