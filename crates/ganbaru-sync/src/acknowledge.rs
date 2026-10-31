//! Acknowledgements: signed operations without changes that record what the local writer
//! applied, so the coordinator can tell which operations every counted device holds.
//!
//! A writer acknowledges after applying an operation of another writer that is not an
//! acknowledgement, at most once per [`ACKNOWLEDGEMENT_INTERVAL_MS`], and seals a keepalive
//! acknowledgement when its newest operation is older than [`KEEPALIVE_INTERVAL_MS`].
//! Acknowledgements of other writers are never a reason to acknowledge, so acknowledgement
//! traffic settles once every device has acknowledged the latest changes.

use ganbaru_sync_contracts::{VersionVector, WriterId};
use sqlx::{Connection, SqliteConnection};

use crate::error::{SyncResult, corrupt, failpoint};
use crate::log::{self, WriterRow, WriterState, seq_i64, stored_u64};
use crate::merge::Batch;
use crate::seal::{Advance, SealReport, SealSlot};
use crate::{Engine, LocalWriter, SpaceContext, SyncError, sql};

/// Shortest time between two acknowledgements of one writer that new operations of other
/// writers trigger.
pub const ACKNOWLEDGEMENT_INTERVAL_MS: u64 = 10 * 60 * 1000;

/// Age of a writer's newest operation after which it seals a keepalive acknowledgement, so
/// the coordinator does not count the device as dormant while it still syncs.
pub const KEEPALIVE_INTERVAL_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// Kind byte of acknowledgements in `sync_ops`.
const ACKNOWLEDGEMENT_KIND: i64 = 3;

impl Engine {
    /// Seals the local writer's genesis when the space does not know it yet, and an
    /// acknowledgement when one is due.
    ///
    /// Commits nothing when neither is needed, so an idle pass does not wake waiting
    /// exchanges. More writers advanced than one header lists seal several acknowledgements in
    /// one transaction.
    pub async fn acknowledge(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<SealReport> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        let id = writer.id();
        let row = log::writer(&mut tx, ctx.space, id).await?;
        if let Some(row) = &row
            && row.state != WriterState::Active
        {
            return Err(SyncError::WriterInactive(id));
        }
        let mut batch = Batch::load(&mut tx, ctx.space).await?;
        let due = match &row {
            Some(row) => acknowledgement_due(&mut tx, ctx, row, &mut batch, now_ms).await?,
            None => {
                others_advanced(&mut tx, ctx, id, &VersionVector::new(), &batch.applied).await?
            }
        };
        let genesis = row.is_none();
        if !genesis && !due {
            return Ok(SealReport::default());
        }
        sql::begin_engine_write(&mut tx).await?;
        let mut slot = SealSlot::new(Some(writer));
        let mut report = SealReport::default();
        let (sealer, writer) = slot
            .open(self, &mut tx, ctx, &mut batch, now_ms)
            .await?
            .ok_or_else(|| corrupt("sealer slot has no writer"))?;
        if genesis {
            report.sealed_ops += 1;
            report.last_seq = Some(1);
        }
        if due {
            sealer
                .seal_advance(
                    self,
                    &mut tx,
                    ctx,
                    &mut batch,
                    writer,
                    Advance::Acknowledgement,
                    None,
                    now_ms,
                    &mut report,
                )
                .await?;
        }
        failpoint!("acknowledge.before_commit");
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(report)
    }
}

/// Whether the existing local writer owes an acknowledgement at `now_ms`.
async fn acknowledgement_due(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    row: &WriterRow,
    batch: &mut Batch,
    now_ms: u64,
) -> SyncResult<bool> {
    let newest = newest_stored_at(conn, ctx, row.id, None).await?;
    if newest.is_none_or(|at| now_ms.saturating_sub(at) >= KEEPALIVE_INTERVAL_MS) {
        return Ok(true);
    }
    if let Some(at) = newest_stored_at(conn, ctx, row.id, Some(ACKNOWLEDGEMENT_KIND)).await?
        && now_ms.saturating_sub(at) < ACKNOWLEDGEMENT_INTERVAL_MS
    {
        return Ok(false);
    }
    let head = batch
        .contexts
        .get(conn, ctx.space, row.id, row.stored)
        .await?
        .clone();
    others_advanced(conn, ctx, row.id, &head, &batch.applied).await
}

/// Whether `applied` covers an operation of another writer beyond `head` that is not an
/// acknowledgement.
async fn others_advanced(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    own: WriterId,
    head: &VersionVector,
    applied: &VersionVector,
) -> SyncResult<bool> {
    for (writer, seq) in applied.iter() {
        let known = head.get(&writer);
        if writer == own || seq <= known {
            continue;
        }
        let found: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM sync_ops
             WHERE space_id = ? AND writer_id = ? AND seq > ? AND seq <= ? AND kind <> ?
             LIMIT 1",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(known)?)
        .bind(seq_i64(seq)?)
        .bind(ACKNOWLEDGEMENT_KIND)
        .fetch_optional(&mut *conn)
        .await?;
        if found.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// When the newest stored operation of `writer`, of `kind` when given, was stored.
async fn newest_stored_at(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: WriterId,
    kind: Option<i64>,
) -> SyncResult<Option<u64>> {
    let newest: Option<i64> = sqlx::query_scalar(
        "SELECT max(stored_at_ms) FROM sync_ops
         WHERE space_id = ? AND writer_id = ? AND (? IS NULL OR kind = ?)",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(kind)
    .bind(kind)
    .fetch_one(&mut *conn)
    .await?;
    newest
        .map(|at| stored_u64(at, "operation storage time"))
        .transpose()
}
