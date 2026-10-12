//! The replication manifest: table classifications, field groups, and merge kinds.
//!
//! A manifest version fixes table ids, group ids, column membership, and value rules. Ids are
//! never reused; tightening a rule bumps the version and keeps the old rule for old operations.

use ganbaru_sync_contracts::{GroupId, GroupMask, TableId};

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
    /// Engine state in `sync_*` tables.
    Engine,
    /// Rows that keep the owner-only whole-vault rules.
    Unconverted,
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
            Self::Text { max_chars } => (0, max_chars),
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
    /// Rows of an owned child table, encoded by the table's domain adapter as one blob.
    Owned(OwnedTable),
}

/// An owned child table whose rows, in order, form one group value of their parent row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnedTable {
    /// Child table.
    pub table: &'static str,
    /// Child column holding the parent row key.
    pub parent_column: &'static str,
    /// Child column holding the zero-based position within the parent, rewritten on replace.
    pub order_column: &'static str,
    /// Child columns passed to the domain adapter, in field order.
    pub columns: &'static [&'static str],
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
    /// A register holding a row key of another table, resolved through redirects and duplicate
    /// hiding when materialized.
    Reference {
        /// Referenced table.
        table: TableId,
    },
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
    /// Whether concurrent differing versions are shown to the user as a conflict.
    pub surfaced: bool,
    /// Whether versions that outlive a deletion are offered for recovery.
    pub recoverable: bool,
    /// Whether materializing a change bumps the table's local revision column.
    pub bumps_revision: bool,
}

impl GroupSpec {
    /// Columns of the replicated row that hold this group; empty for owned storage.
    pub const fn columns(&self) -> &'static [ColumnSpec] {
        match self.storage {
            GroupStorage::Columns(columns) => columns,
            GroupStorage::Owned(_) => &[],
        }
    }

    /// The group as a single-member mask.
    pub const fn mask(&self) -> GroupMask {
        GroupMask::of(self.id)
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
    pub local_columns: &'static [&'static str],
    /// Columns derived from group values by the domain adapter.
    pub derived_columns: &'static [&'static str],
    /// Local revision column bumped once per materialized row change.
    pub revision_column: Option<&'static str>,
    /// Declared resolutions for UNIQUE constraints and multi-column CHECKs.
    pub resolutions: &'static [Resolution],
    /// Whether tombstones may carry `replaced_by` redirects, always to a lower key.
    pub redirects: bool,
    /// Domain adapter for owned values, coupled priorities, derived columns, and recovery.
    pub adapter: Option<&'static str>,
}

impl TableSpec {
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

    /// Owned child storage with its group.
    pub fn owned_children(&self) -> impl Iterator<Item = (&'static GroupSpec, OwnedTable)> {
        self.groups.iter().filter_map(|group| match group.storage {
            GroupStorage::Owned(owned) => Some((group, owned)),
            GroupStorage::Columns(_) => None,
        })
    }
}

/// A manifest version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Manifest {
    /// Manifest version carried in operation headers.
    pub version: u16,
    /// Replicated tables, parents before the tables that reference them.
    pub tables: &'static [TableSpec],
    /// Derived tables, including virtual tables whose shadow tables follow them.
    pub derived: &'static [&'static str],
    /// Engine tables.
    pub engine: &'static [&'static str],
}

impl Manifest {
    /// Replicated table by id.
    pub fn table(&self, id: TableId) -> Option<&'static TableSpec> {
        self.tables.iter().find(|table| table.id == id)
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
