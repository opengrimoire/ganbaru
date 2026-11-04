//! Background runner for due Chat cleanup jobs: checkpoint refs and managed
//! attachment and browser artifact files queued by permanent deletion.

use super::checkpoints::run_checkpoint_cleanup;
use crate::{db, vault};
use chrono::{SecondsFormat, Utc};
use ganbaru_chat::repository::attachments::run_due_attachment_cleanup;
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult, UtcTimestamp};
use sqlx::SqlitePool;
use std::time::Duration;

/// Time between background cleanup runs. Permanent deletion queues files with
/// a one-day grace period, so an hourly pass removes them soon after it ends.
const CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Runs every due Chat cleanup job once and returns how many completed.
/// Attachment files are removed only while this device may write the vault.
pub(crate) async fn run_due_chat_cleanup(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
) -> ChatResult<u64> {
    let now = now_timestamp()?;
    let checkpoint_refs = run_checkpoint_cleanup(app, pool, &now).await?;
    let attachment_files = match vault::active_writable_vault_path(app) {
        Ok(vault_root) => run_due_attachment_cleanup(pool, &vault_root, &now).await?,
        Err(_) => 0,
    };
    Ok(checkpoint_refs + attachment_files)
}

/// Starts the hourly cleanup loop for the active vault. The first pass runs at
/// startup; a pass without an active vault does nothing.
pub(crate) fn start(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(CLEANUP_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if vault::active_vault_path(&app).is_err() {
                continue;
            }
            let db_url = format!("sqlite:{}", vault::APP_SQLITE_FILE);
            let pool = match db::connect_sqlite(app.clone(), db_url).await {
                Ok(pool) => pool,
                Err(error) => {
                    eprintln!("Chat cleanup could not open the database: {error}");
                    continue;
                }
            };
            if let Err(error) = run_due_chat_cleanup(&app, &pool).await {
                eprintln!("Chat cleanup failed with code {:?}", error.code);
            }
        }
    });
}

fn now_timestamp() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "create Chat timestamp", false))
}
