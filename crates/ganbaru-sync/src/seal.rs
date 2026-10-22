//! Sealing local captures into signed operations of the local writer.
//!
//! A sealed operation is applied to the merge state as it is sealed, with the context of
//! everything applied so far, so local writes supersede every version they were made over.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::bounds::{
    MAX_CHANGES_PER_OPERATION, MAX_DEPENDENCIES, MAX_HEADER_BYTES, MAX_OPERATION_BYTES,
};
use ganbaru_sync_contracts::op::{ENVELOPE_OVERHEAD, GENESIS_PREVIOUS_HASH};
use ganbaru_sync_contracts::{
    Cause, Change, ChangeAction, Content, Digest32, Field, GroupId, GroupMask, GroupValue, Header,
    Hlc, HlcClock, Operation, PENDING_KEY_PREFIX, Revocation, RevokeReason, RowKey, Seq,
    SignedCertificate, TableId, Value, VersionVector, WriterId,
};
use sqlx::sqlite::SqliteRow;
use sqlx::{AssertSqlSafe, Connection, Row, SqliteConnection};

use crate::apply::{Constrained, MAX_BYTES_PER_APPLY, MAX_OPS_PER_APPLY, finish_sealed};
use crate::error::{SyncResult, corrupt, failpoint};
use crate::log::{self, WriterState, clamp_ms, seq_i64};
use crate::manifest::{GroupStorage, MergeKind, TableSpec};
use crate::merge::{self, Batch, RowState, mask_param, table_param};
use crate::visibility::{RowRef, Visibility};
use crate::{Engine, LocalWriter, SpaceContext, SyncError, sql, validate};

/// A captured local row that could not be sealed. Its capture is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidRow {
    /// Table of the row.
    pub table: TableId,
    /// Key of the row as captured.
    pub row_key: String,
    /// Which rule the row breaks.
    pub reason: &'static str,
}

/// Result of sealing local captures.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SealReport {
    /// Operations sealed.
    pub sealed_ops: usize,
    /// Changes across those operations.
    pub changes: usize,
    /// Sequence of the last sealed operation.
    pub last_seq: Option<Seq>,
    /// Captured rows that break the manifest and stay captured.
    pub invalid_rows: Vec<InvalidRow>,
    /// Domain tables the call changed while materializing merges and repairs.
    pub changed_tables: Vec<&'static str>,
    /// Captures remain beyond the transaction budget, so the caller seals again.
    pub more: bool,
}

impl SealReport {
    pub(crate) fn absorb(&mut self, other: Self) {
        self.more |= other.more;
        self.sealed_ops += other.sealed_ops;
        self.changes += other.changes;
        self.last_seq = other.last_seq.or(self.last_seq);
        self.invalid_rows.extend(other.invalid_rows);
        for table in other.changed_tables {
            if !self.changed_tables.contains(&table) {
                self.changed_tables.push(table);
            }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.sealed_ops == 0 && self.invalid_rows.is_empty()
    }
}

/// Exact encoded size of one change in the header and the body.
pub(crate) fn change_size(change: &Change) -> (usize, usize) {
    let mut header = 2 + 1 + change.row.as_str().len() + 1 + 8 + 32 * change.groups.len();
    if change.action == ChangeAction::Tombstone {
        header += match (&change.replaced_by, &change.cause) {
            (Some(target), _) => 1 + 1 + target.as_str().len(),
            (None, Some(cause)) => 1 + 2 + 1 + cause.row.as_str().len(),
            (None, None) => 1,
        };
    }
    let body = change
        .groups
        .iter()
        .map(|group| 1 + group.value.encode().len())
        .sum();
    (header, body)
}

/// Header bytes before the changes of an operation with `dependencies` dependencies.
const fn header_base(dependencies: usize) -> usize {
    28 + 24 * dependencies
}

fn fits(header: usize, body: usize) -> bool {
    header <= MAX_HEADER_BYTES && ENVELOPE_OVERHEAD + header + body <= MAX_OPERATION_BYTES
}

/// Whether a change fits an operation alone at the largest dependency count.
pub(crate) fn fits_alone(change: &Change) -> bool {
    let (header, body) = change_size(change);
    fits(header_base(MAX_DEPENDENCIES) + header, body)
}

/// A stored operation of the local writer, applied as it is inserted.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_applied_op(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: WriterId,
    seq: Seq,
    kind: u8,
    hash: &Digest32,
    clock: Hlc,
    context: &VersionVector,
    bytes: &[u8],
    now_ms: u64,
) -> SyncResult<()> {
    sqlx::query(
        "INSERT INTO sync_ops
            (space_id, writer_id, seq, kind, hash, clock, context, envelope, state, hold_reason,
             stored_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'applied', NULL, ?)",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .bind(i64::from(kind))
    .bind(hash.as_slice())
    .bind(clamp_ms(clock.as_u64()))
    .bind(context.encode())
    .bind(bytes)
    .bind(clamp_ms(now_ms))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Header fields an operation keeps when a new writer re-seals it, so it merges exactly as the
/// original did.
#[derive(Debug, Clone)]
pub(crate) struct Replay {
    /// Original clock.
    pub(crate) clock: Hlc,
    /// Original manifest version.
    pub(crate) manifest_version: u32,
    /// Original causal context with the replaced writer's operations renamed to their
    /// replacements.
    pub(crate) context: VersionVector,
}

/// The local writer's chain position while sealing.
#[derive(Debug)]
pub(crate) struct Sealer {
    id: WriterId,
    stored: Seq,
    head_hash: Digest32,
    head_context: VersionVector,
    clock: HlcClock,
}

impl Sealer {
    /// Opens the local writer, sealing its genesis when the space does not know it yet.
    pub(crate) async fn open(
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<Self> {
        let certificate = writer.certificate;
        let cert = &certificate.certificate;
        if cert.writer_key != writer.key.public_key()
            || cert.person_key != ctx.anchor
            || SignedCertificate::decode(&certificate.encode()).is_err()
        {
            return Err(SyncError::WriterMismatch);
        }
        let id = cert.writer_id();
        let newest: Option<i64> =
            sqlx::query_scalar("SELECT max(head_clock) FROM sync_writers WHERE space_id = ?")
                .bind(ctx.space.as_bytes().as_slice())
                .fetch_one(&mut *conn)
                .await?;
        let existing = log::writer(conn, ctx.space, id).await?;
        let own = existing
            .as_ref()
            .map_or(Hlc::ZERO, |row| Hlc::from_u64(row.head_clock));
        let mut clock = HlcClock::new(own);
        if let Some(newest) = newest {
            clock.observe(
                Hlc::from_u64(log::stored_u64(newest, "head clock")?),
                now_ms,
            );
        }
        let Some(row) = existing else {
            return Self::genesis(engine, conn, ctx, batch, writer, clock, now_ms).await;
        };
        if row.state != WriterState::Active {
            return Err(SyncError::WriterInactive(id));
        }
        if row.stored != row.applied {
            return Err(corrupt("local writer has operations that are not applied"));
        }
        let head_context = batch
            .contexts
            .get(conn, ctx.space, id, row.stored)
            .await?
            .clone();
        Ok(Self {
            id,
            stored: row.stored,
            head_hash: row.head_hash,
            head_context,
            clock,
        })
    }

    async fn genesis(
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        mut clock: HlcClock,
        now_ms: u64,
    ) -> SyncResult<Self> {
        let certificate = writer.certificate;
        let cert = &certificate.certificate;
        let id = cert.writer_id();
        writer
            .reservation
            .ensure_reserved(1)
            .map_err(SyncError::Reservation)?;
        let tick = clock.tick(now_ms);
        let operation = Operation {
            space: ctx.space,
            writer: id,
            seq: 1,
            previous_hash: GENESIS_PREVIOUS_HASH,
            header: Header {
                clock: tick,
                manifest_version: u32::from(engine.manifest().version),
                authorization_revision: 0,
                key_epoch: 0,
                dependencies: Vec::new(),
            },
            content: Content::Genesis(certificate.clone()),
        };
        let sealed = operation.seal(writer.key).map_err(SyncError::Seal)?;
        sqlx::query(
            "INSERT INTO sync_writers
                (space_id, writer_id, public_key, device_id, certificate, state, predecessor,
                 cutoff_seq, stored_seq, applied_seq, head_hash, head_clock, created_at_ms)
             VALUES (?, ?, ?, ?, ?, 'active', ?, NULL, 1, 1, ?, ?, ?)",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(id.as_bytes().as_slice())
        .bind(cert.writer_key.as_bytes().as_slice())
        .bind(cert.device_id.as_str())
        .bind(certificate.encode())
        .bind(
            cert.predecessor
                .map(|predecessor| predecessor.as_bytes().to_vec()),
        )
        .bind(sealed.hash().as_slice())
        .bind(clamp_ms(tick.as_u64()))
        .bind(cert.created_at_ms.max(0))
        .execute(&mut *conn)
        .await?;
        let context = VersionVector::new();
        insert_applied_op(
            conn,
            ctx,
            id,
            1,
            operation.kind().as_u8(),
            &sealed.hash(),
            tick,
            &context,
            sealed.bytes(),
            now_ms,
        )
        .await?;
        batch.applied.set(id, 1);
        batch.contexts.insert(id, 1, context.clone());
        Ok(Self {
            id,
            stored: 1,
            head_hash: sealed.hash(),
            head_context: context,
            clock,
        })
    }

    /// Every dependency the next operation needs: what the target context, everything applied
    /// unless a replay names one, adds to the head context, in writer id order.
    fn dependencies(
        &self,
        batch: &Batch,
        replay: Option<&Replay>,
    ) -> SyncResult<Vec<(WriterId, Seq)>> {
        let mut target =
            replay.map_or_else(|| batch.applied.clone(), |replay| replay.context.clone());
        target.set(self.id, 0);
        let mut head = self.head_context.clone();
        head.set(self.id, 0);
        if !target.dominates(&head) || !batch.applied.dominates(&target) {
            return Err(corrupt(
                "an operation context must lie between its head and the applied state",
            ));
        }
        Ok(target.delta_since(&self.head_context))
    }

    /// Dependencies of the next operation once they fit one header. While more writers
    /// advanced than a header lists, this seals operations without changes, each taking the
    /// next [`MAX_DEPENDENCIES`] writers in id order.
    #[allow(clippy::too_many_arguments)]
    async fn catch_up(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        replay: Option<&Replay>,
        now_ms: u64,
        report: &mut SealReport,
    ) -> SyncResult<Vec<(WriterId, Seq)>> {
        loop {
            let mut dependencies = self.dependencies(batch, replay)?;
            if dependencies.len() <= MAX_DEPENDENCIES {
                return Ok(dependencies);
            }
            dependencies.truncate(MAX_DEPENDENCIES);
            let (seq, _, _) = self
                .seal_operation(
                    engine,
                    conn,
                    ctx,
                    batch,
                    writer,
                    dependencies,
                    Content::Changes(Vec::new()),
                    replay,
                    now_ms,
                )
                .await?;
            report.sealed_ops += 1;
            report.last_seq = Some(seq);
        }
    }

    /// Seals an operation without changes that advances to the replay context, replaying one
    /// such operation of a replaced chain.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn seal_advance(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        replay: &Replay,
        now_ms: u64,
        report: &mut SealReport,
    ) -> SyncResult<()> {
        let replay = Some(replay);
        let dependencies = self
            .catch_up(engine, conn, ctx, batch, writer, replay, now_ms, report)
            .await?;
        let (seq, _, _) = self
            .seal_operation(
                engine,
                conn,
                ctx,
                batch,
                writer,
                dependencies,
                Content::Changes(Vec::new()),
                replay,
                now_ms,
            )
            .await?;
        report.sealed_ops += 1;
        report.last_seq = Some(seq);
        Ok(())
    }

    /// Seals one operation with `content`, returning its sequence, clock, and context. Changes
    /// are not applied here.
    #[allow(clippy::too_many_arguments)]
    async fn seal_operation(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        dependencies: Vec<(WriterId, Seq)>,
        content: Content,
        replay: Option<&Replay>,
        now_ms: u64,
    ) -> SyncResult<(Seq, Hlc, VersionVector)> {
        let seq = self.stored + 1;
        writer
            .reservation
            .ensure_reserved(seq)
            .map_err(SyncError::Reservation)?;
        failpoint!("seal.reserve");
        let (clock, manifest_version) = match replay {
            Some(replay) => {
                self.clock.observe(replay.clock, now_ms);
                (replay.clock, replay.manifest_version)
            }
            None => (
                self.clock.tick(now_ms),
                u32::from(engine.manifest().version),
            ),
        };
        let context = log::next_context(&self.head_context, self.id, seq, &dependencies);
        let operation = Operation {
            space: ctx.space,
            writer: self.id,
            seq,
            previous_hash: self.head_hash,
            header: Header {
                clock,
                manifest_version,
                authorization_revision: 0,
                key_epoch: 0,
                dependencies,
            },
            content,
        };
        let sealed = operation.seal(writer.key).map_err(SyncError::Seal)?;
        insert_applied_op(
            conn,
            ctx,
            self.id,
            seq,
            operation.kind().as_u8(),
            &sealed.hash(),
            clock,
            &context,
            sealed.bytes(),
            now_ms,
        )
        .await?;
        sqlx::query(
            "UPDATE sync_writers SET stored_seq = ?, applied_seq = ?, head_hash = ?,
                head_clock = max(head_clock, ?)
             WHERE space_id = ? AND writer_id = ?",
        )
        .bind(seq_i64(seq)?)
        .bind(seq_i64(seq)?)
        .bind(sealed.hash().as_slice())
        .bind(clamp_ms(clock.as_u64()))
        .bind(ctx.space.as_bytes().as_slice())
        .bind(self.id.as_bytes().as_slice())
        .execute(&mut *conn)
        .await?;
        batch.applied.set(self.id, seq);
        batch.contexts.insert(self.id, seq, context.clone());
        self.stored = seq;
        self.head_hash = sealed.hash();
        self.head_context = context.clone();
        Ok((seq, clock, context))
    }

    /// Seals changes into as few operations as their exact sizes allow and applies them.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn seal_changes(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        changes: Vec<Change>,
        replay: Option<&Replay>,
        now_ms: u64,
        report: &mut SealReport,
    ) -> SyncResult<()> {
        let mut pending = changes.into_iter().peekable();
        while pending.peek().is_some() {
            let dependencies = self
                .catch_up(engine, conn, ctx, batch, writer, replay, now_ms, report)
                .await?;
            let mut header = header_base(dependencies.len());
            let mut body = 0;
            let mut chunk = Vec::new();
            while let Some(change) = pending.peek() {
                let (change_header, change_body) = change_size(change);
                let full = chunk.len() == MAX_CHANGES_PER_OPERATION
                    || !fits(header + change_header, body + change_body);
                if full {
                    break;
                }
                header += change_header;
                body += change_body;
                chunk.extend(pending.next());
            }
            if chunk.is_empty() {
                return Err(corrupt("a change does not fit an operation"));
            }
            let count = chunk.len();
            let (seq, op_clock, context) = self
                .seal_operation(
                    engine,
                    conn,
                    ctx,
                    batch,
                    writer,
                    dependencies,
                    Content::Changes(chunk.clone()),
                    replay,
                    now_ms,
                )
                .await?;
            for change in &chunk {
                let table = engine
                    .manifest()
                    .table(change.table)
                    .ok_or_else(|| corrupt("sealed change of an unknown table"))?;
                merge::apply_change(conn, batch, self.id, seq, op_clock, &context, table, change)
                    .await?;
            }
            report.sealed_ops += 1;
            report.changes += count;
            report.last_seq = Some(seq);
        }
        Ok(())
    }

    /// Seals a revocation of `target` and applies its effects.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn seal_revoke(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        revocation: Revocation,
        replay: Option<&Replay>,
        now_ms: u64,
        report: &mut SealReport,
    ) -> SyncResult<()> {
        let dependencies = self
            .catch_up(engine, conn, ctx, batch, writer, replay, now_ms, report)
            .await?;
        let (target, cutoff, reason) = (revocation.writer, revocation.cutoff, revocation.reason);
        let (seq, _, _) = self
            .seal_operation(
                engine,
                conn,
                ctx,
                batch,
                writer,
                dependencies,
                Content::Revoke(revocation),
                replay,
                now_ms,
            )
            .await?;
        merge::revoke_effects(conn, ctx.space, target, cutoff, reason).await?;
        report.sealed_ops += 1;
        report.last_seq = Some(seq);
        Ok(())
    }

    /// Seals every valid capture except deletes still held at `now_ms`, and deletes the
    /// captures it consumed.
    pub(crate) async fn seal_captures(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<SealReport> {
        let mut report = SealReport::default();
        let captures = pending_captures(conn, now_ms).await?;
        let planned = plan_captures(engine, conn, captures, &mut report).await?;
        self.seal_planned(
            engine,
            conn,
            ctx,
            batch,
            writer,
            planned,
            now_ms,
            &mut report,
        )
        .await?;
        Ok(report)
    }

    /// Like [`Self::seal_captures`], taking one batch of at most [`MAX_OPS_PER_SEAL`]
    /// operations and [`MAX_BYTES_PER_SEAL`] change bytes. Captures of changes left for the
    /// next batch stay, and the report says more remain.
    pub(crate) async fn seal_capture_batch(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<SealReport> {
        let mut report = SealReport::default();
        let captures = pending_captures(conn, now_ms).await?;
        let mut planned = plan_captures(engine, conn, captures, &mut report).await?;
        let (selected, rest) = select_batch(engine, std::mem::take(&mut planned.changes));
        if !rest.is_empty() {
            let left: BTreeSet<(i64, &str)> = rest
                .iter()
                .map(|change| (i64::from(change.table.0), change.row.as_str()))
                .collect();
            planned
                .consumed
                .retain(|(table, key)| !left.contains(&(*table, key.as_str())));
            report.more = true;
        }
        planned.changes = selected;
        self.seal_planned(
            engine,
            conn,
            ctx,
            batch,
            writer,
            planned,
            now_ms,
            &mut report,
        )
        .await?;
        Ok(report)
    }

    /// Seals the captures of `rows`, held deletes included, with the captured creates they
    /// depend on, before a remote operation or a materialization changes those rows.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn seal_rows(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        rows: &BTreeSet<RowRef>,
        now_ms: u64,
        report: &mut SealReport,
    ) -> SyncResult<()> {
        let planned = plan_rows(engine, conn, rows, report).await?;
        self.seal_planned(engine, conn, ctx, batch, writer, planned, now_ms, report)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn seal_planned(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        planned: PlannedCaptures,
        now_ms: u64,
        report: &mut SealReport,
    ) -> SyncResult<()> {
        if planned.held {
            failpoint!("hold.before_seal");
        }
        self.seal_changes(
            engine,
            conn,
            ctx,
            batch,
            writer,
            constrained_first(engine, planned.changes),
            None,
            now_ms,
            report,
        )
        .await?;
        delete_captures(conn, &planned.consumed).await
    }
}

/// The sealer slot of an engine call: the local writer it may seal with, opened on first use.
pub(crate) struct SealSlot<'a, 'k> {
    writer: Option<&'a mut LocalWriter<'k>>,
    sealer: Option<Sealer>,
    frozen: bool,
    /// What the call may still seal, when it is bounded.
    budget: Option<SealBudget>,
}

impl<'a, 'k> SealSlot<'a, 'k> {
    pub(crate) fn new(writer: Option<&'a mut LocalWriter<'k>>) -> Self {
        Self {
            writer,
            sealer: None,
            frozen: false,
            budget: None,
        }
    }

    /// A slot whose call seals at most one seal transaction of captures, so a call that would
    /// seal more leaves them to the batched seal loop.
    pub(crate) fn bounded(writer: Option<&'a mut LocalWriter<'k>>) -> Self {
        Self {
            budget: Some(SealBudget::default()),
            ..Self::new(writer)
        }
    }

    /// Charges the changes that sealing the captures of `rows` plans against the budget of a
    /// bounded slot. Returns false, charging nothing, when they do not fit what remains.
    pub(crate) async fn admit(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        rows: &BTreeSet<RowRef>,
    ) -> SyncResult<bool> {
        let Some(budget) = self.budget.as_mut() else {
            return Ok(true);
        };
        let planned = plan_rows(engine, conn, rows, &mut SealReport::default()).await?;
        Ok(budget.charge(&planned.changes))
    }

    /// Writer id of the local writer, sealing or not.
    pub(crate) fn local_id(&self) -> Option<WriterId> {
        self.writer.as_ref().map(|writer| writer.id())
    }

    pub(crate) fn frozen(&self) -> bool {
        self.frozen
    }

    /// Stops sealing for the rest of the call because the local writer is no longer active.
    pub(crate) fn freeze(&mut self) {
        self.frozen = true;
        self.sealer = None;
    }

    /// The opened sealer and its writer; errors when the writer cannot seal.
    pub(crate) async fn open(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        now_ms: u64,
    ) -> SyncResult<Option<(&mut Sealer, &mut LocalWriter<'k>)>> {
        if self.frozen {
            return Ok(None);
        }
        let Some(writer) = self.writer.as_deref_mut() else {
            return Ok(None);
        };
        if self.sealer.is_none() {
            self.sealer = Some(Sealer::open(engine, conn, ctx, batch, writer, now_ms).await?);
        }
        Ok(self.sealer.as_mut().map(|sealer| (sealer, writer)))
    }

    /// Like [`Self::open`], freezing the slot instead of failing when the writer is inactive.
    pub(crate) async fn try_open(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        now_ms: u64,
    ) -> SyncResult<Option<(&mut Sealer, &mut LocalWriter<'k>)>> {
        if self.frozen || self.writer.is_none() {
            return Ok(None);
        }
        if self.sealer.is_none() {
            let writer = self
                .writer
                .as_deref_mut()
                .ok_or_else(|| corrupt("sealer slot lost its writer"))?;
            match Sealer::open(engine, conn, ctx, batch, writer, now_ms).await {
                Ok(sealer) => self.sealer = Some(sealer),
                Err(SyncError::WriterInactive(_)) => {
                    self.freeze();
                    return Ok(None);
                }
                Err(error) => return Err(error),
            }
        }
        self.open(engine, conn, ctx, batch, now_ms).await
    }
}

/// Domain state of a captured row.
pub(crate) enum DomainRow {
    Absent,
    Present(Vec<GroupValue>),
    Invalid(&'static str),
}

/// Reads every group value of a domain row as an operation would carry it.
pub(crate) async fn read_domain(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: &TableSpec,
    key: &str,
) -> SyncResult<DomainRow> {
    let columns: Vec<&str> = table
        .groups
        .iter()
        .flat_map(|group| group.columns().iter().map(|column| column.name))
        .collect();
    let Some(fields) = sql::read_columns(conn, table.name, table.key_column, key, &columns).await?
    else {
        return Ok(DomainRow::Absent);
    };
    let mut offset = 0;
    let mut values = Vec::with_capacity(table.groups.len());
    for group in table.groups {
        let group_fields: Vec<Field> = match group.storage {
            // A present derived-key row exists.
            GroupStorage::Columns(_) if table.is_exists_group(group.id) => {
                vec![Field::Integer(1)]
            }
            GroupStorage::Columns(group_columns) => {
                let slice = &fields[offset..offset + group_columns.len()];
                offset += group_columns.len();
                match slice.iter().cloned().collect::<Option<Vec<Field>>>() {
                    Some(fields) => fields,
                    None => return Ok(DomainRow::Invalid("unreadable column")),
                }
            }
            GroupStorage::Owned(owned) => {
                let rows = sql::read_owned(conn, &owned, key).await?;
                let Some(rows) = rows
                    .into_iter()
                    .map(|row| row.into_iter().collect::<Option<Vec<Field>>>())
                    .collect::<Option<Vec<_>>>()
                else {
                    return Ok(DomainRow::Invalid("unreadable owned row"));
                };
                let Some(adapter) = engine.adapter(table) else {
                    return Ok(DomainRow::Invalid("owned group without adapter"));
                };
                match adapter.encode_owned(table.id, group.id, rows) {
                    Ok(value) => value.into_fields(),
                    Err(error) => return Ok(DomainRow::Invalid(error.0)),
                }
            }
        };
        let Ok(value) = Value::new(group_fields) else {
            return Ok(DomainRow::Invalid("value bounds"));
        };
        if let Err(reason) = validate::group_value(engine, table, group, &value) {
            return Ok(DomainRow::Invalid(reason));
        }
        values.push(GroupValue::new(table.id, group.id, value));
    }
    Ok(DomainRow::Present(values))
}

/// One row of `sync_capture`.
struct Capture {
    table: i64,
    key: String,
    mask: GroupMask,
    /// Whether the row was inserted locally since the last seal.
    created: bool,
    deleted: bool,
    /// Whether the delete is held for Undo, expired or not.
    held: bool,
    replaced_by: Option<String>,
    /// Sync state of the row when planning started.
    state: Option<RowState>,
}

/// Changes planned from captures and the captures they consume.
struct PlannedCaptures {
    changes: Vec<Change>,
    consumed: Vec<(i64, String)>,
    /// Whether a held delete is among the captures.
    held: bool,
}

/// Most operations one seal transaction commits, matching what one apply transaction takes.
pub const MAX_OPS_PER_SEAL: usize = MAX_OPS_PER_APPLY;
/// Most change bytes one seal transaction commits, matching what one apply transaction takes,
/// so a large first seal reaches other devices in batches they apply whole.
pub const MAX_BYTES_PER_SEAL: usize = MAX_BYTES_PER_APPLY;

/// Most changes one seal transaction commits: [`MAX_OPS_PER_SEAL`] full operations.
const MAX_CHANGES_PER_SEAL: usize = MAX_OPS_PER_SEAL.saturating_mul(MAX_CHANGES_PER_OPERATION);

/// Encoded bytes of `change` in its operation, header and body.
fn change_cost(change: &Change) -> usize {
    let (header, body) = change_size(change);
    header + body
}

/// Change bytes and changes one call has sealed, within one seal transaction.
#[derive(Debug, Default)]
pub(crate) struct SealBudget {
    bytes: usize,
    changes: usize,
}

impl SealBudget {
    /// Charges `changes`. Returns false, charging nothing, when they exceed what remains.
    pub(crate) fn charge(&mut self, changes: &[Change]) -> bool {
        let bytes = changes
            .iter()
            .map(change_cost)
            .fold(self.bytes, usize::saturating_add);
        let count = self.changes.saturating_add(changes.len());
        if bytes > MAX_BYTES_PER_SEAL || count > MAX_CHANGES_PER_SEAL {
            return false;
        }
        self.bytes = bytes;
        self.changes = count;
        true
    }
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
        created: row.try_get::<i64, _>(7)? != 0,
        deleted: row.try_get::<i64, _>(4)? != 0,
        held: row.try_get::<Option<i64>, _>(8)?.is_some(),
        replaced_by: row.try_get(5)?,
        state: merge::parse_row_state(row.try_get::<Option<&str>, _>(6)?)?,
    })
}

/// Every capture except deletes still held at `now_ms`, in table and key order.
async fn pending_captures(conn: &mut SqliteConnection, now_ms: u64) -> SyncResult<Vec<Capture>> {
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

/// The captures of `rows`, held deletes included.
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

/// Captured rows of `tables` without a held delete. A held row stays live in merge state, so
/// repairs already account for it, and it seals early only when a change or plan names it.
pub(crate) async fn captured_in(
    conn: &mut SqliteConnection,
    tables: &BTreeSet<TableId>,
) -> SyncResult<BTreeSet<RowRef>> {
    let mut found = BTreeSet::new();
    for table in tables {
        let keys: Vec<String> = sqlx::query_scalar(
            "SELECT row_key FROM sync_capture WHERE table_id = ? AND held_until_ms IS NULL",
        )
        .bind(table_param(*table))
        .fetch_all(&mut *conn)
        .await?;
        found.extend(keys.into_iter().map(|key| (*table, key)));
    }
    Ok(found)
}

/// Rows a change depends on: the rows its reference columns name, and the cause and redirect
/// target of a tombstone. Sealing a change without the create of such a row would materialize
/// the reference as null or hide the row.
pub(crate) fn change_dependencies(engine: &Engine, change: &Change) -> Vec<RowRef> {
    let Some(table) = engine.manifest().table(change.table) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for (group, field, reference) in table.references() {
        let value = change.groups.iter().find(|value| value.group == group.id);
        if let Some(Field::Text(key)) = value.and_then(|value| value.value.fields().get(field)) {
            rows.push((reference.table, key.clone()));
        }
    }
    if let Some(cause) = &change.cause {
        rows.push((cause.table, cause.row.as_str().to_string()));
    }
    if let Some(target) = &change.replaced_by {
        rows.push((change.table, target.as_str().to_string()));
    }
    rows
}

/// Positions of the writes and tombstones of tables whose constraint reads several rows
/// together, by table.
fn constrained_changes(engine: &Engine, changes: &[Change]) -> BTreeMap<TableId, Vec<usize>> {
    let mut constrained: BTreeMap<TableId, Vec<usize>> = BTreeMap::new();
    for (position, change) in changes.iter().enumerate() {
        let together = engine
            .manifest()
            .table(change.table)
            .is_some_and(TableSpec::constrains_rows_together);
        if together && change.action != ChangeAction::Create {
            constrained.entry(change.table).or_default().push(position);
        }
    }
    constrained
}

/// Positions of planned changes by row.
fn change_index(changes: &[Change]) -> BTreeMap<RowRef, usize> {
    changes
        .iter()
        .enumerate()
        .map(|(position, change)| ((change.table, change.row.as_str().to_string()), position))
        .collect()
}

/// Planned changes in sealed order: the writes and tombstones of tables whose constraint reads
/// several rows together, with the changes they depend on, then the rest, each part in planned
/// order. A receiver may apply a seal over several transactions, and its repairs must see a
/// rename or delete before a create that reuses the value it frees.
fn constrained_first(engine: &Engine, changes: Vec<Change>) -> Vec<Change> {
    let constrained = constrained_changes(engine, &changes);
    if constrained.is_empty() {
        return changes;
    }
    let index = change_index(&changes);
    let mut first = vec![false; changes.len()];
    let mut pending: Vec<usize> = constrained.into_values().flatten().collect();
    while let Some(position) = pending.pop() {
        if std::mem::replace(&mut first[position], true) {
            continue;
        }
        pending.extend(
            change_dependencies(engine, &changes[position])
                .into_iter()
                .filter_map(|row| index.get(&row).copied()),
        );
    }
    let (mut ordered, rest): (Vec<_>, Vec<_>) = changes
        .into_iter()
        .zip(first)
        .partition(|(_, first)| *first);
    ordered.extend(rest);
    ordered.into_iter().map(|(change, _)| change).collect()
}

/// The changes one seal transaction takes, in planned order, within [`MAX_BYTES_PER_SEAL`]
/// and [`MAX_OPS_PER_SEAL`] full operations, and whether changes remain. A change is taken
/// together with the planned changes of every row it depends on, so a batch never names a
/// row that a later batch creates.
///
/// Writes and tombstones of a table whose constraint reads several rows together seal as one
/// group ahead of every other change, so a create never seals before the rename or delete that
/// frees its unique value, tree position, or edge, and repairs never act on the stale value.
fn select_batch(engine: &Engine, changes: Vec<Change>) -> (Vec<Change>, Vec<Change>) {
    let index = change_index(&changes);
    let constrained = constrained_changes(engine, &changes);
    let starts: Vec<usize> = constrained
        .values()
        .flatten()
        .copied()
        .chain(0..changes.len())
        .collect();
    let mut taken = vec![false; changes.len()];
    let mut bytes = 0usize;
    let mut count = 0usize;
    for start in starts {
        if taken[start] {
            continue;
        }
        let mut group = vec![start];
        let mut seen = BTreeSet::from([start]);
        let mut pending = vec![start];
        while let Some(position) = pending.pop() {
            let together = constrained
                .get(&changes[position].table)
                .into_iter()
                .flatten()
                .copied();
            let dependencies = change_dependencies(engine, &changes[position])
                .into_iter()
                .filter_map(|row| index.get(&row).copied());
            for dependency in together.chain(dependencies) {
                if !taken[dependency] && seen.insert(dependency) {
                    group.push(dependency);
                    pending.push(dependency);
                }
            }
        }
        let cost: usize = group
            .iter()
            .map(|position| change_cost(&changes[*position]))
            .sum();
        if count > 0
            && (bytes.saturating_add(cost) > MAX_BYTES_PER_SEAL
                || count + group.len() > MAX_CHANGES_PER_SEAL)
        {
            break;
        }
        bytes = bytes.saturating_add(cost);
        count += group.len();
        for position in group {
            taken[position] = true;
        }
    }
    let mut selected = Vec::with_capacity(count);
    let mut rest = Vec::new();
    for (change, taken) in changes.into_iter().zip(taken) {
        if taken {
            selected.push(change);
        } else {
            rest.push(change);
        }
    }
    (selected, rest)
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
    let visibility = Visibility::default();
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

async fn plan_captures(
    engine: &Engine,
    conn: &mut SqliteConnection,
    captures: Vec<Capture>,
    report: &mut SealReport,
) -> SyncResult<PlannedCaptures> {
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
        let Ok(key) = RowKey::new(capture.key.as_str()) else {
            report.invalid_rows.push(invalid("row key"));
            continue;
        };
        let domain = read_domain(engine, conn, table, key.as_str()).await?;
        let deletion = capture.deleted
            && capture.state == Some(RowState::Live)
            && matches!(domain, DomainRow::Absent);
        let change = match (capture.state, domain) {
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
            // A derived-key row inserted again rewrites every group.
            (Some(RowState::Live), DomainRow::Present(groups))
                if capture.created && table.derived_key.is_some() =>
            {
                Some(Change {
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
    let changes = creates
        .into_iter()
        .chain(writes)
        .chain(tombstones)
        .map(|(_, change)| change)
        .collect();
    Ok(PlannedCaptures {
        changes,
        consumed,
        held,
    })
}

/// Plans the captures of `rows` together with the captured creates their changes depend on,
/// which have no sync state yet.
async fn plan_rows(
    engine: &Engine,
    conn: &mut SqliteConnection,
    rows: &BTreeSet<RowRef>,
    report: &mut SealReport,
) -> SyncResult<PlannedCaptures> {
    let mut rows = rows.clone();
    rows.extend(cascaded_deletions(engine, conn, &rows).await?);
    loop {
        let mut attempt = SealReport::default();
        let captures = row_captures(conn, &rows).await?;
        let planned = plan_captures(engine, conn, captures, &mut attempt).await?;
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
    let visibility = Visibility::default();
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

impl Engine {
    /// Seals local captures with the local writer, sealing its genesis first when needed.
    ///
    /// One call commits at most [`MAX_OPS_PER_SEAL`] operations and [`MAX_BYTES_PER_SEAL`]
    /// change bytes; [`SealReport::more`] asks the caller to seal again at once. Deletes held
    /// past `now_ms` stay captured; [`crate::hold::next_hold_expiry`] says when to seal them.
    pub async fn seal(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<SealReport> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        sql::begin_engine_write(&mut tx).await?;
        let mut batch = Batch::load(&mut tx, ctx.space).await?;
        let mut slot = SealSlot::new(Some(writer));
        let mut report = SealReport::default();
        if let Some((sealer, writer)) = slot.open(self, &mut tx, ctx, &mut batch, now_ms).await? {
            report = sealer
                .seal_capture_batch(self, &mut tx, ctx, &mut batch, writer, now_ms)
                .await?;
        }
        report.changed_tables = finish_sealed(
            self,
            &mut tx,
            ctx,
            &mut batch,
            &mut slot,
            Constrained::Skip,
            now_ms,
            &mut report,
        )
        .await?;
        failpoint!("seal.before_commit");
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(report)
    }

    /// Seals a revocation of `target` above `cutoff`, after sealing local captures.
    ///
    /// The cutoff must cover every operation of the target this replica applied, so no
    /// replica needs to undo an applied operation.
    #[allow(clippy::too_many_arguments)]
    pub async fn seal_revoke(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        target: WriterId,
        cutoff: Seq,
        reason: RevokeReason,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<SealReport> {
        if target == writer.id() {
            return Err(SyncError::InvalidRevoke("a writer cannot revoke itself"));
        }
        if writer.certificate.certificate.device_id != ctx.coordinator_device_id {
            return Err(SyncError::InvalidRevoke(
                "only the coordinator revokes writers",
            ));
        }
        if cutoff < 1 {
            return Err(SyncError::InvalidRevoke("cutoff must keep the genesis"));
        }
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        sql::begin_engine_write(&mut tx).await?;
        let mut batch = Batch::load(&mut tx, ctx.space).await?;
        let applied = batch.applied.get(&target);
        if applied < 1 {
            return Err(SyncError::InvalidRevoke(
                "the target's genesis is not applied",
            ));
        }
        if cutoff < applied {
            return Err(SyncError::InvalidRevoke(
                "cutoff is below applied operations",
            ));
        }
        let mut slot = SealSlot::new(Some(writer));
        let mut report = SealReport::default();
        let (sealer, writer) = slot
            .open(self, &mut tx, ctx, &mut batch, now_ms)
            .await?
            .ok_or_else(|| corrupt("sealer slot has no writer"))?;
        report.absorb(
            sealer
                .seal_captures(self, &mut tx, ctx, &mut batch, writer, now_ms)
                .await?,
        );
        sealer
            .seal_revoke(
                self,
                &mut tx,
                ctx,
                &mut batch,
                writer,
                Revocation {
                    writer: target,
                    cutoff,
                    reason,
                },
                None,
                now_ms,
                &mut report,
            )
            .await?;
        report.changed_tables = finish_sealed(
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
        failpoint!("seal.before_commit");
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(report)
    }
}

/// Captures deleted per statement after a seal.
const CAPTURE_DELETE_CHUNK: usize = 64;

/// Deletes consumed captures in chunks, keeping the full-chunk statement cached.
async fn delete_captures(
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
