use ganbaru_projects::task_commands;
use ganbaru_projects::task_reminders::{ProjectTaskReminder, list_upcoming_task_reminders};
use ganbaru_projects::{
    ProjectChecklistItemCreate, ProjectChecklistItemUpdate, ProjectTaskCreate, ProjectTaskUpdate,
    ProjectsMutationRows,
};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

#[tauri::command]
pub async fn projects_create_task<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    task: ProjectTaskCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    task_commands::create_task(&pool, task).await
}

#[tauri::command]
pub async fn projects_update_task<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    task: ProjectTaskUpdate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    task_commands::update_task(&pool, task).await
}

#[tauri::command]
pub async fn projects_create_checklist_item<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    item: ProjectChecklistItemCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    task_commands::create_checklist_item(&pool, item).await
}

#[tauri::command]
pub async fn projects_update_checklist_item<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    item: ProjectChecklistItemUpdate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    task_commands::update_checklist_item(&pool, item).await
}

#[tauri::command]
pub async fn projects_delete_checklist_item<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    item_id: String,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    task_commands::delete_checklist_item(&pool, item_id).await
}

#[tauri::command]
pub async fn projects_list_upcoming_task_reminders<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    from_date: String,
) -> Result<Vec<ProjectTaskReminder>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    list_upcoming_task_reminders(&pool, &from_date).await
}
