//! Quick notes tables: tags (table 1) and notes (table 2).

use ganbaru_sync_contracts::{GroupId, TableId};

use crate::manifest::{
    Collation, ColumnSpec, ColumnType, GroupSpec, GroupStorage, MergeKind, OwnedTable, Resolution,
    TableSpec, group_id,
};

/// Domain adapter name for Quick notes.
pub const ADAPTER: &str = "quick_notes";

/// `quick_note_tags`.
pub const TAGS_TABLE: TableId = TableId(1);
/// `quick_notes`.
pub const NOTES_TABLE: TableId = TableId(2);

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

const TAG_GROUPS: [GroupSpec; 4] = [
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
];

const NOTE_GROUPS: [GroupSpec; 8] = [
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
        GroupStorage::Owned(OwnedTable {
            table: "quick_note_text_runs",
            parent_column: "note_id",
            order_column: "sort_order",
            columns: &["content", "bold", "italic", "underline"],
        }),
        true,
    ),
    content(
        note_group::COLOR,
        "color",
        MergeKind::Register,
        GroupStorage::Columns(&[ColumnSpec {
            name: "color",
            ty: ColumnType::Integer { min: 0, max: 31 },
        }]),
        false,
    ),
    content(
        note_group::TAG,
        "tag",
        MergeKind::Reference { table: TAGS_TABLE },
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
];

/// `quick_note_tags`.
pub const TAGS: TableSpec = TableSpec {
    id: TAGS_TABLE,
    name: "quick_note_tags",
    key_column: "id",
    groups: &TAG_GROUPS,
    local_columns: &[],
    derived_columns: &[],
    revision_column: None,
    resolutions: &[Resolution::DuplicateRepair {
        columns: &["name"],
        collation: Collation::NoCase,
    }],
    redirects: true,
    adapter: None,
};

/// `quick_notes`.
pub const NOTES: TableSpec = TableSpec {
    id: NOTES_TABLE,
    name: "quick_notes",
    key_column: "id",
    groups: &NOTE_GROUPS,
    local_columns: &["revision"],
    derived_columns: &["body_plain_text"],
    revision_column: Some("revision"),
    resolutions: &[Resolution::WithinGroup {
        columns: &["pinned", "archived", "trashed_at"],
        group: note_group::LIFECYCLE,
    }],
    redirects: false,
    adapter: Some(ADAPTER),
};
