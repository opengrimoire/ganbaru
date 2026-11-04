//! Calendar sync conflict commands: reading open event conflicts and resolving one.

use ganbaru_calendar::conflicts::{
    CalendarConflictError, CalendarConflictRead, CalendarConflictResolution,
};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

/// The open title, description, and timing conflicts of every event, each with the devices
/// that wrote the versions.
#[tauri::command]
pub async fn calendar_conflicts<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<CalendarConflictRead, CalendarConflictError> {
    let vault_id = crate::vault::active_vault_id(&app)?;
    let devices = crate::sync::devices::read(&app)?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::conflicts::conflicts_from_pool(&pool, &vault_id, &devices).await
}

/// Resolves an event conflict by keeping the displayed version or using another one, then
/// reloads Calendar views and the Focus Calendar projection as applied sync rows do.
#[tauri::command]
pub async fn calendar_resolve_conflict(
    app: AppHandle,
    db_url: String,
    resolution: CalendarConflictResolution,
) -> Result<(), CalendarConflictError> {
    let vault_id = crate::vault::active_vault_id(&app)?;
    let now_ms = crate::sync::now_ms()?;
    let pool = connect_sqlite(app.clone(), db_url).await?;
    ganbaru_calendar::conflicts::resolve_from_pool(&pool, &vault_id, resolution, now_ms).await?;
    super::sync::publish_change(&app);
    // An event moved to cover now should start its music without waiting for the Calendar poll.
    #[cfg(desktop)]
    if let Err(error) = crate::music::session::calendar_changed(&app) {
        eprintln!("Calendar conflict resolution: {error}");
    }
    Ok(())
}
