//! Applying stored operations in causal order.
//!
//! An operation is ready when its writer's previous operation is applied, its causal context is
//! known and applied, and it is not held. Ready operations apply in clock order within bounded
//! transactions. Local captures of the rows an operation or a materialization changes are sealed
//! first, so a materialized merge never overwrites a local edit that is not sealed yet.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::bounds::MAX_OPERATIONS_PER_CHAIN;
use ganbaru_sync_contracts::{
    AuthorizationRecord, Change, ChangeAction, Content, Envelope, Hlc, OperationKind, Seq, TableId,
    VersionVector, WriterId, ends_group, hlc,
};
use sqlx::sqlite::SqliteRow;
use sqlx::{Connection, Row, SqliteConnection};

use crate::error::{SyncError, SyncResult, corrupt, failpoint};
use crate::exclusion::{Excluding, Retention};
use crate::hooks::ChangeNotice;
use crate::log::{self, WriterState, seq_i64, stored_u64};
use crate::materialize::Materialized;
use crate::merge::{self, Batch};
use crate::seal::{self, SealReport, SealSlot};
use crate::store::HoldReason;
use crate::visibility::{RowRef, Visibility};
use crate::{
    Engine, LocalWriter, SpaceContext, authorization, exclusion, fork, hydrate, materialize,
    repair, sql, store,
};

/// Most operations one apply transaction takes, so a batch of single-change operations stays
/// within the 50 ms desktop apply budget.
///
/// Apply cost grows with each operation and each change it carries, at roughly 1 to 2 ms per
/// single-change Projects operation in release builds on an in-memory replica, against a small
/// fixed cost per transaction. Operations without changes still count, since an
/// authorization entry reloads the writer heads and retention.
pub const MAX_OPS_PER_APPLY: usize = 16;
/// Changes after which one apply transaction takes no further operation. The operation that
/// crosses it still applies whole, so a transaction takes at least one operation however many
/// changes it carries.
///
/// A sealed operation carries at most this many changes, unless its changes hold only whole:
/// writes and tombstones of a table whose constraint reads several rows together, cascaded
/// deletions, the changes they depend on, and repairs. Those fill operations up to the wire
/// limits and form one apply group, which the header of each operation but the last marks as
/// continuing. A group starts only once every operation of it is stored and ready, and then
/// applies whole past every cap, so repairs never see part of it; see
/// [`ganbaru_sync_contracts::ends_group`] for its bounds. Changes past those bounds apply as a
/// chain of groups once the whole chain is stored, and its repairs are sealed once after its
/// last group, see [`MAX_OPERATIONS_PER_CHAIN`]. The transaction may also seal local
/// changes of the rows the remote changes touch, within [`crate::MAX_CHANGES_PER_SEAL`].
pub const MAX_CHANGES_PER_APPLY: usize = 16;
/// Most envelope bytes one apply transaction takes after its first operation.
pub const MAX_BYTES_PER_APPLY: usize = 1 << 20;
/// Largest envelope read ahead with its writer's next candidate. Each writer holds at most one.
const PREFETCH_ENVELOPE_BYTES: usize = 64 * 1024;
/// Most listed stale rows one engine write takes back, so the rows a chain deferred are
/// repaired and materialized over several bounded writes. The write also plans the rows whose
/// references reach a taken row again, through referrers that are hidden or missing from the
/// domain, as any write that changes the row does, see [`repair::expand_references`]. Those
/// referrers are not bounded here: deferring some of them would materialize a row without the
/// rows that follow it. Their repairs seal as one group in as many operations as they need.
pub(crate) const MAX_STALE_ROWS_PER_WRITE: usize = crate::MAX_CHANGES_PER_SEAL;

/// A writer whose operations carry clocks far ahead of local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockWarning {
    /// Writer of the operations.
    pub writer: WriterId,
    /// Greatest clock seen from it in the call.
    pub clock: Hlc,
}

/// Result of one apply transaction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyReport {
    /// Operations applied.
    pub applied: usize,
    /// Changes the applied operations carried.
    pub changes: usize,
    /// Operations held as invalid in the call.
    pub held: usize,
    /// Domain tables the call changed.
    pub changed_tables: Vec<&'static str>,
    /// Changes the registered hooks reported.
    pub notices: Vec<ChangeNotice>,
    /// Writers with clocks far ahead of local time.
    pub clock_warnings: Vec<ClockWarning>,
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
    /// Whether its header says the writer's next operation joins its apply group.
    continues: bool,
}

/// What applying one operation did.
enum Outcome {
    Applied,
    /// An authorization entry applied; it may change which writers and revisions are ready.
    Authorized,
    /// An authorization entry recorded whose selection excludes the operation carrying it.
    Excluded,
    Held,
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
}

/// Waiting operations whose writer's previous operation is applied, that are within their
/// writer's cutoff, whose writer's device the selected chain enrolls, and whose authorization
/// revision a stored chain reaches. A device a reselection leaves unenrolled keeps what applied
/// and its later operations wait until a selected chain enrolls it again.
async fn candidates(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    revision: u64,
) -> SyncResult<Vec<Candidate>> {
    let rows = sqlx::query(
        "SELECT o.writer_id, o.seq, o.clock, o.context, o.state, length(o.envelope), NULL,
            o.group_continues
         FROM sync_writers w JOIN sync_ops o
            ON o.space_id = w.space_id AND o.writer_id = w.writer_id AND o.seq = w.applied_seq + 1
         WHERE w.space_id = ? AND o.state <> 'held' AND o.authorization_revision <= ?
            AND (w.cutoff_seq IS NULL OR o.seq <= w.cutoff_seq)
            AND EXISTS (
                SELECT 1 FROM sync_devices d
                WHERE d.space_id = w.space_id AND d.device_id = w.device_id
            )
         ORDER BY o.clock, o.writer_id",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(seq_i64(revision)?)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter().map(candidate_from_row).collect()
}

/// The candidate `seq` of `writer`, when it is stored, not held, within the writer's cutoff,
/// of an enrolled device, and sealed against a reachable revision. A small envelope is read
/// with it, since the candidate usually applies next.
async fn writer_candidate(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: WriterId,
    seq: Seq,
    revision: u64,
) -> SyncResult<Option<Candidate>> {
    let row = sqlx::query(
        "SELECT writer_id, seq, clock, context, state, length(envelope),
            CASE WHEN length(envelope) <= ? THEN envelope END, group_continues
         FROM sync_ops o
         WHERE space_id = ? AND writer_id = ? AND seq = ? AND state <> 'held'
            AND authorization_revision <= ?
            AND NOT EXISTS (
                SELECT 1 FROM sync_writers w
                WHERE w.space_id = o.space_id AND w.writer_id = o.writer_id
                    AND ((w.cutoff_seq IS NOT NULL AND o.seq > w.cutoff_seq)
                        OR NOT EXISTS (
                            SELECT 1 FROM sync_devices d
                            WHERE d.space_id = w.space_id AND d.device_id = w.device_id
                        ))
            )",
    )
    .bind(i64::try_from(PREFETCH_ENVELOPE_BYTES).unwrap_or(i64::MAX))
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .bind(seq_i64(revision)?)
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref().map(candidate_from_row).transpose()
}

/// A candidate from the columns `writer_id, seq, clock, context, state, length(envelope)`, the
/// envelope or null, and `group_continues`.
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
        continues: row.try_get(7)?,
    })
}

/// How the apply group a candidate starts stands.
enum GroupScan {
    /// The chain the candidate belongs to is closed, and every operation of it from the
    /// candidate on is stored and ready in turn. Holds the operations after the candidate that
    /// complete its group, in sequence order, and, when the group opens a chain that later
    /// groups finish, when the last operation of that chain was stored.
    Complete {
        rest: Vec<(Candidate, VersionVector)>,
        opens: Option<u64>,
    },
    /// The chain is not closed yet, or an operation of it waits.
    Waiting {
        /// It waits on an operation of another writer, which the call may still apply.
        on_others: bool,
    },
    /// The operation at this sequence can never apply inside a chain: it is not a changes or
    /// acknowledgement operation, or its header cannot be read.
    Invalid(Seq),
}

/// Operations of `writer`'s chain before its operation `seq`. A chain starts after the
/// writer's last operation before `seq` whose flag does not continue it, and again after
/// every [`MAX_OPERATIONS_PER_CHAIN`] operations, so every receiver cuts the same chains.
async fn chain_position(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: WriterId,
    seq: Seq,
) -> SyncResult<usize> {
    let end: Option<i64> = sqlx::query_scalar(
        "SELECT seq FROM sync_ops
         WHERE space_id = ? AND writer_id = ? AND seq < ? AND group_continues = 0
         ORDER BY seq DESC LIMIT 1",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .fetch_optional(&mut *conn)
    .await?;
    let end = end
        .map(|end| stored_u64(end, "sequence"))
        .transpose()?
        .unwrap_or(0);
    let run = seq.saturating_sub(end + 1);
    let bound = u64::try_from(MAX_OPERATIONS_PER_CHAIN).unwrap_or(u64::MAX);
    Ok(usize::try_from(run % bound).unwrap_or(0))
}

/// Up to `limit` stored operations of `writer` after `after`, in sequence order, with the
/// columns `seq, kind, state, hold_reason, excluded_revision IS NOT NULL, group_continues,
/// length(envelope), clock, context, authorization_revision, stored_order`.
async fn chain_rows(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: WriterId,
    after: Seq,
    limit: usize,
) -> SyncResult<Vec<SqliteRow>> {
    Ok(sqlx::query(
        "SELECT seq, kind, state, hold_reason, excluded_revision IS NOT NULL, group_continues,
            length(envelope), clock, context, authorization_revision, stored_order
         FROM sync_ops
         WHERE space_id = ? AND writer_id = ? AND seq > ?
         ORDER BY seq LIMIT ?",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(after)?)
    .bind(i64::try_from(limit).unwrap_or(i64::MAX))
    .fetch_all(&mut *conn)
    .await?)
}

/// How many of `rows`, the stored operations after `after` read by [`chain_rows`], belong to
/// a chain that has room for `room` more operations, or `None` while the chain is not closed.
///
/// The chain closes at an operation whose flag does not continue it, at its last allowed
/// operation, and before the writer's cutoff, an operation the selected state excludes, or an
/// operation held as invalid, since none of them ever applies. A missing operation or one held
/// for a newer format or manifest leaves the chain open: it may still arrive or apply.
fn chain_extent(
    rows: &[SqliteRow],
    after: Seq,
    room: usize,
    cutoff: Option<Seq>,
) -> SyncResult<Option<usize>> {
    let past_cutoff = |seq: Seq| cutoff.is_some_and(|cutoff| seq > cutoff);
    let mut expected = after;
    for (count, row) in rows.iter().take(room).enumerate() {
        expected += 1;
        // A cutoff closes the chain though nothing past it is stored: a removed device's later
        // operations may never arrive.
        if past_cutoff(expected) {
            return Ok(Some(count));
        }
        if stored_u64(row.try_get(0)?, "sequence")? != expected {
            return Ok(None);
        }
        let state: String = row.try_get(2)?;
        if state == "held" {
            let excluded: bool = row.try_get(4)?;
            let reason: Option<String> = row.try_get(3)?;
            let invalid = reason.as_deref() == Some(HoldReason::Invalid.as_str());
            return Ok((excluded || invalid).then_some(count));
        }
        if !row.try_get::<bool, _>(5)? {
            return Ok(Some(count + 1));
        }
    }
    if rows.len() >= room {
        return Ok(Some(room));
    }
    Ok(past_cutoff(expected + 1).then_some(rows.len()))
}

/// Reads the chain `first` belongs to and the rest of the apply group it starts, see
/// [`ends_group`] and [`MAX_OPERATIONS_PER_CHAIN`]. No group of a chain starts before the
/// whole chain is stored and closed, so a chain that never closes applies nothing, and once
/// one group applied, the next apply calls finish the chain. Each operation of the chain
/// must be ready once the ones before it applied, without operations of other writers between
/// them, and sealed against `revision`, the revision the candidates were read against.
async fn scan_group(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &Batch,
    first: &Candidate,
    (revision, excluding): (u64, &BTreeMap<WriterId, Excluding>),
) -> SyncResult<GroupScan> {
    let position = chain_position(conn, ctx, first.writer, first.seq).await?;
    let room = MAX_OPERATIONS_PER_CHAIN - position - 1;
    if !first.continues || room == 0 {
        return Ok(GroupScan::Complete {
            rest: Vec::new(),
            opens: None,
        });
    }
    let cutoff = log::writer(conn, ctx.space, first.writer)
        .await?
        .ok_or_else(|| corrupt("operation of an unknown writer reached apply"))?
        .cutoff;
    let rows = chain_rows(conn, ctx, first.writer, first.seq, room).await?;
    let Some(extent) = chain_extent(&rows, first.seq, room, cutoff)? else {
        return Ok(GroupScan::Waiting { on_others: false });
    };
    let mut rest = Vec::new();
    let mut applied = batch.applied.clone();
    applied.set(first.writer, first.seq);
    let mut grouping = !ends_group(first.continues, 1, first.bytes);
    let mut operations = 1usize;
    let mut bytes = first.bytes;
    for row in &rows[..extent] {
        let seq = stored_u64(row.try_get(0)?, "sequence")?;
        let state: String = row.try_get(2)?;
        if state != "waiting" {
            return Err(corrupt("an operation past the applied sequence is applied"));
        }
        let kind = u8::try_from(row.try_get::<i64, _>(1)?)
            .ok()
            .and_then(OperationKind::from_u8);
        if !matches!(
            kind,
            Some(OperationKind::Changes | OperationKind::Acknowledgement)
        ) {
            return Ok(GroupScan::Invalid(seq));
        }
        let length = usize::try_from(row.try_get::<i64, _>(6)?).unwrap_or(usize::MAX);
        if length == 0 {
            return Err(corrupt("a waiting operation has no body"));
        }
        let stored: Vec<u8> = row.try_get(8)?;
        let context = match log::decode_context(&stored)? {
            Some(context) => context,
            None => match resolve_context(engine, conn, ctx, first.writer, seq).await? {
                Some(context) => context,
                None => return Ok(GroupScan::Invalid(seq)),
            },
        };
        let sealed_against = stored_u64(row.try_get(9)?, "authorization revision")?;
        if sealed_against > revision || !exclusion::ready(&applied, &context, excluding) {
            return Ok(GroupScan::Waiting { on_others: true });
        }
        applied.set(first.writer, seq);
        if !grouping {
            continue;
        }
        let continues: bool = row.try_get(5)?;
        let member = Candidate {
            writer: first.writer,
            seq,
            clock: Hlc::from_u64(stored_u64(row.try_get(7)?, "clock")?),
            context: None,
            bytes: length,
            envelope: None,
            continues,
        };
        rest.push((member, context));
        operations += 1;
        bytes = bytes.saturating_add(length);
        grouping = !ends_group(continues, operations, bytes);
    }
    let opens = if position == 0 && rest.len() < extent {
        let mut closed_at = 0;
        for row in &rows[..extent] {
            closed_at = closed_at.max(stored_u64(row.try_get(10)?, "storage order")?);
        }
        Some(closed_at)
    } else {
        None
    };
    Ok(GroupScan::Complete { rest, opens })
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
    /// Reachable authorization revision the candidates were read against.
    revision: u64,
    /// Writers whose operations past a removal cutoff are excluded.
    excluding: BTreeMap<WriterId, Excluding>,
}

impl Heads {
    async fn load(conn: &mut SqliteConnection, ctx: &SpaceContext) -> SyncResult<Self> {
        let revision = authorization::readiness_revision(conn, ctx.space).await?;
        Ok(Self {
            candidates: candidates(conn, ctx, revision).await?,
            revision,
            excluding: exclusion::excluding(conn, ctx.space).await?,
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
                .is_some_and(|context| exclusion::ready(&batch.applied, context, &self.excluding))
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
        let Some(next) = writer_candidate(conn, ctx, writer, seq + 1, self.revision).await? else {
            return Ok(());
        };
        let position = self
            .candidates
            .partition_point(|other| (other.clock, other.writer) < (next.clock, next.writer));
        self.candidates.insert(position, next);
        Ok(())
    }

    /// The first chain the candidates not in `blocked` could open now, as `(storage order of its
    /// last operation, writer)`. Chains open in that order instead of clock order, so a writer
    /// whose clocks run low cannot open every chain before one that has waited longer: every
    /// receiver numbers what it stores itself, and a chain sent later is stored later, however
    /// the receiver's wall clock moves.
    async fn first_opener(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &Batch,
        blocked: &BTreeSet<WriterId>,
    ) -> SyncResult<Option<(u64, WriterId)>> {
        let mut first = None;
        for index in 0..self.candidates.len() {
            let candidate = &mut self.candidates[index];
            if !candidate.continues || blocked.contains(&candidate.writer) {
                continue;
            }
            // A candidate whose header cannot be read is held when it is taken.
            if candidate.context.is_none() {
                candidate.context =
                    resolve_context(engine, conn, ctx, candidate.writer, candidate.seq).await?;
            }
            let candidate = &self.candidates[index];
            let ready = candidate
                .context
                .as_ref()
                .is_some_and(|context| exclusion::ready(&batch.applied, context, &self.excluding));
            if !ready {
                continue;
            }
            let scan = scan_group(
                engine,
                conn,
                ctx,
                batch,
                candidate,
                (self.revision, &self.excluding),
            )
            .await?;
            if let GroupScan::Complete {
                opens: Some(closed_at),
                ..
            } = scan
            {
                let key = (closed_at, candidate.writer);
                first = Some(first.map_or(key, |earlier: (u64, WriterId)| earlier.min(key)));
            }
        }
        Ok(first)
    }
}

/// How far an operation's context reaches past the operations other replicas keep of excluded
/// writers.
#[derive(Debug, Clone, Copy)]
struct Reach {
    /// Past the bound of a removal, see [`exclusion::beyond_bound`].
    beyond: bool,
    /// Past the shared prefix of a forked writer, see [`exclusion::past_fork`].
    past_fork: bool,
}

/// Rows an operation writes or tombstones without knowing them.
#[derive(Debug, Default)]
struct Unknown<'a> {
    /// Rows only an operation past the bound of a removal created, whose changes are skipped.
    skipped: BTreeSet<(TableId, &'a str)>,
    /// Rows only a forked branch created, whose changes apply to a row that waits for a
    /// creation.
    deferred: BTreeSet<(TableId, &'a str)>,
}

/// The rows of `changes` that are written or tombstoned without being known, or `None` when the
/// operation must be held: a change names an unknown table, or writes an unknown row although
/// its context reaches no operation that other replicas drop. A row is known when it is
/// published, created earlier in the operation, or created by a late operation of a removed
/// device.
async fn admitted_rows<'a>(
    engine: &Engine,
    conn: &mut SqliteConnection,
    changes: &'a [Change],
    reach: Reach,
) -> SyncResult<Option<Unknown<'a>>> {
    let mut created: BTreeSet<(TableId, &str)> = BTreeSet::new();
    let mut unknown = Unknown::default();
    for change in changes {
        if engine.manifest().table(change.table).is_none() {
            return Ok(None);
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
                    && !exclusion::created_late(conn, change.table, change.row.as_str()).await?
                {
                    if reach.beyond {
                        unknown.skipped.insert(key);
                    } else if reach.past_fork {
                        unknown.deferred.insert(key);
                    } else {
                        return Ok(None);
                    }
                }
            }
        }
    }
    Ok(Some(unknown))
}

/// Excludes fork branches and records the entries of excluded operations, either of which may
/// select the chain again.
async fn reselect(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
) -> SyncResult<()> {
    fork::reconcile(engine, conn, ctx, batch).await?;
    exclusion::admit_excluded_entries(engine, conn, ctx, batch).await
}

#[allow(clippy::too_many_arguments)]
async fn apply_one(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
    sealing: &mut Sealing<'_, '_, '_>,
    candidate: &mut Candidate,
    (context, reach): (&VersionVector, Reach),
    retention: &mut Retention,
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
        Content::Changes(changes) => {
            let Some(unknown) = admitted_rows(engine, conn, changes, reach).await? else {
                return Ok(Outcome::Held);
            };
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
                let key = (change.table, change.row.as_str());
                if unknown.skipped.contains(&key) {
                    continue;
                }
                if unknown.deferred.contains(&key) {
                    merge::defer_creation(conn, change.table, change.row.as_str()).await?;
                }
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
                    retention
                        .retains(candidate.writer, candidate.seq)
                        .then_some(engine),
                )
                .await?;
            }
            report.changes += changes.len();
            Ok(Outcome::Applied)
        }
        Content::Authorization(record) => {
            // An entry may remove the local device or retire its writer, after which its
            // captures never seal and every later call would roll back over them, so they seal
            // first while the writer still can. Sealed, they become operations the entry
            // excludes and their values late changes.
            if sealing.slot.may_seal(conn, ctx).await? {
                let tables = engine
                    .manifest()
                    .tables
                    .iter()
                    .map(|table| table.id)
                    .collect();
                let captured = seal::captured_in(conn, &tables).await?;
                if !sealing
                    .seal_rows(engine, conn, ctx, batch, captured)
                    .await?
                {
                    return Ok(Outcome::PendingCaptures);
                }
            }
            retention.flush_released(conn, ctx.space).await?;
            apply_authorization(
                engine,
                conn,
                ctx,
                batch,
                sealing.slot,
                candidate,
                record,
                report,
            )
            .await
        }
    }
}

/// Records an authorization entry and selects the chain again. Only a writer of the device the
/// record names as anchor may carry entries; any other carrier is held. The entry is stored and
/// forwarded whether or not selection takes it, and the local writer freezes when the selected
/// chain removes its device. Operations the selected removals exclude, or no longer exclude,
/// change state in the same transaction. Entries of excluded operations, which never get here,
/// are recorded by [`exclusion::settle`] and at the start of each apply call.
#[allow(clippy::too_many_arguments)]
async fn apply_authorization(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
    slot: &mut SealSlot<'_, '_>,
    candidate: &Candidate,
    record: &AuthorizationRecord,
    report: &mut ApplyReport,
) -> SyncResult<Outcome> {
    let issuer = log::writer(conn, ctx.space, candidate.writer)
        .await?
        .ok_or_else(|| corrupt("operation of an unknown writer reached apply"))?;
    if !authorization::carries(&issuer.device_id, record) {
        return Ok(Outcome::Held);
    }
    authorization::insert_entry(conn, ctx.space, candidate.writer, candidate.seq, record).await?;
    if let Some(selection) = authorization::rederive(conn, ctx.space).await? {
        exclusion::settle(engine, conn, ctx, batch, &selection).await?;
    }
    // A local writer without a genesis yet is checked against its device's enrollment when the
    // slot first opens it.
    if let Some(local) = slot.local_id() {
        let inactive = log::writer(conn, ctx.space, local)
            .await?
            .is_some_and(|writer| writer.state != WriterState::Active);
        if inactive {
            slot.freeze();
            report.local_writer_frozen = true;
        }
    }
    // A chain the entry makes selectable may remove its own carrier's device, so the entry
    // excludes the operation that carries it and stays recorded like any excluded entry.
    if excluded(conn, ctx, candidate.writer, candidate.seq).await? {
        return Ok(Outcome::Excluded);
    }
    Ok(Outcome::Authorized)
}

/// Whether the stored operation `seq` of `writer` is excluded.
async fn excluded(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: WriterId,
    seq: Seq,
) -> SyncResult<bool> {
    let excluded: Option<bool> = sqlx::query_scalar(
        "SELECT excluded_revision IS NOT NULL FROM sync_ops
         WHERE space_id = ? AND writer_id = ? AND seq = ?",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(excluded.unwrap_or(false))
}

/// Marks an operation applied. Its writer's `sync_writers.applied_seq` is written by
/// [`flush_applied`].
async fn mark_applied(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
    candidate: &Candidate,
    context: VersionVector,
    retention: &mut Retention,
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
    retention.confirm(candidate.writer, &context);
    retention.release(candidate.writer, candidate.seq);
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

/// Whether a writer is between two groups of a chain: its last applied operation continues a
/// chain that the rest of its stored operations close, so the next apply calls finish the
/// chain. [`scan_group`] starts a chain only once it is closed and every operation of it is
/// ready, so this holds only while those calls run; a chain whose rest went missing or fell
/// past a cutoff counts as finished, so it cannot defer repairs for good. Reads
/// `sync_writers.applied_seq`, which every caller of [`finish`] has written.
async fn chain_resuming(conn: &mut SqliteConnection, ctx: &SpaceContext) -> SyncResult<bool> {
    let writers: Vec<(Vec<u8>, i64, Option<i64>)> = sqlx::query_as(
        "SELECT w.writer_id, w.applied_seq, w.cutoff_seq
         FROM sync_writers w JOIN sync_ops o
            ON o.space_id = w.space_id AND o.writer_id = w.writer_id AND o.seq = w.applied_seq
         WHERE w.space_id = ? AND o.group_continues = 1",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .fetch_all(&mut *conn)
    .await?;
    for (writer, applied, cutoff) in writers {
        let writer = log::writer_id(&writer)?;
        let applied = stored_u64(applied, "applied sequence")?;
        let cutoff = cutoff
            .map(|cutoff| stored_u64(cutoff, "cutoff"))
            .transpose()?;
        let position = chain_position(conn, ctx, writer, applied).await?;
        let room = MAX_OPERATIONS_PER_CHAIN - position - 1;
        if room == 0 {
            continue;
        }
        let rows = chain_rows(conn, ctx, writer, applied, room).await?;
        if chain_extent(&rows, applied, room, cutoff)?.is_some_and(|rest| rest > 0) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether repairs wait for a chain: a writer is between two groups of it, see
/// [`chain_resuming`], and `slot` may seal. A call that cannot seal repairs has none to defer,
/// so a replica without an active local writer never holds rows back for a chain.
async fn repairs_wait(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    slot: &mut SealSlot<'_, '_>,
) -> SyncResult<bool> {
    Ok(slot.may_seal(conn, ctx).await? && chain_resuming(conn, ctx).await?)
}

/// Seals repairs for hidden duplicates, detached tree rows, and cascades, refreshes row flags,
/// and materializes every dirty row, running the registered hooks, then captures the groups an
/// exclusion left to republish. Groups hooks settle stay captured for the next seal, see
/// [`crate::hooks::MaterializeHook::settle`]; hooks settle only when `slot` may seal and no
/// chain is open. Returns what it changed.
///
/// A materialization never overwrites a local edit that is not sealed: captured rows it would
/// write, or cascade to, are sealed first and the merge is planned again. Captures of dirty
/// tables are sealed before repairs as `constrained` says. While repairs wait for a chain, see
/// [`repairs_wait`], every repair waits and the dirty rows are listed as stale rows. Once no
/// chain is open, each finish takes back at most [`MAX_STALE_ROWS_PER_WRITE`] stale rows,
/// plans them with the rows that reference them, seals their repairs, and says whether more
/// remain, see [`MAX_OPERATIONS_PER_CHAIN`].
/// Without a writer that can seal them, or when they exceed what a bounded slot may still
/// seal, it returns `None` and writes nothing, so the caller rolls back.
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
    fork::capture_released(engine, conn, batch, now_ms).await?;
    let resuming = repairs_wait(conn, ctx, slot).await?;
    let stale_left = !resuming
        && exclusion::take_stale(conn, &mut batch.dirty, MAX_STALE_ROWS_PER_WRITE).await?;
    if batch.dirty.is_empty() {
        fork::republish(engine, conn, batch, now_ms).await?;
        return Ok(Some(Materialized {
            stale_left,
            ..Materialized::default()
        }));
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
            // Between two groups of one chain the state is part of one edit, which the rest of
            // the chain completes. Every row it touched stays stale until it has applied, so
            // its repairs are planned only against the state the whole chain leaves.
            if resuming {
                exclusion::defer_stale(conn, &batch.dirty).await?;
                break visibility;
            }
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
            let mut elisions = plan.elisions(engine, &pending);
            // A settle is a local edit, so it needs a slot that could seal it, and it waits
            // with the repairs for the state the whole chain leaves.
            let settles = !resuming && slot.may_seal(conn, ctx).await?;
            let mut changed = materialize::execute(engine, conn, plan, settles, now_ms).await?;
            changed.stale_left = stale_left;
            report.invalid_rows.append(&mut changed.refused);
            // A settled row holds its captured value instead of the winner.
            elisions.retain(|(table, key, _, _)| !changed.settled.contains(&(*table, key.clone())));
            hydrate::elide(conn, &elisions).await?;
            failpoint!("elide.before_commit");
            batch.dirty.clear();
            batch.orphaned.clear();
            fork::republish(engine, conn, batch, now_ms).await?;
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
    ///
    /// On the anchor, excluding a fork branch or operations past a removal cutoff leaves
    /// captures of the groups they wrote, which the next [`Engine::seal`] publishes with their
    /// current values.
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
        if let Some(writer) = &writer {
            exclusion::note_local_device(
                self,
                &mut tx,
                ctx,
                &mut batch,
                &writer.certificate.certificate.device_id,
            )
            .await?;
        }
        let mut slot = SealSlot::bounded(writer);
        let mut sealed = SealReport::default();
        // Writers whose applied sequence moved past `sync_writers`, written before anything
        // reads it again.
        let mut advanced = BTreeSet::new();
        // Excluding a fork branch or admitting an excluded entry selects the chain again, which
        // may remove the local device. Captures the writer could still seal then never would, so
        // when the selection would stop the writer, they seal first and the selection runs again.
        if crate::local::has_pending_captures(&mut tx).await? && slot.may_seal(&mut tx, ctx).await?
        {
            let saved = batch.clone();
            let mut trial = tx.begin().await?;
            reselect(self, &mut trial, ctx, &mut batch).await?;
            if slot.may_seal(&mut trial, ctx).await? {
                trial.commit().await?;
            } else {
                trial.rollback().await?;
                batch = saved;
                let tables = self
                    .manifest()
                    .tables
                    .iter()
                    .map(|table| table.id)
                    .collect();
                let captured = seal::captured_in(&mut tx, &tables).await?;
                let mut sealing = Sealing {
                    slot: &mut slot,
                    advanced: &mut advanced,
                    report: &mut sealed,
                    now_ms,
                };
                if !sealing
                    .seal_rows(self, &mut tx, ctx, &mut batch, captured)
                    .await?
                {
                    drop(tx);
                    return Ok(ApplyReport {
                        pending_captures: true,
                        ..ApplyReport::default()
                    });
                }
                flush_applied(&mut tx, ctx, &batch, &mut advanced).await?;
                reselect(self, &mut tx, ctx, &mut batch).await?;
            }
        } else {
            reselect(self, &mut tx, ctx, &mut batch).await?;
        }
        let mut retention = Retention::load(&mut tx, ctx.space).await?;
        let mut report = ApplyReport::default();
        // Fork evidence stored since the last call forks the local writer without an entry.
        if let Some(local) = slot.local_id()
            && log::writer(&mut tx, ctx.space, local)
                .await?
                .is_some_and(|writer| writer.state == WriterState::Forked)
        {
            slot.freeze();
            report.local_writer_frozen = true;
        }
        let mut blocked = BTreeSet::new();
        // A group waits on an operation of another writer that may apply later in the call.
        let mut deferred = false;
        // Writers whose group waits on its own writer. A cutoff or exclusion an entry selects
        // later in the call may end the group, so they are scanned again after each selection.
        let mut waiting = BTreeSet::new();
        let mut taken = 0usize;
        let mut bytes = 0usize;
        let mut heads = Heads::load(&mut tx, ctx).await?;
        // Chains open one at a time, and only once the rows the last one deferred were taken
        // back, so repairs wait for one chain at a time and every deferral ends. A call that
        // cannot seal repairs defers none and opens chains freely. Of the chains that could
        // open, the one stored whole first opens, see [`Heads::first_opener`]. It is found once
        // per call, so a call scans each candidate's chain at most once for it.
        let may_seal = slot.may_seal(&mut tx, ctx).await?;
        let chain_open = may_seal && chain_resuming(&mut tx, ctx).await?;
        let stale_waiting = may_seal && exclusion::stale_rows_listed(&mut tx).await?;
        let mut opened = false;
        let mut winner: Option<(u64, WriterId)> = None;
        let mut gated = false;
        while let Some((mut candidate, context)) = heads
            .take_ready(self, &mut tx, ctx, &batch, &blocked, &mut report)
            .await?
        {
            if taken >= MAX_OPS_PER_APPLY
                || report.changes >= MAX_CHANGES_PER_APPLY
                || (taken > 0 && bytes.saturating_add(candidate.bytes) > MAX_BYTES_PER_APPLY)
            {
                report.more = true;
                break;
            }
            let scan = scan_group(
                self,
                &mut tx,
                ctx,
                &batch,
                &candidate,
                (heads.revision, &heads.excluding),
            )
            .await?;
            let rest = match scan {
                GroupScan::Complete { rest, opens } => {
                    if let Some(closed_at) = opens {
                        let waits = if chain_open || stale_waiting || opened {
                            true
                        } else if let Some((_, writer)) = winner {
                            writer != candidate.writer
                        } else {
                            let others = heads
                                .first_opener(self, &mut tx, ctx, &batch, &blocked)
                                .await?;
                            match others {
                                Some(first) if first < (closed_at, candidate.writer) => {
                                    winner = Some(first);
                                    true
                                }
                                _ => false,
                            }
                        };
                        if waits {
                            gated = true;
                            blocked.insert(candidate.writer);
                            continue;
                        }
                        opened = true;
                    }
                    rest
                }
                GroupScan::Waiting { on_others } => {
                    deferred |= on_others;
                    if !on_others {
                        waiting.insert(candidate.writer);
                    }
                    blocked.insert(candidate.writer);
                    continue;
                }
                GroupScan::Invalid(seq) => {
                    hold_invalid(&mut tx, ctx, candidate.writer, seq).await?;
                    report.held += 1;
                    blocked.insert(candidate.writer);
                    continue;
                }
            };
            if !rest.is_empty() {
                // The group applies past every cap inside a savepoint, and nothing of it stays
                // when one of its operations cannot apply now.
                let writer = candidate.writer;
                let saved = (
                    batch.clone(),
                    retention.clone(),
                    report.clone(),
                    sealed.clone(),
                    advanced.clone(),
                    taken,
                    bytes,
                );
                let budget = slot.checkpoint();
                let mut savepoint = tx.begin().await?;
                let mut failure = None;
                let mut last = candidate.seq;
                for (mut member, member_context) in
                    std::iter::once((candidate, context)).chain(rest)
                {
                    taken += 1;
                    bytes = bytes.saturating_add(member.bytes);
                    let mut sealing = Sealing {
                        slot: &mut slot,
                        advanced: &mut advanced,
                        report: &mut sealed,
                        now_ms,
                    };
                    let reach = Reach {
                        beyond: exclusion::beyond_bound(&member_context, &heads.excluding),
                        past_fork: exclusion::past_fork(&member_context, &heads.excluding),
                    };
                    match apply_one(
                        self,
                        &mut savepoint,
                        ctx,
                        &mut batch,
                        &mut sealing,
                        &mut member,
                        (&member_context, reach),
                        &mut retention,
                        &mut report,
                    )
                    .await?
                    {
                        Outcome::Applied => {
                            mark_applied(
                                &mut savepoint,
                                ctx,
                                &mut batch,
                                &member,
                                member_context,
                                &mut retention,
                            )
                            .await?;
                            warn_clock(&mut report, member.writer, member.clock, now_ms);
                            report.applied += 1;
                            advanced.insert(member.writer);
                            last = member.seq;
                        }
                        Outcome::Authorized | Outcome::Excluded => {
                            return Err(corrupt("an authorization entry applied inside a group"));
                        }
                        Outcome::Held => {
                            failure = Some(Some(member.seq));
                            break;
                        }
                        Outcome::PendingCaptures => {
                            failure = Some(None);
                            break;
                        }
                    }
                }
                match failure {
                    None => {
                        savepoint.commit().await?;
                        heads.advance(&mut tx, ctx, writer, last).await?;
                    }
                    Some(held) => {
                        savepoint.rollback().await?;
                        (batch, retention, report, sealed, advanced, taken, bytes) = saved;
                        slot.rewind(budget);
                        match held {
                            Some(seq) => {
                                hold_invalid(&mut tx, ctx, writer, seq).await?;
                                report.held += 1;
                            }
                            None => report.pending_captures = true,
                        }
                        blocked.insert(writer);
                    }
                }
                continue;
            }
            taken += 1;
            bytes = bytes.saturating_add(candidate.bytes);
            let mut sealing = Sealing {
                slot: &mut slot,
                advanced: &mut advanced,
                report: &mut sealed,
                now_ms,
            };
            let reach = Reach {
                beyond: exclusion::beyond_bound(&context, &heads.excluding),
                past_fork: exclusion::past_fork(&context, &heads.excluding),
            };
            match apply_one(
                self,
                &mut tx,
                ctx,
                &mut batch,
                &mut sealing,
                &mut candidate,
                (&context, reach),
                &mut retention,
                &mut report,
            )
            .await?
            {
                outcome @ (Outcome::Applied | Outcome::Authorized) => {
                    mark_applied(
                        &mut tx,
                        ctx,
                        &mut batch,
                        &candidate,
                        context,
                        &mut retention,
                    )
                    .await?;
                    warn_clock(&mut report, candidate.writer, candidate.clock, now_ms);
                    report.applied += 1;
                    advanced.insert(candidate.writer);
                    if matches!(outcome, Outcome::Authorized) {
                        flush_applied(&mut tx, ctx, &batch, &mut advanced).await?;
                        retention.flush(&mut tx, ctx.space).await?;
                        retention = Retention::load(&mut tx, ctx.space).await?;
                        heads = Heads::load(&mut tx, ctx).await?;
                        winner = None;
                        for writer in std::mem::take(&mut waiting) {
                            blocked.remove(&writer);
                        }
                    } else {
                        heads
                            .advance(&mut tx, ctx, candidate.writer, candidate.seq)
                            .await?;
                    }
                }
                Outcome::Excluded => {
                    flush_applied(&mut tx, ctx, &batch, &mut advanced).await?;
                    retention.flush(&mut tx, ctx.space).await?;
                    retention = Retention::load(&mut tx, ctx.space).await?;
                    heads = Heads::load(&mut tx, ctx).await?;
                    winner = None;
                    for writer in std::mem::take(&mut waiting) {
                        blocked.remove(&writer);
                    }
                }
                Outcome::Held => {
                    hold_invalid(&mut tx, ctx, candidate.writer, candidate.seq).await?;
                    report.held += 1;
                }
                Outcome::PendingCaptures => {
                    report.pending_captures = true;
                    blocked.insert(candidate.writer);
                }
            }
        }
        if deferred && report.applied > 0 {
            report.more = true;
        }
        // A gated chain opens in a later call once the open chain applied, the chain it waited
        // for was held, or the finish took the deferred rows back.
        if gated && (report.applied > 0 || report.held > 0 || (stale_waiting && !chain_open)) {
            report.more = true;
        }
        flush_applied(&mut tx, ctx, &batch, &mut advanced).await?;
        retention.flush(&mut tx, ctx.space).await?;
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
        report.more |= changed.stale_left;
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
