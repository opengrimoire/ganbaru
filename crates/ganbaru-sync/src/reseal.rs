//! Re-sealing a local writer's operations with a new writer.
//!
//! When a revocation or a fork cuts off operations this replica sealed and applied, the
//! installation replaces its writer. Every operation of the old writer past the kept sequence is
//! removed with its merge effects and sealed again by the new writer with its original clock,
//! manifest version, and causal context, where removed operations of the old writer are renamed
//! to their replacements. Operations of other writers stay as they are, so the merge state after
//! the re-seal is the one every replica computes from the kept chain and the replacements.

use std::collections::BTreeMap;

use ganbaru_sync_contracts::{Content, Envelope, Operation, Seq, TableId, VersionVector, WriterId};
use sqlx::{Connection, Row, SqliteConnection};

use crate::apply::finish;
use crate::error::{SyncResult, corrupt, failpoint};
use crate::log::{self, seq_i64, stored_u64};
use crate::merge::{Batch, table_param};
use crate::seal::{Replay, SealReport, SealSlot};
use crate::{Engine, LocalWriter, SpaceContext, SyncError, sql};

/// Result of a re-seal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResealReport {
    /// Operations of the old writer removed and sealed again.
    pub replaced_ops: usize,
    /// Operations the new writer sealed: replacements first, then pending captures and repairs.
    pub seal: SealReport,
}

/// A removed operation of the old writer.
struct Replaced {
    seq: Seq,
    operation: Operation,
    context: VersionVector,
}

async fn replaced_ops(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    old: WriterId,
    keep_through: Seq,
) -> SyncResult<Vec<Replaced>> {
    let rows = sqlx::query(
        "SELECT seq, envelope, context, state FROM sync_ops
         WHERE space_id = ? AND writer_id = ? AND seq > ? ORDER BY seq",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(old.as_bytes().as_slice())
    .bind(seq_i64(keep_through)?)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(|row| {
            let state: String = row.try_get(3)?;
            if state != "applied" {
                return Err(corrupt("a replaced operation is not applied"));
            }
            let envelope: Vec<u8> = row.try_get(1)?;
            let context: Vec<u8> = row.try_get(2)?;
            let operation = Envelope::decode(&envelope)
                .ok()
                .and_then(|envelope| envelope.operation().ok())
                .ok_or_else(|| corrupt("an applied operation does not decode"))?;
            Ok(Replaced {
                seq: stored_u64(row.try_get(0)?, "sequence")?,
                operation,
                context: log::decode_context(&context)?
                    .ok_or_else(|| corrupt("context of an applied operation is unknown"))?,
            })
        })
        .collect()
}

/// Fails when an applied operation of another writer saw a replaced operation. Contexts only
/// grow along a chain, so checking each writer's applied head is enough.
async fn ensure_unseen(
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    old: WriterId,
    keep_through: Seq,
) -> SyncResult<()> {
    let heads: Vec<(WriterId, Seq)> = batch
        .applied
        .iter()
        .filter(|(writer, _)| *writer != old)
        .collect();
    for (writer, applied) in heads {
        let context = batch
            .contexts
            .get(conn, batch.space, writer, applied)
            .await?;
        if context.get(&old) > keep_through {
            return Err(SyncError::InvalidReseal(
                "another writer's applied operations depend on replaced operations",
            ));
        }
    }
    Ok(())
}

/// A replaced operation's context with removed operations of the old writer renamed.
fn remap(
    context: &VersionVector,
    old: WriterId,
    keep_through: Seq,
    new: WriterId,
    renamed: &BTreeMap<Seq, Seq>,
) -> SyncResult<VersionVector> {
    let mut remapped = VersionVector::new();
    for (writer, seq) in context.iter() {
        if writer == new {
            return Err(corrupt("a replaced operation saw its replacement writer"));
        }
        if writer != old || seq <= keep_through {
            remapped.advance(writer, seq);
            continue;
        }
        remapped.advance(old, keep_through);
        let replacement = renamed.get(&seq).ok_or_else(|| {
            corrupt("a replaced operation saw an operation that was not replaced")
        })?;
        remapped.advance(new, *replacement);
    }
    Ok(remapped)
}

/// Removes the versions and tombstones of the old writer's operations past `keep_through`,
/// revives rows left without tombstones, and marks every touched row dirty.
async fn remove_effects(
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    old: WriterId,
    keep_through: Seq,
) -> SyncResult<()> {
    let keep = seq_i64(keep_through)?;
    let touched: Vec<(i64, String)> = sqlx::query_as(
        "SELECT table_id, row_key FROM sync_register_versions WHERE writer_id = ? AND seq > ?
         UNION SELECT table_id, row_key FROM sync_tombstones WHERE writer_id = ? AND seq > ?",
    )
    .bind(old.as_bytes().as_slice())
    .bind(keep)
    .bind(old.as_bytes().as_slice())
    .bind(keep)
    .fetch_all(&mut *conn)
    .await?;
    for table in ["sync_register_versions", "sync_tombstones"] {
        sqlx::query(&format!(
            "DELETE FROM {table} WHERE writer_id = ? AND seq > ?"
        ))
        .bind(old.as_bytes().as_slice())
        .bind(keep)
        .execute(&mut *conn)
        .await?;
    }
    for (table_id, key) in touched {
        let table = u16::try_from(table_id)
            .map(TableId)
            .map_err(|_| corrupt("merge state of a table id out of range"))?;
        // Replacements re-create every removed version and tombstone, so a row is never left
        // without merge state once the re-seal commits.
        sqlx::query(
            "UPDATE sync_rows SET state = 'live', recovery = 0
             WHERE table_id = ? AND row_key = ? AND state = 'tombstoned'
                AND NOT EXISTS (
                    SELECT 1 FROM sync_tombstones t
                    WHERE t.table_id = sync_rows.table_id AND t.row_key = sync_rows.row_key
                )",
        )
        .bind(table_param(table))
        .bind(&key)
        .execute(&mut *conn)
        .await?;
        batch.dirty.insert((table, key));
    }
    Ok(())
}

/// Removes the old writer's operations past `keep_through` and makes `keep_through` its head.
/// After a revocation the writer is retired and its cutoff is lowered to it, so another copy of
/// the removed operations is refused before the revocation applies. After a fork its state and
/// cutoff stay, so the other copy of the chain past the divergence can be stored.
async fn truncate_chain(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    old: WriterId,
    keep_through: Seq,
    cause: ResealCause,
) -> SyncResult<()> {
    let keep = seq_i64(keep_through)?;
    let (head_hash, head_clock): (Vec<u8>, i64) = sqlx::query_as(
        "SELECT hash, clock FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq = ?",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(old.as_bytes().as_slice())
    .bind(keep)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| corrupt("kept head of a re-sealed chain is missing"))?;
    sqlx::query("DELETE FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq > ?")
        .bind(ctx.space.as_bytes().as_slice())
        .bind(old.as_bytes().as_slice())
        .bind(keep)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "UPDATE sync_writers SET stored_seq = ?1, applied_seq = ?1, head_hash = ?2,
            head_clock = ?3,
            cutoff_seq = CASE WHEN ?6 AND (cutoff_seq IS NULL OR cutoff_seq > ?1) THEN ?1
                ELSE cutoff_seq END,
            state = CASE WHEN ?6 AND state = 'active' THEN 'retired' ELSE state END
         WHERE space_id = ?4 AND writer_id = ?5",
    )
    .bind(keep)
    .bind(head_hash.as_slice())
    .bind(head_clock)
    .bind(ctx.space.as_bytes().as_slice())
    .bind(old.as_bytes().as_slice())
    .bind(cause == ResealCause::Revocation)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Why the old writer's operations are re-sealed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResealCause {
    /// A revocation cut the chain off at its cutoff.
    Revocation,
    /// Another copy of the chain diverges from this one after the kept sequence.
    Fork,
}

impl Engine {
    /// Replaces the old local writer's operations past `keep_through` with operations of a new
    /// writer that merge the same way, then seals pending captures with it. The old writer is
    /// retired unless it is already revoked or forked, and its cutoff is lowered to
    /// `keep_through`.
    ///
    /// `keep_through` is the revocation cutoff from [`crate::ResealNeeded`]. The new writer must
    /// not have sealed anything but its genesis.
    pub async fn reseal(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        old: WriterId,
        keep_through: Seq,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<ResealReport> {
        self.reseal_after(
            conn,
            ctx,
            old,
            keep_through,
            ResealCause::Revocation,
            writer,
            now_ms,
        )
        .await
    }

    /// Re-seals the local writer's operations past `keep_through`, the last sequence another
    /// copy of the chain shares with this one, like [`Engine::reseal`]. The old writer keeps its
    /// state and cutoff, so the other copy's operations past the divergence can be stored and
    /// applied afterwards. Only operations that never left this replica may be re-sealed this way.
    pub async fn reseal_fork(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        old: WriterId,
        keep_through: Seq,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<ResealReport> {
        self.reseal_after(
            conn,
            ctx,
            old,
            keep_through,
            ResealCause::Fork,
            writer,
            now_ms,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn reseal_after(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        old: WriterId,
        keep_through: Seq,
        cause: ResealCause,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<ResealReport> {
        let new = writer.id();
        if new == old {
            return Err(SyncError::InvalidReseal(
                "the new writer must differ from the old one",
            ));
        }
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        let Some(row) = log::writer(&mut tx, ctx.space, old).await? else {
            return Err(SyncError::InvalidReseal("the old writer is unknown"));
        };
        if row.stored != row.applied {
            return Err(SyncError::InvalidReseal(
                "the old writer has operations that are not applied",
            ));
        }
        if keep_through < 1 || keep_through > row.stored {
            return Err(SyncError::InvalidReseal(
                "the kept sequence is outside the old chain",
            ));
        }
        if let Some(existing) = log::writer(&mut tx, ctx.space, new).await?
            && existing.stored > 1
        {
            return Err(SyncError::InvalidReseal(
                "the new writer already sealed operations",
            ));
        }
        sql::begin_engine_write(&mut tx).await?;
        let mut batch = Batch::load(&mut tx, ctx.space).await?;
        ensure_unseen(&mut tx, &mut batch, old, keep_through).await?;
        let replaced = replaced_ops(&mut tx, ctx, old, keep_through).await?;
        remove_effects(&mut tx, &mut batch, old, keep_through).await?;
        truncate_chain(&mut tx, ctx, old, keep_through, cause).await?;
        batch.applied.set(old, keep_through);
        batch.contexts.clear();
        let mut report = ResealReport {
            replaced_ops: replaced.len(),
            seal: SealReport::default(),
        };
        let mut slot = SealSlot::new(Some(writer));
        let (sealer, writer) = slot
            .open(self, &mut tx, ctx, &mut batch, now_ms)
            .await?
            .ok_or_else(|| corrupt("sealer slot has no writer"))?;
        let mut renamed = BTreeMap::new();
        for op in replaced {
            let replay = Replay {
                clock: op.operation.header.clock,
                manifest_version: op.operation.header.manifest_version,
                context: remap(&op.context, old, keep_through, new, &renamed)?,
            };
            match op.operation.content {
                Content::Changes(changes) => {
                    sealer
                        .seal_changes(
                            self,
                            &mut tx,
                            ctx,
                            &mut batch,
                            writer,
                            changes,
                            Some(&replay),
                            now_ms,
                            &mut report.seal,
                        )
                        .await?;
                }
                Content::Revoke(revocation) => {
                    let seq = sealer
                        .seal_revoke(
                            self,
                            &mut tx,
                            ctx,
                            &mut batch,
                            writer,
                            revocation,
                            Some(&replay),
                            now_ms,
                        )
                        .await?;
                    report.seal.sealed_ops += 1;
                    report.seal.last_seq = Some(seq);
                }
                Content::Genesis(_) => {
                    return Err(corrupt("a genesis follows the start of a chain"));
                }
            }
            let replacement = report
                .seal
                .last_seq
                .ok_or_else(|| corrupt("a replaced operation was not sealed again"))?;
            renamed.insert(op.seq, replacement);
        }
        let captured = sealer
            .seal_captures(self, &mut tx, ctx, &mut batch, writer, now_ms)
            .await?;
        report.seal.absorb(captured);
        report.seal.changed_tables = finish(
            self,
            &mut tx,
            ctx,
            &mut batch,
            &mut slot,
            now_ms,
            &mut report.seal,
        )
        .await?;
        failpoint!("reseal.before_commit");
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(report)
    }
}
