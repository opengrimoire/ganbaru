//! Late changes: what a removed device's excluded operations did to rows, folded commutatively
//! from the excluded bodies, and the admin decisions that restore or discard them.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::{
    Change, ChangeAction, Content, Envelope, GroupId, GroupMask, GroupValue, Hlc, LateResolution,
    ResolveLate, RowKey, Seq, SpaceId, TableId, Value, WriterId,
};
use sqlx::{Connection, Row, SqliteConnection};

use crate::adapter::Presentation;
use crate::apply::{Constrained, finish_sealed};
use crate::authorization;
use crate::error::{SyncResult, corrupt};
use crate::log::{self, seq_i64, stored_u64};
use crate::manifest::{MergeKind, TableSpec};
use crate::merge::{self, Batch, RowState, group_id, mask_param, table_param};
use crate::seal::{SealReport, SealSlot};
use crate::{Engine, LocalWriter, SpaceContext, SyncError, sql, store, validate};

use super::note_local_device;

/// What a removed device's late operations did to a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LateKind {
    /// Wrote values of an existing row, or created a row an operation that still counts also
    /// created.
    Values,
    /// Deleted the row.
    Tombstone,
    /// Created a row no operation that still counts created, which stays hidden until an admin
    /// restores it.
    Created,
}

impl LateKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Values => "values",
            Self::Tombstone => "tombstone",
            Self::Created => "created",
        }
    }

    fn parse(value: &str) -> SyncResult<Self> {
        match value {
            "values" => Ok(Self::Values),
            "tombstone" => Ok(Self::Tombstone),
            "created" => Ok(Self::Created),
            other => Err(corrupt(format!("unknown late change {other}"))),
        }
    }
}

fn resolution_text(resolution: LateResolution) -> &'static str {
    match resolution {
        LateResolution::Restore => "restore",
        LateResolution::Discard => "discard",
    }
}

/// The latest late value of one group: by clock, then writer, then sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LateValue {
    pub(crate) clock: Hlc,
    pub(crate) writer: WriterId,
    pub(crate) seq: Seq,
    pub(crate) value: Value,
}

impl LateValue {
    fn order(&self) -> (Hlc, WriterId, Seq) {
        (self.clock, self.writer, self.seq)
    }
}

pub(crate) type LateGroups = BTreeMap<GroupId, LateValue>;

/// Encodes late group values: per group its id, clock, writer, sequence, value length, and
/// value.
fn encode_groups(groups: &LateGroups) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (group, late) in groups {
        let value = late.value.encode();
        bytes.push(group.get());
        bytes.extend_from_slice(&late.clock.as_u64().to_be_bytes());
        bytes.extend_from_slice(late.writer.as_bytes());
        bytes.extend_from_slice(&late.seq.to_be_bytes());
        bytes.extend_from_slice(&u32::try_from(value.len()).unwrap_or(u32::MAX).to_be_bytes());
        bytes.extend_from_slice(&value);
    }
    bytes
}

fn take<'b>(bytes: &mut &'b [u8], count: usize) -> SyncResult<&'b [u8]> {
    if bytes.len() < count {
        return Err(corrupt("late change groups are truncated"));
    }
    let (head, rest) = bytes.split_at(count);
    *bytes = rest;
    Ok(head)
}

fn take_u64(bytes: &mut &[u8]) -> SyncResult<u64> {
    let head = take(bytes, 8)?;
    Ok(u64::from_be_bytes(head.try_into().map_err(|_| {
        corrupt("late change groups are truncated")
    })?))
}

pub(crate) fn decode_groups(mut bytes: &[u8]) -> SyncResult<LateGroups> {
    let mut groups = LateGroups::new();
    while !bytes.is_empty() {
        let group = group_id(i64::from(take(&mut bytes, 1)?[0]))?;
        let clock = Hlc::from_u64(take_u64(&mut bytes)?);
        let writer = log::writer_id(take(&mut bytes, 16)?)?;
        let seq = take_u64(&mut bytes)?;
        let length = take(&mut bytes, 4)?;
        let length = u32::from_be_bytes(
            length
                .try_into()
                .map_err(|_| corrupt("late change groups are truncated"))?,
        );
        let value = take(
            &mut bytes,
            usize::try_from(length).map_err(|_| corrupt("late value length"))?,
        )?;
        let value = Value::decode(value).map_err(|_| corrupt("late value does not decode"))?;
        groups.insert(
            group,
            LateValue {
                clock,
                writer,
                seq,
                value,
            },
        );
    }
    Ok(groups)
}

/// Whether a create of a row of `spec` creates it for exclusion: a row with its own key does,
/// and a derived-key row does not, since any replica may create that row on its own.
pub(super) fn tracks_creation(spec: &TableSpec) -> bool {
    spec.derived_key.is_none()
}

/// What one change of an excluded operation leaves: a create of a row with its own key
/// creates it, and a create of a derived-key row only writes values, since any replica may
/// create that row on its own.
fn late_kind(engine: &Engine, change: &Change) -> Option<LateKind> {
    let spec = engine.manifest().table(change.table)?;
    Some(match change.action {
        ChangeAction::Create if tracks_creation(spec) => LateKind::Created,
        ChangeAction::Create | ChangeAction::Write => LateKind::Values,
        ChangeAction::Tombstone => LateKind::Tombstone,
    })
}

/// Folds the changes of the excluded operation `(writer, seq)` into the late changes of
/// `removal`. A row keeps the strongest kind (created, then tombstone, then values), the union
/// of the groups written, and the latest value per group; a decision on the row stays. Callers
/// skip an operation classified as invalid, which leaves no late change on any replica. An
/// operation of a newer manifest stays, since this version cannot judge it and nothing folds it
/// again after an update.
pub(crate) async fn fold(
    engine: &Engine,
    conn: &mut SqliteConnection,
    removal: u64,
    writer: WriterId,
    seq: Seq,
    clock: Hlc,
    content: &Content,
) -> SyncResult<()> {
    let Content::Changes(changes) = content else {
        return Ok(());
    };
    for change in changes {
        let Some(kind) = late_kind(engine, change) else {
            continue;
        };
        let existing: Option<(String, i64, Option<Vec<u8>>)> = sqlx::query_as(
            "SELECT change, mask, groups FROM sync_late_changes
             WHERE removal_revision = ? AND table_id = ? AND row_key = ?",
        )
        .bind(seq_i64(removal)?)
        .bind(table_param(change.table))
        .bind(change.row.as_str())
        .fetch_optional(&mut *conn)
        .await?;
        let (kind, mut mask, mut groups) = match existing {
            Some((stored, mask, groups)) => (
                LateKind::parse(&stored)?.max(kind),
                GroupMask(mask as u64),
                groups
                    .as_deref()
                    .map(decode_groups)
                    .transpose()?
                    .unwrap_or_default(),
            ),
            None => (kind, GroupMask::EMPTY, LateGroups::new()),
        };
        for group in &change.groups {
            mask.insert(group.group);
            let late = LateValue {
                clock,
                writer,
                seq,
                value: group.value.clone(),
            };
            match groups.get(&group.group) {
                Some(current) if current.order() >= late.order() => {}
                _ => {
                    groups.insert(group.group, late);
                }
            }
        }
        sqlx::query(
            "INSERT INTO sync_late_changes (removal_revision, table_id, row_key, change, mask, groups)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT (removal_revision, table_id, row_key) DO UPDATE SET
                change = excluded.change, mask = excluded.mask, groups = excluded.groups",
        )
        .bind(seq_i64(removal)?)
        .bind(table_param(change.table))
        .bind(change.row.as_str())
        .bind(kind.as_str())
        .bind(mask_param(mask))
        .bind((!groups.is_empty()).then(|| encode_groups(&groups)))
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Lists the rows whose late changes [`fold`] changed outside an engine write as stale, since
/// a late change can hide a row already materialized. See [`take_stale`].
pub(crate) async fn mark_stale(
    engine: &Engine,
    conn: &mut SqliteConnection,
    content: &Content,
) -> SyncResult<()> {
    let Content::Changes(changes) = content else {
        return Ok(());
    };
    for change in changes {
        if late_kind(engine, change).is_none() {
            continue;
        }
        sqlx::query(
            "INSERT INTO sync_stale_rows (table_id, row_key) VALUES (?, ?)
             ON CONFLICT (table_id, row_key) DO NOTHING",
        )
        .bind(table_param(change.table))
        .bind(change.row.as_str())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Lists `rows` as stale, so an engine write after the open chain closes materializes and
/// repairs them; see [`crate::apply`] for chains.
pub(crate) async fn defer_stale(
    conn: &mut SqliteConnection,
    rows: &BTreeSet<(TableId, String)>,
) -> SyncResult<()> {
    for (table, key) in rows {
        sqlx::query(
            "INSERT INTO sync_stale_rows (table_id, row_key) VALUES (?, ?)
             ON CONFLICT (table_id, row_key) DO NOTHING",
        )
        .bind(table_param(*table))
        .bind(key.as_str())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Moves at most `limit` of the rows [`mark_stale`] and [`defer_stale`] listed into `dirty`,
/// first in table and key order, so the engine write materializes them again. Returns whether
/// listed rows remain for a later engine write.
pub(crate) async fn take_stale(
    conn: &mut SqliteConnection,
    dirty: &mut BTreeSet<(TableId, String)>,
    limit: usize,
) -> SyncResult<bool> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "DELETE FROM sync_stale_rows WHERE (table_id, row_key) IN (
            SELECT table_id, row_key FROM sync_stale_rows ORDER BY table_id, row_key LIMIT ?
         )
         RETURNING table_id, row_key",
    )
    .bind(i64::try_from(limit).unwrap_or(i64::MAX))
    .fetch_all(&mut *conn)
    .await?;
    for (table, key) in rows {
        let table = u16::try_from(table)
            .map(TableId)
            .map_err(|_| corrupt("stale row of a table id out of range"))?;
        dirty.insert((table, key));
    }
    stale_rows_listed(conn).await
}

/// Whether [`mark_stale`] or [`defer_stale`] listed rows no engine write took back yet.
pub(crate) async fn stale_rows_listed(conn: &mut SqliteConnection) -> SyncResult<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM sync_stale_rows)")
            .fetch_one(&mut *conn)
            .await?,
    )
}

/// Late change state of a row: per removal, its kind and decision.
type LateState = BTreeMap<(TableId, String), Vec<(i64, String, Option<String>)>>;

async fn late_state(conn: &mut SqliteConnection) -> SyncResult<LateState> {
    let rows: Vec<(i64, i64, String, String, Option<String>)> = sqlx::query_as(
        "SELECT removal_revision, table_id, row_key, change, resolution FROM sync_late_changes
         ORDER BY table_id, row_key, removal_revision",
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut state = LateState::new();
    for (removal, table, key, change, resolution) in rows {
        let table = u16::try_from(table)
            .map(TableId)
            .map_err(|_| corrupt("late change of a table id out of range"))?;
        state
            .entry((table, key))
            .or_default()
            .push((removal, change, resolution));
    }
    Ok(state)
}

/// An excluded operation's writer, sequence, removal revision, envelope, and writer device.
type ExcludedOp = (Vec<u8>, i64, i64, Vec<u8>, String);

/// Derives every late change again from the excluded operations and the decisions of the
/// selected chain, and marks dirty each row whose late state changed. Removals the chain no
/// longer selects lose their late changes with their exclusions.
pub(crate) async fn rebuild_late(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    resolutions: &[(u64, ResolveLate)],
) -> SyncResult<()> {
    let before = late_state(conn).await?;
    sqlx::query("DELETE FROM sync_late_changes")
        .execute(&mut *conn)
        .await?;
    let rows: Vec<ExcludedOp> = sqlx::query_as(
        "SELECT o.writer_id, o.seq, o.excluded_revision, o.envelope, w.device_id
         FROM sync_ops o JOIN sync_writers w
            ON w.space_id = o.space_id AND w.writer_id = o.writer_id
         WHERE o.space_id = ? AND o.excluded_revision IS NOT NULL
         ORDER BY o.writer_id, o.seq",
    )
    .bind(batch.space.as_bytes().as_slice())
    .fetch_all(&mut *conn)
    .await?;
    for (writer, seq, removal, envelope, device) in rows {
        let Ok(envelope) = Envelope::decode(&envelope) else {
            continue;
        };
        if store::classify(engine, &envelope, &device).hold() == Some(store::HoldReason::Invalid) {
            continue;
        }
        let Ok(operation) = envelope.operation() else {
            continue;
        };
        fold(
            engine,
            conn,
            stored_u64(removal, "removal revision")?,
            log::writer_id(&writer)?,
            stored_u64(seq, "sequence")?,
            operation.header.clock,
            &operation.content,
        )
        .await?;
    }
    for (revision, resolve) in resolutions {
        if *revision <= resolve.removal_revision {
            continue;
        }
        for row in &resolve.rows {
            decide(
                conn,
                *revision,
                resolve.removal_revision,
                row.table,
                row.row.as_str(),
                row.resolution,
            )
            .await?;
        }
    }
    crate::fork::drop_restated(conn).await?;
    let after = late_state(conn).await?;
    for row in before.keys().chain(after.keys()) {
        if before.get(row) != after.get(row) {
            batch.dirty.insert(row.clone());
        }
    }
    Ok(())
}

/// Records the decision of revision `revision` on a late change, keeping a row for a change
/// whose operations are not stored yet. The first decision on a row stays, so deciding it again
/// changes nothing.
async fn decide(
    conn: &mut SqliteConnection,
    revision: u64,
    removal: u64,
    table: TableId,
    key: &str,
    resolution: LateResolution,
) -> SyncResult<()> {
    let updated = sqlx::query(
        "UPDATE sync_late_changes SET resolution = ?, resolved_revision = ?
         WHERE removal_revision = ? AND table_id = ? AND row_key = ? AND resolution IS NULL",
    )
    .bind(resolution_text(resolution))
    .bind(seq_i64(revision)?)
    .bind(seq_i64(removal)?)
    .bind(table_param(table))
    .bind(key)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if updated == 0 {
        sqlx::query(
            "INSERT INTO sync_late_changes
                (removal_revision, table_id, row_key, change, mask, groups, resolved_revision,
                 resolution)
             VALUES (?, ?, ?, 'values', 0, NULL, ?, ?)
             ON CONFLICT (removal_revision, table_id, row_key) DO NOTHING",
        )
        .bind(seq_i64(removal)?)
        .bind(table_param(table))
        .bind(key)
        .bind(seq_i64(revision)?)
        .bind(resolution_text(resolution))
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Whether any late change is recorded, so visibility can skip the per-row check otherwise.
pub(crate) async fn any_late(conn: &mut SqliteConnection) -> SyncResult<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM sync_late_changes)")
            .fetch_one(&mut *conn)
            .await?,
    )
}

/// Whether a live row is hidden as a late change: a removed device created it past its cutoff,
/// no operation that still counts created it, and no admin restored it, or excluding a removed
/// device's operations left it without a value for a group. A late create of a row an honest
/// operation created never hides it, since the removed device could otherwise hide any row
/// whose key it knew.
pub(crate) async fn hides(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
) -> SyncResult<bool> {
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT change, resolution FROM sync_late_changes WHERE table_id = ? AND row_key = ?",
    )
    .bind(table_param(table))
    .bind(key)
    .fetch_all(&mut *conn)
    .await?;
    if rows.is_empty() {
        return Ok(false);
    }
    let created = rows.iter().any(|(change, resolution)| {
        change == "created" && resolution.as_deref() != Some("restore")
    });
    if created && !has_creation(conn, table, key).await? {
        return Ok(true);
    }
    let Some(spec) = engine.manifest().table(table) else {
        return Err(corrupt("late change of an unknown table"));
    };
    let groups: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT group_id FROM sync_register_versions WHERE table_id = ? AND row_key = ?",
    )
    .bind(table_param(table))
    .bind(key)
    .fetch_all(&mut *conn)
    .await?;
    Ok(spec.groups.iter().any(|group| {
        !spec.is_exists_group(group.id) && !groups.contains(&i64::from(group.id.get()))
    }))
}

/// Whether a row no published state knows was created by a removed device's late operation,
/// so an honest operation that saw it may still write it.
pub(crate) async fn created_late(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
) -> SyncResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM sync_late_changes
            WHERE table_id = ? AND row_key = ? AND change = 'created'
         )",
    )
    .bind(table_param(table))
    .bind(key)
    .fetch_one(&mut *conn)
    .await?)
}

/// Whether an operation that still counts created this row. Effects of excluded operations
/// left `sync_row_creations` when they were removed, and a replica that never applied them
/// has none, so the answer does not depend on arrival order.
pub(crate) async fn has_creation(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
) -> SyncResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sync_row_creations WHERE table_id = ? AND row_key = ?)",
    )
    .bind(table_param(table))
    .bind(key)
    .fetch_one(&mut *conn)
    .await?)
}

/// The kind a late change has now: a late create of a row an operation that still counts also
/// created acts as late values. The stored kind stays as folded, so it does not depend on
/// arrival order, and this answer follows the replica's creations as they change.
async fn effective_kind(conn: &mut SqliteConnection, record: &LateRecord) -> SyncResult<LateKind> {
    if record.kind == LateKind::Created && has_creation(conn, record.table, &record.key).await? {
        return Ok(LateKind::Values);
    }
    Ok(record.kind)
}

/// The clock of the operation carrying the selected entry of `revision`, read from the record
/// store, which holds it on every replica that selected it. `None` when this replica selected
/// no such entry or its envelope does not decode.
pub(crate) async fn entry_clock(
    conn: &mut SqliteConnection,
    space: SpaceId,
    revision: u64,
) -> SyncResult<Option<Hlc>> {
    let envelope: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT e.envelope FROM sync_auth_selected s
         JOIN sync_auth_envelopes e
            ON e.space_id = s.space_id AND e.envelope_hash = s.envelope_hash AND e.role = 'record'
         WHERE s.space_id = ? AND s.revision = ?
         LIMIT 1",
    )
    .bind(space.as_bytes().as_slice())
    .bind(seq_i64(revision)?)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(envelope.and_then(|bytes| {
        Envelope::decode(&bytes)
            .ok()?
            .header_fields()
            .ok()
            .map(|header| header.clock)
    }))
}

/// One stored late change.
#[derive(Debug, Clone)]
pub(crate) struct LateRecord {
    pub(crate) removal: u64,
    pub(crate) table: TableId,
    pub(crate) key: String,
    pub(crate) kind: LateKind,
    pub(crate) groups: LateGroups,
    pub(crate) resolution: Option<LateResolution>,
}

/// Late changes, all or one row's, in removal, table, and key order.
pub(crate) async fn records(
    conn: &mut SqliteConnection,
    row: Option<(u64, TableId, &str)>,
) -> SyncResult<Vec<LateRecord>> {
    let mut sql = String::from(
        "SELECT removal_revision, table_id, row_key, change, groups, resolution
         FROM sync_late_changes",
    );
    if row.is_some() {
        sql.push_str(" WHERE removal_revision = ? AND table_id = ? AND row_key = ?");
    }
    sql.push_str(" ORDER BY removal_revision, table_id, row_key");
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));
    if let Some((removal, table, key)) = row {
        query = query
            .bind(seq_i64(removal)?)
            .bind(table_param(table))
            .bind(key);
    }
    let rows = query.fetch_all(&mut *conn).await?;
    rows.iter()
        .map(|row| {
            let change: String = row.try_get(3)?;
            let groups: Option<Vec<u8>> = row.try_get(4)?;
            let resolution: Option<String> = row.try_get(5)?;
            Ok(LateRecord {
                removal: stored_u64(row.try_get(0)?, "removal revision")?,
                table: u16::try_from(row.try_get::<i64, _>(1)?)
                    .map(TableId)
                    .map_err(|_| corrupt("late change of a table id out of range"))?,
                key: row.try_get(2)?,
                kind: LateKind::parse(&change)?,
                groups: groups
                    .as_deref()
                    .map(decode_groups)
                    .transpose()?
                    .unwrap_or_default(),
                resolution: match resolution.as_deref() {
                    None => None,
                    Some("restore") => Some(LateResolution::Restore),
                    Some("discard") => Some(LateResolution::Discard),
                    Some(other) => return Err(corrupt(format!("unknown late resolution {other}"))),
                },
            })
        })
        .collect()
}

/// A row a removed device changed after its removal cutoff, waiting for an admin to restore or
/// discard the change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LateChange {
    /// Revision of the removal that excluded the change.
    pub removal_revision: u64,
    /// Device the removal removed.
    pub device_id: String,
    /// Table of the row.
    pub table: TableId,
    /// Row key.
    pub row_key: String,
    /// What the excluded operations did to the row, with a late create of a row an operation
    /// that still counts created listed as values.
    pub kind: LateKind,
    /// Title and preview from the late values.
    pub presentation: Presentation,
    /// Latest late value per group written.
    pub values: Vec<(GroupId, Value)>,
    /// Clock of the newest late value, or `None` when the change only deleted the row.
    pub clock: Option<Hlc>,
    /// Whether restoring the change can write it back. A row the removed device owns only that
    /// device may write, so restoring it records the decision and writes nothing, and the
    /// listing offers only discarding it.
    pub restorable: bool,
}

/// The change restoring a late change seals: the late values no current version is newer than,
/// as a create of every group when only the removed device created the row or no published
/// state knows it, or a deletion of a live row. A write leaves out Immutable groups, which the
/// row already holds, unless it is a derived-key re-add of every group. A late value counts at
/// most at the clock of the removal entry, since the removed device signed it alone: a forged
/// clock ahead of the removal cannot beat an honest edit from before the removal was seen. A
/// row another device owns seals nothing, since only its owner can write it: the restore
/// decision is all that is recorded. [`admit_restoring_change`] judges the result, since values
/// that were valid in separate operations can compose into a change that is not.
async fn restoring_change(
    engine: &Engine,
    conn: &mut SqliteConnection,
    space: SpaceId,
    record: &LateRecord,
    sealing_device: &str,
) -> SyncResult<Option<Change>> {
    let Some(spec) = engine.manifest().table(record.table) else {
        return Err(corrupt("late change of an unknown table"));
    };
    if !validate::device_owns_row(spec, &record.key, sealing_device) {
        return Ok(None);
    }
    let row = RowKey::new(record.key.clone()).map_err(|_| corrupt("stored row key"))?;
    let state = merge::row_state(conn, record.table, &record.key).await?;
    let kind = effective_kind(conn, record).await?;
    if kind == LateKind::Tombstone {
        return Ok((state == Some(RowState::Live)).then(|| Change {
            table: record.table,
            row,
            action: ChangeAction::Tombstone,
            groups: Vec::new(),
            replaced_by: None,
            cause: None,
        }));
    }
    let versions = merge::versions(engine, conn, record.table, &record.key).await?;
    let restated = crate::fork::restated(conn, record.table, &record.key).await?;
    // Without the removal entry no late clock is trusted, so every honest version wins.
    let removal_clock = entry_clock(conn, space, record.removal)
        .await?
        .unwrap_or(Hlc::from_u64(0));
    // A version restating an older value this replica sealed again is no newer edit. Only the
    // replica that sealed a restatement, the anchor or a former anchor, records it, so on another
    // admin a restatement still counts as newer and the group keeps the restated value.
    let newer = |group: GroupId, late: &LateValue| {
        versions.iter().any(|version| {
            version.group == group
                && version.clock > late.clock.min(removal_clock)
                && restated.get(&group) != Some(&version.hash)
        })
    };
    // A late create comes back as a create, which writes every group: a group an honest
    // operation wrote after the late value keeps that honest value.
    if kind == LateKind::Created {
        let winners = merge::winners(engine, spec, &versions);
        let mut groups = Vec::with_capacity(spec.groups.len());
        for group in spec.groups {
            let value = match (record.groups.get(&group.id), winners.get(&group.id)) {
                (Some(late), _) if !newer(group.id, late) => late.value.clone(),
                (_, Some(winner)) => winner.value.clone(),
                (Some(late), None) => late.value.clone(),
                (None, None) => return Err(corrupt("a late create lacks a group value")),
            };
            groups.push(GroupValue::new(spec.id, group.id, value));
        }
        return Ok(Some(Change {
            table: record.table,
            row,
            action: ChangeAction::Create,
            groups,
            replaced_by: None,
            cause: None,
        }));
    }
    let mut groups = record
        .groups
        .iter()
        .filter(|(group, late)| !newer(**group, late))
        .map(|(group, late)| GroupValue::new(spec.id, *group, late.value.clone()))
        .collect::<Vec<_>>();
    let action = match state {
        None => ChangeAction::Create,
        Some(_) => ChangeAction::Write,
    };
    let readd = spec.derived_key.is_some() && groups.len() == spec.groups.len();
    if action == ChangeAction::Write && !readd {
        groups.retain(|value| {
            spec.group(value.group)
                .is_some_and(|group| group.kind != MergeKind::Immutable)
        });
    }
    if groups.is_empty() {
        return Ok(None);
    }
    Ok(Some(Change {
        table: record.table,
        row,
        action,
        groups,
        replaced_by: None,
        cause: None,
    }))
}

/// Refuses a restoring change the manifest or its row owner would refuse. Sealing validates
/// nothing, so peers would hold such an operation as invalid and stall the sealing writer. The
/// listing stays, and an admin can discard it.
fn admit_restoring_change(
    engine: &Engine,
    change: &Change,
    sealing_device: &str,
) -> SyncResult<()> {
    let Some(spec) = engine.manifest().table(change.table) else {
        return Err(corrupt("late change of an unknown table"));
    };
    validate::change(engine, change)
        .and_then(|()| validate::writer_change(spec, change, sealing_device))
        .map_err(SyncError::InvalidRequest)
}

impl Engine {
    /// Late changes of the selected removals no admin decided yet, in removal, table, and key
    /// order.
    pub async fn late_changes(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<Vec<LateChange>> {
        log::ensure_space(conn, ctx.space).await?;
        let pending = records(conn, None)
            .await?
            .into_iter()
            .filter(|record| record.resolution.is_none())
            .collect::<Vec<_>>();
        if pending.is_empty() {
            return Ok(Vec::new());
        }
        let removed: BTreeMap<u64, String> = authorization::select(conn, ctx.space)
            .await?
            .map(|selection| selection.state.view().devices)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|device| Some((device.removed_revision?, device.device_id)))
            .collect();
        let mut changes = Vec::with_capacity(pending.len());
        for record in pending {
            let Some(spec) = self.manifest().table(record.table) else {
                return Err(corrupt("late change of an unknown table"));
            };
            let values = record
                .groups
                .iter()
                .map(|(group, late)| (*group, late.value.clone()))
                .collect::<Vec<_>>();
            let presentation = self
                .adapter(spec)
                .map(|adapter| adapter.presentation(spec.id, &values))
                .unwrap_or_default();
            let kind = effective_kind(conn, &record).await?;
            changes.push(LateChange {
                removal_revision: record.removal,
                device_id: removed
                    .get(&record.removal)
                    .cloned()
                    .ok_or_else(|| corrupt("late change of an unknown removal"))?,
                table: record.table,
                row_key: record.key,
                kind,
                presentation,
                clock: record.groups.values().map(|late| late.clock).max(),
                values,
                // Every replica holds a write to an owned row by another device as invalid,
                // so a late change to one is the removed device's own row.
                restorable: spec.owner.is_none(),
            });
        }
        Ok(changes)
    }

    /// Applies a late change [`Engine::late_changes`] listed as an ordinary write of the local
    /// writer, before an admin sequences `resolve_late` restoring it: the late values no
    /// current version is newer than, or the deletion of a live row. A change already decided,
    /// or one with nothing left to write, seals no change.
    ///
    /// # Errors
    ///
    /// Returns [`SyncError::InvalidRequest`] with nothing sealed when the change to seal would
    /// not pass validation on a peer, which leaves the late change listed for a discard.
    pub async fn restore_late_change(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        late: &LateChange,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<SealReport> {
        if self.manifest().table(late.table).is_none() {
            return Err(SyncError::InvalidRequest("unknown table"));
        }
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        let key = Some((late.removal_revision, late.table, late.row_key.as_str()));
        let Some(record) = records(&mut tx, key).await?.pop() else {
            return Err(SyncError::InvalidRequest("row has no late change"));
        };
        if record.resolution.is_some() {
            return Ok(SealReport::default());
        }
        sql::begin_engine_write(&mut tx).await?;
        let mut batch = Batch::load(&mut tx, ctx.space).await?;
        note_local_device(
            self,
            &mut tx,
            ctx,
            &mut batch,
            &writer.certificate.certificate.device_id,
        )
        .await?;
        let change = restoring_change(
            self,
            &mut tx,
            ctx.space,
            &record,
            &writer.certificate.certificate.device_id,
        )
        .await?;
        if let Some(change) = &change {
            admit_restoring_change(self, change, &writer.certificate.certificate.device_id)?;
        }
        let mut slot = SealSlot::new(Some(writer));
        let (sealer, writer) = slot
            .open(self, &mut tx, ctx, &mut batch, now_ms)
            .await?
            .ok_or_else(|| corrupt("sealer slot has no writer"))?;
        let mut report = sealer
            .seal_captures(self, &mut tx, ctx, &mut batch, writer, now_ms)
            .await?;
        if let Some(change) = change {
            sealer
                .seal_changes(
                    self,
                    &mut tx,
                    ctx,
                    &mut batch,
                    writer,
                    vec![change],
                    None,
                    now_ms,
                    &mut report,
                )
                .await?;
        }
        let materialized = finish_sealed(
            self,
            &mut tx,
            ctx,
            &mut batch,
            &mut slot,
            Constrained::Seal,
            now_ms,
            &mut report,
        )
        .await?;
        report.record(materialized);
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(report)
    }
}
