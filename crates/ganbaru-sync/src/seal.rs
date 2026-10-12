//! Sealing local captures into signed operations of the local writer.
//!
//! A sealed operation is applied to the merge state as it is sealed, with the context of
//! everything applied so far, so local writes supersede every version they were made over.

use std::collections::BTreeMap;

use ganbaru_sync_contracts::bounds::{
    MAX_CHANGES_PER_OPERATION, MAX_DEPENDENCIES, MAX_HEADER_BYTES, MAX_OPERATION_BYTES,
};
use ganbaru_sync_contracts::op::{ENVELOPE_OVERHEAD, GENESIS_PREVIOUS_HASH};
use ganbaru_sync_contracts::{
    Change, ChangeAction, Content, Digest32, Field, GroupMask, GroupValue, Header, Hlc, HlcClock,
    Operation, Revocation, RevokeReason, RowKey, Seq, SignedCertificate, TableId, Value,
    VersionVector, WriterId,
};
use sqlx::{Connection, Row, SqliteConnection};

use crate::apply::finish;
use crate::error::{SyncResult, corrupt, failpoint};
use crate::log::{self, WriterState, clamp_ms, seq_i64};
use crate::manifest::{GroupStorage, MergeKind, TableSpec};
use crate::merge::{self, Batch, RowState, mask_param, table_param};
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
}

impl SealReport {
    pub(crate) fn absorb(&mut self, other: Self) {
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
        header += match &change.replaced_by {
            Some(target) => 1 + 1 + target.as_str().len(),
            None => 1,
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

    /// Dependencies of the next operation: what the target context, everything applied unless
    /// a replay names one, adds to the head context.
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
        let dependencies = target.delta_since(&self.head_context);
        if dependencies.len() > MAX_DEPENDENCIES {
            return Err(SyncError::TooManyWriters);
        }
        Ok(dependencies)
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
            let dependencies = self.dependencies(batch, replay)?;
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
    ) -> SyncResult<Seq> {
        let dependencies = self.dependencies(batch, replay)?;
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
        Ok(seq)
    }

    /// Seals every valid capture and deletes the captures it consumed.
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
        let planned = plan_captures(engine, conn, &mut report).await?;
        self.seal_changes(
            engine,
            conn,
            ctx,
            batch,
            writer,
            planned.changes,
            None,
            now_ms,
            &mut report,
        )
        .await?;
        delete_captures(conn, &planned.consumed).await?;
        Ok(report)
    }
}

/// The sealer slot of an engine call: the local writer it may seal with, opened on first use.
pub(crate) struct SealSlot<'a, 'k> {
    writer: Option<&'a mut LocalWriter<'k>>,
    sealer: Option<Sealer>,
    frozen: bool,
}

impl<'a, 'k> SealSlot<'a, 'k> {
    pub(crate) fn new(writer: Option<&'a mut LocalWriter<'k>>) -> Self {
        Self {
            writer,
            sealer: None,
            frozen: false,
        }
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
    deleted: bool,
    replaced_by: Option<String>,
    /// Sync state of the row when planning started.
    state: Option<RowState>,
}

/// Changes planned from captures and the captures they consume.
struct PlannedCaptures {
    changes: Vec<Change>,
    consumed: Vec<(i64, String)>,
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
    }
}

async fn plan_captures(
    engine: &Engine,
    conn: &mut SqliteConnection,
    report: &mut SealReport,
) -> SyncResult<PlannedCaptures> {
    let rows = sqlx::query(
        "SELECT c.table_id, c.row_key, c.mask, c.forced_mask, c.deleted, c.replaced_by, r.state
         FROM sync_capture c
         LEFT JOIN sync_rows r ON r.table_id = c.table_id AND r.row_key = c.row_key
         ORDER BY c.table_id, c.row_key",
    )
    .fetch_all(&mut *conn)
    .await?;
    let captures = rows
        .iter()
        .map(|row| {
            let mask: i64 = row.try_get(2)?;
            let forced: i64 = row.try_get(3)?;
            Ok(Capture {
                table: row.try_get(0)?,
                key: row.try_get(1)?,
                mask: GroupMask((mask | forced) as u64),
                deleted: row.try_get::<i64, _>(4)? != 0,
                replaced_by: row.try_get(5)?,
                state: merge::parse_row_state(row.try_get::<Option<&str>, _>(6)?)?,
            })
        })
        .collect::<SyncResult<Vec<_>>>()?;
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
            (None | Some(RowState::Tombstoned), DomainRow::Absent) => None,
            (None, DomainRow::Present(groups)) => Some(Change {
                table: table.id,
                row: key.clone(),
                action: ChangeAction::Create,
                groups,
                replaced_by: None,
            }),
            // An absent live row without a local delete is hidden by the merge; its repair is
            // sealed by the engine, not from the capture.
            (Some(RowState::Live), DomainRow::Absent) => capture
                .deleted
                .then(|| tombstone(table, &key, capture.replaced_by.as_deref())),
            (Some(RowState::Live), DomainRow::Present(groups)) => {
                let groups: Vec<GroupValue> = groups
                    .into_iter()
                    .filter(|value| {
                        capture.mask.contains(value.group)
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
            Some(change) if change.action == ChangeAction::Create => {
                creates.push((position, change));
            }
            Some(change) if change.action == ChangeAction::Write => writes.push((position, change)),
            Some(change) => tombstones.push((usize::MAX - position, change)),
            None => {}
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
    Ok(PlannedCaptures { changes, consumed })
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
                .seal_captures(self, &mut tx, ctx, &mut batch, writer, now_ms)
                .await?;
        }
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
        let seq = sealer
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
            )
            .await?;
        report.sealed_ops += 1;
        report.last_seq = Some(seq);
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
        let mut query = sqlx::query(&sql).persistent(chunk.len() == CAPTURE_DELETE_CHUNK);
        for (table, key) in chunk {
            query = query.bind(*table).bind(key.as_str());
        }
        query.execute(&mut *conn).await?;
    }
    Ok(())
}
