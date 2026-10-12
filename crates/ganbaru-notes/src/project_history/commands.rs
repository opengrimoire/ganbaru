use super::*;

pub async fn notes_preview_project_history_restore(
    pool: &SqlitePool,
    project_id: String,
    version_id: String,
) -> Result<NotesProjectHistoryRestorePlanDto, String> {
    restore::preview_restore(pool, &project_id, &version_id).await
}

pub async fn notes_restore_project_history_version(
    pool: &SqlitePool,
    project_id: String,
    version_id: String,
) -> Result<NotesProjectHistoryVersionDto, String> {
    restore::restore_version(pool, &project_id, &version_id).await
}
