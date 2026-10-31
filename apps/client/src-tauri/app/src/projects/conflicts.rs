//! Projects sync conflict commands: reading a project's conflicts and resolving one.

use ganbaru_projects::ProjectsMutationRows;
use ganbaru_projects::conflicts::{
    ProjectsConflictError, ProjectsConflictRead, ProjectsConflictResolution,
};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

/// The conflicts of a project: names, task titles and descriptions, checklist item titles, and
/// dependencies hidden to break a cycle, each with the devices that wrote the versions.
#[tauri::command]
pub async fn projects_conflicts<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    project_id: String,
) -> Result<ProjectsConflictRead, ProjectsConflictError> {
    let vault_id = crate::vault::active_vault_id(&app)?;
    let devices = crate::sync::devices::read(&app)?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_projects::conflicts::conflicts_from_pool(&pool, &vault_id, &devices, &project_id).await
}

/// Resolves a text conflict by keeping the displayed version or using another one, or a hidden
/// dependency by keeping it removed or adding it again.
#[tauri::command]
pub async fn projects_resolve_conflict<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    resolution: ProjectsConflictResolution,
) -> Result<ProjectsMutationRows, ProjectsConflictError> {
    let vault_id = crate::vault::active_vault_id(&app)?;
    let now_ms = crate::sync::now_ms()?;
    let pool = connect_sqlite(app, db_url).await?;
    ganbaru_projects::conflicts::resolve_from_pool(&pool, &vault_id, resolution, now_ms).await
}
