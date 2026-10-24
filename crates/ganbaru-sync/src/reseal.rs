//! Re-sealing a local writer's operations with a new writer.
//!
//! When a fork cuts off operations this replica sealed and applied, the installation replaces
//! its writer. Every operation of the old writer past the kept sequence is removed with its merge
//! effects and sealed again by the new writer with its original clock, manifest version, and
//! causal context, where removed operations of the old writer are renamed to their replacements.
//! Authorization entries are dropped instead. Operations of other writers stay as they are, so
//! the merge state after the re-seal is the one every replica computes from the kept chain and
//! the replacements.

use std::collections::BTreeMap;

use ganbaru_sync_contracts::{Content, Envelope, Operation, Seq, TableId, VersionVector, WriterId};
use sqlx::{Connection, Row, SqliteConnection};

use crate::apply::{Constrained, finish_sealed};
use crate::error::{SyncResult, corrupt, failpoint};
use crate::log::{self, seq_i64, stored_u64};
use crate::merge::{Batch, table_param};
use crate::seal::{Advance, Replay, SealReport, SealSlot};
use crate::{Engine, LocalWriter, SpaceContext, SyncError, authorization, exclusion, fork, sql};

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
            if envelope.is_empty() {
                return Err(SyncError::InvalidReseal(
                    "a replaced operation was compacted",
                ));
            }
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

/// A replaced operation's context with removed operations of the old writer renamed to the
/// replacement of the latest one it saw. Authorization operations are dropped rather than
/// replaced, so a context that saw only dropped ones past `keep_through` keeps the old writer at
/// `keep_through`. Every other forked writer stays at the last sequence its copies share, since
/// no replica applies either copy past it.
fn remap(
    context: &VersionVector,
    old: WriterId,
    keep_through: Seq,
    new: WriterId,
    renamed: &BTreeMap<Seq, Seq>,
    prefixes: &BTreeMap<WriterId, Seq>,
) -> SyncResult<VersionVector> {
    let mut remapped = VersionVector::new();
    for (writer, seq) in context.iter() {
        if writer == new {
            return Err(corrupt("a replaced operation saw its replacement writer"));
        }
        if writer != old {
            let seq = prefixes.get(&writer).map_or(seq, |keep| seq.min(*keep));
            remapped.advance(writer, seq);
            continue;
        }
        if seq <= keep_through {
            remapped.advance(writer, seq);
            continue;
        }
        remapped.advance(old, keep_through);
        if let Some((_, replacement)) = renamed.range(..=seq).next_back() {
            remapped.advance(new, *replacement);
        }
    }
    Ok(remapped)
}

/// Removes the versions, tombstones, and row creations of a writer's operations past
/// `keep_through`, revives rows left without tombstones, and marks every touched row dirty. A
/// re-seal replaces what it removes; a removal cutoff leaves rows its excluded operations
/// created without merge state, which late changes keep hidden.
pub(crate) async fn remove_effects(
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
    for table in [
        "sync_register_versions",
        "sync_tombstones",
        "sync_row_creations",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE writer_id = ? AND seq > ?"
        )))
        .bind(old.as_bytes().as_slice())
        .bind(keep)
        .execute(&mut *conn)
        .await?;
    }
    for (table_id, key) in touched {
        let table = u16::try_from(table_id)
            .map(TableId)
            .map_err(|_| corrupt("merge state of a table id out of range"))?;
        // A row whose every tombstone was removed is live again.
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

impl Engine {
    /// Replaces the old local writer's operations past `keep_through`, the last sequence another
    /// copy of the chain shares with this one, with operations of a new writer that merge the same
    /// way, then seals pending captures with it. The new writer must not have sealed anything but
    /// its genesis.
    ///
    /// `keep_through` comes from stored fork evidence, see [`Engine::own_forks`], or from a
    /// local comparison with another copy of the chain; with evidence stored, the kept sequence
    /// is at most the one it proves shared. Every replica that holds the evidence excludes both
    /// copies past it, so replaced operations may have left this replica and been seen by other
    /// writers: their operations stay, and only lose the effects of what they saw. Authorization
    /// entries past the kept sequence are dropped, not sealed again, since a fork of the anchor
    /// freezes its epoch and only a handover continues the log.
    ///
    /// Each installation that holds a copy of the forked chain re-seals its own copy this way;
    /// an installation destroyed before it does loses its copy, which no other replica keeps.
    pub async fn reseal_fork(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        old: WriterId,
        keep_through: Seq,
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
        let prefixes = fork::shared_prefixes(&mut tx, ctx.space).await?;
        let proven = prefixes.get(&old).copied();
        let keep_through = proven.map_or(keep_through, |proven| keep_through.min(proven));
        if (keep_through < 1 && proven != Some(0)) || keep_through > row.stored {
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
        let replaced = replaced_ops(&mut tx, ctx, old, keep_through).await?;
        let dropped_entries = replaced
            .iter()
            .any(|op| matches!(op.operation.content, Content::Authorization(_)));
        remove_effects(&mut tx, &mut batch, old, keep_through).await?;
        fork::drop_branch(&mut tx, ctx.space, old, keep_through, row.applied).await?;
        batch.applied.set(old, keep_through);
        batch.contexts.clear();
        if dropped_entries
            && let Some(selection) = authorization::rederive(&mut tx, ctx.space).await?
        {
            exclusion::settle(self, &mut tx, ctx, &mut batch, &selection).await?;
        }
        let replaced: Vec<Replaced> = replaced
            .into_iter()
            .filter(|op| !matches!(op.operation.content, Content::Authorization(_)))
            .collect();
        let mut report = ResealReport {
            replaced_ops: replaced.len(),
            seal: SealReport::default(),
        };
        let mut slot = SealSlot::new(Some(writer));
        let (sealer, writer) = slot
            .open(self, &mut tx, ctx, &mut batch, now_ms)
            .await?
            .ok_or_else(|| corrupt("sealer slot has no writer"))?;
        let excluding = exclusion::excluding(&mut tx, ctx.space).await?;
        let mut renamed = BTreeMap::new();
        let last = replaced.len();
        for (index, op) in replaced.into_iter().enumerate() {
            // The replacement chain closes every group by its last replayed operation, so later
            // operations of the new writer never join it.
            let replay = Replay {
                clock: op.operation.header.clock,
                manifest_version: op.operation.header.manifest_version,
                context: remap(&op.context, old, keep_through, new, &renamed, &prefixes)?,
                excluding: &excluding,
                continues: op.operation.header.group_continues && index + 1 < last,
            };
            match op.operation.content {
                Content::Changes(changes) if changes.is_empty() => {
                    sealer
                        .seal_advance(
                            self,
                            &mut tx,
                            ctx,
                            &mut batch,
                            writer,
                            Advance::Changes,
                            Some(&replay),
                            now_ms,
                            &mut report.seal,
                        )
                        .await?;
                }
                Content::Acknowledgement => {
                    sealer
                        .seal_advance(
                            self,
                            &mut tx,
                            ctx,
                            &mut batch,
                            writer,
                            Advance::Acknowledgement,
                            Some(&replay),
                            now_ms,
                            &mut report.seal,
                        )
                        .await?;
                }
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
                Content::Genesis(_) => {
                    return Err(corrupt("a genesis follows the start of a chain"));
                }
                Content::Authorization(_) => {
                    return Err(corrupt("an authorization operation is sealed again"));
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
        let materialized = finish_sealed(
            self,
            &mut tx,
            ctx,
            &mut batch,
            &mut slot,
            Constrained::Seal,
            now_ms,
            &mut report.seal,
        )
        .await?;
        report.seal.record(materialized);
        failpoint!("reseal.before_commit");
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(report)
    }
}
