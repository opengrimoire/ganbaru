use ganbaru_projects::preferences;
use ganbaru_projects::{ProjectViewPreferenceUpsert, ProjectsMutationRows};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

#[tauri::command]
pub async fn projects_upsert_view_preference<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    preference: ProjectViewPreferenceUpsert,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    preferences::upsert_view_preference(&pool, preference).await
}

#[tauri::command]
pub async fn projects_delete_view_preference<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    project_id: String,
    view_id: String,
    preference_key: String,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    preferences::delete_view_preference(&pool, project_id, view_id, preference_key).await
}
