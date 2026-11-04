//! Authorized Tauri commands for the local Git workspace service.

use super::workspace::mutation::ChatWorkspaceMutationRegistry;
use super::workspace::{AuthorizedWorkingFolder, WorkingFolderAuthorizationOperation};
use crate::db;
use ganbaru_chat::git::{self, GitStatusRead};
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitPushRequest {
    pub working_folder_id: ProjectWorkingFolderId,
    pub remote: Option<String>,
    pub branch: Option<String>,
    pub force_with_lease: bool,
    pub destructive_confirmed: bool,
    pub execution_environment_id: Option<String>,
}

#[tauri::command]
pub async fn chat_git_status(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    execution_environment_id: Option<String>,
) -> ChatResult<GitStatusRead> {
    let authorized = authorized_folder(
        &app,
        &db_url,
        &working_folder_id,
        execution_environment_id.as_deref(),
        WorkingFolderAuthorizationOperation::Git,
    )
    .await?;
    let folder = authorized.git_folder();
    git::status(folder).await
}

#[tauri::command]
pub async fn chat_git_stage(
    app: tauri::AppHandle,
    mutations: tauri::State<'_, ChatWorkspaceMutationRegistry>,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    paths: Vec<String>,
    execution_environment_id: Option<String>,
) -> ChatResult<GitStatusRead> {
    validate_paths(&paths)?;
    let authorized = authorized_folder(
        &app,
        &db_url,
        &working_folder_id,
        execution_environment_id.as_deref(),
        WorkingFolderAuthorizationOperation::GitUser,
    )
    .await?;
    let folder = authorized.git_folder();
    let _guard = mutations.try_mutation(&authorized.canonical_path)?;
    git::stage(folder, &paths).await?;
    git::status(folder).await
}

#[tauri::command]
pub async fn chat_git_unstage(
    app: tauri::AppHandle,
    mutations: tauri::State<'_, ChatWorkspaceMutationRegistry>,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    paths: Vec<String>,
    execution_environment_id: Option<String>,
) -> ChatResult<GitStatusRead> {
    validate_paths(&paths)?;
    let authorized = authorized_folder(
        &app,
        &db_url,
        &working_folder_id,
        execution_environment_id.as_deref(),
        WorkingFolderAuthorizationOperation::GitUser,
    )
    .await?;
    let folder = authorized.git_folder();
    let _guard = mutations.try_mutation(&authorized.canonical_path)?;
    git::unstage(folder, &paths).await?;
    git::status(folder).await
}

#[tauri::command]
pub async fn chat_git_commit(
    app: tauri::AppHandle,
    mutations: tauri::State<'_, ChatWorkspaceMutationRegistry>,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    message: String,
    execution_environment_id: Option<String>,
) -> ChatResult<GitStatusRead> {
    let message = message.trim();
    if message.is_empty() || message.len() > 4_000 || message.chars().any(char::is_control) {
        return Err(ChatError::validation(
            "message",
            "Commit message is invalid",
        ));
    }
    let authorized = authorized_folder(
        &app,
        &db_url,
        &working_folder_id,
        execution_environment_id.as_deref(),
        WorkingFolderAuthorizationOperation::GitUser,
    )
    .await?;
    let folder = authorized.git_folder();
    let _guard = mutations.try_mutation(&authorized.canonical_path)?;
    git::commit(folder, message).await?;
    git::status(folder).await
}

#[tauri::command]
pub async fn chat_git_fetch(
    app: tauri::AppHandle,
    mutations: tauri::State<'_, ChatWorkspaceMutationRegistry>,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    remote: Option<String>,
    execution_environment_id: Option<String>,
) -> ChatResult<GitStatusRead> {
    let remote = remote.as_deref().map(validate_ref_name).transpose()?;
    let authorized = authorized_folder(
        &app,
        &db_url,
        &working_folder_id,
        execution_environment_id.as_deref(),
        WorkingFolderAuthorizationOperation::GitUser,
    )
    .await?;
    let folder = authorized.git_folder();
    let _guard = mutations.try_mutation(&authorized.canonical_path)?;
    git::fetch(folder, remote).await?;
    git::status(folder).await
}

#[tauri::command]
pub async fn chat_git_pull(
    app: tauri::AppHandle,
    mutations: tauri::State<'_, ChatWorkspaceMutationRegistry>,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    remote: Option<String>,
    branch: Option<String>,
    execution_environment_id: Option<String>,
) -> ChatResult<GitStatusRead> {
    let remote = remote.as_deref().map(validate_ref_name).transpose()?;
    let branch = branch.as_deref().map(validate_ref_name).transpose()?;
    let authorized = authorized_folder(
        &app,
        &db_url,
        &working_folder_id,
        execution_environment_id.as_deref(),
        WorkingFolderAuthorizationOperation::GitUser,
    )
    .await?;
    let folder = authorized.git_folder();
    let _guard = mutations.try_mutation(&authorized.canonical_path)?;
    git::pull(folder, remote, branch).await?;
    git::status(folder).await
}

#[tauri::command]
pub async fn chat_git_push(
    app: tauri::AppHandle,
    mutations: tauri::State<'_, ChatWorkspaceMutationRegistry>,
    db_url: String,
    request: GitPushRequest,
) -> ChatResult<GitStatusRead> {
    if request.force_with_lease && !request.destructive_confirmed {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Force push requires explicit confirmation",
            true,
        ));
    }
    let remote = request
        .remote
        .as_deref()
        .map(validate_ref_name)
        .transpose()?;
    let branch = request
        .branch
        .as_deref()
        .map(validate_ref_name)
        .transpose()?;
    let authorized = authorized_folder(
        &app,
        &db_url,
        &request.working_folder_id,
        request.execution_environment_id.as_deref(),
        WorkingFolderAuthorizationOperation::GitUser,
    )
    .await?;
    let folder = authorized.git_folder();
    let _guard = mutations.try_mutation(&authorized.canonical_path)?;
    git::push(folder, remote, branch, request.force_with_lease).await?;
    git::status(folder).await
}

#[tauri::command]
pub async fn chat_git_initialize(
    app: tauri::AppHandle,
    mutations: tauri::State<'_, ChatWorkspaceMutationRegistry>,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    initial_branch: Option<String>,
    execution_environment_id: Option<String>,
) -> ChatResult<GitStatusRead> {
    let initial_branch = initial_branch
        .as_deref()
        .map(validate_ref_name)
        .transpose()?;
    let authorized = authorized_folder(
        &app,
        &db_url,
        &working_folder_id,
        execution_environment_id.as_deref(),
        WorkingFolderAuthorizationOperation::GitUser,
    )
    .await?;
    let folder = authorized.git_folder();
    let _guard = mutations.try_mutation(&authorized.canonical_path)?;
    git::initialize(folder, initial_branch).await?;
    super::workspace::commands::reconcile_working_folder_binding_for_id(
        &app,
        &db_url,
        &working_folder_id,
    )
    .await?;
    git::status(folder).await
}

#[tauri::command]
pub async fn chat_git_clone(
    app: tauri::AppHandle,
    mutations: tauri::State<'_, ChatWorkspaceMutationRegistry>,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    remote_url: String,
    remote_name: String,
    execution_environment_id: Option<String>,
) -> ChatResult<GitStatusRead> {
    validate_remote_url(&remote_url)?;
    let remote_name = validate_ref_name(&remote_name)?;
    let authorized = authorized_folder(
        &app,
        &db_url,
        &working_folder_id,
        execution_environment_id.as_deref(),
        WorkingFolderAuthorizationOperation::GitUser,
    )
    .await?;
    let folder = authorized.git_folder();
    let _guard = mutations.try_mutation(&authorized.canonical_path)?;
    let mut entries = std::fs::read_dir(&authorized.canonical_path).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Permission,
            "Working folder could not be inspected before cloning",
            true,
        )
    })?;
    if entries
        .next()
        .transpose()
        .map_err(|_| {
            ChatError::new(
                ChatErrorCode::Permission,
                "Working folder could not be inspected before cloning",
                true,
            )
        })?
        .is_some()
    {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Cloning requires an empty working folder",
            true,
        ));
    }
    git::clone_into(folder, &remote_url, remote_name).await?;
    super::workspace::commands::reconcile_working_folder_binding_for_id(
        &app,
        &db_url,
        &working_folder_id,
    )
    .await?;
    git::status(folder).await
}

#[tauri::command]
pub async fn chat_git_discard(
    app: tauri::AppHandle,
    mutations: tauri::State<'_, ChatWorkspaceMutationRegistry>,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    paths: Vec<String>,
    destructive_confirmed: bool,
    execution_environment_id: Option<String>,
) -> ChatResult<GitStatusRead> {
    if !destructive_confirmed {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Discarding changes requires explicit confirmation",
            true,
        ));
    }
    validate_paths(&paths)?;
    let authorized = authorized_folder(
        &app,
        &db_url,
        &working_folder_id,
        execution_environment_id.as_deref(),
        WorkingFolderAuthorizationOperation::GitUser,
    )
    .await?;
    let folder = authorized.git_folder();
    let _guard = mutations.try_mutation(&authorized.canonical_path)?;
    let current = git::status(folder).await?;
    let selected = paths
        .iter()
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>();
    let mut tracked = Vec::new();
    let mut untracked = Vec::new();
    for file in current.files {
        if !selected.contains(file.relative_path.as_str()) {
            continue;
        }
        if file.untracked {
            untracked.push(file.relative_path);
        } else if file.worktree_status != "." && file.worktree_status != "!" {
            tracked.push(file.relative_path);
        }
    }
    if tracked.len() + untracked.len() != paths.len() {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Git changes changed before discard",
            true,
        ));
    }
    git::discard_paths(folder, &tracked, &untracked).await?;
    git::status(folder).await
}

async fn authorized_folder(
    app: &tauri::AppHandle,
    db_url: &str,
    working_folder_id: &ProjectWorkingFolderId,
    execution_environment_id: Option<&str>,
    operation: WorkingFolderAuthorizationOperation,
) -> ChatResult<AuthorizedWorkingFolder> {
    let pool = db::connect_sqlite(app.clone(), db_url.to_string())
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))?;
    let authorized = super::workspace::commands::authorize_working_folder(
        app,
        &pool,
        working_folder_id,
        operation,
    )
    .await?;
    super::execution_environment::resolve_environment_workspace(
        app,
        &pool,
        authorized,
        execution_environment_id,
    )
    .await
}

fn validate_paths(paths: &[String]) -> ChatResult<()> {
    if paths.is_empty() || paths.len() > 10_000 {
        return Err(ChatError::validation(
            "paths",
            "Git path selection is invalid",
        ));
    }
    for path in paths {
        validate_relative_path(path)?;
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> ChatResult<&str> {
    if path.is_empty()
        || path.len() > 4_096
        || path.starts_with('/')
        || path.starts_with('-')
        || path.contains('\0')
        || std::path::Path::new(path).components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(ChatError::validation("path", "Git path is invalid"));
    }
    Ok(path)
}

fn validate_ref_name(value: &str) -> ChatResult<&str> {
    if value.is_empty()
        || value.len() > 1_024
        || value.starts_with('-')
        || value.chars().any(char::is_whitespace)
        || value.chars().any(char::is_control)
    {
        return Err(ChatError::validation(
            "reference",
            "Git reference is invalid",
        ));
    }
    Ok(value)
}

fn validate_remote_url(value: &str) -> ChatResult<&str> {
    if value.is_empty()
        || value.len() > 8_192
        || value.starts_with('-')
        || value.chars().any(char::is_whitespace)
        || value.chars().any(char::is_control)
    {
        return Err(ChatError::validation(
            "remoteUrl",
            "Git remote URL is invalid",
        ));
    }
    let supported = value.starts_with("https://")
        || value.starts_with("ssh://")
        || (value.starts_with("git@") && value.contains(':'));
    if !supported {
        return Err(ChatError::validation(
            "remoteUrl",
            "Git remote URL must use HTTPS or SSH",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{validate_ref_name, validate_relative_path, validate_remote_url};

    #[test]
    fn rejects_git_option_and_traversal_injections() {
        assert!(validate_relative_path("src/lib.rs").is_ok());
        assert!(validate_relative_path("../secret").is_err());
        assert!(validate_relative_path("--upload-pack=bad").is_err());
        assert!(validate_ref_name("origin").is_ok());
        assert!(validate_ref_name("--exec=bad").is_err());
        assert!(validate_remote_url("https://github.com/owner/repository.git").is_ok());
        assert!(validate_remote_url("git@github.com:owner/repository.git").is_ok());
        assert!(validate_remote_url("file:///tmp/repository").is_err());
    }
}
