#[cfg(desktop)]
use ganbaru_chat::repository::project_files;
use ganbaru_projects::project_commands;
use ganbaru_projects::{ProjectCreate, ProjectUpdate, ProjectsMutationRows};
#[cfg(desktop)]
use sqlx::SqlitePool;
#[cfg(desktop)]
use std::path::Path;
use tauri::{AppHandle, Runtime};

use crate::chat::sync::RevocationWake;
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
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let vault_root = vault::active_writable_vault_path(&app)?;
    let mutation = project_commands::update_project(&pool, &vault_root, project).await?;
    // An update may archive the project, which ends the live authority of its Chat runs.
    crate::chat::authority_epoch::bump_authority_epoch(&app);
    Ok(mutation)
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

/// Deletes a project with what it owns, then removes the managed Chat files its rows named.
/// Its Notes pages and Calendar events stay without a project, and its managed working folder
/// stays on disk.
#[tauri::command]
pub async fn projects_delete_project<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    project_id: String,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let vault_root = vault::active_writable_vault_path(&app)?;
    #[cfg(mobile)]
    let _ = vault_root;
    #[cfg(desktop)]
    let chat_files =
        project_files::managed_file_paths_of_projects(&pool, std::slice::from_ref(&project_id))
            .await
            .map_err(|error| format!("list Chat files of the project: {}", error.message))?;
    let mutation = delete_project_and_wake(&pool, project_id, &app).await?;
    #[cfg(desktop)]
    remove_deleted_project_chat_files(&pool, &vault_root, &chat_files).await;
    Ok(mutation)
}

/// Deletes a project, then wakes the access revocation drain: the cascade removes the project's
/// Chat conversations with the authorizations and runs that targeted them, so only the drain's
/// orphaned run sweep can stop those runs.
pub(crate) async fn delete_project_and_wake(
    pool: &sqlx::SqlitePool,
    project_id: String,
    wake: &impl RevocationWake,
) -> Result<ProjectsMutationRows, String> {
    let mutation = project_commands::delete_project(pool, project_id).await?;
    wake.wake_revocation_drain();
    Ok(mutation)
}

/// Removes the managed Chat files of a deleted project. The deletion has already committed, so a
/// file that cannot be removed is reported and left for manual cleanup instead of failing the
/// command. Mobile builds have no Chat repository, so files a vault carried to a phone stay
/// until a desktop deletes them.
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
            "Could not check Chat files of a deleted project: {}",
            error.message
        ),
    }
}
