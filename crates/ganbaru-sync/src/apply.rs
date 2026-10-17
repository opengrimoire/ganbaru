//! Applying stored operations in causal order.
//!
//! An operation is ready when its writer's previous operation is applied, its causal context is
//! known and applied, and it is not held. Ready operations apply in clock order within bounded
//! transactions. Each transaction first seals pending local captures, so a materialized merge
//! never overwrites a local edit that is not sealed yet.

use std::collections::BTreeSet;

use ganbaru_sync_contracts::{
    Change, ChangeAction, Content, Envelope, Hlc, OpId, Revocation, Seq, TableId, VersionVector,
    WriterId, hlc,
};
use sqlx::sqlite::SqliteRow;
use sqlx::{Connection, Row, SqliteConnection};

use crate::error::{SyncResult, corrupt, failpoint};
use crate::log::{self, seq_i64, stored_u64};
use crate::merge::{self, Batch};
use crate::seal::{SealReport, SealSlot};
use crate::{Engine, LocalWriter, SpaceContext, materialize, repair, sql, store};

/// Most operations one apply transaction takes.
pub const MAX_OPS_PER_APPLY: usize = 256;
/// Most envelope bytes one apply transaction takes after its first operation.
pub const MAX_BYTES_PER_APPLY: usize = 1 << 20;
/// Largest envelope read ahead with its writer's next candidate. Each writer holds at most one.
const PREFETCH_ENVELOPE_BYTES: usize = 64 * 1024;

/// A writer whose operations carry clocks far ahead of local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockWarning {
    /// Writer of the operations.
    pub writer: WriterId,
    /// Greatest clock seen from it in the call.
    pub clock: Hlc,
}

/// A revocation of the local writer cuts off operations this replica already applied, so the
/// local writer's operations past `keep_through` must be re-sealed by a new writer before the
/// revocation can apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResealNeeded {
    /// The revoked local writer.
    pub writer: WriterId,
    /// Last operation of it that stays.
    pub keep_through: Seq,
}

/// Result of one apply transaction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyReport {
    /// Operations applied.
    pub applied: usize,
    /// Operations held as invalid in the call, including those past a revocation cutoff.
    pub held: usize,
    /// Domain tables the call changed.
    pub changed_tables: Vec<&'static str>,
    /// Writers with clocks far ahead of local time.
    pub clock_warnings: Vec<ClockWarning>,
    /// The local writer must be re-sealed before a revocation of it can apply.
    pub reseal_needed: Option<ResealNeeded>,
    /// Revocations of remote writers that cut off operations this replica applied. They stay
    /// stored and block their writer until the conflict is resolved.
    pub blocked_revocations: Vec<OpId>,
    /// Local captures could not be sealed, so nothing was applied.
    pub pending_captures: bool,
    /// More ready operations remain beyond the transaction budget.
    pub more: bool,
    /// Local captures and repairs sealed by the call.
    pub seal: Option<SealReport>,
    /// The local writer was found or became inactive; it cannot seal any more.
    pub local_writer_frozen: bool,
}

/// The next operation of a writer whose previous operation is applied.
struct Candidate {
    writer: WriterId,
    seq: Seq,
    clock: Hlc,
    context: Option<VersionVector>,
    bytes: usize,
    /// The envelope, when it was read with the candidate.
    envelope: Option<Vec<u8>>,
}

/// What applying one operation did.
enum Outcome {
    Applied,
    /// A revocation applied; it may hold operations of other writers.
    Revoked,
    Held,
    Blocked,
}

async fn candidates(conn: &mut SqliteConnection, ctx: &SpaceContext) -> SyncResult<Vec<Candidate>> {
    let rows = sqlx::query(
        "SELECT o.writer_id, o.seq, o.clock, o.context, o.state, length(o.envelope), NULL
         FROM sync_writers w JOIN sync_ops o
            ON o.space_id = w.space_id AND o.writer_id = w.writer_id AND o.seq = w.applied_seq + 1
         WHERE w.space_id = ? AND o.state <> 'held'
         ORDER BY o.clock, o.writer_id",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .fetch_all(&mut *conn)
    .await?;
    rows.iter().map(candidate_from_row).collect()
}

/// The candidate `seq` of `writer`, when it is stored and not held. A small envelope is read
/// with it, since the candidate usually applies next.
async fn writer_candidate(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: WriterId,
    seq: Seq,
) -> SyncResult<Option<Candidate>> {
    let row = sqlx::query(
        "SELECT writer_id, seq, clock, context, state, length(envelope),
            CASE WHEN length(envelope) <= ? THEN envelope END
         FROM sync_ops
         WHERE space_id = ? AND writer_id = ? AND seq = ? AND state <> 'held'",
    )
    .bind(i64::try_from(PREFETCH_ENVELOPE_BYTES).unwrap_or(i64::MAX))
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref().map(candidate_from_row).transpose()
}

/// A candidate from the columns `writer_id, seq, clock, context, state, length(envelope)`, and
/// the envelope or null.
fn candidate_from_row(row: &SqliteRow) -> SyncResult<Candidate> {
    let state: String = row.try_get(4)?;
    if state != "waiting" {
        return Err(corrupt("an operation past the applied sequence is applied"));
    }
    let writer: Vec<u8> = row.try_get(0)?;
    let context: Vec<u8> = row.try_get(3)?;
    Ok(Candidate {
        writer: log::writer_id(&writer)?,
        seq: stored_u64(row.try_get(1)?, "sequence")?,
        clock: Hlc::from_u64(stored_u64(row.try_get(2)?, "clock")?),
        context: log::decode_context(&context)?,
        bytes: usize::try_from(row.try_get::<i64, _>(5)?).unwrap_or(usize::MAX),
        envelope: row.try_get(6)?,
    })
}

async fn envelope_bytes(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: WriterId,
    seq: Seq,
) -> SyncResult<Vec<u8>> {
    Ok(sqlx::query_scalar(
        "SELECT envelope FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq = ?",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .fetch_one(&mut *conn)
    .await?)
}

async fn hold_invalid(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: WriterId,
    seq: Seq,
) -> SyncResult<()> {
    sqlx::query(
        "UPDATE sync_ops SET state = 'held', hold_reason = 'invalid'
         WHERE space_id = ? AND writer_id = ? AND seq = ?",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Derives and stores the context of a candidate whose context was unknown when it was stored.
/// `None` means its header cannot be read, so it must be held.
async fn resolve_context(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: WriterId,
    seq: Seq,
) -> SyncResult<Option<VersionVector>> {
    let bytes = envelope_bytes(conn, ctx, writer, seq).await?;
    let Ok(envelope) = Envelope::decode(&bytes) else {
        return Ok(None);
    };
    let classified = store::classify(engine, &envelope);
    let Some((_, dependencies)) = classified.header() else {
        return Ok(None);
    };
    let context = store::derive_context(conn, ctx, writer, seq, Some(dependencies))
        .await?
        .ok_or_else(|| corrupt("context of an applied operation is unknown"))?;
    sqlx::query("UPDATE sync_ops SET context = ? WHERE space_id = ? AND writer_id = ? AND seq = ?")
        .bind(context.encode())
        .bind(ctx.space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(seq)?)
        .execute(&mut *conn)
        .await?;
    Ok(Some(context))
}

/// Candidates of one transaction in clock order. Applying an operation only moves its writer's
/// candidate, so the list is refreshed per writer instead of read again for every operation.
struct Heads {
    candidates: Vec<Candidate>,
}

impl Heads {
    async fn load(conn: &mut SqliteConnection, ctx: &SpaceContext) -> SyncResult<Self> {
        Ok(Self {
            candidates: candidates(conn, ctx).await?,
        })
    }

    /// Takes the first ready candidate in clock order, holding candidates whose header cannot
    /// be read.
    async fn take_ready(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &Batch,
        blocked: &BTreeSet<WriterId>,
        report: &mut ApplyReport,
    ) -> SyncResult<Option<(Candidate, VersionVector)>> {
        let mut index = 0;
        while let Some(candidate) = self.candidates.get_mut(index) {
            if blocked.contains(&candidate.writer) {
                index += 1;
                continue;
            }
            if candidate.context.is_none() {
                let (writer, seq) = (candidate.writer, candidate.seq);
                match resolve_context(engine, conn, ctx, writer, seq).await? {
                    Some(context) => candidate.context = Some(context),
                    None => {
                        hold_invalid(conn, ctx, writer, seq).await?;
                        report.held += 1;
                        self.candidates.remove(index);
                        continue;
                    }
                }
            }
            if candidate
                .context
                .as_ref()
                .is_some_and(|context| batch.applied.dominates(context))
            {
                let mut candidate = self.candidates.remove(index);
                let context = candidate
                    .context
                    .take()
                    .ok_or_else(|| corrupt("a ready candidate lacks its context"))?;
                return Ok(Some((candidate, context)));
            }
            index += 1;
        }
        Ok(None)
    }

    /// Adds the next candidate of `writer` after its operation `seq` applied.
    async fn advance(
        &mut self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        writer: WriterId,
        seq: Seq,
    ) -> SyncResult<()> {
        let Some(next) = writer_candidate(conn, ctx, writer, seq + 1).await? else {
            return Ok(());
        };
        let position = self
            .candidates
            .partition_point(|other| (other.clock, other.writer) < (next.clock, next.writer));
        self.candidates.insert(position, next);
        Ok(())
    }
}

/// Whether every change names a known table and every write or tombstone a published row.
async fn rows_exist(
    engine: &Engine,
    conn: &mut SqliteConnection,
    changes: &[Change],
) -> SyncResult<bool> {
    let mut created: BTreeSet<(TableId, &str)> = BTreeSet::new();
    for change in changes {
        if engine.manifest().table(change.table).is_none() {
            return Ok(false);
        }
        let key = (change.table, change.row.as_str());
        match change.action {
            ChangeAction::Create => {
                created.insert(key);
            }
            ChangeAction::Write | ChangeAction::Tombstone => {
                if !created.contains(&key)
                    && merge::row_state(conn, change.table, change.row.as_str())
                        .await?
                        .is_none()
                {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
async fn apply_revoke(
    conn: &mut SqliteConnection,
    batch: &Batch,
    slot: &mut SealSlot<'_, '_>,
    id: OpId,
    context: &VersionVector,
    revocation: &Revocation,
    report: &mut ApplyReport,
) -> SyncResult<Outcome> {
    let target = revocation.writer;
    if revocation.cutoff < 1 || target == id.writer || !context.covers(&target, 1) {
        return Ok(Outcome::Held);
    }
    let local = slot.local_id() == Some(target);
    if batch.applied.get(&target) > revocation.cutoff {
        if local {
            report.reseal_needed = Some(ResealNeeded {
                writer: target,
                keep_through: revocation.cutoff,
            });
        } else {
            report.blocked_revocations.push(id);
        }
        return Ok(Outcome::Blocked);
    }
    let held = merge::revoke_effects(
        conn,
        batch.space,
        target,
        revocation.cutoff,
        revocation.reason,
    )
    .await?;
    report.held += usize::try_from(held).unwrap_or(usize::MAX);
    if local {
        slot.freeze();
        report.local_writer_frozen = true;
    }
    Ok(Outcome::Revoked)
}

#[allow(clippy::too_many_arguments)]
async fn apply_one(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
    slot: &mut SealSlot<'_, '_>,
    candidate: &mut Candidate,
    context: &VersionVector,
    report: &mut ApplyReport,
) -> SyncResult<Outcome> {
    let bytes = match candidate.envelope.take() {
        Some(bytes) => bytes,
        None => envelope_bytes(conn, ctx, candidate.writer, candidate.seq).await?,
    };
    let Ok(envelope) = Envelope::decode(&bytes) else {
        return Ok(Outcome::Held);
    };
    let Ok(operation) = envelope.operation() else {
        return Ok(Outcome::Held);
    };
    match &operation.content {
        Content::Genesis(_) => Ok(Outcome::Applied),
        Content::Changes(changes) => {
            if !rows_exist(engine, conn, changes).await? {
                return Ok(Outcome::Held);
            }
            for change in changes {
                let table = engine
                    .manifest()
                    .table(change.table)
                    .ok_or_else(|| corrupt("change of an unknown table reached apply"))?;
                merge::apply_change(
                    conn,
                    batch,
                    candidate.writer,
                    candidate.seq,
                    operation.header.clock,
                    context,
                    table,
                    change,
                )
                .await?;
            }
            Ok(Outcome::Applied)
        }
        Content::Revoke(revocation) => {
            apply_revoke(
                conn,
                batch,
                slot,
                operation.id(),
                context,
                revocation,
                report,
            )
            .await
        }
    }
}

/// Marks an operation applied. Its writer's `sync_writers.applied_seq` is written by
/// [`flush_applied`].
async fn mark_applied(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
    candidate: &Candidate,
    context: VersionVector,
) -> SyncResult<()> {
    sqlx::query(
        "UPDATE sync_ops SET state = 'applied', context = ?
         WHERE space_id = ? AND writer_id = ? AND seq = ?",
    )
    .bind(context.encode())
    .bind(ctx.space.as_bytes().as_slice())
    .bind(candidate.writer.as_bytes().as_slice())
    .bind(seq_i64(candidate.seq)?)
    .execute(&mut *conn)
    .await?;
    batch.applied.set(candidate.writer, candidate.seq);
    batch
        .contexts
        .insert(candidate.writer, candidate.seq, context);
    Ok(())
}

/// Writes the applied sequence of each writer in `advanced` from the batch and clears it.
async fn flush_applied(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &Batch,
    advanced: &mut BTreeSet<WriterId>,
) -> SyncResult<()> {
    for writer in std::mem::take(advanced) {
        sqlx::query("UPDATE sync_writers SET applied_seq = ? WHERE space_id = ? AND writer_id = ?")
            .bind(seq_i64(batch.applied.get(&writer))?)
            .bind(ctx.space.as_bytes().as_slice())
            .bind(writer.as_bytes().as_slice())
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

fn warn_clock(report: &mut ApplyReport, writer: WriterId, clock: Hlc, now_ms: u64) {
    if !hlc::is_far_ahead(clock, now_ms) {
        return;
    }
    match report
        .clock_warnings
        .iter_mut()
        .find(|warning| warning.writer == writer)
    {
        Some(warning) => warning.clock = warning.clock.max(clock),
        None => report.clock_warnings.push(ClockWarning { writer, clock }),
    }
}

/// Seals repairs for hidden duplicates, refreshes row flags, and materializes every dirty row.
/// Returns the domain tables it changed.
pub(crate) async fn finish(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
    slot: &mut SealSlot<'_, '_>,
    now_ms: u64,
    report: &mut SealReport,
) -> SyncResult<Vec<&'static str>> {
    if batch.dirty.is_empty() {
        return Ok(Vec::new());
    }
    let mut repaired = false;
    let hidden = loop {
        let hidden = repair::hidden_rows(engine, conn, batch).await?;
        repair::expand_references(engine, conn, batch).await?;
        let repairs = repair::repair_changes(engine, &hidden)?;
        if repaired || repairs.is_empty() {
            break hidden;
        }
        repaired = true;
        let Some((sealer, writer)) = slot.try_open(engine, conn, ctx, batch, now_ms).await? else {
            break hidden;
        };
        sealer
            .seal_changes(
                engine, conn, ctx, batch, writer, repairs, None, now_ms, report,
            )
            .await?;
    };
    let merged = merge::refresh_flags(engine, conn, batch).await?;
    let changed = materialize::materialize(engine, conn, batch, &hidden, merged).await?;
    batch.dirty.clear();
    Ok(changed)
}

impl Engine {
    /// Applies ready operations in one bounded transaction, sealing local captures first.
    ///
    /// Without a local writer, or when it is no longer active, pending captures stop the call
    /// before anything applies, because materialized merges would overwrite those local edits.
    pub async fn apply(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        writer: Option<&mut LocalWriter<'_>>,
        now_ms: u64,
    ) -> SyncResult<ApplyReport> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        sql::begin_engine_write(&mut tx).await?;
        let mut batch = Batch::load(&mut tx, ctx.space).await?;
        let mut slot = SealSlot::new(writer);
        let mut report = ApplyReport::default();
        let mut sealed = SealReport::default();
        let captures: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_capture")
            .fetch_one(&mut *tx)
            .await?;
        if captures > 0 {
            match slot
                .try_open(self, &mut tx, ctx, &mut batch, now_ms)
                .await?
            {
                Some((sealer, writer)) => {
                    sealed = sealer
                        .seal_captures(self, &mut tx, ctx, &mut batch, writer, now_ms)
                        .await?;
                }
                None => {
                    report.pending_captures = true;
                    report.local_writer_frozen = slot.frozen();
                    return Ok(report);
                }
            }
        }
        let mut blocked = BTreeSet::new();
        let mut taken = 0usize;
        let mut bytes = 0usize;
        let mut heads = Heads::load(&mut tx, ctx).await?;
        // Writers whose applied sequence moved past `sync_writers`, written before anything
        // reads it again.
        let mut advanced = BTreeSet::new();
        while let Some((mut candidate, context)) = heads
            .take_ready(self, &mut tx, ctx, &batch, &blocked, &mut report)
            .await?
        {
            if taken >= MAX_OPS_PER_APPLY
                || (taken > 0 && bytes.saturating_add(candidate.bytes) > MAX_BYTES_PER_APPLY)
            {
                report.more = true;
                break;
            }
            taken += 1;
            bytes = bytes.saturating_add(candidate.bytes);
            match apply_one(
                self,
                &mut tx,
                ctx,
                &mut batch,
                &mut slot,
                &mut candidate,
                &context,
                &mut report,
            )
            .await?
            {
                outcome @ (Outcome::Applied | Outcome::Revoked) => {
                    mark_applied(&mut tx, ctx, &mut batch, &candidate, context).await?;
                    warn_clock(&mut report, candidate.writer, candidate.clock, now_ms);
                    report.applied += 1;
                    advanced.insert(candidate.writer);
                    if matches!(outcome, Outcome::Revoked) {
                        flush_applied(&mut tx, ctx, &batch, &mut advanced).await?;
                        heads = Heads::load(&mut tx, ctx).await?;
                    } else {
                        heads
                            .advance(&mut tx, ctx, candidate.writer, candidate.seq)
                            .await?;
                    }
                }
                Outcome::Held => {
                    hold_invalid(&mut tx, ctx, candidate.writer, candidate.seq).await?;
                    report.held += 1;
                }
                Outcome::Blocked => {
                    blocked.insert(candidate.writer);
                }
            }
        }
        flush_applied(&mut tx, ctx, &batch, &mut advanced).await?;
        report.changed_tables = finish(
            self,
            &mut tx,
            ctx,
            &mut batch,
            &mut slot,
            now_ms,
            &mut sealed,
        )
        .await?;
        report.local_writer_frozen |= slot.frozen();
        if !sealed.is_empty() {
            report.seal = Some(sealed);
        }
        failpoint!("apply.before_commit");
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(report)
    }
}
