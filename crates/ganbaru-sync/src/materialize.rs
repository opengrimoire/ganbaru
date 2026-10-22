//! Writing the merge state of dirty rows into the domain tables.
//!
//! A live row materializes with the winning value of every group, its references resolved
//! through redirects and duplicate hiding, and the derived columns its adapter computes. A
//! tombstoned row and a row [`Visibility`] hides are deleted. A reference to a hidden row
//! materializes as null, and so does the parent of a detached tree row. Engine writes run with
//! capture suppressed and foreign keys deferred, so rows of several tables, including the
//! tables of one reference cycle and rows referencing their own table, can change in steps and
//! are checked at commit.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::{Field, TableId};
use sqlx::{AssertSqlSafe, SqliteConnection};

use crate::Engine;
use crate::error::{SyncResult, corrupt};
use crate::manifest::{
    ColumnSpec, GroupSpec, GroupStorage, MergeKind, OwnedTable, Resolution, TableSpec,
};
use crate::merge::{self, Batch, MergedRows, RowState};
use crate::sql::{self, ident};
use crate::visibility::{RowRef, Visibility};

/// Domain state a row must have.
enum Desired {
    Absent,
    Present(Present),
}

/// Domain values of a live row.
struct Present {
    columns: Vec<(&'static GroupSpec, Vec<Field>)>,
    owned: Vec<(&'static GroupSpec, OwnedTable, Vec<Vec<Field>>)>,
    derived: Vec<(&'static str, Field)>,
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
    owned: Vec<(OwnedTable, Vec<Vec<Field>>)>,
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

async fn desired(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: &'static TableSpec,
    key: &str,
    visibility: &mut Visibility,
    merged: &mut MergedRows,
) -> SyncResult<Option<Desired>> {
    let (state, versions) = match merged.remove(&(table.id, key.to_string())) {
        Some((state, versions)) => (state, Some(versions)),
        None => match merge::row_state(conn, table.id, key).await? {
            Some(state) => (state, None),
            None => return Ok(None),
        },
    };
    if state == RowState::Tombstoned || visibility.hidden(engine, conn, table.id, key).await? {
        return Ok(Some(Desired::Absent));
    }
    let versions = match versions {
        Some(versions) => versions,
        None => merge::versions(conn, table.id, key).await?,
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
    };
    for group in table.groups {
        if table.is_exists_group(group.id) {
            continue;
        }
        let version = winners
            .get(&group.id)
            .ok_or_else(|| corrupt("a live row lacks a group version"))?;
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
        match group.storage {
            GroupStorage::Columns(columns) => {
                let mut fields = version.value.fields().to_vec();
                if fields.len() != columns.len() {
                    return Err(corrupt("stored value does not match its group"));
                }
                let identity = table.has_composite_key() && group.kind == MergeKind::Immutable;
                for (field, reference) in group.reference_columns().filter(|_| !identity) {
                    if detached_parent == Some(group.id) {
                        fields[field] = Field::Null;
                    } else if let Some(Field::Text(referenced)) = fields.get(field) {
                        let resolved = resolve_reference(
                            engine,
                            conn,
                            visibility,
                            reference.table,
                            referenced,
                        )
                        .await?;
                        fields[field] = resolved;
                    }
                }
                present.columns.push((group, fields));
            }
            GroupStorage::Owned(owned) => {
                let adapter = adapter.ok_or_else(|| corrupt("owned group without adapter"))?;
                let rows = adapter
                    .decode_owned(table.id, group.id, &version.value)
                    .map_err(|error| corrupt(format!("stored value: {error}")))?;
                present.owned.push((group, owned, rows));
            }
        }
    }
    Ok(Some(Desired::Present(present)))
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

/// Keys of domain rows of `referrer` whose reference columns name `key` of `target`.
async fn domain_referrers(
    conn: &mut SqliteConnection,
    referrer: &TableSpec,
    target: TableId,
    key: &str,
) -> SyncResult<Vec<String>> {
    let mut keys = Vec::new();
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
        let same = stored.len() == rows.len()
            && stored.iter().zip(rows).all(|(stored, row)| {
                stored.len() == row.len()
                    && stored
                        .iter()
                        .zip(row)
                        .all(|(stored, field)| stored.as_ref() == Some(field))
            });
        if !same {
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
    while let Some(index) = worklists.iter().position(|keys| !keys.is_empty()) {
        let table = &tables[index];
        let keys = std::mem::take(&mut worklists[index]);
        for key in keys {
            if plans[index].contains_key(&key) {
                continue;
            }
            let desired = desired(engine, conn, table, &key, visibility, &mut merged).await?;
            if matches!(desired, Some(Desired::Absent)) && domain_exists(conn, table, &key).await? {
                // Deleting the row clears or cascades references to it at once, so referrers
                // must be materialized again even when their merge state did not change.
                for (position, referrer) in tables.iter().enumerate() {
                    let keys = domain_referrers(conn, referrer, table.id, &key).await?;
                    worklists[position].extend(keys);
                }
            }
            plans[index].insert(key, desired);
        }
    }
    Ok(Plan { plans })
}

/// Writes a [`Plan`] into the domain tables and returns the tables it changed, in manifest
/// order.
pub(crate) async fn execute(
    engine: &Engine,
    conn: &mut SqliteConnection,
    plan: Plan,
) -> SyncResult<Vec<&'static str>> {
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

    let mut changed = vec![false; tables.len()];
    for (index, table) in tables.iter().enumerate().rev() {
        for (key, desired) in &plans[index] {
            if !matches!(desired, Desired::Absent) || !domain_exists(conn, table, key).await? {
                continue;
            }
            for (_, owned) in table.owned_children() {
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
            changed[index] = true;
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
        changed[index] = true;
        move_aside(conn, table, &updates).await?;
        for (key, update) in &updates {
            execute_update(conn, table, key, update).await?;
        }
        for (key, present) in inserts {
            execute_insert(conn, table, key, present).await?;
        }
    }
    Ok(tables
        .iter()
        .zip(changed)
        .filter(|(_, changed)| *changed)
        .map(|(table, _)| table.name)
        .collect())
}
