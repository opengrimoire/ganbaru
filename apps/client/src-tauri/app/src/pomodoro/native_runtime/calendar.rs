use ganbaru_pomodoro::{FocusErrorCode, FocusExecutionError};
use sqlx::{Sqlite, Transaction};

pub(super) type CalendarContext = ganbaru_calendar::reads::focus_context::FocusCalendarContext;

/// Resolve the owner from canonical Calendar and committed execution in the accepted transaction.
pub(super) async fn resolve(
    tx: &mut Transaction<'_, Sqlite>,
    now_ms: i64,
    requested: Option<&str>,
) -> Result<CalendarContext, FocusExecutionError> {
    ganbaru_calendar::reads::focus_context::resolve(tx, now_ms, requested)
        .await
        .map_err(|message| FocusExecutionError {
            code: FocusErrorCode::Unavailable,
            message,
            current_revision: None,
        })
}
