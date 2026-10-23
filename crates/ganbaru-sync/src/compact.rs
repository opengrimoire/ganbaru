//! Compaction: dropping the bodies of operations every counted device holds.
//!
//! A writer's operations at or below its compaction limit are compacted in sequence order: each
//! one's envelope is replaced by the empty blob while its row, hash, clock, and context stay, so
//! chains, hashes, and acknowledgements still read. `sync_writers.compacted_seq` is the highest
//! sequence whose envelope may be empty; an empty envelope above it is corrupt state. Only
//! applied changes and acknowledgements are compacted. A genesis, a revocation, a kind this
//! format reserves, and the head of every chain keep their bodies, because certificates,
//! cutoffs, and head verification read them. A re-seal may later cut a chain back to a
//! compacted operation; that writer is no longer local, so only its head hash and clock are
//! read.
//!
//! Before a body is dropped, every elided version the operation wrote is stored back from the
//! body, since a later invalid local edit or a concurrent sibling may need the old winner after
//! the domain row changed.
//!
//! The limit is the stability vector, met with what this replica applied. A client further
//! caps its own writers at the head the coordinator last confirmed with the same hash, so a
//! fork re-seal never needs a compacted body, and leaves its other own writers whole. A peer
//! whose stored sequence of a writer is below that writer's `compacted_seq` cannot be served
//! the missing operations and must bootstrap from a snapshot.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use ganbaru_sync_contracts::{
    Content, Envelope, OperationKind, Seq, SpaceId, VersionVector, WriterId,
};
use sqlx::{Connection, Row, SqliteConnection};

use crate::error::{SyncResult, corrupt, failpoint};
use crate::log::{self, seq_i64, stored_u64};
use crate::merge::table_param;
use crate::{Engine, SpaceContext};

/// Most operations one compaction transaction visits.
pub const COMPACTION_BATCH_OPS: usize = 256;

/// Time after which a compaction run starts no further transaction.
pub const COMPACTION_TIME_BUDGET: Duration = Duration::from_millis(200);

/// What one compaction run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompactionReport {
    /// Operations whose body was dropped.
    pub compacted: usize,
    /// Elided versions stored back before their operation's body was dropped.
    pub hydrated: usize,
    /// Transactions committed.
    pub batches: usize,
    /// Whether operations below the limit remain because the run stopped at its time budget.
    pub more: bool,
}

/// The compaction limit of a replica: `stable`, with every writer in `own_writers` capped at
/// `confirmed` when it names that writer and left out otherwise. The coordinator passes no own
/// writers, since its stable vector already waits for every counted device.
pub fn compaction_limit(
    stable: &VersionVector,
    own_writers: &BTreeSet<WriterId>,
    confirmed: Option<(WriterId, Seq)>,
) -> VersionVector {
    let mut limit = VersionVector::new();
    for (writer, seq) in stable.iter() {
        let seq = if own_writers.contains(&writer) {
            match confirmed {
                Some((confirmed_writer, head)) if confirmed_writer == writer => seq.min(head),
                _ => 0,
            }
        } else {
            seq
        };
        if seq > 0 {
            limit.set(writer, seq);
        }
    }
    limit
}

/// One writer's operations a run may compact: from `next` through `through`.
struct Lane {
    writer: WriterId,
    next: Seq,
    through: Seq,
}

impl Engine {
    /// Replaces the stored stability vector of the space, which only the coordinator's reports
    /// change.
    pub async fn record_stable(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        stable: &VersionVector,
    ) -> SyncResult<()> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        sqlx::query("DELETE FROM sync_stable WHERE space_id = ?")
            .bind(ctx.space.as_bytes().as_slice())
            .execute(&mut *tx)
            .await?;
        for (writer, seq) in stable.iter().filter(|(_, seq)| *seq > 0) {
            sqlx::query("INSERT INTO sync_stable (space_id, writer_id, seq) VALUES (?, ?, ?)")
                .bind(ctx.space.as_bytes().as_slice())
                .bind(writer.as_bytes().as_slice())
                .bind(seq_i64(seq)?)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// The stability vector last recorded for the space.
    pub async fn stable_vector(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<VersionVector> {
        log::ensure_space(conn, ctx.space).await?;
        let rows = sqlx::query("SELECT writer_id, seq FROM sync_stable WHERE space_id = ?")
            .bind(ctx.space.as_bytes().as_slice())
            .fetch_all(&mut *conn)
            .await?;
        let mut stable = VersionVector::new();
        for row in rows {
            let writer: Vec<u8> = row.try_get(0)?;
            stable.set(
                log::writer_id(&writer)?,
                stored_u64(row.try_get(1)?, "stable sequence")?,
            );
        }
        Ok(stable)
    }

    /// Highest compacted sequence per writer, without writers that compacted nothing.
    pub async fn compacted_vector(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<VersionVector> {
        log::ensure_space(conn, ctx.space).await?;
        log::compacted_vector(conn, ctx.space).await
    }

    /// Compacts operations up to `limit` with the default batch size and time budget.
    pub async fn compact(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        limit: &VersionVector,
    ) -> SyncResult<CompactionReport> {
        self.compact_with(
            conn,
            ctx,
            limit,
            COMPACTION_BATCH_OPS,
            COMPACTION_TIME_BUDGET,
        )
        .await
    }

    /// Compacts operations up to `limit` in transactions of at most `batch_ops` operations,
    /// starting no transaction after `budget`. Every committed transaction leaves each writer's
    /// compacted operations a contiguous prefix.
    pub async fn compact_with(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        limit: &VersionVector,
        batch_ops: usize,
        budget: Duration,
    ) -> SyncResult<CompactionReport> {
        let started = Instant::now();
        let batch_ops = batch_ops.max(1);
        let mut report = CompactionReport::default();
        log::ensure_space(conn, ctx.space).await?;
        let mut lanes = lanes(conn, ctx.space, limit).await?;
        while !lanes.is_empty() {
            if report.batches > 0 && started.elapsed() >= budget {
                report.more = true;
                break;
            }
            let mut tx = conn.begin().await?;
            let mut visited = 0;
            for lane in &mut lanes {
                while visited < batch_ops && lane.next <= lane.through {
                    let take = (lane.through - lane.next + 1).min((batch_ops - visited) as u64);
                    let done = compact_range(&mut tx, ctx.space, lane, take, &mut report).await?;
                    visited += done;
                }
                if visited >= batch_ops {
                    break;
                }
            }
            failpoint!("compact_batch");
            tx.commit().await?;
            report.batches += 1;
            lanes.retain(|lane| lane.next <= lane.through);
        }
        Ok(report)
    }
}

/// Writers with operations to compact under `limit`: applied, below the head, and above what
/// was compacted.
async fn lanes(
    conn: &mut SqliteConnection,
    space: SpaceId,
    limit: &VersionVector,
) -> SyncResult<Vec<Lane>> {
    let mut lanes = Vec::new();
    for writer in log::writers(conn, space).await? {
        let through = limit
            .get(&writer.id)
            .min(writer.applied)
            .min(writer.stored.saturating_sub(1));
        if through > writer.compacted {
            lanes.push(Lane {
                writer: writer.id,
                next: writer.compacted + 1,
                through,
            });
        }
    }
    Ok(lanes)
}

/// Compacts up to `take` operations of a lane from its next sequence and advances its
/// `compacted_seq`. Returns how many operations it visited.
async fn compact_range(
    conn: &mut SqliteConnection,
    space: SpaceId,
    lane: &mut Lane,
    take: u64,
    report: &mut CompactionReport,
) -> SyncResult<usize> {
    let last = lane.next + take - 1;
    let rows = sqlx::query(
        "SELECT seq, kind, state, envelope FROM sync_ops
         WHERE space_id = ? AND writer_id = ? AND seq BETWEEN ? AND ? ORDER BY seq",
    )
    .bind(space.as_bytes().as_slice())
    .bind(lane.writer.as_bytes().as_slice())
    .bind(seq_i64(lane.next)?)
    .bind(seq_i64(last)?)
    .fetch_all(&mut *conn)
    .await?;
    let mut expected = lane.next;
    for row in &rows {
        let seq = stored_u64(row.try_get(0)?, "sequence")?;
        if seq != expected {
            return Err(corrupt("a stored chain has a gap below its head"));
        }
        let state: String = row.try_get(2)?;
        if state != "applied" {
            return Err(corrupt(
                "an operation below the applied sequence is not applied",
            ));
        }
        let envelope: Vec<u8> = row.try_get(3)?;
        report.hydrated += hydrate_versions(conn, lane.writer, seq, &envelope).await?;
        let kind: i64 = row.try_get(1)?;
        if compactable(kind) {
            sqlx::query(
                "UPDATE sync_ops SET envelope = x''
                 WHERE space_id = ? AND writer_id = ? AND seq = ?",
            )
            .bind(space.as_bytes().as_slice())
            .bind(lane.writer.as_bytes().as_slice())
            .bind(seq_i64(seq)?)
            .execute(&mut *conn)
            .await?;
            report.compacted += 1;
        }
        expected += 1;
    }
    if expected != last + 1 {
        return Err(corrupt("a stored chain has a gap below its head"));
    }
    sqlx::query("UPDATE sync_writers SET compacted_seq = ? WHERE space_id = ? AND writer_id = ?")
        .bind(seq_i64(last)?)
        .bind(space.as_bytes().as_slice())
        .bind(lane.writer.as_bytes().as_slice())
        .execute(&mut *conn)
        .await?;
    lane.next = last + 1;
    Ok(rows.len())
}

/// Whether compaction drops the body of an operation of wire `kind`.
fn compactable(kind: i64) -> bool {
    kind == i64::from(OperationKind::Changes.as_u8())
        || kind == i64::from(OperationKind::Acknowledgement.as_u8())
}

/// Stores back every elided version operation `(writer, seq)` wrote, from its body. Returns how
/// many it stored.
async fn hydrate_versions(
    conn: &mut SqliteConnection,
    writer: WriterId,
    seq: Seq,
    envelope: &[u8],
) -> SyncResult<usize> {
    if envelope.is_empty() {
        return Err(corrupt(
            "an operation above the compacted sequence has no body",
        ));
    }
    let operation = Envelope::decode(envelope)
        .ok()
        .and_then(|envelope| envelope.operation().ok())
        .ok_or_else(|| corrupt("an applied operation does not decode"))?;
    let Content::Changes(changes) = operation.content else {
        return Ok(0);
    };
    let mut hydrated = 0;
    for change in &changes {
        for group in &change.groups {
            let hash: Option<Vec<u8>> = sqlx::query_scalar(
                "SELECT value_hash FROM sync_register_versions
                 WHERE table_id = ? AND row_key = ? AND group_id = ? AND writer_id = ?
                    AND seq = ? AND value IS NULL",
            )
            .bind(table_param(change.table))
            .bind(change.row.as_str())
            .bind(i64::from(group.group.get()))
            .bind(writer.as_bytes().as_slice())
            .bind(seq_i64(seq)?)
            .fetch_optional(&mut *conn)
            .await?;
            let Some(hash) = hash else {
                continue;
            };
            if hash.as_slice() != group.hash.as_slice() {
                return Err(corrupt("an elided version does not match its operation"));
            }
            sqlx::query(
                "UPDATE sync_register_versions SET value = ?
                 WHERE table_id = ? AND row_key = ? AND group_id = ? AND writer_id = ?",
            )
            .bind(group.value.encode())
            .bind(table_param(change.table))
            .bind(change.row.as_str())
            .bind(i64::from(group.group.get()))
            .bind(writer.as_bytes().as_slice())
            .execute(&mut *conn)
            .await?;
            hydrated += 1;
        }
    }
    Ok(hydrated)
}
