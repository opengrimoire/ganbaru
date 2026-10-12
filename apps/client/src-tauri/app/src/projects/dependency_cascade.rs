use ganbaru_projects::ProjectsMutationRows;
use ganbaru_projects::dependency_cascade::{
    self, DependencyCascadeApply, DependencyCascadePreview,
};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

#[tauri::command]
pub async fn projects_preview_dependency_cascade<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    project_id: String,
) -> Result<DependencyCascadePreview, String> {
    let pool = connect_sqlite(app, db_url).await?;
    dependency_cascade::preview_dependency_cascade(&pool, &project_id).await
}

#[tauri::command]
pub async fn projects_apply_dependency_cascade<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: DependencyCascadeApply,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    dependency_cascade::apply_dependency_cascade(&pool, &request).await
}
