use ganbaru_projects::ProjectsMutationRows;
use ganbaru_projects::task_bulk::{self, TaskBulkRequest};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

#[tauri::command]
pub async fn projects_apply_task_bulk<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: TaskBulkRequest,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    task_bulk::apply_task_bulk(&pool, &request).await
}
