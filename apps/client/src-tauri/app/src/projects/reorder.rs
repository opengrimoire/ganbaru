use ganbaru_projects::ProjectsMutationRows;
use ganbaru_projects::reorder::{self, ProjectReorderRequest};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

#[tauri::command]
pub async fn projects_reorder_item<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: ProjectReorderRequest,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    reorder::reorder_item(&pool, &request).await
}
