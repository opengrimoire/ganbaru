//! Removal exclusion, displaced versions, and late changes.
//!
//! A selected removal lists a cutoff per writer of the removed device: what the anchor had
//! stored when it sequenced the removal. Operations past it are excluded on every replica but
//! the removed device itself. They are still stored, up to [`excluded_limit`] per
//! writer, and forwarded, but held as invalid, and the merge effects of those already applied
//! are removed. The versions such an operation superseded come back from
//! `sync_displaced_versions`, which a member fills while the anchor has not confirmed storing
//! the superseding operation; the anchor never applies an operation past a cutoff, so it keeps
//! none. After a handover the cutoff can still fall below what an earlier anchor installation
//! stored and confirmed, which kept nothing either, so the anchor seals again the current value
//! of every live group an operation it excludes writes. The charter epoch has no earlier
//! anchor, and sealing again there would only bury late values an admin may still restore.
//! Operations of other writers stay applied: an honest operation that saw an excluded one
//! keeps its effects. The authorization entries excluded operations carry still count for chain
//! selection, so a competing chain wins on every replica whatever order it arrived in.
//!
//! Excluded operations leave late changes an admin restores or discards. They are derived from
//! the excluded bodies alone, folded commutatively, so every replica that stores the same
//! operations derives the same rows in any order.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::{
    Change, ChangeAction, Content, Envelope, GroupId, GroupMask, GroupValue, Hlc, LateResolution,
    OperationKind, ResolveLate, RowKey, Seq, SpaceId, TableId, Value, VersionVector, WriterId,
};
use sqlx::{Connection, Row, SqliteConnection};

use crate::adapter::Presentation;
use crate::apply::{Constrained, finish_sealed};
use crate::authorization::{self, Selection};
use crate::error::{SyncResult, corrupt};
use crate::log::{self, seq_i64, stored_u64};
use crate::merge::{self, Batch, RowState, group_id, mask_param, table_param};
use crate::seal::{SealReport, SealSlot};
use crate::{Engine, LocalWriter, SpaceContext, SyncError, reseal, sql, store};

/// Most operations past its removal cutoff a removed device's writer may still have stored.
/// Later ones are refused as revoked.
pub const MAX_EXCLUDED_PER_WRITER: u64 = 1024;

/// Most writers the removal does not name, with cutoff 0, that keep the excluded operations of a
/// removed device: the ones with the lowest writer ids, so every replica derives the same late
/// changes whatever order the writers arrived in. Each keeps up to [`MAX_EXCLUDED_PER_WRITER`]
/// operations. A surplus writer keeps only its genesis and the authorization entries contiguous
/// from it, a genesis above the lowest ids is refused as revoked, and a lower one displaces the
/// highest, so a removed device that mints writers stores a bounded amount anywhere.
pub const MAX_UNNAMED_WRITERS_PER_REMOVED_DEVICE: i64 = 2;

/// A static query with, between `$before` and `$after`, the SQL condition on the writer row
/// aliased `w` that holds for a surplus unnamed writer, see
/// [`MAX_UNNAMED_WRITERS_PER_REMOVED_DEVICE`]. The condition reads that bound from parameter
/// `?$bound`.
macro_rules! surplus_unnamed_query {
    ($before:literal, $bound:literal, $after:literal) => {
        concat!(
            $before,
            "(w.removal_cutoff_seq = 0 AND (
                SELECT count(*) FROM sync_writers u
                WHERE u.space_id = w.space_id AND u.device_id = w.device_id
                    AND u.removal_cutoff_seq = 0 AND u.writer_id < w.writer_id
             ) >= ?",
            $bound,
            ")",
            $after
        )
    };
}

/// Whether `writer` is a surplus unnamed writer of a removed device, see
/// [`MAX_UNNAMED_WRITERS_PER_REMOVED_DEVICE`].
pub(crate) async fn is_surplus_unnamed(
    conn: &mut SqliteConnection,
    space: SpaceId,
    writer: WriterId,
) -> SyncResult<bool> {
    let surplus: i64 = sqlx::query_scalar(surplus_unnamed_query!(
        "SELECT EXISTS (SELECT 1 FROM sync_writers w
         WHERE w.space_id = ?1 AND w.writer_id = ?2 AND ",
        3,
        ")"
    ))
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(MAX_UNNAMED_WRITERS_PER_REMOVED_DEVICE)
    .fetch_one(&mut *conn)
    .await?;
    Ok(surplus != 0)
}

/// Highest sequence a surplus unnamed writer may have stored outside authorization entries: its
/// genesis.
pub(crate) const SURPLUS_LIMIT: Seq = 1;

/// Highest sequence of an authorization entry a surplus unnamed writer may have stored,
/// contiguous from its genesis.
pub(crate) const SURPLUS_ENTRY_LIMIT: Seq = SURPLUS_LIMIT + MAX_EXCLUDED_ENTRIES_PER_WRITER;

/// Most authorization entries past [`excluded_limit`] a removed device's writer may still have
/// stored, contiguous from it. On a chain the replica did not select, the device may be an
/// anchor whose entries veto the selected chain, so they are kept past the bound on excluded
/// operations.
pub const MAX_EXCLUDED_ENTRIES_PER_WRITER: u64 = 1024;

/// Highest sequence a removed device's writer with removal cutoff `cutoff` may have stored.
pub(crate) const fn excluded_limit(cutoff: Seq) -> Seq {
    cutoff.saturating_add(MAX_EXCLUDED_PER_WRITER)
}

/// Highest sequence of an authorization entry a removed device's writer with removal cutoff
/// `cutoff` may have stored, see [`MAX_EXCLUDED_ENTRIES_PER_WRITER`].
pub(crate) const fn entry_limit(cutoff: Seq) -> Seq {
    excluded_limit(cutoff).saturating_add(MAX_EXCLUDED_ENTRIES_PER_WRITER)
}

/// Revision of the selected removal of `device_id`.
pub(crate) async fn removal_revision(
    conn: &mut SqliteConnection,
    space: SpaceId,
    device_id: &str,
) -> SyncResult<u64> {
    let revision: Option<Option<i64>> = sqlx::query_scalar(
        "SELECT removed_revision FROM sync_devices WHERE space_id = ? AND device_id = ?",
    )
    .bind(space.as_bytes().as_slice())
    .bind(device_id)
    .fetch_optional(&mut *conn)
    .await?;
    let revision = revision
        .flatten()
        .ok_or_else(|| corrupt("a writer with a removal cutoff has no selected removal"))?;
    stored_u64(revision, "removal revision")
}

/// Device of the local writer, once an engine call that seals recorded it.
pub(crate) async fn local_device(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<Option<String>> {
    let device: Option<Option<String>> =
        sqlx::query_scalar("SELECT local_device_id FROM sync_spaces WHERE space_id = ?")
            .bind(space.as_bytes().as_slice())
            .fetch_optional(&mut *conn)
            .await?;
    Ok(device.flatten())
}

/// Records the device of the local writer. Returns whether it changed, which changes what this
/// replica excludes.
pub(crate) async fn record_local_device(
    conn: &mut SqliteConnection,
    space: SpaceId,
    device_id: &str,
) -> SyncResult<bool> {
    let changed = sqlx::query(
        "UPDATE sync_spaces SET local_device_id = ?1
         WHERE space_id = ?2 AND local_device_id IS NOT ?1",
    )
    .bind(device_id)
    .bind(space.as_bytes().as_slice())
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if changed > 0 {
        crate::fork::unsettle(conn, space).await?;
    }
    Ok(changed > 0)
}

/// A writer some of whose operations the selected state excludes on this replica.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Excluding {
    /// Removal cutoff and stored sequence, when the writer's device was removed and is not the
    /// local one.
    pub(crate) removal: Option<(Seq, Seq)>,
    /// Last sequence the copies of a forked writer share.
    pub(crate) fork: Option<Seq>,
}

/// Writers some of whose operations are excluded on this replica: every writer of a removed
/// device but the local one, and every forked writer.
pub(crate) async fn excluding(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<BTreeMap<WriterId, Excluding>> {
    let rows: Vec<(Vec<u8>, i64, i64)> = sqlx::query_as(
        "SELECT w.writer_id, w.removal_cutoff_seq, w.stored_seq
         FROM sync_writers w JOIN sync_spaces s ON s.space_id = w.space_id
         WHERE w.space_id = ? AND w.removal_cutoff_seq IS NOT NULL
            AND (s.local_device_id IS NULL OR w.device_id <> s.local_device_id)",
    )
    .bind(space.as_bytes().as_slice())
    .fetch_all(&mut *conn)
    .await?;
    let mut excluding = BTreeMap::new();
    for (writer, cutoff, stored) in &rows {
        excluding.insert(
            log::writer_id(writer)?,
            Excluding {
                removal: Some((
                    stored_u64(*cutoff, "removal cutoff")?,
                    stored_u64(*stored, "stored sequence")?,
                )),
                fork: None,
            },
        );
    }
    for (writer, keep) in crate::fork::shared_prefixes(conn, space).await? {
        excluding
            .entry(writer)
            .or_insert_with(Excluding::default)
            .fork = Some(keep);
    }
    Ok(excluding)
}

/// Whether an operation with causal context `context` may apply after `applied`.
///
/// A context entry `(W, s)` is satisfied when `W` is applied through `s`, or when the selected
/// state excludes `(W, s)` and `W` is applied through what stays:
///
/// - Fork: `s` is past the last sequence the copies of the forked writer `W` share, and `W` is
///   applied through that sequence. No replica stores either branch once it holds the evidence.
/// - Removal: `s` is past the removal cutoff of `W`, `W` is applied through the cutoff, and the
///   excluded operation is stored, or `s` is past the [`excluded_limit`] of the
///   cutoff, where no replica stores it. Storing the excluded operations first lets the late
///   changes they leave know the rows they created.
///
/// The excluded operations never apply, so waiting for them would hold an honest operation
/// forever. A fork clamps `s` to the shared prefix before the removal rule applies.
pub(crate) fn ready(
    applied: &VersionVector,
    context: &VersionVector,
    excluding: &BTreeMap<WriterId, Excluding>,
) -> bool {
    if excluding.is_empty() {
        return applied.dominates(context);
    }
    context.iter().all(|(writer, seq)| {
        let Some(excluded) = excluding.get(&writer) else {
            return applied.get(&writer) >= seq;
        };
        let seq = excluded.fork.map_or(seq, |keep| seq.min(keep));
        match excluded.removal {
            Some((cutoff, stored)) if seq > cutoff => {
                let beyond = seq > excluded_limit(cutoff);
                applied.get(&writer) >= cutoff && (stored >= seq || beyond)
            }
            _ => applied.get(&writer) >= seq,
        }
    })
}

/// Whether `context` covers an operation past the [`excluded_limit`] of the
/// removal cutoff of an excluded writer. No replica keeps such an operation, so a row only it
/// created is unknown everywhere, and an honest change to that row is dropped on every replica
/// alike instead of holding its writer.
pub(crate) fn beyond_bound(
    context: &VersionVector,
    excluding: &BTreeMap<WriterId, Excluding>,
) -> bool {
    !excluding.is_empty()
        && context.iter().any(|(writer, seq)| {
            excluding
                .get(&writer)
                .and_then(|excluded| excluded.removal)
                .is_some_and(|(cutoff, _)| seq > excluded_limit(cutoff))
        })
}

/// Whether `context` covers an operation past the shared prefix of a forked writer. No replica
/// but the forked device keeps such an operation, so a row only it created is unknown, and an
/// honest change to that row applies to a row that waits, hidden, for a creation: the forked
/// device's re-sealed branch creates it again on every replica, whichever arrives first.
pub(crate) fn past_fork(
    context: &VersionVector,
    excluding: &BTreeMap<WriterId, Excluding>,
) -> bool {
    !excluding.is_empty()
        && context.iter().any(|(writer, seq)| {
            excluding
                .get(&writer)
                .and_then(|excluded| excluded.fork)
                .is_some_and(|keep| seq > keep)
        })
}

/// Brings operation states in line with the selected removals, inside an engine write.
///
/// Operations past a removal cutoff become held as invalid with the removal's revision; the
/// merge effects of applied ones are removed, their writer's applied sequence falls back to
/// the cutoff, and the versions they superseded are restored. Operations no selected removal
/// excludes any more are classified again and wait to apply. Returns whether any operation
/// changed.
pub(crate) async fn reconcile(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
) -> SyncResult<bool> {
    let local = local_device(conn, ctx.space).await?;
    let rows: Vec<(Vec<u8>, i64, String, i64, i64)> = sqlx::query_as(
        "SELECT o.writer_id, o.seq, o.state, d.removed_revision, w.removal_cutoff_seq
         FROM sync_writers w
         JOIN sync_devices d ON d.space_id = w.space_id AND d.device_id = w.device_id
         JOIN sync_ops o ON o.space_id = w.space_id AND o.writer_id = w.writer_id
            AND o.seq > w.removal_cutoff_seq
         WHERE w.space_id = ?1 AND w.removal_cutoff_seq IS NOT NULL
            AND d.removed_revision IS NOT NULL
            AND (?2 IS NULL OR w.device_id <> ?2)
            AND o.excluded_revision IS NOT d.removed_revision
         ORDER BY o.writer_id, o.seq",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(local.as_deref())
    .fetch_all(&mut *conn)
    .await?;
    // Writers with newly excluded operations: their cutoff, removal revision, and whether any
    // of them is applied.
    let mut writers: BTreeMap<WriterId, (Seq, u64, bool)> = BTreeMap::new();
    for (writer, _, state, revision, cutoff) in &rows {
        let entry = writers.entry(log::writer_id(writer)?).or_insert((
            stored_u64(*cutoff, "removal cutoff")?,
            stored_u64(*revision, "removal revision")?,
            false,
        ));
        entry.2 |= state == "applied";
    }
    // An earlier anchor may have stored these operations, which released what they displaced;
    // this replica may be that anchor, the current one or a former one.
    let reseals = !writers.is_empty() && crate::fork::reseals_excluded(conn, ctx.space).await?;
    for (writer, (cutoff, _, _)) in &writers {
        if reseals {
            crate::fork::collect_republish(engine, conn, ctx.space, batch, *writer, *cutoff)
                .await?;
        } else {
            crate::fork::collect_released(engine, conn, ctx.space, batch, *writer, *cutoff).await?;
        }
    }
    for (writer, (cutoff, revision, _)) in &writers {
        sqlx::query(
            "UPDATE sync_ops SET state = 'held', hold_reason = 'invalid', excluded_revision = ?1
             WHERE space_id = ?2 AND writer_id = ?3 AND seq > ?4
                AND excluded_revision IS NOT ?1",
        )
        .bind(seq_i64(*revision)?)
        .bind(ctx.space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(*cutoff)?)
        .execute(&mut *conn)
        .await?;
    }
    let removed: Vec<(WriterId, Seq)> = writers
        .iter()
        .filter(|(_, (_, _, applied))| *applied)
        .map(|(writer, (cutoff, _, _))| (*writer, *cutoff))
        .collect();
    for (writer, cutoff) in &removed {
        reseal::remove_effects(conn, batch, *writer, *cutoff).await?;
        batch.applied.set(*writer, *cutoff);
        sqlx::query(
            "UPDATE sync_writers SET applied_seq = min(applied_seq, ?)
             WHERE space_id = ? AND writer_id = ?",
        )
        .bind(seq_i64(*cutoff)?)
        .bind(ctx.space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .execute(&mut *conn)
        .await?;
    }
    for (writer, cutoff) in &removed {
        restore_displaced(engine, conn, batch, *writer, *cutoff).await?;
    }
    if !removed.is_empty() {
        drop_orphan_rows(conn, batch).await?;
    }
    let released = release(engine, conn, ctx, local.as_deref()).await?;
    Ok(!writers.is_empty() || released)
}

/// Drops the merge rows of dirty rows left without versions, tombstones, and creations: rows
/// only excluded operations created, which a replica that never applied them does not hold
/// either. Each dropped row is marked orphaned so it materializes absent. A row an applied
/// operation created stays even without versions, which happens when excluded operations
/// displaced every version it had and nothing retained them; the anchor publishes it again.
pub(crate) async fn drop_orphan_rows(
    conn: &mut SqliteConnection,
    batch: &mut Batch,
) -> SyncResult<()> {
    let mut orphaned = Vec::new();
    for (table, key) in &batch.dirty {
        let dropped = sqlx::query(
            "DELETE FROM sync_rows WHERE table_id = ?1 AND row_key = ?2
                AND NOT EXISTS (
                    SELECT 1 FROM sync_register_versions WHERE table_id = ?1 AND row_key = ?2
                )
                AND NOT EXISTS (
                    SELECT 1 FROM sync_tombstones WHERE table_id = ?1 AND row_key = ?2
                )
                AND NOT EXISTS (
                    SELECT 1 FROM sync_row_creations WHERE table_id = ?1 AND row_key = ?2
                )",
        )
        .bind(table_param(*table))
        .bind(key)
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if dropped > 0 {
            orphaned.push((*table, key.clone()));
        }
    }
    batch.orphaned.extend(orphaned);
    Ok(())
}

/// Classifies again every excluded operation no selected removal excludes any more. Returns
/// whether there was one.
async fn release(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    local: Option<&str>,
) -> SyncResult<bool> {
    let rows: Vec<(Vec<u8>, i64, Vec<u8>)> = sqlx::query_as(
        "SELECT o.writer_id, o.seq, o.envelope
         FROM sync_ops o JOIN sync_writers w
            ON w.space_id = o.space_id AND w.writer_id = o.writer_id
         WHERE o.space_id = ?1 AND o.excluded_revision IS NOT NULL
            AND (w.removal_cutoff_seq IS NULL OR o.seq <= w.removal_cutoff_seq
                OR w.device_id = ?2)
         ORDER BY o.writer_id, o.seq",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(local)
    .fetch_all(&mut *conn)
    .await?;
    for (writer, seq, envelope) in &rows {
        let hold = match Envelope::decode(envelope) {
            Ok(envelope) => store::classify(engine, &envelope).hold(),
            Err(_) => Some(store::HoldReason::Invalid),
        };
        sqlx::query(
            "UPDATE sync_ops SET excluded_revision = NULL, state = ?, hold_reason = ?
             WHERE space_id = ? AND writer_id = ? AND seq = ?",
        )
        .bind(if hold.is_some() { "held" } else { "waiting" })
        .bind(hold.map(store::HoldReason::as_str))
        .bind(ctx.space.as_bytes().as_slice())
        .bind(writer.as_slice())
        .bind(seq)
        .execute(&mut *conn)
        .await?;
    }
    Ok(!rows.is_empty())
}

/// Restores the versions the operations of `writer` past `cutoff` superseded, unless the
/// superseded version's own operation is excluded, its writer has a later version of the group,
/// or a remaining version of the group saw it. Displaced rows of those operations, and those of
/// versions the operations wrote, are dropped.
pub(crate) async fn restore_displaced(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    writer: WriterId,
    cutoff: Seq,
) -> SyncResult<()> {
    let rows = sqlx::query(
        "SELECT d.table_id, d.row_key, d.group_id, d.writer_id, d.seq, d.clock, d.value_hash,
            d.value, o.excluded_revision IS NOT NULL
         FROM sync_displaced_versions d
         LEFT JOIN sync_ops o ON o.space_id = ?1 AND o.writer_id = d.writer_id AND o.seq = d.seq
         WHERE d.displaced_by_writer = ?2 AND d.displaced_by_seq > ?3
         ORDER BY d.table_id, d.row_key, d.group_id, d.writer_id, d.seq DESC",
    )
    .bind(batch.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(cutoff)?)
    .fetch_all(&mut *conn)
    .await?;
    for statement in [
        "DELETE FROM sync_displaced_versions WHERE displaced_by_writer = ? AND displaced_by_seq > ?",
        "DELETE FROM sync_displaced_versions WHERE writer_id = ? AND seq > ?",
    ] {
        sqlx::query(statement)
            .bind(writer.as_bytes().as_slice())
            .bind(seq_i64(cutoff)?)
            .execute(&mut *conn)
            .await?;
    }
    for row in &rows {
        let excluded: bool = row.try_get(8)?;
        if excluded {
            continue;
        }
        let table = u16::try_from(row.try_get::<i64, _>(0)?)
            .map(TableId)
            .map_err(|_| corrupt("displaced version of a table id out of range"))?;
        let Some(spec) = engine.manifest().table(table) else {
            return Err(corrupt("displaced version of an unknown table"));
        };
        let key: String = row.try_get(1)?;
        let stored = merge::stored_from_row(row, 2)?;
        if merge::row_state(conn, table, &key).await?.is_none() {
            continue;
        }
        let existing: Vec<(Vec<u8>, i64)> = sqlx::query_as(
            "SELECT writer_id, seq FROM sync_register_versions
             WHERE table_id = ? AND row_key = ? AND group_id = ?",
        )
        .bind(table_param(table))
        .bind(&key)
        .bind(i64::from(stored.group.get()))
        .fetch_all(&mut *conn)
        .await?;
        let mut superseded = false;
        for (other, other_seq) in &existing {
            let other = log::writer_id(other)?;
            let other_seq = stored_u64(*other_seq, "version sequence")?;
            if other == stored.writer
                || batch
                    .contexts
                    .get(conn, batch.space, other, other_seq)
                    .await?
                    .covers(&stored.writer, stored.seq)
            {
                superseded = true;
                break;
            }
        }
        if superseded {
            continue;
        }
        let value = stored
            .value
            .clone()
            .ok_or_else(|| corrupt("a displaced version has no value"))?;
        merge::restore_version(conn, spec, &key, &stored, value).await?;
        batch.dirty.insert((table, key));
    }
    Ok(())
}

/// Versions retained for removal exclusion and the anchor marks that release them, for one
/// engine write.
///
/// A member retains what an operation supersedes while the operation's writer belongs to
/// neither the anchor nor the local device and the anchor has not confirmed storing it: only
/// such an operation can still fall past a removal cutoff. The anchor and a replica that does
/// not know its own device retain nothing; the anchor instead records, per writer of another
/// device, the highest sequence it applied, so it seals the groups those operations wrote again
/// if they are excluded later.
#[derive(Debug, Clone, Default)]
pub(crate) struct Retention {
    /// Writers of devices other than the anchor and the local one.
    retained: BTreeSet<WriterId>,
    /// Writers of the anchor, whose operation contexts confirm what the anchor stored.
    anchor_writers: BTreeSet<WriterId>,
    marks: BTreeMap<WriterId, Seq>,
    /// Marks raised since the last flush.
    raised: BTreeSet<WriterId>,
    /// On the anchor, writers of other devices, whose applied operations release what they
    /// displace.
    releasing: BTreeSet<WriterId>,
    /// Highest sequence applied per releasing writer since the last flush.
    released: BTreeMap<WriterId, Seq>,
}

impl Retention {
    pub(crate) async fn load(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<Self> {
        let devices: Option<(Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT anchor_device_id, local_device_id FROM sync_spaces WHERE space_id = ?",
        )
        .bind(space.as_bytes().as_slice())
        .fetch_optional(&mut *conn)
        .await?;
        let Some((Some(anchor), Some(local))) = devices else {
            return Ok(Self::default());
        };
        let mut retention = Self::default();
        let writers: Vec<(Vec<u8>, String)> =
            sqlx::query_as("SELECT writer_id, device_id FROM sync_writers WHERE space_id = ?")
                .bind(space.as_bytes().as_slice())
                .fetch_all(&mut *conn)
                .await?;
        for (writer, device) in writers {
            let writer = log::writer_id(&writer)?;
            if device == local {
                continue;
            }
            if anchor == local {
                retention.releasing.insert(writer);
            } else if device == anchor {
                retention.anchor_writers.insert(writer);
            } else {
                retention.retained.insert(writer);
            }
        }
        if anchor != local {
            retention.marks = marks(conn, space).await?;
        }
        Ok(retention)
    }

    /// Records that the anchor applied the operation `(writer, seq)` without retaining what it
    /// displaced.
    pub(crate) fn release(&mut self, writer: WriterId, seq: Seq) {
        if self.releasing.contains(&writer) {
            let mark = self.released.entry(writer).or_insert(0);
            *mark = (*mark).max(seq);
        }
    }

    /// Whether the versions the operation `(writer, seq)` supersedes are retained.
    pub(crate) fn retains(&self, writer: WriterId, seq: Seq) -> bool {
        self.retained.contains(&writer) && seq > self.marks.get(&writer).copied().unwrap_or(0)
    }

    /// Raises the marks from the context of an applied operation of `writer`: an operation
    /// of the anchor saw only what the anchor had applied, which it stored.
    pub(crate) fn confirm(&mut self, writer: WriterId, context: &VersionVector) {
        if !self.anchor_writers.contains(&writer) {
            return;
        }
        for (other, seq) in context.iter() {
            let mark = self.marks.entry(other).or_insert(0);
            if seq > *mark {
                *mark = seq;
                self.raised.insert(other);
            }
        }
    }

    /// Writes raised marks and drops the displaced versions they release, and records what the
    /// anchor applied.
    pub(crate) async fn flush(
        &mut self,
        conn: &mut SqliteConnection,
        space: SpaceId,
    ) -> SyncResult<()> {
        self.flush_released(conn, space).await?;
        let raised = std::mem::take(&mut self.raised);
        if raised.is_empty() {
            return Ok(());
        }
        let marks = raised
            .into_iter()
            .map(|writer| (writer, self.marks.get(&writer).copied().unwrap_or(0)))
            .collect::<Vec<_>>();
        write_marks(conn, space, &marks).await
    }

    /// Records what the anchor applied, before an authorization entry may exclude it.
    pub(crate) async fn flush_released(
        &mut self,
        conn: &mut SqliteConnection,
        space: SpaceId,
    ) -> SyncResult<()> {
        for (writer, seq) in std::mem::take(&mut self.released) {
            sqlx::query(
                "INSERT INTO sync_released_marks (space_id, writer_id, seq) VALUES (?, ?, ?)
                 ON CONFLICT (space_id, writer_id) DO UPDATE SET seq = max(seq, excluded.seq)",
            )
            .bind(space.as_bytes().as_slice())
            .bind(writer.as_bytes().as_slice())
            .bind(seq_i64(seq)?)
            .execute(&mut *conn)
            .await?;
        }
        Ok(())
    }
}

/// Brings exclusions and late changes in line with a new selection of the authorization chain,
/// inside the engine write that selected it. Entries carried by operations it excludes are
/// recorded and the chain selected again until nothing new is recorded, which ends because each
/// round records at least one stored operation.
pub(crate) async fn settle(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
    selection: &Selection,
) -> SyncResult<()> {
    let mut resolutions = selection.resolutions.clone();
    loop {
        let bounded = drop_beyond_bound(engine, conn, ctx.space, batch).await?;
        reconcile(engine, conn, ctx, batch).await?;
        if !record_excluded_entries(conn, ctx.space).await? && !bounded {
            break;
        }
        match authorization::rederive(conn, ctx.space).await? {
            Some(next) => resolutions = next.resolutions,
            None => break,
        }
    }
    rebuild_late(engine, conn, batch, &resolutions).await
}

/// Drops the operations a removed device's writers have stored past the
/// [`excluded_limit`] of their removal cutoff, or past the genesis of a surplus unnamed writer
/// (see [`MAX_UNNAMED_WRITERS_PER_REMOVED_DEVICE`]), which a replica stores only while it does
/// not know the removal or the lower writers yet: a replica that learned them first refuses
/// them, so dropping them leaves what never storing them leaves. Authorization entries
/// contiguous past that bound stay up to their own bound, as a replica that knew the removal
/// stores them too; the
/// entries of what is dropped go with it. Rows a dropped operation created
/// lose their whole merge state, honest changes included, since a replica that never stored
/// the creation skips those changes (see [`beyond_bound`]). The local device keeps its own
/// operations. Returns whether a dropped operation carried an entry.
async fn drop_beyond_bound(
    engine: &Engine,
    conn: &mut SqliteConnection,
    space: SpaceId,
    batch: &mut Batch,
) -> SyncResult<bool> {
    let local = local_device(conn, space).await?;
    let rows: Vec<(Vec<u8>, i64, i64, bool)> = sqlx::query_as(surplus_unnamed_query!(
        "SELECT writer_id, removal_cutoff_seq, applied_seq, surplus FROM (
            SELECT w.writer_id, w.removal_cutoff_seq, w.applied_seq, w.stored_seq, ",
        4,
        " AS surplus
            FROM sync_writers w
            JOIN sync_devices d ON d.space_id = w.space_id AND d.device_id = w.device_id
            WHERE w.space_id = ?1 AND w.removal_cutoff_seq IS NOT NULL
                AND d.removed_revision IS NOT NULL
                AND (?2 IS NULL OR w.device_id <> ?2)
         )
         WHERE stored_seq > removal_cutoff_seq + ?3 OR (stored_seq > ?5 AND surplus)"
    ))
    .bind(space.as_bytes().as_slice())
    .bind(local.as_deref())
    .bind(seq_i64(MAX_EXCLUDED_PER_WRITER)?)
    .bind(MAX_UNNAMED_WRITERS_PER_REMOVED_DEVICE)
    .bind(seq_i64(SURPLUS_LIMIT)?)
    .fetch_all(&mut *conn)
    .await?;
    let mut entries = false;
    for (writer, cutoff, applied, surplus) in rows {
        let writer = log::writer_id(&writer)?;
        let cutoff = stored_u64(cutoff, "removal cutoff")?;
        let (limit, entry_bound) = if surplus {
            (SURPLUS_LIMIT, SURPLUS_ENTRY_LIMIT)
        } else {
            (excluded_limit(cutoff), entry_limit(cutoff))
        };
        // Authorization entries contiguous past the bound stay, up to their own bound.
        let first_dropped: Option<i64> = sqlx::query_scalar(
            "SELECT min(seq) FROM sync_ops
             WHERE space_id = ? AND writer_id = ? AND seq > ? AND (kind <> ? OR seq > ?)",
        )
        .bind(space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(limit)?)
        .bind(i64::from(OperationKind::Authorization.as_u8()))
        .bind(seq_i64(entry_bound)?)
        .fetch_one(&mut *conn)
        .await?;
        let Some(first_dropped) = first_dropped else {
            continue;
        };
        let bound = stored_u64(first_dropped, "sequence")?.saturating_sub(1);
        let carried: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sync_auth_entries WHERE space_id = ? AND writer_id = ? AND seq > ?",
        )
        .bind(space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(bound)?)
        .fetch_one(&mut *conn)
        .await?;
        entries |= carried > 0;
        let created = created_past(engine, conn, space, writer, bound).await?;
        // The applied sequence stays: excluding the operations within the bound lowers it to
        // the cutoff and removes the effects of every applied one.
        crate::fork::drop_branch(conn, space, writer, bound, stored_u64(applied, "applied")?)
            .await?;
        for (table, key) in created {
            purge_row(conn, batch, table, key).await?;
        }
    }
    Ok(entries)
}

/// Rows created by the applied operations of `writer` past `bound`.
pub(crate) async fn created_past(
    engine: &Engine,
    conn: &mut SqliteConnection,
    space: SpaceId,
    writer: WriterId,
    bound: Seq,
) -> SyncResult<BTreeSet<(TableId, String)>> {
    let envelopes: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT envelope FROM sync_ops
         WHERE space_id = ? AND writer_id = ? AND seq > ? AND state = 'applied'",
    )
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(bound)?)
    .fetch_all(&mut *conn)
    .await?;
    let mut created = BTreeSet::new();
    for envelope in envelopes {
        let Some(Content::Changes(changes)) = Envelope::decode(&envelope)
            .ok()
            .and_then(|envelope| envelope.operation().ok())
            .map(|operation| operation.content)
        else {
            continue;
        };
        for change in &changes {
            if late_kind(engine, change) == Some(LateKind::Created) {
                created.insert((change.table, change.row.as_str().to_string()));
            }
        }
    }
    Ok(created)
}

/// Removes every version, tombstone, and displaced version of a row with its merge row, and
/// marks it dirty and orphaned so it materializes absent.
async fn purge_row(
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    table: TableId,
    key: String,
) -> SyncResult<()> {
    for merge_table in [
        "sync_version_refs",
        "sync_register_versions",
        "sync_tombstones",
        "sync_displaced_versions",
        "sync_rows",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {merge_table} WHERE table_id = ? AND row_key = ?"
        )))
        .bind(table_param(table))
        .bind(&key)
        .execute(&mut *conn)
        .await?;
    }
    batch.dirty.insert((table, key.clone()));
    batch.orphaned.insert((table, key));
    Ok(())
}

/// Records the entries of excluded operations stored since the last engine write and settles
/// again when the chain they extend changes the selection.
pub(crate) async fn admit_excluded_entries(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
) -> SyncResult<()> {
    if !record_excluded_entries(conn, ctx.space).await?
        && !displaced_unnamed(conn, ctx.space).await?
    {
        return Ok(());
    }
    if let Some(selection) = authorization::rederive(conn, ctx.space).await? {
        settle(engine, conn, ctx, batch, &selection).await?;
    }
    Ok(())
}

/// Whether a lower unnamed writer stored since the last engine write displaced one that still
/// holds operations past its genesis, which [`settle`] drops along with their late changes.
async fn displaced_unnamed(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<bool> {
    let local = local_device(conn, space).await?;
    let displaced: i64 = sqlx::query_scalar(surplus_unnamed_query!(
        "SELECT EXISTS (SELECT 1 FROM sync_writers w
         JOIN sync_devices d ON d.space_id = w.space_id AND d.device_id = w.device_id
         WHERE w.space_id = ?1 AND d.removed_revision IS NOT NULL
            AND (?6 IS NULL OR w.device_id <> ?6) AND ",
        2,
        " AND EXISTS (
                SELECT 1 FROM sync_ops o
                WHERE o.space_id = w.space_id AND o.writer_id = w.writer_id AND o.seq > ?3
                    AND (o.kind <> ?4 OR o.seq > ?5)
            ))"
    ))
    .bind(space.as_bytes().as_slice())
    .bind(MAX_UNNAMED_WRITERS_PER_REMOVED_DEVICE)
    .bind(seq_i64(SURPLUS_LIMIT)?)
    .bind(i64::from(OperationKind::Authorization.as_u8()))
    .bind(seq_i64(SURPLUS_ENTRY_LIMIT)?)
    .bind(local.as_deref())
    .fetch_one(&mut *conn)
    .await?;
    Ok(displaced != 0)
}

/// Records the authorization entries that excluded operations carry, which never apply. Chain
/// selection has to see every entry a replica stores whatever became of its carrier: otherwise
/// two replicas that learn of competing chains in different orders each keep the first, since
/// its removals exclude the other chain's carrier. The same holds for entries whose operations
/// wait: a yield a device held for data seals after writing waits behind its held row changes,
/// and a kit handover sealed against a revision past a frozen point waits until that revision is
/// reachable, yet each has to resolve the contest that keeps it waiting. Returns whether any
/// entry was new.
async fn record_excluded_entries(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<bool> {
    let rows: Vec<(Vec<u8>, i64, Vec<u8>)> = sqlx::query_as(
        "SELECT o.writer_id, o.seq, o.envelope FROM sync_ops o
         WHERE o.space_id = ?1 AND o.kind = ?2
            AND (o.excluded_revision IS NOT NULL OR o.state = 'waiting')
            AND NOT EXISTS (
                SELECT 1 FROM sync_auth_entries e
                WHERE e.space_id = o.space_id AND e.writer_id = o.writer_id AND e.seq = o.seq
            )
         ORDER BY o.writer_id, o.seq",
    )
    .bind(space.as_bytes().as_slice())
    .bind(i64::from(OperationKind::Authorization.as_u8()))
    .fetch_all(&mut *conn)
    .await?;
    let mut recorded = false;
    for (writer, seq, envelope) in rows {
        let Some(operation) = Envelope::decode(&envelope)
            .ok()
            .and_then(|envelope| envelope.operation().ok())
        else {
            continue;
        };
        let Content::Authorization(record) = &operation.content else {
            continue;
        };
        let writer = log::writer_id(&writer)?;
        let carrier = log::writer(conn, space, writer)
            .await?
            .ok_or_else(|| corrupt("an excluded operation has no writer"))?;
        if !authorization::carries(&carrier.device_id, record) {
            continue;
        }
        authorization::insert_entry(conn, space, writer, stored_u64(seq, "sequence")?, record)
            .await?;
        recorded = true;
    }
    Ok(recorded)
}

/// Records the device of the local writer and, when it changed, settles exclusions again: the
/// local device never excludes its own operations.
pub(crate) async fn note_local_device(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
    device_id: &str,
) -> SyncResult<()> {
    if !record_local_device(conn, ctx.space, device_id).await? {
        return Ok(());
    }
    if let Some(selection) = authorization::select(conn, ctx.space).await? {
        settle(engine, conn, ctx, batch, &selection).await?;
    }
    Ok(())
}

async fn marks(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<BTreeMap<WriterId, Seq>> {
    let rows: Vec<(Vec<u8>, i64)> =
        sqlx::query_as("SELECT writer_id, seq FROM sync_anchor_marks WHERE space_id = ?")
            .bind(space.as_bytes().as_slice())
            .fetch_all(&mut *conn)
            .await?;
    rows.iter()
        .map(|(writer, seq)| Ok((log::writer_id(writer)?, stored_u64(*seq, "anchor mark")?)))
        .collect()
}

/// Raises anchor marks and drops the displaced versions whose superseding operation they
/// cover.
async fn write_marks(
    conn: &mut SqliteConnection,
    space: SpaceId,
    marks: &[(WriterId, Seq)],
) -> SyncResult<()> {
    for (writer, seq) in marks {
        if *seq == 0 {
            continue;
        }
        sqlx::query(
            "INSERT INTO sync_anchor_marks (space_id, writer_id, seq) VALUES (?, ?, ?)
             ON CONFLICT (space_id, writer_id) DO UPDATE SET seq = max(seq, excluded.seq)",
        )
        .bind(space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(*seq)?)
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            "DELETE FROM sync_displaced_versions
             WHERE displaced_by_writer = ?1 AND displaced_by_seq <= (
                SELECT seq FROM sync_anchor_marks WHERE space_id = ?2 AND writer_id = ?1
             )",
        )
        .bind(writer.as_bytes().as_slice())
        .bind(space.as_bytes().as_slice())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Records what the anchor reported storing in a direct exchange, inside an engine write.
///
/// The report counts only once every operation of the anchor's writers it lists is applied
/// here, so every removal it sequenced by then is known and caps the marks of its device's
/// writers at their cutoffs: the anchor stores operations past a cutoff too.
pub(crate) async fn record_marks(
    conn: &mut SqliteConnection,
    space: SpaceId,
    anchor: &str,
    stored: &VersionVector,
) -> SyncResult<bool> {
    let current: Option<Option<String>> =
        sqlx::query_scalar("SELECT anchor_device_id FROM sync_spaces WHERE space_id = ?")
            .bind(space.as_bytes().as_slice())
            .fetch_optional(&mut *conn)
            .await?;
    if current.flatten().as_deref() != Some(anchor) {
        return Ok(false);
    }
    let applied = log::applied_vector(conn, space).await?;
    let mut caps = BTreeMap::new();
    for writer in log::writers(conn, space).await? {
        if writer.device_id == anchor && applied.get(&writer.id) < stored.get(&writer.id) {
            return Ok(false);
        }
        if let Some(cutoff) = writer.removal_cutoff {
            caps.insert(writer.id, cutoff);
        }
    }
    let marks = stored
        .iter()
        .map(|(writer, seq)| (writer, caps.get(&writer).map_or(seq, |cap| seq.min(*cap))))
        .collect::<Vec<_>>();
    write_marks(conn, space, &marks).await?;
    Ok(true)
}

/// What a removed device's late operations did to a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LateKind {
    /// Wrote values of an existing row.
    Values,
    /// Deleted the row.
    Tombstone,
    /// Created the row, which stays hidden until an admin restores it.
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

/// What one change of an excluded operation leaves: a create of a row with its own key
/// creates it, and a create of a derived-key row only writes values, since any replica may
/// create that row on its own.
fn late_kind(engine: &Engine, change: &Change) -> Option<LateKind> {
    let spec = engine.manifest().table(change.table)?;
    Some(match change.action {
        ChangeAction::Create if spec.derived_key.is_none() => LateKind::Created,
        ChangeAction::Create | ChangeAction::Write => LateKind::Values,
        ChangeAction::Tombstone => LateKind::Tombstone,
    })
}

/// Folds the changes of the excluded operation `(writer, seq)` into the late changes of
/// `removal`. A row keeps the strongest kind (created, then tombstone, then values), the union
/// of the groups written, and the latest value per group; a decision on the row stays.
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
    let rows: Vec<(Vec<u8>, i64, i64, Vec<u8>)> = sqlx::query_as(
        "SELECT writer_id, seq, excluded_revision, envelope FROM sync_ops
         WHERE space_id = ? AND excluded_revision IS NOT NULL ORDER BY writer_id, seq",
    )
    .bind(batch.space.as_bytes().as_slice())
    .fetch_all(&mut *conn)
    .await?;
    for (writer, seq, removal, envelope) in rows {
        let Some(operation) = Envelope::decode(&envelope)
            .ok()
            .and_then(|envelope| envelope.operation().ok())
        else {
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

/// Whether a live row is hidden as a late change: a removed device created it past its cutoff
/// and no admin restored it, or excluding a removed device's operations left it without a
/// value for a group.
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
    if rows
        .iter()
        .any(|(change, resolution)| change == "created" && resolution.as_deref() != Some("restore"))
    {
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
    /// What the excluded operations did to the row.
    pub kind: LateKind,
    /// Title and preview from the late values.
    pub presentation: Presentation,
    /// Latest late value per group written.
    pub values: Vec<(GroupId, Value)>,
    /// Clock of the newest late value, or `None` when the change only deleted the row.
    pub clock: Option<Hlc>,
}

/// The change restoring a late change seals: the late values no current version is newer than,
/// as a create of every group when the removed device created the row or no published state
/// knows it, or a deletion of a live row.
async fn restoring_change(
    engine: &Engine,
    conn: &mut SqliteConnection,
    record: &LateRecord,
) -> SyncResult<Option<Change>> {
    let Some(spec) = engine.manifest().table(record.table) else {
        return Err(corrupt("late change of an unknown table"));
    };
    let row = RowKey::new(record.key.clone()).map_err(|_| corrupt("stored row key"))?;
    let state = merge::row_state(conn, record.table, &record.key).await?;
    if record.kind == LateKind::Tombstone {
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
    // A version restating an older value this replica sealed again is no newer edit. Only the
    // replica that sealed a restatement, the anchor or a former anchor, records it, so on another
    // admin a restatement still counts as newer and the group keeps the restated value.
    let newer = |group: GroupId, late: &LateValue| {
        versions.iter().any(|version| {
            version.group == group
                && version.clock > late.clock
                && restated.get(&group) != Some(&version.hash)
        })
    };
    // A late create comes back as a create, which writes every group: a group an honest
    // operation wrote after the late value keeps that honest value.
    if record.kind == LateKind::Created {
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
    let groups = record
        .groups
        .iter()
        .filter(|(group, late)| !newer(**group, late))
        .map(|(group, late)| GroupValue::new(spec.id, *group, late.value.clone()))
        .collect::<Vec<_>>();
    if groups.is_empty() {
        return Ok(None);
    }
    let action = match state {
        None => ChangeAction::Create,
        Some(_) => ChangeAction::Write,
    };
    Ok(Some(Change {
        table: record.table,
        row,
        action,
        groups,
        replaced_by: None,
        cause: None,
    }))
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
            changes.push(LateChange {
                removal_revision: record.removal,
                device_id: removed
                    .get(&record.removal)
                    .cloned()
                    .ok_or_else(|| corrupt("late change of an unknown removal"))?,
                table: record.table,
                row_key: record.key,
                kind: record.kind,
                presentation,
                clock: record.groups.values().map(|late| late.clock).max(),
                values,
            });
        }
        Ok(changes)
    }

    /// Applies a late change [`Engine::late_changes`] listed as an ordinary write of the local
    /// writer, before an admin sequences `resolve_late` restoring it: the late values no
    /// current version is newer than, or the deletion of a live row. A change already decided,
    /// or one with nothing left to write, seals no change.
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
        let change = restoring_change(self, &mut tx, &record).await?;
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

    /// Records the stored vector the anchor reported in a direct exchange as anchor marks, and
    /// drops the displaced versions they cover. A report from a device that is not the anchor
    /// of the selected chain, or one listing anchor operations not applied here yet, changes
    /// nothing. Returns whether the marks were recorded.
    pub async fn record_anchor_marks(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        anchor_device_id: &str,
        stored: &VersionVector,
    ) -> SyncResult<bool> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        let recorded = record_marks(&mut tx, ctx.space, anchor_device_id, stored).await?;
        tx.commit().await?;
        Ok(recorded)
    }
}
