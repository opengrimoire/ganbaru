//! Writing the merge state of dirty rows into the domain tables.
//!
//! A live row materializes with the winning value of every group, its references resolved
//! through redirects and duplicate hiding, and the derived columns its adapter computes. A
//! tombstoned row and a row [`Visibility`] hides are deleted. A reference to a hidden row
//! materializes as null, and so does the parent of a detached tree row. An owned row whose
//! reference reaches no shown row is left out with its nested rows, while it stays in the group
//! value. Engine writes run with capture suppressed and foreign keys deferred, so rows of
//! several tables, including the tables of one reference cycle and rows referencing their own
//! table, can change in steps and are checked at commit. After the writes, hooks settle the
//! concurrent versions of the groups they name, then run with the keys of every row that
//! changed. Values elided from a row are stored again before the row is updated or deleted,
//! see [`crate::hydrate`].

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::{Field, GroupId, TableId, WriterId};
use sqlx::{AssertSqlSafe, SqliteConnection};

use crate::Engine;
use crate::codec::{self, OwnedRow};
use crate::error::{SyncError, SyncResult, corrupt};
use crate::hooks::{ChangeNotice, ConcurrentVersion, Notices};
use crate::hydrate::{self, Elision};
use crate::manifest::{ColumnSpec, GroupSpec, MergeKind, OwnedTable, Resolution, TableSpec};
use crate::merge::{self, Batch, MergedRows, RowState};
use crate::seal::{self, InvalidRow};
use crate::sql::{self, ident};
use crate::validate;
use crate::visibility::{RowRef, Visibility};

/// Domain state a row must have.
enum Desired {
    Absent,
    Present(Present),
}

/// Groups a table's hook settles that keep several concurrent versions, with them.
type Concurrent = Vec<(&'static GroupSpec, Vec<ConcurrentVersion>)>;

/// Domain values of a live row.
struct Present {
    columns: Vec<(&'static GroupSpec, Vec<Field>)>,
    owned: Vec<(&'static GroupSpec, &'static OwnedTable, Vec<OwnedRow>)>,
    derived: Vec<(&'static str, Field)>,
    /// Winning versions whose stored value the materialized row holds and may drop.
    elisions: Vec<(GroupId, WriterId)>,
    concurrent: Concurrent,
}

/// What a materialization changed: the tables in manifest order and what hooks reported.
#[derive(Debug, Default)]
pub(crate) struct Materialized {
    pub(crate) tables: Vec<&'static str>,
    pub(crate) notices: Vec<ChangeNotice>,
    /// Whether rows listed as stale remain for a later engine write, since one write takes
    /// back a bounded number of them.
    pub(crate) stale_left: bool,
    /// Rows a hook settled, which now hold a captured value instead of the merge's winner.
    pub(crate) settled: BTreeSet<RowRef>,
    /// Settled values that break their group's rules, which were not written.
    pub(crate) refused: Vec<InvalidRow>,
}

/// What a domain row needs to match its desired values.
enum Diff {
    Missing,
    Same,
    Changed(Update),
}

/// Columns, derived columns, and owned rows that differ, and whether the revision is bumped.
struct Update {
    columns: Vec<(&'static str, Field)>,
    owned: Vec<(&'static OwnedTable, Vec<OwnedRow>)>,
    bump: bool,
}

/// The row a reference resolves to: the row [`Visibility::follow`] reaches while it is shown,
/// or null.
async fn resolve_reference(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &mut Visibility,
    table: TableId,
    key: &str,
) -> SyncResult<Field> {
    let Some(target) = visibility.follow(conn, table, key).await? else {
        return Ok(Field::Null);
    };
    if visibility.hidden(engine, conn, table, &target).await? {
        return Ok(Field::Null);
    }
    Ok(Field::Text(target))
}

/// Resolves the references of one owned row in place. Returns false when one reaches no shown
/// row, so the row is left out.
async fn resolve_owned_row(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &mut Visibility,
    owned: &OwnedTable,
    fields: &mut [Field],
) -> SyncResult<bool> {
    for (field, reference) in owned.reference_fields() {
        let Some(Field::Text(target)) = fields.get(field) else {
            continue;
        };
        let target = target.clone();
        match resolve_reference(engine, conn, visibility, reference.table, &target).await? {
            Field::Null => return Ok(false),
            resolved => fields[field] = resolved,
        }
    }
    Ok(true)
}

/// The owned rows to materialize: references resolved, and rows whose reference reaches no
/// shown row left out, a parent row with its nested rows.
async fn resolve_owned(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &mut Visibility,
    owned: &OwnedTable,
    rows: Vec<OwnedRow>,
) -> SyncResult<Vec<OwnedRow>> {
    let mut kept = Vec::with_capacity(rows.len());
    for mut row in rows {
        if !resolve_owned_row(engine, conn, visibility, owned, &mut row.fields).await? {
            continue;
        }
        for (nested, nested_rows) in owned.nested.iter().zip(row.nested.iter_mut()) {
            let mut nested_kept = Vec::with_capacity(nested_rows.len());
            for mut nested_row in std::mem::take(nested_rows) {
                if resolve_owned_row(engine, conn, visibility, nested, &mut nested_row.fields)
                    .await?
                {
                    nested_kept.push(nested_row);
                }
            }
            *nested_rows = nested_kept;
        }
        kept.push(row);
    }
    Ok(kept)
}

/// The domain state of one row, or `None` when the row has no merge state and nothing says it
/// must be absent. `orphaned` marks a row whose merge state an exclusion dropped.
async fn desired(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: &'static TableSpec,
    key: &str,
    orphaned: bool,
    visibility: &mut Visibility,
    merged: &mut MergedRows,
) -> SyncResult<Option<Desired>> {
    let (state, versions) = match merged.remove(&(table.id, key.to_string())) {
        Some((state, versions)) => (state, Some(versions)),
        None => match merge::row_state(conn, table.id, key).await? {
            Some(state) => (state, None),
            None if orphaned => return Ok(Some(Desired::Absent)),
            None if visibility.late_created(conn, table.id, key).await? => {
                return Ok(Some(Desired::Absent));
            }
            None => return Ok(None),
        },
    };
    if state == RowState::Tombstoned || visibility.hidden(engine, conn, table.id, key).await? {
        return Ok(Some(Desired::Absent));
    }
    let versions = match versions {
        Some(versions) => versions,
        None => merge::versions(engine, conn, table.id, key).await?,
    };
    let winners = merge::winners(engine, table, &versions);
    let detached_parent = table
        .tree
        .filter(|_| visibility.is_detached(table.id, key))
        .map(|tree| tree.parent_group);
    let adapter = engine.adapter(table);
    let mut present = Present {
        columns: Vec::new(),
        owned: Vec::new(),
        derived: Vec::new(),
        elisions: Vec::new(),
        concurrent: concurrent_versions(engine, table, &versions, &winners),
    };
    for group in table.groups {
        if table.is_exists_group(group.id) {
            continue;
        }
        // Excluding a forked branch the anchor already confirmed can leave a group without a
        // version, since nothing kept the version that branch displaced. The domain row keeps
        // its value until an operation writes the group again, as the anchor's republished
        // value does.
        let Some(version) = winners.get(&group.id) else {
            continue;
        };
        if let Some(adapter) = adapter {
            let derived = adapter
                .derived(table.id, group.id, &version.value)
                .map_err(|error| corrupt(format!("stored value: {error}")))?;
            if let Some((column, _)) = derived
                .iter()
                .find(|(column, _)| !table.derived_columns.contains(column))
            {
                return Err(corrupt(format!(
                    "adapter derived undeclared column {column}"
                )));
            }
            present.derived.extend(derived);
        }
        if version.value.fields().len() != group.field_count() {
            return Err(corrupt("stored value does not match its group"));
        }
        let stored = &version.value.fields()[..group.columns().len()];
        let mut fields = stored.to_vec();
        let identity = table.has_composite_key() && group.kind == MergeKind::Immutable;
        for (field, reference) in group.reference_columns().filter(|_| !identity) {
            if detached_parent == Some(group.id) {
                fields[field] = Field::Null;
            } else if let Some(Field::Text(referenced)) = fields.get(field) {
                let resolved =
                    resolve_reference(engine, conn, visibility, reference.table, referenced)
                        .await?;
                fields[field] = resolved;
            }
        }
        // The domain row holds the winning value exactly when resolution changed no column
        // and every owned row is kept unchanged, re-encoding to the stored blob.
        let mut holds_value = !version.elided && table.may_elide(group) && fields == stored;
        present.columns.push((group, fields));
        for (index, owned) in group.owned_tables().iter().enumerate() {
            let blob = codec::owned_blob(group, &version.value, index)
                .map_err(|error| corrupt(format!("stored value: {error}")))?;
            let rows = codec::decode_table(engine, table, owned, blob)
                .map_err(|error| corrupt(format!("stored value: {error}")))?;
            let rows = resolve_owned(engine, conn, visibility, owned, rows).await?;
            holds_value = holds_value
                && codec::encode_table(engine, table, owned, rows.clone())
                    .is_ok_and(|encoded| encoded == blob);
            present.owned.push((group, owned, rows));
        }
        if holds_value {
            present.elisions.push((group.id, version.writer));
        }
    }
    Ok(Some(Desired::Present(present)))
}

/// The concurrent versions of each group of `table` its hook settles, for groups that keep
/// more than one.
fn concurrent_versions(
    engine: &Engine,
    table: &'static TableSpec,
    versions: &[merge::Version],
    winners: &BTreeMap<GroupId, &merge::Version>,
) -> Concurrent {
    let mut concurrent = Vec::new();
    for id in engine.hooks.settled_groups(table) {
        let Some(group) = table.group(*id) else {
            continue;
        };
        let retained: Vec<&merge::Version> = versions
            .iter()
            .filter(|version| version.group == *id)
            .collect();
        if retained.len() < 2 {
            continue;
        }
        let won = winners.get(id).map(|version| version.writer);
        concurrent.push((
            group,
            retained
                .into_iter()
                .map(|version| ConcurrentVersion {
                    writer: version.writer,
                    clock: version.clock,
                    value: version.value.clone(),
                    won: won == Some(version.writer),
                })
                .collect(),
        ));
    }
    concurrent
}

pub(crate) async fn domain_exists(
    conn: &mut SqliteConnection,
    table: &TableSpec,
    key: &str,
) -> SyncResult<bool> {
    Ok(
        sql::read_columns(conn, table.name, table.key_column, key, &[])
            .await?
            .is_some(),
    )
}

/// Keys of domain rows of `referrer` whose reference columns, or owned rows, name `key` of
/// `target`, and the row of `referrer` keyed by `key` when `target` is its key parent.
async fn domain_referrers(
    conn: &mut SqliteConnection,
    referrer: &TableSpec,
    target: TableId,
    key: &str,
) -> SyncResult<Vec<String>> {
    let mut keys = Vec::new();
    if referrer.key_parent == Some(target) && domain_exists(conn, referrer, key).await? {
        keys.push(key.to_string());
    }
    for (_, _, owned, reference) in referrer.owned_references() {
        if reference.table != target {
            continue;
        }
        let sql = format!(
            "SELECT DISTINCT {} FROM {} WHERE {} = ?",
            ident(owned.parent_column),
            ident(owned.table),
            ident(reference.column)
        );
        let found: Vec<String> = sqlx::query_scalar(AssertSqlSafe(sql.as_str()))
            .bind(key)
            .fetch_all(&mut *conn)
            .await?;
        keys.extend(found);
    }
    for (_, _, reference) in referrer.references() {
        if reference.table != target {
            continue;
        }
        let sql = format!(
            "SELECT {} FROM {} WHERE {} = ?",
            ident(referrer.key_column),
            ident(referrer.name),
            ident(reference.column)
        );
        let found: Vec<String> = sqlx::query_scalar(AssertSqlSafe(sql.as_str()))
            .bind(key)
            .fetch_all(&mut *conn)
            .await?;
        keys.extend(found);
    }
    Ok(keys)
}

async fn diff(
    conn: &mut SqliteConnection,
    table: &TableSpec,
    key: &str,
    present: &Present,
) -> SyncResult<Diff> {
    let names: Vec<&str> = present
        .columns
        .iter()
        .flat_map(|(group, _)| group.columns().iter().map(|column| column.name))
        .chain(present.derived.iter().map(|(column, _)| *column))
        .collect();
    let Some(current) = sql::read_columns(conn, table.name, table.key_column, key, &names).await?
    else {
        return Ok(Diff::Missing);
    };
    let mut current = current.into_iter();
    let mut update = Update {
        columns: Vec::new(),
        owned: Vec::new(),
        bump: false,
    };
    for (group, fields) in &present.columns {
        for (column, field) in group.columns().iter().zip(fields) {
            if current.next().flatten().as_ref() != Some(field) {
                update.columns.push((column.name, field.clone()));
                update.bump |= group.bumps_revision;
            }
        }
    }
    // A changed unique column set is written whole, because moving it aside replaces one of
    // its columns with a placeholder.
    for (columns, _) in swap_rules(table)? {
        if !update
            .columns
            .iter()
            .any(|(column, _)| columns.contains(column))
        {
            continue;
        }
        for (group, fields) in &present.columns {
            for (column, field) in group.columns().iter().zip(fields) {
                if columns.contains(&column.name)
                    && !update
                        .columns
                        .iter()
                        .any(|(changed, _)| *changed == column.name)
                {
                    update.columns.push((column.name, field.clone()));
                }
            }
        }
    }
    for (column, field) in &present.derived {
        if current.next().flatten().as_ref() != Some(field) {
            update.columns.push((column, field.clone()));
        }
    }
    for (group, owned, rows) in &present.owned {
        let stored = sql::read_owned(conn, owned, key).await?;
        if stored.as_ref() != Some(rows) || !sql::owned_numbered(conn, owned, key).await? {
            update.owned.push((*owned, rows.clone()));
            update.bump |= group.bumps_revision;
        }
    }
    if update.columns.is_empty() && update.owned.is_empty() {
        Ok(Diff::Same)
    } else {
        Ok(Diff::Changed(update))
    }
}

/// For each duplicate rule, its columns and the column that can hold a placeholder.
fn swap_rules(
    table: &TableSpec,
) -> SyncResult<Vec<(&'static [&'static str], &'static ColumnSpec)>> {
    table
        .resolutions
        .iter()
        .filter_map(|resolution| match resolution {
            Resolution::DuplicateRepair { columns, .. } => Some(*columns),
            Resolution::WithinGroup { .. } => None,
        })
        .map(|columns| {
            table
                .groups
                .iter()
                .flat_map(|group| group.columns())
                .find(|spec| columns.contains(&spec.name) && spec.ty.placeholder(0).is_some())
                .map(|spec| (columns, spec))
                .ok_or_else(|| corrupt("a duplicate rule has no placeholder column"))
        })
        .collect()
}

/// Moves the unique values of rows about to change aside, so merged rows can swap values.
async fn move_aside(
    conn: &mut SqliteConnection,
    table: &TableSpec,
    updates: &[(&String, Update)],
) -> SyncResult<()> {
    let rules = swap_rules(table)?;
    let mut next = 0u64;
    for (key, update) in updates {
        for (columns, spec) in &rules {
            if !update
                .columns
                .iter()
                .any(|(column, _)| columns.contains(column))
            {
                continue;
            }
            let probe = format!(
                "SELECT count(*) FROM {} WHERE {} = ?",
                ident(table.name),
                ident(spec.name)
            );
            let placeholder = loop {
                let candidate = spec
                    .ty
                    .placeholder(next)
                    .ok_or_else(|| corrupt("placeholders are exhausted"))?;
                next += 1;
                let taken: i64 = sqlx::query_scalar(AssertSqlSafe(probe.as_str()))
                    .bind(&candidate)
                    .fetch_one(&mut *conn)
                    .await?;
                if taken == 0 {
                    break candidate;
                }
            };
            let sql = format!(
                "UPDATE {} SET {} = ? WHERE {} = ?",
                ident(table.name),
                ident(spec.name),
                ident(table.key_column)
            );
            sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(placeholder)
                .bind(key.as_str())
                .execute(&mut *conn)
                .await?;
        }
    }
    Ok(())
}

async fn execute_update(
    conn: &mut SqliteConnection,
    table: &TableSpec,
    key: &str,
    update: &Update,
) -> SyncResult<()> {
    let mut assignments: Vec<String> = update
        .columns
        .iter()
        .map(|(column, _)| format!("{} = ?", ident(column)))
        .collect();
    if update.bump
        && let Some(revision) = table.revision_column
    {
        assignments.push(format!("{name} = {name} + 1", name = ident(revision)));
    }
    if !assignments.is_empty() {
        let sql = format!(
            "UPDATE {} SET {} WHERE {} = ?",
            ident(table.name),
            assignments.join(", "),
            ident(table.key_column)
        );
        let mut query = sqlx::query(AssertSqlSafe(sql.as_str()));
        for (_, field) in &update.columns {
            query = sql::bind_field(query, field);
        }
        query.bind(key).execute(&mut *conn).await?;
    }
    for (owned, rows) in &update.owned {
        sql::replace_owned(conn, owned, key, rows).await?;
    }
    Ok(())
}

async fn execute_insert(
    conn: &mut SqliteConnection,
    table: &TableSpec,
    key: &str,
    present: &Present,
) -> SyncResult<()> {
    let columns: Vec<String> = std::iter::once(table.key_column)
        .chain(
            present
                .columns
                .iter()
                .flat_map(|(group, _)| group.columns().iter().map(|column| column.name)),
        )
        .chain(present.derived.iter().map(|(column, _)| *column))
        .chain(table.key_derived_columns.iter().map(|column| column.name))
        .map(ident)
        .collect();
    // Local columns take their declared literals; the manifest was validated at engine
    // construction, so each is a single SQL literal.
    let local_names = table.local_columns.iter().map(|column| ident(column.name));
    let local_values = table.local_columns.iter().map(|column| column.materialize);
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        ident(table.name),
        columns
            .iter()
            .cloned()
            .chain(local_names)
            .collect::<Vec<_>>()
            .join(", "),
        vec!["?"; columns.len()]
            .into_iter()
            .chain(local_values)
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut query = sqlx::query(AssertSqlSafe(sql.as_str())).bind(key);
    for field in present
        .columns
        .iter()
        .flat_map(|(_, fields)| fields)
        .chain(present.derived.iter().map(|(_, field)| field))
    {
        query = sql::bind_field(query, field);
    }
    for column in table.key_derived_columns {
        query = query.bind(column.value(key));
    }
    query.execute(&mut *conn).await?;
    for (_, owned, rows) in &present.owned {
        sql::replace_owned(conn, owned, key, rows).await?;
    }
    Ok(())
}

/// The desired state of every row a materialization writes, per manifest table. A row without
/// sync state is listed with `None`: it is left as it is, though a deletion may cascade to it.
pub(crate) struct Plan {
    plans: Vec<BTreeMap<String, Option<Desired>>>,
    /// Per manifest table, rows whose references or owned rows name a row the plan deletes,
    /// which the deletion may change by cascade.
    touched: Vec<BTreeSet<String>>,
}

impl Plan {
    /// Every row the materialization writes or may cascade to.
    pub(crate) fn rows(&self, engine: &Engine) -> impl Iterator<Item = RowRef> {
        engine
            .manifest()
            .tables
            .iter()
            .zip(&self.plans)
            .flat_map(|(table, plan)| plan.keys().map(|key| (table.id, key.clone())))
    }

    /// Winning versions whose stored value the materialized rows will hold, leaving out rows
    /// in `pending`, whose local edits may differ from the merge.
    pub(crate) fn elisions(&self, engine: &Engine, pending: &BTreeSet<RowRef>) -> Vec<Elision> {
        let mut elisions = Vec::new();
        for (table, plan) in engine.manifest().tables.iter().zip(&self.plans) {
            for (key, desired) in plan {
                let Some(Desired::Present(present)) = desired else {
                    continue;
                };
                if pending.contains(&(table.id, key.clone())) {
                    continue;
                }
                elisions.extend(
                    present
                        .elisions
                        .iter()
                        .map(|(group, writer)| (table.id, key.clone(), *group, *writer)),
                );
            }
        }
        elisions
    }
}

/// Groups whose domain columns or owned rows an update writes.
fn updated_groups(table: &TableSpec, update: &Update) -> BTreeSet<GroupId> {
    table
        .groups
        .iter()
        .filter(|group| {
            group.columns().iter().any(|column| {
                update
                    .columns
                    .iter()
                    .any(|(changed, _)| *changed == column.name)
            }) || group.owned_tables().iter().any(|owned| {
                update
                    .owned
                    .iter()
                    .any(|(changed, _)| changed.table == owned.table)
            })
        })
        .map(|group| group.id)
        .collect()
}

/// Plans every dirty row and the rows, in any table, whose references a deletion would clear
/// or cascade, without writing anything.
/// `merged` holds the merge state already read for dirty rows; other rows are read here.
pub(crate) async fn plan(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &Batch,
    visibility: &mut Visibility,
    mut merged: MergedRows,
) -> SyncResult<Plan> {
    let tables = engine.manifest().tables;
    let mut worklists: Vec<BTreeSet<String>> = vec![BTreeSet::new(); tables.len()];
    for (table_id, key) in &batch.dirty {
        let index = tables
            .iter()
            .position(|table| table.id == *table_id)
            .ok_or_else(|| corrupt("dirty row of an unknown table"))?;
        worklists[index].insert(key.clone());
    }
    // Planning runs until no table has unplanned work, because a referrer may be listed before
    // its target, in the same table, or in the same reference cycle.
    let mut plans: Vec<BTreeMap<String, Option<Desired>>> =
        (0..tables.len()).map(|_| BTreeMap::new()).collect();
    let mut touched: Vec<BTreeSet<String>> = vec![BTreeSet::new(); tables.len()];
    while let Some(index) = worklists.iter().position(|keys| !keys.is_empty()) {
        let table = &tables[index];
        let keys = std::mem::take(&mut worklists[index]);
        for key in keys {
            if plans[index].contains_key(&key) {
                continue;
            }
            let orphaned = batch.orphaned.contains(&(table.id, key.clone()));
            let desired =
                desired(engine, conn, table, &key, orphaned, visibility, &mut merged).await?;
            if matches!(desired, Some(Desired::Absent)) && domain_exists(conn, table, &key).await? {
                // Deleting the row clears or cascades references to it at once, so referrers
                // must be materialized again even when their merge state did not change.
                for (position, referrer) in tables.iter().enumerate() {
                    let keys = domain_referrers(conn, referrer, table.id, &key).await?;
                    touched[position].extend(keys.iter().cloned());
                    worklists[position].extend(keys);
                }
            }
            plans[index].insert(key, desired);
        }
    }
    Ok(Plan { plans, touched })
}

/// Whether a local capture of the row waits to be sealed.
async fn has_capture(conn: &mut SqliteConnection, table: TableId, key: &str) -> SyncResult<bool> {
    let found: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sync_capture WHERE table_id = ? AND row_key = ?")
            .bind(merge::table_param(table))
            .bind(key)
            .fetch_one(&mut *conn)
            .await?;
    Ok(found > 0)
}

/// Lets each hook settle the concurrent versions of its settled groups on the live rows of the
/// plan, see [`crate::hooks::MaterializeHook::settle`]. A settled value is written to the
/// domain row, with the derived columns the adapter computes from it, and captured as a local
/// edit of its group; its row joins `changed`. A reference column of the value must name a row
/// one of the versions names, and is written resolved as a winner's would be.
async fn settle(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &mut Visibility,
    plans: &[BTreeMap<String, Desired>],
    changed: &mut [BTreeSet<String>],
    now_ms: u64,
    materialized: &mut Materialized,
) -> SyncResult<()> {
    for (index, table) in engine.manifest().tables.iter().enumerate() {
        let Some(hook) = engine.hooks.get(table.id) else {
            continue;
        };
        let rows: Vec<(&String, &Concurrent)> = plans[index]
            .iter()
            .filter_map(|(key, desired)| match desired {
                Desired::Present(present) if !present.concurrent.is_empty() => {
                    Some((key, &present.concurrent))
                }
                _ => None,
            })
            .collect();
        if rows.is_empty() {
            continue;
        }
        for (key, concurrent) in rows {
            if has_capture(conn, table.id, key).await? {
                continue;
            }
            for (group, versions) in concurrent {
                let decided = hook
                    .settle(&mut *conn, table, key, group.id, versions)
                    .await
                    .map_err(|error| SyncError::Hook {
                        table: table.name,
                        error,
                    })?;
                let Some(value) = decided else {
                    continue;
                };
                if versions
                    .iter()
                    .any(|version| version.won && version.value == value)
                {
                    continue;
                }
                let written = match settled_update(engine, table, group, &value)? {
                    Ok(mut update) => {
                        match resolve_settled_references(
                            engine,
                            conn,
                            visibility,
                            group,
                            versions,
                            &mut update,
                        )
                        .await?
                        {
                            Ok(()) => {
                                write_settled(engine, conn, table, key, group, &update, now_ms)
                                    .await?
                                    .err()
                            }
                            Err(reason) => Some(reason),
                        }
                    }
                    Err(reason) => Some(reason),
                };
                if let Some(reason) = written {
                    materialized.refused.push(InvalidRow {
                        table: table.id,
                        row_key: key.clone(),
                        reason,
                    });
                    continue;
                }
                changed[index].insert(key.clone());
                materialized.settled.insert((table.id, key.clone()));
            }
        }
    }
    Ok(())
}

/// Resolves the reference columns of a settled `update`, whose leading columns are the group's
/// in order, as [`desired`] resolves a winner's. A hook only chooses among the references the
/// concurrent `versions` name, so a value naming any other row is refused.
async fn resolve_settled_references(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &mut Visibility,
    group: &'static GroupSpec,
    versions: &[ConcurrentVersion],
    update: &mut Update,
) -> SyncResult<Result<(), validate::Invalid>> {
    for (field, reference) in group.reference_columns() {
        let Some((_, settled)) = update.columns.get(field) else {
            return Err(corrupt("settled value does not match its group"));
        };
        let named = versions
            .iter()
            .any(|version| version.value.fields().get(field) == Some(settled));
        if !named {
            return Ok(Err(
                "settled reference names a row no concurrent version names",
            ));
        }
        if let Field::Text(referenced) = settled {
            let referenced = referenced.clone();
            let resolved =
                resolve_reference(engine, conn, visibility, reference.table, &referenced).await?;
            update.columns[field].1 = resolved;
        }
    }
    Ok(Ok(()))
}

/// The update that writes a settled value into the columns of its group and the derived
/// columns the adapter computes from it, or why the value breaks the group's rules.
fn settled_update(
    engine: &Engine,
    table: &'static TableSpec,
    group: &'static GroupSpec,
    value: &ganbaru_sync_contracts::Value,
) -> SyncResult<Result<Update, validate::Invalid>> {
    if let Err(reason) = validate::group_value(engine, table, group, value) {
        return Ok(Err(reason));
    }
    let mut columns: Vec<(&'static str, Field)> = group
        .columns()
        .iter()
        .zip(value.fields())
        .map(|(column, field)| (column.name, field.clone()))
        .collect();
    if let Some(adapter) = engine.adapter(table) {
        let derived = match adapter.derived(table.id, group.id, value) {
            Ok(derived) => derived,
            Err(error) => return Ok(Err(error.0)),
        };
        if let Some((column, _)) = derived
            .iter()
            .find(|(column, _)| !table.derived_columns.contains(column))
        {
            return Err(corrupt(format!(
                "adapter derived undeclared column {column}"
            )));
        }
        columns.extend(derived);
    }
    Ok(Ok(Update {
        columns,
        owned: Vec::new(),
        bump: group.bumps_revision,
    }))
}

/// Writes a settled `update` and captures it as a local edit of `group`, storing again a winner
/// the row held while its value was elided. Inside a savepoint, so a value a constraint or an
/// aborting trigger of the domain table refuses writes nothing and is returned as invalid
/// instead of failing apply. Foreign keys are deferred to the engine write's commit, so they
/// cannot be refused here; none can fail: a settled group holds no references (see
/// [`crate::hooks::settle_problem`]) and derived columns are never foreign keys (see
/// [`crate::manifest::TableSpec::derived_columns`]).
async fn write_settled(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: &'static TableSpec,
    key: &str,
    group: &'static GroupSpec,
    update: &Update,
    now_ms: u64,
) -> SyncResult<Result<(), validate::Invalid>> {
    let mut savepoint = sqlx::Connection::begin(&mut *conn).await?;
    hydrate::persist(
        engine,
        &mut savepoint,
        table,
        key,
        Some(&BTreeSet::from([group.id])),
    )
    .await?;
    match execute_update(&mut savepoint, table, key, update).await {
        Ok(()) => {}
        Err(SyncError::Database(sqlx::Error::Database(error))) if is_constraint(error.as_ref()) => {
            savepoint.rollback().await?;
            return Ok(Err("settled value refused by the domain table"));
        }
        Err(error) => return Err(error),
    }
    seal::capture_edit(&mut savepoint, table.id, key, group.mask(), now_ms).await?;
    savepoint.commit().await?;
    Ok(Ok(()))
}

/// SQLite's primary result code for a constraint violation, the low byte of every extended
/// `SQLITE_CONSTRAINT_*` code.
const SQLITE_CONSTRAINT: i64 = 19;

/// Whether a database error is a constraint violation of any kind: a CHECK, NOT NULL, UNIQUE,
/// or foreign key, and also a domain trigger's `RAISE(ABORT)`, which SQLite reports as
/// `SQLITE_CONSTRAINT_TRIGGER` and sqlx maps to [`sqlx::error::ErrorKind::Other`].
fn is_constraint(error: &dyn sqlx::error::DatabaseError) -> bool {
    error
        .code()
        .and_then(|code| code.parse::<i64>().ok())
        .is_some_and(|code| code & 0xff == SQLITE_CONSTRAINT)
}

/// Writes a [`Plan`] into the domain tables, lets hooks settle concurrent versions when
/// `settles` says the caller could seal the settled values, then runs the registered hooks,
/// and returns what changed.
pub(crate) async fn execute(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &mut Visibility,
    plan: Plan,
    settles: bool,
    now_ms: u64,
) -> SyncResult<Materialized> {
    let tables = engine.manifest().tables;
    let plans: Vec<BTreeMap<String, Desired>> = plan
        .plans
        .into_iter()
        .map(|plan| {
            plan.into_iter()
                .filter_map(|(key, desired)| desired.map(|desired| (key, desired)))
                .collect()
        })
        .collect();

    // Values elided from rows that go absent, or that a deletion clears or cascades to, are
    // stored again while the domain rows still hold them.
    for (index, table) in tables.iter().enumerate() {
        for (key, desired) in &plans[index] {
            if matches!(desired, Desired::Absent) {
                hydrate::persist(engine, conn, table, key, None).await?;
            }
        }
        for key in &plan.touched[index] {
            hydrate::persist(engine, conn, table, key, None).await?;
        }
    }

    let mut changed: Vec<BTreeSet<String>> = vec![BTreeSet::new(); tables.len()];
    let mut deleted = false;
    for (index, table) in tables.iter().enumerate().rev() {
        for (key, desired) in &plans[index] {
            if !matches!(desired, Desired::Absent) || !domain_exists(conn, table, key).await? {
                continue;
            }
            // Nested tables follow their owned table, so the reverse deletes them first.
            let children: Vec<_> = table.owned_children().collect();
            for (_, owned) in children.into_iter().rev() {
                let sql = format!(
                    "DELETE FROM {} WHERE {} = ?",
                    ident(owned.table),
                    ident(owned.parent_column)
                );
                sqlx::query(AssertSqlSafe(sql.as_str()))
                    .bind(key)
                    .execute(&mut *conn)
                    .await?;
            }
            let sql = format!(
                "DELETE FROM {} WHERE {} = ?",
                ident(table.name),
                ident(table.key_column)
            );
            sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(key)
                .execute(&mut *conn)
                .await?;
            changed[index].insert(key.clone());
            deleted = true;
        }
    }
    if deleted {
        for (keys, touched) in changed.iter_mut().zip(plan.touched) {
            keys.extend(touched);
        }
    }

    for (index, table) in tables.iter().enumerate() {
        let mut updates = Vec::new();
        let mut inserts = Vec::new();
        for (key, desired) in &plans[index] {
            let Desired::Present(present) = desired else {
                continue;
            };
            match diff(conn, table, key, present).await? {
                Diff::Missing => inserts.push((key, present)),
                Diff::Same => {}
                Diff::Changed(update) => updates.push((key, update)),
            }
        }
        if updates.is_empty() && inserts.is_empty() {
            continue;
        }
        for (key, update) in &updates {
            let groups = updated_groups(table, update);
            hydrate::persist(engine, conn, table, key, Some(&groups)).await?;
        }
        move_aside(conn, table, &updates).await?;
        for (key, update) in &updates {
            execute_update(conn, table, key, update).await?;
            changed[index].insert((*key).clone());
        }
        for (key, present) in inserts {
            execute_insert(conn, table, key, present).await?;
            changed[index].insert(key.clone());
        }
    }

    let mut materialized = Materialized::default();
    if settles {
        settle(
            engine,
            conn,
            visibility,
            &plans,
            &mut changed,
            now_ms,
            &mut materialized,
        )
        .await?;
    }

    let mut notices = Notices::default();
    for (table, keys) in tables.iter().zip(&changed) {
        let Some(hook) = engine.hooks.get(table.id) else {
            continue;
        };
        if keys.is_empty() {
            continue;
        }
        let keys: Vec<String> = keys.iter().cloned().collect();
        hook.after_materialize(&mut *conn, table, &keys, &mut notices)
            .await
            .map_err(|error| SyncError::Hook {
                table: table.name,
                error,
            })?;
    }
    materialized.tables = tables
        .iter()
        .zip(&changed)
        .filter(|(_, keys)| !keys.is_empty())
        .map(|(table, _)| table.name)
        .collect();
    materialized.notices = notices.0.into_iter().collect();
    Ok(materialized)
}
