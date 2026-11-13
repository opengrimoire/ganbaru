//! Daily teammate usage summaries: the flush loop of the executing device and the read every
//! device uses.
//!
//! The loop adds this device's run usage to its summaries every
//! [`USAGE_SUMMARY_FLUSH_SECONDS`], and right away when a run leaves the working state, so a
//! settled run reaches other devices without waiting for the next tick.

use super::channels::chat_pool;
use ganbaru_chat::usage_summaries::{ChatTeammateUsageDayRead, read_teammate_usage};
#[cfg(desktop)]
use ganbaru_chat::usage_summaries::{
    FlushScope, USAGE_SUMMARY_FLUSH_SECONDS, flush_teammate_usage,
};
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatParticipantId, ChatResult};
use jiff::tz::TimeZone;
use sqlx::SqlitePool;

/// Wakes the flush loop after a committed run state change.
#[cfg(desktop)]
#[derive(Default)]
pub(crate) struct UsageSummaryWake(tokio::sync::Notify);

#[cfg(desktop)]
impl UsageSummaryWake {
    /// Asks the loop to add the usage of runs that stopped working.
    pub(crate) fn wake(&self) {
        self.0.notify_one();
    }
}

fn local_time_error(_error: String) -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "This device's local time could not be determined",
        true,
    )
}

fn now_millis() -> i64 {
    crate::vault::now_utc().timestamp_millis()
}

/// The `YYYY-MM-DD` date in `zone` and the UTC timestamp of the instant `epoch_ms`.
fn local_date_and_timestamp(epoch_ms: i64, zone: &TimeZone) -> ChatResult<(String, String)> {
    let local_date =
        ganbaru_civil_time::local_date_key(epoch_ms, zone).map_err(local_time_error)?;
    let timestamp = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(epoch_ms)
        .ok_or_else(|| local_time_error(String::new()))?
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    Ok((local_date, timestamp))
}

#[cfg(desktop)]
/// Adds this device's run usage at `epoch_ms` to its summaries for the local date in `zone`.
pub(crate) async fn flush_at(
    pool: &SqlitePool,
    device_id: &str,
    epoch_ms: i64,
    zone: &TimeZone,
    scope: FlushScope,
) -> ChatResult<usize> {
    let (local_date, now) = local_date_and_timestamp(epoch_ms, zone)?;
    flush_teammate_usage(pool, device_id, &local_date, &now, scope).await
}

/// Reads a teammate's usage per local day, with the retention window ending today in `zone`.
pub(crate) async fn read_usage_at(
    pool: &SqlitePool,
    teammate_id: &ChatParticipantId,
    from_date: &str,
    to_date: &str,
    epoch_ms: i64,
    zone: &TimeZone,
) -> ChatResult<Vec<ChatTeammateUsageDayRead>> {
    let (today, _) = local_date_and_timestamp(epoch_ms, zone)?;
    read_teammate_usage(pool, teammate_id.as_str(), from_date, to_date, &today).await
}

#[cfg(desktop)]
/// Runs one flush for the active vault, logging failures by code only.
async fn flush_active_vault(app: &tauri::AppHandle, scope: FlushScope) {
    if crate::vault::active_vault_path(app).is_err() {
        return;
    }
    let device_id = match crate::vault::ensure_device_id(app) {
        Ok(device_id) => device_id,
        Err(error) => {
            eprintln!("Chat usage summaries could not read the device id: {error}");
            return;
        }
    };
    let db_url = format!("sqlite:{}", crate::vault::APP_SQLITE_FILE);
    let pool = match crate::db::connect_sqlite(app.clone(), db_url).await {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("Chat usage summaries could not open the database: {error}");
            return;
        }
    };
    let flushed = match ganbaru_civil_time::system_zone() {
        Ok(zone) => flush_at(&pool, &device_id, now_millis(), &zone, scope).await,
        Err(error) => Err(local_time_error(error)),
    };
    if let Err(error) = flushed {
        eprintln!("Chat usage summary flush failed with code {:?}", error.code);
    }
}

#[cfg(desktop)]
/// Starts the flush loop. The first flush runs at startup; a wake flushes only runs that are
/// not working, and every tick flushes all of them.
pub(crate) fn start(app: &tauri::AppHandle) {
    use tauri::Manager;
    app.manage(UsageSummaryWake::default());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(USAGE_SUMMARY_FLUSH_SECONDS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let wake = app.state::<UsageSummaryWake>();
        loop {
            let scope = tokio::select! {
                _ = interval.tick() => FlushScope::All,
                () = wake.0.notified() => FlushScope::NotWorking,
            };
            flush_active_vault(&app, scope).await;
        }
    });
}

#[cfg(desktop)]
/// Wakes the flush loop when it runs; does nothing on a device without it.
pub(crate) fn wake<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    if let Some(wake) = app.try_state::<UsageSummaryWake>() {
        wake.wake();
    }
}

/// Reads a teammate's usage per local day from `from_date` to `to_date` (`YYYY-MM-DD`, both
/// included), summed across the devices that ran it, within the last
/// [`ganbaru_chat::usage_summaries::USAGE_DAILY_RETENTION_DAYS`] local days.
#[tauri::command]
pub async fn chat_read_teammate_usage(
    app: tauri::AppHandle,
    db_url: String,
    teammate_id: ChatParticipantId,
    from_date: String,
    to_date: String,
) -> ChatResult<Vec<ChatTeammateUsageDayRead>> {
    let pool = chat_pool(app, db_url).await?;
    let zone = ganbaru_civil_time::system_zone().map_err(local_time_error)?;
    read_usage_at(
        &pool,
        &teammate_id,
        &from_date,
        &to_date,
        now_millis(),
        &zone,
    )
    .await
}

#[cfg(all(test, desktop))]
mod tests;
