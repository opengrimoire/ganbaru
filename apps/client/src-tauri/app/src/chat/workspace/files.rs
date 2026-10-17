//! Tauri command adapters for bounded core workspace-file services.

use super::{AuthorizedWorkingFolder, resolve_workspace_relative_path};
use crate::db;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId,
};
use sqlx::SqlitePool;

pub use ganbaru_chat::workspace::files::*;

#[tauri::command]
pub async fn projects_list_working_folder_directory(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    relative_path: String,
    include_ignored: bool,
    execution_environment_id: Option<String>,
) -> ChatResult<ProjectWorkingFolderDirectoryRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let authorized = require_workspace(&app, &pool, &working_folder_id).await?;
    let authorized = crate::chat::execution_environment::resolve_environment_workspace(
        &app,
        &pool,
        authorized,
        execution_environment_id.as_deref(),
    )
    .await?;
    list_workspace_directory(&authorized, &relative_path, include_ignored)
}

#[tauri::command]
pub async fn projects_preview_working_folder_file(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    relative_path: String,
    execution_environment_id: Option<String>,
) -> ChatResult<ProjectWorkingFolderFilePreview> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let authorized = require_workspace(&app, &pool, &working_folder_id).await?;
    let authorized = crate::chat::execution_environment::resolve_environment_workspace(
        &app,
        &pool,
        authorized,
        execution_environment_id.as_deref(),
    )
    .await?;
    preview_workspace_file(&authorized, &relative_path)
}

#[tauri::command]
pub async fn projects_save_working_folder_file(
    app: tauri::AppHandle,
    observers: tauri::State<'_, super::observer::ChatWorkspaceObserverRegistry>,
    mutations: tauri::State<'_, super::mutation::ChatWorkspaceMutationRegistry>,
    db_url: String,
    request: SaveProjectWorkingFolderFileRequest,
) -> ChatResult<ProjectWorkingFolderFilePreview> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let authorized = require_workspace_for(
        &app,
        &pool,
        &request.working_folder_id,
        super::WorkingFolderAuthorizationOperation::FileWrite,
    )
    .await?;
    let authorized = crate::chat::execution_environment::resolve_environment_workspace(
        &app,
        &pool,
        authorized,
        request.execution_environment_id.as_deref(),
    )
    .await?;
    let _mutation = mutations.try_mutation(&authorized.canonical_path)?;
    let saved = save_workspace_file(
        &authorized,
        &request.relative_path,
        &request.contents,
        &request.expected_revision,
    )?;
    observers.invalidate_paths(
        &request.working_folder_id,
        request.execution_environment_id.as_deref(),
        vec![request.relative_path],
        false,
    );
    Ok(saved)
}

#[tauri::command]
pub async fn projects_save_working_folder_file_copy(
    app: tauri::AppHandle,
    observers: tauri::State<'_, super::observer::ChatWorkspaceObserverRegistry>,
    mutations: tauri::State<'_, super::mutation::ChatWorkspaceMutationRegistry>,
    db_url: String,
    request: SaveProjectWorkingFolderFileCopyRequest,
) -> ChatResult<ProjectWorkingFolderFilePreview> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let authorized = require_workspace_for(
        &app,
        &pool,
        &request.working_folder_id,
        super::WorkingFolderAuthorizationOperation::FileWrite,
    )
    .await?;
    let authorized = crate::chat::execution_environment::resolve_environment_workspace(
        &app,
        &pool,
        authorized,
        request.execution_environment_id.as_deref(),
    )
    .await?;
    let _mutation = mutations.try_mutation(&authorized.canonical_path)?;
    let saved = save_workspace_file_copy(
        &authorized,
        &request.source_relative_path,
        &request.target_relative_path,
        &request.contents,
    )?;
    observers.invalidate_paths(
        &request.working_folder_id,
        request.execution_environment_id.as_deref(),
        vec![request.target_relative_path],
        false,
    );
    Ok(saved)
}

#[tauri::command]
pub async fn projects_recreate_working_folder_file(
    app: tauri::AppHandle,
    observers: tauri::State<'_, super::observer::ChatWorkspaceObserverRegistry>,
    mutations: tauri::State<'_, super::mutation::ChatWorkspaceMutationRegistry>,
    db_url: String,
    request: RecreateProjectWorkingFolderFileRequest,
) -> ChatResult<ProjectWorkingFolderFilePreview> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let authorized = require_workspace_for(
        &app,
        &pool,
        &request.working_folder_id,
        super::WorkingFolderAuthorizationOperation::FileWrite,
    )
    .await?;
    let authorized = crate::chat::execution_environment::resolve_environment_workspace(
        &app,
        &pool,
        authorized,
        request.execution_environment_id.as_deref(),
    )
    .await?;
    let _mutation = mutations.try_mutation(&authorized.canonical_path)?;
    let recreated = recreate_workspace_file(
        &authorized,
        &request.relative_path,
        &request.contents,
        request.confirmed,
    )?;
    observers.invalidate_paths(
        &request.working_folder_id,
        request.execution_environment_id.as_deref(),
        vec![request.relative_path],
        false,
    );
    Ok(recreated)
}

#[tauri::command]
pub async fn projects_open_working_folder_file(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    relative_path: String,
    execution_environment_id: Option<String>,
) -> ChatResult<()> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let authorized = require_workspace(&app, &pool, &working_folder_id).await?;
    let authorized = crate::chat::execution_environment::resolve_environment_workspace(
        &app,
        &pool,
        authorized,
        execution_environment_id.as_deref(),
    )
    .await?;
    let path = resolve_workspace_relative_path(&authorized, &relative_path)?;
    super::open_authorized_path(&authorized, &path)
}

async fn require_workspace(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatResult<AuthorizedWorkingFolder> {
    require_workspace_for(
        app,
        pool,
        working_folder_id,
        super::WorkingFolderAuthorizationOperation::FileRead,
    )
    .await
}

pub(crate) async fn require_workspace_for(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    working_folder_id: &ProjectWorkingFolderId,
    operation: super::WorkingFolderAuthorizationOperation,
) -> ChatResult<AuthorizedWorkingFolder> {
    super::commands::authorize_working_folder(app, pool, working_folder_id, operation).await
}

async fn chat_pool(app: tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app, db_url)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))
}
