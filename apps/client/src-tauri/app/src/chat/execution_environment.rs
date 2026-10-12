//! Device-local Chat execution environments and safe Git worktree lifecycle.

use super::device_state::{read_active_device_scope, update_active_device_scope};
use super::workspace::{AuthorizedWorkingFolder, WorkingFolderAuthorizationOperation};
use crate::db;
use chrono::{SecondsFormat, Utc};
use ganbaru_chat::git;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ChatThreadId, ProjectWorkingFolderId, UtcTimestamp,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use sqlx::{Row, SqlitePool};
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt};
#[cfg(windows)]
use std::{
    fs::OpenOptions,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
};
use tauri::Manager;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_READ, FILE_SHARE_WRITE,
};

const WORKTREE_DIRECTORY: &str = "chat-worktrees";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatExecutionEnvironmentKind {
    CurrentFolder,
    Worktree,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatExecutionEnvironmentRead {
    pub id: String,
    pub working_folder_id: ProjectWorkingFolderId,
    pub kind: ChatExecutionEnvironmentKind,
    pub display_name: String,
    pub lifecycle_state: String,
    pub branch_name: Option<String>,
    pub base_reference: Option<String>,
    pub remote_name: Option<String>,
    pub cleanup_state: Option<String>,
    pub local_path: Option<String>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChatWorktreeRequest {
    pub environment_id: String,
    pub working_folder_id: ProjectWorkingFolderId,
    pub display_name: String,
    pub branch_name: String,
    pub base_reference: String,
    pub remote_name: Option<String>,
    pub fetch_remote: bool,
}

#[tauri::command]
pub async fn chat_list_execution_environments(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
) -> ChatResult<Vec<ChatExecutionEnvironmentRead>> {
    let pool = chat_pool(&app, db_url).await?;
    let paths = read_active_device_scope(&app)
        .map_err(device_state_error)?
        .execution_environment_paths;
    let rows = sqlx::query(
        "SELECT environment.id, environment.working_folder_id, environment.kind,
                environment.display_name, environment.lifecycle_state, environment.created_at,
                environment.updated_at, worktree.branch_name, worktree.base_reference,
                worktree.remote_name, worktree.cleanup_state
         FROM chat_execution_environments environment
         LEFT JOIN chat_worktrees worktree
           ON worktree.execution_environment_id = environment.id
         WHERE environment.working_folder_id = ? AND environment.archived_at IS NULL
         ORDER BY environment.kind, environment.created_at, environment.id",
    )
    .bind(working_folder_id.as_str())
    .fetch_all(&pool)
    .await
    .map_err(persistence_error)?;
    rows.into_iter()
        .map(|row| parse_environment(row, &paths))
        .collect()
}

#[tauri::command]
pub async fn chat_create_worktree_environment(
    app: tauri::AppHandle,
    mutations: tauri::State<'_, super::workspace::mutation::ChatWorkspaceMutationRegistry>,
    db_url: String,
    request: CreateChatWorktreeRequest,
) -> ChatResult<ChatExecutionEnvironmentRead> {
    validate_id(&request.environment_id)?;
    validate_label(&request.display_name, "displayName", 240)?;
    validate_ref(&request.branch_name, "branchName")?;
    validate_ref(&request.base_reference, "baseReference")?;
    if let Some(remote) = request.remote_name.as_deref() {
        validate_ref(remote, "remoteName")?;
    }
    let pool = chat_pool(&app, db_url).await?;
    let root = authorized_workspace(&app, &pool, &request.working_folder_id).await?;
    let _mutation = mutations.try_mutation(&root.canonical_path)?;
    if request.fetch_remote {
        git::fetch(&root.canonical_path, request.remote_name.as_deref()).await?;
    }
    let path = worktree_path(&app, &request.environment_id)?;
    prepare_worktree_parent(&app)?;
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Chat worktree directory already exists",
                true,
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(error)),
    }
    let path_text = path
        .to_str()
        .ok_or_else(|| ChatError::validation("environmentId", "Worktree path is unsupported"))?
        .to_string();
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_execution_environments
            (id, working_folder_id, kind, display_name, repository_identity,
             lifecycle_state, created_at, updated_at)
         VALUES (?, ?, 'worktree', ?, ?, 'creating', ?, ?)",
    )
    .bind(&request.environment_id)
    .bind(request.working_folder_id.as_str())
    .bind(request.display_name.trim())
    .bind(&root.repository_identity)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_worktrees
            (execution_environment_id, branch_name, base_reference, remote_name,
             cleanup_policy, cleanup_state, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'ask', 'retained', ?, ?)",
    )
    .bind(&request.environment_id)
    .bind(&request.branch_name)
    .bind(&request.base_reference)
    .bind(&request.remote_name)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    update_active_device_scope(&app, |scope| {
        scope
            .execution_environment_paths
            .insert(request.environment_id.clone(), path_text.clone());
        Ok(())
    })
    .map_err(device_state_error)?;
    if let Err(error) = git::add_worktree(
        &root.canonical_path,
        &path_text,
        &request.branch_name,
        &request.base_reference,
    )
    .await
    {
        mark_environment_failure(&pool, &request.environment_id, &error.message).await?;
        return Err(error);
    }
    if let Err(error) = verify_worktree_ownership(&app, &root, &request.environment_id, &path).await
    {
        mark_environment_failure(&pool, &request.environment_id, &error.message).await?;
        return Err(error);
    }
    let head = git::head_object_id(&path).await?;
    sqlx::query(
        "UPDATE chat_execution_environments SET lifecycle_state = 'available', updated_at = ?
         WHERE id = ?",
    )
    .bind(now.as_str())
    .bind(&request.environment_id)
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_worktrees SET head_object_id = ?, updated_at = ?
         WHERE execution_environment_id = ?",
    )
    .bind(head)
    .bind(now.as_str())
    .bind(&request.environment_id)
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    read_environment(&app, &pool, &request.environment_id).await
}

#[tauri::command]
pub async fn chat_read_thread_execution_environment(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
) -> ChatResult<Option<String>> {
    let pool = chat_pool(&app, db_url).await?;
    sqlx::query_scalar(
        "SELECT execution_environment_id FROM chat_threads WHERE id = ? AND state != 'closed'",
    )
    .bind(thread_id.as_str())
    .fetch_optional(&pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Active Chat thread was not found",
            true,
        )
    })
}

#[tauri::command]
pub async fn chat_remove_worktree_environment(
    app: tauri::AppHandle,
    mutations: tauri::State<'_, super::workspace::mutation::ChatWorkspaceMutationRegistry>,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    environment_id: String,
    confirmed: bool,
) -> ChatResult<()> {
    if !confirmed {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Worktree removal requires explicit confirmation",
            true,
        ));
    }
    let pool = chat_pool(&app, db_url).await?;
    let root = authorized_workspace(&app, &pool, &working_folder_id).await?;
    let _mutation = mutations.try_mutation(&root.canonical_path)?;
    let environment = read_environment(&app, &pool, &environment_id).await?;
    if environment.working_folder_id != working_folder_id
        || environment.kind != ChatExecutionEnvironmentKind::Worktree
    {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Worktree environment belongs to another workspace",
            true,
        ));
    }
    let resolved =
        resolve_environment_workspace(&app, &pool, root.clone(), Some(&environment_id)).await?;
    let _worktree_mutation = mutations.try_mutation(&resolved.canonical_path)?;
    let path_text = resolved
        .canonical_path
        .to_str()
        .ok_or_else(|| ChatError::validation("environmentId", "Worktree path is unsupported"))?
        .to_string();
    let status = git::status(&resolved.canonical_path).await?;
    if !status.files.is_empty() {
        sqlx::query(
            "UPDATE chat_worktrees SET cleanup_state = 'dirty', updated_at = ?
             WHERE execution_environment_id = ?",
        )
        .bind(now_timestamp()?.as_str())
        .bind(&environment_id)
        .execute(&pool)
        .await
        .map_err(persistence_error)?;
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Dirty worktrees are never removed automatically",
            true,
        ));
    }
    git::remove_worktree(&root.canonical_path, &path_text).await?;
    let now = now_timestamp()?;
    sqlx::query(
        "UPDATE chat_execution_environments
         SET lifecycle_state = 'removed', archived_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(&environment_id)
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_worktrees
         SET cleanup_state = 'cleaned', removed_at = ?, updated_at = ?
         WHERE execution_environment_id = ?",
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(&environment_id)
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    update_active_device_scope(&app, |scope| {
        scope.execution_environment_paths.remove(&environment_id);
        Ok(())
    })
    .map_err(device_state_error)
}

pub async fn resolve_environment_workspace(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    authorized: AuthorizedWorkingFolder,
    environment_id: Option<&str>,
) -> ChatResult<AuthorizedWorkingFolder> {
    let Some(environment_id) = environment_id else {
        return Ok(authorized);
    };
    let environment = read_environment(app, pool, environment_id).await?;
    if environment.working_folder_id != authorized.working_folder_id
        || environment.lifecycle_state != "available"
    {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Execution environment is unavailable for this workspace",
            true,
        ));
    }
    if environment.kind == ChatExecutionEnvironmentKind::CurrentFolder {
        return Ok(authorized);
    }
    if authorized.repository_kind != ganbaru_chat_contracts::models::RepositoryKind::Git
        || authorized.repository_identity.is_none()
    {
        return Err(worktree_ownership_error());
    }
    let stored_path = environment.local_path.ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "Worktree path is unavailable on this device",
            true,
        )
    })?;
    let expected_path = worktree_path(app, environment_id)?;
    if Path::new(&stored_path) != expected_path {
        return Err(worktree_ownership_error());
    }
    let persisted_identity: Option<String> = sqlx::query_scalar(
        "SELECT repository_identity FROM chat_execution_environments WHERE id = ?",
    )
    .bind(environment_id)
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    if persisted_identity != authorized.repository_identity {
        return Err(worktree_ownership_error());
    }
    verify_worktree_ownership(app, &authorized, environment_id, &expected_path).await?;
    Ok(AuthorizedWorkingFolder {
        canonical_path: expected_path,
        ..authorized
    })
}

/// Resolves a Chat thread to its authorized, device-local execution environment.
///
/// The thread owns the environment selection. Callers must not fall back to the
/// project folder because doing so could read or mutate a different worktree.
pub async fn authorize_thread_environment(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    operation: WorkingFolderAuthorizationOperation,
) -> ChatResult<(ProjectWorkingFolderId, String, AuthorizedWorkingFolder, u64)> {
    let row = sqlx::query(
        "SELECT working_folder_id, execution_environment_id, revision
         FROM chat_threads WHERE id = ? AND state != 'closed'",
    )
    .bind(thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Active Chat thread was not found",
            true,
        )
    })?;
    let working_folder_id = ProjectWorkingFolderId::new(
        row.try_get::<String, _>("working_folder_id")
            .map_err(persistence_error)?,
    )
    .map_err(|_| persistence_error("invalid working folder ID"))?;
    let environment_id = row
        .try_get::<Option<String>, _>("execution_environment_id")
        .map_err(persistence_error)?
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::ConfigurationInvalid,
                "Chat thread has no execution environment",
                true,
            )
        })?;
    let revision = u64::try_from(
        row.try_get::<i64, _>("revision")
            .map_err(persistence_error)?,
    )
    .map_err(|_| persistence_error("invalid thread revision"))?;
    let authorized = super::workspace::commands::authorize_working_folder(
        app,
        pool,
        &working_folder_id,
        operation,
    )
    .await?;
    let resolved =
        resolve_environment_workspace(app, pool, authorized, Some(&environment_id)).await?;
    Ok((working_folder_id, environment_id, resolved, revision))
}

async fn authorized_workspace(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatResult<AuthorizedWorkingFolder> {
    super::workspace::commands::authorize_working_folder(
        app,
        pool,
        working_folder_id,
        WorkingFolderAuthorizationOperation::Git,
    )
    .await
}

async fn read_environment(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    environment_id: &str,
) -> ChatResult<ChatExecutionEnvironmentRead> {
    let paths = read_active_device_scope(app)
        .map_err(device_state_error)?
        .execution_environment_paths;
    let row = sqlx::query(
        "SELECT environment.id, environment.working_folder_id, environment.kind,
                environment.display_name, environment.lifecycle_state, environment.created_at,
                environment.updated_at, worktree.branch_name, worktree.base_reference,
                worktree.remote_name, worktree.cleanup_state
         FROM chat_execution_environments environment
         LEFT JOIN chat_worktrees worktree
           ON worktree.execution_environment_id = environment.id
         WHERE environment.id = ?",
    )
    .bind(environment_id)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Chat execution environment was not found",
            true,
        )
    })?;
    parse_environment(row, &paths)
}

fn parse_environment(
    row: sqlx::sqlite::SqliteRow,
    paths: &std::collections::BTreeMap<String, String>,
) -> ChatResult<ChatExecutionEnvironmentRead> {
    let id: String = row.try_get("id").map_err(persistence_error)?;
    let kind = match row
        .try_get::<String, _>("kind")
        .map_err(persistence_error)?
        .as_str()
    {
        "current_folder" => ChatExecutionEnvironmentKind::CurrentFolder,
        "worktree" => ChatExecutionEnvironmentKind::Worktree,
        _ => return Err(persistence_error("invalid environment kind")),
    };
    Ok(ChatExecutionEnvironmentRead {
        local_path: paths.get(&id).cloned(),
        id,
        working_folder_id: ProjectWorkingFolderId::new(
            row.try_get::<String, _>("working_folder_id")
                .map_err(persistence_error)?,
        )
        .map_err(|_| persistence_error("invalid working folder ID"))?,
        kind,
        display_name: row.try_get("display_name").map_err(persistence_error)?,
        lifecycle_state: row.try_get("lifecycle_state").map_err(persistence_error)?,
        branch_name: row.try_get("branch_name").map_err(persistence_error)?,
        base_reference: row.try_get("base_reference").map_err(persistence_error)?,
        remote_name: row.try_get("remote_name").map_err(persistence_error)?,
        cleanup_state: row.try_get("cleanup_state").map_err(persistence_error)?,
        created_at: timestamp(&row, "created_at")?,
        updated_at: timestamp(&row, "updated_at")?,
    })
}

async fn mark_environment_failure(
    pool: &SqlitePool,
    environment_id: &str,
    detail: &str,
) -> ChatResult<()> {
    let now = now_timestamp()?;
    sqlx::query(
        "UPDATE chat_execution_environments SET lifecycle_state = 'cleanup_failed', updated_at = ?
         WHERE id = ?",
    )
    .bind(now.as_str())
    .bind(environment_id)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_worktrees
         SET cleanup_state = 'failed', cleanup_error_code = 'create_failed',
             cleanup_error_detail = ?, updated_at = ?
         WHERE execution_environment_id = ?",
    )
    .bind(detail.chars().take(2_000).collect::<String>())
    .bind(now.as_str())
    .bind(environment_id)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

fn worktree_path(app: &tauri::AppHandle, environment_id: &str) -> ChatResult<PathBuf> {
    worktree_root(app).map(|root| deterministic_worktree_path(&root, environment_id))
}

fn worktree_root(app: &tauri::AppHandle) -> ChatResult<PathBuf> {
    app.path().app_local_data_dir().map_err(device_state_error)
}

fn deterministic_worktree_path(local_data_root: &Path, environment_id: &str) -> PathBuf {
    let digest = format!("{:x}", sha2::Sha256::digest(environment_id.as_bytes()));
    local_data_root.join(WORKTREE_DIRECTORY).join(&digest[..24])
}

fn prepare_worktree_parent(app: &tauri::AppHandle) -> ChatResult<()> {
    let root = worktree_root(app)?;
    if let Err(error) = fs::create_dir_all(&root) {
        return Err(io_error(error));
    }
    let root_metadata = fs::symlink_metadata(&root).map_err(io_error)?;
    if path_metadata_is_link_or_reparse(&root_metadata) || !root_metadata.is_dir() {
        return Err(worktree_ownership_error());
    }
    let parent = root.join(WORKTREE_DIRECTORY);
    match fs::symlink_metadata(&parent) {
        Ok(metadata) if path_metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() => {
            Err(worktree_ownership_error())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&parent).map_err(io_error)?;
            let metadata = fs::symlink_metadata(&parent).map_err(io_error)?;
            if path_metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
                Err(worktree_ownership_error())
            } else {
                Ok(())
            }
        }
        Err(error) => Err(io_error(error)),
    }
}

async fn verify_worktree_ownership(
    app: &tauri::AppHandle,
    authorized: &AuthorizedWorkingFolder,
    environment_id: &str,
    candidate: &Path,
) -> ChatResult<()> {
    if authorized.repository_kind != ganbaru_chat_contracts::models::RepositoryKind::Git
        || authorized.repository_identity.is_none()
    {
        return Err(worktree_ownership_error());
    }
    let local_data_root = worktree_root(app)?;
    let expected = deterministic_worktree_path(&local_data_root, environment_id);
    if candidate != expected {
        return Err(worktree_ownership_error());
    }
    let identity_before = managed_worktree_identity(&local_data_root, candidate)?;
    let listed = git::worktrees(&authorized.canonical_path).await?;
    if !registered_worktree_matches(&listed, candidate) {
        return Err(worktree_ownership_error());
    }
    let repository_common = git::common_directory(&authorized.canonical_path).await?;
    let worktree_common = git::common_directory(candidate).await?;
    let repository_common = repository_common
        .canonicalize()
        .map_err(|_| worktree_ownership_error())?;
    let worktree_common = worktree_common
        .canonicalize()
        .map_err(|_| worktree_ownership_error())?;
    if repository_common != worktree_common {
        return Err(worktree_ownership_error());
    }
    let identity_after = managed_worktree_identity(&local_data_root, candidate)?;
    if identity_before != identity_after {
        return Err(worktree_ownership_error());
    }
    Ok(())
}

fn registered_worktree_matches(listed: &[git::GitWorktreeRead], candidate: &Path) -> bool {
    listed.iter().any(|worktree| {
        !worktree.bare
            && !worktree.prunable
            && worktree_path_matches(candidate, Path::new(&worktree.path))
    })
}

fn worktree_path_matches(expected: &Path, listed: &Path) -> bool {
    if expected == listed {
        return true;
    }
    #[cfg(windows)]
    {
        expected
            .to_string_lossy()
            .replace('/', "\\")
            .eq_ignore_ascii_case(&listed.to_string_lossy().replace('/', "\\"))
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(unix)]
fn managed_worktree_identity(local_data_root: &Path, candidate: &Path) -> ChatResult<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;

    let expected_parent = local_data_root.join(WORKTREE_DIRECTORY);
    if candidate.parent() != Some(expected_parent.as_path()) {
        return Err(worktree_ownership_error());
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW);
    let root = options
        .open(local_data_root)
        .map_err(|_| worktree_ownership_error())?;
    let parent = open_directory_at(&root, WORKTREE_DIRECTORY)?;
    let name = candidate.file_name().ok_or_else(worktree_ownership_error)?;
    let worktree = open_directory_at(&parent, name)?;
    let metadata = worktree
        .metadata()
        .map_err(|_| worktree_ownership_error())?;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(unix)]
fn open_directory_at(parent: &fs::File, name: impl AsRef<std::ffi::OsStr>) -> ChatResult<fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};

    let name = validated_relative_directory_name(name.as_ref())?;
    // SAFETY: `parent` owns a valid directory descriptor for this call, and
    // `name` is a live NUL-terminated C string. These flags require no variadic
    // mode argument, and `openat` does not retain either borrowed value.
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_RDONLY,
        )
    };
    if descriptor < 0 {
        return Err(worktree_ownership_error());
    }
    // SAFETY: A nonnegative `openat` result is a newly owned descriptor. Its
    // ownership is transferred exactly once to `File`, which closes it on drop.
    Ok(unsafe { fs::File::from_raw_fd(descriptor) })
}

#[cfg(unix)]
fn validated_relative_directory_name(name: &std::ffi::OsStr) -> ChatResult<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;

    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.contains(&b'/') {
        return Err(worktree_ownership_error());
    }
    std::ffi::CString::new(bytes).map_err(|_| worktree_ownership_error())
}

#[cfg(windows)]
fn managed_worktree_identity(local_data_root: &Path, candidate: &Path) -> ChatResult<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let expected_parent = local_data_root.join(WORKTREE_DIRECTORY);
    if candidate.parent() != Some(expected_parent.as_path()) {
        return Err(worktree_ownership_error());
    }
    let mut parent_handles = Vec::new();
    for path in [
        local_data_root.to_path_buf(),
        local_data_root.join(WORKTREE_DIRECTORY),
        candidate.to_path_buf(),
    ] {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_BACKUP_SEMANTICS.0);
        let file = options
            .open(&path)
            .map_err(|_| worktree_ownership_error())?;
        let metadata = file.metadata().map_err(|_| worktree_ownership_error())?;
        if !metadata.is_dir() || path_metadata_is_link_or_reparse(&metadata) {
            return Err(worktree_ownership_error());
        }
        if path == candidate {
            let mut information = BY_HANDLE_FILE_INFORMATION::default();
            // SAFETY: `file` owns a valid open directory handle for this call,
            // and `information` is initialized writable storage of the exact
            // type expected by Windows. The API does not retain either pointer.
            unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information) }
                .map_err(|_| worktree_ownership_error())?;
            return Ok((
                information.dwVolumeSerialNumber,
                (u64::from(information.nFileIndexHigh) << 32)
                    | u64::from(information.nFileIndexLow),
            ));
        }
        parent_handles.push(file);
    }
    Err(worktree_ownership_error())
}

#[cfg(not(any(unix, windows)))]
fn managed_worktree_identity(local_data_root: &Path, candidate: &Path) -> ChatResult<PathBuf> {
    let expected_parent = local_data_root.join(WORKTREE_DIRECTORY);
    if candidate.parent() != Some(expected_parent.as_path()) {
        return Err(worktree_ownership_error());
    }
    for path in [local_data_root, expected_parent.as_path(), candidate] {
        let metadata = fs::symlink_metadata(path).map_err(|_| worktree_ownership_error())?;
        if path_metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
            return Err(worktree_ownership_error());
        }
    }
    candidate
        .canonicalize()
        .map_err(|_| worktree_ownership_error())
}

#[cfg(windows)]
fn path_metadata_is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
}

#[cfg(not(windows))]
fn path_metadata_is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn worktree_ownership_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Chat worktree ownership could not be verified on this device",
        true,
    )
}

fn validate_id(value: &str) -> ChatResult<()> {
    validate_label(value, "environmentId", 2_048)
}

fn validate_label(value: &str, field: &str, maximum: usize) -> ChatResult<()> {
    let value = value.trim();
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        return Err(ChatError::validation(
            field,
            "Execution environment value is invalid",
        ));
    }
    Ok(())
}

fn validate_ref(value: &str, field: &str) -> ChatResult<()> {
    validate_label(value, field, 1_024)?;
    if value.starts_with('-') || value.chars().any(char::is_whitespace) {
        return Err(ChatError::validation(field, "Git reference is invalid"));
    }
    Ok(())
}

fn timestamp(row: &sqlx::sqlite::SqliteRow, column: &str) -> ChatResult<UtcTimestamp> {
    UtcTimestamp::new(
        row.try_get::<String, _>(column)
            .map_err(persistence_error)?,
    )
    .map_err(|_| persistence_error("invalid timestamp"))
}

fn now_timestamp() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "create Chat timestamp", false))
}

async fn chat_pool(app: &tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app.clone(), db_url)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))
}

fn io_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Chat worktree directory could not be created",
        true,
    )
}

fn device_state_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat execution environment device state failed",
        true,
    )
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat execution environment persistence failed",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("test clock should be valid")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "ganbaru-chat-worktree-ownership-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("test directory should be created");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn worktree_paths_are_deterministic_and_environment_scoped() {
        let root = Path::new("device-state");
        let first = deterministic_worktree_path(root, "environment:first");
        assert_eq!(
            first,
            deterministic_worktree_path(root, "environment:first")
        );
        assert_ne!(
            first,
            deterministic_worktree_path(root, "environment:second")
        );
        let expected_parent = root.join(WORKTREE_DIRECTORY);
        assert_eq!(first.parent(), Some(expected_parent.as_path()));
        assert_eq!(
            first
                .file_name()
                .expect("worktree should have a hashed name")
                .to_string_lossy()
                .len(),
            24
        );
    }

    #[test]
    fn managed_worktree_identity_rejects_paths_outside_the_device_root() {
        let directory = TestDirectory::new();
        let parent = directory.0.join(WORKTREE_DIRECTORY);
        fs::create_dir(&parent).expect("worktree parent should be created");
        let managed = parent.join("managed");
        fs::create_dir(&managed).expect("managed worktree should be created");
        assert!(managed_worktree_identity(&directory.0, &managed).is_ok());

        let outside = directory.0.join("outside");
        fs::create_dir(&outside).expect("outside directory should be created");
        assert!(managed_worktree_identity(&directory.0, &outside).is_err());
    }

    #[test]
    fn worktree_registration_requires_the_exact_live_non_bare_path() {
        let candidate = Path::new("/device/chat-worktrees/managed");
        let mut entry = git::GitWorktreeRead {
            path: candidate.to_string_lossy().to_string(),
            head: "0123456789abcdef".to_string(),
            branch: Some("feature".to_string()),
            bare: false,
            detached: false,
            locked: false,
            prunable: false,
        };
        assert!(registered_worktree_matches(&[entry.clone()], candidate));
        entry.prunable = true;
        assert!(!registered_worktree_matches(&[entry.clone()], candidate));
        entry.prunable = false;
        entry.bare = true;
        assert!(!registered_worktree_matches(&[entry], candidate));
    }

    #[cfg(unix)]
    #[test]
    fn managed_worktree_identity_rejects_a_substituted_root() {
        let directory = TestDirectory::new();
        let parent = directory.0.join(WORKTREE_DIRECTORY);
        fs::create_dir(&parent).expect("worktree parent should be created");
        let real = directory.0.join("real");
        fs::create_dir(&real).expect("real directory should be created");
        let substituted = parent.join("managed");
        std::os::unix::fs::symlink(&real, &substituted)
            .expect("substituted worktree should be created");
        assert!(managed_worktree_identity(&directory.0, &substituted).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn descriptor_relative_directory_names_are_single_components() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        assert!(validated_relative_directory_name(OsStr::new("managed")).is_ok());
        for invalid in ["", ".", "..", "/absolute", "nested/path"] {
            assert!(validated_relative_directory_name(OsStr::new(invalid)).is_err());
        }
        assert!(validated_relative_directory_name(OsStr::from_bytes(b"nul\0byte")).is_err());
    }
}
