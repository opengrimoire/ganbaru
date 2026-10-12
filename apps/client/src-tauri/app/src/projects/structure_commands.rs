use ganbaru_projects::structure_commands;
use ganbaru_projects::{
    ProjectGroupCreate, ProjectGroupUpdate, ProjectPriorityCreate, ProjectPriorityUpdate,
    ProjectSectionCreate, ProjectSectionUpdate, ProjectStatusCreate, ProjectStatusUpdate,
    ProjectTagCreate, ProjectTagUpdate, ProjectsMutationRows,
};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

#[tauri::command]
pub async fn projects_create_group<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    group: ProjectGroupCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::create_group(&pool, group).await
}

#[tauri::command]
pub async fn projects_update_group<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    group: ProjectGroupUpdate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::update_group(&pool, group).await
}

#[tauri::command]
pub async fn projects_delete_group<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    group_id: String,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::delete_group(&pool, group_id).await
}

#[tauri::command]
pub async fn projects_set_group_collapsed<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    group_id: String,
    collapsed: bool,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::set_group_collapsed(&pool, group_id, collapsed).await
}

#[tauri::command]
pub async fn projects_create_section<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    section: ProjectSectionCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::create_section(&pool, section).await
}

#[tauri::command]
pub async fn projects_update_section<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    section: ProjectSectionUpdate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::update_section(&pool, section).await
}

#[tauri::command]
pub async fn projects_create_status<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    status: ProjectStatusCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::create_status(&pool, status).await
}

#[tauri::command]
pub async fn projects_update_status<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    status: ProjectStatusUpdate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::update_status(&pool, status).await
}

#[tauri::command]
pub async fn projects_delete_status<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    status_id: String,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::delete_status(&pool, status_id).await
}

#[tauri::command]
pub async fn projects_create_priority<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    priority: ProjectPriorityCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::create_priority(&pool, priority).await
}

#[tauri::command]
pub async fn projects_update_priority<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    priority: ProjectPriorityUpdate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::update_priority(&pool, priority).await
}

#[tauri::command]
pub async fn projects_delete_priority<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    project_id: String,
    priority_id: String,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::delete_priority(&pool, project_id, priority_id).await
}

#[tauri::command]
pub async fn projects_create_tag<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    tag: ProjectTagCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::create_tag(&pool, tag).await
}

#[tauri::command]
pub async fn projects_update_tag<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    tag: ProjectTagUpdate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::update_tag(&pool, tag).await
}

#[tauri::command]
pub async fn projects_delete_tag<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    tag_id: String,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::delete_tag(&pool, tag_id).await
}
