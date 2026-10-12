//! Calendar iCalendar import command adapter.

use ganbaru_calendar::import::{CalendarBulkImportPayload, CalendarBulkImportSummary};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

/// Validates and imports iCalendar events into their target calendar in one transaction.
#[tauri::command]
pub async fn calendar_bulk_import<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    payload: CalendarBulkImportPayload,
) -> Result<CalendarBulkImportSummary, String> {
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_calendar::import::bulk_import(&pool, payload).await
}
