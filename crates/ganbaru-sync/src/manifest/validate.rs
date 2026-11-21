//! Manifest validation: the structural facts the engine relies on, checked per manifest, per
//! table, and per group, from ids and ranges to references, owned tables, derived keys, trees,
//! edges, publication, retention, and delete reach.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use ganbaru_sync_contracts::bounds::{MAX_BLOB_FIELD_BYTES, MAX_FIELDS_PER_VALUE};
use ganbaru_sync_contracts::{TableId, derived_key};

use crate::assets::local_asset_problems;

use super::{
    ColumnType, Deletion, GroupSpec, GroupStorage, Manifest, MergeKind, OnDelete, OwnedBound,
    OwnedCodec, OwnedOrder, OwnedTable, Resolution, TableSpec, is_sql_literal,
};

impl Manifest {
    /// Checks the facts the engine relies on: table ids are nonzero, unique, and inside their
    /// domain's range; domain names are unique and ranges disjoint; asset class names are unique
    /// relative paths within the asset byte limit, and `Local` asset columns name them; every
    /// `Local` column
    /// declares a SQL literal; key-derived columns are named apart from the key of a
    /// single-column key; references, owned tables, derived keys, trees, edges, and
    /// publication are well formed; retention rules can hold; no delete can reach a
    /// never-deleted table; and tables follow
    /// the dependency order of their references. Adapter and hook registration is checked by the
    /// engine.
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
            for problem in local_asset_problems(domain) {
                let _ = writeln!(problems, "{}: {problem}", domain.name);
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
                for column in table.key_derived_columns {
                    if column.name.is_empty() || column.name == table.key_column {
                        let _ = writeln!(
                            problems,
                            "{}: a key-derived column needs its own name",
                            table.name
                        );
                    }
                    if table.has_composite_key() {
                        let _ = writeln!(
                            problems,
                            "{}.{}: a composite-key table has no key-derived columns",
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
                .chain(self.key_parent_problems(table))
                .chain(self.deletion_problems(table))
                .chain(self.retention_problems(table))
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

    /// Why a key parent cannot hide its children by key. Both tables hold one row per plain
    /// key, so the child's key names the parent row; the parent never redirects, since the key
    /// could not follow; and the parent holds no deletes, since a held delete would not reach
    /// the child.
    fn key_parent_problems(&self, table: &TableSpec) -> Vec<String> {
        let Some(parent) = table.key_parent else {
            return Vec::new();
        };
        let mut problems = Vec::new();
        if table.has_composite_key() || table.derived_key.is_some() {
            problems.push("a key-parent child is keyed by a plain row key".to_string());
        }
        match self.table(parent) {
            Some(spec) if spec.has_composite_key() || spec.derived_key.is_some() => {
                problems.push("the key parent is keyed by a plain row key".to_string());
            }
            Some(spec) if spec.redirects => {
                problems.push("the key parent does not redirect".to_string());
            }
            Some(_) if self.may_hold_deletes(parent) => {
                problems.push("the key parent holds no deletes".to_string());
            }
            _ => {}
        }
        problems
    }

    /// Why a never-deleted table could still lose a row: a delete of a key parent or of a
    /// cascading reference's table would reach it, so those tables are never deleted either.
    fn deletion_problems(&self, table: &TableSpec) -> Vec<String> {
        if table.deletion != Deletion::Never {
            return Vec::new();
        }
        let deletable = |parent: TableId| {
            self.table(parent)
                .is_some_and(|spec| spec.deletion != Deletion::Never)
        };
        let mut problems = Vec::new();
        if table.key_parent.is_some_and(deletable) {
            problems.push("the key parent of a never-deleted table is never deleted".to_string());
        }
        for (_, _, reference) in table.references() {
            if reference.on_delete == OnDelete::Cascade && deletable(reference.table) {
                problems.push(format!(
                    "{} cascades from a table that is not never deleted",
                    reference.column
                ));
            }
        }
        problems
    }

    /// Why a retention rule cannot hold. Its group is immutable and holds the text column and a
    /// cascading reference to a parent that never redirects, so every replica reads one parent
    /// for the life of the row. Duplicate classes, trees, and edges read raw sync state, so a
    /// retained table has none, and what the parent's cascade reaches is free of retention,
    /// trees, and edges, so reading it never returns to a retained row or to unfinished
    /// derivation. A retained row can be tombstoned, which stores no conflict mask, so a
    /// retained table surfaces no group.
    fn retention_problems(&self, table: &TableSpec) -> Vec<String> {
        let Some(retained) = table.retained else {
            return Vec::new();
        };
        let mut problems = Vec::new();
        let duplicate_repair = table
            .resolutions
            .iter()
            .any(|resolution| matches!(resolution, Resolution::DuplicateRepair { .. }));
        if table.tree.is_some() || table.acyclic.is_some() || duplicate_repair {
            problems.push("a retained table repairs duplicates, a tree, or edges".to_string());
        }
        if table.groups.iter().any(|group| group.surfaced) {
            problems.push("a retained table surfaces a group".to_string());
        }
        match table.group(retained.group) {
            Some(group) if group.kind == MergeKind::Immutable => {
                let selects = group
                    .columns()
                    .iter()
                    .any(|column| column.name == retained.column && column.ty.is_text());
                if !selects {
                    problems.push(format!(
                        "retention column {} is not text of the group",
                        retained.column
                    ));
                }
                let references = group.reference_columns().any(|(_, reference)| {
                    reference.table == retained.parent && reference.on_delete == OnDelete::Cascade
                });
                if !references {
                    problems
                        .push("the retention group does not cascade from the parent".to_string());
                }
            }
            _ => problems.push("the retention group is not immutable".to_string()),
        }
        match self.table(retained.parent) {
            Some(parent) if parent.redirects => {
                problems.push("the retention parent redirects".to_string());
            }
            Some(parent) => {
                let reaches_derived = self.cascade_closure(parent).iter().any(|id| {
                    self.table(*id).is_some_and(|reached| {
                        reached.retained.is_some()
                            || reached.tree.is_some()
                            || reached.acyclic.is_some()
                    })
                });
                if reaches_derived {
                    problems.push(
                        "the retention parent reaches retention, a tree, or edges".to_string(),
                    );
                }
            }
            None => problems.push("the retention parent is not a table".to_string()),
        }
        problems
    }

    /// `table` and the tables its cascading references and key parents reach.
    fn cascade_closure(&self, table: &TableSpec) -> BTreeSet<TableId> {
        let mut reached = BTreeSet::from([table.id]);
        let mut pending = vec![table.id];
        while let Some(id) = pending.pop() {
            let Some(spec) = self.table(id) else {
                continue;
            };
            let cascades = spec
                .references()
                .filter(|(_, _, reference)| reference.on_delete == OnDelete::Cascade)
                .map(|(_, _, reference)| reference.table);
            for next in cascades.chain(spec.key_parent) {
                if reached.insert(next) {
                    pending.push(next);
                }
            }
        }
        reached
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
        if let Some(owner) = table.owner {
            let column = owner.device_column();
            let bound = column == table.key_column
                || table.groups.iter().any(|group| {
                    group.kind == MergeKind::Immutable
                        && group.columns().iter().any(|spec| spec.name == column)
                });
            if !bound {
                problems.push(format!(
                    "owner column {column} is neither the key nor an immutable column"
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
}

/// Why a group's references are malformed. Every reference is a single optional row key
/// column, declared once. An immutable reference cascades, because clearing it would change an
/// immutable column outside apply, which the immutability guard refuses.
fn reference_problems(group: &GroupSpec) -> Vec<String> {
    let mut problems = Vec::new();
    let columns = group.columns();
    match group.kind {
        MergeKind::Reference { .. } => {
            if !matches!(columns, [column] if column.ty.is_row_key()) {
                problems.push("a reference group holds one row key column".to_string());
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
            Some(column) if !column.ty.is_row_key() => problems.push(format!(
                "{} holds a reference but is not a row key",
                reference.column
            )),
            Some(column)
                if column.ty == ColumnType::RowKey && reference.on_delete != OnDelete::Cascade =>
            {
                problems.push(format!(
                    "{} is required, so its reference must cascade",
                    reference.column
                ));
            }
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
/// generic codec writes fit one blob field unless the domain bounds them. Nested tables take the
/// bound of their parent table.
pub(crate) fn owned_table_problems(owned: &OwnedTable, nested: bool) -> Vec<String> {
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
            Some(column) if column.ty.is_row_key() => {}
            _ => problems.push(format!("{} is not a row key data column", reference.column)),
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
            if !nested
                && owned.bound == OwnedBound::Static
                && owned.max_encoded_bytes() > MAX_BLOB_FIELD_BYTES
            {
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
/// derived-key table never tombstones, so it neither redirects nor repairs duplicates. Its key
/// inputs are immutable columns, and its immutable columns are the key column or key inputs.
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
    // A re-add, or a Create covering a known row, writes every group again, and merge lets it
    // replace Immutable versions, so they may hold only values the key derives from.
    for column in table.immutable_columns() {
        if column.name != table.key_column && !derived.key_inputs.contains(&column.name) {
            problems.push(format!(
                "immutable column {} is neither the key column nor a key input",
                column.name
            ));
        }
    }
    match derived.prefix {
        Some(prefix) => {
            if derived_key(prefix, &[KEY_PREFIX_PROBE]).is_err() {
                problems.push(format!(
                    "key prefix {prefix} is not a valid derived key prefix"
                ));
            }
            if derived.key_inputs.is_empty() {
                problems.push("a key prefix needs key inputs".to_string());
            }
        }
        None => {
            if !derived.key_inputs.is_empty() {
                problems.push("key inputs need a key prefix".to_string());
            }
            if table.has_composite_key() {
                problems.push("a composite-key table derives its key with a prefix".to_string());
            }
        }
    }
    problems
}

/// A valid key part, used to check a declared prefix through the only key encoder.
const KEY_PREFIX_PROBE: &str = "probe";

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
    if let Some(position) = tree.position_group
        && table.group(position).map(|group| group.kind) != Some(MergeKind::Position)
    {
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
