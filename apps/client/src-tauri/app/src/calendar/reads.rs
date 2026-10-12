//! Calendar read command adapters.

#[cfg(any(test, mobile))]
use ganbaru_calendar::reads::CalendarNotificationSchedulerRows;
use ganbaru_calendar::reads::native_window::{NativeCalendarWindow, NativeWindowRequest};
use ganbaru_calendar::reads::{
    CalendarExportSnapshot, CalendarFullEventRows, CalendarPanelEventRows, CalendarWindowRows,
};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

/// Read and expand canonical Calendar data in one native request.
#[tauri::command]
pub async fn calendar_load_native_window<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NativeWindowRequest,
) -> Result<NativeCalendarWindow, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::reads::load_native_window(&pool, request).await
}

/// Return native occurrences only for commitments with a persisted Focus configuration.
#[tauri::command]
pub async fn calendar_load_native_focus_window<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NativeWindowRequest,
) -> Result<NativeCalendarWindow, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::reads::load_native_focus_window(&pool, request).await
}

/// Read only reminder-bearing canonical events and expand their home-zone occurrences.
#[tauri::command]
pub async fn calendar_load_native_notification_window<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NativeWindowRequest,
) -> Result<NativeCalendarWindow, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::reads::load_native_notification_window(&pool, request).await
}

/// Return one bounded, consistent Calendar export snapshot from the active vault.
#[tauri::command]
pub async fn calendar_load_export_snapshot<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    calendar_id: String,
) -> Result<CalendarExportSnapshot, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::reads::load_export_snapshot(&pool, calendar_id).await
}

/// Load the stored events and overrides that intersect one render window.
#[tauri::command]
pub async fn calendar_load_window<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    window_start_date: String,
    window_end_date: String,
    window_start_utc: String,
    window_end_exclusive_utc: String,
    include_total_event_count: Option<bool>,
) -> Result<CalendarWindowRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::reads::load_window(
        &pool,
        window_start_date,
        window_end_date,
        window_start_utc,
        window_end_exclusive_utc,
        include_total_event_count,
    )
    .await
}

/// Load the reminder-bearing rows the mobile notification scheduler expands.
#[cfg(any(test, mobile))]
#[tauri::command]
pub async fn calendar_load_notification_scheduler_window<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    window_start_date: String,
    window_end_date: String,
    window_start_utc: String,
    window_end_exclusive_utc: String,
) -> Result<CalendarNotificationSchedulerRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::reads::load_notification_scheduler_window(
        &pool,
        window_start_date,
        window_end_date,
        window_start_utc,
        window_end_exclusive_utc,
    )
    .await
}

/// Load one event with the attendees the side panel shows.
#[tauri::command]
pub async fn calendar_load_panel_event<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
) -> Result<CalendarPanelEventRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::reads::load_panel_event(&pool, id).await
}

/// Load one event with every child row the full editor needs.
#[tauri::command]
pub async fn calendar_load_full_event<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
) -> Result<CalendarFullEventRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::reads::load_full_event(&pool, id).await
}
