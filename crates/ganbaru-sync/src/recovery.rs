//! Recovery of edits that outlived a deletion.
//!
//! A deleted row is flagged for recovery while it retains a version of a recoverable group that
//! its deletion did not see: an edit made concurrently with the deletion. What a deletion saw is
//! its coverage. A tombstone without a cause covers the context of its operation; a tombstone
//! naming a cause covers what the cause's deletion covers, because a repair tombstone sealed
//! later may have seen edits that the cause's deletion did not. A live row hidden by cascade
//! from a deleted root is checked against the root's coverage, and a removed derived-key row
//! covers the context of its winning removal.
//!
//! Only tombstoned rows carry the flag. A live row hidden by cascade holding an unseen edit
//! flags the tombstoned root it is grouped under instead. Rows are grouped under a root: a
//! row's own deletion when it has a tombstone without a cause, otherwise the root its causes
//! or its cascade reach. A group is offered while a member is flagged and the group still
//! holds an unseen edit. Restoring creates new rows from the retained values of the root and
//! its descendants; restoring or discarding then seals deletions that cover every retained
//! version of the root and of each row in the group holding an unseen edit, which closes the
//! offer on every replica.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ganbaru_sync_contracts::{
    Change, ChangeAction, GroupId, Hlc, RowKey, TableId, Value, VersionVector, WriterId,
};
use sqlx::{Connection, Row, SqliteConnection};

use crate::adapter::Presentation;
use crate::apply::finish;
use crate::error::{SyncResult, corrupt, failpoint};
use crate::manifest::TableSpec;
use crate::merge::{self, Batch, RowState, Version, table_param};
use crate::repair;
use crate::seal::{SealReport, SealSlot, existence_write};
use crate::visibility::{MAX_REDIRECT_DEPTH, RowRef, Visibility, removed};
use crate::{Engine, LocalWriter, SpaceContext, SyncError, log, sql};

/// A deleted row, with its deleted descendants, holding edits the deletion did not see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryEntry {
    /// Table of the row.
    pub table: TableId,
    /// Row key.
    pub row_key: String,
    /// Title and preview from the retained values.
    pub presentation: Presentation,
    /// Deleted descendants that restoring the row brings back with it.
    pub descendants: usize,
    /// Writer of the newest unseen edit in the row or its descendants.
    pub writer: WriterId,
    /// Device of that writer.
    pub device_id: String,
    /// Clock of that edit.
    pub clock: Hlc,
}

/// A deleted descendant of a row offered for recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryDescendant {
    /// Table of the row.
    pub table: TableId,
    /// Row key.
    pub row_key: String,
    /// Winning retained value per group.
    pub values: Vec<(GroupId, Value)>,
}

/// One tombstone of a row: its operation and the row it names as its cause.
struct Tombstone {
    writer: WriterId,
    seq: u64,
    cause: Option<RowRef>,
}

/// Winning retained value per group.
fn retained_values(
    engine: &Engine,
    table: &TableSpec,
    versions: &[Version],
) -> Vec<(GroupId, Value)> {
    merge::winners(engine, table, versions)
        .into_iter()
        .map(|(group, version)| (group, version.value.clone()))
        .collect()
}

fn table_spec(engine: &Engine, table: TableId) -> SyncResult<&'static TableSpec> {
    engine
        .manifest()
        .table(table)
        .ok_or_else(|| corrupt("recovery row of an unknown table"))
}

fn stored_table(table: i64) -> SyncResult<TableId> {
    u16::try_from(table)
        .map(TableId)
        .map_err(|_| corrupt("table id out of range"))
}

async fn tombstones(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
) -> SyncResult<Vec<Tombstone>> {
    let rows = sqlx::query(
        "SELECT writer_id, seq, cause_table, cause_row FROM sync_tombstones
         WHERE table_id = ? AND row_key = ?",
    )
    .bind(table_param(table))
    .bind(key)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(|row| {
            let writer: Vec<u8> = row.try_get(0)?;
            let cause_table: Option<i64> = row.try_get(2)?;
            let cause_row: Option<String> = row.try_get(3)?;
            Ok(Tombstone {
                writer: log::writer_id(&writer)?,
                seq: log::stored_u64(row.try_get(1)?, "tombstone sequence")?,
                cause: match (cause_table, cause_row) {
                    (Some(table), Some(row)) => Some((stored_table(table)?, row)),
                    _ => None,
                },
            })
        })
        .collect()
}

/// The context of an operation, advanced past the operation itself.
async fn operation_context(
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    writer: WriterId,
    seq: u64,
) -> SyncResult<VersionVector> {
    let mut context = batch
        .contexts
        .get(conn, batch.space, writer, seq)
        .await?
        .clone();
    context.advance(writer, seq);
    Ok(context)
}

/// The context of the winning removal of a removed derived-key row.
async fn removal_context(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    table: &TableSpec,
    key: &str,
) -> SyncResult<Option<VersionVector>> {
    let Some(exists) = table.exists_group() else {
        return Ok(None);
    };
    let versions = merge::versions(conn, table.id, key).await?;
    let winner = merge::winner(
        engine,
        table,
        exists,
        versions.iter().filter(|version| version.group == exists.id),
    );
    match winner {
        Some(version) if merge::is_removal(Some(&version.value)) => Ok(Some(
            operation_context(conn, batch, version.writer, version.seq).await?,
        )),
        _ => Ok(None),
    }
}

/// Whether a row named as a cause is deleted: tombstoned, removed, or hidden by cascade from
/// a deleted root.
async fn deleted(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &mut Visibility,
    (table, key): &RowRef,
) -> SyncResult<bool> {
    Ok(match merge::row_state(conn, *table, key).await? {
        None => false,
        Some(RowState::Tombstoned) => true,
        Some(RowState::Live) => {
            removed(engine, conn, *table, key).await?
                || visibility
                    .cascade_roots(engine, conn, *table, key)
                    .await?
                    .cause()
                    .is_some()
        }
    })
}

/// The contexts that decide which retained versions of a deleted row its deletion saw. A
/// tombstone whose cause is no longer deleted covers its own context.
pub(crate) async fn coverage(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    visibility: &mut Visibility,
    table: TableId,
    key: &str,
) -> SyncResult<Vec<VersionVector>> {
    let start = (table, key.to_string());
    let mut visited = BTreeSet::from([start.clone()]);
    let mut pending = vec![start];
    let mut contexts = Vec::new();
    while let Some((table, key)) = pending.pop() {
        match merge::row_state(conn, table, &key).await? {
            None => {}
            Some(RowState::Tombstoned) => {
                for tombstone in tombstones(conn, table, &key).await? {
                    match tombstone.cause {
                        Some(cause) if deleted(engine, conn, visibility, &cause).await? => {
                            if visited.insert(cause.clone()) {
                                pending.push(cause);
                            }
                        }
                        _ => contexts.push(
                            operation_context(conn, batch, tombstone.writer, tombstone.seq).await?,
                        ),
                    }
                }
            }
            Some(RowState::Live) => {
                let spec = table_spec(engine, table)?;
                if let Some(context) = removal_context(engine, conn, batch, spec, &key).await? {
                    contexts.push(context);
                }
                let roots = visibility.cascade_roots(engine, conn, table, &key).await?;
                if let Some(root) = roots.cause() {
                    let root = (root.table, root.key.clone());
                    if visited.insert(root.clone()) {
                        pending.push(root);
                    }
                }
            }
        }
    }
    Ok(contexts)
}

/// Whether a version is covered by none of the contexts.
fn uncovered(version: &Version, covering: &[VersionVector]) -> bool {
    !covering
        .iter()
        .any(|context| context.covers(&version.writer, version.seq))
}

/// Retained versions of recoverable groups that the coverage does not cover.
fn unseen<'v>(
    table: &TableSpec,
    versions: &'v [Version],
    covering: &[VersionVector],
) -> impl Iterator<Item = &'v Version> {
    versions.iter().filter(move |version| {
        table
            .group(version.group)
            .is_some_and(|group| group.recoverable)
            && uncovered(version, covering)
    })
}

/// The recovery flag of a tombstoned row: it, or a live descendant grouped under it, holds a
/// recoverable version its coverage does not cover. Live rows carry no flag; their root
/// carries it for them.
pub(crate) async fn flagged(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    visibility: &mut Visibility,
    table: TableId,
    key: &str,
) -> SyncResult<bool> {
    let row = (table, key.to_string());
    if newest_unseen(engine, conn, batch, visibility, &row)
        .await?
        .is_some()
    {
        return Ok(true);
    }
    for descendant in descendants(engine, conn, visibility, &row).await? {
        if merge::row_state(conn, descendant.0, &descendant.1).await? == Some(RowState::Live)
            && newest_unseen(engine, conn, batch, visibility, &descendant)
                .await?
                .is_some()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The tombstoned root whose flag a live row hidden by cascade from a deleted root feeds.
pub(crate) async fn flag_root(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &mut Visibility,
    table: TableId,
    key: &str,
) -> SyncResult<Option<RowRef>> {
    let roots = visibility.cascade_roots(engine, conn, table, key).await?;
    if roots.cause().is_none() {
        return Ok(None);
    }
    let root = group_root(engine, conn, visibility, (table, key.to_string())).await?;
    Ok(
        (merge::row_state(conn, root.0, &root.1).await? == Some(RowState::Tombstoned))
            .then_some(root),
    )
}

/// The root a deleted row is grouped under for recovery: itself when it has a tombstone
/// without a cause, otherwise the lowest cause of its tombstones or the cause of its cascade,
/// followed until a row has no further cause.
pub(crate) async fn group_root(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &mut Visibility,
    row: RowRef,
) -> SyncResult<RowRef> {
    let mut current = row;
    for _ in 0..MAX_REDIRECT_DEPTH {
        let (table, key) = &current;
        let next = match merge::row_state(conn, *table, key).await? {
            None => None,
            Some(RowState::Tombstoned) => {
                let tombstones = tombstones(conn, *table, key).await?;
                if tombstones.iter().any(|tombstone| tombstone.cause.is_none()) {
                    None
                } else {
                    tombstones
                        .into_iter()
                        .filter_map(|tombstone| tombstone.cause)
                        .min()
                }
            }
            Some(RowState::Live) => visibility
                .cascade_roots(engine, conn, *table, key)
                .await?
                .cause()
                .map(|root| (root.table, root.key.clone())),
        };
        match next {
            Some(next) if next != current => current = next,
            _ => break,
        }
    }
    Ok(current)
}

/// The root, its flagged members, and its descendants, each once.
fn group_rows(root: &RowRef, members: &[RowRef], descendants: &[RowRef]) -> BTreeSet<RowRef> {
    std::iter::once(root)
        .chain(members)
        .chain(descendants)
        .cloned()
        .collect()
}

/// Clock and writer of the newest recoverable version of a deleted row that its coverage does
/// not cover.
async fn newest_unseen(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    visibility: &mut Visibility,
    (table, key): &RowRef,
) -> SyncResult<Option<(Hlc, WriterId)>> {
    let spec = table_spec(engine, *table)?;
    let versions = merge::versions(conn, *table, key).await?;
    let covering = coverage(engine, conn, batch, visibility, *table, key).await?;
    Ok(unseen(spec, &versions, &covering)
        .map(|version| (version.clock, version.writer))
        .max())
}

/// Visibility for reads outside an apply, with every hidden duplicate.
async fn read_visibility(engine: &Engine, conn: &mut SqliteConnection) -> SyncResult<Visibility> {
    let duplicates = repair::duplicates(engine, conn, &mut BTreeSet::new()).await?;
    Ok(Visibility::with_duplicates(duplicates))
}

/// Flagged rows grouped by their root.
async fn recovery_groups(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &mut Visibility,
) -> SyncResult<BTreeMap<RowRef, Vec<RowRef>>> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT table_id, row_key FROM sync_rows WHERE recovery = 1 ORDER BY table_id, row_key",
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut groups: BTreeMap<RowRef, Vec<RowRef>> = BTreeMap::new();
    for (table, key) in rows {
        let row = (stored_table(table)?, key);
        let root = group_root(engine, conn, visibility, row.clone()).await?;
        groups.entry(root).or_default().push(row);
    }
    Ok(groups)
}

/// Deleted rows grouped under a root, nearest first: the rows whose references, followed
/// through other members, reach it and whose own group root it is.
async fn descendants(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &mut Visibility,
    root: &RowRef,
) -> SyncResult<Vec<RowRef>> {
    let mut seen = BTreeSet::from([root.clone()]);
    let mut queue = std::collections::VecDeque::from([root.clone()]);
    let mut found = Vec::new();
    while let Some((table, key)) = queue.pop_front() {
        for referrer in repair::referrers(engine, conn, table, &key).await? {
            if !seen.insert(referrer.clone())
                || !deleted(engine, conn, visibility, &referrer).await?
                || group_root(engine, conn, visibility, referrer.clone()).await? != *root
            {
                continue;
            }
            found.push(referrer.clone());
            queue.push_back(referrer);
        }
    }
    Ok(found)
}

impl Engine {
    /// Rows offered for recovery, by table and key, each with its descendants.
    pub async fn recovery_entries(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<Vec<RecoveryEntry>> {
        let flagged: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_rows WHERE recovery = 1")
            .fetch_one(&mut *conn)
            .await?;
        if flagged == 0 {
            return Ok(Vec::new());
        }
        let devices: HashMap<WriterId, String> = log::writers(conn, ctx.space)
            .await?
            .into_iter()
            .map(|writer| (writer.id, writer.device_id))
            .collect();
        let mut batch = Batch::load(conn, ctx.space).await?;
        let mut visibility = read_visibility(self, conn).await?;
        let groups = recovery_groups(self, conn, &mut visibility).await?;
        let mut entries = Vec::with_capacity(groups.len());
        for (root, members) in groups {
            let descendants = descendants(self, conn, &mut visibility, &root).await?;
            let mut newest: Option<(Hlc, WriterId)> = None;
            for row in group_rows(&root, &members, &descendants) {
                newest =
                    newest.max(newest_unseen(self, conn, &mut batch, &mut visibility, &row).await?);
            }
            // A flag left by a descendant that is no longer hidden offers nothing.
            let Some((clock, writer)) = newest else {
                continue;
            };
            let spec = table_spec(self, root.0)?;
            let versions = merge::versions(conn, root.0, &root.1).await?;
            let values = retained_values(self, spec, &versions);
            let presentation = self
                .adapter(spec)
                .map(|adapter| adapter.presentation(spec.id, &values))
                .unwrap_or_default();
            let descendants = descendants.len();
            entries.push(RecoveryEntry {
                table: root.0,
                row_key: root.1,
                presentation,
                descendants,
                writer,
                device_id: devices
                    .get(&writer)
                    .cloned()
                    .ok_or_else(|| corrupt("version of an unknown writer"))?,
                clock,
            });
        }
        Ok(entries)
    }

    /// Winning retained value of every group of a row offered for recovery, or `None` when the
    /// row is not offered.
    pub async fn recovery_values(
        &self,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<Option<Vec<(GroupId, Value)>>> {
        let Some(spec) = self.manifest().table(table) else {
            return Err(SyncError::InvalidRequest("unknown table"));
        };
        let mut visibility = read_visibility(self, conn).await?;
        let root = (table, key.to_string());
        if !recovery_groups(self, conn, &mut visibility)
            .await?
            .contains_key(&root)
        {
            return Ok(None);
        }
        let versions = merge::versions(conn, table, key).await?;
        Ok(Some(retained_values(self, spec, &versions)))
    }

    /// Deleted descendants of a row offered for recovery with their retained values, nearest
    /// first, or `None` when the row is not offered. Restoring the row restores them too.
    pub async fn recovery_descendants(
        &self,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<Option<Vec<RecoveryDescendant>>> {
        if self.manifest().table(table).is_none() {
            return Err(SyncError::InvalidRequest("unknown table"));
        }
        let mut visibility = read_visibility(self, conn).await?;
        let root = (table, key.to_string());
        if !recovery_groups(self, conn, &mut visibility)
            .await?
            .contains_key(&root)
        {
            return Ok(None);
        }
        let mut found = Vec::new();
        for (table, key) in descendants(self, conn, &mut visibility, &root).await? {
            let spec = table_spec(self, table)?;
            let versions = merge::versions(conn, table, &key).await?;
            found.push(RecoveryDescendant {
                table,
                values: retained_values(self, spec, &versions),
                row_key: key,
            });
        }
        Ok(Some(found))
    }

    /// Closes the recovery offer of a row after the caller restored it as a new row or chose
    /// to discard it: seals pending captures, then a deletion of the row and of each member or
    /// descendant holding an unseen edit, covering every retained version: a tombstone, or a
    /// removal for a derived-key row.
    pub async fn close_recovery(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        table: TableId,
        key: &str,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<SealReport> {
        if self.manifest().table(table).is_none() {
            return Err(SyncError::InvalidRequest("unknown table"));
        }
        RowKey::new(key).map_err(|_| SyncError::InvalidRequest("row key"))?;
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        let root = (table, key.to_string());
        let mut visibility = read_visibility(self, &mut tx).await?;
        let Some(members) = recovery_groups(self, &mut tx, &mut visibility)
            .await?
            .remove(&root)
        else {
            return Err(SyncError::InvalidRequest("row is not offered for recovery"));
        };
        let descendants = descendants(self, &mut tx, &mut visibility, &root).await?;
        sql::begin_engine_write(&mut tx).await?;
        let mut batch = Batch::load(&mut tx, ctx.space).await?;
        let mut targets = Vec::new();
        for row in group_rows(&root, &members, &descendants) {
            if row == root
                || newest_unseen(self, &mut tx, &mut batch, &mut visibility, &row)
                    .await?
                    .is_some()
            {
                targets.push(row);
            }
        }
        let mut slot = SealSlot::new(Some(writer));
        let (sealer, writer) = slot
            .open(self, &mut tx, ctx, &mut batch, now_ms)
            .await?
            .ok_or_else(|| corrupt("sealer slot has no writer"))?;
        let mut report = sealer
            .seal_captures(self, &mut tx, ctx, &mut batch, writer, now_ms)
            .await?;
        let mut changes = Vec::with_capacity(targets.len());
        for (table, key) in targets {
            let spec = table_spec(self, table)?;
            let live = merge::row_state(&mut tx, table, &key).await? == Some(RowState::Live);
            let row = RowKey::new(key).map_err(|_| corrupt("stored row key"))?;
            changes.push(match spec.exists_group() {
                Some(exists) if live => existence_write(spec, &row, exists.id, false),
                _ => Change {
                    table,
                    row,
                    action: ChangeAction::Tombstone,
                    groups: Vec::new(),
                    replaced_by: None,
                    cause: None,
                },
            });
        }
        sealer
            .seal_changes(
                self,
                &mut tx,
                ctx,
                &mut batch,
                writer,
                changes,
                None,
                now_ms,
                &mut report,
            )
            .await?;
        report.changed_tables = finish(
            self,
            &mut tx,
            ctx,
            &mut batch,
            &mut slot,
            now_ms,
            &mut report,
        )
        .await?;
        failpoint!("recovery.before_commit");
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(report)
    }
}
