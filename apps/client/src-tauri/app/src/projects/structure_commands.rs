#[cfg(desktop)]
use ganbaru_chat::repository::project_files;
use ganbaru_projects::structure_commands;
use ganbaru_projects::{
    GroupProjectReorder, ProjectGroupCreate, ProjectGroupReorder, ProjectGroupUpdate,
    ProjectPriorityCreate, ProjectPriorityUpdate, ProjectSectionCreate, ProjectSectionUpdate,
    ProjectStatusCreate, ProjectStatusUpdate, ProjectTagCreate, ProjectTagUpdate,
    ProjectsMutationRows,
};
#[cfg(desktop)]
use sqlx::SqlitePool;
#[cfg(desktop)]
use std::path::Path;
use tauri::{AppHandle, Runtime};

use crate::chat::sync::RevocationWake;
use crate::db::connect_sqlite;
use crate::vault;

#[tauri::command]
pub async fn projects_create_group<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    group: ProjectGroupCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let vault_root = vault::active_writable_vault_path(&app)?;
    structure_commands::create_group(&pool, &vault_root, group).await
}

#[tauri::command]
pub async fn projects_update_group<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    group: ProjectGroupUpdate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let vault_root = vault::active_writable_vault_path(&app)?;
    structure_commands::update_group(&pool, &vault_root, group).await
}

/// Deletes a group with its projects. The deletion cascades into Notes rows.
#[tauri::command]
pub async fn projects_delete_group<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    group_id: String,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let vault_root = vault::active_writable_vault_path(&app)?;
    #[cfg(mobile)]
    let _ = vault_root;
    #[cfg(desktop)]
    let chat_files = {
        let project_ids = structure_commands::group_project_ids(&pool, &group_id).await?;
        project_files::managed_file_paths_of_projects(&pool, &project_ids)
            .await
            .map_err(|error| format!("list Chat files of deleted projects: {}", error.message))?
    };
    let mutation = delete_group_and_wake(&pool, group_id, &app).await?;
    #[cfg(desktop)]
    remove_deleted_project_chat_files(&pool, &vault_root, &chat_files).await;
    Ok(mutation)
}

/// Deletes a group with its projects, then wakes the access revocation drain: the cascade
/// removes the projects' Chat conversations with the authorizations and runs that targeted
/// them, so only the drain's orphaned run sweep can stop those runs.
pub(crate) async fn delete_group_and_wake(
    pool: &sqlx::SqlitePool,
    group_id: String,
    wake: &impl RevocationWake,
) -> Result<ProjectsMutationRows, String> {
    let mutation = structure_commands::delete_group(pool, group_id).await?;
    wake.wake_revocation_drain();
    Ok(mutation)
}

/// Removes the managed Chat files of deleted projects. The group deletion has already
/// committed, so a file that cannot be removed is reported and left for manual cleanup
/// instead of failing the command. Mobile builds have no Chat repository, so files a vault
/// carried to a phone stay until a desktop owner deletes them.
#[cfg(desktop)]
async fn remove_deleted_project_chat_files(pool: &SqlitePool, vault_root: &Path, paths: &[String]) {
    match project_files::remove_unreferenced_managed_files(pool, vault_root, paths).await {
        Ok(failures) => {
            for failure in failures {
                eprintln!(
                    "Could not remove Chat file {} of a deleted project: {}",
                    failure.managed_relative_path, failure.reason
                );
            }
        }
        Err(error) => eprintln!(
            "Could not check Chat files of deleted projects: {}",
            error.message
        ),
    }
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

#[tauri::command]
pub async fn projects_reorder_groups<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    reorder: ProjectGroupReorder,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::reorder_groups(&pool, reorder).await
}

#[tauri::command]
pub async fn projects_reorder_group_projects<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    reorder: GroupProjectReorder,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    structure_commands::reorder_group_projects(&pool, reorder).await
}
