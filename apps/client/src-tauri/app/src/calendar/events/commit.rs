//! Calendar Save command adapter.

use ganbaru_calendar::events::commit::{CommitFailure, CommitReply, CommitRequest};

/// Enqueue Save with the owner that serializes Calendar and Focus publication.
#[tauri::command]
pub(crate) async fn calendar_commit_edit(
    app: tauri::AppHandle,
    request: CommitRequest,
) -> Result<CommitReply, CommitFailure> {
    request.validate()?;
    crate::pomodoro::native_runtime::commit_calendar_edit(&app, request).await
}
