//! Duplicate repair and reference expansion.
//!
//! Live rows whose winning values collide on a declared unique column set form a class. Only the
//! lowest key of a class is materialized; the others are hidden, and replicas that can seal
//! repair them with tombstones redirecting to that key. Hiding is a pure function of the merge
//! state, so replicas that cannot seal still converge on the same domain rows.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::{Change, ChangeAction, Field, GroupId, RowKey, TableId, Value};
use sqlx::SqliteConnection;

use crate::Engine;
use crate::error::{SyncResult, corrupt};
use crate::manifest::{Collation, MergeKind, Resolution, TableSpec};
use crate::merge::{self, Batch, Version, table_param};
use crate::sql;

/// Hidden live rows per table, each with the key of the row it collides with and is kept.
pub(crate) type Hidden = BTreeMap<TableId, BTreeMap<String, String>>;

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
    let mut query = sqlx::query(&sql).bind(table_param(table.id));
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

/// Hidden rows of every table with a duplicate rule, marking dirty the rows whose
/// materialization a dirty row's old or new unique values may change.
pub(crate) async fn hidden_rows(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
) -> SyncResult<Hidden> {
    let mut hidden = Hidden::new();
    for table in engine.manifest().tables {
        let dirty: Vec<String> = batch
            .dirty
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
            for key in &dirty {
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
                    batch.dirty.insert((table.id, key.clone()));
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

/// Marks dirty every row whose reference may resolve differently because a referenced row, or
/// a row redirecting to it, is dirty.
pub(crate) async fn expand_references(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
) -> SyncResult<()> {
    for target in engine.manifest().tables {
        let referrers: BTreeSet<(TableId, GroupId)> = engine
            .manifest()
            .tables
            .iter()
            .flat_map(|table| {
                table
                    .groups
                    .iter()
                    .filter_map(move |group| match group.kind {
                        MergeKind::Reference { table: referenced } if referenced == target.id => {
                            Some((table.id, group.id))
                        }
                        _ => None,
                    })
            })
            .collect();
        if referrers.is_empty() {
            continue;
        }
        let mut pending: Vec<String> = batch
            .dirty
            .iter()
            .filter(|(table_id, _)| *table_id == target.id)
            .map(|(_, key)| key.clone())
            .collect();
        let mut seen: BTreeSet<String> = pending.iter().cloned().collect();
        while let Some(key) = pending.pop() {
            let redirected: Vec<String> = sqlx::query_scalar(
                "SELECT DISTINCT row_key FROM sync_tombstones WHERE table_id = ? AND replaced_by = ?",
            )
            .bind(table_param(target.id))
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
            let rows: Vec<(i64, String, i64)> = sqlx::query_as(
                "SELECT DISTINCT table_id, row_key, group_id FROM sync_register_versions
                 WHERE ref_key = ?",
            )
            .bind(&key)
            .fetch_all(&mut *conn)
            .await?;
            for (table_id, row_key, group_id) in rows {
                let table_id = u16::try_from(table_id)
                    .map(TableId)
                    .map_err(|_| corrupt("table id out of range"))?;
                if referrers.contains(&(table_id, merge::group_id(group_id)?)) {
                    batch.dirty.insert((table_id, row_key));
                }
            }
        }
    }
    Ok(())
}

/// Tombstones for hidden rows, redirecting to the kept row where the table allows it, with
/// referencing tables first.
pub(crate) fn repair_changes(engine: &Engine, hidden: &Hidden) -> SyncResult<Vec<Change>> {
    let mut changes = Vec::new();
    for table in engine.manifest().tables.iter().rev() {
        let Some(rows) = hidden.get(&table.id) else {
            continue;
        };
        for (key, keeper) in rows {
            let row = RowKey::new(key.as_str()).map_err(|_| corrupt("stored row key"))?;
            let replaced_by = if table.redirects {
                Some(RowKey::new(keeper.as_str()).map_err(|_| corrupt("stored row key"))?)
            } else {
                None
            };
            changes.push(Change {
                table: table.id,
                row,
                action: ChangeAction::Tombstone,
                groups: Vec::new(),
                replaced_by,
            });
        }
    }
    Ok(changes)
}
