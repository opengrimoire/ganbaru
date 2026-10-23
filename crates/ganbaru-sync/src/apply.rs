//! Applying stored operations in causal order.
//!
//! An operation is ready when its writer's previous operation is applied, its causal context is
//! known and applied, and it is not held. Ready operations apply in clock order within bounded
//! transactions. Local captures of the rows an operation or a materialization changes are sealed
//! first, so a materialized merge never overwrites a local edit that is not sealed yet.

use std::collections::BTreeSet;

use ganbaru_sync_contracts::{
    Change, ChangeAction, Content, Envelope, Hlc, OpId, Revocation, Seq, TableId, VersionVector,
    WriterId, hlc,
};
use sqlx::sqlite::SqliteRow;
use sqlx::{Connection, Row, SqliteConnection};

use crate::error::{SyncError, SyncResult, corrupt, failpoint};
use crate::hooks::ChangeNotice;
use crate::log::{self, seq_i64, stored_u64};
use crate::materialize::Materialized;
use crate::merge::{self, Batch};
use crate::seal::{self, SealReport, SealSlot};
use crate::visibility::{RowRef, Visibility};
use crate::{Engine, LocalWriter, SpaceContext, hold, hydrate, materialize, repair, sql, store};

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
    /// Changes the registered hooks reported.
    pub notices: Vec<ChangeNotice>,
    /// Writers with clocks far ahead of local time.
    pub clock_warnings: Vec<ClockWarning>,
    /// The local writer must be re-sealed before a revocation of it can apply.
    pub reseal_needed: Option<ResealNeeded>,
    /// Revocations of remote writers that cut off operations this replica applied. They stay
    /// stored and block their writer until the conflict is resolved.
    pub blocked_revocations: Vec<OpId>,
    /// Local captures of rows the call would change could not be sealed, so the operations
    /// changing them did not apply, or nothing applied when a merge would overwrite them.
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
    /// It touches rows with local captures and no writer can seal them first.
    PendingCaptures,
}

/// What [`apply_one`] needs to seal local captures before a remote operation changes them.
struct Sealing<'s, 'a, 'k> {
    slot: &'s mut SealSlot<'a, 'k>,
    /// Writers whose applied sequence is not written to `sync_writers` yet.
    advanced: &'s mut BTreeSet<WriterId>,
    report: &'s mut SealReport,
    now_ms: u64,
}

impl Sealing<'_, '_, '_> {
    /// Seals the captures of `rows`, held deletes included. Returns false when rows are
    /// captured and no writer can seal them, or they exceed what the call may still seal.
    async fn seal_rows(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        rows: BTreeSet<RowRef>,
    ) -> SyncResult<bool> {
        let rows = seal::captured(conn, rows).await?;
        if rows.is_empty() {
            return Ok(true);
        }
        if !self.slot.admit(engine, conn, &rows).await? {
            return Ok(false);
        }
        let Some((sealer, writer)) = self
            .slot
            .try_open(engine, conn, ctx, batch, self.now_ms)
            .await?
        else {
            return Ok(false);
        };
        flush_applied(conn, ctx, batch, self.advanced).await?;
        sealer
            .seal_rows(
                engine,
                conn,
                ctx,
                batch,
                writer,
                &rows,
                self.now_ms,
                self.report,
            )
            .await?;
        Ok(true)
    }

    /// Seals every capture before the local writer is revoked, so the revocation sees every
    /// local edit this replica made with it. Holds end here, because a revoked writer can never
    /// seal a held delete later.
    async fn seal_all(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
    ) -> SyncResult<()> {
        let captured: Option<i64> = sqlx::query_scalar("SELECT 1 FROM sync_capture LIMIT 1")
            .fetch_optional(&mut *conn)
            .await?;
        if captured.is_none() {
            return Ok(());
        }
        let Some((sealer, writer)) = self
            .slot
            .try_open(engine, conn, ctx, batch, self.now_ms)
            .await?
        else {
            return Ok(());
        };
        flush_applied(conn, ctx, batch, self.advanced).await?;
        hold::expire_holds(conn).await?;
        let sealed = sealer
            .seal_captures(engine, conn, ctx, batch, writer, self.now_ms)
            .await?;
        self.report.absorb(sealed);
        Ok(())
    }
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
    // Compaction only drops bodies of applied operations, so a waiting one without a body is
    // corrupt state rather than an invalid operation to hold.
    if row.try_get::<i64, _>(5)? == 0 {
        return Err(corrupt("a waiting operation has no body"));
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
    let bytes: Vec<u8> = sqlx::query_scalar(
        "SELECT envelope FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq = ?",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .fetch_one(&mut *conn)
    .await?;
    if bytes.is_empty() {
        return Err(corrupt("a waiting operation has no body"));
    }
    Ok(bytes)
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
    sealing: &mut Sealing<'_, '_, '_>,
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
        Content::Genesis(_) | Content::Acknowledgement => Ok(Outcome::Applied),
        Content::Authorization(_) => Ok(Outcome::Held),
        Content::Changes(changes) => {
            if !rows_exist(engine, conn, changes).await? {
                return Ok(Outcome::Held);
            }
            // The rows it changes and the rows it names, so a reference never materializes to
            // a row whose local delete is still held.
            let touched = changes
                .iter()
                .flat_map(|change| {
                    std::iter::once((change.table, change.row.as_str().to_string()))
                        .chain(seal::change_dependencies(engine, change))
                })
                .collect();
            if !sealing.seal_rows(engine, conn, ctx, batch, touched).await? {
                return Ok(Outcome::PendingCaptures);
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
            // Store refuses these already. One stored while another device was the coordinator
            // stays waiting without revoking, so it applies only if that device coordinates again.
            let issuer = log::writer(conn, ctx.space, candidate.writer)
                .await?
                .ok_or_else(|| corrupt("operation of an unknown writer reached apply"))?;
            if !store::may_issue(ctx, &issuer, envelope.kind) {
                return Ok(Outcome::Blocked);
            }
            if sealing.slot.local_id() == Some(revocation.writer) {
                sealing.seal_all(engine, conn, ctx, batch).await?;
            }
            apply_revoke(
                conn,
                batch,
                sealing.slot,
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

/// How [`finish`] treats the captures of a table whose constraint reads several rows together
/// (a unique column set, a tree, or acyclic edges) when the merge changes that table. Repairs
/// read merge state only, so such captures must be sealed before repairs see the table.
/// A bounded [`SealSlot`] limits what this sealing may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Constrained {
    /// Seal them before repairs.
    Seal,
    /// Leave them, for a local seal: it seals the writes and tombstones of such a table before
    /// any create of it, so sealed state stays valid for the table.
    Skip,
}

/// Tables among `tables` whose constraint reads several rows together.
fn constrained_tables(engine: &Engine, tables: impl Iterator<Item = TableId>) -> BTreeSet<TableId> {
    tables
        .filter(|table| {
            engine
                .manifest()
                .table(*table)
                .is_some_and(|spec| spec.constrains_rows_together())
        })
        .collect()
}

/// Seals the captures of `rows` for [`finish`]. Returns false when no writer can seal them or
/// they exceed what a bounded slot may still seal.
#[allow(clippy::too_many_arguments)]
async fn seal_for_finish(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
    slot: &mut SealSlot<'_, '_>,
    rows: &BTreeSet<RowRef>,
    now_ms: u64,
    report: &mut SealReport,
) -> SyncResult<bool> {
    if !slot.admit(engine, conn, rows).await? {
        return Ok(false);
    }
    let Some((sealer, writer)) = slot.try_open(engine, conn, ctx, batch, now_ms).await? else {
        return Ok(false);
    };
    sealer
        .seal_rows(engine, conn, ctx, batch, writer, rows, now_ms, report)
        .await?;
    Ok(true)
}

/// Seals repairs for hidden duplicates, detached tree rows, and cascades, refreshes row flags,
/// and materializes every dirty row, running the registered hooks. Returns what it changed.
///
/// A materialization never overwrites a local edit that is not sealed: captured rows it would
/// write, or cascade to, are sealed first and the merge is planned again. Captures of dirty
/// tables are sealed before repairs as `constrained` says. Without a writer that can seal them,
/// or when they exceed what a bounded slot may still seal, it returns `None` and writes
/// nothing, so the caller rolls back.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn finish(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
    slot: &mut SealSlot<'_, '_>,
    constrained: Constrained,
    now_ms: u64,
    report: &mut SealReport,
) -> SyncResult<Option<Materialized>> {
    if batch.dirty.is_empty() {
        return Ok(Some(Materialized::default()));
    }
    // Captured rows already sealed once. A capture that stays is an invalid row, which the
    // merge overwrites as it would without the capture. Captures only shrink inside the
    // transaction, so the loop ends.
    let mut attempted = BTreeSet::new();
    loop {
        if constrained != Constrained::Skip {
            let tables = constrained_tables(engine, batch.dirty.iter().map(|(table, _)| *table));
            let mut rows = seal::captured_in(conn, &tables).await?;
            rows.retain(|row| !attempted.contains(row));
            if !rows.is_empty() {
                if !seal_for_finish(engine, conn, ctx, batch, slot, &rows, now_ms, report).await? {
                    return Ok(None);
                }
                attempted.extend(rows);
            }
        }
        let mut repaired = false;
        let mut visibility = loop {
            let mut visibility = Visibility::load(engine, conn, &mut batch.dirty).await?;
            repair::expand_references(engine, conn, batch, &mut visibility).await?;
            let repairs =
                repair::repair_changes(engine, conn, &mut visibility, &batch.dirty).await?;
            if repaired || repairs.is_empty() {
                break visibility;
            }
            repaired = true;
            let Some((sealer, writer)) = slot.try_open(engine, conn, ctx, batch, now_ms).await?
            else {
                break visibility;
            };
            sealer
                .seal_changes(
                    engine, conn, ctx, batch, writer, repairs, None, now_ms, report,
                )
                .await?;
        };
        let merged = merge::refresh_flags(engine, conn, batch, &mut visibility).await?;
        let plan = materialize::plan(engine, conn, batch, &mut visibility, merged).await?;
        let pending = seal::captured(conn, plan.rows(engine)).await?;
        let mut captured = pending.clone();
        if constrained != Constrained::Skip {
            let tables = constrained_tables(engine, plan.rows(engine).map(|(table, _)| table));
            captured.extend(seal::captured_in(conn, &tables).await?);
        }
        captured.retain(|row| !attempted.contains(row));
        if captured.is_empty() {
            // Rows whose capture stays keep their values: the capture may still be sealed.
            let elisions = plan.elisions(engine, &pending);
            let changed = materialize::execute(engine, conn, plan).await?;
            hydrate::elide(conn, &elisions).await?;
            failpoint!("elide.before_commit");
            batch.dirty.clear();
            return Ok(Some(changed));
        }
        if !seal_for_finish(engine, conn, ctx, batch, slot, &captured, now_ms, report).await? {
            return Ok(None);
        }
        attempted.extend(captured);
    }
}

/// [`finish`] for a call that seals with its writer, where captured rows left unsealed mean
/// the writer is no longer active. Its slot is never bounded.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn finish_sealed(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
    slot: &mut SealSlot<'_, '_>,
    constrained: Constrained,
    now_ms: u64,
    report: &mut SealReport,
) -> SyncResult<Materialized> {
    match finish(engine, conn, ctx, batch, slot, constrained, now_ms, report).await? {
        Some(changed) => Ok(changed),
        None => Err(match slot.local_id() {
            Some(writer) => SyncError::WriterInactive(writer),
            None => corrupt("a sealing call has no writer"),
        }),
    }
}

impl Engine {
    /// Applies ready operations in one bounded transaction.
    ///
    /// Local captures of the rows an operation changes, held deletes included, are sealed
    /// before it applies. Every capture of a changed table whose constraint reads several rows
    /// together is sealed before repairs, and captures of rows the merge would materialize are
    /// sealed before the domain tables are written, so a merge never overwrites a local edit
    /// that is not sealed and every repair sees the local edits it must account for. Other
    /// captures stay for [`Engine::seal`]. The call seals at most one seal transaction of
    /// captures. Without a local writer, when it is no longer active, or when the captures
    /// exceed that bound, an operation that touches captured rows stops the call before it
    /// applies, and a finish that would seal captured rows rolls the call back. The report then
    /// says captures are pending, and the caller seals them in batches and applies again.
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
        let mut slot = SealSlot::bounded(writer);
        let mut report = ApplyReport::default();
        let mut sealed = SealReport::default();
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
            let mut sealing = Sealing {
                slot: &mut slot,
                advanced: &mut advanced,
                report: &mut sealed,
                now_ms,
            };
            match apply_one(
                self,
                &mut tx,
                ctx,
                &mut batch,
                &mut sealing,
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
                Outcome::PendingCaptures => {
                    report.pending_captures = true;
                    blocked.insert(candidate.writer);
                }
            }
        }
        flush_applied(&mut tx, ctx, &batch, &mut advanced).await?;
        let Some(changed) = finish(
            self,
            &mut tx,
            ctx,
            &mut batch,
            &mut slot,
            Constrained::Seal,
            now_ms,
            &mut sealed,
        )
        .await?
        else {
            drop(tx);
            return Ok(ApplyReport {
                pending_captures: true,
                local_writer_frozen: slot.frozen(),
                ..ApplyReport::default()
            });
        };
        report.changed_tables = changed.tables;
        report.notices = changed.notices;
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
