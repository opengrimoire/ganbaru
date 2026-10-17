//! Calendar collection command adapters.

use ganbaru_calendar::calendars::{CalendarRead, CalendarWrite};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

/// Lists every calendar by name.
#[tauri::command]
pub async fn calendar_list_calendars<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<Vec<CalendarRead>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::calendars::list_calendars(&pool).await
}

/// Finds the calendar imported from an iCalendar file name.
#[tauri::command]
pub async fn calendar_find_imported_calendar<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    filename: String,
) -> Result<Option<CalendarRead>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::calendars::find_imported_calendar(&pool, filename).await
}

/// Counts the events stored in one calendar.
#[tauri::command]
pub async fn calendar_count_events<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    calendar_id: String,
) -> Result<i64, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::calendars::count_events(&pool, calendar_id).await
}

/// Creates a calendar.
#[tauri::command]
pub async fn calendar_add_calendar<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    calendar: CalendarWrite,
) -> Result<(), String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::calendars::add_calendar(&pool, calendar).await
}

/// Shows or hides a calendar.
#[tauri::command]
pub async fn calendar_set_visibility<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
    visible: bool,
    updated_at: String,
) -> Result<(), String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::calendars::set_visibility(&pool, id, visible, updated_at).await
}

/// Removes a calendar, archiving protected history and keeping archived import graphs.
#[tauri::command]
pub async fn calendar_remove_calendar<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
) -> Result<(), String> {
    let device = crate::calendar::device_date::source(&app);
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::calendars::remove_calendar(&pool, device, id).await
}
