//! Surfaced conflicts: live rows whose surfaced groups keep concurrent versions with different
//! values. The merge already picked a winner; the user may keep it or choose another version,
//! and either choice seals a write that supersedes every version it saw.

use std::collections::HashMap;

use ganbaru_sync_contracts::{GroupId, GroupMask, Hlc, TableId, Value, WriterId};
use sqlx::{Connection, SqliteConnection};

use crate::error::{SyncResult, corrupt};
use crate::merge::{self, RowState, table_param};
use crate::seal::force_capture;
use crate::{Engine, SpaceContext, SyncError, log, sql};

/// SQL expression for the open conflict mask of the `sync_rows` row aliased `r`: its groups in
/// conflict, less the groups a local resolution forced for the next seal. A resolution waiting
/// for its seal is already settled for the user.
pub const OPEN_CONFLICT_MASK_SQL: &str = "(r.conflict_mask & ~COALESCE((SELECT c.forced_mask \
     FROM sync_capture c WHERE c.table_id = r.table_id AND c.row_key = r.row_key), 0))";

/// A live row with surfaced conflicts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictRow {
    /// Table of the row.
    pub table: TableId,
    /// Row key.
    pub row_key: String,
    /// Groups in conflict.
    pub groups: GroupMask,
}

/// One concurrent version of a conflicted group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictVersion {
    /// Writer that sealed it.
    pub writer: WriterId,
    /// Device the writer belongs to.
    pub device_id: String,
    /// Clock of its operation.
    pub clock: Hlc,
    /// Group value.
    pub value: Value,
    /// Whether this version is the materialized winner.
    pub displayed: bool,
}

/// The concurrent versions of one conflicted group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictDetail {
    /// Group in conflict.
    pub group: GroupId,
    /// Versions, newest clock first.
    pub versions: Vec<ConflictVersion>,
}

impl Engine {
    /// Live rows of a table with surfaced conflicts, by row key.
    pub async fn conflicts(
        &self,
        conn: &mut SqliteConnection,
        table: TableId,
    ) -> SyncResult<Vec<ConflictRow>> {
        let sql = format!(
            "SELECT row_key, {OPEN_CONFLICT_MASK_SQL} AS open_mask FROM sync_rows r
             WHERE r.table_id = ? AND open_mask <> 0 ORDER BY row_key"
        );
        let rows: Vec<(String, i64)> = sqlx::query_as(&sql)
            .bind(table_param(table))
            .fetch_all(&mut *conn)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(row_key, mask)| ConflictRow {
                table,
                row_key,
                // The mask is stored as the signed form of its bits.
                groups: GroupMask(mask as u64),
            })
            .collect())
    }

    /// Concurrent versions of every conflicted group of a row; empty when it has none.
    pub async fn conflict_details(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        table: TableId,
        key: &str,
    ) -> SyncResult<Vec<ConflictDetail>> {
        let Some(spec) = self.manifest().table(table) else {
            return Err(SyncError::InvalidRequest("unknown table"));
        };
        let sql = format!(
            "SELECT {OPEN_CONFLICT_MASK_SQL} FROM sync_rows r WHERE r.table_id = ? AND r.row_key = ?"
        );
        let mask: Option<i64> = sqlx::query_scalar(&sql)
            .bind(table_param(table))
            .bind(key)
            .fetch_optional(&mut *conn)
            .await?;
        let mask = GroupMask(mask.unwrap_or(0) as u64);
        if mask == GroupMask::EMPTY {
            return Ok(Vec::new());
        }
        let devices: HashMap<WriterId, String> = log::writers(conn, ctx.space)
            .await?
            .into_iter()
            .map(|writer| (writer.id, writer.device_id))
            .collect();
        let versions = merge::versions(conn, table, key).await?;
        let winners = merge::winners(self, spec, &versions);
        let mut details = Vec::new();
        for group in spec.groups.iter().filter(|group| mask.contains(group.id)) {
            let winner = winners.get(&group.id).map(|version| version.writer);
            let mut group_versions: Vec<ConflictVersion> = versions
                .iter()
                .filter(|version| version.group == group.id)
                .map(|version| {
                    Ok(ConflictVersion {
                        writer: version.writer,
                        device_id: devices
                            .get(&version.writer)
                            .cloned()
                            .ok_or_else(|| corrupt("version of an unknown writer"))?,
                        clock: version.clock,
                        value: version.value.clone(),
                        displayed: winner == Some(version.writer),
                    })
                })
                .collect::<SyncResult<_>>()?;
            group_versions
                .sort_by_key(|version| std::cmp::Reverse((version.clock, version.writer)));
            details.push(ConflictDetail {
                group: group.id,
                versions: group_versions,
            });
        }
        Ok(details)
    }

    /// Marks a group of a live row to be sealed even when its value did not change, so the
    /// next seal resolves its conflict. The caller writes the chosen value to the domain row
    /// first, in the same transaction.
    pub async fn force_group(
        &self,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
        group: GroupId,
        now_ms: u64,
    ) -> SyncResult<()> {
        let Some(spec) = self.manifest().table(table) else {
            return Err(SyncError::InvalidRequest("unknown table"));
        };
        let Some(group_spec) = spec.group(group) else {
            return Err(SyncError::InvalidRequest("unknown group"));
        };
        if !group_spec.surfaced {
            return Err(SyncError::InvalidRequest("group is not surfaced"));
        }
        let mut tx = conn.begin().await?;
        if merge::row_state(&mut tx, table, key).await? != Some(RowState::Live)
            || sql::read_columns(&mut tx, spec.name, spec.key_column, key, &[])
                .await?
                .is_none()
        {
            return Err(SyncError::InvalidRequest("row is not live"));
        }
        force_capture(&mut tx, table, key, group_spec.mask(), now_ms).await?;
        tx.commit().await?;
        Ok(())
    }
}
