//! A test-only fixture domain beside the vault domains, so engine behavior that no vault domain
//! uses yet runs through the simulation: `Local` tables and columns, and every trigger policy.
//! Fixture table ids start at [`FIXTURE_TABLE_IDS`], above every vault domain.

use std::sync::LazyLock;

use ganbaru_sync_contracts::TableId;
use sqlx::SqlitePool;

use super::{execute, migrated_pool};
use crate::domains::vault_adapters;
use crate::manifest::vault::{VAULT_MANIFEST_VERSION, quick_notes};
use crate::manifest::{
    ColumnSpec, ColumnType, DeviceTable, DomainManifest, DomainTrigger, GroupSpec, GroupStorage,
    LocalColumn, Manifest, MergeKind, TableIdRange, TableSpec, TriggerPolicy, group_id,
};
use crate::triggers::{self, NOT_APPLYING};
use crate::{Engine, Hooks};

/// First table id of the test-only fixture domains; vault domains stay below it.
pub(super) const FIXTURE_TABLE_IDS: u16 = 60_000;

/// Replicated fixture items. `label_edits` counts label changes on this device only.
const ITEMS: TableSpec = TableSpec {
    id: TableId(FIXTURE_TABLE_IDS),
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
};

/// The fixture domain.
pub(super) const FIXTURE_DOMAIN: DomainManifest = DomainManifest {
    name: "fixture",
    table_ids: TableIdRange {
        first: FIXTURE_TABLE_IDS,
        last: FIXTURE_TABLE_IDS + 15,
    },
    tables: &[ITEMS],
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
