//! Portable preferences: `vault_preferences` (table 16), one row per preference key.

use ganbaru_sync_contracts::{GroupId, TableId};

use crate::assets::AssetClass;
use crate::domains::preferences::{MAX_PROFILE_IMAGE_BYTES, MAX_VALUE_BYTES};
use crate::manifest::{
    ColumnSpec, ColumnType, DerivedKey, DomainManifest, GroupSpec, GroupStorage, MergeKind,
    TableIdRange, TableSpec, group_id,
};

/// Domain name for portable preferences.
pub const DOMAIN_NAME: &str = "preferences";

/// Domain adapter name for portable preferences.
pub const ADAPTER: &str = "preferences";

/// `vault_preferences`.
pub const PREFERENCES_TABLE: TableId = TableId(16);

/// Preference field groups.
pub mod group {
    use super::{GroupId, group_id};

    /// `value`, the canonical JSON text of the preference.
    pub const VALUE: GroupId = group_id(0);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(1);
    /// Existence register of the derived-key row.
    pub const EXISTS: GroupId = group_id(2);
}

const fn plain(
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

const GROUPS: [GroupSpec; 3] = [
    plain(
        group::VALUE,
        "value",
        MergeKind::Register,
        &[ColumnSpec {
            name: "value",
            ty: ColumnType::Text {
                max_chars: MAX_VALUE_BYTES,
            },
        }],
    ),
    plain(
        group::UPDATED,
        "updated",
        MergeKind::Max,
        &[ColumnSpec {
            name: "updated_at",
            ty: ColumnType::Timestamp,
        }],
    ),
    plain(group::EXISTS, "exists", MergeKind::Register, &[]),
];

/// `vault_preferences`. The preference key is the row key, so a key removed on one device can
/// be set again on another; values merge as whole registers and never surface conflicts.
pub const PREFERENCES: TableSpec = TableSpec {
    id: PREFERENCES_TABLE,
    name: "vault_preferences",
    key_column: "key",
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
    derived_key: Some(DerivedKey {
        exists_group: group::EXISTS,
        prefix: None,
        key_inputs: &[],
    }),
    tree: None,
    acyclic: None,
    publish_when: None,
    published_with: None,
    key_parent: None,
    holds_deletes: false,
};

/// Profile pictures that the profile image preference references.
pub const PROFILE_CLASS: AssetClass = AssetClass {
    name: "profile",
    max_bytes: MAX_PROFILE_IMAGE_BYTES,
};

/// The portable preferences domain. Themes take the rest of the 16 to 31 range.
pub const DOMAIN: DomainManifest = DomainManifest {
    name: DOMAIN_NAME,
    table_ids: TableIdRange {
        first: 16,
        last: 16,
    },
    tables: &[PREFERENCES],
    derived: &[],
    local: &[],
    engine: &[],
    unconverted: &[],
    assets: &[PROFILE_CLASS],
};
