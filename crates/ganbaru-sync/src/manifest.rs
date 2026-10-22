//! The replication manifest: table classifications, field groups, and merge kinds.
//!
//! A manifest version fixes table ids, group ids, column membership, and value rules, and ids are
//! never reused. Operations of every version up to the current one validate against the current
//! manifest, so a new version may only add tables and groups or loosen rules; tightening a rule
//! needs per-version validation first. A conformance test records each version's canonical
//! [`Manifest::summary`], so a change to the summary without a version bump fails.
//!
//! Each domain declares its tables in one [`DomainManifest`]; a manifest flattens a list of
//! domains in dependency order.

use std::fmt::Write as _;

use ganbaru_sync_contracts::bounds::{
    MAX_BLOB_FIELD_BYTES, MAX_FIELDS_PER_VALUE, MAX_ORDER_KEY_BYTES, MAX_ROW_KEY_BYTES,
    MAX_TEXT_FIELD_BYTES,
};
use ganbaru_sync_contracts::{GroupId, GroupMask, TableId};

use crate::assets::AssetClass;

pub mod order;
pub mod summary;
pub mod vault;

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
    /// Device-bound rows that never replicate, are writable on replicas, and are emptied in
    /// snapshot copies.
    Local,
    /// Rows that keep the owner-only whole-vault rules.
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
    /// Domain triggers on the table.
    pub triggers: &'static [DomainTrigger],
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
    /// Whether a linked replica may write the domain's replicated tables. Otherwise guards keep
    /// them owner-only on replicas while the engine still applies remote changes to them.
    pub replica_writable: bool,
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
}

impl ColumnType {
    /// Whether the column holds text, so comparisons must ignore declared collations.
    pub const fn is_text(self) -> bool {
        !matches!(self, Self::Integer { .. } | Self::Flag)
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
            Self::Integer { .. } | Self::Flag => return TAG + INTEGER,
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
            Self::OptionalRowKey => MAX_ROW_KEY_BYTES,
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
    /// Whether versions that outlive a deletion are offered for recovery.
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
    /// A unique column set: among colliding live rows only the lowest key is materialized, and
    /// replicas seal repair tombstones redirecting the others to it.
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
    /// Columns derived from group values by the domain adapter.
    pub derived_columns: &'static [&'static str],
    /// Columns a [`crate::hooks::MaterializeHook`] and domain code maintain from other rows,
    /// outside every group. They are never captured, have defaults and no foreign keys, and are
    /// left out of values and the summary, so materialize inserts rows with their defaults.
    pub hook_columns: &'static [&'static str],
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
    /// make a published row fail it again, and a merged winner that fails it is not
    /// materialized.
    pub publish_when: Option<&'static str>,
    /// The predicate table whose row publishes this table's rows.
    pub published_with: Option<PublishedWith>,
    /// Whether domain code may hold local deletes of the table for Undo with
    /// [`crate::hold::hold_deletes`]. Rows that the held delete removes through
    /// `ON DELETE CASCADE` inherit the hold, see [`Manifest::may_hold_deletes`]. Device
    /// behavior, so it is not in the summary.
    pub holds_deletes: bool,
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
}

/// A tree: rows nest through a `Reference` group into the table itself and are ordered by a
/// `Position` group. A cycle among winning parents is broken at the row whose winning parent
/// version has the greatest `(clock, writer)`, which materializes as a root and keeps its order
/// key. A root has depth 0 and its children depth 1; every row deeper than `max_depth` also
/// materializes as a root. Replicas that can seal write those parents as null.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeSpec {
    /// The `Reference` group naming the parent row in the same table.
    pub parent_group: GroupId,
    /// The `Position` group ordering siblings.
    pub position_group: GroupId,
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
    /// Columns besides the primary key that the key is derived from, such as the inputs of a
    /// domain id formula. `Immutable` groups of the table hold only primary key columns and
    /// these, because a re-add seals every group again and must write the same values.
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
    /// `Local` tables.
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
        let local = domains
            .iter()
            .flat_map(|domain| domain.local.iter().map(|table| table.name))
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

    /// Checks the facts the engine relies on: table ids are nonzero, unique, and inside their
    /// domain's range; domain names are unique and ranges disjoint; asset class names are unique
    /// relative paths within the asset byte limit; every `Local` column
    /// declares a SQL literal; references, owned tables, derived keys, trees, edges, and
    /// publication are well formed; and tables follow the dependency order of their references.
    /// Adapter and hook registration is checked by the engine.
    pub fn validate(&self) -> Result<(), String> {
        let mut problems = String::new();
        for (index, domain) in self.domains.iter().enumerate() {
            let range = domain.table_ids;
            if range.first == 0 || range.first > range.last {
                let _ = writeln!(problems, "{}: invalid table id range", domain.name);
            }
            for (position, class) in domain.assets.iter().enumerate() {
                if let Some(problem) = class.problem() {
                    let _ = writeln!(
                        problems,
                        "{}: asset class {}: {problem}",
                        domain.name, class.name
                    );
                }
                let repeated = domain.assets[position + 1..]
                    .iter()
                    .chain(
                        self.domains[index + 1..]
                            .iter()
                            .flat_map(|other| other.assets),
                    )
                    .any(|other| other.name == class.name);
                if repeated {
                    let _ = writeln!(problems, "{}: asset class named twice", class.name);
                }
            }
            for other in &self.domains[index + 1..] {
                if other.name == domain.name {
                    let _ = writeln!(problems, "{}: domain named twice", domain.name);
                }
                if other.table_ids.overlaps(range) {
                    let _ = writeln!(
                        problems,
                        "{} and {}: table id ranges overlap",
                        domain.name, other.name
                    );
                }
            }
            for table in domain.tables {
                if !range.contains(table.id) {
                    let _ = writeln!(
                        problems,
                        "{}: table id {} is outside the {} range",
                        table.name, table.id.0, domain.name
                    );
                }
                for column in table.local_columns {
                    if !is_sql_literal(column.materialize) {
                        let _ = writeln!(
                            problems,
                            "{}.{}: materialization value is not a SQL literal",
                            table.name, column.name
                        );
                    }
                }
            }
        }
        for (index, table) in self.tables.iter().enumerate() {
            if self.tables[index + 1..]
                .iter()
                .any(|other| other.id == table.id)
            {
                let _ = writeln!(problems, "{}: duplicate table id", table.name);
            }
            for group in table.groups {
                for problem in reference_problems(group)
                    .into_iter()
                    .chain(owned_group_problems(group))
                {
                    let _ = writeln!(problems, "{}.{}: {problem}", table.name, group.name);
                }
                let custom = group
                    .owned_tables()
                    .iter()
                    .any(|owned| owned.codec == OwnedCodec::Custom);
                if (custom || group.kind == MergeKind::Coupled) && table.adapter.is_none() {
                    let _ = writeln!(
                        problems,
                        "{}.{}: a custom owned codec or coupled group needs an adapter",
                        table.name, group.name
                    );
                }
            }
            for problem in derived_key_problems(table)
                .into_iter()
                .chain(tree_problems(table))
                .chain(acyclic_problems(table))
                .chain(self.acyclic_cascade_problems(table))
                .chain(self.publication_problems(table))
            {
                let _ = writeln!(problems, "{}: {problem}", table.name);
            }
        }
        for problem in self.order_problems(&self.reference_dependencies()) {
            let _ = writeln!(problems, "{problem}");
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.trim_end().to_string())
        }
    }

    /// Why a table's publication declarations are malformed. A predicate is one non-blank
    /// condition. A `published_with` column is an immutable cascading reference into a
    /// predicate table, and only such a child references a predicate table, because a reference
    /// from anywhere else could name a draft. Derived-key tables neither publish by predicate
    /// nor hold deletes, since their deletes are existence writes.
    fn publication_problems(&self, table: &TableSpec) -> Vec<String> {
        let mut problems = Vec::new();
        if let Some(predicate) = table.publish_when
            && (predicate.trim().is_empty() || predicate.contains(';'))
        {
            problems.push("the publication predicate is not one condition".to_string());
        }
        if table.derived_key.is_some()
            && (table.publish_when.is_some()
                || table.published_with.is_some()
                || table.holds_deletes)
        {
            problems.push(
                "a derived-key table has no publication predicate, parent publication, or \
                 delete holds"
                    .to_string(),
            );
        }
        if let Some(with) = table.published_with {
            if table.publish_when.is_some() {
                problems.push("a table publishes by its predicate or with its parent".to_string());
            }
            if self.table(with.parent).is_none_or(|parent| {
                parent.publish_when.is_none() || parent.published_with.is_some()
            }) {
                problems.push("the publishing parent is not a predicate table".to_string());
            }
            let declared = table.references().any(|(group, _, reference)| {
                group.kind == MergeKind::Immutable
                    && reference.column == with.column
                    && reference.table == with.parent
                    && reference.on_delete == OnDelete::Cascade
            });
            if !declared {
                problems.push(format!(
                    "{} is not an immutable cascading reference into the publishing parent",
                    with.column
                ));
            }
        }
        for (_, _, reference) in table.references() {
            let target_publishes = self
                .table(reference.table)
                .is_some_and(|target| target.publish_when.is_some());
            let publishes_with = table.published_with.is_some_and(|with| {
                with.parent == reference.table && with.column == reference.column
            });
            if target_publishes && !publishes_with {
                problems.push(format!(
                    "{} references a predicate table without publishing with it",
                    reference.column
                ));
            }
        }
        problems
    }

    /// Why an acyclic edge table cannot keep its edges: an endpoint table reaches the edge table
    /// through cascading references, so deleting an edge could cascade into the rows it joins
    /// while cycle detection reads them.
    fn acyclic_cascade_problems(&self, table: &TableSpec) -> Vec<String> {
        let Some(acyclic) = table.acyclic else {
            return Vec::new();
        };
        let endpoints = table
            .references()
            .filter(|(_, _, reference)| {
                reference.column == acyclic.from_column || reference.column == acyclic.to_column
            })
            .map(|(_, _, reference)| reference.table)
            .collect::<Vec<_>>();
        let mut problems = Vec::new();
        for endpoint in endpoints {
            let mut seen = vec![endpoint];
            let mut pending = vec![endpoint];
            while let Some(current) = pending.pop() {
                let Some(spec) = self.table(current) else {
                    continue;
                };
                for (_, _, reference) in spec.references() {
                    if reference.on_delete == OnDelete::Cascade && !seen.contains(&reference.table)
                    {
                        seen.push(reference.table);
                        pending.push(reference.table);
                    }
                }
            }
            if seen.contains(&table.id) {
                let name = self.table(endpoint).map_or("an endpoint", |spec| spec.name);
                problems.push(format!(
                    "endpoint table {name} reaches the edge table through cascading references"
                ));
            }
        }
        problems.dedup();
        problems
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

    /// Names of the domains a linked replica may write, in domain order.
    pub fn replica_writable_domains(&self) -> impl Iterator<Item = &'static str> {
        self.domains
            .iter()
            .filter(|domain| domain.replica_writable)
            .map(|domain| domain.name)
    }

    /// Replicated table by id.
    pub fn table(&self, id: TableId) -> Option<&'static TableSpec> {
        self.tables.iter().find(|table| table.id == id)
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

/// Why a group's references are malformed. Every reference is a single optional row key
/// column, declared once. An immutable reference cascades, because clearing it would change an
/// immutable column outside apply, which the immutability guard refuses.
fn reference_problems(group: &GroupSpec) -> Vec<String> {
    let mut problems = Vec::new();
    let columns = group.columns();
    match group.kind {
        MergeKind::Reference { .. } => {
            if !matches!(columns, [column] if column.ty == ColumnType::OptionalRowKey) {
                problems.push("a reference group holds one optional row key column".to_string());
            }
            if !group.references.is_empty() {
                problems.push("a reference group declares no reference columns".to_string());
            }
        }
        MergeKind::Register | MergeKind::Immutable | MergeKind::Coupled => {}
        MergeKind::Max | MergeKind::Position => {
            if !group.references.is_empty() {
                problems.push(
                    "only register, immutable, and coupled groups declare reference columns"
                        .to_string(),
                );
            }
        }
    }
    for (index, reference) in group.references.iter().enumerate() {
        match columns
            .iter()
            .find(|column| column.name == reference.column)
        {
            None => problems.push(format!("{} is not a column of the group", reference.column)),
            Some(column) if column.ty != ColumnType::OptionalRowKey => problems.push(format!(
                "{} holds a reference but is not an optional row key",
                reference.column
            )),
            Some(_) => {}
        }
        if group.references[..index]
            .iter()
            .any(|other| other.column == reference.column)
        {
            problems.push(format!("{} is declared twice", reference.column));
        }
        if group.kind == MergeKind::Immutable && reference.on_delete != OnDelete::Cascade {
            problems.push(format!(
                "{} is an immutable reference that does not cascade",
                reference.column
            ));
        }
    }
    problems
}

/// Why a group's owned tables are malformed. Owned tables belong to immutable, register, and
/// coupled groups, and the value keeps within the field and blob bounds.
fn owned_group_problems(group: &GroupSpec) -> Vec<String> {
    let GroupStorage::Owned { tables, .. } = group.storage else {
        return Vec::new();
    };
    let mut problems = Vec::new();
    if !matches!(
        group.kind,
        MergeKind::Immutable | MergeKind::Register | MergeKind::Coupled
    ) {
        problems.push("only immutable, register, and coupled groups own tables".to_string());
    }
    if tables.is_empty() {
        problems.push("an owned group names no owned table".to_string());
    }
    if group.field_count() > MAX_FIELDS_PER_VALUE {
        problems.push("the group value has too many fields".to_string());
    }
    for owned in tables {
        for problem in owned_table_problems(owned, false) {
            problems.push(format!("{}: {problem}", owned.table));
        }
    }
    problems
}

/// Why an owned table is malformed. Nesting is one level deep under the generic codec, a
/// custom codec has no references or nested tables, columns are distinct, and the rows the
/// generic codec writes fit one blob field.
fn owned_table_problems(owned: &OwnedTable, nested: bool) -> Vec<String> {
    let mut problems = Vec::new();
    if owned.columns.is_empty() {
        problems.push("no data columns".to_string());
    }
    if owned.max_rows == 0 || owned.max_rows > usize::from(u16::MAX) {
        problems.push("max_rows is outside 1..=65535".to_string());
    }
    match (owned.order, nested, owned.codec) {
        (OwnedOrder::Ordinal(_), false, _) | (OwnedOrder::Nested { .. }, true, _) => {}
        (OwnedOrder::Key(columns), false, OwnedCodec::Custom) => {
            if columns.is_empty() {
                problems.push("a key order names no column".to_string());
            }
            for column in columns {
                if !owned.columns.iter().any(|spec| spec.name == *column) {
                    problems.push(format!("key column {column} is not a data column"));
                }
            }
        }
        _ => problems.push("the order does not fit the table's nesting and codec".to_string()),
    }
    let written = owned.written_columns();
    if written
        .iter()
        .enumerate()
        .any(|(index, column)| written[..index].contains(column))
    {
        problems.push("a column is named twice".to_string());
    }
    for column in owned.unique {
        if !owned.columns.iter().any(|spec| spec.name == *column) {
            problems.push(format!("unique column {column} is not a data column"));
        }
    }
    for (index, reference) in owned.references.iter().enumerate() {
        match owned
            .columns
            .iter()
            .find(|column| column.name == reference.column)
        {
            Some(column) if column.ty == ColumnType::OptionalRowKey => {}
            _ => problems.push(format!(
                "{} is not an optional row key data column",
                reference.column
            )),
        }
        if owned.references[..index]
            .iter()
            .any(|other| other.column == reference.column)
        {
            problems.push(format!("{} is declared twice", reference.column));
        }
    }
    match owned.codec {
        OwnedCodec::Custom => {
            if !owned.references.is_empty() || !owned.nested.is_empty() {
                problems.push("a custom codec has no references or nested tables".to_string());
            }
        }
        OwnedCodec::Generic => {
            if owned.columns.len() > MAX_FIELDS_PER_VALUE {
                problems.push("a row has too many fields for one value".to_string());
            }
            if !nested && owned.max_encoded_bytes() > MAX_BLOB_FIELD_BYTES {
                problems.push("the largest encoding exceeds the blob field bound".to_string());
            }
        }
    }
    if nested && !owned.nested.is_empty() {
        problems.push("nested tables do not nest further".to_string());
    }
    if nested && owned.codec != OwnedCodec::Generic {
        problems.push("nested tables use the generic codec".to_string());
    }
    for child in owned.nested {
        for problem in owned_table_problems(child, true) {
            problems.push(format!("{}: {problem}", child.table));
        }
    }
    problems
}

/// Why a table's derived key or existence group is malformed. Only the existence group has no
/// column; it is a plain register the user never sees in a conflict or recovery entry. A
/// derived-key table never tombstones, so it neither redirects nor repairs duplicates.
fn derived_key_problems(table: &TableSpec) -> Vec<String> {
    let mut problems = Vec::new();
    for group in table.groups {
        let columnless = matches!(group.storage, GroupStorage::Columns([]));
        if columnless && !table.is_exists_group(group.id) {
            problems.push(format!(
                "{} has no columns and is not the existence group",
                group.name
            ));
        }
    }
    let Some(derived) = table.derived_key else {
        return problems;
    };
    match table.group(derived.exists_group) {
        None => problems.push("the existence group is not a group of the table".to_string()),
        Some(group) => {
            if group.kind != MergeKind::Register
                || !matches!(group.storage, GroupStorage::Columns([]))
                || !group.references.is_empty()
                || group.surfaced
                || group.recoverable
            {
                problems.push(format!(
                    "existence group {} is not a hidden register without columns",
                    group.name
                ));
            }
        }
    }
    if table.redirects {
        problems.push("a derived-key table never redirects".to_string());
    }
    if table
        .resolutions
        .iter()
        .any(|resolution| matches!(resolution, Resolution::DuplicateRepair { .. }))
    {
        problems.push("a derived-key table has no duplicate repair".to_string());
    }
    for input in derived.key_inputs {
        if !table
            .immutable_columns()
            .any(|column| column.name == *input)
        {
            problems.push(format!("key input {input} is not an immutable column"));
        }
    }
    problems
}

/// Why a table's tree declaration is malformed: the parent group references the table itself,
/// the position group is a `Position` group, and the table has a single row key it never
/// derives from content.
fn tree_problems(table: &TableSpec) -> Vec<String> {
    let mut problems = Vec::new();
    let Some(tree) = table.tree else {
        return problems;
    };
    match table.group(tree.parent_group).map(|group| group.kind) {
        Some(MergeKind::Reference { table: target, .. }) if target == table.id => {}
        _ => problems.push("the tree parent is not a reference group into the table".to_string()),
    }
    if table.group(tree.position_group).map(|group| group.kind) != Some(MergeKind::Position) {
        problems.push("the tree position is not a position group".to_string());
    }
    if table.derived_key.is_some() {
        problems.push("a tree table has no derived key".to_string());
    }
    problems
}

/// Why a table's acyclic edge declaration is malformed: the table has a derived key and both
/// ends are distinct immutable references into the same table.
fn acyclic_problems(table: &TableSpec) -> Vec<String> {
    let mut problems = Vec::new();
    let Some(acyclic) = table.acyclic else {
        return problems;
    };
    if table.derived_key.is_none() {
        problems.push("an acyclic edge table has no derived key".to_string());
    }
    let end = |column: &str| {
        table
            .references()
            .find(|(group, _, reference)| {
                group.kind == MergeKind::Immutable && reference.column == column
            })
            .map(|(_, _, reference)| reference.table)
    };
    match (end(acyclic.from_column), end(acyclic.to_column)) {
        (Some(from), Some(to)) if from == to && acyclic.from_column != acyclic.to_column => {}
        _ => problems
            .push("acyclic edge ends are not two immutable references into one table".to_string()),
    }
    problems
}

/// Group id for static manifest tables.
pub(crate) const fn group_id(id: u8) -> GroupId {
    match GroupId::new(id) {
        Some(group) => group,
        None => panic!("group id out of range"),
    }
}
