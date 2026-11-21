//! Capture planning: reading `sync_capture`, turning captured rows into the changes a seal
//! publishes, cascading local deletions, holding invalid dependents, and forcing or clearing
//! captures.

use std::collections::{BTreeMap, BTreeSet};

#[cfg(doc)]
use ganbaru_sync_contracts::op::ends_group;
use ganbaru_sync_contracts::{
    Cause, Change, ChangeAction, Digest32, Field, GroupId, GroupMask, GroupValue,
    PENDING_KEY_PREFIX, RowKey, TableId, Value,
};
use sqlx::sqlite::SqliteRow;
use sqlx::{AssertSqlSafe, Row, SqliteConnection};

use crate::error::{SyncResult, corrupt};
use crate::log::{self, clamp_ms};
use crate::manifest::{MergeKind, TableSpec};
use crate::merge::{self, RowState, mask_param, table_param};
use crate::visibility::{RowRef, Visibility};
use crate::{Engine, validate};

use super::{DomainRow, InvalidRow, SealReport, change_dependencies, fits_alone, read_domain};

/// One row of `sync_capture`.
pub(super) struct Capture {
    table: i64,
    key: String,
    mask: GroupMask,
    /// Groups written even when their value equals the retained winner.
    forced: GroupMask,
    /// Whether the row was inserted locally since the last seal.
    created: bool,
    deleted: bool,
    /// Whether the capture is held for Undo, expired or not.
    held: bool,
    replaced_by: Option<String>,
    /// Sync state of the row when planning started.
    state: Option<RowState>,
}

/// Changes planned from captures and the captures they consume.
pub(super) struct PlannedCaptures {
    pub(super) changes: Vec<Change>,
    pub(super) consumed: Vec<(i64, String)>,
    /// Whether a held capture is among the captures.
    pub(super) held: bool,
}

/// Columns of a capture and the sync state of its row.
const CAPTURE_SELECT: &str = "SELECT c.table_id, c.row_key, c.mask, c.forced_mask, c.deleted, c.replaced_by, r.state, c.created, c.held_until_ms
     FROM sync_capture c
     LEFT JOIN sync_rows r ON r.table_id = c.table_id AND r.row_key = c.row_key";

fn capture_from_row(row: &SqliteRow) -> SyncResult<Capture> {
    let mask: i64 = row.try_get(2)?;
    let forced: i64 = row.try_get(3)?;
    Ok(Capture {
        table: row.try_get(0)?,
        key: row.try_get(1)?,
        mask: GroupMask((mask | forced) as u64),
        forced: GroupMask(forced as u64),
        created: row.try_get::<i64, _>(7)? != 0,
        deleted: row.try_get::<i64, _>(4)? != 0,
        held: row.try_get::<Option<i64>, _>(8)?.is_some(),
        replaced_by: row.try_get(5)?,
        state: merge::parse_row_state(row.try_get::<Option<&str>, _>(6)?)?,
    })
}

/// Every capture except those still held at `now_ms`, in table and key order.
pub(super) async fn pending_captures(
    conn: &mut SqliteConnection,
    now_ms: u64,
) -> SyncResult<Vec<Capture>> {
    let sql = format!(
        "{CAPTURE_SELECT}
         WHERE c.held_until_ms IS NULL OR c.held_until_ms <= ?
         ORDER BY c.table_id, c.row_key"
    );
    let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
        .bind(clamp_ms(now_ms))
        .fetch_all(&mut *conn)
        .await?;
    rows.iter().map(capture_from_row).collect()
}

/// The captures of `rows`, held ones included.
async fn row_captures(
    conn: &mut SqliteConnection,
    rows: &BTreeSet<RowRef>,
) -> SyncResult<Vec<Capture>> {
    let sql = format!("{CAPTURE_SELECT} WHERE c.table_id = ? AND c.row_key = ?");
    let mut captures = Vec::new();
    for (table, key) in rows {
        let row = sqlx::query(AssertSqlSafe(sql.as_str()))
            .bind(table_param(*table))
            .bind(key.as_str())
            .fetch_optional(&mut *conn)
            .await?;
        if let Some(row) = row {
            captures.push(capture_from_row(&row)?);
        }
    }
    Ok(captures)
}

/// Captured rows among `rows`.
pub(crate) async fn captured(
    conn: &mut SqliteConnection,
    rows: impl IntoIterator<Item = RowRef>,
) -> SyncResult<BTreeSet<RowRef>> {
    let any: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM sync_capture)")
        .fetch_one(&mut *conn)
        .await?;
    let mut found = BTreeSet::new();
    if !any {
        return Ok(found);
    }
    for (table, key) in rows {
        let hit: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM sync_capture WHERE table_id = ? AND row_key = ?")
                .bind(table_param(table))
                .bind(key.as_str())
                .fetch_optional(&mut *conn)
                .await?;
        if hit.is_some() {
            found.insert((table, key));
        }
    }
    Ok(found)
}

/// Captured rows of `tables` without a held delete. A row with a held delete stays live in
/// merge state, so repairs already account for it, and it seals early only when a change or
/// plan names it. A held insert is returned: merge state does not know the row, so it seals
/// early when a constraint over its table must read it.
pub(crate) async fn captured_in(
    conn: &mut SqliteConnection,
    tables: &BTreeSet<TableId>,
) -> SyncResult<BTreeSet<RowRef>> {
    let mut found = BTreeSet::new();
    for table in tables {
        let keys: Vec<String> = sqlx::query_scalar(
            "SELECT row_key FROM sync_capture
             WHERE table_id = ? AND (held_until_ms IS NULL OR deleted = 0)",
        )
        .bind(table_param(*table))
        .fetch_all(&mut *conn)
        .await?;
        found.extend(keys.into_iter().map(|key| (*table, key)));
    }
    Ok(found)
}

fn tombstone(table: &TableSpec, key: &RowKey, replaced_by: Option<&str>) -> Change {
    let target = replaced_by
        .filter(|_| table.redirects)
        .and_then(|target| RowKey::new(target).ok())
        .filter(|target| target < key);
    Change {
        table: table.id,
        row: key.clone(),
        action: ChangeAction::Tombstone,
        groups: Vec::new(),
        replaced_by: target,
        cause: None,
    }
}

/// A write of a derived-key row's existence register, which stands in for its tombstone.
pub(crate) fn existence_write(
    table: &TableSpec,
    key: &RowKey,
    exists: GroupId,
    present: bool,
) -> Change {
    Change {
        table: table.id,
        row: key.clone(),
        action: ChangeAction::Write,
        groups: vec![GroupValue::new(
            table.id,
            exists,
            Value::new(vec![Field::Integer(i64::from(present))])
                .expect("an existence value fits the bounds"),
        )],
        replaced_by: None,
        cause: None,
    }
}

/// The cause of each local deletion that a cascade from another deletion of the same seal
/// explains. Deleted rows are linked by their cascading references to other deleted rows; the
/// lowest row of each strongly connected component that links to no other component is a
/// root, so a reference cycle deleted together has one. Every other deleted row names the
/// lowest root it reaches.
async fn local_causes(
    engine: &Engine,
    conn: &mut SqliteConnection,
    deletions: &BTreeMap<RowRef, (usize, Change)>,
) -> SyncResult<BTreeMap<RowRef, RowRef>> {
    let mut visibility = Visibility::default();
    let rows: Vec<&RowRef> = deletions.keys().collect();
    let position: BTreeMap<&RowRef, usize> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| (*row, index))
        .collect();
    let mut adjacency = Vec::with_capacity(rows.len());
    for row in &rows {
        let table = engine
            .manifest()
            .table(row.0)
            .ok_or_else(|| corrupt("capture of an unknown table"))?;
        let targets: BTreeSet<usize> = visibility
            .cascade_targets(engine, conn, table, &row.1)
            .await?
            .into_iter()
            .filter_map(|target| position.get(&target.row).copied())
            .collect();
        adjacency.push(targets.into_iter().collect::<Vec<_>>());
    }
    Ok(sink_roots(&adjacency)
        .into_iter()
        .enumerate()
        .filter_map(|(index, root)| root.map(|root| (rows[index].clone(), rows[root].clone())))
        .collect())
}

/// For each node of a directed graph, the lowest node of a sink strongly connected component
/// that it reaches, a sink component being one with no edge to another component. The lowest
/// node of a sink component itself has none.
pub(crate) fn sink_roots(adjacency: &[Vec<usize>]) -> Vec<Option<usize>> {
    let count = adjacency.len();
    let unvisited = usize::MAX;
    let mut index = vec![unvisited; count];
    let mut low = vec![0; count];
    let mut on_stack = vec![false; count];
    let mut stack = Vec::new();
    let mut component = vec![0; count];
    // Components in the order Tarjan's algorithm completes them, which lists every component
    // after the components it reaches.
    let mut components: Vec<Vec<usize>> = Vec::new();
    let mut next = 0;
    for start in 0..count {
        if index[start] != unvisited {
            continue;
        }
        index[start] = next;
        low[start] = next;
        next += 1;
        stack.push(start);
        on_stack[start] = true;
        let mut frames = vec![(start, 0usize)];
        while let Some(frame) = frames.last_mut() {
            let (node, edge) = *frame;
            frame.1 += 1;
            if let Some(&target) = adjacency[node].get(edge) {
                if index[target] == unvisited {
                    index[target] = next;
                    low[target] = next;
                    next += 1;
                    stack.push(target);
                    on_stack[target] = true;
                    frames.push((target, 0));
                } else if on_stack[target] {
                    low[node] = low[node].min(index[target]);
                }
                continue;
            }
            frames.pop();
            if let Some(&(parent, _)) = frames.last() {
                low[parent] = low[parent].min(low[node]);
            }
            if low[node] == index[node] {
                let mut members = Vec::new();
                while let Some(member) = stack.pop() {
                    on_stack[member] = false;
                    component[member] = components.len();
                    members.push(member);
                    if member == node {
                        break;
                    }
                }
                components.push(members);
            }
        }
    }
    let mut best: Vec<Option<usize>> = Vec::with_capacity(components.len());
    for (id, members) in components.iter().enumerate() {
        let reached = members
            .iter()
            .flat_map(|member| adjacency[*member].iter())
            .map(|target| component[*target])
            .filter(|target| *target != id)
            .filter_map(|target| best[target])
            .min();
        let sink = members
            .iter()
            .flat_map(|member| adjacency[*member].iter())
            .all(|target| component[*target] == id);
        best.push(if sink {
            members.iter().min().copied()
        } else {
            reached
        });
    }
    (0..count)
        .map(|node| best[component[node]].filter(|root| *root != node))
        .collect()
}

/// Captured groups of a live row whose value hashes to their uncontested winner, so writing
/// them again changes nothing. A group in `forced` is never among them, so resolving a conflict
/// still writes.
async fn unchanged_groups(
    conn: &mut SqliteConnection,
    table: &TableSpec,
    key: &RowKey,
    forced: GroupMask,
    groups: &[GroupValue],
) -> SyncResult<BTreeSet<GroupId>> {
    if groups.is_empty() {
        return Ok(BTreeSet::new());
    }
    let rows: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT group_id, value_hash FROM sync_register_versions
         WHERE table_id = ? AND row_key = ?",
    )
    .bind(table_param(table.id))
    .bind(key.as_str())
    .fetch_all(&mut *conn)
    .await?;
    let mut retained: BTreeMap<GroupId, BTreeSet<Digest32>> = BTreeMap::new();
    for (group, hash) in rows {
        retained
            .entry(merge::group_id(group)?)
            .or_default()
            .insert(log::digest(&hash)?);
    }
    Ok(groups
        .iter()
        .filter(|value| {
            !forced.contains(value.group)
                && retained
                    .get(&value.group)
                    .is_some_and(|hashes| hashes.len() == 1 && hashes.contains(&value.hash))
        })
        .map(|value| value.group)
        .collect())
}

/// `groups` without those in `unchanged`. The existence group stays while another group is
/// written, and the change is dropped when nothing else remains and it is unchanged too.
fn without_unchanged(
    table: &TableSpec,
    groups: Vec<GroupValue>,
    unchanged: &BTreeSet<GroupId>,
) -> Vec<GroupValue> {
    let kept: Vec<GroupValue> = groups
        .into_iter()
        .filter(|value| table.is_exists_group(value.group) || !unchanged.contains(&value.group))
        .collect();
    if kept.iter().all(|value| unchanged.contains(&value.group)) {
        return Vec::new();
    }
    kept
}

/// Plans the changes of `captures`. A capture of a row `local_device` does not own is consumed
/// without a change, since no replica would accept it.
pub(super) async fn plan_captures(
    engine: &Engine,
    conn: &mut SqliteConnection,
    captures: Vec<Capture>,
    local_device: Option<&str>,
    report: &mut SealReport,
) -> SyncResult<PlannedCaptures> {
    let invalid_from = report.invalid_rows.len();
    let held = captures.iter().any(|capture| capture.held);
    let order: BTreeMap<TableId, usize> = engine
        .manifest()
        .tables
        .iter()
        .enumerate()
        .map(|(index, table)| (table.id, index))
        .collect();
    let mut creates = Vec::new();
    let mut writes = Vec::new();
    let mut tombstones = Vec::new();
    let mut consumed = Vec::new();
    // Local deletions of published rows, held back until their causes are known.
    let mut deletions: BTreeMap<RowRef, (usize, Change)> = BTreeMap::new();
    let mut visibility = Visibility::default();
    for capture in captures {
        let invalid = |reason: &'static str| InvalidRow {
            table: TableId(u16::try_from(capture.table).unwrap_or(0)),
            row_key: capture.key.clone(),
            reason,
        };
        let Some(table) = u16::try_from(capture.table)
            .ok()
            .and_then(|id| engine.manifest().table(TableId(id)))
        else {
            report.invalid_rows.push(invalid("unknown table"));
            continue;
        };
        if local_device
            .is_some_and(|device| !validate::device_owns_row(table, &capture.key, device))
        {
            consumed.push((capture.table, capture.key.clone()));
            report.foreign_owned_rows += 1;
            continue;
        }
        let Ok(key) = RowKey::new(capture.key.as_str()) else {
            report.invalid_rows.push(invalid("row key"));
            continue;
        };
        let domain = read_domain(engine, conn, table, key.as_str()).await?;
        let deletion = capture.deleted
            && capture.state == Some(RowState::Live)
            && matches!(domain, DomainRow::Absent);
        // A retained row stays in the domain under its tombstone and keeps publishing edits.
        let state = match (capture.state, &domain) {
            (Some(RowState::Tombstoned), DomainRow::Present(_))
                if visibility
                    .retained(engine, conn, table.id, key.as_str())
                    .await? =>
            {
                Some(RowState::Live)
            }
            (state, _) => state,
        };
        let change = match (state, domain) {
            (_, DomainRow::Invalid(reason)) => {
                report.invalid_rows.push(invalid(reason));
                continue;
            }
            (Some(RowState::Tombstoned), DomainRow::Present(_)) => {
                report
                    .invalid_rows
                    .push(invalid("tombstoned row is present"));
                continue;
            }
            // A placeholder key is never published; the row stays captured until it is rekeyed.
            (_, DomainRow::Present(_)) if key.as_str().starts_with(PENDING_KEY_PREFIX) => {
                report.invalid_rows.push(invalid("pending key"));
                continue;
            }
            (None | Some(RowState::Tombstoned), DomainRow::Absent) => None,
            (None, DomainRow::Present(groups)) => Some(Change {
                table: table.id,
                row: key.clone(),
                action: ChangeAction::Create,
                groups,
                replaced_by: None,
                cause: None,
            }),
            // An absent live row without a local delete is hidden by the merge; its repair is
            // sealed by the engine, not from the capture.
            (Some(RowState::Live), DomainRow::Absent) => {
                capture.deleted.then(|| match table.exists_group() {
                    Some(exists) => existence_write(table, &key, exists.id, false),
                    None => tombstone(table, &key, capture.replaced_by.as_deref()),
                })
            }
            // A derived-key row inserted again rewrites every group, unless every group equals
            // its uncontested winner.
            (Some(RowState::Live), DomainRow::Present(groups))
                if capture.created && table.derived_key.is_some() =>
            {
                let unchanged =
                    unchanged_groups(conn, table, &key, capture.forced, &groups).await?;
                (unchanged.len() < groups.len()).then(|| Change {
                    table: table.id,
                    row: key.clone(),
                    action: ChangeAction::Write,
                    groups,
                    replaced_by: None,
                    cause: None,
                })
            }
            (Some(RowState::Live), DomainRow::Present(groups)) => {
                let groups: Vec<GroupValue> = groups
                    .into_iter()
                    .filter(|value| {
                        (capture.mask.contains(value.group) || table.is_exists_group(value.group))
                            && table
                                .group(value.group)
                                .is_some_and(|group| group.kind != MergeKind::Immutable)
                    })
                    .collect();
                let unchanged =
                    unchanged_groups(conn, table, &key, capture.forced, &groups).await?;
                let groups = without_unchanged(table, groups, &unchanged);
                (!groups.is_empty()).then(|| Change {
                    table: table.id,
                    row: key.clone(),
                    action: ChangeAction::Write,
                    groups,
                    replaced_by: None,
                    cause: None,
                })
            }
        };
        if let Some(change) = &change {
            if let Err(reason) = validate::change(engine, change) {
                report.invalid_rows.push(invalid(reason));
                continue;
            }
            if !fits_alone(change) {
                report.invalid_rows.push(invalid("operation size"));
                continue;
            }
        }
        consumed.push((capture.table, capture.key.clone()));
        let position = order.get(&table.id).copied().unwrap_or(usize::MAX);
        match change {
            Some(change) if deletion => {
                deletions.insert((table.id, capture.key.clone()), (position, change));
            }
            Some(change) if change.action == ChangeAction::Create => {
                creates.push((position, change));
            }
            Some(change) if change.action == ChangeAction::Write => writes.push((position, change)),
            Some(change) => tombstones.push((usize::MAX - position, change)),
            None => {}
        }
    }
    let causes = local_causes(engine, conn, &deletions).await?;
    for (row, (position, mut change)) in deletions {
        match (causes.get(&row), change.action) {
            // A derived-key row deleted by cascade stays live in sync state, hidden by its
            // cause, so restoring the cause shows it again.
            (Some(_), ChangeAction::Write) => {}
            (Some(cause), ChangeAction::Tombstone) if change.replaced_by.is_none() => {
                change.cause = Some(Cause {
                    table: cause.0,
                    row: RowKey::new(cause.1.as_str()).map_err(|_| corrupt("stored row key"))?,
                });
                tombstones.push((usize::MAX - position, change));
            }
            (_, ChangeAction::Write) => writes.push((position, change)),
            _ => tombstones.push((usize::MAX - position, change)),
        }
    }
    creates.sort_by_key(|(position, _)| *position);
    writes.sort_by_key(|(position, _)| *position);
    tombstones.sort_by_key(|(position, _)| *position);
    let mut changes = creates
        .into_iter()
        .chain(writes)
        .chain(tombstones)
        .map(|(_, change)| change)
        .collect();
    hold_invalid_dependents(
        engine,
        conn,
        &mut changes,
        &mut consumed,
        report,
        invalid_from,
    )
    .await?;
    Ok(PlannedCaptures {
        changes,
        consumed,
        held,
    })
}

/// Holds back every planned change that depends on a row with no sync state whose capture was
/// found invalid from `report.invalid_rows[invalid_from..]`, and the changes depending on those
/// in turn. Sealing such a change would publish a reference to a row no device can receive.
/// A held change keeps its capture and is reported as invalid, so it seals once its dependency
/// is fixed.
async fn hold_invalid_dependents(
    engine: &Engine,
    conn: &mut SqliteConnection,
    changes: &mut Vec<Change>,
    consumed: &mut Vec<(i64, String)>,
    report: &mut SealReport,
    invalid_from: usize,
) -> SyncResult<()> {
    let mut invalid: BTreeSet<RowRef> = report.invalid_rows[invalid_from..]
        .iter()
        .map(|row| (row.table, row.row_key.clone()))
        .collect();
    if invalid.is_empty() {
        return Ok(());
    }
    loop {
        let mut held = Vec::new();
        for (index, change) in changes.iter().enumerate() {
            for row in change_dependencies(engine, change) {
                if invalid.contains(&row) && merge::row_state(conn, row.0, &row.1).await?.is_none()
                {
                    held.push(index);
                    break;
                }
            }
        }
        if held.is_empty() {
            return Ok(());
        }
        for index in held.into_iter().rev() {
            let change = changes.remove(index);
            let key = change.row.as_str().to_string();
            consumed.retain(|(table, row)| !(*table == i64::from(change.table.0) && *row == key));
            report.invalid_rows.push(InvalidRow {
                table: change.table,
                row_key: key.clone(),
                reason: "invalid dependency",
            });
            invalid.insert((change.table, key));
        }
    }
}

/// Plans the captures of `rows` together with the captured creates their changes depend on,
/// which have no sync state yet.
pub(super) async fn plan_rows(
    engine: &Engine,
    conn: &mut SqliteConnection,
    rows: &BTreeSet<RowRef>,
    local_device: Option<&str>,
    report: &mut SealReport,
) -> SyncResult<PlannedCaptures> {
    let mut rows = rows.clone();
    rows.extend(cascaded_deletions(engine, conn, &rows).await?);
    loop {
        let mut attempt = SealReport::default();
        let captures = row_captures(conn, &rows).await?;
        let planned = plan_captures(engine, conn, captures, local_device, &mut attempt).await?;
        let mut missing = BTreeSet::new();
        for change in &planned.changes {
            for row in change_dependencies(engine, change) {
                if !rows.contains(&row) && merge::row_state(conn, row.0, &row.1).await?.is_none() {
                    missing.insert(row);
                }
            }
        }
        let missing = captured(conn, missing).await?;
        if missing.is_empty() {
            report.invalid_rows.extend(attempt.invalid_rows);
            report.foreign_owned_rows += attempt.foreign_owned_rows;
            return Ok(planned);
        }
        rows.extend(missing);
    }
}

/// Captured deletions linked to a captured deletion among `rows` through cascading references,
/// in either direction, so a cascade seals whole and its tombstones keep their cause.
async fn cascaded_deletions(
    engine: &Engine,
    conn: &mut SqliteConnection,
    rows: &BTreeSet<RowRef>,
) -> SyncResult<BTreeSet<RowRef>> {
    let deleted: Vec<(i64, String)> =
        sqlx::query_as("SELECT table_id, row_key FROM sync_capture WHERE deleted = 1")
            .fetch_all(&mut *conn)
            .await?;
    let deleted: BTreeSet<RowRef> = deleted
        .into_iter()
        .map(|(table, key)| {
            u16::try_from(table)
                .map(|table| (TableId(table), key))
                .map_err(|_| corrupt("table id out of range"))
        })
        .collect::<SyncResult<_>>()?;
    let mut found: BTreeSet<RowRef> = rows.intersection(&deleted).cloned().collect();
    if found.is_empty() {
        return Ok(BTreeSet::new());
    }
    let mut visibility = Visibility::default();
    let mut edges: BTreeMap<RowRef, BTreeSet<RowRef>> = BTreeMap::new();
    for row in &deleted {
        let Some(table) = engine.manifest().table(row.0) else {
            continue;
        };
        if merge::row_state(conn, row.0, &row.1).await?.is_none() {
            continue;
        }
        for target in visibility
            .cascade_targets(engine, conn, table, &row.1)
            .await?
        {
            if deleted.contains(&target.row) {
                edges
                    .entry(row.clone())
                    .or_default()
                    .insert(target.row.clone());
                edges.entry(target.row).or_default().insert(row.clone());
            }
        }
    }
    let mut pending: Vec<RowRef> = found.iter().cloned().collect();
    while let Some(row) = pending.pop() {
        for next in edges.get(&row).into_iter().flatten() {
            if found.insert(next.clone()) {
                pending.push(next.clone());
            }
        }
    }
    Ok(found)
}

/// Records captures for a local row with `mask` added to its forced groups.
pub(crate) async fn force_capture(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
    mask: GroupMask,
    now_ms: u64,
) -> SyncResult<()> {
    sqlx::query(
        "INSERT INTO sync_capture
            (table_id, row_key, mask, forced_mask, created, deleted, replaced_by, captured_at_ms)
         VALUES (?, ?, ?, ?, 0, 0, NULL, ?)
         ON CONFLICT (table_id, row_key) DO UPDATE SET
            mask = mask | excluded.mask, forced_mask = forced_mask | excluded.forced_mask",
    )
    .bind(table_param(table))
    .bind(key)
    .bind(mask_param(mask))
    .bind(mask_param(mask))
    .bind(clamp_ms(now_ms))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Records a capture of `mask` on a local row without forcing it, as a capture trigger does for
/// a local edit: the next seal leaves out a group whose value equals its uncontested winner.
pub(crate) async fn capture_edit(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
    mask: GroupMask,
    now_ms: u64,
) -> SyncResult<()> {
    sqlx::query(
        "INSERT INTO sync_capture
            (table_id, row_key, mask, forced_mask, created, deleted, replaced_by, captured_at_ms)
         VALUES (?, ?, ?, 0, 0, 0, NULL, ?)
         ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask",
    )
    .bind(table_param(table))
    .bind(key)
    .bind(mask_param(mask))
    .bind(clamp_ms(now_ms))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Records a delete capture for a live row of a derived-key table that its domain table does
/// not hold, so the next seal writes its existence register as absent again.
pub(crate) async fn force_delete_capture(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
    exists: GroupMask,
    now_ms: u64,
) -> SyncResult<()> {
    sqlx::query(
        "INSERT INTO sync_capture
            (table_id, row_key, mask, forced_mask, created, deleted, replaced_by, captured_at_ms)
         VALUES (?, ?, ?, ?, 0, 1, NULL, ?)
         ON CONFLICT (table_id, row_key) DO UPDATE SET
            mask = mask | excluded.mask, forced_mask = forced_mask | excluded.forced_mask,
            deleted = 1",
    )
    .bind(table_param(table))
    .bind(key)
    .bind(mask_param(exists))
    .bind(mask_param(exists))
    .bind(clamp_ms(now_ms))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Captures deleted per statement after a seal.
const CAPTURE_DELETE_CHUNK: usize = 64;

/// Deletes consumed captures in chunks, keeping the full-chunk statement cached.
pub(super) async fn delete_captures(
    conn: &mut SqliteConnection,
    consumed: &[(i64, String)],
) -> SyncResult<()> {
    for chunk in consumed.chunks(CAPTURE_DELETE_CHUNK) {
        let rows = vec!["(?, ?)"; chunk.len()].join(", ");
        let sql = format!("DELETE FROM sync_capture WHERE (table_id, row_key) IN (VALUES {rows})");
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .persistent(chunk.len() == CAPTURE_DELETE_CHUNK);
        for (table, key) in chunk {
            query = query.bind(*table).bind(key.as_str());
        }
        query.execute(&mut *conn).await?;
    }
    Ok(())
}
