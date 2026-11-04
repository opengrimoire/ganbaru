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
        .map_err(unavailable)
}

/// Resolve the owner for automatic admission, which skips occurrences other devices are
/// focusing on.
pub(super) async fn resolve_for_admission(
    tx: &mut Transaction<'_, Sqlite>,
    now_ms: i64,
    local_device_id: &str,
) -> Result<CalendarContext, FocusExecutionError> {
    ganbaru_calendar::reads::focus_context::resolve_for_admission(tx, now_ms, local_device_id)
        .await
        .map_err(unavailable)
}

fn unavailable(message: String) -> FocusExecutionError {
    FocusExecutionError {
        code: FocusErrorCode::Unavailable,
        message,
        current_revision: None,
    }
}
