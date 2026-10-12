use ganbaru_projects::workspace;
use ganbaru_projects::{
    ProjectOptionalDataKind, ProjectTaskDetailData, ProjectTaskViewPage, ProjectTaskViewRequest,
    ProjectViewId, ProjectsOptionalData, ProjectsWorkspaceSnapshot,
};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

#[tauri::command]
pub async fn projects_load_workspace<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    preferred_project_id: Option<String>,
    active_view: ProjectViewId,
) -> Result<ProjectsWorkspaceSnapshot, String> {
    let pool = connect_sqlite(app, db_url).await?;
    workspace::load_workspace(&pool, preferred_project_id.as_deref(), active_view).await
}

#[tauri::command]
pub async fn projects_refresh_workspace<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    preferred_project_id: Option<String>,
    active_view: ProjectViewId,
) -> Result<ProjectsWorkspaceSnapshot, String> {
    let pool = connect_sqlite(app, db_url).await?;
    workspace::refresh_workspace(&pool, preferred_project_id.as_deref(), active_view).await
}

#[tauri::command]
pub async fn projects_load_task_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: ProjectTaskViewRequest,
) -> Result<ProjectTaskViewPage, String> {
    let pool = connect_sqlite(app, db_url).await?;
    workspace::load_task_view(&pool, request).await
}

#[tauri::command]
pub async fn projects_load_task_detail<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    task_id: String,
) -> Result<ProjectTaskDetailData, String> {
    let pool = connect_sqlite(app, db_url).await?;
    workspace::load_task_detail(&pool, &task_id).await
}

#[tauri::command]
pub async fn projects_load_optional_data<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    project_id: Option<String>,
    kind: ProjectOptionalDataKind,
) -> Result<ProjectsOptionalData, String> {
    let pool = connect_sqlite(app, db_url).await?;
    workspace::load_optional_data(&pool, project_id.as_deref(), kind).await
}
