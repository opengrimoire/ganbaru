//! Flushes this device's daily usage totals into the vault on one shared cadence.
//!
//! Every flush path of the process (the usage read, exit, backgrounding, and vault snapshots)
//! shares one planner, so a bucket flushes at most once per interval unless a caller forces it.

use ganbaru_distractions::usage::totals::{FlushPlanner, flush_own_totals};
use ganbaru_distractions::usage::{UsageDateWindow, usage_date_window};
use sqlx::SqlitePool;
use std::sync::LazyLock;
use std::time::Instant;

static PLANNER: LazyLock<tokio::sync::Mutex<FlushPlanner>> =
    LazyLock::new(|| tokio::sync::Mutex::new(FlushPlanner::default()));

/// The usage retention window around today's UTC date.
pub(crate) fn current_usage_date_window() -> UsageDateWindow {
    usage_date_window(crate::vault::now_utc().date_naive())
}

/// Flushes the due totals of `device_id` in `vault_id`; `force` flushes every stale bucket.
/// Returns how many totals were written.
pub(crate) async fn flush(
    pool: &SqlitePool,
    vault_id: &str,
    device_id: &str,
    force: bool,
) -> Result<usize, String> {
    let mut planner = PLANNER.lock().await;
    flush_own_totals(
        pool,
        &mut planner,
        vault_id,
        device_id,
        &current_usage_date_window(),
        Instant::now(),
        force,
    )
    .await
}

/// Moves this device's spooled desktop samples into the journal and its spooled browser block
/// events into the vault, then flushes its totals. The totals flush even when a spool step
/// fails, so samples the journal already holds keep reaching the vault; the first error is
/// returned afterwards.
#[cfg(desktop)]
pub(crate) async fn drain_and_flush<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &SqlitePool,
    vault_id: &str,
    device_id: &str,
    force: bool,
) -> Result<usize, String> {
    let drained = crate::distractions::spool::drain(
        app,
        pool,
        vault_id,
        device_id,
        &current_usage_date_window(),
    )
    .await;
    let ingested =
        crate::distractions::spool::ingest_block_events(app, pool, vault_id, device_id).await;
    let flushed = flush(pool, vault_id, device_id, force).await;
    drained?;
    ingested?;
    flushed
}

/// Forces the active vault's spooled samples, spooled block events, and stale totals out before
/// exit or a vault snapshot.
#[cfg(desktop)]
pub(crate) async fn flush_active_vault<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    use tauri::Manager;
    if crate::vault::available_active_vault_path(app)?.is_none() {
        return Ok(());
    }
    let vault_id = crate::vault::active_vault_id(app)?;
    let device_id = crate::vault::ensure_device_id(app)?;
    let _permit = app
        .state::<crate::vault::write_fence::VaultWriteFence>()
        .acquire_managed_write()?;
    if crate::vault::active_vault_id(app)? != vault_id {
        return Err("usage totals vault changed before the flush".into());
    }
    let pool = crate::db::connect_vault(app).await?;
    drain_and_flush(app, &pool, &vault_id, &device_id, true)
        .await
        .map(|_| ())
}
