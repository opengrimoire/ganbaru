//! Recovery of edits that outlived a deletion.
//!
//! A tombstoned row is offered for recovery while it retains a version of a recoverable group
//! that none of its tombstones saw: an edit made concurrently with the deletion. Restoring
//! creates a new row from the retained values; restoring or discarding then seals a tombstone
//! that covers every retained version, which closes the offer on every replica.

use std::collections::HashMap;

use ganbaru_sync_contracts::{
    Change, ChangeAction, GroupId, Hlc, RowKey, TableId, Value, WriterId,
};
use sqlx::{Connection, SqliteConnection};

use crate::adapter::Presentation;
use crate::apply::finish;
use crate::error::{SyncResult, corrupt, failpoint};
use crate::manifest::TableSpec;
use crate::merge::{self, Batch, RowState, Version, table_param};
use crate::seal::{SealReport, SealSlot};
use crate::{Engine, LocalWriter, SpaceContext, SyncError, log, sql};

/// A deleted row with edits the deletion did not see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryEntry {
    /// Table of the row.
    pub table: TableId,
    /// Row key.
    pub row_key: String,
    /// Title and preview from the retained values.
    pub presentation: Presentation,
    /// Writer of the newest unseen edit.
    pub writer: WriterId,
    /// Device of that writer.
    pub device_id: String,
    /// Clock of that edit.
    pub clock: Hlc,
}

/// Winning retained value per group.
fn retained_values(
    engine: &Engine,
    table: &TableSpec,
    versions: &[Version],
) -> Vec<(GroupId, Value)> {
    merge::winners(engine, table, versions)
        .into_iter()
        .map(|(group, version)| (group, version.value.clone()))
        .collect()
}

/// Whether a row is tombstoned and offered for recovery.
async fn recoverable(conn: &mut SqliteConnection, table: TableId, key: &str) -> SyncResult<bool> {
    let flag: Option<i64> = sqlx::query_scalar(
        "SELECT recovery FROM sync_rows WHERE table_id = ? AND row_key = ? AND state = 'tombstoned'",
    )
    .bind(table_param(table))
    .bind(key)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(flag == Some(1))
}

impl Engine {
    /// Rows offered for recovery, by table and key.
    pub async fn recovery_entries(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<Vec<RecoveryEntry>> {
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT table_id, row_key FROM sync_rows WHERE recovery = 1 ORDER BY table_id, row_key",
        )
        .fetch_all(&mut *conn)
        .await?;
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let devices: HashMap<WriterId, String> = log::writers(conn, ctx.space)
            .await?
            .into_iter()
            .map(|writer| (writer.id, writer.device_id))
            .collect();
        let mut batch = Batch::load(conn, ctx.space).await?;
        let mut entries = Vec::with_capacity(rows.len());
        for (table_id, key) in rows {
            let table = u16::try_from(table_id)
                .ok()
                .and_then(|id| self.manifest().table(TableId(id)))
                .ok_or_else(|| corrupt("recovery row of an unknown table"))?;
            let versions = merge::versions(conn, table.id, &key).await?;
            let covering = merge::tombstone_contexts(conn, &mut batch, table.id, &key).await?;
            let Some(newest) = versions
                .iter()
                .filter(|version| {
                    table
                        .group(version.group)
                        .is_some_and(|group| group.recoverable)
                        && merge::uncovered(version, &covering)
                })
                .max_by_key(|version| (version.clock, version.writer))
            else {
                return Err(corrupt("a recovery row has no unseen edit"));
            };
            let values = retained_values(self, table, &versions);
            let presentation = self
                .adapter(table)
                .map(|adapter| adapter.presentation(table.id, &values))
                .unwrap_or_default();
            entries.push(RecoveryEntry {
                table: table.id,
                row_key: key,
                presentation,
                writer: newest.writer,
                device_id: devices
                    .get(&newest.writer)
                    .cloned()
                    .ok_or_else(|| corrupt("version of an unknown writer"))?,
                clock: newest.clock,
            });
        }
        Ok(entries)
    }

    /// Winning retained value of every group of a row offered for recovery, or `None` when the
    /// row is not offered.
    pub async fn recovery_values(
        &self,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<Option<Vec<(GroupId, Value)>>> {
        let Some(spec) = self.manifest().table(table) else {
            return Err(SyncError::InvalidRequest("unknown table"));
        };
        if !recoverable(conn, table, key).await? {
            return Ok(None);
        }
        let versions = merge::versions(conn, table, key).await?;
        Ok(Some(retained_values(self, spec, &versions)))
    }

    /// Closes the recovery offer of a row after the caller restored it as a new row or chose
    /// to discard it: seals pending captures, then a tombstone that covers every retained
    /// version.
    pub async fn close_recovery(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        table: TableId,
        key: &str,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<SealReport> {
        let Some(spec) = self.manifest().table(table) else {
            return Err(SyncError::InvalidRequest("unknown table"));
        };
        let row = RowKey::new(key).map_err(|_| SyncError::InvalidRequest("row key"))?;
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        if merge::row_state(&mut tx, table, key).await? != Some(RowState::Tombstoned)
            || !recoverable(&mut tx, table, key).await?
        {
            return Err(SyncError::InvalidRequest("row is not offered for recovery"));
        }
        sql::begin_engine_write(&mut tx).await?;
        let mut batch = Batch::load(&mut tx, ctx.space).await?;
        let mut slot = SealSlot::new(Some(writer));
        let (sealer, writer) = slot
            .open(self, &mut tx, ctx, &mut batch, now_ms)
            .await?
            .ok_or_else(|| corrupt("sealer slot has no writer"))?;
        let mut report = sealer
            .seal_captures(self, &mut tx, ctx, &mut batch, writer, now_ms)
            .await?;
        let tombstone = Change {
            table: spec.id,
            row,
            action: ChangeAction::Tombstone,
            groups: Vec::new(),
            replaced_by: None,
        };
        sealer
            .seal_changes(
                self,
                &mut tx,
                ctx,
                &mut batch,
                writer,
                vec![tombstone],
                None,
                now_ms,
                &mut report,
            )
            .await?;
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
        failpoint!("recovery.before_commit");
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(report)
    }
}
