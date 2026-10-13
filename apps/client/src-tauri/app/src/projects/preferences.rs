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
