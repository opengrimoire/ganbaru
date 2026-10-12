//! Recovery offers of the active vault. Listing reads the engine directly; restoring and
//! discarding seal with the local writer, so the service runs them inside a pass.

use ganbaru_sync::{DeviceRef, Engine, local};
pub(crate) use ganbaru_sync_replica::recovery::{
    RecoveryAction, RecoveryChoice, close, recoverable_table,
};
use serde::Serialize;
use tauri::{AppHandle, Runtime};
use tokio::sync::oneshot;

/// A deleted row with edits the deletion did not see.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecoveryEntryView {
    /// Domain table, by name.
    pub table: String,
    pub row_key: String,
    pub title: String,
    pub preview: String,
    /// Device of the newest unseen edit.
    pub device: DeviceRef,
    /// When that edit was made, in Unix milliseconds.
    pub edited_at_ms: u64,
}

/// A recovery action waiting for the service, which owns the local writer.
pub(crate) struct RecoveryRequest {
    pub choice: RecoveryChoice,
    /// Receives the id of the restored row, if any.
    pub reply: oneshot::Sender<Result<Option<String>, String>>,
}

/// Recovery offers of the active vault, newest edit first.
pub(crate) async fn list<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<RecoveryEntryView>, String> {
    let Some(pool) = crate::db::connect_active_vault_for_sync(app).await? else {
        return Ok(Vec::new());
    };
    let vault_id = crate::vault::active_vault_id(app)?;
    let engine = Engine::vault();
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}"))?;
    let Some(ctx) = local::space_context(&mut conn, &vault_id)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
    else {
        return Ok(Vec::new());
    };
    let entries = engine
        .recovery_entries(&mut conn, &ctx)
        .await
        .map_err(|error| format!("read recovery offers: {error}"))?;
    drop(conn);
    let devices = super::devices::read(app)?;
    let mut views: Vec<RecoveryEntryView> = entries
        .into_iter()
        .filter_map(|entry| {
            let table = engine.manifest().table(entry.table)?;
            Some(RecoveryEntryView {
                table: table.name.to_string(),
                row_key: entry.row_key,
                title: entry.presentation.title,
                preview: entry.presentation.preview,
                device: devices.describe(&entry.device_id),
                edited_at_ms: entry.clock.physical_ms(),
            })
        })
        .collect();
    views.sort_by(|left, right| {
        right
            .edited_at_ms
            .cmp(&left.edited_at_ms)
            .then_with(|| left.row_key.cmp(&right.row_key))
    });
    Ok(views)
}
