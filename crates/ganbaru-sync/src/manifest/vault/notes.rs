//! Notes: table ids 288 to 351.
//!
//! The structure of Notes replicates: the local user row, folders, pages, page aliases, and
//! blocks. Page parents and block pages are hook columns that the Notes hook derives from the
//! replicated placement: a nested page names its `child_page` block, whose placement gives the
//! parent page or block, and a block names its parent page or block, whose ancestry gives its
//! page. Block plain text is a hook column derived from the block text. Block nesting is not an
//! engine tree; the Notes repair pass on the vault owner breaks block cycles and flattens blocks
//! nested too deep. Folders and database row parents are engine trees.
//!
//! Search, link, backlink, asset reference, and rollup indexes and their state rows are derived.
//! Undo state, edit receipts, read and mute state, notification settings and notifications, view
//! state, and every history table stay on the device. Comments, suggestions, templates,
//! databases, data sources, properties, values, views, and relation edges are not converted yet.
//! Ids 293 to 351 are reserved.

use ganbaru_sync_contracts::{GroupId, TableId};

use crate::domains::notes::{
    MAX_BLOCK_ATTRS_BYTES, MAX_BLOCK_TEXT_BYTES, MAX_NAME_CHARS, MAX_TEXT_CHARS,
};
use crate::manifest::vault::projects::PROJECTS_TABLE;
use crate::manifest::{
    Collation, ColumnSpec, ColumnType, DeviceTable, DomainManifest, GroupSpec, GroupStorage,
    MergeKind, OnDelete, ReferenceColumn, Resolution, TableIdRange, TableSpec, TreeSpec, group_id,
};

/// Domain name for Notes.
pub const DOMAIN_NAME: &str = "notes";

/// Domain adapter name for Notes.
pub const ADAPTER: &str = "notes";

/// `notes_local_users`.
pub const LOCAL_USERS_TABLE: TableId = TableId(288);
/// `notes_folders`.
pub const FOLDERS_TABLE: TableId = TableId(289);
/// `notes_pages`.
pub const PAGES_TABLE: TableId = TableId(290);
/// `notes_page_aliases`.
pub const ALIASES_TABLE: TableId = TableId(291);
/// `notes_blocks`.
pub const BLOCKS_TABLE: TableId = TableId(292);

/// Greatest folder nesting depth, as Notes checks it.
pub const MAX_FOLDER_DEPTH: u8 = 64;
/// Greatest database row hierarchy depth, as Notes checks it.
pub const MAX_ROW_DEPTH: u8 = 32;

/// Local user field groups.
pub mod local_user_group {
    use super::{GroupId, group_id};

    /// `created_time`.
    pub const CREATED: GroupId = group_id(0);
}

/// Folder field groups.
pub mod folder_group {
    use super::{GroupId, group_id};

    /// `project_id` and `created_time`, fixed at creation.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `parent_folder_id`.
    pub const PARENT: GroupId = group_id(2);
    /// `last_edited_time`.
    pub const UPDATED: GroupId = group_id(3);
}

/// Page field groups.
pub mod page_group {
    use super::{GroupId, group_id};

    /// `created_time`.
    pub const CREATED: GroupId = group_id(0);
    /// Import source identity and the page URLs, which only imports and restores write.
    pub const ORIGIN: GroupId = group_id(1);
    /// `title`.
    pub const TITLE: GroupId = group_id(2);
    /// `child_block_id`, `parent_data_source_id`, and `folder_id`: where the page lives.
    pub const PLACEMENT: GroupId = group_id(3);
    /// `project_id`.
    pub const PROJECT: GroupId = group_id(4);
    /// `parent_row_page_id`.
    pub const ROW_PARENT: GroupId = group_id(5);
    /// `icon` and `cover`.
    pub const APPEARANCE: GroupId = group_id(6);
    /// `in_trash`, `trashed_time`, and `trash_owner`.
    pub const LIFECYCLE: GroupId = group_id(7);
    /// `archived`.
    pub const ARCHIVED: GroupId = group_id(8);
    /// `last_edited_time`.
    pub const UPDATED: GroupId = group_id(9);
}

/// Page alias field groups.
pub mod alias_group {
    use super::{GroupId, group_id};

    /// `page_id` and `created_time`, fixed at creation.
    pub const CREATED: GroupId = group_id(0);
    /// `alias` and `normalized_alias`.
    pub const ALIAS: GroupId = group_id(1);
    /// `last_edited_time`.
    pub const UPDATED: GroupId = group_id(2);
}

/// Block field groups.
pub mod block_group {
    use super::{GroupId, group_id};

    /// `created_time` and the import source identity, which imports and restores write.
    pub const ORIGIN: GroupId = group_id(0);
    /// `parent_page_id` and `parent_block_id`, exactly one of them set.
    pub const PLACEMENT: GroupId = group_id(1);
    /// `order_key` among the siblings.
    pub const POSITION: GroupId = group_id(2);
    /// `type` and `attrs`.
    pub const KIND: GroupId = group_id(3);
    /// `text`.
    pub const TEXT: GroupId = group_id(4);
    /// `in_trash` and `trash_owner`.
    pub const LIFECYCLE: GroupId = group_id(5);
    /// `last_edited_time`.
    pub const UPDATED: GroupId = group_id(6);
}

/// A hidden, non-recoverable group.
const fn group(
    id: GroupId,
    name: &'static str,
    kind: MergeKind,
    columns: &'static [ColumnSpec],
    references: &'static [ReferenceColumn],
) -> GroupSpec {
    GroupSpec {
        id,
        name,
        kind,
        storage: GroupStorage::Columns(columns),
        references,
        surfaced: false,
        recoverable: false,
        bumps_revision: false,
    }
}

/// A `Register` group without references.
const fn register(id: GroupId, name: &'static str, columns: &'static [ColumnSpec]) -> GroupSpec {
    group(id, name, MergeKind::Register, columns, &[])
}

/// `spec`, shown as a conflict when concurrent versions differ and offered in recovery entries.
const fn surfaced(spec: GroupSpec) -> GroupSpec {
    GroupSpec {
        surfaced: true,
        recoverable: true,
        ..spec
    }
}

/// A one-column reference group.
const fn reference(
    id: GroupId,
    name: &'static str,
    column: &'static [ColumnSpec],
    table: TableId,
    on_delete: OnDelete,
) -> GroupSpec {
    group(
        id,
        name,
        MergeKind::Reference { table, on_delete },
        column,
        &[],
    )
}

/// The `updated` group of a table with `last_edited_time`.
const fn updated(id: GroupId) -> GroupSpec {
    group(id, "updated", MergeKind::Max, &[LAST_EDITED_TIME], &[])
}

const fn reference_column(
    column: &'static str,
    table: TableId,
    on_delete: OnDelete,
) -> ReferenceColumn {
    ReferenceColumn {
        column,
        table,
        on_delete,
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

const fn row_key(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalRowKey,
    }
}

const fn within(columns: &'static [&'static str], group: GroupId) -> Resolution {
    Resolution::WithinGroup { columns, group }
}

/// The `created_time` column of a `created` group.
const CREATED_TIME: ColumnSpec = timestamp("created_time");

/// The `last_edited_time` column of an `updated` group.
const LAST_EDITED_TIME: ColumnSpec = timestamp("last_edited_time");

/// A table keyed by `id` that the Notes adapter checks; each table overrides what it declares.
const BASE: TableSpec = TableSpec {
    id: LOCAL_USERS_TABLE,
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

/// `notes_local_users`.
pub const LOCAL_USERS: TableSpec = TableSpec {
    id: LOCAL_USERS_TABLE,
    name: "notes_local_users",
    groups: &[group(
        local_user_group::CREATED,
        "created",
        MergeKind::Immutable,
        &[CREATED_TIME],
        &[],
    )],
    ..BASE
};

/// `notes_folders`.
pub const FOLDERS: TableSpec = TableSpec {
    id: FOLDERS_TABLE,
    name: "notes_folders",
    groups: &[
        group(
            folder_group::CREATED,
            "created",
            MergeKind::Immutable,
            &[row_key("project_id"), CREATED_TIME],
            &[reference_column(
                "project_id",
                PROJECTS_TABLE,
                OnDelete::Cascade,
            )],
        ),
        register(folder_group::NAME, "name", &[text("name", MAX_NAME_CHARS)]),
        reference(
            folder_group::PARENT,
            "parent",
            &[row_key("parent_folder_id")],
            FOLDERS_TABLE,
            OnDelete::SetNull,
        ),
        updated(folder_group::UPDATED),
    ],
    tree: Some(TreeSpec {
        parent_group: folder_group::PARENT,
        position_group: None,
        max_depth: Some(MAX_FOLDER_DEPTH),
    }),
    ..BASE
};

/// `notes_pages`.
pub const PAGES: TableSpec = TableSpec {
    id: PAGES_TABLE,
    name: "notes_pages",
    groups: &[
        group(
            page_group::CREATED,
            "created",
            MergeKind::Immutable,
            &[CREATED_TIME],
            &[],
        ),
        register(
            page_group::ORIGIN,
            "origin",
            &[
                optional_text("source_provider", MAX_TEXT_CHARS),
                optional_text("source_object_id", MAX_TEXT_CHARS),
                optional_text("source_workspace_id", MAX_TEXT_CHARS),
                optional_text("source_last_edited_time", MAX_TEXT_CHARS),
                optional_text("url", MAX_TEXT_CHARS),
                optional_text("public_url", MAX_TEXT_CHARS),
            ],
        ),
        surfaced(register(
            page_group::TITLE,
            "title",
            &[text("title", MAX_TEXT_CHARS)],
        )),
        group(
            page_group::PLACEMENT,
            "placement",
            MergeKind::Register,
            &[
                row_key("child_block_id"),
                row_key("parent_data_source_id"),
                row_key("folder_id"),
            ],
            &[
                reference_column("child_block_id", BLOCKS_TABLE, OnDelete::Cascade),
                reference_column("folder_id", FOLDERS_TABLE, OnDelete::SetNull),
            ],
        ),
        reference(
            page_group::PROJECT,
            "project",
            &[row_key("project_id")],
            PROJECTS_TABLE,
            OnDelete::SetNull,
        ),
        reference(
            page_group::ROW_PARENT,
            "row_parent",
            &[row_key("parent_row_page_id")],
            PAGES_TABLE,
            OnDelete::SetNull,
        ),
        register(
            page_group::APPEARANCE,
            "appearance",
            &[
                optional_text("icon", MAX_TEXT_CHARS),
                optional_text("cover", MAX_TEXT_CHARS),
            ],
        ),
        group(
            page_group::LIFECYCLE,
            "lifecycle",
            MergeKind::Coupled,
            &[
                flag("in_trash"),
                optional_timestamp("trashed_time"),
                optional_text("trash_owner", MAX_TEXT_CHARS),
            ],
            &[],
        ),
        register(page_group::ARCHIVED, "archived", &[flag("archived")]),
        updated(page_group::UPDATED),
    ],
    hook_columns: &["parent_type", "parent_page_id", "parent_block_id"],
    resolutions: &[
        within(
            &["child_block_id", "parent_data_source_id"],
            page_group::PLACEMENT,
        ),
        within(
            &["folder_id", "child_block_id", "parent_data_source_id"],
            page_group::PLACEMENT,
        ),
    ],
    tree: Some(TreeSpec {
        parent_group: page_group::ROW_PARENT,
        position_group: None,
        max_depth: Some(MAX_ROW_DEPTH),
    }),
    ..BASE
};

/// `notes_page_aliases`.
pub const ALIASES: TableSpec = TableSpec {
    id: ALIASES_TABLE,
    name: "notes_page_aliases",
    groups: &[
        group(
            alias_group::CREATED,
            "created",
            MergeKind::Immutable,
            &[row_key("page_id"), CREATED_TIME],
            &[reference_column("page_id", PAGES_TABLE, OnDelete::Cascade)],
        ),
        register(
            alias_group::ALIAS,
            "alias",
            &[
                text("alias", MAX_NAME_CHARS),
                text("normalized_alias", MAX_TEXT_CHARS),
            ],
        ),
        updated(alias_group::UPDATED),
    ],
    resolutions: &[Resolution::DuplicateRepair {
        columns: &["normalized_alias"],
        collation: Collation::Binary,
    }],
    redirects: true,
    ..BASE
};

/// `notes_blocks`.
pub const BLOCKS: TableSpec = TableSpec {
    id: BLOCKS_TABLE,
    name: "notes_blocks",
    groups: &[
        register(
            block_group::ORIGIN,
            "origin",
            &[
                CREATED_TIME,
                optional_text("source_provider", MAX_TEXT_CHARS),
                optional_text("source_object_id", MAX_TEXT_CHARS),
                optional_text("source_last_edited_time", MAX_TEXT_CHARS),
            ],
        ),
        group(
            block_group::PLACEMENT,
            "placement",
            MergeKind::Register,
            &[row_key("parent_page_id"), row_key("parent_block_id")],
            &[
                reference_column("parent_page_id", PAGES_TABLE, OnDelete::Cascade),
                reference_column("parent_block_id", BLOCKS_TABLE, OnDelete::Cascade),
            ],
        ),
        group(
            block_group::POSITION,
            "position",
            MergeKind::Position,
            &[ColumnSpec {
                name: "order_key",
                ty: ColumnType::OrderKey,
            }],
            &[],
        ),
        register(
            block_group::KIND,
            "kind",
            &[
                text("type", MAX_NAME_CHARS),
                text("attrs", MAX_BLOCK_ATTRS_BYTES),
            ],
        ),
        surfaced(register(
            block_group::TEXT,
            "text",
            &[text("text", MAX_BLOCK_TEXT_BYTES)],
        )),
        register(
            block_group::LIFECYCLE,
            "lifecycle",
            &[
                flag("in_trash"),
                optional_text("trash_owner", MAX_TEXT_CHARS),
            ],
        ),
        updated(block_group::UPDATED),
    ],
    derived_columns: &["parent_type"],
    hook_columns: &["page_id", "plain_text"],
    resolutions: &[within(
        &["parent_page_id", "parent_block_id"],
        block_group::PLACEMENT,
    )],
    ..BASE
};

/// A device table without domain triggers or assets.
const fn device(name: &'static str) -> DeviceTable {
    DeviceTable {
        name,
        triggers: &[],
        assets: &[],
    }
}

/// Notes domain. Only the vault owner writes it until Notes commands refuse owner-only writes
/// on replicas.
pub const DOMAIN: DomainManifest = DomainManifest {
    name: DOMAIN_NAME,
    table_ids: TableIdRange {
        first: 288,
        last: 351,
    },
    tables: &[LOCAL_USERS, FOLDERS, PAGES, BLOCKS, ALIASES],
    derived: &[
        "notes_asset_references",
        "notes_backlink_index",
        "notes_backlink_index_state",
        "notes_data_source_rollup_cache",
        "notes_link_facts",
        "notes_link_facts_state",
        "notes_search_fts",
        "notes_search_index",
        "notes_search_index_state",
        "notes_unresolved_link_index",
        "notes_unresolved_link_index_state",
    ],
    local: &[
        device("notes_block_view_state"),
        device("notes_comment_thread_mutes"),
        device("notes_comment_thread_reads"),
        device("notes_edit_receipts"),
        device("notes_history_bundle_chunks"),
        device("notes_history_bundles"),
        device("notes_history_maintenance_state"),
        device("notes_mention_notifications"),
        device("notes_page_history_pending"),
        device("notes_page_history_settings"),
        device("notes_page_history_snapshots"),
        device("notes_page_notification_settings"),
        device("notes_project_history_asset_pins"),
        device("notes_project_history_bundle_references"),
        device("notes_project_history_dirty"),
        device("notes_project_history_versions"),
        device("notes_sync_repair_pending"),
        device("notes_undo_state"),
    ],
    engine: &[],
    replica_writable: false,
    assets: &[],
};
