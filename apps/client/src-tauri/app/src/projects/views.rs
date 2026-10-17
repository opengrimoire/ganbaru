use ganbaru_projects::views;
use ganbaru_projects::{
    ProjectViewCreate, ProjectViewRename, ProjectViewReorder, ProjectsMutationRows,
};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

#[tauri::command]
pub async fn projects_create_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    view: ProjectViewCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    views::create_view(&pool, view).await
}

#[tauri::command]
pub async fn projects_rename_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    rename: ProjectViewRename,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    views::rename_view(&pool, rename).await
}

#[tauri::command]
pub async fn projects_reorder_views<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    reorder: ProjectViewReorder,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    views::reorder_views(&pool, reorder).await
}

#[tauri::command]
pub async fn projects_delete_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    id: String,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    views::delete_view(&pool, &id).await
}
