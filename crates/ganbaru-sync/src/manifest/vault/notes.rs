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
//! Databases, data sources, properties, options, property values, relation edges, views, and
//! page and data source templates replicate too. A database's parent columns are hook columns
//! copied from its `child_database` block. Properties, options, values, and edges are keyed by
//! the derived key of their identity with an existence register, so a removed row can be added
//! again. A row page cascades from its data source. Template blocks are owned rows of their
//! template's content group (ids 301 and 303 stay reserved for them).
//!
//! Comment threads, their inline anchors, comments, reactions, and suggestions replicate. The
//! page columns of threads, anchors, and suggestions are hook columns derived from the parent
//! page or block. A resolved status, a deletion, and an accepted or rejected suggestion are
//! coupled states the adapter ranks, so concurrent decisions settle the same way everywhere. A
//! reaction is keyed by the derived key of its comment, author, and emoji with an existence
//! register, so toggling it again adds the same row back.
//!
//! Asset rows replicate, keyed by `{class}:{sha256}`, and their bytes travel as three asset
//! classes: page icons, page covers, and files. Block, page, database, data source, value,
//! comment, and template JSON name the files they reference. Project history asset pins are a
//! device table whose asset ids keep pinned files from the sweep.
//!
//! Search, link, backlink, asset reference, and rollup indexes and their state rows are derived.
//! Undo state, edit receipts, read and mute state, notification settings and notifications, view
//! state, and every history table stay on the device. Ids 310 to 351 are reserved.

use ganbaru_sync_contracts::{GroupId, TableId};

use crate::assets::{AssetClass, LocalAssetColumn, LocalDigest, MAX_CONTENT_TYPE_BYTES};
use crate::domains::notes::{
    MAX_BLOCK_ATTRS_BYTES, MAX_BLOCK_TEXT_BYTES, MAX_FILE_BYTES, MAX_NAME_CHARS,
    MAX_PAGE_COVER_BYTES, MAX_PAGE_ICON_BYTES, MAX_TEMPLATE_CONTENT_BYTES, MAX_TEXT_CHARS,
};
use crate::manifest::vault::projects::PROJECTS_TABLE;
use crate::manifest::{
    Collation, ColumnSpec, ColumnType, DerivedKey, DeviceTable, DomainManifest, DomainTrigger,
    GroupSpec, GroupStorage, LocalColumn, MergeKind, OnDelete, OwnedBound, OwnedCodec, OwnedOrder,
    OwnedTable, ReferenceColumn, Resolution, SYNC_KEY_COLUMN, TableIdRange, TableSpec, TreeSpec,
    TriggerPolicy, group_id,
};

/// Domain name for Notes.
pub const DOMAIN_NAME: &str = "notes";

/// Domain adapter name for Notes.
pub const ADAPTER: &str = "notes";

/// Page icon images, at most 3 MiB each.
pub const PAGE_ICONS_CLASS: AssetClass = AssetClass {
    name: "notes/page-icons",
    max_bytes: MAX_PAGE_ICON_BYTES,
};

/// Page and database cover images, at most 8 MiB each.
pub const PAGE_COVERS_CLASS: AssetClass = AssetClass {
    name: "notes/page-covers",
    max_bytes: MAX_PAGE_COVER_BYTES,
};

/// Files attached to blocks, file property values, and comments, at most 50 MiB each.
pub const FILES_CLASS: AssetClass = AssetClass {
    name: "notes/files",
    max_bytes: MAX_FILE_BYTES,
};

/// Names of every Notes asset class, which an asset id `{class}:{sha256}` may name.
pub const ASSET_CLASS_NAMES: &[&str] = &[
    PAGE_ICONS_CLASS.name,
    PAGE_COVERS_CLASS.name,
    FILES_CLASS.name,
];

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
/// `notes_databases`.
pub const DATABASES_TABLE: TableId = TableId(293);
/// `notes_data_sources`.
pub const DATA_SOURCES_TABLE: TableId = TableId(294);
/// `notes_data_source_properties`.
pub const PROPERTIES_TABLE: TableId = TableId(295);
/// `notes_data_source_property_options`.
pub const OPTIONS_TABLE: TableId = TableId(296);
/// `notes_page_property_values`.
pub const VALUES_TABLE: TableId = TableId(297);
/// `notes_relation_edges`.
pub const RELATION_EDGES_TABLE: TableId = TableId(298);
/// `notes_database_views`.
pub const VIEWS_TABLE: TableId = TableId(299);
/// `notes_page_templates`.
pub const PAGE_TEMPLATES_TABLE: TableId = TableId(300);
/// `notes_data_source_templates`.
pub const DATA_SOURCE_TEMPLATES_TABLE: TableId = TableId(302);
/// `notes_comment_threads`.
pub const THREADS_TABLE: TableId = TableId(304);
/// `notes_comment_thread_anchors`.
pub const ANCHORS_TABLE: TableId = TableId(305);
/// `notes_comments`.
pub const COMMENTS_TABLE: TableId = TableId(306);
/// `notes_suggestions`.
pub const SUGGESTIONS_TABLE: TableId = TableId(307);
/// `notes_assets`.
pub const ASSETS_TABLE: TableId = TableId(308);
/// `notes_comment_reactions`.
pub const REACTIONS_TABLE: TableId = TableId(309);

/// Owned `notes_page_template_blocks` of a page template's content (id 301 is reserved for it).
pub const PAGE_TEMPLATE_BLOCKS_OWNED: &str = "notes_page_template_blocks";
/// Owned `notes_data_source_template_blocks` of a data source template's content (id 303 is
/// reserved for it).
pub const DATA_SOURCE_TEMPLATE_BLOCKS_OWNED: &str = "notes_data_source_template_blocks";

/// Derived key prefix of `notes_data_source_properties`.
pub const PROPERTY_KEY_PREFIX: &str = "notes-property";
/// Derived key prefix of `notes_data_source_property_options`.
pub const OPTION_KEY_PREFIX: &str = "notes-property-option";
/// Derived key prefix of `notes_page_property_values`.
pub const VALUE_KEY_PREFIX: &str = "notes-property-value";
/// Derived key prefix of `notes_relation_edges`.
pub const RELATION_EDGE_KEY_PREFIX: &str = "notes-relation-edge";
/// Derived key prefix of `notes_comment_reactions`.
pub const REACTION_KEY_PREFIX: &str = "notes-comment-reaction";

/// Greatest characters of an anchored or suggested text span, as Notes checks it.
pub const MAX_SPAN_CHARS: usize = 2_000;
/// Greatest characters of the context kept before or after a span, as Notes checks it.
pub const MAX_CONTEXT_CHARS: usize = 120;
/// Greatest characters of a reaction emoji, as Notes checks it.
pub const MAX_EMOJI_CHARS: usize = 16;

/// Greatest number of blocks one template holds; template writes refuse more, so the smallest
/// encoded blocks still fit one blob field.
pub const MAX_TEMPLATE_BLOCKS: usize = 1_024;

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

/// Database field groups.
pub mod database_group {
    use super::{GroupId, group_id};

    /// `block_id` and `created_time`, fixed at creation.
    pub const CREATED: GroupId = group_id(0);
    /// Import source identity and the database URLs.
    pub const ORIGIN: GroupId = group_id(1);
    /// `title` and `title_rich_text`.
    pub const TITLE: GroupId = group_id(2);
    /// `description`.
    pub const DESCRIPTION: GroupId = group_id(3);
    /// `icon` and `cover`.
    pub const APPEARANCE: GroupId = group_id(4);
    /// `is_inline`.
    pub const INLINE: GroupId = group_id(5);
    /// `in_trash`.
    pub const LIFECYCLE: GroupId = group_id(6);
    /// `last_edited_time`.
    pub const UPDATED: GroupId = group_id(7);
}

/// Data source field groups.
pub mod data_source_group {
    use super::{GroupId, group_id};

    /// `database_id` and `created_time`, fixed at creation.
    pub const CREATED: GroupId = group_id(0);
    /// Import source identity.
    pub const ORIGIN: GroupId = group_id(1);
    /// `title` and `title_rich_text`.
    pub const TITLE: GroupId = group_id(2);
    /// `description`.
    pub const DESCRIPTION: GroupId = group_id(3);
    /// `icon`.
    pub const ICON: GroupId = group_id(4);
    /// `in_trash`.
    pub const LIFECYCLE: GroupId = group_id(5);
    /// `default_template_id`.
    pub const DEFAULT_TEMPLATE: GroupId = group_id(6);
    /// `last_edited_time`.
    pub const UPDATED: GroupId = group_id(7);
}

/// Data source property field groups.
pub mod property_group {
    use super::{GroupId, group_id};

    /// `data_source_id` and `property_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `type` and `config`.
    pub const CONFIG: GroupId = group_id(2);
    /// `order_key` among the properties of the data source.
    pub const POSITION: GroupId = group_id(3);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(4);
}

/// Property option field groups.
pub mod option_group {
    use super::{GroupId, group_id};

    /// `data_source_id`, `property_id`, and `option_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `property_key`, the `sync_key` of the property.
    pub const PROPERTY: GroupId = group_id(1);
    /// `name`.
    pub const NAME: GroupId = group_id(2);
    /// `color`.
    pub const COLOR: GroupId = group_id(3);
    /// `status_group`.
    pub const STATUS_GROUP: GroupId = group_id(4);
    /// `position` among the options of the property.
    pub const POSITION: GroupId = group_id(5);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(6);
}

/// Page property value field groups.
pub mod value_group {
    use super::{GroupId, group_id};

    /// `page_id` and `property_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `value_type` and `value`.
    pub const VALUE: GroupId = group_id(1);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(2);
}

/// Relation edge field groups.
pub mod relation_edge_group {
    use super::{GroupId, group_id};

    /// `source_page_id`, `source_property_id`, and `target_page_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `created_time`, which orders the links of a property.
    pub const CREATED: GroupId = group_id(1);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(2);
}

/// Database view field groups.
pub mod view_group {
    use super::{GroupId, group_id};

    /// `database_id`, `data_source_id`, and `created_time`, fixed at creation.
    pub const PARENT: GroupId = group_id(0);
    /// Import source identity and the view URL.
    pub const ORIGIN: GroupId = group_id(1);
    /// `name`.
    pub const NAME: GroupId = group_id(2);
    /// `type`, `filter`, `sorts`, and `configuration`, which only make sense together.
    pub const LAYOUT: GroupId = group_id(3);
    /// `order_key` among the views of the database.
    pub const POSITION: GroupId = group_id(4);
    /// `last_edited_time`.
    pub const UPDATED: GroupId = group_id(5);
}

/// Page template field groups.
pub mod page_template_group {
    use super::{GroupId, group_id};

    /// `created_time`.
    pub const CREATED: GroupId = group_id(0);
    /// `source_page_id`.
    pub const SOURCE: GroupId = group_id(1);
    /// `name`.
    pub const NAME: GroupId = group_id(2);
    /// `properties`, `icon`, `cover`, and the template blocks, edited as one snapshot.
    pub const CONTENT: GroupId = group_id(3);
    /// `last_edited_time`.
    pub const UPDATED: GroupId = group_id(4);
}

/// Data source template field groups.
pub mod data_source_template_group {
    use super::{GroupId, group_id};

    /// `data_source_id` and `created_time`, fixed at creation.
    pub const CREATED: GroupId = group_id(0);
    /// `source_page_id`.
    pub const SOURCE: GroupId = group_id(1);
    /// `name`.
    pub const NAME: GroupId = group_id(2);
    /// `properties` and the template blocks, edited as one snapshot.
    pub const CONTENT: GroupId = group_id(3);
    /// `last_edited_time`.
    pub const UPDATED: GroupId = group_id(4);
}

/// Comment thread field groups.
pub mod thread_group {
    use super::{GroupId, group_id};

    /// `parent_type`, `parent_page_id`, `parent_block_id`, and `created_time`, fixed at
    /// creation.
    pub const CREATED: GroupId = group_id(0);
    /// Import source identity.
    pub const ORIGIN: GroupId = group_id(1);
    /// `status`, `resolved_at`, and `resolved_by`.
    pub const STATUS: GroupId = group_id(2);
    /// `last_edited_time`.
    pub const UPDATED: GroupId = group_id(3);
}

/// Comment thread anchor field groups.
pub mod anchor_group {
    use super::{GroupId, group_id};

    /// `created_time`.
    pub const CREATED: GroupId = group_id(0);
    /// `block_id`.
    pub const BLOCK: GroupId = group_id(1);
    /// The offsets and the quoted text with its context.
    pub const RANGE: GroupId = group_id(2);
    /// `last_edited_time`.
    pub const UPDATED: GroupId = group_id(3);
}

/// Comment field groups.
pub mod comment_group {
    use super::{GroupId, group_id};

    /// `thread_id`, the author, and `created_time`, fixed at creation.
    pub const CREATED: GroupId = group_id(0);
    /// Import source identity.
    pub const ORIGIN: GroupId = group_id(1);
    /// `rich_text`, `plain_text`, and `attachments`.
    pub const BODY: GroupId = group_id(2);
    /// `deleted_at`.
    pub const DELETED: GroupId = group_id(3);
    /// `last_edited_time`.
    pub const UPDATED: GroupId = group_id(4);
}

/// Comment reaction field groups.
pub mod reaction_group {
    use super::{GroupId, group_id};

    /// `comment_id`, `created_by`, and `emoji`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `created_time` and `display_name`.
    pub const CREATED: GroupId = group_id(1);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(2);
}

/// Suggestion field groups.
pub mod suggestion_group {
    use super::{GroupId, group_id};

    /// The block, author, range, original and proposed text, and `created_time`, fixed at
    /// creation.
    pub const CONTENT: GroupId = group_id(0);
    /// `status` with the time and person of the decision.
    pub const STATUS: GroupId = group_id(1);
    /// `last_edited_time`.
    pub const UPDATED: GroupId = group_id(2);
}

/// Asset field groups.
pub mod asset_group {
    use super::{GroupId, group_id};

    /// `asset_path`, `kind`, `content_type`, `byte_size`, and `sha256`, computed from the bytes.
    pub const CONTENT: GroupId = group_id(0);
    /// `source_type`, `original_name`, and `created_at`.
    pub const DETAILS: GroupId = group_id(1);
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
    key_parent: None,
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
                reference_column(
                    "parent_data_source_id",
                    DATA_SOURCES_TABLE,
                    OnDelete::Cascade,
                ),
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
    triggers: &[
        gated("notes_pages_local_nesting_insert"),
        gated("notes_pages_local_nesting_update"),
        gated("notes_pages_park_child_page_blocks"),
    ],
    ..BASE
};

/// A domain trigger that does not fire while the engine applies.
const fn gated(name: &'static str) -> DomainTrigger {
    DomainTrigger {
        name,
        policy: TriggerPolicy::Gated,
    }
}

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
        group(
            block_group::LIFECYCLE,
            "lifecycle",
            MergeKind::Coupled,
            &[
                flag("in_trash"),
                optional_text("trash_owner", MAX_TEXT_CHARS),
            ],
            &[],
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

const fn trimmed(name: &'static str, max_chars: usize) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::TrimmedText {
            min_chars: 1,
            max_chars,
        },
    }
}

const fn order_key(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OrderKey,
    }
}

/// An `Immutable` group with its reference columns.
const fn immutable(
    id: GroupId,
    name: &'static str,
    columns: &'static [ColumnSpec],
    references: &'static [ReferenceColumn],
) -> GroupSpec {
    group(id, name, MergeKind::Immutable, columns, references)
}

/// A `Position` group holding one order key column.
const fn position(id: GroupId, column: &'static [ColumnSpec]) -> GroupSpec {
    group(id, "position", MergeKind::Position, column, &[])
}

/// The existence register of a derived-key table.
const fn exists(id: GroupId) -> GroupSpec {
    group(id, "exists", MergeKind::Register, &[], &[])
}

/// The `Coupled` trash flag of a database or data source, where trashing wins.
const fn lifecycle(id: GroupId) -> GroupSpec {
    group(id, "lifecycle", MergeKind::Coupled, &TRASH_FLAG, &[])
}

/// The trash flag of a database or data source.
const TRASH_FLAG: [ColumnSpec; 1] = [flag("in_trash")];

/// The import source identity columns every imported Notes object carries.
const SOURCE_COLUMNS: [ColumnSpec; 4] = [
    optional_text("source_provider", MAX_TEXT_CHARS),
    optional_text("source_object_id", MAX_TEXT_CHARS),
    optional_text("source_workspace_id", MAX_TEXT_CHARS),
    optional_text("source_last_edited_time", MAX_TEXT_CHARS),
];

/// `notes_databases`, keyed by the id of its `child_database` block. Its parent columns are
/// hook columns copied from that block's placement.
pub const DATABASES: TableSpec = TableSpec {
    id: DATABASES_TABLE,
    name: "notes_databases",
    groups: &[
        immutable(
            database_group::CREATED,
            "created",
            &[row_key("block_id"), CREATED_TIME],
            &[reference_column(
                "block_id",
                BLOCKS_TABLE,
                OnDelete::Cascade,
            )],
        ),
        register(
            database_group::ORIGIN,
            "origin",
            &[
                SOURCE_COLUMNS[0],
                SOURCE_COLUMNS[1],
                SOURCE_COLUMNS[2],
                SOURCE_COLUMNS[3],
                optional_text("url", MAX_TEXT_CHARS),
                optional_text("public_url", MAX_TEXT_CHARS),
            ],
        ),
        register(
            database_group::TITLE,
            "title",
            &[
                text("title", MAX_TEXT_CHARS),
                text("title_rich_text", MAX_TEXT_CHARS),
            ],
        ),
        register(
            database_group::DESCRIPTION,
            "description",
            &[text("description", MAX_TEXT_CHARS)],
        ),
        register(
            database_group::APPEARANCE,
            "appearance",
            &[
                optional_text("icon", MAX_TEXT_CHARS),
                optional_text("cover", MAX_TEXT_CHARS),
            ],
        ),
        register(database_group::INLINE, "inline", &[flag("is_inline")]),
        lifecycle(database_group::LIFECYCLE),
        updated(database_group::UPDATED),
    ],
    hook_columns: &["parent_type", "parent_page_id", "parent_block_id"],
    ..BASE
};

/// `notes_data_sources`.
pub const DATA_SOURCES: TableSpec = TableSpec {
    id: DATA_SOURCES_TABLE,
    name: "notes_data_sources",
    groups: &[
        immutable(
            data_source_group::CREATED,
            "created",
            &[row_key("database_id"), CREATED_TIME],
            &[reference_column(
                "database_id",
                DATABASES_TABLE,
                OnDelete::Cascade,
            )],
        ),
        register(data_source_group::ORIGIN, "origin", &SOURCE_COLUMNS),
        register(
            data_source_group::TITLE,
            "title",
            &[
                text("title", MAX_TEXT_CHARS),
                text("title_rich_text", MAX_TEXT_CHARS),
            ],
        ),
        register(
            data_source_group::DESCRIPTION,
            "description",
            &[text("description", MAX_TEXT_CHARS)],
        ),
        register(
            data_source_group::ICON,
            "icon",
            &[optional_text("icon", MAX_TEXT_CHARS)],
        ),
        lifecycle(data_source_group::LIFECYCLE),
        reference(
            data_source_group::DEFAULT_TEMPLATE,
            "default_template",
            &[row_key("default_template_id")],
            DATA_SOURCE_TEMPLATES_TABLE,
            OnDelete::SetNull,
        ),
        updated(data_source_group::UPDATED),
    ],
    ..BASE
};

/// Columns of a template block, in value field order.
pub const TEMPLATE_BLOCK_COLUMNS: [ColumnSpec; 9] = [
    trimmed("id", MAX_NAME_CHARS),
    text("parent_type", MAX_NAME_CHARS),
    optional_text("parent_block_id", MAX_NAME_CHARS),
    flag("has_children"),
    text("type", MAX_NAME_CHARS),
    text("payload", MAX_TEMPLATE_CONTENT_BYTES),
    text("plain_text", MAX_TEMPLATE_CONTENT_BYTES),
    CREATED_TIME,
    LAST_EDITED_TIME,
];

/// An owned template block table: a template's blocks in preorder. Notes keeps every template
/// within its content bound, and the adapter checks that ids are unique and parents exist.
const fn template_blocks(table: &'static str) -> OwnedTable {
    OwnedTable {
        table,
        parent_column: "template_id",
        order: OwnedOrder::Ordinal("ordinal"),
        columns: &TEMPLATE_BLOCK_COLUMNS,
        references: &[],
        unique: &[],
        max_rows: MAX_TEMPLATE_BLOCKS,
        codec: OwnedCodec::Generic,
        nested: &[],
        bound: OwnedBound::Domain,
    }
}

/// The blocks of a page template.
pub const PAGE_TEMPLATE_BLOCKS: OwnedTable = template_blocks(PAGE_TEMPLATE_BLOCKS_OWNED);

/// The blocks of a data source template.
pub const DATA_SOURCE_TEMPLATE_BLOCKS: OwnedTable =
    template_blocks(DATA_SOURCE_TEMPLATE_BLOCKS_OWNED);

/// `notes_data_source_templates`. A template is edited as a whole snapshot, so its properties
/// and blocks are one group.
pub const DATA_SOURCE_TEMPLATES: TableSpec = TableSpec {
    id: DATA_SOURCE_TEMPLATES_TABLE,
    name: "notes_data_source_templates",
    groups: &[
        immutable(
            data_source_template_group::CREATED,
            "created",
            &[row_key("data_source_id"), CREATED_TIME],
            &[reference_column(
                "data_source_id",
                DATA_SOURCES_TABLE,
                OnDelete::Cascade,
            )],
        ),
        reference(
            data_source_template_group::SOURCE,
            "source",
            &[row_key("source_page_id")],
            PAGES_TABLE,
            OnDelete::SetNull,
        ),
        register(
            data_source_template_group::NAME,
            "name",
            &[text("name", MAX_NAME_CHARS)],
        ),
        surfaced(GroupSpec {
            storage: GroupStorage::Owned {
                columns: &[text("properties", MAX_TEMPLATE_CONTENT_BYTES)],
                tables: &[DATA_SOURCE_TEMPLATE_BLOCKS],
            },
            ..register(data_source_template_group::CONTENT, "content", &[])
        }),
        updated(data_source_template_group::UPDATED),
    ],
    ..BASE
};

/// `notes_page_templates`. A template is edited as a whole snapshot, so its properties,
/// appearance, and blocks are one group.
pub const PAGE_TEMPLATES: TableSpec = TableSpec {
    id: PAGE_TEMPLATES_TABLE,
    name: "notes_page_templates",
    groups: &[
        group(
            page_template_group::CREATED,
            "created",
            MergeKind::Immutable,
            &[CREATED_TIME],
            &[],
        ),
        reference(
            page_template_group::SOURCE,
            "source",
            &[row_key("source_page_id")],
            PAGES_TABLE,
            OnDelete::SetNull,
        ),
        register(
            page_template_group::NAME,
            "name",
            &[trimmed("name", MAX_NAME_CHARS)],
        ),
        surfaced(GroupSpec {
            storage: GroupStorage::Owned {
                columns: &[
                    text("properties", MAX_TEMPLATE_CONTENT_BYTES),
                    optional_text("icon", MAX_TEXT_CHARS),
                    optional_text("cover", MAX_TEXT_CHARS),
                ],
                tables: &[PAGE_TEMPLATE_BLOCKS],
            },
            ..register(page_template_group::CONTENT, "content", &[])
        }),
        updated(page_template_group::UPDATED),
    ],
    ..BASE
};

/// A composite-key Notes table keyed by `sync_key`.
const KEYED: TableSpec = TableSpec {
    key_column: SYNC_KEY_COLUMN,
    ..BASE
};

/// `notes_data_source_properties`, keyed by data source and property id. Names are unique per
/// data source only through Notes writes, so reads suffix a name that a merge repeats.
pub const PROPERTIES: TableSpec = TableSpec {
    id: PROPERTIES_TABLE,
    name: "notes_data_source_properties",
    groups: &[
        immutable(
            property_group::IDENTITY,
            "identity",
            &[
                row_key("data_source_id"),
                trimmed("property_id", MAX_NAME_CHARS),
            ],
            &[reference_column(
                "data_source_id",
                DATA_SOURCES_TABLE,
                OnDelete::Cascade,
            )],
        ),
        register(
            property_group::NAME,
            "name",
            &[trimmed("name", MAX_NAME_CHARS)],
        ),
        surfaced(register(
            property_group::CONFIG,
            "config",
            &[
                trimmed("type", MAX_NAME_CHARS),
                text("config", MAX_TEXT_CHARS),
            ],
        )),
        position(property_group::POSITION, &[order_key("order_key")]),
        exists(property_group::EXISTS),
    ],
    derived_key: Some(DerivedKey {
        exists_group: property_group::EXISTS,
        prefix: Some(PROPERTY_KEY_PREFIX),
        key_inputs: &["data_source_id", "property_id"],
    }),
    ..KEYED
};

/// `notes_data_source_property_options`, keyed by data source, property, and option id. An
/// option names its property by the property's `sync_key`, so it goes with the property, and
/// concurrent option adds both survive.
pub const OPTIONS: TableSpec = TableSpec {
    id: OPTIONS_TABLE,
    name: "notes_data_source_property_options",
    groups: &[
        immutable(
            option_group::IDENTITY,
            "identity",
            &[
                text("data_source_id", MAX_NAME_CHARS),
                text("property_id", MAX_NAME_CHARS),
                trimmed("option_id", MAX_NAME_CHARS),
            ],
            &[],
        ),
        reference(
            option_group::PROPERTY,
            "property",
            &[row_key("property_key")],
            PROPERTIES_TABLE,
            OnDelete::Cascade,
        ),
        register(option_group::NAME, "name", &[text("name", MAX_NAME_CHARS)]),
        register(
            option_group::COLOR,
            "color",
            &[text("color", MAX_NAME_CHARS)],
        ),
        register(
            option_group::STATUS_GROUP,
            "status_group",
            &[optional_text("status_group", MAX_NAME_CHARS)],
        ),
        position(option_group::POSITION, &[order_key("position")]),
        exists(option_group::EXISTS),
    ],
    derived_key: Some(DerivedKey {
        exists_group: option_group::EXISTS,
        prefix: Some(OPTION_KEY_PREFIX),
        key_inputs: &["data_source_id", "property_id", "option_id"],
    }),
    ..KEYED
};

/// `notes_page_property_values`, keyed by page and property id. A value keeps its own type, so
/// a concurrent property type change leaves it stored and readable as a mismatch.
pub const VALUES: TableSpec = TableSpec {
    id: VALUES_TABLE,
    name: "notes_page_property_values",
    groups: &[
        immutable(
            value_group::IDENTITY,
            "identity",
            &[row_key("page_id"), trimmed("property_id", MAX_NAME_CHARS)],
            &[reference_column("page_id", PAGES_TABLE, OnDelete::Cascade)],
        ),
        surfaced(register(
            value_group::VALUE,
            "value",
            &[
                trimmed("value_type", MAX_NAME_CHARS),
                text("value", MAX_TEXT_CHARS),
            ],
        )),
        exists(value_group::EXISTS),
    ],
    derived_key: Some(DerivedKey {
        exists_group: value_group::EXISTS,
        prefix: Some(VALUE_KEY_PREFIX),
        key_inputs: &["page_id", "property_id"],
    }),
    ..KEYED
};

/// `notes_relation_edges`, keyed by source page, source property, and target page.
pub const RELATION_EDGES: TableSpec = TableSpec {
    id: RELATION_EDGES_TABLE,
    name: "notes_relation_edges",
    groups: &[
        immutable(
            relation_edge_group::IDENTITY,
            "identity",
            &[
                row_key("source_page_id"),
                trimmed("source_property_id", MAX_NAME_CHARS),
                row_key("target_page_id"),
            ],
            &[
                reference_column("source_page_id", PAGES_TABLE, OnDelete::Cascade),
                reference_column("target_page_id", PAGES_TABLE, OnDelete::Cascade),
            ],
        ),
        register(relation_edge_group::CREATED, "created", &[CREATED_TIME]),
        exists(relation_edge_group::EXISTS),
    ],
    derived_key: Some(DerivedKey {
        exists_group: relation_edge_group::EXISTS,
        prefix: Some(RELATION_EDGE_KEY_PREFIX),
        key_inputs: &["source_page_id", "source_property_id", "target_page_id"],
    }),
    ..KEYED
};

/// `notes_database_views`.
pub const VIEWS: TableSpec = TableSpec {
    id: VIEWS_TABLE,
    name: "notes_database_views",
    groups: &[
        immutable(
            view_group::PARENT,
            "parent",
            &[
                row_key("database_id"),
                row_key("data_source_id"),
                CREATED_TIME,
            ],
            &[
                reference_column("database_id", DATABASES_TABLE, OnDelete::Cascade),
                reference_column("data_source_id", DATA_SOURCES_TABLE, OnDelete::Cascade),
            ],
        ),
        register(
            view_group::ORIGIN,
            "origin",
            &[
                SOURCE_COLUMNS[0],
                SOURCE_COLUMNS[1],
                SOURCE_COLUMNS[2],
                SOURCE_COLUMNS[3],
                optional_text("url", MAX_TEXT_CHARS),
            ],
        ),
        register(view_group::NAME, "name", &[text("name", MAX_NAME_CHARS)]),
        group(
            view_group::LAYOUT,
            "layout",
            MergeKind::Coupled,
            &[
                text("type", MAX_NAME_CHARS),
                optional_text("filter", MAX_TEXT_CHARS),
                text("sorts", MAX_TEXT_CHARS),
                optional_text("configuration", MAX_TEXT_CHARS),
            ],
            &[],
        ),
        position(view_group::POSITION, &[order_key("order_key")]),
        updated(view_group::UPDATED),
    ],
    ..BASE
};

/// A non-negative integer offset into block text.
const fn offset(name: &'static str, min: i64) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Integer { min, max: i64::MAX },
    }
}

/// `notes_comment_threads`. A thread's `page_id` is a hook column: its parent page, or the page
/// of its parent block.
pub const THREADS: TableSpec = TableSpec {
    id: THREADS_TABLE,
    name: "notes_comment_threads",
    groups: &[
        immutable(
            thread_group::CREATED,
            "created",
            &[
                text("parent_type", MAX_NAME_CHARS),
                row_key("parent_page_id"),
                row_key("parent_block_id"),
                CREATED_TIME,
            ],
            &[
                reference_column("parent_page_id", PAGES_TABLE, OnDelete::Cascade),
                reference_column("parent_block_id", BLOCKS_TABLE, OnDelete::Cascade),
            ],
        ),
        register(thread_group::ORIGIN, "origin", &SOURCE_COLUMNS),
        surfaced(group(
            thread_group::STATUS,
            "status",
            MergeKind::Coupled,
            &[
                text("status", MAX_NAME_CHARS),
                optional_timestamp("resolved_at"),
                optional_text("resolved_by", MAX_TEXT_CHARS),
            ],
            &[],
        )),
        updated(thread_group::UPDATED),
    ],
    hook_columns: &["page_id"],
    resolutions: &[
        within(
            &["parent_type", "parent_page_id", "parent_block_id"],
            thread_group::CREATED,
        ),
        within(
            &["status", "resolved_at", "resolved_by"],
            thread_group::STATUS,
        ),
    ],
    ..BASE
};

/// `notes_comment_thread_anchors`, keyed by its thread. Its `page_id` is a hook column: the page
/// of its block.
pub const ANCHORS: TableSpec = TableSpec {
    id: ANCHORS_TABLE,
    name: "notes_comment_thread_anchors",
    key_column: "thread_id",
    key_parent: Some(THREADS_TABLE),
    groups: &[
        immutable(anchor_group::CREATED, "created", &[CREATED_TIME], &[]),
        reference(
            anchor_group::BLOCK,
            "block",
            &[row_key("block_id")],
            BLOCKS_TABLE,
            OnDelete::Cascade,
        ),
        register(
            anchor_group::RANGE,
            "range",
            &[
                offset("start_offset", 0),
                offset("end_offset", 1),
                text("anchor_text", MAX_SPAN_CHARS),
                text("prefix_text", MAX_CONTEXT_CHARS),
                text("suffix_text", MAX_CONTEXT_CHARS),
            ],
        ),
        updated(anchor_group::UPDATED),
    ],
    hook_columns: &["page_id"],
    resolutions: &[within(&["start_offset", "end_offset"], anchor_group::RANGE)],
    ..BASE
};

/// `notes_comments`. A deleted comment keeps its row, so a deletion wins over a concurrent edit.
pub const COMMENTS: TableSpec = TableSpec {
    id: COMMENTS_TABLE,
    name: "notes_comments",
    groups: &[
        immutable(
            comment_group::CREATED,
            "created",
            &[
                row_key("thread_id"),
                text("created_by", MAX_NAME_CHARS),
                optional_text("display_name", MAX_TEXT_CHARS),
                CREATED_TIME,
            ],
            &[reference_column(
                "thread_id",
                THREADS_TABLE,
                OnDelete::Cascade,
            )],
        ),
        register(comment_group::ORIGIN, "origin", &SOURCE_COLUMNS),
        surfaced(register(
            comment_group::BODY,
            "body",
            &[
                text("rich_text", MAX_TEXT_CHARS),
                text("plain_text", MAX_TEXT_CHARS),
                text("attachments", MAX_TEXT_CHARS),
            ],
        )),
        group(
            comment_group::DELETED,
            "deleted",
            MergeKind::Coupled,
            &[optional_timestamp("deleted_at")],
            &[],
        ),
        updated(comment_group::UPDATED),
    ],
    ..BASE
};

/// `notes_comment_reactions`, keyed by comment, author, and emoji. Removing a reaction writes
/// its existence register, so adding it again revives the same row.
pub const REACTIONS: TableSpec = TableSpec {
    id: REACTIONS_TABLE,
    name: "notes_comment_reactions",
    groups: &[
        immutable(
            reaction_group::IDENTITY,
            "identity",
            &[
                row_key("comment_id"),
                text("created_by", MAX_NAME_CHARS),
                text("emoji", MAX_EMOJI_CHARS),
            ],
            &[reference_column(
                "comment_id",
                COMMENTS_TABLE,
                OnDelete::Cascade,
            )],
        ),
        register(
            reaction_group::CREATED,
            "created",
            &[CREATED_TIME, optional_text("display_name", MAX_TEXT_CHARS)],
        ),
        exists(reaction_group::EXISTS),
    ],
    derived_key: Some(DerivedKey {
        exists_group: reaction_group::EXISTS,
        prefix: Some(REACTION_KEY_PREFIX),
        key_inputs: &["comment_id", "created_by", "emoji"],
    }),
    ..KEYED
};

/// `notes_suggestions`. A suggestion's content is fixed; only its decision changes, and an
/// acceptance wins over a concurrent rejection. Its `page_id` is a hook column: the page of its
/// block.
pub const SUGGESTIONS: TableSpec = TableSpec {
    id: SUGGESTIONS_TABLE,
    name: "notes_suggestions",
    groups: &[
        immutable(
            suggestion_group::CONTENT,
            "content",
            &[
                row_key("block_id"),
                row_key("created_by"),
                optional_text("display_name", MAX_TEXT_CHARS),
                offset("range_start", 0),
                offset("range_end", 1),
                text("original_text", MAX_SPAN_CHARS),
                text("proposed_text", MAX_SPAN_CHARS),
                text("prefix_text", MAX_CONTEXT_CHARS),
                text("suffix_text", MAX_CONTEXT_CHARS),
                CREATED_TIME,
            ],
            &[
                reference_column("block_id", BLOCKS_TABLE, OnDelete::Cascade),
                reference_column("created_by", LOCAL_USERS_TABLE, OnDelete::Cascade),
            ],
        ),
        group(
            suggestion_group::STATUS,
            "status",
            MergeKind::Coupled,
            &[
                text("status", MAX_NAME_CHARS),
                optional_timestamp("accepted_at"),
                row_key("accepted_by"),
                optional_timestamp("rejected_at"),
                row_key("rejected_by"),
            ],
            &[
                reference_column("accepted_by", LOCAL_USERS_TABLE, OnDelete::SetNull),
                reference_column("rejected_by", LOCAL_USERS_TABLE, OnDelete::SetNull),
            ],
        ),
        updated(suggestion_group::UPDATED),
    ],
    hook_columns: &["page_id"],
    resolutions: &[
        within(&["range_start", "range_end"], suggestion_group::CONTENT),
        within(
            &[
                "status",
                "accepted_at",
                "accepted_by",
                "rejected_at",
                "rejected_by",
            ],
            suggestion_group::STATUS,
        ),
    ],
    ..BASE
};

/// `notes_assets`, keyed by its asset id `{class}:{sha256}`. The content group is immutable:
/// when two devices add the same bytes under different content types, one write wins
/// everywhere and references under the other path still resolve the row by digest; the source
/// type, original name, and creation time are a register. A row is never deleted.
/// Whether this device holds the bytes (`storage_state` and `missing_at`) and when this device
/// last changed the row (`updated_at`, which the Notes hook stamps on arrival) stay local.
pub const ASSETS: TableSpec = TableSpec {
    id: ASSETS_TABLE,
    name: "notes_assets",
    groups: &[
        immutable(
            asset_group::CONTENT,
            "content",
            &[
                text("asset_path", MAX_NAME_CHARS),
                text("kind", MAX_NAME_CHARS),
                text("content_type", MAX_CONTENT_TYPE_BYTES),
                ColumnSpec {
                    name: "byte_size",
                    ty: ColumnType::Integer {
                        min: 1,
                        max: MAX_FILE_BYTES as i64,
                    },
                },
                text("sha256", MAX_NAME_CHARS),
            ],
            &[],
        ),
        register(
            asset_group::DETAILS,
            "details",
            &[
                text("source_type", MAX_NAME_CHARS),
                optional_text("original_name", MAX_TEXT_CHARS),
                timestamp("created_at"),
            ],
        ),
    ],
    local_columns: &[
        LocalColumn {
            name: "storage_state",
            materialize: "'available'",
        },
        LocalColumn {
            name: "missing_at",
            materialize: "NULL",
        },
        LocalColumn {
            name: "updated_at",
            materialize: "'1970-01-01T00:00:00.000Z'",
        },
    ],
    resolutions: &[within(&["kind", "content_type"], asset_group::CONTENT)],
    ..BASE
};

/// The managed files a page's undo and redo entries name: the block snapshots they hold carry
/// each file's bare digest, which keeps a file of that digest in every Notes class.
const UNDO_STATE_ASSETS: [LocalAssetColumn; 3] = [
    undo_state_asset(PAGE_ICONS_CLASS.name),
    undo_state_asset(PAGE_COVERS_CLASS.name),
    undo_state_asset(FILES_CLASS.name),
];

/// A digest anywhere in the undo state payload naming a file of `class`.
const fn undo_state_asset(class: &'static str) -> LocalAssetColumn {
    LocalAssetColumn {
        column: "state_payload",
        json_path: Some("$"),
        digest: LocalDigest::Sha256 { class },
    }
}

/// The history retention every device reads before it keeps page history.
const PAGE_HISTORY_SETTINGS_SEED: &str =
    "INSERT OR IGNORE INTO notes_page_history_settings (id, retention_days) VALUES (1, 30)";

/// The history maintenance state every device reads before it prunes page history.
const HISTORY_MAINTENANCE_SEED: &str =
    "INSERT OR IGNORE INTO notes_history_maintenance_state (id, last_run_at) VALUES (1, NULL)";

/// A device table without domain triggers or assets.
const fn device(name: &'static str) -> DeviceTable {
    DeviceTable {
        name,
        seed: None,
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
    tables: &[
        LOCAL_USERS,
        FOLDERS,
        PAGES,
        BLOCKS,
        DATABASES,
        DATA_SOURCES,
        DATA_SOURCE_TEMPLATES,
        ALIASES,
        PROPERTIES,
        OPTIONS,
        VALUES,
        RELATION_EDGES,
        VIEWS,
        PAGE_TEMPLATES,
        THREADS,
        ANCHORS,
        COMMENTS,
        REACTIONS,
        SUGGESTIONS,
        ASSETS,
    ],
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
        DeviceTable {
            seed: Some(HISTORY_MAINTENANCE_SEED),
            ..device("notes_history_maintenance_state")
        },
        device("notes_mention_notifications"),
        device("notes_page_history_pending"),
        DeviceTable {
            seed: Some(PAGE_HISTORY_SETTINGS_SEED),
            ..device("notes_page_history_settings")
        },
        DeviceTable {
            name: "notes_page_history_snapshots",
            seed: None,
            triggers: &[],
            assets: &[LocalAssetColumn {
                column: "asset_ids",
                json_path: Some("$"),
                digest: LocalDigest::AssetId {
                    classes: ASSET_CLASS_NAMES,
                },
            }],
        },
        device("notes_page_notification_settings"),
        DeviceTable {
            name: "notes_project_history_asset_pins",
            seed: None,
            triggers: &[],
            assets: &[LocalAssetColumn {
                column: "asset_id",
                json_path: None,
                digest: LocalDigest::AssetId {
                    classes: ASSET_CLASS_NAMES,
                },
            }],
        },
        device("notes_project_history_bundle_references"),
        device("notes_project_history_dirty"),
        device("notes_project_history_versions"),
        device("notes_sync_repair_pending"),
        DeviceTable {
            name: "notes_undo_state",
            seed: None,
            triggers: &[],
            assets: &UNDO_STATE_ASSETS,
        },
    ],
    engine: &[],
    unconverted: &[],
    assets: &[PAGE_ICONS_CLASS, PAGE_COVERS_CLASS, FILES_CLASS],
};
