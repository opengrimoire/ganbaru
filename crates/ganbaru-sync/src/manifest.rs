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

use ganbaru_sync_contracts::{GroupId, GroupMask, TableId};

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
    "sync_tombstones",
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
    /// Replicated tables, parents before the tables that reference them.
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
    pub local_columns: &'static [LocalColumn],
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
    /// Domain triggers on the table and its owned child tables.
    pub triggers: &'static [DomainTrigger],
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

    /// Local column by name.
    pub fn local_column(&self, name: &str) -> Option<&'static LocalColumn> {
        self.local_columns.iter().find(|column| column.name == name)
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
    /// Domains in dependency order.
    pub domains: &'static [&'static DomainManifest],
    /// Replicated tables of every domain, parents before the tables that reference them.
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
    /// domain's range; domain names are unique and ranges disjoint; and every `Local` column
    /// declares a SQL literal. Adapter registration is checked by the engine.
    pub fn validate(&self) -> Result<(), String> {
        let mut problems = String::new();
        for (index, domain) in self.domains.iter().enumerate() {
            let range = domain.table_ids;
            if range.first == 0 || range.first > range.last {
                let _ = writeln!(problems, "{}: invalid table id range", domain.name);
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
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.trim_end().to_string())
        }
    }

    /// Domain of a replicated table or owned child table.
    pub fn domain_of(&self, table: &str) -> Option<&'static DomainManifest> {
        self.domains.iter().copied().find(|domain| {
            domain.tables.iter().any(|spec| {
                spec.name == table || spec.owned_children().any(|(_, owned)| owned.table == table)
            })
        })
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
