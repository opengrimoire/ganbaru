//! Tauri command adapters for hosted source-control services.

use super::credentials::PlatformCredentialStore;
use super::workspace::WorkingFolderAuthorizationOperation;
use crate::db;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId,
};
use std::path::PathBuf;
use tauri::Manager;

pub use ganbaru_chat::source_control::*;

#[tauri::command]
pub async fn chat_discover_source_control(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    execution_environment_id: Option<String>,
) -> ChatResult<Vec<HostedSourceControlRead>> {
    let root = source_control_root(
        &app,
        &db_url,
        &working_folder_id,
        execution_environment_id.as_deref(),
    )
    .await?;
    discover_source_control(&root, &PlatformCredentialStore::default()).await
}

#[tauri::command]
pub async fn chat_list_hosted_change_requests(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    execution_environment_id: Option<String>,
    provider_kind: HostedSourceControlKind,
    repository_slug: String,
    limit: u32,
) -> ChatResult<Vec<HostedChangeRequestRead>> {
    let root = source_control_root(
        &app,
        &db_url,
        &working_folder_id,
        execution_environment_id.as_deref(),
    )
    .await?;
    list_hosted_change_requests(
        &root,
        &PlatformCredentialStore::default(),
        provider_kind,
        repository_slug,
        limit,
    )
    .await
}

#[tauri::command]
pub async fn chat_create_hosted_change_request(
    app: tauri::AppHandle,
    db_url: String,
    request: CreateHostedChangeRequest,
) -> ChatResult<HostedChangeRequestRead> {
    let root = source_control_root(
        &app,
        &db_url,
        &request.working_folder_id,
        request.execution_environment_id.as_deref(),
    )
    .await?;
    create_hosted_change_request(&root, &PlatformCredentialStore::default(), request).await
}

#[tauri::command]
pub async fn chat_checkout_hosted_change_request(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    execution_environment_id: Option<String>,
    provider_kind: HostedSourceControlKind,
    reference: String,
    remote_name: Option<String>,
) -> ChatResult<ganbaru_chat::git::GitStatusRead> {
    let root = source_control_root(
        &app,
        &db_url,
        &working_folder_id,
        execution_environment_id.as_deref(),
    )
    .await?;
    let _guard = app
        .state::<super::workspace::mutation::ChatWorkspaceMutationRegistry>()
        .try_mutation(&root)?;
    checkout_hosted_change_request(&root, provider_kind, reference, remote_name).await
}

#[tauri::command]
pub async fn chat_configure_bitbucket_credential(
    request: ConfigureBitbucketCredential,
) -> ChatResult<()> {
    configure_bitbucket_credential(&PlatformCredentialStore::default(), request).await
}

#[tauri::command]
pub fn chat_remove_bitbucket_credential(repository_slug: String) -> ChatResult<bool> {
    remove_bitbucket_credential(&PlatformCredentialStore::default(), repository_slug)
}

async fn source_control_root(
    app: &tauri::AppHandle,
    db_url: &str,
    working_folder_id: &ProjectWorkingFolderId,
    execution_environment_id: Option<&str>,
) -> ChatResult<PathBuf> {
    let pool = db::connect_sqlite(app.clone(), db_url.to_string())
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))?;
    let authorized = super::workspace::commands::authorize_working_folder(
        app,
        &pool,
        working_folder_id,
        WorkingFolderAuthorizationOperation::Git,
    )
    .await?;
    Ok(super::execution_environment::resolve_environment_workspace(
        app,
        &pool,
        authorized,
        execution_environment_id,
    )
    .await?
    .canonical_path)
}
