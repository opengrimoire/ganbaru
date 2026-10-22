//! Duplicate repair, cascade repair, and reference expansion.
//!
//! Live rows whose winning values collide on a declared unique column set form a class. Only the
//! lowest key of a class is materialized; the others are hidden, and replicas that can seal
//! repair them with tombstones redirecting to that key. A live row hidden by cascade from a
//! deleted root is repaired with a tombstone naming that root as its cause, and a tree row
//! detached to break a cycle or limit depth with a null write of its parent. Hiding and
//! detaching are pure functions of the merge state, so replicas that cannot seal still
//! converge on the same domain rows.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::{
    Cause, Change, ChangeAction, Field, GroupId, GroupValue, RowKey, TableId, Value,
};
use sqlx::SqliteConnection;

use crate::Engine;
use crate::error::{SyncResult, corrupt};
use crate::manifest::{Collation, Resolution, TableSpec, TreeSpec};
use crate::materialize;
use crate::merge::{self, Batch, RowState, Version, table_param};
use crate::sql;
use crate::visibility::{Duplicates, RowRef, Visibility};

/// Where one unique column lives: its group and its field index in the group value.
#[derive(Debug, Clone, Copy)]
struct Location {
    group: GroupId,
    field: usize,
}

fn locate(table: &TableSpec, columns: &[&str]) -> SyncResult<Vec<Location>> {
    columns
        .iter()
        .map(|column| {
            table
                .groups
                .iter()
                .find_map(|group| {
                    group
                        .columns()
                        .iter()
                        .position(|spec| spec.name == *column)
                        .map(|field| Location {
                            group: group.id,
                            field,
                        })
                })
                .ok_or_else(|| corrupt(format!("unique column {column} is not replicated")))
        })
        .collect()
}

/// The class a row's unique fields fall in under a collation; `None` when a field is null,
/// because null never collides.
pub(crate) fn class_key(fields: &[Field], collation: Collation) -> Option<Vec<u8>> {
    let folded = fields
        .iter()
        .map(|field| match field {
            Field::Null => None,
            Field::Text(text) => Some(Field::Text(collation.fold(text))),
            other => Some(other.clone()),
        })
        .collect::<Option<Vec<Field>>>()?;
    Value::new(folded).ok().map(|value| value.encode())
}

fn rules(table: &TableSpec) -> impl Iterator<Item = (&'static [&'static str], Collation)> + '_ {
    table
        .resolutions
        .iter()
        .filter_map(|resolution| match resolution {
            Resolution::DuplicateRepair { columns, collation } => Some((*columns, *collation)),
            Resolution::WithinGroup { .. } => None,
        })
}

/// Unique fields of a live row from its winning versions.
fn winning_fields(
    engine: &Engine,
    table: &TableSpec,
    locations: &[Location],
    versions: &[Version],
) -> SyncResult<Option<Vec<Field>>> {
    let winners = merge::winners(engine, table, versions);
    let mut fields = Vec::with_capacity(locations.len());
    for location in locations {
        let Some(version) = winners.get(&location.group) else {
            return Ok(None);
        };
        let field = version
            .value
            .fields()
            .get(location.field)
            .ok_or_else(|| corrupt("stored value is shorter than its group"))?;
        fields.push(field.clone());
    }
    Ok(Some(fields))
}

/// Classes of the live rows of a table under one rule, by class key.
async fn classes(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: &TableSpec,
    locations: &[Location],
    collation: Collation,
) -> SyncResult<BTreeMap<Vec<u8>, BTreeSet<String>>> {
    let groups: BTreeSet<GroupId> = locations.iter().map(|location| location.group).collect();
    let placeholders = vec!["?"; groups.len()].join(", ");
    let sql = format!(
        "SELECT v.row_key, v.group_id, v.writer_id, v.seq, v.clock, v.value_hash, v.value
         FROM sync_register_versions v JOIN sync_rows r
            ON r.table_id = v.table_id AND r.row_key = v.row_key
         WHERE v.table_id = ? AND r.state = 'live' AND v.group_id IN ({placeholders})
         ORDER BY v.row_key"
    );
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.as_str())).bind(table_param(table.id));
    for group in &groups {
        query = query.bind(i64::from(group.get()));
    }
    let rows = query.fetch_all(&mut *conn).await?;
    let mut by_row: BTreeMap<String, Vec<Version>> = BTreeMap::new();
    for row in &rows {
        let key: String = sqlx::Row::try_get(row, 0)?;
        by_row
            .entry(key)
            .or_default()
            .push(merge::version_from_row(row, 1)?);
    }
    let mut classes: BTreeMap<Vec<u8>, BTreeSet<String>> = BTreeMap::new();
    for (key, versions) in by_row {
        let Some(fields) = winning_fields(engine, table, locations, &versions)? else {
            continue;
        };
        if let Some(class) = class_key(&fields, collation) {
            classes.entry(class).or_default().insert(key);
        }
    }
    Ok(classes)
}

/// Hidden duplicates of every table with a duplicate rule, marking dirty the rows whose
/// materialization a dirty row's old or new unique values may change.
pub(crate) async fn duplicates(
    engine: &Engine,
    conn: &mut SqliteConnection,
    dirty: &mut BTreeSet<RowRef>,
) -> SyncResult<Duplicates> {
    let mut hidden = Duplicates::new();
    for table in engine.manifest().tables {
        let keys: Vec<String> = dirty
            .iter()
            .filter(|(table_id, _)| *table_id == table.id)
            .map(|(_, key)| key.clone())
            .collect();
        let mut table_hidden = BTreeMap::new();
        for (columns, collation) in rules(table) {
            let locations = locate(table, columns)?;
            let classes = classes(engine, conn, table, &locations, collation).await?;
            let class_of: BTreeMap<&str, &Vec<u8>> = classes
                .iter()
                .flat_map(|(class, members)| members.iter().map(move |key| (key.as_str(), class)))
                .collect();
            let mut affected: BTreeSet<&Vec<u8>> = BTreeSet::new();
            for key in &keys {
                if let Some(class) = class_of.get(key.as_str()) {
                    affected.insert(class);
                }
                let materialized =
                    sql::read_columns(conn, table.name, table.key_column, key, columns).await?;
                let previous = materialized
                    .and_then(|fields| fields.into_iter().collect::<Option<Vec<Field>>>())
                    .and_then(|fields| class_key(&fields, collation));
                if let Some(class) = previous.and_then(|class| classes.get_key_value(&class)) {
                    affected.insert(class.0);
                }
            }
            for class in affected {
                for key in &classes[class] {
                    dirty.insert((table.id, key.clone()));
                }
            }
            for members in classes.values().filter(|members| members.len() > 1) {
                let mut members = members.iter();
                let Some(keeper) = members.next() else {
                    continue;
                };
                for member in members {
                    table_hidden
                        .entry(member.clone())
                        .or_insert_with(|| keeper.clone());
                }
            }
        }
        if !table_hidden.is_empty() {
            hidden.insert(table.id, table_hidden);
        }
    }
    Ok(hidden)
}

/// Rows whose references in any retained version name a row, directly or through a tombstone
/// redirecting to it, and rows whose tombstones name it as their cause.
pub(crate) async fn referrers(
    engine: &Engine,
    conn: &mut SqliteConnection,
    target: TableId,
    key: &str,
) -> SyncResult<BTreeSet<RowRef>> {
    let specs: BTreeSet<(TableId, GroupId, usize)> = engine
        .manifest()
        .tables
        .iter()
        .flat_map(|table| {
            let columns = table
                .references()
                .filter(|(_, _, reference)| reference.table == target)
                .map(move |(group, field, _)| (table.id, group.id, field));
            let owned = table
                .owned_references()
                .filter(|(_, _, _, reference)| reference.table == target)
                .map(move |(group, field, _, _)| (table.id, group.id, field));
            columns.chain(owned)
        })
        .collect();
    let caused: Vec<(i64, String)> = sqlx::query_as(
        "SELECT DISTINCT table_id, row_key FROM sync_tombstones
         WHERE cause_table = ? AND cause_row = ?",
    )
    .bind(table_param(target))
    .bind(key)
    .fetch_all(&mut *conn)
    .await?;
    let mut found = caused
        .into_iter()
        .map(|(table_id, row_key)| {
            u16::try_from(table_id)
                .map(|table| (TableId(table), row_key))
                .map_err(|_| corrupt("table id out of range"))
        })
        .collect::<SyncResult<BTreeSet<RowRef>>>()?;
    if specs.is_empty() {
        return Ok(found);
    }
    let mut pending = vec![key.to_string()];
    let mut seen: BTreeSet<String> = pending.iter().cloned().collect();
    while let Some(key) = pending.pop() {
        let redirected: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT row_key FROM sync_tombstones WHERE table_id = ? AND replaced_by = ?",
        )
        .bind(table_param(target))
        .bind(&key)
        .fetch_all(&mut *conn)
        .await?;
        for source in redirected {
            if seen.insert(source.clone()) {
                pending.push(source);
            }
        }
    }
    for key in seen {
        let rows: Vec<(i64, String, i64, i64)> = sqlx::query_as(
            "SELECT DISTINCT table_id, row_key, group_id, field FROM sync_version_refs
             WHERE ref_key = ?",
        )
        .bind(&key)
        .fetch_all(&mut *conn)
        .await?;
        for (table_id, row_key, group_id, field) in rows {
            let table_id = u16::try_from(table_id)
                .map(TableId)
                .map_err(|_| corrupt("table id out of range"))?;
            let field =
                usize::try_from(field).map_err(|_| corrupt("reference field out of range"))?;
            if specs.contains(&(table_id, merge::group_id(group_id)?, field)) {
                found.insert((table_id, row_key));
            }
        }
    }
    Ok(found)
}

/// Whether the marking continues through a published referrer: it is hidden, so the recovery
/// state of the rows below it may follow a changed root, or it is shown but missing from the
/// domain, so materializing it inserts it.
async fn continues_through(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &mut Visibility,
    (table, key): &RowRef,
) -> SyncResult<bool> {
    if merge::row_state(conn, *table, key).await?.is_none() {
        return Ok(false);
    }
    if visibility.hidden(engine, conn, *table, key).await? {
        return Ok(true);
    }
    let spec = engine
        .manifest()
        .table(*table)
        .ok_or_else(|| corrupt("dirty row of an unknown table"))?;
    Ok(!materialize::domain_exists(conn, spec, key).await?)
}

/// Marks dirty every row whose reference may resolve differently because a referenced row, or
/// a row redirecting to it, is dirty, and every row whose tombstone names a dirty row as its
/// cause. The marking continues through referrers that are hidden or become shown, so a
/// cascade reaches every descendant.
pub(crate) async fn expand_references(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    visibility: &mut Visibility,
) -> SyncResult<()> {
    let mut pending: Vec<RowRef> = batch.dirty.iter().cloned().collect();
    let mut seen: BTreeSet<RowRef> = pending.iter().cloned().collect();
    while let Some((table, key)) = pending.pop() {
        for referrer in referrers(engine, conn, table, &key).await? {
            batch.dirty.insert(referrer.clone());
            if seen.insert(referrer.clone())
                && continues_through(engine, conn, visibility, &referrer).await?
            {
                pending.push(referrer);
            }
        }
    }
    Ok(())
}

fn stored_key(key: &str) -> SyncResult<RowKey> {
    RowKey::new(key).map_err(|_| corrupt("stored row key"))
}

/// Null writes of the parent group of every detached tree row that is otherwise shown, so the
/// row stays a root once the repair wins.
async fn detach_writes(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &mut Visibility,
    table: &TableSpec,
    tree: TreeSpec,
    changes: &mut Vec<Change>,
) -> SyncResult<()> {
    let detached: Vec<String> = visibility
        .detached
        .iter()
        .filter(|(detached, _)| *detached == table.id)
        .map(|(_, key)| key.clone())
        .collect();
    for key in detached {
        if visibility.hidden(engine, conn, table.id, &key).await? {
            continue;
        }
        let value = Value::new(vec![Field::Null])
            .map_err(|_| corrupt("null parent value out of bounds"))?;
        changes.push(Change {
            table: table.id,
            row: stored_key(&key)?,
            action: ChangeAction::Write,
            groups: vec![GroupValue::new(table.id, tree.parent_group, value)],
            replaced_by: None,
            cause: None,
        });
    }
    Ok(())
}

/// Repairs for the merge state: null parents for detached tree rows, tombstones for hidden
/// duplicates, redirecting to the kept row where the table allows it, and tombstones naming
/// their cause for dirty live rows hidden by cascade from a deleted root, with referencing
/// tables first. Derived-key rows are never tombstoned; a hidden one stays live in sync state.
pub(crate) async fn repair_changes(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &mut Visibility,
    dirty: &BTreeSet<RowRef>,
) -> SyncResult<Vec<Change>> {
    let mut changes = Vec::new();
    for table in engine.manifest().tables.iter().rev() {
        if table.derived_key.is_some() {
            continue;
        }
        let duplicates = visibility
            .duplicates
            .get(&table.id)
            .cloned()
            .unwrap_or_default();
        if let Some(tree) = table.tree {
            detach_writes(engine, conn, visibility, table, tree, &mut changes).await?;
        }
        for (key, keeper) in &duplicates {
            changes.push(Change {
                table: table.id,
                row: stored_key(key)?,
                action: ChangeAction::Tombstone,
                groups: Vec::new(),
                replaced_by: table.redirects.then(|| stored_key(keeper)).transpose()?,
                cause: None,
            });
        }
        for (_, key) in dirty.iter().filter(|(dirty, _)| *dirty == table.id) {
            if merge::row_state(conn, table.id, key).await? != Some(RowState::Live)
                || visibility.self_hidden(engine, conn, table.id, key).await?
            {
                continue;
            }
            let roots = visibility
                .cascade_roots(engine, conn, table.id, key)
                .await?;
            let Some(root) = roots.cause() else {
                continue;
            };
            changes.push(Change {
                table: table.id,
                row: stored_key(key)?,
                action: ChangeAction::Tombstone,
                groups: Vec::new(),
                replaced_by: None,
                cause: Some(Cause {
                    table: root.table,
                    row: stored_key(&root.key)?,
                }),
            });
        }
    }
    Ok(changes)
}
