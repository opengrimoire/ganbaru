//! Which devices sealed a replicated row, as read from the operation log.
//!
//! Domain columns such as a Chat message's author or display label are writer claims: a linked
//! device can seal any value there. The creating device comes from the signed log instead.

use crate::error::{SyncError, SyncResult};
use crate::log;
use crate::manifest::{Manifest, TableSpec};
use crate::merge::{group_id, keeps_immutable, table_param};
use ganbaru_sync_contracts::{TableId, WriterId};
use sqlx::{Row, SqliteConnection};
use std::collections::{BTreeSet, HashMap};

/// Returns the devices whose writers created `row_key` of `table` in `manifest`, ordered by
/// device id.
///
/// A Create that left the row's Immutable groups as they were, because the row already held a
/// version it saw or one of its own writer's, still counts as a creation for exclusion, but its
/// device is named only while its writer holds a version of one of those groups. An exclusion
/// that lets the kept value in therefore names it. Empty when the row was never captured or sync
/// state was reset.
pub async fn row_creating_devices(
    connection: &mut SqliteConnection,
    manifest: &Manifest,
    table: TableId,
    row_key: &str,
) -> SyncResult<Vec<String>> {
    let spec = manifest
        .table(table)
        .ok_or(SyncError::InvalidRequest("provenance of an unknown table"))?;
    let creators = sqlx::query(
        "SELECT DISTINCT w.device_id, c.writer_id
         FROM sync_row_creations c JOIN sync_writers w ON w.writer_id = c.writer_id
         WHERE c.table_id = ? AND c.row_key = ?",
    )
    .bind(table_param(table))
    .bind(row_key)
    .fetch_all(&mut *connection)
    .await?;
    let kept = if spec.groups.iter().any(|group| keeps_immutable(spec, group)) {
        Some(immutable_writers(connection, spec, row_key).await?)
    } else {
        None
    };
    let mut devices = BTreeSet::new();
    for row in &creators {
        let writer = log::writer_id(&row.try_get::<Vec<u8>, _>(1)?)?;
        if kept.as_ref().is_none_or(|kept| kept.contains(&writer)) {
            devices.insert(row.try_get::<String, _>(0)?);
        }
    }
    Ok(devices.into_iter().collect())
}

/// Writers holding a version of an Immutable group of `row_key` that a Create leaves as it is.
async fn immutable_writers(
    connection: &mut SqliteConnection,
    spec: &TableSpec,
    row_key: &str,
) -> SyncResult<BTreeSet<WriterId>> {
    let rows = sqlx::query(
        "SELECT DISTINCT group_id, writer_id FROM sync_register_versions
         WHERE table_id = ? AND row_key = ?",
    )
    .bind(table_param(spec.id))
    .bind(row_key)
    .fetch_all(&mut *connection)
    .await?;
    let mut writers = BTreeSet::new();
    for row in &rows {
        let group = group_id(row.try_get(0)?)?;
        if spec
            .group(group)
            .is_some_and(|group| keeps_immutable(spec, group))
        {
            writers.insert(log::writer_id(&row.try_get::<Vec<u8>, _>(1)?)?);
        }
    }
    Ok(writers)
}

/// Returns the label of every device the authorization log enrolled, keyed by device id.
///
/// A removed device keeps its label, so a row it created can still name it.
pub async fn device_labels(
    connection: &mut SqliteConnection,
) -> SyncResult<HashMap<String, String>> {
    let rows = sqlx::query("SELECT device_id, label FROM sync_devices WHERE label IS NOT NULL")
        .fetch_all(&mut *connection)
        .await?;
    rows.iter()
        .map(|row| Ok((row.try_get("device_id")?, row.try_get("label")?)))
        .collect()
}
