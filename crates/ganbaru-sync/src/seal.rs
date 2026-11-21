//! Sealing local captures into signed operations of the local writer.
//!
//! A sealed operation is applied to the merge state as it is sealed, with the context of
//! everything applied so far, so local writes supersede every version they were made over.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::bounds::{
    MAX_CHANGES_PER_OPERATION, MAX_DEPENDENCIES, MAX_HEADER_BYTES, MAX_OPERATION_BYTES,
};
#[cfg(doc)]
use ganbaru_sync_contracts::op::ends_group;
use ganbaru_sync_contracts::op::{ENVELOPE_OVERHEAD, GENESIS_PREVIOUS_HASH};
use ganbaru_sync_contracts::{
    AuthorizationRecord, Change, ChangeAction, Content, Digest32, Field, GroupValue, Header, Hlc,
    HlcClock, Operation, OperationKind, Seq, SignedCertificate, TableId, Value, VersionVector,
    WriterId,
};
use sqlx::{Connection, SqliteConnection};

use crate::apply::{Constrained, MAX_BYTES_PER_APPLY, MAX_CHANGES_PER_APPLY, finish_sealed};
use crate::authorization::{self, Enrollment};
use crate::error::{SyncResult, corrupt, failpoint};
use crate::exclusion::{self, Excluding};
use crate::hooks::ChangeNotice;
use crate::log::{self, WriterState, clamp_ms, seq_i64};
use crate::manifest::{GroupSpec, TableSpec};
use crate::materialize::Materialized;
use crate::merge::{self, Batch};
use crate::visibility::RowRef;
use crate::{Engine, LocalWriter, SpaceContext, SyncError, codec, sql, validate};

mod capture;

#[cfg(test)]
pub(crate) use capture::sink_roots;
use capture::{PlannedCaptures, delete_captures, pending_captures, plan_captures, plan_rows};
pub(crate) use capture::{
    capture_edit, captured, captured_in, existence_write, force_capture, force_delete_capture,
};

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
    /// Captures of rows another device owns, consumed without sealing.
    pub foreign_owned_rows: usize,
    /// Domain tables the call changed while materializing merges and repairs.
    pub changed_tables: Vec<&'static str>,
    /// Changes the registered hooks reported while materializing.
    pub notices: Vec<ChangeNotice>,
    /// Captures, or stale rows a chain deferred, remain beyond the transaction budget, so the
    /// caller seals again; each seal takes a bounded number of stale rows back.
    pub more: bool,
}

impl SealReport {
    /// Records what the materialization of the call changed, and asks for another call while
    /// it left stale rows listed.
    pub(crate) fn record(&mut self, materialized: Materialized) {
        self.changed_tables = materialized.tables;
        self.notices = materialized.notices;
        self.more |= materialized.stale_left;
    }

    pub(crate) fn absorb(&mut self, other: Self) {
        self.more |= other.more;
        self.sealed_ops += other.sealed_ops;
        self.changes += other.changes;
        self.last_seq = other.last_seq.or(self.last_seq);
        self.invalid_rows.extend(other.invalid_rows);
        self.foreign_owned_rows += other.foreign_owned_rows;
        for table in other.changed_tables {
            if !self.changed_tables.contains(&table) {
                self.changed_tables.push(table);
            }
        }
        for notice in other.notices {
            if !self.notices.contains(&notice) {
                self.notices.push(notice);
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
    revision: u64,
    context: &VersionVector,
    bytes: &[u8],
    continues: bool,
    now_ms: u64,
) -> SyncResult<()> {
    let order = log::next_stored_order(conn, &ctx.space).await?;
    sqlx::query(
        "INSERT INTO sync_ops
            (space_id, writer_id, seq, kind, hash, clock, authorization_revision, context,
             envelope, state, hold_reason, stored_at_ms, stored_order, group_continues)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'applied', NULL, ?, ?, ?)",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .bind(i64::from(kind))
    .bind(hash.as_slice())
    .bind(clamp_ms(clock.as_u64()))
    .bind(seq_i64(revision)?)
    .bind(context.encode())
    .bind(bytes)
    .bind(clamp_ms(now_ms))
    .bind(order)
    .bind(continues)
    .execute(&mut *conn)
    .await?;
    if kind == OperationKind::Genesis.as_u8() || kind == OperationKind::Authorization.as_u8() {
        crate::records::admit(conn, ctx.space, bytes, now_ms).await?;
    }
    Ok(())
}

/// Header fields an operation keeps when a new writer re-seals it, so it merges exactly as the
/// original did.
#[derive(Debug, Clone)]
pub(crate) struct Replay<'a> {
    /// Original clock.
    pub(crate) clock: Hlc,
    /// Original manifest version.
    pub(crate) manifest_version: u32,
    /// Original causal context with the replaced writer's operations renamed to their
    /// replacements.
    pub(crate) context: VersionVector,
    /// Writers the selected state excludes operations of. The original context may cover
    /// operations past a removal cutoff that never apply, which the replacement still names.
    pub(crate) excluding: &'a BTreeMap<WriterId, Excluding>,
    /// Whether the original operation's apply group continues into a later replayed
    /// operation.
    pub(crate) continues: bool,
}

/// The kind of an operation that only advances its dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Advance {
    /// An operation without changes, which acknowledges like any other operation.
    Changes,
    /// A signed acknowledgement, which never makes another writer acknowledge.
    Acknowledgement,
}

impl Advance {
    fn content(self) -> Content {
        match self {
            Self::Changes => Content::Changes(Vec::new()),
            Self::Acknowledgement => Content::Acknowledgement,
        }
    }
}

/// The local writer's chain position while sealing.
#[derive(Debug)]
pub(crate) struct Sealer {
    id: WriterId,
    stored: Seq,
    head_hash: Digest32,
    head_context: VersionVector,
    clock: HlcClock,
    /// Revision content operations are stamped with: the selected authorization revision when
    /// the sealer opened, never below the writer's own earlier operations.
    revision: u64,
    /// Whether the last sealed operation marks the writer's next one as continuing its set of
    /// changes. Receivers cut the set at the bounds of [`ends_group`] and still read it as one
    /// chain, so an operation sealed inside the set keeps the mark past a cut. Every call
    /// closes its sets, so a sealer opens with none.
    continues: bool,
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
            || SignedCertificate::decode(&certificate.encode()).is_err()
        {
            return Err(SyncError::WriterMismatch);
        }
        let id = cert.writer_id();
        let revision = authorization::sealing_revision(conn, ctx.space, id).await?;
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
            return Self::genesis(engine, conn, ctx, batch, writer, clock, revision, now_ms).await;
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
            revision,
            continues: false,
        })
    }

    /// Seals the genesis of a writer of an enrolled device that is not removed.
    #[allow(clippy::too_many_arguments)]
    async fn genesis(
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        mut clock: HlcClock,
        revision: u64,
        now_ms: u64,
    ) -> SyncResult<Self> {
        let certificate = writer.certificate;
        let cert = &certificate.certificate;
        match authorization::enrollment(conn, ctx.space, &cert.device_id, &cert.device_key).await? {
            Enrollment::Enrolled => {}
            Enrollment::WrongKey => return Err(SyncError::WriterMismatch),
            Enrollment::Removed => return Err(SyncError::DeviceRemoved),
            Enrollment::Absent => return Err(SyncError::NotEnrolled),
        }
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
                authorization_revision: revision,
                key_epoch: 0,
                group_continues: false,
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
            revision,
            &context,
            sealed.bytes(),
            false,
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
            revision,
            continues: false,
        })
    }

    /// Every dependency the next operation needs: what the target context, everything applied
    /// unless a replay names one, adds to the head context, in writer id order.
    ///
    /// Without a replay the head context joins the target: a removal exclusion can take back
    /// applied operations the head context saw, and a context never shrinks along a chain. A
    /// replayed context may name operations the selected state excludes, as far as an honest
    /// operation may wait for them, see [`exclusion::ready`].
    fn dependencies(
        &self,
        batch: &Batch,
        replay: Option<&Replay<'_>>,
    ) -> SyncResult<Vec<(WriterId, Seq)>> {
        let mut head = self.head_context.clone();
        head.set(self.id, 0);
        let mut target = match replay {
            Some(replay) => replay.context.clone(),
            None => {
                let mut target = batch.applied.clone();
                target.merge(&head);
                target
            }
        };
        target.set(self.id, 0);
        let bounded = match replay {
            Some(replay) => exclusion::ready(&batch.applied, &target, replay.excluding),
            None => {
                let mut bound = batch.applied.clone();
                bound.merge(&head);
                bound.dominates(&target)
            }
        };
        if !target.dominates(&head) || !bounded {
            return Err(corrupt(
                "an operation context must lie between its head and the applied state",
            ));
        }
        Ok(target.delta_since(&self.head_context))
    }

    /// Dependencies of the next operation once they fit one header. While more writers
    /// advanced than a header lists, this seals operations of the `advance` kind, each taking
    /// the next [`MAX_DEPENDENCIES`] writers in id order. Inside an open apply group they join
    /// it, so the operation that follows them still applies with the group.
    #[allow(clippy::too_many_arguments)]
    async fn catch_up(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        advance: Advance,
        replay: Option<&Replay<'_>>,
        now_ms: u64,
        report: &mut SealReport,
    ) -> SyncResult<Vec<(WriterId, Seq)>> {
        loop {
            let mut dependencies = self.dependencies(batch, replay)?;
            if dependencies.len() <= MAX_DEPENDENCIES {
                return Ok(dependencies);
            }
            dependencies.truncate(MAX_DEPENDENCIES);
            let continues = self.continues && advance == Advance::Changes;
            let (seq, _, _) = self
                .seal_operation(
                    engine,
                    conn,
                    ctx,
                    batch,
                    writer,
                    dependencies,
                    advance.content(),
                    replay,
                    continues,
                    now_ms,
                )
                .await?;
            report.sealed_ops += 1;
            report.last_seq = Some(seq);
        }
    }

    /// Seals an operation of the `advance` kind that advances to the replay context, or to
    /// everything applied without one. A replay re-seals one such operation of a replaced
    /// chain.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn seal_advance(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        advance: Advance,
        replay: Option<&Replay<'_>>,
        now_ms: u64,
        report: &mut SealReport,
    ) -> SyncResult<()> {
        let dependencies = self
            .catch_up(
                engine, conn, ctx, batch, writer, advance, replay, now_ms, report,
            )
            .await?;
        let continues =
            advance == Advance::Changes && replay.is_some_and(|replay| replay.continues);
        let (seq, _, _) = self
            .seal_operation(
                engine,
                conn,
                ctx,
                batch,
                writer,
                dependencies,
                advance.content(),
                replay,
                continues,
                now_ms,
            )
            .await?;
        report.sealed_ops += 1;
        report.last_seq = Some(seq);
        Ok(())
    }

    /// Seals one operation with `content`, returning its sequence, clock, and context. Changes
    /// are not applied here. With `continues` the writer's next operation joins its apply
    /// group, unless the group reached a bound of [`ends_group`], which receivers apply alike.
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
        replay: Option<&Replay<'_>>,
        continues: bool,
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
        // An authorization entry creating revision R is sealed against R - 1, so it never waits
        // on itself.
        let revision = match &content {
            Content::Authorization(record) => record.revision.saturating_sub(1),
            _ => self.revision,
        };
        let operation = Operation {
            space: ctx.space,
            writer: self.id,
            seq,
            previous_hash: self.head_hash,
            header: Header {
                clock,
                manifest_version,
                authorization_revision: revision,
                key_epoch: 0,
                group_continues: continues,
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
            revision,
            &context,
            sealed.bytes(),
            continues,
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
        self.continues = continues;
        Ok((seq, clock, context))
    }

    /// Seals changes into as few operations as the wire limits allow and applies them, so a
    /// receiver applies them in one transaction. Repairs, recovery discards, and replays seal
    /// this way: each is a set of changes that only holds whole.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn seal_changes(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        changes: Vec<Change>,
        replay: Option<&Replay<'_>>,
        now_ms: u64,
        report: &mut SealReport,
    ) -> SyncResult<()> {
        let whole = changes.len();
        self.seal_leading(
            engine, conn, ctx, batch, writer, changes, whole, replay, now_ms, report,
        )
        .await
    }

    /// Seals changes into as few operations as their exact sizes allow and applies them. The
    /// first `whole` changes fill operations up to the wire limits and form one apply group, so
    /// a receiver applies them in one transaction within the bounds of [`ends_group`]; an
    /// operation that carries more than [`MAX_CHANGES_PER_SEALED_OPERATION`] changes holds only
    /// such changes. The rest take at most [`MAX_CHANGES_PER_SEALED_OPERATION`] changes each.
    /// A replay keeps the group of the operation it re-seals.
    #[allow(clippy::too_many_arguments)]
    async fn seal_leading(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        changes: Vec<Change>,
        whole: usize,
        replay: Option<&Replay<'_>>,
        now_ms: u64,
        report: &mut SealReport,
    ) -> SyncResult<()> {
        let mut whole = whole.min(changes.len());
        let mut pending = changes.into_iter().peekable();
        while pending.peek().is_some() {
            // A replay keeps the operation it re-seals whole, as its changes share one clock.
            let per_operation = match replay {
                Some(_) => MAX_CHANGES_PER_OPERATION,
                None => whole.clamp(MAX_CHANGES_PER_SEALED_OPERATION, MAX_CHANGES_PER_OPERATION),
            };
            let dependencies = self
                .catch_up(
                    engine,
                    conn,
                    ctx,
                    batch,
                    writer,
                    Advance::Changes,
                    replay,
                    now_ms,
                    report,
                )
                .await?;
            let mut header = header_base(dependencies.len());
            let mut body = 0;
            let mut chunk = Vec::new();
            while let Some(change) = pending.peek() {
                let (change_header, change_body) = change_size(change);
                let full = chunk.len() == per_operation
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
            whole = whole.saturating_sub(count);
            // The leading changes form one apply group; a replay keeps the original group.
            let continues = match replay {
                Some(replay) => pending.peek().is_some() || replay.continues,
                None => whole > 0,
            };
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
                    continues,
                    now_ms,
                )
                .await?;
            for change in &chunk {
                let table = engine
                    .manifest()
                    .table(change.table)
                    .ok_or_else(|| corrupt("sealed change of an unknown table"))?;
                merge::apply_change(
                    engine, conn, batch, self.id, seq, op_clock, &context, table, change, false,
                )
                .await?;
            }
            report.sealed_ops += 1;
            report.changes += count;
            report.last_seq = Some(seq);
        }
        Ok(())
    }

    /// Seals an authorization entry in the anchor writer's chain and returns its sequence. The
    /// caller records the entry and selects the chain again.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn seal_authorization(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        batch: &mut Batch,
        writer: &mut LocalWriter<'_>,
        record: AuthorizationRecord,
        now_ms: u64,
        report: &mut SealReport,
    ) -> SyncResult<Seq> {
        let dependencies = self
            .catch_up(
                engine,
                conn,
                ctx,
                batch,
                writer,
                Advance::Changes,
                None,
                now_ms,
                report,
            )
            .await?;
        let (seq, _, _) = self
            .seal_operation(
                engine,
                conn,
                ctx,
                batch,
                writer,
                dependencies,
                Content::Authorization(Box::new(record)),
                None,
                false,
                now_ms,
            )
            .await?;
        report.sealed_ops += 1;
        report.last_seq = Some(seq);
        Ok(seq)
    }

    /// Seals every valid capture except those still held at `now_ms`, and deletes the
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
        let device = writer.certificate.certificate.device_id.as_str();
        let planned = plan_captures(engine, conn, captures, Some(device), &mut report).await?;
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

    /// Like [`Self::seal_captures`], taking one batch of at most [`MAX_CHANGES_PER_SEAL`]
    /// changes and [`MAX_BYTES_PER_SEAL`] change bytes. Captures of changes left for the
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
        let device = writer.certificate.certificate.device_id.as_str();
        let mut planned = plan_captures(engine, conn, captures, Some(device), &mut report).await?;
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

    /// Seals the captures of `rows`, held ones included, with the captured creates they
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
        let device = writer.certificate.certificate.device_id.as_str();
        let planned = plan_rows(engine, conn, rows, Some(device), report).await?;
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
        let (changes, together) = constrained_first(engine, planned.changes);
        self.seal_leading(
            engine, conn, ctx, batch, writer, changes, together, None, now_ms, report,
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
        let device = self
            .writer
            .as_ref()
            .map(|writer| writer.certificate.certificate.device_id.as_str());
        let planned = plan_rows(engine, conn, rows, device, &mut SealReport::default()).await?;
        Ok(budget.charge(&planned.changes))
    }

    /// Writer id of the local writer, sealing or not.
    pub(crate) fn local_id(&self) -> Option<WriterId> {
        self.writer.as_ref().map(|writer| writer.id())
    }

    pub(crate) fn frozen(&self) -> bool {
        self.frozen
    }

    /// Whether the slot could seal now: it has a writer, is not frozen, and the writer is active
    /// or, before its genesis, its device is enrolled. Unlike [`Self::try_open`] it never
    /// freezes the slot or seals a genesis.
    pub(crate) async fn may_seal(
        &mut self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<bool> {
        let Some(writer) = self.writer.as_deref() else {
            return Ok(false);
        };
        if self.frozen {
            return Ok(false);
        }
        if self.sealer.is_some() {
            return Ok(true);
        }
        let cert = &writer.certificate.certificate;
        Ok(
            match log::writer(conn, ctx.space, cert.writer_id()).await? {
                Some(row) => row.state == WriterState::Active,
                None => matches!(
                    authorization::enrollment(conn, ctx.space, &cert.device_id, &cert.device_key)
                        .await?,
                    Enrollment::Enrolled
                ),
            },
        )
    }

    /// What the slot may still seal, for [`Self::rewind`] after a savepoint rolls back.
    pub(crate) fn checkpoint(&self) -> Option<SealBudget> {
        self.budget.clone()
    }

    /// Returns to a checkpoint once the savepoint it was taken before rolled back. The sealer
    /// reopens from the stored chain on next use, since operations it sealed since are gone.
    pub(crate) fn rewind(&mut self, budget: Option<SealBudget>) {
        self.budget = budget;
        self.sealer = None;
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
                Err(
                    SyncError::WriterInactive(_)
                    | SyncError::DeviceRemoved
                    | SyncError::NotEnrolled,
                ) => {
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
        // A present derived-key row exists.
        let group_fields: Vec<Field> = if table.is_exists_group(group.id) {
            vec![Field::Integer(1)]
        } else {
            let count = group.columns().len();
            let slice = &fields[offset..offset + count];
            offset += count;
            let Some(mut group_fields) = slice.iter().cloned().collect::<Option<Vec<Field>>>()
            else {
                return Ok(DomainRow::Invalid("unreadable column"));
            };
            for owned in group.owned_tables() {
                let Some(rows) = sql::read_owned(conn, owned, key).await? else {
                    return Ok(DomainRow::Invalid("unreadable owned row"));
                };
                match codec::encode_table(engine, table, owned, rows) {
                    Ok(bytes) => group_fields.push(Field::Blob(bytes)),
                    Err(reason) => return Ok(DomainRow::Invalid(reason)),
                }
            }
            group_fields
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

/// Reads one group value of a domain row as an operation would carry it, or `None` when the
/// row is absent or its value is invalid.
pub(crate) async fn read_group_value(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: &TableSpec,
    group: &GroupSpec,
    key: &str,
) -> SyncResult<Option<Value>> {
    let columns: Vec<&str> = group.columns().iter().map(|column| column.name).collect();
    let Some(fields) = sql::read_columns(conn, table.name, table.key_column, key, &columns).await?
    else {
        return Ok(None);
    };
    let Some(mut group_fields) = fields.into_iter().collect::<Option<Vec<Field>>>() else {
        return Ok(None);
    };
    for owned in group.owned_tables() {
        let Some(rows) = sql::read_owned(conn, owned, key).await? else {
            return Ok(None);
        };
        match codec::encode_table(engine, table, owned, rows) {
            Ok(bytes) => group_fields.push(Field::Blob(bytes)),
            Err(_) => return Ok(None),
        }
    }
    let Ok(value) = Value::new(group_fields) else {
        return Ok(None);
    };
    Ok(validate::group_value(engine, table, group, &value)
        .is_ok()
        .then_some(value))
}

/// Most operations one seal transaction commits. A receiver applies them over several
/// transactions of at most [`crate::MAX_OPS_PER_APPLY`] operations each.
///
/// The seal cap stays well above the apply cap because every seal call plans the whole
/// capture backlog before it selects a batch, and deletions cascade only among deletions
/// planned together. Small seal batches would make a large first seal quadratic and split
/// cascade families across operations.
pub const MAX_OPS_PER_SEAL: usize = 256;
/// Most change bytes one seal transaction commits, matching what one apply transaction takes
/// after its first operation.
pub const MAX_BYTES_PER_SEAL: usize = MAX_BYTES_PER_APPLY;

/// Most changes one sealed operation carries, so it applies within one apply transaction of
/// [`MAX_CHANGES_PER_APPLY`] changes, unless the changes must apply together: the writes and
/// tombstones of tables whose constraint reads several rows together, cascaded deletions, a
/// published parent with its `published_with` children, and the changes they depend on fill
/// operations up to the wire limits instead.
pub(crate) const MAX_CHANGES_PER_SEALED_OPERATION: usize =
    if MAX_CHANGES_PER_APPLY < MAX_CHANGES_PER_OPERATION {
        MAX_CHANGES_PER_APPLY
    } else {
        MAX_CHANGES_PER_OPERATION
    };

/// Most changes one seal transaction commits: [`MAX_OPS_PER_SEAL`] full sealed operations.
pub const MAX_CHANGES_PER_SEAL: usize =
    MAX_OPS_PER_SEAL.saturating_mul(MAX_CHANGES_PER_SEALED_OPERATION);

/// Encoded bytes of `change` in its operation, header and body.
fn change_cost(change: &Change) -> usize {
    let (header, body) = change_size(change);
    header + body
}

/// Change bytes and changes one call has sealed, within one seal transaction.
#[derive(Debug, Clone, Default)]
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

/// Rows a change depends on: its key parent, the rows its reference columns and owned rows
/// name, and the cause and redirect target of a tombstone. Sealing a change without the create
/// of such a row would materialize the reference as null, leave the owned row out, or hide the
/// row.
pub(crate) fn change_dependencies(engine: &Engine, change: &Change) -> Vec<RowRef> {
    let Some(table) = engine.manifest().table(change.table) else {
        return Vec::new();
    };
    let mut rows: Vec<RowRef> = table
        .key_parent
        .map(|parent| (parent, change.row.as_str().to_string()))
        .into_iter()
        .collect();
    for (group, field, reference) in table.references() {
        let value = change.groups.iter().find(|value| value.group == group.id);
        if let Some(Field::Text(key)) = value.and_then(|value| value.value.fields().get(field)) {
            rows.push((reference.table, key.clone()));
        }
    }
    for value in &change.groups {
        // A sealed value was validated, so its owned blobs decode.
        if let Some(group) = table.group(value.group)
            && let Ok(targets) = codec::owned_targets(group, &value.value)
        {
            rows.extend(targets.into_iter().map(|(_, target, key)| (target, key)));
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

/// Positions of the tombstones that name a cause, by the row they name.
fn cascaded_changes(changes: &[Change]) -> BTreeMap<RowRef, Vec<usize>> {
    let mut cascaded: BTreeMap<RowRef, Vec<usize>> = BTreeMap::new();
    for (position, change) in changes.iter().enumerate() {
        if let Some(cause) = &change.cause {
            cascaded
                .entry((cause.table, cause.row.as_str().to_string()))
                .or_default()
                .push(position);
        }
    }
    cascaded
}

/// Positions of the creates of `published_with` children, by the created parent row they name.
/// Such a parent and those children publish together, see [`crate::manifest::PublishedWith`].
fn published_changes(
    engine: &Engine,
    changes: &[Change],
    index: &BTreeMap<RowRef, usize>,
) -> BTreeMap<RowRef, Vec<usize>> {
    let mut published: BTreeMap<RowRef, Vec<usize>> = BTreeMap::new();
    for (position, change) in changes.iter().enumerate() {
        if change.action != ChangeAction::Create {
            continue;
        }
        let Some(table) = engine.manifest().table(change.table) else {
            continue;
        };
        let Some(with) = table.published_with else {
            continue;
        };
        let parent = table
            .references()
            .filter(|(_, _, reference)| reference.column == with.column)
            .find_map(|(group, field, _)| {
                let value = change.groups.iter().find(|value| value.group == group.id)?;
                match value.value.fields().get(field) {
                    Some(Field::Text(key)) => Some((with.parent, key.clone())),
                    _ => None,
                }
            });
        let Some(parent) = parent else {
            continue;
        };
        let created = index
            .get(&parent)
            .is_some_and(|parent| changes[*parent].action == ChangeAction::Create);
        if created {
            published.entry(parent).or_default().push(position);
        }
    }
    published
}

/// Planned changes in sealed order, and how many lead it. The writes and tombstones of tables
/// whose constraint reads several rows together, every cascaded deletion, and every published
/// parent with its `published_with` children lead, with the changes they depend on, the
/// deletions that name a leading row as their cause, and the children a leading row publishes,
/// then the rest follows, each part in planned order.
///
/// A receiver may apply a seal over several transactions, and neither the repairs it seals at
/// the end of each nor the domain that reads its rows may see part of these changes: a create
/// that reuses a value before the rename that frees it, a task nested under a parent that has
/// not moved yet, a deleted root whose cascaded rows are still live, or a closed Focus run
/// without its segments. The leading changes therefore seal in operations a receiver applies
/// in one transaction.
fn constrained_first(engine: &Engine, changes: Vec<Change>) -> (Vec<Change>, usize) {
    let constrained = constrained_changes(engine, &changes);
    let cascaded = cascaded_changes(&changes);
    let index = change_index(&changes);
    let published = published_changes(engine, &changes, &index);
    if constrained.is_empty() && cascaded.is_empty() && published.is_empty() {
        return (changes, 0);
    }
    let mut first = vec![false; changes.len()];
    let mut pending: Vec<usize> = constrained
        .into_values()
        .chain(cascaded.values().cloned())
        .chain(published.values().cloned())
        .flatten()
        .collect();
    while let Some(position) = pending.pop() {
        if std::mem::replace(&mut first[position], true) {
            continue;
        }
        let change = &changes[position];
        pending.extend(
            change_dependencies(engine, change)
                .into_iter()
                .filter_map(|row| index.get(&row).copied()),
        );
        let row = (change.table, change.row.as_str().to_string());
        pending.extend(
            cascaded
                .get(&row)
                .into_iter()
                .chain(published.get(&row))
                .flatten()
                .copied(),
        );
    }
    let (mut ordered, rest): (Vec<_>, Vec<_>) = changes
        .into_iter()
        .zip(first)
        .partition(|(_, first)| *first);
    let leading = ordered.len();
    ordered.extend(rest);
    (
        ordered.into_iter().map(|(change, _)| change).collect(),
        leading,
    )
}

/// The changes one seal transaction takes, in planned order, within [`MAX_BYTES_PER_SEAL`]
/// and [`MAX_CHANGES_PER_SEAL`], and whether changes remain. A change is taken
/// together with the planned changes of every row it depends on, so a batch never names a
/// row that a later batch creates.
///
/// Writes and tombstones of a table whose constraint reads several rows together seal as one
/// group ahead of every other change, so a create never seals before the rename or delete that
/// frees its unique value, tree position, or edge, and repairs never act on the stale value.
/// A deleted row is taken with every deletion that names it as its cause, so a cascade seals
/// whole, and a published parent with the `published_with` children it publishes.
fn select_batch(engine: &Engine, changes: Vec<Change>) -> (Vec<Change>, Vec<Change>) {
    let index = change_index(&changes);
    let constrained = constrained_changes(engine, &changes);
    let cascaded = cascaded_changes(&changes);
    let published = published_changes(engine, &changes, &index);
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
            let change = &changes[position];
            let row = (change.table, change.row.as_str().to_string());
            let together = constrained
                .get(&change.table)
                .into_iter()
                .chain(cascaded.get(&row))
                .chain(published.get(&row))
                .flatten()
                .copied();
            let dependencies = change_dependencies(engine, change)
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

impl Engine {
    /// Seals local captures with the local writer, sealing its genesis first when needed.
    ///
    /// One call commits at most [`MAX_OPS_PER_SEAL`] operations, [`MAX_CHANGES_PER_SEAL`]
    /// changes, and [`MAX_BYTES_PER_SEAL`] change bytes; [`SealReport::more`] asks the caller
    /// to seal again at once. Captures held past `now_ms` stay captured;
    /// [`crate::hold::next_hold_expiry`] says when to seal them.
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
        let materialized = finish_sealed(
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
        report.record(materialized);
        failpoint!("seal.before_commit");
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(report)
    }
}
