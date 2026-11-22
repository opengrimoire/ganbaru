//! Removal exclusion, displaced versions, and late changes.
//!
//! A selected removal lists a cutoff per writer of the removed device: what the anchor had
//! stored when it sequenced the removal. Operations past it are excluded on every replica but
//! the removed device itself. They are still stored, up to [`excluded_limit`] per
//! writer, and forwarded, but held as invalid, and the merge effects of those already applied
//! are removed. The versions such an operation superseded come back from
//! `sync_displaced_versions`, which a member fills while the anchor has not confirmed storing
//! the superseding operation; the anchor never applies an operation past a cutoff, so it keeps
//! none. Every replica also keeps there, once per Create, the value of a Create an Immutable
//! group left out, and once removals are settled the group holds exactly the kept Creates that
//! saw no other, so excluding a version ends as if the Create arrived after the exclusion,
//! whatever the order. After a handover the cutoff can still fall below what an earlier anchor installation
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
    Envelope, GroupId, OperationKind, Seq, SpaceId, TableId, VersionVector, WriterId,
};
use sqlx::{Connection, Row, SqliteConnection};

use crate::authorization::{self, Selection};
use crate::error::{SyncResult, corrupt};
use crate::log::{self, seq_i64, stored_u64};
use crate::merge::{self, Batch, table_param};
use crate::op_state::{self, OpMove, OpState};
use crate::{Engine, SpaceContext, records, reseal, store};

mod late;

pub use late::{LateChange, LateKind};
pub(crate) use late::{
    any_late, created_late, decode_groups, defer_stale, fold, has_creation, hides, mark_stale,
    stale_rows_listed, take_stale,
};
use late::{rebuild_late, tracks_creation};

/// Most operations past its removal cutoff a removed device's writer may still have stored.
/// Later ones are refused as revoked.
pub const MAX_EXCLUDED_PER_WRITER: u64 = 1024;

/// Most writers the removal does not name, with cutoff 0, that keep the excluded operations of a
/// removed device: the ones with the lowest writer ids, so every replica derives the same late
/// changes whatever order the writers arrived in. Each keeps up to [`MAX_EXCLUDED_PER_WRITER`]
/// operations. A surplus writer keeps only its genesis and the authorization entries contiguous
/// from it, and a lower one displaces the highest, so a removed device that mints writers stores
/// a bounded amount anywhere. Every replica stores a surplus genesis within the writer limits,
/// so a context covering it becomes ready everywhere. Past those limits a lower unnamed genesis,
/// or one the removal names, displaces the highest unnamed writer, and a context covering a
/// writer the limits refuse for good is past the bound everywhere.
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
    /// What the removal keeps of the writer, when its device was removed and is not the local
    /// one.
    pub(crate) removal: Option<Removal>,
    /// Last sequence the copies of a forked writer share.
    pub(crate) fork: Option<Seq>,
}

/// What a removal keeps of one writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Removal {
    /// Removal cutoff of the writer.
    pub(crate) cutoff: Seq,
    /// Last sequence of the writer stored here.
    pub(crate) stored: Seq,
    /// Last sequence of the writer outside authorization entries any replica keeps:
    /// [`excluded_limit`] of the cutoff, [`SURPLUS_LIMIT`] for a surplus unnamed writer, or 0
    /// for an unnamed writer the writer limits refuse.
    pub(crate) limit: Seq,
}

/// Writers some of whose operations are excluded on this replica: every writer of a removed
/// device but the local one, stored or refused by the writer limits, and every forked writer.
pub(crate) async fn excluding(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<BTreeMap<WriterId, Excluding>> {
    let rows: Vec<(Vec<u8>, i64, i64, bool)> = sqlx::query_as(surplus_unnamed_query!(
        "SELECT w.writer_id, w.removal_cutoff_seq, w.stored_seq, ",
        2,
        " AS surplus
         FROM sync_writers w JOIN sync_spaces s ON s.space_id = w.space_id
         WHERE w.space_id = ?1 AND w.removal_cutoff_seq IS NOT NULL
            AND (s.local_device_id IS NULL OR w.device_id <> s.local_device_id)"
    ))
    .bind(space.as_bytes().as_slice())
    .bind(MAX_UNNAMED_WRITERS_PER_REMOVED_DEVICE)
    .fetch_all(&mut *conn)
    .await?;
    let mut excluding = BTreeMap::new();
    for (writer, cutoff, stored, surplus) in &rows {
        let cutoff = stored_u64(*cutoff, "removal cutoff")?;
        excluding.insert(
            log::writer_id(writer)?,
            Excluding {
                removal: Some(Removal {
                    cutoff,
                    stored: stored_u64(*stored, "stored sequence")?,
                    limit: if *surplus {
                        SURPLUS_LIMIT
                    } else {
                        excluded_limit(cutoff)
                    },
                }),
                fork: None,
            },
        );
    }
    for writer in refused_unnamed(conn, space).await? {
        excluding.insert(
            writer,
            Excluding {
                removal: Some(Removal {
                    cutoff: 0,
                    stored: 0,
                    limit: 0,
                }),
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

/// Writers of a removed device but the local one whose certificate is stored but whose genesis
/// the writer limits refuse for good: the removal does not name them, the device or the space is
/// full, and no stored unnamed writer of the device has a higher id to displace. Every replica
/// that stores the same geneses refuses them alike, so a context covering one is past the bound
/// everywhere. A writer the limits would still admit is left out, and a context covering it
/// waits for its genesis.
async fn refused_unnamed(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<Vec<WriterId>> {
    let rows: Vec<(Vec<u8>, String)> = sqlx::query_as(
        "SELECT DISTINCT e.writer_id, e.device_id
         FROM sync_auth_envelopes e
         JOIN sync_devices d
            ON d.space_id = e.space_id AND d.device_id = e.device_id
                AND d.device_key = e.device_key
         JOIN sync_spaces s ON s.space_id = e.space_id
         WHERE e.space_id = ?1 AND e.role = 'certificate' AND d.removed_revision IS NOT NULL
            AND (s.local_device_id IS NULL OR e.device_id <> s.local_device_id)
            AND NOT EXISTS (
                SELECT 1 FROM sync_writers w
                WHERE w.space_id = e.space_id AND w.writer_id = e.writer_id
            )
            AND NOT EXISTS (
                SELECT 1 FROM sync_writers w
                WHERE w.space_id = e.space_id AND w.device_id = e.device_id
                    AND w.removal_cutoff_seq = 0 AND w.writer_id > e.writer_id
            )",
    )
    .bind(space.as_bytes().as_slice())
    .fetch_all(&mut *conn)
    .await?;
    let mut refused = Vec::new();
    for (writer, device) in rows {
        let writer = log::writer_id(&writer)?;
        if !store::within_writer_limits(conn, space, &device, true).await?
            && authorization::removal_cutoff(conn, space, &device, writer).await? == 0
        {
            refused.push(writer);
        }
    }
    Ok(refused)
}

/// Whether an operation with causal context `context` may apply after `applied`.
///
/// A context entry `(W, s)` is satisfied when `W` is applied through `s`, or when the selected
/// state excludes `(W, s)` and `W` is applied through what stays:
///
/// - Fork: `s` is past the last sequence the copies of the forked writer `W` share, and `W` is
///   applied through that sequence. No replica stores either branch once it holds the evidence.
/// - Removal: `s` is past the removal cutoff of `W`, `W` is applied through the cutoff, and the
///   excluded operation is stored, or `s` is past the writer's [`Removal::limit`], where no
///   replica stores it. Storing the excluded operations first lets the late changes they leave
///   know the rows they created. A surplus unnamed writer's limit is its genesis, which every
///   replica stores within the writer limits. An unnamed writer those limits refuse for good,
///   whose genesis no replica keeps, has limit 0.
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
            Some(removal) if seq > removal.cutoff => {
                applied.get(&writer) >= removal.cutoff
                    && (removal.stored >= seq || seq > removal.limit)
            }
            _ => applied.get(&writer) >= seq,
        }
    })
}

/// Whether `context` covers an operation past the [`Removal::limit`] of an excluded writer. No
/// replica keeps such an operation, so a row only it created is unknown everywhere, and an
/// honest change to that row is dropped on every replica alike instead of holding its writer.
pub(crate) fn beyond_bound(
    context: &VersionVector,
    excluding: &BTreeMap<WriterId, Excluding>,
) -> bool {
    !excluding.is_empty()
        && context.iter().any(|(writer, seq)| {
            excluding
                .get(&writer)
                .and_then(|excluded| excluded.removal)
                .is_some_and(|removal| seq > removal.limit)
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
    // How many operations of each writer the guarded update below must exclude.
    let mut excluding: BTreeMap<WriterId, u64> = BTreeMap::new();
    for (writer, _, state, revision, cutoff) in &rows {
        *excluding.entry(log::writer_id(writer)?).or_default() += 1;
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
        let attempted = OpMove::Exclude {
            revision: *revision,
        };
        let (state, hold_reason) = OpState::Excluded {
            revision: *revision,
        }
        .columns();
        let affected = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE sync_ops SET state = ?2, hold_reason = ?3, excluded_revision = ?1
             WHERE space_id = ?4 AND writer_id = ?5 AND seq > ?6 AND {}",
            attempted.source()
        )))
        .bind(seq_i64(*revision)?)
        .bind(state)
        .bind(hold_reason)
        .bind(ctx.space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(*cutoff)?)
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if Some(&affected) != excluding.get(writer) {
            return Err(corrupt(
                "the operations to exclude changed while their removal was applied",
            ));
        }
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
        merge::settle_kept(engine, conn, batch).await?;
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

/// Classifies again every excluded operation no selected removal excludes any more, writing the
/// state and the signed header fields a fresh store of the operation writes, since the hold it
/// was stored with may have left them out. Returns whether there was one.
async fn release(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    local: Option<&str>,
) -> SyncResult<bool> {
    let rows: Vec<(Vec<u8>, i64, Vec<u8>, String)> = sqlx::query_as(
        "SELECT o.writer_id, o.seq, o.envelope, w.device_id
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
    for (writer_bytes, seq, envelope, device) in &rows {
        let writer = log::writer_id(writer_bytes)?;
        let classified = match Envelope::decode(envelope) {
            Ok(envelope) => store::classify(engine, &envelope, device),
            Err(_) => store::Classified::Malformed,
        };
        let hold = match &classified {
            store::Classified::Malformed => Some(store::HoldReason::Invalid),
            classified => classified.hold(),
        };
        let context = store::derive_context(
            conn,
            ctx,
            writer,
            stored_u64(*seq, "sequence")?,
            classified.header().map(|(_, dependencies)| dependencies),
        )
        .await?;
        let attempted = OpMove::Release(hold);
        let (state, hold_reason) = OpState::stored(hold, None).columns();
        let affected = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE sync_ops SET excluded_revision = NULL, state = ?, hold_reason = ?,
                context = ?, authorization_revision = ?, group_continues = ?
             WHERE space_id = ? AND writer_id = ? AND seq = ? AND {}",
            attempted.source()
        )))
        .bind(state)
        .bind(hold_reason)
        .bind(log::encode_context(context.as_ref()))
        .bind(seq_i64(classified.revision())?)
        .bind(classified.continues())
        .bind(ctx.space.as_bytes().as_slice())
        .bind(writer_bytes.as_slice())
        .bind(seq)
        .execute(&mut *conn)
        .await?
        .rows_affected();
        op_state::expect_one(affected, attempted)?;
    }
    Ok(!rows.is_empty())
}

/// Versions of one group as `(writer, seq)`.
async fn group_versions(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
    group: GroupId,
) -> SyncResult<Vec<(WriterId, Seq)>> {
    let rows: Vec<(Vec<u8>, i64)> = sqlx::query_as(
        "SELECT writer_id, seq FROM sync_register_versions
         WHERE table_id = ? AND row_key = ? AND group_id = ?",
    )
    .bind(table_param(table))
    .bind(key)
    .bind(i64::from(group.get()))
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(|(writer, seq)| {
            Ok((
                log::writer_id(writer)?,
                stored_u64(*seq, "version sequence")?,
            ))
        })
        .collect()
}

/// Restores the versions the operations of `writer` past `cutoff` superseded, unless the
/// superseded version's own operation is excluded or gone, its writer has a later version of the
/// group, or a remaining version of the group saw it. Displaced rows of those operations, and
/// those of versions the operations wrote, are dropped. Creates held back from a group
/// [`merge::keeps_immutable`] are settled by [`merge::settle_kept`] once every removal is.
pub(crate) async fn restore_displaced(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    writer: WriterId,
    cutoff: Seq,
) -> SyncResult<()> {
    let rows = sqlx::query(
        "SELECT d.table_id, d.row_key, d.group_id, d.writer_id, d.seq, d.clock, d.value_hash,
            d.value, o.seq IS NULL OR o.excluded_revision IS NOT NULL
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
        let group = spec
            .group(stored.group)
            .ok_or_else(|| corrupt("displaced version of an unknown group"))?;
        if merge::keeps_immutable(spec, group) {
            continue;
        }
        let existing = group_versions(conn, table, &key, stored.group).await?;
        let mut superseded = false;
        for (other, other_seq) in existing {
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
        if !records::grown(conn, ctx.space).await? && !bounded {
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
/// stores them too; the entries of what is dropped go with it. A row loses its whole merge
/// state only when no kept operation created it, honest changes included, since a replica that
/// never stored the creation skips those changes (see [`beyond_bound`]). A row a kept operation
/// also created keeps its merge state, and excluding the dropped operations' applied effects
/// removes what they wrote to it. The local device keeps its own operations. Returns whether a
/// dropped operation carried an entry.
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
            "SELECT count(*) FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq > ? AND kind = ?",
        )
        .bind(space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(bound)?)
        .bind(i64::from(OperationKind::Authorization.as_u8()))
        .fetch_one(&mut *conn)
        .await?;
        entries |= carried > 0;
        let created = created_past(engine, conn, space, writer, bound).await?;
        // The applied sequence stays: excluding the operations within the bound lowers it to
        // the cutoff and removes the effects of every applied one.
        crate::fork::drop_branch(conn, space, writer, bound, stored_u64(applied, "applied")?)
            .await?;
        for (table, key) in uncreated_after_drop(conn, writer, bound, created).await? {
            purge_row(conn, batch, table, key).await?;
        }
    }
    Ok(entries)
}

/// Rows created by the applied operations of `writer` past `bound`, read from the recorded
/// creations so that operations whose bodies were compacted count too. Only rows of tables that
/// track their creation count, see [`tracks_creation`].
pub(crate) async fn created_past(
    engine: &Engine,
    conn: &mut SqliteConnection,
    space: SpaceId,
    writer: WriterId,
    bound: Seq,
) -> SyncResult<BTreeSet<(TableId, String)>> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT c.table_id, c.row_key FROM sync_row_creations c
         JOIN sync_ops o ON o.writer_id = c.writer_id AND o.seq = c.seq
         WHERE o.space_id = ? AND c.writer_id = ? AND c.seq > ?",
    )
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(bound)?)
    .fetch_all(&mut *conn)
    .await?;
    let mut created = BTreeSet::new();
    for (table, key) in rows {
        let table = u16::try_from(table)
            .map(TableId)
            .map_err(|_| corrupt("row creation of a table id out of range"))?;
        if engine.manifest().table(table).is_some_and(tracks_creation) {
            created.insert((table, key));
        }
    }
    Ok(created)
}

/// Forgets the creations of `writer` past `bound`, and keeps of `rows` those no other
/// creation holds, which are the rows only dropped operations created.
async fn uncreated_after_drop(
    conn: &mut SqliteConnection,
    writer: WriterId,
    bound: Seq,
    rows: BTreeSet<(TableId, String)>,
) -> SyncResult<BTreeSet<(TableId, String)>> {
    sqlx::query("DELETE FROM sync_row_creations WHERE writer_id = ? AND seq > ?")
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(bound)?)
        .execute(&mut *conn)
        .await?;
    let mut uncreated = BTreeSet::new();
    for (table, key) in rows {
        if !has_creation(conn, table, &key).await? {
            uncreated.insert((table, key));
        }
    }
    Ok(uncreated)
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

/// Selects the chain again when the record store grew since the last walk, or a lower unnamed
/// writer displaced one, and settles exclusions for the result.
pub(crate) async fn admit_excluded_entries(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
) -> SyncResult<()> {
    if !records::grown(conn, ctx.space).await? && !displaced_unnamed(conn, ctx.space).await? {
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
        // The envelope this replica keeps is the one sealed to the local device's key.
        authorization::write_space_keys(conn, ctx.space, &selection).await?;
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
/// cover. Held Creates stay: a cutoff after a handover or a fork can still fall below a mark,
/// and only the held Create can fill its group again.
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
             )
                AND NOT (writer_id = ?1 AND seq = displaced_by_seq)",
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

impl Engine {
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
