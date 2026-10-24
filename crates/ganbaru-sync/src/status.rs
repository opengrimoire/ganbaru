//! Engine state for status reporting, and re-evaluation of held operations after an update.

use std::collections::BTreeMap;

use ganbaru_sync_contracts::hlc::{self, CLOCK_WARNING_AHEAD_MS, Hlc};
use ganbaru_sync_contracts::{Envelope, Seq, TableId, WriterId};
use sqlx::{Connection, Row, SqliteConnection};

use crate::error::{SyncResult, corrupt, failpoint};
use crate::log::{self, WriterState, clamp_ms, seq_i64, stored_u64};
use crate::store::{self, Classified, HoldReason};
use crate::{ClockWarning, Engine, OPEN_CONFLICT_MASK_SQL, SpaceContext};

/// One writer of the space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterSummary {
    /// Writer id.
    pub writer: WriterId,
    /// Device the writer belongs to.
    pub device_id: String,
    /// Lifecycle state.
    pub state: WriterState,
    /// Highest stored sequence.
    pub stored: Seq,
    /// Highest applied sequence.
    pub applied: Seq,
    /// Last operation that stays, when revoked, forked, or re-sealed.
    pub cutoff: Option<Seq>,
}

/// Held operations by reason.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeldCounts {
    /// Held because this version cannot read their format.
    pub newer_format: usize,
    /// Held because they use a newer manifest.
    pub newer_manifest: usize,
    /// Held because they break the operation or manifest rules, or follow a revocation cutoff.
    pub invalid: usize,
}

impl HeldCounts {
    /// Every held operation.
    pub const fn total(&self) -> usize {
        self.newer_format + self.newer_manifest + self.invalid
    }

    fn add(&mut self, reason: HoldReason, count: usize) {
        match reason {
            HoldReason::NewerFormat => self.newer_format += count,
            HoldReason::NewerManifest => self.newer_manifest += count,
            HoldReason::Invalid => self.invalid += count,
        }
    }
}

/// A held operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeldOp {
    /// Writer of the operation.
    pub writer: WriterId,
    /// Sequence of the operation.
    pub seq: Seq,
    /// Why it is held.
    pub reason: HoldReason,
    /// When it was stored, in Unix milliseconds.
    pub stored_at_ms: u64,
}

/// Counts that summarize the engine state of a space.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncStatus {
    /// Writers by id.
    pub writers: Vec<WriterSummary>,
    /// Stored operations waiting to be applied.
    pub waiting: usize,
    /// Envelope bytes of the waiting operations.
    pub waiting_bytes: u64,
    /// Held operations by reason, without the excluded ones.
    pub held: HeldCounts,
    /// Operations of removed devices past their removal cutoff, stored and forwarded but never
    /// applied while the removal stays selected.
    pub excluded: usize,
    /// Local rows with changes that are not sealed yet.
    pub pending_captures: usize,
    /// Live rows with surfaced conflicts, per table name, for tables that have any.
    pub conflicts: Vec<(&'static str, usize)>,
    /// Recovery offers per table name of their root row, for tables that have any. Each
    /// offer counts once however many deleted descendants it brings back.
    pub recovery: Vec<(&'static str, usize)>,
}

fn count(value: i64) -> SyncResult<usize> {
    usize::try_from(value).map_err(|_| corrupt("negative count"))
}

impl Engine {
    /// Summarizes writers, pending operations, captures, conflicts, and recovery offers. Recovery
    /// offers are counted as `recovery_entries` lists them, so the counts match the list. Every
    /// read runs in one transaction, so the counts describe one database state even while a
    /// pass applies or seals on another connection.
    pub async fn status(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<SyncStatus> {
        let mut tx = conn.begin().await?;
        let status = self.read_status(&mut tx, ctx).await?;
        tx.commit().await?;
        Ok(status)
    }

    async fn read_status(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<SyncStatus> {
        log::ensure_space(conn, ctx.space).await?;
        let writers = log::writers(conn, ctx.space)
            .await?
            .into_iter()
            .map(|writer| WriterSummary {
                writer: writer.id,
                device_id: writer.device_id,
                state: writer.state,
                stored: writer.stored,
                applied: writer.applied,
                cutoff: writer.cutoff,
            })
            .collect();
        let mut status = SyncStatus {
            writers,
            ..SyncStatus::default()
        };
        let pending = sqlx::query(
            "SELECT CASE WHEN excluded_revision IS NULL THEN state ELSE 'excluded' END,
                hold_reason, count(*), coalesce(sum(length(envelope)), 0)
             FROM sync_ops WHERE space_id = ? AND state <> 'applied'
             GROUP BY 1, hold_reason",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .fetch_all(&mut *conn)
        .await?;
        for row in pending {
            let state: String = row.try_get(0)?;
            let reason: Option<String> = row.try_get(1)?;
            let ops = count(row.try_get(2)?)?;
            match (state.as_str(), reason) {
                ("waiting", None) => {
                    status.waiting = ops;
                    status.waiting_bytes = stored_u64(row.try_get(3)?, "waiting bytes")?;
                }
                ("held", Some(reason)) => status.held.add(HoldReason::parse(&reason)?, ops),
                ("excluded", _) => status.excluded += ops,
                _ => return Err(corrupt("pending operation with an unknown state")),
            }
        }
        status.pending_captures = count(
            sqlx::query_scalar("SELECT count(*) FROM sync_capture")
                .fetch_one(&mut *conn)
                .await?,
        )?;
        let sql = format!(
            "SELECT r.table_id, count(*) FROM sync_rows r WHERE {OPEN_CONFLICT_MASK_SQL} <> 0
             GROUP BY r.table_id ORDER BY r.table_id"
        );
        let conflicts: Vec<(i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(sql.as_str()))
            .fetch_all(&mut *conn)
            .await?;
        for (table_id, rows) in conflicts {
            let table = u16::try_from(table_id)
                .ok()
                .and_then(|id| self.manifest().table(TableId(id)))
                .ok_or_else(|| corrupt("conflict on a row of an unknown table"))?;
            status.conflicts.push((table.name, count(rows)?));
        }
        let mut offers: BTreeMap<TableId, usize> = BTreeMap::new();
        for entry in self.recovery_entries(conn, ctx).await? {
            *offers.entry(entry.table).or_default() += 1;
        }
        for (table, offers) in offers {
            let table = self
                .manifest()
                .table(table)
                .ok_or_else(|| corrupt("recovery offer on a row of an unknown table"))?;
            status.recovery.push((table.name, offers));
        }
        Ok(status)
    }

    /// Writers with merged versions whose clocks are far ahead of `now_ms`, each with its
    /// greatest such clock, in writer order. Those versions win concurrent edits until local time
    /// catches up, so the interface names their devices.
    pub async fn far_ahead_writers(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        now_ms: u64,
    ) -> SyncResult<Vec<ClockWarning>> {
        log::ensure_space(conn, ctx.space).await?;
        let threshold = Hlc::new(now_ms.saturating_add(CLOCK_WARNING_AHEAD_MS), 0);
        let rows = sqlx::query(
            "SELECT writer_id, max(clock) FROM sync_register_versions WHERE clock >= ?
             GROUP BY writer_id ORDER BY writer_id",
        )
        .bind(clamp_ms(threshold.as_u64()))
        .fetch_all(&mut *conn)
        .await?;
        let mut warnings = Vec::with_capacity(rows.len());
        for row in rows {
            let writer: Vec<u8> = row.try_get(0)?;
            let clock = Hlc::from_u64(stored_u64(row.try_get(1)?, "version clock")?);
            if hlc::is_far_ahead(clock, now_ms) {
                warnings.push(ClockWarning {
                    writer: log::writer_id(&writer)?,
                    clock,
                });
            }
        }
        Ok(warnings)
    }

    /// Held operations in writer and sequence order, at most `limit`, without the operations
    /// a removal excludes.
    pub async fn held_ops(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        limit: u32,
    ) -> SyncResult<Vec<HeldOp>> {
        log::ensure_space(conn, ctx.space).await?;
        let rows = sqlx::query(
            "SELECT writer_id, seq, hold_reason, stored_at_ms FROM sync_ops
             WHERE space_id = ? AND state <> 'applied' AND state = 'held'
                AND excluded_revision IS NULL
             ORDER BY writer_id, seq LIMIT ?",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(i64::from(limit))
        .fetch_all(&mut *conn)
        .await?;
        rows.iter()
            .map(|row| {
                let writer: Vec<u8> = row.try_get(0)?;
                let reason: Option<String> = row.try_get(2)?;
                Ok(HeldOp {
                    writer: log::writer_id(&writer)?,
                    seq: stored_u64(row.try_get(1)?, "sequence")?,
                    reason: HoldReason::parse(
                        reason
                            .as_deref()
                            .ok_or_else(|| corrupt("held operation without a reason"))?,
                    )?,
                    stored_at_ms: stored_u64(row.try_get(3)?, "stored time")?,
                })
            })
            .collect()
    }

    /// Classifies operations held for a newer format or manifest again, so an updated app
    /// applies what it can now read. Returns how many became waiting.
    pub async fn reevaluate_held(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<usize> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        let rows = sqlx::query(
            "SELECT writer_id, seq, hold_reason, envelope FROM sync_ops
             WHERE space_id = ? AND state <> 'applied' AND state = 'held'
                AND hold_reason IN ('newer_format', 'newer_manifest')
                AND excluded_revision IS NULL
             ORDER BY writer_id, seq",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .fetch_all(&mut *tx)
        .await?;
        let mut released = 0;
        for row in rows {
            let writer: Vec<u8> = row.try_get(0)?;
            let writer = log::writer_id(&writer)?;
            let seq = stored_u64(row.try_get(1)?, "sequence")?;
            let previous: Option<String> = row.try_get(2)?;
            let envelope: Vec<u8> = row.try_get(3)?;
            let classified = match Envelope::decode(&envelope) {
                Ok(envelope) => store::classify(self, &envelope),
                Err(_) => Classified::Malformed,
            };
            let reason = match &classified {
                Classified::Waiting {
                    dependencies,
                    revision,
                    continues,
                    ..
                } => {
                    let context =
                        store::derive_context(&mut tx, ctx, writer, seq, Some(dependencies))
                            .await?;
                    sqlx::query(
                        "UPDATE sync_ops SET state = 'waiting', hold_reason = NULL, context = ?,
                            authorization_revision = ?, group_continues = ?
                         WHERE space_id = ? AND writer_id = ? AND seq = ?",
                    )
                    .bind(log::encode_context(context.as_ref()))
                    .bind(seq_i64(*revision)?)
                    .bind(*continues)
                    .bind(ctx.space.as_bytes().as_slice())
                    .bind(writer.as_bytes().as_slice())
                    .bind(seq_i64(seq)?)
                    .execute(&mut *tx)
                    .await?;
                    released += 1;
                    continue;
                }
                Classified::Held { reason, .. } => *reason,
                Classified::Malformed => HoldReason::Invalid,
            };
            if previous.as_deref() != Some(reason.as_str()) {
                sqlx::query(
                    "UPDATE sync_ops SET hold_reason = ?
                     WHERE space_id = ? AND writer_id = ? AND seq = ?",
                )
                .bind(reason.as_str())
                .bind(ctx.space.as_bytes().as_slice())
                .bind(writer.as_bytes().as_slice())
                .bind(seq_i64(seq)?)
                .execute(&mut *tx)
                .await?;
            }
        }
        failpoint!("reevaluate.before_commit");
        tx.commit().await?;
        Ok(released)
    }
}
