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

/// A column of a `Register` or `Immutable` group holding a row key of a replicated table,
/// resolved like a [`MergeKind::Reference`] group when materialized.
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
    /// Reference columns of a `Register` or `Immutable` group; a [`MergeKind::Reference`]
    /// group declares its single reference in its kind instead.
    pub references: &'static [ReferenceColumn],
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
    /// domain's range; domain names are unique and ranges disjoint; every `Local` column
    /// declares a SQL literal; references and derived keys are well formed; and tables follow the dependency
    /// order of their references. Adapter registration is checked by the engine.
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
            for group in table.groups {
                for problem in reference_problems(group) {
                    let _ = writeln!(problems, "{}.{}: {problem}", table.name, group.name);
                }
            }
            for problem in derived_key_problems(table)
                .into_iter()
                .chain(tree_problems(table))
                .chain(acyclic_problems(table))
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
        MergeKind::Register | MergeKind::Immutable => {}
        MergeKind::Coupled | MergeKind::Max | MergeKind::Position => {
            if !group.references.is_empty() {
                problems.push(
                    "only register and immutable groups declare reference columns".to_string(),
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
