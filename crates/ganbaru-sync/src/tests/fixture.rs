//! A test-only fixture domain beside the vault domains, so engine behavior that no vault domain
//! uses yet runs through the simulation: `Local` tables and columns, every trigger policy, a
//! cascading reference cycle, a group with several reference columns, an immutable reference,
//! a reference into its own table, a derived-key table with a composite primary key, and a tree
//! of nodes with a maximum depth whose parent cascades, with a recoverable label, an acyclic
//! edge set between items, drafts that publish by predicate with parts published with them,
//! and memos whose deletes can be held, with lines and derived-key tags that inherit the hold,
//! lists with generic owned entries referencing memberships and a hook-maintained entry count,
//! and boards whose coupled group owns cards with nested checks beside a reference column.
//! Fixture table ids start at [`FIXTURE_TABLE_IDS`], above every vault domain.

use std::sync::LazyLock;

use ganbaru_sync_contracts::{Field, GroupId, TableId, Value, derived_key};
use sqlx::{AssertSqlSafe, SqliteConnection, SqlitePool};

use super::{execute, migrated_pool};
use crate::adapter::{AdapterError, Adapters, DomainAdapter, Presentation};
use crate::assets::AssetClass;
use crate::domains::vault_adapters;
use crate::hooks::{ChangeSink, HookFuture, MaterializeHook};
use crate::manifest::vault::{VAULT_MANIFEST_VERSION, quick_notes};
use crate::manifest::{
    AcyclicSpec, Collation, ColumnSpec, ColumnType, DerivedKey, DeviceTable, DomainManifest,
    DomainTrigger, GroupSpec, GroupStorage, KeyDerivedColumn, LocalColumn, Manifest, MergeKind,
    OnDelete, OwnedCodec, OwnedOrder, OwnedReference, OwnedTable, PublishedWith, ReferenceColumn,
    Resolution, SYNC_KEY_COLUMN, TableIdRange, TableSpec, TreeSpec, TriggerPolicy, group_id,
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
    hook_columns: &[],
    key_derived_columns: &[],
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
    publish_when: None,
    published_with: None,
    holds_deletes: false,
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
    hook_columns: &[],
    key_derived_columns: &[],
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
    publish_when: None,
    published_with: None,
    holds_deletes: false,
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
    hook_columns: &[],
    key_derived_columns: &[],
    revision_column: None,
    resolutions: &[],
    redirects: false,
    adapter: None,
    triggers: &[],
    derived_key: None,
    tree: None,
    acyclic: None,
    publish_when: None,
    published_with: None,
    holds_deletes: false,
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

/// The details group of [`MEMBERSHIPS`], recoverable so a role edited while its item is
/// deleted is offered with the item.
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
    recoverable: true,
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
    hook_columns: &[],
    key_derived_columns: &[],
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
    publish_when: None,
    published_with: None,
    holds_deletes: false,
};

/// Group ids of [`NODES`].
pub(super) mod node_group {
    use ganbaru_sync_contracts::GroupId;

    use crate::manifest::group_id;

    /// The parent node.
    pub(in crate::tests) const PARENT: GroupId = group_id(0);
    /// The position among siblings.
    pub(in crate::tests) const POSITION: GroupId = group_id(1);
    /// The recoverable label.
    pub(in crate::tests) const LABEL: GroupId = group_id(2);
}

/// Deepest a node may nest: a root has depth 0.
pub(super) const NODE_MAX_DEPTH: u8 = 2;

/// A tree of nodes whose parent cascades its deletion to their children.
pub(super) const NODES: TableSpec = TableSpec {
    id: NODES_TABLE,
    name: "fixture_nodes",
    key_column: "id",
    groups: &[
        GroupSpec {
            id: node_group::PARENT,
            name: "parent",
            kind: MergeKind::Reference {
                table: NODES_TABLE,
                on_delete: OnDelete::Cascade,
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
        GroupSpec {
            id: node_group::POSITION,
            name: "position",
            kind: MergeKind::Position,
            storage: GroupStorage::Columns(&[ColumnSpec {
                name: "position",
                ty: ColumnType::OrderKey,
            }]),
            references: &[],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
        GroupSpec {
            id: node_group::LABEL,
            name: "label",
            kind: MergeKind::Register,
            storage: GroupStorage::Columns(&[ColumnSpec {
                name: "label",
                ty: ColumnType::Text { max_chars: 40 },
            }]),
            references: &[],
            surfaced: false,
            recoverable: true,
            bumps_revision: false,
        },
    ],
    local_columns: &[],
    derived_columns: &[],
    hook_columns: &[],
    key_derived_columns: &[],
    revision_column: None,
    resolutions: &[],
    redirects: false,
    adapter: None,
    triggers: &[],
    derived_key: None,
    tree: Some(TreeSpec {
        parent_group: node_group::PARENT,
        position_group: node_group::POSITION,
        max_depth: Some(NODE_MAX_DEPTH),
    }),
    acyclic: None,
    publish_when: None,
    published_with: None,
    holds_deletes: false,
};

/// `fixture_nodes`.
pub(super) const NODES_TABLE: TableId = TableId(FIXTURE_TABLE_IDS + 5);

/// Prefix of [`EDGES`] keys.
const EDGE_KEY_PREFIX: &str = "fixture_edge";

/// The `sync_key` of the edge from `from` to `to`.
pub(super) fn edge_key(from: &str, to: &str) -> String {
    derived_key(EDGE_KEY_PREFIX, &[from, to])
        .expect("fixture ids are valid key parts")
        .into_string()
}

/// Group ids of [`EDGES`].
pub(super) mod edge_group {
    use ganbaru_sync_contracts::GroupId;

    use crate::manifest::group_id;

    /// The primary key columns.
    pub(in crate::tests) const IDENTITY: GroupId = group_id(0);
    /// Existence register.
    pub(in crate::tests) const EXISTS: GroupId = group_id(1);
}

/// Directed edges between items that never form a cycle, such as task dependencies.
pub(super) const EDGES: TableSpec = TableSpec {
    id: EDGES_TABLE,
    name: "fixture_edges",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        GroupSpec {
            id: edge_group::IDENTITY,
            name: "identity",
            kind: MergeKind::Immutable,
            storage: GroupStorage::Columns(&[
                ColumnSpec {
                    name: "from_id",
                    ty: ColumnType::OptionalRowKey,
                },
                ColumnSpec {
                    name: "to_id",
                    ty: ColumnType::OptionalRowKey,
                },
            ]),
            references: &[
                ReferenceColumn {
                    column: "from_id",
                    table: ITEMS_TABLE,
                    on_delete: OnDelete::Cascade,
                },
                ReferenceColumn {
                    column: "to_id",
                    table: ITEMS_TABLE,
                    on_delete: OnDelete::Cascade,
                },
            ],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
        GroupSpec {
            id: edge_group::EXISTS,
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
    hook_columns: &[],
    key_derived_columns: &[],
    revision_column: None,
    resolutions: &[],
    redirects: false,
    adapter: None,
    triggers: &[],
    derived_key: Some(DerivedKey {
        exists_group: edge_group::EXISTS,
        key_inputs: &[],
    }),
    tree: None,
    acyclic: Some(AcyclicSpec {
        from_column: "from_id",
        to_column: "to_id",
    }),
    publish_when: None,
    published_with: None,
    holds_deletes: false,
};

/// `fixture_edges`.
pub(super) const EDGES_TABLE: TableId = TableId(FIXTURE_TABLE_IDS + 6);

/// Group ids of [`DRAFTS`].
pub(super) mod draft_group {
    use ganbaru_sync_contracts::GroupId;

    use crate::manifest::group_id;

    /// The title.
    pub(in crate::tests) const TITLE: GroupId = group_id(0);
    /// The publication flag.
    pub(in crate::tests) const PUBLISHED: GroupId = group_id(1);
}

/// Drafts that publish once their flag is set, like a chat message composed before it is sent.
pub(super) const DRAFTS: TableSpec = TableSpec {
    id: DRAFTS_TABLE,
    name: "fixture_drafts",
    key_column: "id",
    groups: &[
        GroupSpec {
            id: draft_group::TITLE,
            name: "title",
            kind: MergeKind::Register,
            storage: GroupStorage::Columns(&[ColumnSpec {
                name: "title",
                ty: ColumnType::Text { max_chars: 40 },
            }]),
            references: &[],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
        GroupSpec {
            id: draft_group::PUBLISHED,
            name: "published",
            kind: MergeKind::Register,
            storage: GroupStorage::Columns(&[ColumnSpec {
                name: "published",
                ty: ColumnType::Flag,
            }]),
            references: &[],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
    ],
    local_columns: &[],
    derived_columns: &[],
    hook_columns: &[],
    key_derived_columns: &[],
    revision_column: None,
    resolutions: &[],
    redirects: false,
    adapter: None,
    triggers: &[],
    derived_key: None,
    tree: None,
    acyclic: None,
    publish_when: Some("published = 1"),
    published_with: None,
    holds_deletes: false,
};

/// `fixture_drafts`.
pub(super) const DRAFTS_TABLE: TableId = TableId(FIXTURE_TABLE_IDS + 7);

/// Parts of a draft, published with it.
pub(super) const DRAFT_PARTS: TableSpec = TableSpec {
    id: DRAFT_PARTS_TABLE,
    name: "fixture_draft_parts",
    key_column: "id",
    groups: &[
        GroupSpec {
            id: group_id(0),
            name: "draft",
            kind: MergeKind::Immutable,
            storage: GroupStorage::Columns(&[ColumnSpec {
                name: "draft_id",
                ty: ColumnType::OptionalRowKey,
            }]),
            references: &[ReferenceColumn {
                column: "draft_id",
                table: DRAFTS_TABLE,
                on_delete: OnDelete::Cascade,
            }],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
        GroupSpec {
            id: group_id(1),
            name: "body",
            kind: MergeKind::Register,
            storage: GroupStorage::Columns(&[ColumnSpec {
                name: "body",
                ty: ColumnType::Text { max_chars: 40 },
            }]),
            references: &[],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
    ],
    publish_when: None,
    published_with: Some(PublishedWith {
        column: "draft_id",
        parent: DRAFTS_TABLE,
    }),
    ..DRAFTS
};

/// `fixture_draft_parts`.
pub(super) const DRAFT_PARTS_TABLE: TableId = TableId(FIXTURE_TABLE_IDS + 8);

/// Memos whose local deletes domain Undo can hold.
pub(super) const MEMOS: TableSpec = TableSpec {
    id: MEMOS_TABLE,
    name: "fixture_memos",
    key_column: "id",
    groups: &[LABEL],
    publish_when: None,
    holds_deletes: true,
    ..DRAFTS
};

/// `fixture_memos`.
pub(super) const MEMOS_TABLE: TableId = TableId(FIXTURE_TABLE_IDS + 9);

/// The immutable cascading reference from a memo child to its memo.
const MEMO_REFERENCE: ColumnSpec = ColumnSpec {
    name: "memo_id",
    ty: ColumnType::OptionalRowKey,
};

/// Lines of a memo, removed with it, so they inherit a hold on its delete.
pub(super) const MEMO_LINES: TableSpec = TableSpec {
    id: MEMO_LINES_TABLE,
    name: "fixture_memo_lines",
    key_column: "id",
    groups: &[
        GroupSpec {
            id: group_id(0),
            name: "memo",
            kind: MergeKind::Immutable,
            storage: GroupStorage::Columns(&[MEMO_REFERENCE]),
            references: &[ReferenceColumn {
                column: "memo_id",
                table: MEMOS_TABLE,
                on_delete: OnDelete::Cascade,
            }],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
        GroupSpec {
            id: group_id(1),
            name: "body",
            kind: MergeKind::Register,
            storage: GroupStorage::Columns(&[ColumnSpec {
                name: "body",
                ty: ColumnType::Text { max_chars: 40 },
            }]),
            references: &[],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
    ],
    holds_deletes: false,
    ..MEMOS
};

/// `fixture_memo_lines`.
pub(super) const MEMO_LINES_TABLE: TableId = TableId(FIXTURE_TABLE_IDS + 10);

/// Prefix of [`MEMO_TAGS`] keys.
const MEMO_TAG_KEY_PREFIX: &str = "fixture_memo_tag";

/// The `sync_key` of `tag` on `memo`.
pub(super) fn memo_tag_key(memo: &str, tag: &str) -> String {
    derived_key(MEMO_TAG_KEY_PREFIX, &[memo, tag])
        .expect("fixture ids are valid key parts")
        .into_string()
}

/// Tags of a memo: a derived-key table removed with the memo, so its removals inherit a hold
/// on the memo's delete.
pub(super) const MEMO_TAGS: TableSpec = TableSpec {
    id: MEMO_TAGS_TABLE,
    name: "fixture_memo_tags",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        GroupSpec {
            id: group_id(0),
            name: "identity",
            kind: MergeKind::Immutable,
            storage: GroupStorage::Columns(&[
                MEMO_REFERENCE,
                ColumnSpec {
                    name: "tag",
                    ty: ColumnType::Text { max_chars: 20 },
                },
            ]),
            references: &[ReferenceColumn {
                column: "memo_id",
                table: MEMOS_TABLE,
                on_delete: OnDelete::Cascade,
            }],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
        GroupSpec {
            id: group_id(1),
            name: "exists",
            kind: MergeKind::Register,
            storage: GroupStorage::Columns(&[]),
            references: &[],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
    ],
    derived_key: Some(DerivedKey {
        exists_group: group_id(1),
        key_inputs: &[],
    }),
    holds_deletes: false,
    ..MEMOS
};

/// `fixture_memo_tags`.
pub(super) const MEMO_TAGS_TABLE: TableId = TableId(FIXTURE_TABLE_IDS + 11);

/// The immutable creation time of a list or board.
const CREATED: GroupSpec = GroupSpec {
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
};

/// Most entries of one list.
pub(super) const MAX_LIST_ENTRIES: usize = 6;

/// Entries of a list on the generic codec. Entry ids are unique data columns that materialize
/// writes unchanged, and an entry naming a membership that is removed, missing, or not yet
/// received is left out until the membership exists.
pub(super) const LIST_ENTRIES: OwnedTable = OwnedTable {
    table: "fixture_list_entries",
    parent_column: "list_id",
    order: OwnedOrder::Ordinal("ordinal"),
    columns: &[
        ColumnSpec {
            name: "entry_id",
            ty: ColumnType::Text { max_chars: 20 },
        },
        ColumnSpec {
            name: "body",
            ty: ColumnType::Text { max_chars: 40 },
        },
        ColumnSpec {
            name: "note",
            ty: ColumnType::OptionalText { max_chars: 20 },
        },
        ColumnSpec {
            name: "member_key",
            ty: ColumnType::OptionalRowKey,
        },
    ],
    references: &[OwnedReference {
        column: "member_key",
        table: MEMBERSHIPS_TABLE,
    }],
    unique: &["entry_id"],
    max_rows: MAX_LIST_ENTRIES,
    codec: OwnedCodec::Generic,
    nested: &[],
};

/// Field groups of [`LISTS`].
pub(super) mod list_group {
    use super::{GroupId, group_id};

    /// `title` and the entries.
    pub const ENTRIES: GroupId = group_id(1);
}

/// Lists, always published, whose title and entries form one register. `entry_count` is a hook
/// column the fixture hook keeps equal to the materialized entries.
pub(super) const LISTS: TableSpec = TableSpec {
    id: LISTS_TABLE,
    name: "fixture_lists",
    key_column: "id",
    groups: &[
        CREATED,
        GroupSpec {
            id: list_group::ENTRIES,
            name: "entries",
            kind: MergeKind::Register,
            storage: GroupStorage::Owned {
                columns: &[ColumnSpec {
                    name: "title",
                    ty: ColumnType::Text { max_chars: 40 },
                }],
                tables: &[LIST_ENTRIES],
            },
            references: &[],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
    ],
    hook_columns: &["entry_count"],
    holds_deletes: false,
    ..MEMOS
};

/// `fixture_lists`.
pub(super) const LISTS_TABLE: TableId = TableId(FIXTURE_TABLE_IDS + 12);

/// Checks of a board card, nested in the card through its ordinal.
pub(super) const BOARD_CHECKS: OwnedTable = OwnedTable {
    table: "fixture_board_checks",
    parent_column: "board_id",
    order: OwnedOrder::Nested {
        parent_ordinal: "card_ordinal",
        ordinal: "ordinal",
    },
    columns: &[
        ColumnSpec {
            name: "check_id",
            ty: ColumnType::Text { max_chars: 20 },
        },
        ColumnSpec {
            name: "done",
            ty: ColumnType::Flag,
        },
    ],
    references: &[],
    unique: &["check_id"],
    max_rows: 3,
    codec: OwnedCodec::Generic,
    nested: &[],
};

/// Cards of a board. A card naming a deleted item is left out with its checks.
pub(super) const BOARD_CARDS: OwnedTable = OwnedTable {
    table: "fixture_board_cards",
    parent_column: "board_id",
    order: OwnedOrder::Ordinal("ordinal"),
    columns: &[
        ColumnSpec {
            name: "card_id",
            ty: ColumnType::Text { max_chars: 20 },
        },
        ColumnSpec {
            name: "title",
            ty: ColumnType::Text { max_chars: 40 },
        },
        ColumnSpec {
            name: "item_id",
            ty: ColumnType::OptionalRowKey,
        },
    ],
    references: &[OwnedReference {
        column: "item_id",
        table: ITEMS_TABLE,
    }],
    unique: &["card_id"],
    max_rows: 4,
    codec: OwnedCodec::Generic,
    nested: &[BOARD_CHECKS],
};

/// Field groups of [`BOARDS`].
pub(super) mod board_group {
    use super::{GroupId, group_id};

    /// Owner, stage, and cards.
    pub const STATE: GroupId = group_id(1);
}

/// Prefix of a board's `handle`, a key-derived column.
pub(super) const BOARD_HANDLE_PREFIX: &str = "board:";

/// Boards whose owner reference, stage, and cards form one coupled group: a later stage wins
/// before clocks are compared. `handle` is the row key behind [`BOARD_HANDLE_PREFIX`].
pub(super) const BOARDS: TableSpec = TableSpec {
    id: BOARDS_TABLE,
    name: "fixture_boards",
    key_column: "id",
    groups: &[
        CREATED,
        GroupSpec {
            id: board_group::STATE,
            name: "state",
            kind: MergeKind::Coupled,
            storage: GroupStorage::Owned {
                columns: &[
                    ColumnSpec {
                        name: "owner_id",
                        ty: ColumnType::OptionalRowKey,
                    },
                    ColumnSpec {
                        name: "stage",
                        ty: ColumnType::Integer {
                            min: 0,
                            max: MAX_BOARD_STAGE,
                        },
                    },
                ],
                tables: &[BOARD_CARDS],
            },
            references: &[ReferenceColumn {
                column: "owner_id",
                table: ITEMS_TABLE,
                on_delete: OnDelete::SetNull,
            }],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        },
    ],
    key_derived_columns: &[KeyDerivedColumn {
        name: "handle",
        prefix: BOARD_HANDLE_PREFIX,
    }],
    adapter: Some(FIXTURE_ADAPTER),
    holds_deletes: false,
    ..MEMOS
};

/// `fixture_boards`.
pub(super) const BOARDS_TABLE: TableId = TableId(FIXTURE_TABLE_IDS + 13);

/// Last board stage.
pub(super) const MAX_BOARD_STAGE: i64 = 2;

/// Name of [`FixtureAdapter`].
const FIXTURE_ADAPTER: &str = "fixture";

/// Adapter of the fixture domain: a board's stage is its coupled priority.
#[derive(Debug)]
pub(super) struct FixtureAdapter;

impl DomainAdapter for FixtureAdapter {
    fn name(&self) -> &'static str {
        FIXTURE_ADAPTER
    }

    fn derived(
        &self,
        _table: TableId,
        _group: GroupId,
        _value: &Value,
    ) -> Result<Vec<(&'static str, Field)>, AdapterError> {
        Ok(Vec::new())
    }

    fn priority(&self, table: TableId, group: GroupId, value: &Value) -> u8 {
        match (table, group, value.fields().get(1)) {
            (BOARDS_TABLE, board_group::STATE, Some(Field::Integer(stage))) => {
                u8::try_from(*stage).unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn validate(
        &self,
        _table: TableId,
        _group: GroupId,
        _value: &Value,
    ) -> Result<(), AdapterError> {
        Ok(())
    }

    fn presentation(&self, _table: TableId, _values: &[(GroupId, Value)]) -> Presentation {
        Presentation::default()
    }
}

/// Topic of the change notices [`FixtureHook`] reports.
pub(super) const LIST_COUNT_TOPIC: &str = "fixture_list_counts";

/// SQL that recomputes the hook column and count row of every list.
const RECOMPUTE_LISTS_SQL: &str = "
    UPDATE fixture_lists SET entry_count =
        (SELECT count(*) FROM fixture_list_entries WHERE list_id = fixture_lists.id)
    WHERE entry_count <>
        (SELECT count(*) FROM fixture_list_entries WHERE list_id = fixture_lists.id);
    DELETE FROM fixture_list_counts;
    INSERT INTO fixture_list_counts (list_id, entries) SELECT id, entry_count FROM fixture_lists;";

/// Owned tables whose rows a local delete of a referenced row removes by cascade: the table,
/// its owner column, and its ordinal column.
const COMPACTED: &[(&str, &str, &str)] = &[
    ("fixture_list_entries", "list_id", "ordinal"),
    ("fixture_board_cards", "board_id", "ordinal"),
];

/// What fixture domain code does after a local edit, in the edit's transaction: numbers owned
/// rows from zero again where a cascade left gaps, as materialize writes them, and recomputes
/// the list entry counts.
pub(super) async fn after_local_edit(conn: &mut SqliteConnection) -> Result<(), sqlx::Error> {
    for (table, owner, ordinal) in COMPACTED {
        let rows: Vec<(String, i64)> = sqlx::query_as(AssertSqlSafe(format!(
            "SELECT {owner}, {ordinal} FROM {table} ORDER BY {owner}, {ordinal}"
        )))
        .fetch_all(&mut *conn)
        .await?;
        let mut position = 0;
        let mut previous: Option<&str> = None;
        let mut moved = false;
        for (key, at) in &rows {
            if previous != Some(key.as_str()) {
                position = 0;
                previous = Some(key);
            }
            if *at != position {
                // Moved rows pass through negative ordinals, so no two rows share one.
                sqlx::query(AssertSqlSafe(format!(
                    "UPDATE {table} SET {ordinal} = ? WHERE {owner} = ? AND {ordinal} = ?"
                )))
                .bind(-1 - position)
                .bind(key)
                .bind(at)
                .execute(&mut *conn)
                .await?;
                moved = true;
            }
            position += 1;
        }
        if moved {
            sqlx::raw_sql(AssertSqlSafe(format!(
                "UPDATE {table} SET {ordinal} = -1 - {ordinal} WHERE {ordinal} < 0"
            )))
            .execute(&mut *conn)
            .await?;
        }
    }
    sqlx::raw_sql(RECOMPUTE_LISTS_SQL)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Recomputes the hook column and count row of each list in `keys`; a deleted list loses its
/// count row.
pub(super) async fn recompute_lists(
    conn: &mut SqliteConnection,
    keys: &[String],
) -> Result<(), sqlx::Error> {
    for key in keys {
        sqlx::query(
            "UPDATE fixture_lists SET entry_count =
                 (SELECT count(*) FROM fixture_list_entries WHERE list_id = ?1)
             WHERE id = ?1",
        )
        .bind(key)
        .execute(&mut *conn)
        .await?;
        sqlx::query("DELETE FROM fixture_list_counts WHERE list_id = ?")
            .bind(key)
            .execute(&mut *conn)
            .await?;
        sqlx::query(
            "INSERT INTO fixture_list_counts (list_id, entries)
             SELECT id, entry_count FROM fixture_lists WHERE id = ?",
        )
        .bind(key)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// The fixture hook: keeps list entry counts and reports each recomputed list.
#[derive(Debug)]
pub(super) struct FixtureHook;

impl MaterializeHook for FixtureHook {
    fn after_materialize<'a>(
        &'a self,
        conn: &'a mut SqliteConnection,
        _table: &'static TableSpec,
        changed_keys: &'a [String],
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(async move {
            recompute_lists(conn, changed_keys).await?;
            for key in changed_keys {
                changes.report(LIST_COUNT_TOPIC, key.clone());
            }
            Ok(())
        })
    }
}

/// The fixture domain.
pub(super) const FIXTURE_DOMAIN: DomainManifest = DomainManifest {
    name: "fixture",
    table_ids: TableIdRange {
        first: FIXTURE_TABLE_IDS,
        last: FIXTURE_TABLE_IDS + 15,
    },
    tables: &[
        ITEMS,
        CYCLE_A,
        CYCLE_B,
        LINKS,
        MEMBERSHIPS,
        NODES,
        EDGES,
        DRAFTS,
        DRAFT_PARTS,
        MEMOS,
        MEMO_LINES,
        MEMO_TAGS,
        LISTS,
        BOARDS,
    ],
    derived: &["fixture_items_index", "fixture_list_counts"],
    local: &[DeviceTable {
        name: "fixture_item_marks",
        triggers: &[DomainTrigger {
            name: "fixture_item_marks_refuse_negative",
            policy: TriggerPolicy::ApplierVisible,
        }],
    }],
    engine: &[],
    replica_writable: true,
    assets: &[AssetClass {
        name: "fixture/files",
        max_bytes: 1024 * 1024,
    }],
};

/// The vault domains followed by the fixture domain.
pub(super) const TEST_DOMAINS: &[&DomainManifest] = &[&quick_notes::DOMAIN, &FIXTURE_DOMAIN];

/// The manifest of [`TEST_DOMAINS`].
pub(super) static TEST_MANIFEST: LazyLock<Manifest> =
    LazyLock::new(|| Manifest::from_domains(VAULT_MANIFEST_VERSION, TEST_DOMAINS));

/// The vault adapters and [`FixtureAdapter`].
pub(super) fn test_adapters() -> Adapters {
    vault_adapters().with(Box::new(FixtureAdapter))
}

/// An engine for [`TEST_MANIFEST`] with [`test_adapters`] and [`FixtureHook`] on lists.
pub(super) fn test_engine() -> Engine {
    Engine::with(
        &TEST_MANIFEST,
        test_adapters(),
        Hooks::new().with(LISTS_TABLE, Box::new(FixtureHook)),
    )
    .expect("the test manifest, adapters, and hooks should agree")
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
         CREATE TABLE fixture_nodes (
             id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
             parent_id TEXT REFERENCES fixture_nodes(id) ON DELETE CASCADE,
             position TEXT NOT NULL,
             label TEXT NOT NULL CHECK (length(label) <= 40)
         );
         CREATE TABLE fixture_edges (
             from_id TEXT NOT NULL REFERENCES fixture_items(id) ON DELETE CASCADE,
             to_id TEXT NOT NULL REFERENCES fixture_items(id) ON DELETE CASCADE,
             sync_key TEXT NOT NULL UNIQUE,
             PRIMARY KEY (from_id, to_id)
         );
         CREATE TABLE fixture_drafts (
             id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
             title TEXT NOT NULL CHECK (length(title) <= 40),
             published INTEGER NOT NULL CHECK (published IN (0, 1))
         );
         CREATE TABLE fixture_draft_parts (
             id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
             draft_id TEXT NOT NULL REFERENCES fixture_drafts(id) ON DELETE CASCADE,
             body TEXT NOT NULL CHECK (length(body) <= 40)
         );
         CREATE TABLE fixture_memos (
             id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
             label TEXT NOT NULL CHECK (length(label) <= 40)
         );
         CREATE TABLE fixture_memo_lines (
             id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
             memo_id TEXT NOT NULL REFERENCES fixture_memos(id) ON DELETE CASCADE,
             body TEXT NOT NULL CHECK (length(body) <= 40)
         );
         CREATE TABLE fixture_memo_tags (
             memo_id TEXT NOT NULL REFERENCES fixture_memos(id) ON DELETE CASCADE,
             tag TEXT NOT NULL CHECK (length(tag) <= 20),
             sync_key TEXT NOT NULL UNIQUE,
             PRIMARY KEY (memo_id, tag)
         );
         CREATE TABLE fixture_lists (
             id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
             created_at TEXT NOT NULL,
             title TEXT NOT NULL CHECK (length(title) <= 40),
             entry_count INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE fixture_list_entries (
             list_id TEXT NOT NULL REFERENCES fixture_lists(id) ON DELETE CASCADE,
             ordinal INTEGER NOT NULL,
             entry_id TEXT NOT NULL UNIQUE,
             body TEXT NOT NULL CHECK (length(body) <= 40),
             note TEXT CHECK (length(note) <= 20),
             member_key TEXT REFERENCES fixture_memberships(sync_key) ON DELETE CASCADE,
             PRIMARY KEY (list_id, ordinal)
         );
         CREATE TABLE fixture_list_counts (
             list_id TEXT PRIMARY KEY,
             entries INTEGER NOT NULL
         );
         CREATE TABLE fixture_boards (
             id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
             created_at TEXT NOT NULL,
             owner_id TEXT REFERENCES fixture_items(id) ON DELETE SET NULL,
             stage INTEGER NOT NULL CHECK (stage BETWEEN 0 AND {MAX_BOARD_STAGE}),
             handle TEXT NOT NULL UNIQUE CHECK (handle = '{BOARD_HANDLE_PREFIX}' || id)
         );
         CREATE TABLE fixture_board_cards (
             board_id TEXT NOT NULL REFERENCES fixture_boards(id) ON DELETE CASCADE,
             ordinal INTEGER NOT NULL,
             card_id TEXT NOT NULL UNIQUE,
             title TEXT NOT NULL CHECK (length(title) <= 40),
             item_id TEXT REFERENCES fixture_items(id) ON DELETE CASCADE,
             PRIMARY KEY (board_id, ordinal)
         );
         CREATE TABLE fixture_board_checks (
             board_id TEXT NOT NULL,
             card_ordinal INTEGER NOT NULL,
             ordinal INTEGER NOT NULL,
             check_id TEXT NOT NULL UNIQUE,
             done INTEGER NOT NULL CHECK (done IN (0, 1)),
             PRIMARY KEY (board_id, card_ordinal, ordinal),
             FOREIGN KEY (board_id, card_ordinal)
                 REFERENCES fixture_board_cards(board_id, ordinal)
                 ON DELETE CASCADE ON UPDATE CASCADE
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
