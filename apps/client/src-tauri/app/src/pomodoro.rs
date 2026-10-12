//! Native Focus commands and canonical history reads. WebViews cannot write execution.

use crate::db::connect_sqlite;
pub use ganbaru_pomodoro::*;
use tauri::{AppHandle, Runtime};

#[cfg(desktop)]
pub(crate) mod idle;
pub(crate) mod native_runtime;
#[cfg(desktop)]
pub(crate) mod overlay;
#[cfg(desktop)]
pub(crate) use native_runtime::{
    FocusNativeContext, capture_native_context, native_control_in_context,
};
pub(crate) use native_runtime::{
    invalidate_calendar, resume_after_vault_handoff, setup, stop_for_vault_handoff,
};

/// Read canonical execution history, resolving current occurrence aliases for the Calendar rail.
#[tauri::command]
pub async fn pomodoro_load_segments_for_events<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    event_ids: Vec<String>,
) -> Result<Vec<PomodoroSegmentRead>, String> {
    ganbaru_pomodoro::pomodoro_load_segments_for_events(
        connect_sqlite(app, db_url).await?,
        event_ids,
    )
    .await
}
