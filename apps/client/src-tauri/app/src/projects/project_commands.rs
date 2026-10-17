use ganbaru_projects::project_commands;
use ganbaru_projects::{ProjectCreate, ProjectUpdate, ProjectsMutationRows};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;
use crate::vault;

#[tauri::command]
pub async fn projects_create_project<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    project: ProjectCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let vault_root = vault::active_writable_vault_path(&app)?;
    project_commands::create_project(&pool, &vault_root, project).await
}

#[tauri::command]
pub async fn projects_update_project<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    project: ProjectUpdate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    project_commands::update_project(&pool, project).await
}

#[tauri::command]
pub async fn projects_update_notes_settings<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    project_id: String,
    notes_default_open_mode: Option<String>,
    notes_history_retention_days: Option<i64>,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    project_commands::update_notes_settings(
        &pool,
        project_id,
        notes_default_open_mode,
        notes_history_retention_days,
    )
    .await
}
