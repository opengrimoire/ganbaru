//! A test-only fixture domain beside the vault domains, so engine behavior that no vault domain
//! uses yet runs through the simulation: `Local` tables and columns, every trigger policy, a
//! cascading reference cycle, a group with several reference columns, an immutable reference,
//! a reference into its own table, and a derived-key table with a composite primary key.
//! Fixture table ids start at [`FIXTURE_TABLE_IDS`], above every vault domain.

use std::sync::LazyLock;

use ganbaru_sync_contracts::{TableId, derived_key};
use sqlx::SqlitePool;

use super::{execute, migrated_pool};
use crate::domains::vault_adapters;
use crate::manifest::vault::{VAULT_MANIFEST_VERSION, quick_notes};
use crate::manifest::{
    Collation, ColumnSpec, ColumnType, DerivedKey, DeviceTable, DomainManifest, DomainTrigger,
    GroupSpec, GroupStorage, LocalColumn, Manifest, MergeKind, OnDelete, ReferenceColumn,
    Resolution, SYNC_KEY_COLUMN, TableIdRange, TableSpec, TriggerPolicy, group_id,
};
use crate::triggers::{self, NOT_APPLYING};
use crate::{Engine, Hooks};

/// First table id of the test-only fixture domains; vault domains stay below it.
pub(super) const FIXTURE_TABLE_IDS: u16 = 60_000;

/// Replicated fixture items. `label_edits` counts label changes on this device only.
pub(super) const ITEMS: TableSpec = TableSpec {
    id: ITEMS_TABLE,
    name: "fixture_items",
    key_column: "id",
    groups: &[
        GroupSpec {
            id: group_id(0),
            name: "created",
            kind: MergeKind::Immutable,
            storage: GroupStorage::Columns(&[ColumnSpec {
                name: "created_at",
                ty: ColumnType::Timestamp,
            }]),
            references: &[],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
        GroupSpec {
            id: group_id(1),
            name: "label",
            kind: MergeKind::Register,
            storage: GroupStorage::Columns(&[ColumnSpec {
                name: "label",
                ty: ColumnType::Text { max_chars: 80 },
            }]),
            references: &[],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
    ],
    local_columns: &[LocalColumn {
        name: "label_edits",
        materialize: "0",
    }],
    derived_columns: &[],
    revision_column: None,
    resolutions: &[],
    redirects: false,
    adapter: None,
    triggers: &[
        DomainTrigger {
            name: "fixture_items_count_label_edits",
            policy: TriggerPolicy::SameRow,
        },
        DomainTrigger {
            name: "fixture_items_mark_local_creates",
            policy: TriggerPolicy::Gated,
        },
        DomainTrigger {
            name: "fixture_items_index_insert",
            policy: TriggerPolicy::DerivedIndex,
        },
        DomainTrigger {
            name: "fixture_items_index_update",
            policy: TriggerPolicy::DerivedIndex,
        },
        DomainTrigger {
            name: "fixture_items_index_delete",
            policy: TriggerPolicy::DerivedIndex,
        },
    ],
    derived_key: None,
    tree: None,
    acyclic: None,
};

/// `fixture_items`.
pub(super) const ITEMS_TABLE: TableId = TableId(FIXTURE_TABLE_IDS);
/// `fixture_cycle_a`.
pub(super) const CYCLE_A_TABLE: TableId = TableId(FIXTURE_TABLE_IDS + 1);
/// `fixture_cycle_b`.
pub(super) const CYCLE_B_TABLE: TableId = TableId(FIXTURE_TABLE_IDS + 2);
/// `fixture_links`.
pub(super) const LINKS_TABLE: TableId = TableId(FIXTURE_TABLE_IDS + 3);
/// `fixture_memberships`.
pub(super) const MEMBERSHIPS_TABLE: TableId = TableId(FIXTURE_TABLE_IDS + 4);

const LABEL: GroupSpec = GroupSpec {
    id: group_id(0),
    name: "label",
    kind: MergeKind::Register,
    storage: GroupStorage::Columns(&[ColumnSpec {
        name: "label",
        ty: ColumnType::Text { max_chars: 40 },
    }]),
    references: &[],
    surfaced: false,
    recoverable: false,
    bumps_revision: false,
};

/// A group holding one reference column of its own name.
const fn reference(name: &'static str, table: TableId, on_delete: OnDelete) -> GroupSpec {
    GroupSpec {
        id: group_id(1),
        name,
        kind: MergeKind::Reference { table, on_delete },
        storage: GroupStorage::Columns(&[ColumnSpec {
            name: "peer_id",
            ty: ColumnType::OptionalRowKey,
        }]),
        references: &[],
        surfaced: false,
        recoverable: false,
        bumps_revision: false,
    }
}

/// One table of a reference cycle with [`CYCLE_B`]; deleting a row deletes the rows that
/// reference it. Labels are unique, so duplicates redirect to the lowest key.
pub(super) const CYCLE_A: TableSpec = TableSpec {
    id: CYCLE_A_TABLE,
    name: "fixture_cycle_a",
    key_column: "id",
    groups: &[LABEL, reference("peer", CYCLE_B_TABLE, OnDelete::Cascade)],
    local_columns: &[],
    derived_columns: &[],
    revision_column: None,
    resolutions: &[Resolution::DuplicateRepair {
        columns: &["label"],
        collation: Collation::Binary,
    }],
    redirects: true,
    adapter: None,
    triggers: &[],
    derived_key: None,
    tree: None,
    acyclic: None,
};

/// The other table of the reference cycle with [`CYCLE_A`].
pub(super) const CYCLE_B: TableSpec = TableSpec {
    id: CYCLE_B_TABLE,
    name: "fixture_cycle_b",
    groups: &[LABEL, reference("peer", CYCLE_A_TABLE, OnDelete::Cascade)],
    resolutions: &[],
    redirects: false,
    ..CYCLE_A
};

/// Links with an immutable owner item, a register group referencing both cycle tables, and a
/// parent link in the same table.
pub(super) const LINKS: TableSpec = TableSpec {
    id: LINKS_TABLE,
    name: "fixture_links",
    key_column: "id",
    groups: &[
        GroupSpec {
            id: group_id(0),
            name: "owner",
            kind: MergeKind::Immutable,
            storage: GroupStorage::Columns(&[ColumnSpec {
                name: "owner_id",
                ty: ColumnType::OptionalRowKey,
            }]),
            references: &[ReferenceColumn {
                column: "owner_id",
                table: ITEMS_TABLE,
                on_delete: OnDelete::Cascade,
            }],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
        GroupSpec {
            id: group_id(1),
            name: "ends",
            kind: MergeKind::Register,
            storage: GroupStorage::Columns(&[
                ColumnSpec {
                    name: "note",
                    ty: ColumnType::Text { max_chars: 40 },
                },
                ColumnSpec {
                    name: "left_id",
                    ty: ColumnType::OptionalRowKey,
                },
                ColumnSpec {
                    name: "right_id",
                    ty: ColumnType::OptionalRowKey,
                },
            ]),
            references: &[
                ReferenceColumn {
                    column: "left_id",
                    table: CYCLE_A_TABLE,
                    on_delete: OnDelete::SetNull,
                },
                ReferenceColumn {
                    column: "right_id",
                    table: CYCLE_B_TABLE,
                    on_delete: OnDelete::SetNull,
                },
            ],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
        GroupSpec {
            id: group_id(2),
            name: "parent",
            kind: MergeKind::Reference {
                table: LINKS_TABLE,
                on_delete: OnDelete::SetNull,
            },
            storage: GroupStorage::Columns(&[ColumnSpec {
                name: "parent_id",
                ty: ColumnType::OptionalRowKey,
            }]),
            references: &[],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
    ],
    local_columns: &[],
    derived_columns: &[],
    revision_column: None,
    resolutions: &[],
    redirects: false,
    adapter: None,
    triggers: &[],
    derived_key: None,
    tree: None,
    acyclic: None,
};

/// Prefix of [`MEMBERSHIPS`] keys.
const MEMBERSHIP_KEY_PREFIX: &str = "fixture_membership";

/// The `sync_key` of the membership of `item` in `end`.
pub(super) fn membership_key(item: &str, end: &str) -> String {
    derived_key(MEMBERSHIP_KEY_PREFIX, &[item, end])
        .expect("fixture ids are valid key parts")
        .into_string()
}

/// Group ids of [`MEMBERSHIPS`].
pub(super) mod membership_group {
    use ganbaru_sync_contracts::GroupId;

    use crate::manifest::group_id;

    /// The primary key columns.
    pub(in crate::tests) const IDENTITY: GroupId = group_id(0);
    /// Creation time and role.
    pub(in crate::tests) const DETAILS: GroupId = group_id(1);
    /// Existence register.
    pub(in crate::tests) const EXISTS: GroupId = group_id(2);
}

/// The identity group of [`MEMBERSHIPS`]: its composite primary key, both columns cascading
/// references that are never redirected.
pub(super) const MEMBERSHIP_IDENTITY: GroupSpec = GroupSpec {
    id: membership_group::IDENTITY,
    name: "identity",
    kind: MergeKind::Immutable,
    storage: GroupStorage::Columns(&[
        ColumnSpec {
            name: "item_id",
            ty: ColumnType::OptionalRowKey,
        },
        ColumnSpec {
            name: "end_id",
            ty: ColumnType::OptionalRowKey,
        },
    ]),
    references: &[
        ReferenceColumn {
            column: "item_id",
            table: ITEMS_TABLE,
            on_delete: OnDelete::Cascade,
        },
        ReferenceColumn {
            column: "end_id",
            table: CYCLE_A_TABLE,
            on_delete: OnDelete::Cascade,
        },
    ],
    surfaced: false,
    recoverable: false,
    bumps_revision: false,
};

/// The details group of [`MEMBERSHIPS`].
pub(super) const MEMBERSHIP_DETAILS: GroupSpec = GroupSpec {
    id: membership_group::DETAILS,
    name: "details",
    kind: MergeKind::Register,
    storage: GroupStorage::Columns(&[
        ColumnSpec {
            name: "created_at",
            ty: ColumnType::Timestamp,
        },
        ColumnSpec {
            name: "role",
            ty: ColumnType::Text { max_chars: 20 },
        },
    ]),
    references: &[],
    surfaced: false,
    recoverable: false,
    bumps_revision: false,
};

/// Memberships of an item in a cycle row: a composite primary key keyed by `sync_key`, and a
/// derived key, so a removed membership can be added again.
pub(super) const MEMBERSHIPS: TableSpec = TableSpec {
    id: MEMBERSHIPS_TABLE,
    name: "fixture_memberships",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        MEMBERSHIP_IDENTITY,
        MEMBERSHIP_DETAILS,
        GroupSpec {
            id: membership_group::EXISTS,
            name: "exists",
            kind: MergeKind::Register,
            storage: GroupStorage::Columns(&[]),
            references: &[],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
    ],
    local_columns: &[],
    derived_columns: &[],
    revision_column: None,
    resolutions: &[],
    redirects: false,
    adapter: None,
    triggers: &[],
    derived_key: Some(DerivedKey {
        exists_group: membership_group::EXISTS,
        key_inputs: &[],
    }),
    tree: None,
    acyclic: None,
};

/// The fixture domain.
pub(super) const FIXTURE_DOMAIN: DomainManifest = DomainManifest {
    name: "fixture",
    table_ids: TableIdRange {
        first: FIXTURE_TABLE_IDS,
        last: FIXTURE_TABLE_IDS + 15,
    },
    tables: &[ITEMS, CYCLE_A, CYCLE_B, LINKS, MEMBERSHIPS],
    derived: &["fixture_items_index"],
    local: &[DeviceTable {
        name: "fixture_item_marks",
        triggers: &[DomainTrigger {
            name: "fixture_item_marks_refuse_negative",
            policy: TriggerPolicy::ApplierVisible,
        }],
    }],
    engine: &[],
    replica_writable: true,
};

/// The vault domains followed by the fixture domain.
pub(super) const TEST_DOMAINS: &[&DomainManifest] = &[&quick_notes::DOMAIN, &FIXTURE_DOMAIN];

/// The manifest of [`TEST_DOMAINS`].
pub(super) static TEST_MANIFEST: LazyLock<Manifest> =
    LazyLock::new(|| Manifest::from_domains(VAULT_MANIFEST_VERSION, TEST_DOMAINS));

/// An engine for [`TEST_MANIFEST`] with the vault adapters.
pub(super) fn test_engine() -> Engine {
    Engine::with(&TEST_MANIFEST, vault_adapters(), Hooks::default())
        .expect("the test manifest and the vault adapters should agree")
}

/// Schema of the fixture domain. `label_edits` has no default, so every insert must name it and
/// a remote create only succeeds through its materialization value.
fn fixture_schema() -> String {
    format!(
        "CREATE TABLE fixture_items (
             id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
             created_at TEXT NOT NULL CHECK (trim(created_at) <> ''),
             label TEXT NOT NULL CHECK (length(label) <= 80),
             label_edits INTEGER NOT NULL CHECK (label_edits >= 0)
         );
         CREATE TABLE fixture_item_marks (
             item_id TEXT PRIMARY KEY REFERENCES fixture_items(id) ON DELETE CASCADE,
             mark INTEGER NOT NULL
         );
         CREATE TABLE fixture_cycle_a (
             id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
             label TEXT NOT NULL CHECK (length(label) <= 40),
             peer_id TEXT REFERENCES fixture_cycle_b(id) ON DELETE CASCADE,
             UNIQUE (label)
         );
         CREATE TABLE fixture_cycle_b (
             id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
             label TEXT NOT NULL CHECK (length(label) <= 40),
             peer_id TEXT REFERENCES fixture_cycle_a(id) ON DELETE CASCADE
         );
         CREATE TABLE fixture_links (
             id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
             owner_id TEXT REFERENCES fixture_items(id) ON DELETE CASCADE,
             note TEXT NOT NULL CHECK (length(note) <= 40),
             left_id TEXT REFERENCES fixture_cycle_a(id) ON DELETE SET NULL,
             right_id TEXT REFERENCES fixture_cycle_b(id) ON DELETE SET NULL,
             parent_id TEXT REFERENCES fixture_links(id) ON DELETE SET NULL
         );
         CREATE TABLE fixture_memberships (
             item_id TEXT NOT NULL REFERENCES fixture_items(id) ON DELETE CASCADE,
             end_id TEXT NOT NULL REFERENCES fixture_cycle_a(id) ON DELETE CASCADE,
             sync_key TEXT NOT NULL UNIQUE,
             created_at TEXT NOT NULL,
             role TEXT NOT NULL CHECK (length(role) <= 20),
             PRIMARY KEY (item_id, end_id)
         );
         CREATE TABLE fixture_items_index (
             item_id TEXT PRIMARY KEY,
             label TEXT NOT NULL
         );
         CREATE TRIGGER fixture_items_count_label_edits AFTER UPDATE OF label ON fixture_items
         BEGIN
             UPDATE fixture_items SET label_edits = label_edits + 1 WHERE id = NEW.id;
         END;
         CREATE TRIGGER fixture_items_mark_local_creates AFTER INSERT ON fixture_items
         WHEN {NOT_APPLYING}
         BEGIN
             INSERT INTO fixture_item_marks (item_id, mark) VALUES (NEW.id, 1);
         END;
         CREATE TRIGGER fixture_item_marks_refuse_negative BEFORE INSERT ON fixture_item_marks
         WHEN NEW.mark < 0
         BEGIN
             SELECT RAISE(ABORT, 'fixture marks are never negative');
         END;
         CREATE TRIGGER fixture_items_index_insert AFTER INSERT ON fixture_items
         BEGIN
             INSERT INTO fixture_items_index (item_id, label) VALUES (NEW.id, NEW.label);
         END;
         CREATE TRIGGER fixture_items_index_update AFTER UPDATE OF label ON fixture_items
         BEGIN
             UPDATE fixture_items_index SET label = NEW.label WHERE item_id = NEW.id;
         END;
         CREATE TRIGGER fixture_items_index_delete AFTER DELETE ON fixture_items
         BEGIN
             DELETE FROM fixture_items_index WHERE item_id = OLD.id;
         END;"
    )
}

/// A freshly migrated vault database with the fixture domain's schema and the capture triggers
/// of [`TEST_MANIFEST`] installed.
pub(super) async fn fixture_pool() -> SqlitePool {
    let pool = migrated_pool().await;
    execute(&pool, &fixture_schema()).await;
    let installed: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type = 'trigger'")
            .fetch_all(&pool)
            .await
            .unwrap();
    for trigger in triggers::render(&TEST_MANIFEST) {
        if !installed.contains(&trigger.name) {
            execute(&pool, &trigger.sql).await;
        }
    }
    pool
}
