use ganbaru_projects::relationship_commands;
use ganbaru_projects::{
    ProjectLinkableEvent, ProjectLinkableEventSearch, ProjectTaskDependencyCreate,
    ProjectTaskEventLinkCreate, ProjectTaskTagLinkCreate, ProjectsMutationRows,
};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

#[tauri::command]
pub async fn projects_create_task_dependency<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    dependency: ProjectTaskDependencyCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    relationship_commands::create_task_dependency(&pool, dependency).await
}

#[tauri::command]
pub async fn projects_delete_task_dependency<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    dependency_id: String,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    relationship_commands::delete_task_dependency(&pool, dependency_id).await
}

#[tauri::command]
pub async fn projects_link_task_tag<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    link: ProjectTaskTagLinkCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    relationship_commands::link_task_tag(&pool, link).await
}

#[tauri::command]
pub async fn projects_unlink_task_tag<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    task_id: String,
    tag_id: String,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    relationship_commands::unlink_task_tag(&pool, task_id, tag_id).await
}

#[tauri::command]
pub async fn projects_link_task_event<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    link: ProjectTaskEventLinkCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    relationship_commands::link_task_event(&pool, link).await
}

#[tauri::command]
pub async fn projects_unlink_task_event<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    task_id: String,
    event_id: String,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    relationship_commands::unlink_task_event(&pool, task_id, event_id).await
}

#[tauri::command]
pub async fn projects_search_linkable_events<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    search: ProjectLinkableEventSearch,
) -> Result<Vec<ProjectLinkableEvent>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    relationship_commands::search_linkable_events(&pool, search).await
}
