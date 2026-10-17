use crate::db::connect_sqlite;
use tauri::{AppHandle, Runtime};

pub use ganbaru_notes::project_history::{
    NotesHistoricalPageDto, NotesHistoryRetentionImpactDto, NotesMutationResultDto,
    NotesProjectHistoryRestorePlanDto, NotesProjectHistoryScheduleDto, NotesProjectHistoryTreeDto,
    NotesProjectHistoryVersionDto, NotesProjectHistoryVersionListDto,
};
pub(crate) use ganbaru_notes::project_history::{
    create_safety_checkpoint_for_page, ensure_page_baseline_for_mutation, mutation_result,
};

pub mod commands {
    use super::*;

    #[tauri::command]
    pub async fn notes_preview_project_history_restore<R: Runtime>(
        app: AppHandle<R>,
        db_url: String,
        project_id: String,
        version_id: String,
    ) -> Result<NotesProjectHistoryRestorePlanDto, String> {
        let pool = connect_sqlite(app, db_url).await?;
        ganbaru_notes::project_history::commands::notes_preview_project_history_restore(
            &pool, project_id, version_id,
        )
        .await
    }

    #[tauri::command]
    pub async fn notes_restore_project_history_version<R: Runtime>(
        app: AppHandle<R>,
        db_url: String,
        project_id: String,
        version_id: String,
    ) -> Result<NotesProjectHistoryVersionDto, String> {
        let pool = connect_sqlite(app, db_url).await?;
        ganbaru_notes::project_history::commands::notes_restore_project_history_version(
            &pool, project_id, version_id,
        )
        .await
    }
}

pub mod reads {
    use super::*;

    #[tauri::command]
    pub async fn notes_list_project_history_versions<R: Runtime>(
        app: AppHandle<R>,
        db_url: String,
        project_id: String,
        cursor_time: Option<String>,
        cursor_id: Option<String>,
        page_size: Option<i64>,
    ) -> Result<NotesProjectHistoryVersionListDto, String> {
        let pool = connect_sqlite(app, db_url).await?;
        ganbaru_notes::project_history::reads::notes_list_project_history_versions(
            &pool,
            project_id,
            cursor_time,
            cursor_id,
            page_size,
        )
        .await
    }

    #[tauri::command]
    pub async fn notes_load_project_history_tree<R: Runtime>(
        app: AppHandle<R>,
        db_url: String,
        project_id: String,
        version_id: String,
    ) -> Result<NotesProjectHistoryTreeDto, String> {
        let pool = connect_sqlite(app, db_url).await?;
        ganbaru_notes::project_history::reads::notes_load_project_history_tree(
            &pool, project_id, version_id,
        )
        .await
    }

    #[tauri::command]
    pub async fn notes_load_project_history_page<R: Runtime>(
        app: AppHandle<R>,
        db_url: String,
        project_id: String,
        version_id: String,
        page_id: String,
    ) -> Result<NotesHistoricalPageDto, String> {
        let pool = connect_sqlite(app, db_url).await?;
        ganbaru_notes::project_history::reads::notes_load_project_history_page(
            &pool, project_id, version_id, page_id,
        )
        .await
    }
}

pub mod retention {
    use super::*;

    #[tauri::command]
    pub async fn notes_history_retention_impact<R: Runtime>(
        app: AppHandle<R>,
        db_url: String,
        project_id: Option<String>,
        retention_days: i64,
    ) -> Result<NotesHistoryRetentionImpactDto, String> {
        let pool = connect_sqlite(app, db_url).await?;
        ganbaru_notes::project_history::retention::notes_history_retention_impact(
            &pool,
            project_id,
            retention_days,
        )
        .await
    }

    #[tauri::command]
    pub async fn notes_prune_project_history<R: Runtime>(
        app: AppHandle<R>,
        db_url: String,
        project_id: String,
    ) -> Result<i64, String> {
        let pool = connect_sqlite(app, db_url).await?;
        ganbaru_notes::project_history::retention::notes_prune_project_history(&pool, project_id)
            .await
    }
}

pub mod schedule {
    use super::*;

    #[tauri::command]
    pub async fn notes_initialize_project_history<R: Runtime>(
        app: AppHandle<R>,
        db_url: String,
        project_id: String,
    ) -> Result<Option<NotesProjectHistoryVersionDto>, String> {
        let pool = connect_sqlite(app, db_url).await?;
        ganbaru_notes::project_history::schedule::notes_initialize_project_history(
            &pool, project_id,
        )
        .await
    }

    #[tauri::command]
    pub async fn notes_flush_due_project_history<R: Runtime>(
        app: AppHandle<R>,
        db_url: String,
    ) -> Result<NotesProjectHistoryScheduleDto, String> {
        let pool = connect_sqlite(app, db_url).await?;
        ganbaru_notes::project_history::schedule::notes_flush_due_project_history(&pool).await
    }
}
