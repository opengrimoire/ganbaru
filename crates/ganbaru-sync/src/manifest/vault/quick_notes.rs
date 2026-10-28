//! Quick notes tables: tags (table 1) and notes (table 2).

use ganbaru_sync_contracts::{GroupId, TableId};

use crate::assets::AssetClass;

use crate::domains::quick_notes::{
    MAX_BODY_CHARS, MAX_CONTENT_TYPE_CHARS, MAX_IMAGE_BYTES, MAX_IMAGE_DIMENSION, MAX_IMAGES,
    MAX_RUNS, SHA256_HEX_CHARS,
};
use crate::manifest::{
    Collation, ColumnSpec, ColumnType, DomainManifest, DomainTrigger, GroupSpec, GroupStorage,
    LocalColumn, MergeKind, OnDelete, OwnedBound, OwnedCodec, OwnedOrder, OwnedTable, Resolution,
    TableIdRange, TableSpec, TriggerPolicy, group_id,
};

/// Domain name for Quick notes.
pub const DOMAIN_NAME: &str = "quick_notes";

/// Domain adapter name for Quick notes.
pub const ADAPTER: &str = "quick_notes";

/// `quick_note_tags`.
pub const TAGS_TABLE: TableId = TableId(1);
/// `quick_notes`.
pub const NOTES_TABLE: TableId = TableId(2);
/// Owned text runs of a note body.
pub const RUNS_OWNED: &str = "quick_note_text_runs";
/// Owned image references of a note.
pub const IMAGES_OWNED: &str = "quick_note_images";

/// Tag field groups.
pub mod tag_group {
    use super::{GroupId, group_id};

    /// `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `order_key`.
    pub const ORDER: GroupId = group_id(2);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(3);
    /// `color`.
    pub const COLOR: GroupId = group_id(4);
}

/// Note field groups.
pub mod note_group {
    use super::{GroupId, group_id};

    /// `created_at`.
    pub const CREATED: GroupId = group_id(0);
    /// `title`.
    pub const TITLE: GroupId = group_id(1);
    /// Owned `quick_note_text_runs`; `body_plain_text` is derived.
    pub const BODY: GroupId = group_id(2);
    /// `color`.
    pub const COLOR: GroupId = group_id(3);
    /// `tag_id`.
    pub const TAG: GroupId = group_id(4);
    /// `pinned`, `archived`, and `trashed_at`.
    pub const LIFECYCLE: GroupId = group_id(5);
    /// `order_key`.
    pub const ORDER: GroupId = group_id(6);
    /// `updated_at`.
    pub const UPDATED: GroupId = group_id(7);
    /// Owned `quick_note_images`, references to content-addressed files in the vault.
    pub const IMAGES: GroupId = group_id(8);
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

const fn content(
    id: GroupId,
    name: &'static str,
    kind: MergeKind,
    storage: GroupStorage,
    surfaced: bool,
) -> GroupSpec {
    GroupSpec {
        id,
        name,
        kind,
        storage,
        references: &[],
        surfaced,
        recoverable: true,
        bumps_revision: true,
    }
}

const CREATED_AT: &[ColumnSpec] = &[ColumnSpec {
    name: "created_at",
    ty: ColumnType::Timestamp,
}];
const UPDATED_AT: &[ColumnSpec] = &[ColumnSpec {
    name: "updated_at",
    ty: ColumnType::Timestamp,
}];
const ORDER_KEY: &[ColumnSpec] = &[ColumnSpec {
    name: "order_key",
    ty: ColumnType::OrderKey,
}];
/// A palette slot shared by tags and notes.
const COLOR: &[ColumnSpec] = &[ColumnSpec {
    name: "color",
    ty: ColumnType::Integer { min: 0, max: 31 },
}];

/// A style flag of a text run.
const fn flag(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Flag,
    }
}

/// An image measure in `1..=max`.
const fn positive(name: &'static str, max: u32) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Integer {
            min: 1,
            max: max as i64,
        },
    }
}

const TAG_GROUPS: [GroupSpec; 5] = [
    plain(
        tag_group::CREATED,
        "created",
        MergeKind::Immutable,
        CREATED_AT,
    ),
    plain(
        tag_group::NAME,
        "name",
        MergeKind::Register,
        &[ColumnSpec {
            name: "name",
            ty: ColumnType::TrimmedText {
                min_chars: 1,
                max_chars: 40,
            },
        }],
    ),
    plain(tag_group::ORDER, "order", MergeKind::Position, ORDER_KEY),
    plain(tag_group::UPDATED, "updated", MergeKind::Max, UPDATED_AT),
    plain(tag_group::COLOR, "color", MergeKind::Register, COLOR),
];

const NOTE_GROUPS: [GroupSpec; 9] = [
    plain(
        note_group::CREATED,
        "created",
        MergeKind::Immutable,
        CREATED_AT,
    ),
    content(
        note_group::TITLE,
        "title",
        MergeKind::Register,
        GroupStorage::Columns(&[ColumnSpec {
            name: "title",
            ty: ColumnType::Text { max_chars: 200 },
        }]),
        true,
    ),
    content(
        note_group::BODY,
        "body",
        MergeKind::Register,
        GroupStorage::Owned {
            columns: &[],
            tables: &[OwnedTable {
                table: RUNS_OWNED,
                parent_column: "note_id",
                order: OwnedOrder::Ordinal("sort_order"),
                columns: &[
                    ColumnSpec {
                        name: "content",
                        ty: ColumnType::Text {
                            max_chars: MAX_BODY_CHARS,
                        },
                    },
                    flag("bold"),
                    flag("italic"),
                    flag("underline"),
                ],
                references: &[],
                unique: &[],
                max_rows: MAX_RUNS,
                codec: OwnedCodec::Custom,
                nested: &[],
                bound: OwnedBound::Static,
            }],
        },
        true,
    ),
    content(
        note_group::COLOR,
        "color",
        MergeKind::Register,
        GroupStorage::Columns(COLOR),
        false,
    ),
    content(
        note_group::TAG,
        "tag",
        MergeKind::Reference {
            table: TAGS_TABLE,
            on_delete: OnDelete::SetNull,
        },
        GroupStorage::Columns(&[ColumnSpec {
            name: "tag_id",
            ty: ColumnType::OptionalRowKey,
        }]),
        false,
    ),
    content(
        note_group::LIFECYCLE,
        "lifecycle",
        MergeKind::Coupled,
        GroupStorage::Columns(&[
            ColumnSpec {
                name: "pinned",
                ty: ColumnType::Flag,
            },
            ColumnSpec {
                name: "archived",
                ty: ColumnType::Flag,
            },
            ColumnSpec {
                name: "trashed_at",
                ty: ColumnType::OptionalTimestamp,
            },
        ]),
        false,
    ),
    plain(note_group::ORDER, "order", MergeKind::Position, ORDER_KEY),
    plain(note_group::UPDATED, "updated", MergeKind::Max, UPDATED_AT),
    content(
        note_group::IMAGES,
        "images",
        MergeKind::Register,
        GroupStorage::Owned {
            columns: &[],
            tables: &[OwnedTable {
                table: IMAGES_OWNED,
                parent_column: "note_id",
                order: OwnedOrder::Ordinal("sort_order"),
                columns: &[
                    ColumnSpec {
                        name: "sha256",
                        ty: ColumnType::Text {
                            max_chars: SHA256_HEX_CHARS,
                        },
                    },
                    ColumnSpec {
                        name: "content_type",
                        ty: ColumnType::Text {
                            max_chars: MAX_CONTENT_TYPE_CHARS,
                        },
                    },
                    positive("byte_size", MAX_IMAGE_BYTES),
                    positive("width", MAX_IMAGE_DIMENSION),
                    positive("height", MAX_IMAGE_DIMENSION),
                ],
                references: &[],
                unique: &[],
                max_rows: MAX_IMAGES,
                codec: OwnedCodec::Custom,
                nested: &[],
                bound: OwnedBound::Static,
            }],
        },
        true,
    ),
];

/// `quick_note_tags`.
pub const TAGS: TableSpec = TableSpec {
    id: TAGS_TABLE,
    name: "quick_note_tags",
    key_column: "id",
    groups: &TAG_GROUPS,
    local_columns: &[],
    derived_columns: &[],
    hook_columns: &[],
    key_derived_columns: &[],
    revision_column: None,
    resolutions: &[Resolution::DuplicateRepair {
        columns: &["name"],
        collation: Collation::NoCase,
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

/// `quick_notes`.
pub const NOTES: TableSpec = TableSpec {
    id: NOTES_TABLE,
    name: "quick_notes",
    key_column: "id",
    groups: &NOTE_GROUPS,
    local_columns: &[LocalColumn {
        name: "revision",
        materialize: "1",
    }],
    derived_columns: &["body_plain_text"],
    hook_columns: &[],
    key_derived_columns: &[],
    revision_column: Some("revision"),
    resolutions: &[Resolution::WithinGroup {
        columns: &["pinned", "archived", "trashed_at"],
        group: note_group::LIFECYCLE,
    }],
    redirects: false,
    adapter: Some(ADAPTER),
    triggers: &[
        search_trigger("quick_notes_search_delete"),
        search_trigger("quick_notes_search_insert"),
        search_trigger("quick_notes_search_update"),
    ],
    derived_key: None,
    tree: None,
    acyclic: None,
    publish_when: None,
    published_with: None,
    holds_deletes: false,
};

/// A full-text index trigger, which keeps `quick_notes_search_fts` in step with every write.
const fn search_trigger(name: &'static str) -> DomainTrigger {
    DomainTrigger {
        name,
        policy: TriggerPolicy::DerivedIndex,
    }
}

/// Image files that note image lists reference.
pub const IMAGES_CLASS: AssetClass = AssetClass {
    name: "quick-notes/images",
    max_bytes: MAX_IMAGE_BYTES,
};

/// The Quick notes domain.
pub const DOMAIN: DomainManifest = DomainManifest {
    name: DOMAIN_NAME,
    table_ids: TableIdRange { first: 1, last: 15 },
    tables: &[TAGS, NOTES],
    derived: &["quick_notes_search_fts"],
    local: &[],
    engine: &[],
    replica_writable: true,
    unconverted: &[],
    assets: &[IMAGES_CLASS],
};
