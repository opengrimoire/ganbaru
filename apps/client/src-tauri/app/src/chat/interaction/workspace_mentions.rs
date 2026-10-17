//! Workspace mention validation and prompt catalog operations.

use super::support::{chat_pool, require_workspace};
use crate::chat::interaction_commands::SearchWorkingFolderPathsRequest;
use crate::chat::workspace::{
    WorkingFolderAuthorizationOperation, resolve_workspace_relative_path,
};
use ganbaru_chat::composer::prompt_catalog::{merge_prompt_entries, read_static_prompt_catalog};
use ganbaru_chat::composer::workspace_mentions::{
    ProjectWorkingFolderPathPage, search_workspace_paths,
    workspace_mention_is_safety_excluded as mention_is_safety_excluded,
};
use ganbaru_chat::runtime::ChatRuntimeRegistry;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatPromptCatalogEntry, ChatResult, ChatThreadId,
    ProjectWorkingFolderId, ProviderInstanceId,
};
use tauri::Manager;

pub(crate) async fn search_working_folder_paths(
    app: tauri::AppHandle,
    db_url: String,
    request: SearchWorkingFolderPathsRequest,
) -> ChatResult<ProjectWorkingFolderPathPage> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let authorized = require_workspace(
        &app,
        &pool,
        &request.working_folder_id,
        WorkingFolderAuthorizationOperation::MentionResolution,
    )
    .await?;
    let authorized = crate::chat::execution_environment::resolve_environment_workspace(
        &app,
        &pool,
        authorized,
        request.execution_environment_id.as_deref(),
    )
    .await?;
    let root = authorized.canonical_path;
    tauri::async_runtime::spawn_blocking(move || {
        search_workspace_paths(
            &root,
            &request.query,
            request.include_ignored,
            request.cursor.as_deref(),
            request.limit,
        )
    })
    .await
    .map_err(workspace_search_worker_error)?
}

pub(crate) async fn validate_working_folder_mentions(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    relative_paths: Vec<String>,
    execution_environment_id: Option<String>,
) -> ChatResult<()> {
    if relative_paths.len() > 100 {
        return Err(ChatError::validation(
            "mentions",
            "Too many workspace mentions",
        ));
    }
    let pool = chat_pool(app.clone(), db_url).await?;
    let authorized = require_workspace(
        &app,
        &pool,
        &working_folder_id,
        WorkingFolderAuthorizationOperation::MentionResolution,
    )
    .await?;
    let authorized = crate::chat::execution_environment::resolve_environment_workspace(
        &app,
        &pool,
        authorized,
        execution_environment_id.as_deref(),
    )
    .await?;
    for path in relative_paths {
        if mention_is_safety_excluded(&path) {
            return Err(ChatError::new(
                ChatErrorCode::Permission,
                "This workspace path is excluded from Chat context",
                false,
            ));
        }
        resolve_workspace_relative_path(&authorized, &path)?;
    }
    Ok(())
}

pub(crate) async fn list_prompt_catalog(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    provider_instance_id: ProviderInstanceId,
    thread_id: Option<ChatThreadId>,
) -> ChatResult<Vec<ChatPromptCatalogEntry>> {
    let settings = crate::chat::settings_commands::read_provider(&app, &provider_instance_id)?;
    let stale = settings
        .last_probe
        .as_ref()
        .is_none_or(|probe| probe.state != ganbaru_chat_contracts::models::ProbeState::Healthy);
    let pool = chat_pool(app.clone(), db_url).await?;
    let workspace = require_workspace(
        &app,
        &pool,
        &working_folder_id,
        WorkingFolderAuthorizationOperation::FileRead,
    )
    .await?;
    let workspace_path = workspace.canonical_path;
    let configuration = settings.configuration;
    let mut entries = tauri::async_runtime::spawn_blocking(move || {
        read_static_prompt_catalog(&workspace_path, &configuration, stale)
    })
    .await
    .map_err(prompt_catalog_worker_error)?;
    if let Some(thread_id) = thread_id {
        let live = app
            .state::<ChatRuntimeRegistry>()
            .owner(thread_id)?
            .prompt_catalog()
            .await?;
        entries.extend(live);
    }
    Ok(merge_prompt_entries(entries))
}

pub(crate) fn workspace_mention_is_safety_excluded(relative_path: &str) -> bool {
    mention_is_safety_excluded(relative_path)
}

fn workspace_search_worker_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Workspace mention search worker stopped",
        true,
    )
}

fn prompt_catalog_worker_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Provider prompt catalog worker stopped",
        true,
    )
}
