//! The replication manifest: table classifications, field groups, and merge kinds.
//!
//! A manifest version fixes table ids, group ids, column membership, and value rules, and ids are
//! never reused. Operations of every version up to the current one validate against the current
//! manifest. A version no release carries yet changes in place, tightening included, because no
//! device outside development holds its operations. Once a release carries a version, it never
//! changes, and a later version may only add tables and groups or loosen rules; tightening a rule
//! then needs per-version validation first. A conformance test records each version's canonical
//! [`Manifest::summary`], so a change to the summary fails until the recorded snapshot is updated
//! (in place for an unreleased version, under a version bump otherwise).
//!
//! Each domain declares its tables in one [`DomainManifest`]; a manifest flattens a list of
//! domains in dependency order.

use std::collections::BTreeMap;

#[cfg(doc)]
use ganbaru_sync_contracts::bounds::MAX_BLOB_FIELD_BYTES;
use ganbaru_sync_contracts::bounds::{
    MAX_ORDER_KEY_BYTES, MAX_ROW_KEY_BYTES, MAX_TEXT_FIELD_BYTES,
};
use ganbaru_sync_contracts::{GroupId, GroupMask, TableId};

use crate::assets::{AssetClass, LocalAssetColumn};
use crate::domains::themes::is_theme_id;

pub mod order;
pub mod summary;
mod validate;
pub mod vault;

#[cfg(test)]
pub(crate) use validate::owned_table_problems;

/// How the engine treats a table in the vault schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    /// Rows replicate through operations.
    Replicated,
    /// Rows belong to a group of a replicated parent row and replicate inside that group's value.
    OwnedChildren,
    /// Rows are a projection maintained from replicated rows, such as a search index.
    Derived,
    /// Engine state in `sync_*` tables, or a domain's engine-side table.
    Engine,
    /// Device-bound rows that never replicate, are written on every device, and are emptied in
    /// snapshot copies.
    Local,
    /// Rows of a table no domain declares, which never replicate.
    Unconverted,
}

/// Engine tables every manifest declares.
pub const SYNC_TABLES: &[&str] = &[
    "sync_spaces",
    "sync_apply_state",
    "sync_capture",
    "sync_writers",
    "sync_ops",
    "sync_rows",
    "sync_register_versions",
    "sync_version_refs",
    "sync_tombstones",
    "sync_seeded_tables",
    "sync_stable",
    "sync_auth_envelopes",
    "sync_auth_wanted",
    "sync_auth_refetch",
    "sync_auth_selected",
    "sync_devices",
    "sync_displaced_versions",
    "sync_late_changes",
    "sync_fork_evidence",
    "sync_row_creations",
    "sync_stale_rows",
    "sync_refused_rows",
    "sync_unique_classes",
    "sync_contested_classes",
    "sync_unique_class_rules",
    "sync_recovery_kit",
];

/// Engine tables of device-bound state that every manifest classifies as `Local`, so snapshot
/// copies empty them.
pub const SYNC_LOCAL_TABLES: &[&str] = &[
    "sync_anchor_marks",
    "sync_restated_groups",
    "sync_released_marks",
    "sync_space_keys",
];

/// What a domain trigger may do, so applying remote changes cannot be broken by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerPolicy {
    /// Writes only `Local` or derived columns of the triggering row, and keeps firing during
    /// apply.
    SameRow,
    /// Its `WHEN` clause requires that the engine is not applying.
    Gated,
    /// Writes nothing and only aborts, so the applier sees it and never produces the state it
    /// refuses.
    ApplierVisible,
    /// Writes only `Derived` tables and keeps firing during apply.
    DerivedIndex,
}

/// A non-engine trigger on a converted, `Local`, or domain engine table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainTrigger {
    /// Trigger name.
    pub name: &'static str,
    /// What the trigger may do.
    pub policy: TriggerPolicy,
}

/// A table of a domain that is not replicated, with the domain triggers installed on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceTable {
    /// SQL table name.
    pub name: &'static str,
    /// Statement that writes the row readers of a `Local` table require, skipping a row the
    /// device already has, and matching the row a new vault's schema seeds. `None` when the
    /// table starts empty.
    pub seed: Option<&'static str>,
    /// Domain triggers on the table.
    pub triggers: &'static [DomainTrigger],
    /// Columns of a `Local` or declared unconverted table naming managed asset files the sweep
    /// keeps. Empty on engine tables.
    pub assets: &'static [LocalAssetColumn],
}

/// A column of a replicated table kept per device and never replicated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalColumn {
    /// Column name.
    pub name: &'static str,
    /// SQL literal written when materialize inserts a row from a remote create and when a
    /// snapshot copy is prepared: an integer, `NULL`, or a single-quoted string.
    pub materialize: &'static str,
}

/// A column of a replicated table that holds a fixed prefix followed by the row key, outside
/// every group. Materialize writes it when it inserts a row, and the schema pins it with
/// `CHECK (column = 'prefix' || key)`, so it never changes and is left out of values, codecs,
/// and the summary. Its unique index and CHECK are implied by the key and need no resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyDerivedColumn {
    /// Column name.
    pub name: &'static str,
    /// Text written before the row key, bound as a parameter.
    pub prefix: &'static str,
}

impl KeyDerivedColumn {
    /// The column value of a row key.
    pub fn value(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }
}

/// Whether text is one SQL literal a [`LocalColumn`] may declare.
pub fn is_sql_literal(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return true;
    }
    if text == "NULL" {
        return true;
    }
    text.len() >= 2
        && text.starts_with('\'')
        && text.ends_with('\'')
        && !text[1..text.len() - 1].replace("''", "").contains('\'')
}

/// An inclusive range of table ids reserved for one domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableIdRange {
    /// First reserved id.
    pub first: u16,
    /// Last reserved id.
    pub last: u16,
}

impl TableIdRange {
    /// Whether the range holds an id.
    pub const fn contains(self, id: TableId) -> bool {
        self.first <= id.0 && id.0 <= self.last
    }

    /// Whether two ranges share an id.
    pub const fn overlaps(self, other: Self) -> bool {
        self.first <= other.last && other.first <= self.last
    }
}

/// One domain's part of a manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainManifest {
    /// Domain name, used by the app to admit replica writes.
    pub name: &'static str,
    /// Table ids reserved for the domain.
    pub table_ids: TableIdRange,
    /// Replicated tables in dependency order (see [`order`]).
    pub tables: &'static [TableSpec],
    /// Derived tables, including virtual tables whose shadow tables follow them.
    pub derived: &'static [&'static str],
    /// `Local` tables.
    pub local: &'static [DeviceTable],
    /// Engine-side tables of the domain outside `sync_*`.
    pub engine: &'static [DeviceTable],
    /// Tables of the domain that sync has not converted yet and that travel with the vault as
    /// they are, declared only so the sweep keeps the asset files their columns name. They are
    /// neither `Local` nor engine tables, so snapshot copies and carry-forward leave their rows
    /// alone. They carry no triggers.
    pub unconverted: &'static [DeviceTable],
    /// Managed asset classes whose files the domain's values reference.
    pub assets: &'static [AssetClass],
}

/// Largest timestamp text in bytes.
pub const MAX_TIMESTAMP_BYTES: usize = 64;

/// Value rule for one column of a field group.
///
/// Text never contains NUL and never exceeds the operation text field bound. Blank means only
/// U+0020 spaces, as SQLite `trim` defines it. Value guard triggers enforce the same rules on
/// every local write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    /// Text of at most `max_chars` characters.
    Text {
        /// Character limit.
        max_chars: usize,
    },
    /// Null or a [`ColumnType::Text`] value.
    OptionalText {
        /// Character limit.
        max_chars: usize,
    },
    /// Text whose space-trimmed form has `min_chars..=max_chars` characters.
    TrimmedText {
        /// Lower character bound after trimming.
        min_chars: usize,
        /// Upper character bound after trimming.
        max_chars: usize,
    },
    /// Integer in `min..=max`.
    Integer {
        /// Lower bound.
        min: i64,
        /// Upper bound.
        max: i64,
    },
    /// Null or a [`ColumnType::Integer`] value.
    OptionalInteger {
        /// Lower bound.
        min: i64,
        /// Upper bound.
        max: i64,
    },
    /// Integer 0 or 1.
    Flag,
    /// Non-blank text of at most [`MAX_TIMESTAMP_BYTES`].
    Timestamp,
    /// Null or a [`ColumnType::Timestamp`] value.
    OptionalTimestamp,
    /// An order key.
    OrderKey,
    /// Null or the row key of a referenced row.
    OptionalRowKey,
    /// A row key that is never null. Used for every reference and key column that is NOT NULL
    /// in its domain table.
    RowKey,
}

impl ColumnType {
    /// Whether the column holds a row key, required or nullable.
    pub const fn is_row_key(self) -> bool {
        matches!(self, Self::RowKey | Self::OptionalRowKey)
    }

    /// Whether the column holds text, so comparisons must ignore declared collations.
    pub const fn is_text(self) -> bool {
        !matches!(
            self,
            Self::Integer { .. } | Self::OptionalInteger { .. } | Self::Flag
        )
    }

    /// Text that moves a unique value aside while merged rows swap values, numbered by `n`, or
    /// `None` when the column cannot hold it. It starts with U+007F, which user text avoids.
    pub fn placeholder(self, n: u64) -> Option<String> {
        let (min, max) = match self {
            Self::Text { max_chars } | Self::OptionalText { max_chars } => (0, max_chars),
            Self::TrimmedText {
                min_chars,
                max_chars,
            } => (min_chars, max_chars),
            _ => return None,
        };
        let digits = n.to_string();
        let chars = (1 + digits.len()).max(min);
        (chars <= max).then(|| format!("\u{7f}{digits:0>width$}", width = chars - 1))
    }

    /// Largest encoded size of one field of this type inside a value: a tag byte, then an
    /// integer or a length-prefixed UTF-8 text.
    pub const fn max_field_bytes(self) -> usize {
        const TAG: usize = 1;
        const INTEGER: usize = 8;
        const LENGTH: usize = 4;
        const MAX_UTF8_BYTES_PER_CHAR: usize = 4;
        let text_bytes = match self {
            Self::Integer { .. } | Self::OptionalInteger { .. } | Self::Flag => {
                return TAG + INTEGER;
            }
            Self::Text { max_chars }
            | Self::OptionalText { max_chars }
            | Self::TrimmedText { max_chars, .. } => {
                let bytes = max_chars.saturating_mul(MAX_UTF8_BYTES_PER_CHAR);
                if bytes < MAX_TEXT_FIELD_BYTES {
                    bytes
                } else {
                    MAX_TEXT_FIELD_BYTES
                }
            }
            Self::Timestamp | Self::OptionalTimestamp => MAX_TIMESTAMP_BYTES,
            Self::OrderKey => MAX_ORDER_KEY_BYTES,
            Self::RowKey | Self::OptionalRowKey => MAX_ROW_KEY_BYTES,
        };
        TAG + LENGTH + text_bytes
    }
}

/// One column of a field group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnSpec {
    /// Column name.
    pub name: &'static str,
    /// Value rule.
    pub ty: ColumnType,
}

/// Where a group's value lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupStorage {
    /// Columns of the replicated row, in value field order.
    Columns(&'static [ColumnSpec]),
    /// Columns of the replicated row, possibly none, followed by owned child tables. The value
    /// holds the column fields in order, then one blob field per owned table.
    Owned {
        /// Columns of the replicated row.
        columns: &'static [ColumnSpec],
        /// Owned child tables.
        tables: &'static [OwnedTable],
    },
}

/// An owned child table whose rows, in order, form one blob field of a group value of their
/// owner row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnedTable {
    /// Child table.
    pub table: &'static str,
    /// Child column holding the owner row key.
    pub parent_column: &'static str,
    /// How rows are keyed and ordered within their owner.
    pub order: OwnedOrder,
    /// Child data columns in field order. Key columns of [`OwnedOrder::Key`] are among them;
    /// ordinal columns are not.
    pub columns: &'static [ColumnSpec],
    /// Columns holding a row key of a replicated table. A row whose target is missing, deleted,
    /// removed, or hidden is not materialized but stays in the group value.
    pub references: &'static [OwnedReference],
    /// Data columns with a table-wide unique index. Rows of one owner never repeat a value of
    /// them, and materialize writes them unchanged.
    pub unique: &'static [&'static str],
    /// Largest number of rows per owner row, or per parent row of a nested table.
    pub max_rows: usize,
    /// How the rows encode into the blob.
    pub codec: OwnedCodec,
    /// Nested owned tables whose rows belong to one row of this table. Their blobs are written
    /// inside each row of this table.
    pub nested: &'static [OwnedTable],
    /// Who keeps the encoded rows within one blob field.
    pub bound: OwnedBound,
}

/// Who keeps the encoded rows of an owned table within [`MAX_BLOB_FIELD_BYTES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedBound {
    /// The manifest: the largest encoding the generic codec can write for the declared columns
    /// and row limits fits one blob field.
    Static,
    /// The owning domain: its write validation keeps every group value holding the table within
    /// [`MAX_BLOB_FIELD_BYTES`], although the declared columns and row limits allow more. A
    /// value that still exceeds the bound is reported by seal as an invalid row and stays
    /// captured, so it is never published.
    Domain,
}

/// How the rows of an owned table are keyed and ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedOrder {
    /// Primary key `(parent, ordinal)`, with the zero-based position written from 0 on every
    /// replace.
    Ordinal(&'static str),
    /// A nested table: primary key `(parent, parent_ordinal, ordinal)` and a foreign key from
    /// `(parent, parent_ordinal)` to the parent owned row with `ON DELETE CASCADE`.
    Nested {
        /// Column holding the ordinal of the parent owned row.
        parent_ordinal: &'static str,
        /// Column holding the zero-based position within the parent owned row.
        ordinal: &'static str,
    },
    /// Primary key `(parent, columns...)` over data columns, with rows in ascending key order.
    /// Only a custom codec keys rows this way.
    Key(&'static [&'static str]),
}

impl OwnedOrder {
    /// Columns the table orders rows by, after the parent column.
    pub fn order_columns(self) -> Vec<&'static str> {
        match self {
            Self::Ordinal(ordinal) => vec![ordinal],
            Self::Nested {
                parent_ordinal,
                ordinal,
            } => vec![parent_ordinal, ordinal],
            Self::Key(columns) => columns.to_vec(),
        }
    }

    /// Ordinal columns the engine writes besides the data columns.
    pub fn ordinal_columns(self) -> Vec<&'static str> {
        match self {
            Self::Key(_) => Vec::new(),
            order => order.order_columns(),
        }
    }
}

/// How an owned table's rows encode into a blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedCodec {
    /// The engine's codec in [`crate::codec`]: a row count, then each row as a value of its
    /// columns followed by its nested tables.
    Generic,
    /// The table's domain adapter encodes the rows.
    Custom,
}

/// A column of an owned table holding a row key of a replicated table, with a domain foreign
/// key `ON DELETE CASCADE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnedReference {
    /// Column of the owned table.
    pub column: &'static str,
    /// Referenced table.
    pub table: TableId,
}

impl OwnedTable {
    /// Every column the engine writes: the parent column, ordinal columns, then data columns.
    pub fn written_columns(&self) -> Vec<&'static str> {
        std::iter::once(self.parent_column)
            .chain(self.order.ordinal_columns())
            .chain(self.columns.iter().map(|column| column.name))
            .collect()
    }

    /// Largest blob the generic codec writes for this table, nested tables included.
    pub fn max_encoded_bytes(&self) -> usize {
        const COUNT: usize = 2;
        const FIELD_COUNT: usize = 1;
        let row = self.columns.iter().fold(FIELD_COUNT, |bytes, column| {
            bytes.saturating_add(column.ty.max_field_bytes())
        });
        let row = self.nested.iter().fold(row, |bytes, nested| {
            bytes.saturating_add(nested.max_encoded_bytes())
        });
        COUNT.saturating_add(self.max_rows.saturating_mul(row))
    }

    /// Field index of a reference column among the data columns.
    pub fn reference_fields(&self) -> impl Iterator<Item = (usize, OwnedReference)> + 'static {
        let columns = self.columns;
        self.references.iter().filter_map(move |reference| {
            columns
                .iter()
                .position(|column| column.name == reference.column)
                .map(|field| (field, *reference))
        })
    }
}

/// How concurrent versions of a group merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeKind {
    /// Set by create only; concurrent creates keep the lowest `(clock, writer, seq)`.
    Immutable,
    /// Greatest `(clock, writer)` wins.
    Register,
    /// Greatest `(priority, clock, writer)` wins, with the priority from the domain adapter.
    Coupled,
    /// Greatest value wins.
    Max,
    /// An order key; greatest `(clock, writer)` wins.
    Position,
    /// A register holding a row key of a replicated table, possibly its own, resolved through
    /// redirects and duplicate hiding when materialized.
    Reference {
        /// Referenced table.
        table: TableId,
        /// Action of the domain foreign key when the referenced row is deleted.
        on_delete: OnDelete,
    },
}

/// The `ON DELETE` action of a reference column's domain foreign key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDelete {
    /// `ON DELETE SET NULL`.
    SetNull,
    /// `ON DELETE CASCADE`.
    Cascade,
}

impl OnDelete {
    /// The action as `pragma_foreign_key_list` reports it.
    pub const fn sql_name(self) -> &'static str {
        match self {
            Self::SetNull => "SET NULL",
            Self::Cascade => "CASCADE",
        }
    }
}

/// A column of a `Register`, `Immutable`, or `Coupled` group holding a row key of a replicated
/// table, resolved like a [`MergeKind::Reference`] group when materialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceColumn {
    /// Column of the group.
    pub column: &'static str,
    /// Referenced table.
    pub table: TableId,
    /// Action of the domain foreign key when the referenced row is deleted.
    pub on_delete: OnDelete,
}

/// One field group of a replicated table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupSpec {
    /// Group id, stable within the table.
    pub id: GroupId,
    /// Name for diagnostics and adapters.
    pub name: &'static str,
    /// Merge rule.
    pub kind: MergeKind,
    /// Value location.
    pub storage: GroupStorage,
    /// Reference columns of a `Register`, `Immutable`, or `Coupled` group; a
    /// [`MergeKind::Reference`] group declares its single reference in its kind instead.
    pub references: &'static [ReferenceColumn],
    /// Whether concurrent differing versions are shown to the user as a conflict.
    pub surfaced: bool,
    /// Whether versions that outlive a deletion are offered for recovery. On an `Immutable`
    /// group the only version is the insert, so a row created concurrently with the deletion of
    /// its cascade parent offers that parent for recovery.
    pub recoverable: bool,
    /// Whether materializing a change bumps the table's local revision column.
    pub bumps_revision: bool,
}

impl GroupSpec {
    /// Columns of the replicated row that hold this group.
    pub const fn columns(&self) -> &'static [ColumnSpec] {
        match self.storage {
            GroupStorage::Columns(columns) | GroupStorage::Owned { columns, .. } => columns,
        }
    }

    /// Owned child tables of the group, whose blobs follow the column fields.
    pub const fn owned_tables(&self) -> &'static [OwnedTable] {
        match self.storage {
            GroupStorage::Columns(_) => &[],
            GroupStorage::Owned { tables, .. } => tables,
        }
    }

    /// Number of fields in the group value.
    pub const fn field_count(&self) -> usize {
        self.columns().len() + self.owned_tables().len()
    }

    /// Every owned reference of the group: the index of the owned table's blob field, the
    /// owned table, and the reference, nested tables included.
    pub fn owned_references(
        &self,
    ) -> impl Iterator<Item = (usize, &'static OwnedTable, OwnedReference)> + 'static {
        let first = self.columns().len();
        self.owned_tables()
            .iter()
            .enumerate()
            .flat_map(move |(index, owned)| {
                std::iter::once(owned)
                    .chain(owned.nested.iter())
                    .flat_map(move |table| {
                        table
                            .references
                            .iter()
                            .map(move |reference| (first + index, table, *reference))
                    })
            })
    }

    /// The group as a single-member mask.
    pub const fn mask(&self) -> GroupMask {
        GroupMask::of(self.id)
    }

    /// Every reference of the group with the index of its field in the group value: the
    /// column of a [`MergeKind::Reference`] group, then the declared reference columns.
    pub fn reference_columns(&self) -> impl Iterator<Item = (usize, ReferenceColumn)> + 'static {
        let columns = self.columns();
        let references = self.references;
        let single = match (self.kind, columns) {
            (MergeKind::Reference { table, on_delete }, [column]) => Some((
                0,
                ReferenceColumn {
                    column: column.name,
                    table,
                    on_delete,
                },
            )),
            _ => None,
        };
        single
            .into_iter()
            .chain(references.iter().filter_map(move |reference| {
                columns
                    .iter()
                    .position(|column| column.name == reference.column)
                    .map(|field| (field, *reference))
            }))
    }
}

/// Collation of a unique constraint, as SQLite compares its text values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Collation {
    /// Byte comparison.
    Binary,
    /// ASCII letters compare case-insensitively; every other byte compares exactly.
    NoCase,
}

impl Collation {
    /// SQLite name of the collation.
    pub const fn sql_name(self) -> &'static str {
        match self {
            Self::Binary => "BINARY",
            Self::NoCase => "NOCASE",
        }
    }

    /// The form two texts share exactly when the collation compares them equal.
    pub fn fold(self, text: &str) -> String {
        match self {
            Self::Binary => text.to_string(),
            Self::NoCase => text.to_ascii_lowercase(),
        }
    }
}

/// How the engine keeps a schema constraint true for every merged state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// A unique column set: among colliding live rows that are not hidden for another reason,
    /// the lowest key is materialized, and replicas seal repair tombstones redirecting the others
    /// to it. A row that collides under several rules is hidden behind the lowest shown row it
    /// collides with.
    DuplicateRepair {
        /// Columns of the unique constraint.
        columns: &'static [&'static str],
        /// Collation of its text columns.
        collation: Collation,
    },
    /// A multi-column CHECK whose columns all belong to one group, so every winning value
    /// satisfies it.
    WithinGroup {
        /// Columns the CHECK reads.
        columns: &'static [&'static str],
        /// Group holding all of them.
        group: GroupId,
    },
}

/// A replicated table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableSpec {
    /// Table id, never reused.
    pub id: TableId,
    /// SQL table name.
    pub name: &'static str,
    /// Text primary key column holding the row key.
    pub key_column: &'static str,
    /// Field groups in id order.
    pub groups: &'static [GroupSpec],
    /// Columns kept per replica and never replicated.
    pub local_columns: &'static [LocalColumn],
    /// Columns derived from group values by the domain adapter. They never hold foreign keys:
    /// the engine writes them beside a settled value, where a foreign key would only fail at
    /// the commit of the whole apply. The schema conformance tests enforce it.
    pub derived_columns: &'static [&'static str],
    /// Columns a [`crate::hooks::MaterializeHook`] and domain code maintain from other rows,
    /// outside every group. They are never captured, have defaults and no foreign keys, and are
    /// left out of values and the summary, so materialize inserts rows with their defaults.
    pub hook_columns: &'static [&'static str],
    /// Columns holding a prefix followed by the row key. A table with a composite primary key
    /// declares none, since its key column is itself derived.
    pub key_derived_columns: &'static [KeyDerivedColumn],
    /// Local revision column bumped once per materialized row change.
    pub revision_column: Option<&'static str>,
    /// Declared resolutions for UNIQUE constraints and multi-column CHECKs.
    pub resolutions: &'static [Resolution],
    /// Whether tombstones may carry `replaced_by` redirects, always to a lower key.
    pub redirects: bool,
    /// Domain adapter for owned values, coupled priorities, derived columns, and recovery.
    pub adapter: Option<&'static str>,
    /// Domain triggers on the table and its owned child tables.
    pub triggers: &'static [DomainTrigger],
    /// Whether the row key is a function of the row's content, so a removed row can be added
    /// again under the same key.
    pub derived_key: Option<DerivedKey>,
    /// Whether rows nest through a reference into the table itself, with cycle and depth
    /// repair.
    pub tree: Option<TreeSpec>,
    /// Whether the rows of a derived-key table are directed edges that never form a cycle.
    pub acyclic: Option<AcyclicSpec>,
    /// A SQL condition over the key and group columns that a row satisfies once it is
    /// published. Rows that fail it are drafts: they are not captured, a local update cannot
    /// make a published row fail it again, a merged winner that fails it is not materialized,
    /// and no replicated operation changes a local draft or adds a row to it.
    pub publish_when: Option<&'static str>,
    /// The predicate table whose row publishes this table's rows.
    pub published_with: Option<PublishedWith>,
    /// The earlier table whose row key this table's key also names, through a key foreign key
    /// with `ON DELETE CASCADE`. The key is not a group column, so no reference declares it: the
    /// row is hidden while that parent row is, and it is recovered with it.
    pub key_parent: Option<TableId>,
    /// The device a row belongs to, when only that device may write it. Every replica refuses a
    /// change from another device, and a device never seals another device's row.
    pub owner: Option<RowOwner>,
    /// Whether domain code may hold local deletes of the table for Undo with
    /// [`crate::hold::hold_deletes`]. Rows that the held delete removes through
    /// `ON DELETE CASCADE` inherit the hold, see [`Manifest::may_hold_deletes`]. Device
    /// behavior, so it is not in the summary.
    pub holds_deletes: bool,
    /// What the table accepts as a key beyond the [`RowKey`] rules.
    ///
    /// [`RowKey`]: ganbaru_sync_contracts::RowKey
    pub key_rule: RowKeyRule,
    /// Whether rows of the table are ever deleted.
    pub deletion: Deletion,
    /// The rows a tombstone of their own does not remove while their parent lives.
    pub retained: Option<RowRetention>,
}

/// Which rows of a table outlive a tombstone of their own: while the value of `column` in the
/// immutable `group` is `value` and the row of `parent` that the same group references is shown,
/// the row materializes and reads as live. The merge state keeps the tombstone, so the rule needs
/// no repair write and replicas that cannot seal agree on it. The row goes with its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowRetention {
    /// Immutable group holding the column and the cascading reference to the parent.
    pub group: GroupId,
    /// Column of the group that selects the retained rows.
    pub column: &'static str,
    /// Value of the column on a retained row.
    pub value: &'static str,
    /// Table the rows are retained under.
    pub parent: TableId,
}

/// Whether a table's rows can be deleted through sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deletion {
    /// Rows are deleted by tombstones, repairs, and cascades.
    Ordinary,
    /// No replicated operation deletes a row: a tombstone is invalid, repair seals none, and a
    /// late one is neither listed nor restored.
    Never,
}

/// How a row key names the device that owns the row, in a form a change carries whatever its
/// action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowOwner {
    /// The row key is the owning device id.
    KeyIsDevice {
        /// Column holding the device id.
        device_column: &'static str,
    },
    /// The row key starts with `prefix`, the owner tag, and `:`. See
    /// [`device_owner_tag`](ganbaru_sync_contracts::device_owner_tag).
    KeyTag {
        /// What the row key starts with, separator included.
        prefix: &'static str,
        /// Column holding the device id.
        device_column: &'static str,
    },
}

impl RowOwner {
    /// Column holding the device id, the key column or an immutable group column.
    pub const fn device_column(self) -> &'static str {
        match self {
            Self::KeyIsDevice { device_column } | Self::KeyTag { device_column, .. } => {
                device_column
            }
        }
    }
}

/// A rule on the row keys of a table, mirroring the key CHECK of its domain table or a stricter
/// rule of the table's insert value guard. Every change carries the key, whatever its action, so
/// a key the schema refuses never spreads. Lengths count characters, as `length()` does in SQLite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKeyRule {
    /// Every valid [`RowKey`](ganbaru_sync_contracts::RowKey).
    Any,
    /// Only this key, for a singleton row.
    Fixed(&'static str),
    /// Keys of exactly this many characters.
    ExactChars(usize),
    /// Keys of at most this many characters.
    MaxChars(usize),
    /// A theme id, see [`is_theme_id`]. The insert value guard states the rule, because the
    /// table's CHECK only refuses the built-in themes.
    ThemeSlug,
}

impl RowKeyRule {
    /// Whether the rule admits `key`.
    pub fn admits(self, key: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Fixed(fixed) => key == fixed,
            Self::ExactChars(chars) => key.chars().count() == chars,
            Self::MaxChars(chars) => key.chars().count() <= chars,
            Self::ThemeSlug => is_theme_id(key),
        }
    }
}

/// A child table published together with a row of a predicate table: while that row fails
/// its predicate, writes to its children are not captured, and the seal that publishes it
/// captures every child row naming it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishedWith {
    /// Column of an `Immutable` group referencing the parent row with `ON DELETE CASCADE`.
    pub column: &'static str,
    /// The predicate table.
    pub parent: TableId,
    /// Whether a row whose column is null publishes at once, as a row with no parent. The
    /// column is then nullable in the schema, and `NOT NULL` otherwise. Since the reference is
    /// immutable and cascades, a row never moves between the two shapes: a null stays null, and
    /// deleting the parent deletes its children instead of making them publish. `ON DELETE SET
    /// NULL` is refused for that reason.
    pub null_publishes: bool,
}

/// A tree: rows nest through a `Reference` group into the table itself and are ordered by a
/// `Position` group, or by order keys outside the tree when siblings have none of their own. A
/// cycle among winning parents is broken at the row whose winning parent version has the
/// greatest `(clock, writer)`, which materializes as a root and keeps its order key. A row that
/// names itself is a cycle of length 1, so a CHECK that the parent differs from the key never
/// refuses a materialized row. A root has depth 0 and its children depth 1; every row deeper
/// than `max_depth` also materializes as a root. Replicas that can seal write those parents as
/// null.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeSpec {
    /// The `Reference` group naming the parent row in the same table.
    pub parent_group: GroupId,
    /// The `Position` group ordering siblings, if siblings are ordered within the tree.
    pub position_group: Option<GroupId>,
    /// Greatest depth a row may have, if any.
    pub max_depth: Option<u8>,
}

/// A derived-key table whose live rows are directed edges between two reference columns of
/// its identity group. Edges are taken in ascending `(clock, writer)` of their winning
/// existence version; an edge that would close a cycle is hidden and surfaced as a conflict of
/// its existence group, which the user resolves by removing an edge of the cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcyclicSpec {
    /// Column naming the row an edge starts at.
    pub from_column: &'static str,
    /// Column naming the row an edge ends at.
    pub to_column: &'static str,
}

/// Key column of a replicated table with a composite primary key: the
/// [`ganbaru_sync_contracts::derived_key`] of its primary key columns.
pub const SYNC_KEY_COLUMN: &str = "sync_key";

/// A table whose row key is derived from its content. Its rows are never tombstoned: a
/// deletion writes 0 to the existence group and an insert of a known key writes 1, so a
/// removed row can be added again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedKey {
    /// The `Register` group with no domain column whose value is `[Integer(1)]` while the row
    /// exists and `[Integer(0)]` after it is removed.
    pub exists_group: GroupId,
    /// Prefix of [`ganbaru_sync_contracts::derived_key`] when the row key is that function of
    /// [`Self::key_inputs`]. `None` only for a natural key with no key inputs, such as a
    /// preference name. A composite-key table always declares one.
    pub prefix: Option<&'static str>,
    /// Columns the key is derived from, in derivation order: the primary key columns of a
    /// composite-key table, or the inputs of a domain id formula. Every create and every re-add
    /// is checked to carry the key these values derive. `Immutable` groups of the table hold
    /// only primary key columns and these, because a re-add seals every group again and must
    /// write the same values.
    pub key_inputs: &'static [&'static str],
}

impl TableSpec {
    /// Whether the table keys rows by [`SYNC_KEY_COLUMN`] because its primary key is composite.
    pub fn has_composite_key(&self) -> bool {
        self.key_column == SYNC_KEY_COLUMN
    }

    /// Whether a constraint reads several rows of the table together: a unique column set, a
    /// tree, or acyclic edges.
    pub fn constrains_rows_together(&self) -> bool {
        self.tree.is_some()
            || self.acyclic.is_some()
            || self
                .resolutions
                .iter()
                .any(|resolution| matches!(resolution, Resolution::DuplicateRepair { .. }))
    }

    /// The existence group of a derived-key table.
    pub fn exists_group(&self) -> Option<&'static GroupSpec> {
        self.derived_key
            .and_then(|key| self.group(key.exists_group))
    }

    /// Whether `group` is the existence group of a derived-key table.
    pub fn is_exists_group(&self, group: GroupId) -> bool {
        self.derived_key
            .is_some_and(|key| key.exists_group == group)
    }

    /// Whether the winning version of `group` may keep no stored value while the domain row
    /// holds it. `Reference` groups, the existence group, a tree's parent group, the groups of
    /// a unique column set, and every group of an acyclic edge table keep theirs, because merge
    /// and repair read them across the whole table.
    pub fn may_elide(&self, group: &GroupSpec) -> bool {
        !matches!(group.kind, MergeKind::Reference { .. })
            && !self.is_exists_group(group.id)
            && self.tree.is_none_or(|tree| tree.parent_group != group.id)
            && self.acyclic.is_none()
            && !self.resolutions.iter().any(|resolution| match resolution {
                Resolution::DuplicateRepair { columns, .. } => group
                    .columns()
                    .iter()
                    .any(|column| columns.contains(&column.name)),
                Resolution::WithinGroup { .. } => false,
            })
    }

    /// Group by id.
    pub fn group(&self, id: GroupId) -> Option<&'static GroupSpec> {
        self.groups.iter().find(|group| group.id == id)
    }

    /// Every group of the table.
    pub fn full_mask(&self) -> GroupMask {
        self.groups
            .iter()
            .fold(GroupMask::EMPTY, |mask, group| mask.union(group.mask()))
    }

    /// Columns of immutable groups.
    pub fn immutable_columns(&self) -> impl Iterator<Item = &'static ColumnSpec> {
        self.groups
            .iter()
            .filter(|group| group.kind == MergeKind::Immutable)
            .flat_map(|group| group.columns())
    }

    /// Every reference of the table with its group and the index of its field in the group
    /// value.
    pub fn references(
        &self,
    ) -> impl Iterator<Item = (&'static GroupSpec, usize, ReferenceColumn)> + 'static {
        let groups = self.groups;
        groups.iter().flat_map(|group| {
            group
                .reference_columns()
                .map(move |(field, reference)| (group, field, reference))
        })
    }

    /// Local column by name.
    pub fn local_column(&self, name: &str) -> Option<&'static LocalColumn> {
        self.local_columns.iter().find(|column| column.name == name)
    }

    /// Every owned child table with its group, each followed by its nested tables.
    pub fn owned_children(
        &self,
    ) -> impl Iterator<Item = (&'static GroupSpec, &'static OwnedTable)> + 'static {
        let groups = self.groups;
        groups.iter().flat_map(|group| {
            group.owned_tables().iter().flat_map(move |owned| {
                std::iter::once(owned)
                    .chain(owned.nested.iter())
                    .map(move |table| (group, table))
            })
        })
    }

    /// Every owned reference of the table with its group, the index of the owned table's blob
    /// field in the group value, the owned table holding it, and the reference.
    pub fn owned_references(
        &self,
    ) -> impl Iterator<
        Item = (
            &'static GroupSpec,
            usize,
            &'static OwnedTable,
            OwnedReference,
        ),
    > + 'static {
        let groups = self.groups;
        groups.iter().flat_map(|group| {
            group
                .owned_references()
                .map(move |(field, owned, reference)| (group, field, owned, reference))
        })
    }
}

/// A manifest version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Manifest {
    /// Manifest version carried in operation headers.
    pub version: u16,
    /// Domains in dependency order.
    pub domains: &'static [&'static DomainManifest],
    /// Replicated tables of every domain in dependency order (see [`order`]).
    pub tables: &'static [TableSpec],
    /// Derived tables, including virtual tables whose shadow tables follow them.
    pub derived: &'static [&'static str],
    /// `Local` tables: [`SYNC_LOCAL_TABLES`] and the domains' `Local` tables.
    pub local: &'static [&'static str],
    /// Engine tables: [`SYNC_TABLES`] and the domains' engine-side tables.
    pub engine: &'static [&'static str],
}

impl Manifest {
    /// The manifest of a domain list. The flattened lists are allocated once and live for the
    /// rest of the process, so this belongs in a `LazyLock` static.
    pub fn from_domains(version: u16, domains: &'static [&'static DomainManifest]) -> Self {
        fn leak<T>(items: Vec<T>) -> &'static [T] {
            Box::leak(items.into_boxed_slice())
        }
        let tables = domains
            .iter()
            .flat_map(|domain| domain.tables.iter().copied())
            .collect();
        let derived = domains
            .iter()
            .flat_map(|domain| domain.derived.iter().copied())
            .collect();
        let local = SYNC_LOCAL_TABLES
            .iter()
            .copied()
            .chain(
                domains
                    .iter()
                    .flat_map(|domain| domain.local.iter().map(|table| table.name)),
            )
            .collect();
        let engine = SYNC_TABLES
            .iter()
            .copied()
            .chain(
                domains
                    .iter()
                    .flat_map(|domain| domain.engine.iter().map(|table| table.name)),
            )
            .collect();
        Self {
            version,
            domains,
            tables: leak(tables),
            derived: leak(derived),
            local: leak(local),
            engine: leak(engine),
        }
    }

    /// The seed statements of the domains' `Local` tables.
    pub fn local_seeds(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.domains
            .iter()
            .flat_map(|domain| domain.local.iter())
            .filter_map(|table| table.seed)
    }

    /// Domain of a replicated table or owned child table.
    pub fn domain_of(&self, table: &str) -> Option<&'static DomainManifest> {
        self.domains.iter().copied().find(|domain| {
            domain.tables.iter().any(|spec| {
                spec.name == table || spec.owned_children().any(|(_, owned)| owned.table == table)
            })
        })
    }

    /// Registered asset class by name with the domain that declares it.
    pub fn asset_class(
        &self,
        name: &str,
    ) -> Option<(&'static DomainManifest, &'static AssetClass)> {
        self.domains.iter().copied().find_map(|domain| {
            domain
                .assets
                .iter()
                .find(|class| class.name == name)
                .map(|class| (domain, class))
        })
    }

    /// Every registered asset class in domain order.
    pub fn asset_classes(&self) -> impl Iterator<Item = &'static AssetClass> {
        self.domains.iter().flat_map(|domain| domain.assets)
    }

    /// Replicated table by id.
    pub fn table(&self, id: TableId) -> Option<&'static TableSpec> {
        self.tables.iter().find(|table| table.id == id)
    }

    /// The lowest table of the component of every table, in the graph of reference columns of
    /// either delete action, owned references, and key parents, with direction ignored. Nothing
    /// that follows a reference leaves a component.
    pub(crate) fn reference_components(&self) -> BTreeMap<TableId, TableId> {
        fn root(parents: &BTreeMap<TableId, TableId>, table: TableId) -> TableId {
            let mut current = table;
            while let Some(parent) = parents.get(&current).filter(|parent| **parent != current) {
                current = *parent;
            }
            current
        }
        let mut parents: BTreeMap<TableId, TableId> = self
            .tables
            .iter()
            .map(|table| (table.id, table.id))
            .collect();
        for table in self.tables {
            let referenced = table
                .references()
                .map(|(_, _, reference)| reference.table)
                .chain(table.owned_references().map(|(_, _, _, owned)| owned.table))
                .chain(table.key_parent);
            for target in referenced {
                let (left, right) = (root(&parents, table.id), root(&parents, target));
                parents.insert(left.max(right), left.min(right));
            }
        }
        self.tables
            .iter()
            .map(|table| (table.id, root(&parents, table.id)))
            .collect()
    }

    /// Whether a local delete of `table` can be held: the table holds deletes, or it
    /// references a table that can with `ON DELETE CASCADE`, so its rows inherit the hold of
    /// the delete that cascades to them.
    pub fn may_hold_deletes(&self, table: TableId) -> bool {
        let mut holding: Vec<TableId> = self
            .tables
            .iter()
            .filter(|spec| spec.holds_deletes)
            .map(|spec| spec.id)
            .collect();
        loop {
            let inherited: Vec<TableId> = self
                .tables
                .iter()
                .filter(|spec| !holding.contains(&spec.id))
                .filter(|spec| {
                    spec.references().any(|(_, _, reference)| {
                        reference.on_delete == OnDelete::Cascade
                            && holding.contains(&reference.table)
                    })
                })
                .map(|spec| spec.id)
                .collect();
            if inherited.is_empty() {
                return holding.contains(&table);
            }
            holding.extend(inherited);
        }
    }

    /// Tables whose rows can root a recovery offer, in manifest order: tables with a
    /// recoverable group and, transitively, the tables they reference with `ON DELETE CASCADE`
    /// or name as their key parent, because a cascaded deletion groups its rows under the deleted parent. Derived-key tables
    /// are left out: a removed derived-key row stays live in merge state, so it never carries
    /// the recovery flag, and adding the key again is how it comes back.
    pub fn recovery_root_tables(&self) -> Vec<&'static TableSpec> {
        let mut roots: Vec<TableId> = self
            .tables
            .iter()
            .filter(|spec| spec.groups.iter().any(|group| group.recoverable))
            .map(|spec| spec.id)
            .collect();
        loop {
            let parents: Vec<TableId> = self
                .tables
                .iter()
                .filter(|spec| roots.contains(&spec.id))
                .flat_map(|spec| {
                    spec.references()
                        .filter(|(_, _, reference)| reference.on_delete == OnDelete::Cascade)
                        .map(|(_, _, reference)| reference.table)
                        .chain(spec.key_parent)
                })
                .filter(|table| !roots.contains(table))
                .collect();
            if parents.is_empty() {
                break;
            }
            roots.extend(parents);
            roots.sort_unstable();
            roots.dedup();
        }
        self.tables
            .iter()
            .filter(|spec| roots.contains(&spec.id) && spec.derived_key.is_none())
            .collect()
    }

    /// Replicated table by SQL name.
    pub fn table_named(&self, name: &str) -> Option<&'static TableSpec> {
        self.tables.iter().find(|table| table.name == name)
    }

    /// Classification of a schema table; tables the manifest does not name are unconverted.
    pub fn classify(&self, name: &str) -> Classification {
        if self.table_named(name).is_some() {
            Classification::Replicated
        } else if self
            .tables
            .iter()
            .flat_map(TableSpec::owned_children)
            .any(|(_, owned)| owned.table == name)
        {
            Classification::OwnedChildren
        } else if self.derived.contains(&name) {
            Classification::Derived
        } else if self.local.contains(&name) {
            Classification::Local
        } else if self.engine.contains(&name) {
            Classification::Engine
        } else {
            Classification::Unconverted
        }
    }
}

/// Group id for static manifest tables.
pub(crate) const fn group_id(id: u8) -> GroupId {
    match GroupId::new(id) {
        Some(group) => group,
        None => panic!("group id out of range"),
    }
}
