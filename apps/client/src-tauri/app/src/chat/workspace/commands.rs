//! Tauri commands for project-owned working-folder management.

use super::{
    AuthorizedWorkingFolder, CreateProjectWorkingFolderRequest, ProjectWorkingFolder,
    ProjectWorkingFolderRead, WorkingFolderAuthorizationOperation, WorkingFolderKind,
    authorize_workspace, ensure_managed_working_folder_binding, filesystem_identity,
    initialized_repository_identity, open_authorized_workspace, prepare_workspace_binding,
    probe_repository, remove_active_device_binding, store_active_device_binding,
    validate_external_folder_outside_vault, workspace_read,
};
use crate::db;
use crate::projects::working_folders::{
    read_active_working_folder_scope, update_active_working_folder_scope,
};
use chrono::{SecondsFormat, Utc};
use ganbaru_chat::repository::workspaces as repository;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId, UtcTimestamp,
};
use sqlx::SqlitePool;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Manager, Runtime};
use tauri_plugin_dialog::{DialogExt, FilePath};

const MAX_PENDING_FOLDER_SELECTIONS: usize = 64;

#[derive(Clone)]
struct FolderSelectionReceipt {
    id: String,
    project_id: String,
    working_folder_id: Option<ProjectWorkingFolderId>,
    vault_root: PathBuf,
    path: PathBuf,
    filesystem_identity: String,
}

/// Bounded, device-local native selections retained until settings explicitly applies them.
#[derive(Default)]
pub struct ProjectWorkingFolderSelections(Mutex<VecDeque<FolderSelectionReceipt>>);

impl ProjectWorkingFolderSelections {
    /// Keep native receipts bounded without persisting them in the vault or device config.
    fn insert(&self, receipt: FolderSelectionReceipt) -> ChatResult<()> {
        let mut selections = self.0.lock().map_err(|_| selection_registry_error())?;
        while selections.len() >= MAX_PENDING_FOLDER_SELECTIONS {
            selections.pop_front();
        }
        selections.push_back(receipt);
        Ok(())
    }

    /// Resolve only receipts authorized for this project, target, and active vault.
    fn read(
        &self,
        selection_id: &str,
        project_id: &str,
        working_folder_id: Option<&ProjectWorkingFolderId>,
        vault_root: &Path,
    ) -> ChatResult<FolderSelectionReceipt> {
        let selections = self.0.lock().map_err(|_| selection_registry_error())?;
        selections
            .iter()
            .find(|receipt| receipt.id == selection_id)
            .filter(|receipt| {
                receipt.project_id == project_id
                    && receipt.working_folder_id.as_ref() == working_folder_id
                    && receipt.vault_root == vault_root
            })
            .cloned()
            .ok_or_else(|| {
                ChatError::validation(
                    "selectionId",
                    "Choose this working folder again before saving",
                )
            })
    }
}

fn selection_registry_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Pending project working-folder selections could not be accessed",
        true,
    )
}

/// A preview contains an opaque native authorization receipt, never a persisted binding.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkingFolderSelectionRead {
    selection_id: String,
    canonical_path: String,
    display_name: String,
}

/// Select an external path without changing project configuration, bindings, or terminals.
#[tauri::command]
pub async fn projects_pick_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    project_id: String,
    working_folder_id: Option<ProjectWorkingFolderId>,
    title: String,
) -> ChatResult<Option<ProjectWorkingFolderSelectionRead>> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let vault_root = crate::vault::active_vault_path(&app).map_err(device_state_error)?;
    if let Some(id) = &working_folder_id {
        let workspace = repository::read_workspace(&pool, id).await?;
        if workspace.project_id != project_id || workspace.kind != WorkingFolderKind::External {
            return Err(ChatError::validation(
                "workingFolderId",
                "Only this project's external folders can be rebound",
            ));
        }
    }
    let start = working_folder_id
        .as_ref()
        .and_then(|id| workspace_picker_start_directory(&app, id));
    let Some(selection) = pick_workspace_folder(&app, &title, start).await? else {
        return Ok(None);
    };
    validate_external_folder_outside_vault(&app, &selection)?;
    let path = std::fs::canonicalize(selection).map_err(|_| {
        ChatError::validation(
            "workingFolderPath",
            "Selected working folder is unavailable",
        )
    })?;
    let canonical_path = path
        .to_str()
        .ok_or_else(|| {
            ChatError::validation("workingFolderPath", "Selected path is not valid UTF-8")
        })?
        .to_string();
    let display_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("External folder")
        .to_string();
    let id = crate::chat::internal_mcp::generate_opaque_handle("working-folder-selection")?;
    let receipt = FolderSelectionReceipt {
        id: id.clone(),
        project_id,
        working_folder_id,
        path,
        vault_root,
        filesystem_identity: filesystem_identity(Path::new(&canonical_path), b"working-folder")?,
    };
    let registry = app.state::<ProjectWorkingFolderSelections>();
    registry.insert(receipt)?;
    Ok(Some(ProjectWorkingFolderSelectionRead {
        selection_id: id,
        canonical_path,
        display_name,
    }))
}

/// Apply a native selection only to the project and folder for which it was authorized.
fn selected_folder_path(
    app: &tauri::AppHandle,
    selection_id: &str,
    project_id: &str,
    working_folder_id: Option<&ProjectWorkingFolderId>,
) -> ChatResult<PathBuf> {
    let vault_root = crate::vault::active_vault_path(app).map_err(device_state_error)?;
    let registry = app.state::<ProjectWorkingFolderSelections>();
    let receipt = registry.read(selection_id, project_id, working_folder_id, &vault_root)?;
    if filesystem_identity(&receipt.path, b"working-folder")? != receipt.filesystem_identity {
        return Err(ChatError::validation(
            "workingFolderPath",
            "The selected folder changed before Save. Choose it again",
        ));
    }
    Ok(receipt.path)
}

/// Create an external association from a staged native selection, retaining its draft identity.
#[tauri::command]
pub async fn projects_add_selected_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    request: CreateProjectWorkingFolderRequest,
    selection_id: String,
) -> ChatResult<ProjectWorkingFolderRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let selection = selected_folder_path(&app, &selection_id, &request.project_id, None)?;
    add_external_workspace(&app, &pool, request, &selection).await
}

/// Rebind an existing external folder only when Save applies its staged native selection.
#[tauri::command]
pub async fn projects_bind_selected_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    selection_id: String,
) -> ChatResult<ProjectWorkingFolderRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let workspace = repository::read_workspace(&pool, &working_folder_id).await?;
    let selection = selected_folder_path(
        &app,
        &selection_id,
        &workspace.project_id,
        Some(&working_folder_id),
    )?;
    bind_workspace(&app, &pool, workspace, &selection).await
}

#[tauri::command]
pub async fn projects_list_working_folders(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<Vec<ProjectWorkingFolderRead>> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let workspaces = repository::list_workspaces(&pool).await?;
    let mut reconciled = Vec::with_capacity(workspaces.len());
    for workspace in workspaces {
        ensure_managed_working_folder_binding(&app, &workspace)?;
        reconciled.push(reconcile_working_folder_binding(&app, &pool, workspace).await?);
    }
    let scope = read_active_working_folder_scope(&app).map_err(device_state_error)?;
    reconciled
        .into_iter()
        .map(|workspace| workspace_read(workspace, &scope))
        .collect()
}

#[tauri::command]
pub async fn projects_list_working_folders_cached(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<Vec<ProjectWorkingFolderRead>> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let workspaces = repository::list_workspaces(&pool).await?;
    let mut scope = read_active_working_folder_scope(&app).map_err(device_state_error)?;
    for workspace in &workspaces {
        if workspace.kind == WorkingFolderKind::Managed
            && !scope.bindings.contains_key(&workspace.id)
        {
            ensure_managed_working_folder_binding(&app, workspace)?;
            scope = read_active_working_folder_scope(&app).map_err(device_state_error)?;
        }
    }
    workspaces
        .into_iter()
        .map(|workspace| workspace_read(workspace, &scope))
        .collect()
}

pub(crate) async fn reconcile_working_folder_binding_for_id(
    app: &tauri::AppHandle,
    db_url: &str,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatResult<ProjectWorkingFolder> {
    let pool = chat_pool(app.clone(), db_url.to_string()).await?;
    let workspace = repository::read_workspace(&pool, working_folder_id).await?;
    reconcile_working_folder_binding(app, &pool, workspace).await
}

pub(crate) async fn authorize_working_folder<R: Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &SqlitePool,
    working_folder_id: &ProjectWorkingFolderId,
    operation: WorkingFolderAuthorizationOperation,
) -> ChatResult<AuthorizedWorkingFolder> {
    let workspace = repository::read_workspace(pool, working_folder_id).await?;
    let workspace = reconcile_working_folder_binding(app, pool, workspace).await?;
    let scope = read_active_working_folder_scope(app).map_err(device_state_error)?;
    authorize_workspace(&workspace, &scope, operation)
}

async fn reconcile_working_folder_binding<R: Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &SqlitePool,
    workspace: ProjectWorkingFolder,
) -> ChatResult<ProjectWorkingFolder> {
    let scope = read_active_working_folder_scope(app).map_err(device_state_error)?;
    let Some(existing_binding) = scope.bindings.get(&workspace.id) else {
        return Ok(workspace);
    };
    let Ok(canonical_path) = std::fs::canonicalize(&existing_binding.canonical_path) else {
        return Ok(workspace);
    };
    if canonical_path.to_str() != Some(existing_binding.canonical_path.as_str()) {
        return Ok(workspace);
    }
    let Ok(current_filesystem_identity) = filesystem_identity(&canonical_path, b"working-folder")
    else {
        return Ok(workspace);
    };
    if existing_binding.filesystem_identity != current_filesystem_identity {
        return Ok(workspace);
    }
    let Ok(probe) = probe_repository(&canonical_path) else {
        return Ok(workspace);
    };
    let repository_transition_identity =
        initialized_repository_identity(&workspace, existing_binding, &probe);

    if let Some(logical_identity) = repository_transition_identity {
        let mut binding = existing_binding.clone();
        binding.repository_kind = probe.kind;
        binding.repository_identity = Some(logical_identity.clone());
        binding.repository_storage_identity = probe.identity.clone();
        binding.last_verified_at = now_timestamp()?;
        store_active_device_binding(app, &workspace.id, binding)?;
        return repository::set_workspace_repository(
            pool,
            &workspace.id,
            probe.kind,
            Some(&logical_identity),
            &now_timestamp()?,
        )
        .await;
    }
    Ok(workspace)
}

#[tauri::command]
pub async fn projects_add_external_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    request: CreateProjectWorkingFolderRequest,
    title: String,
) -> ChatResult<Option<ProjectWorkingFolderRead>> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let Some(selection) = pick_workspace_folder(&app, &title, None).await? else {
        return Ok(None);
    };
    add_external_workspace(&app, &pool, request, &selection)
        .await
        .map(Some)
}

/// Validate an external selection again at Save and resume incomplete creation by stable identity.
async fn add_external_workspace(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    mut request: CreateProjectWorkingFolderRequest,
    selection: &Path,
) -> ChatResult<ProjectWorkingFolderRead> {
    if request.display_name.trim().is_empty() {
        request.display_name = selection
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("External folder")
            .to_string();
    }
    validate_external_folder_outside_vault(app, selection)?;
    ensure_unique_project_path(pool, app, &request.project_id, Some(&request.id), selection)
        .await?;
    let existing = repository::list_workspaces(pool)
        .await?
        .into_iter()
        .find(|workspace| workspace.id == request.id);
    let workspace = if let Some(workspace) = existing {
        if workspace.project_id != request.project_id
            || workspace.kind != WorkingFolderKind::External
            || workspace.archived_at.is_some()
        {
            return Err(ChatError::validation(
                "workingFolderId",
                "Working folder identity belongs to another association",
            ));
        }
        workspace
    } else {
        repository::create_workspace(pool, &request, &now_timestamp()?).await?
    };
    let (probe, binding) = prepare_workspace_binding(&workspace, selection)?;
    let workspace = if probe.kind == ganbaru_chat_contracts::models::RepositoryKind::Git {
        repository::set_workspace_repository(
            pool,
            &workspace.id,
            probe.kind,
            probe.compatibility_identity.as_deref(),
            &now_timestamp()?,
        )
        .await?
    } else {
        workspace
    };
    store_active_device_binding(app, &workspace.id, binding)?;
    read_workspace(app, workspace)
}

#[tauri::command]
pub async fn projects_rename_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    display_name: String,
    expected_revision: u64,
) -> ChatResult<ProjectWorkingFolderRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let workspace = repository::rename_workspace(
        &pool,
        &working_folder_id,
        &display_name,
        expected_revision,
        &now_timestamp()?,
    )
    .await?;
    read_workspace(&app, workspace)
}

#[tauri::command]
pub async fn projects_locate_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    title: String,
) -> ChatResult<Option<ProjectWorkingFolderRead>> {
    pick_and_bind_workspace(&app, &db_url, &working_folder_id, &title).await
}

#[tauri::command]
pub async fn projects_rebind_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    title: String,
) -> ChatResult<Option<ProjectWorkingFolderRead>> {
    pick_and_bind_workspace(&app, &db_url, &working_folder_id, &title).await
}

#[tauri::command]
pub async fn projects_unbind_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
) -> ChatResult<ProjectWorkingFolderRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let workspace = repository::read_workspace(&pool, &working_folder_id).await?;
    if workspace.kind == WorkingFolderKind::Managed {
        return Err(ChatError::validation(
            "workingFolderId",
            "Managed project working folders cannot be unbound",
        ));
    }
    app.state::<crate::chat::terminal::ChatTerminalRegistry>()
        .shutdown_workspace(&working_folder_id)?;
    remove_active_device_binding(&app, &working_folder_id)?;
    read_workspace(&app, workspace)
}

#[tauri::command]
pub async fn projects_archive_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    expected_revision: u64,
) -> ChatResult<ProjectWorkingFolderRead> {
    set_workspace_archived(app, db_url, working_folder_id, expected_revision, true).await
}

#[tauri::command]
pub async fn projects_restore_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    expected_revision: u64,
) -> ChatResult<ProjectWorkingFolderRead> {
    set_workspace_archived(app, db_url, working_folder_id, expected_revision, false).await
}

#[tauri::command]
pub async fn projects_open_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
) -> ChatResult<()> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let authorized = authorize_working_folder(
        &app,
        &pool,
        &working_folder_id,
        WorkingFolderAuthorizationOperation::FileRead,
    )
    .await?;
    open_authorized_workspace(&authorized)
}

#[tauri::command]
pub async fn projects_remove_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
) -> ChatResult<()> {
    let pool = chat_pool(app.clone(), db_url).await?;
    app.state::<crate::chat::terminal::ChatTerminalRegistry>()
        .shutdown_workspace(&working_folder_id)?;
    repository::remove_external_working_folder(&pool, &working_folder_id).await?;
    remove_active_device_binding(&app, &working_folder_id)
}

#[tauri::command]
pub async fn projects_recreate_managed_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
) -> ChatResult<ProjectWorkingFolderRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let workspace = repository::read_workspace(&pool, &working_folder_id).await?;
    if workspace.kind != WorkingFolderKind::Managed {
        return Err(ChatError::validation(
            "workingFolderId",
            "Only managed project working folders can be recreated",
        ));
    }
    remove_active_device_binding(&app, &working_folder_id)?;
    ensure_managed_working_folder_binding(&app, &workspace)?;
    read_workspace(&app, workspace)
}

#[tauri::command]
pub async fn projects_remember_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    project_id: String,
    working_folder_id: ProjectWorkingFolderId,
) -> ChatResult<()> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let folder = repository::read_workspace(&pool, &working_folder_id).await?;
    if folder.project_id != project_id || folder.archived_at.is_some() {
        return Err(ChatError::validation(
            "workingFolderId",
            "The selected working folder is not active in this project",
        ));
    }
    update_active_working_folder_scope(&app, |scope| {
        scope
            .last_selected_by_project
            .insert(project_id, working_folder_id);
        Ok(())
    })
    .map_err(device_state_error)
}

#[tauri::command]
pub fn projects_last_working_folder(
    app: tauri::AppHandle,
    project_id: String,
) -> ChatResult<Option<ProjectWorkingFolderId>> {
    if project_id.trim().is_empty() || project_id.chars().any(char::is_control) {
        return Err(ChatError::validation("projectId", "project ID is invalid"));
    }
    Ok(read_active_working_folder_scope(&app)
        .map_err(device_state_error)?
        .last_selected_by_project
        .get(&project_id)
        .cloned())
}

async fn set_workspace_archived(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    expected_revision: u64,
    archived: bool,
) -> ChatResult<ProjectWorkingFolderRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let existing = repository::read_workspace(&pool, &working_folder_id).await?;
    if existing.kind == WorkingFolderKind::Managed {
        return Err(ChatError::validation(
            "workingFolderId",
            "Managed project working folders cannot be archived",
        ));
    }
    let workspace = repository::set_workspace_archived(
        &pool,
        &working_folder_id,
        archived,
        expected_revision,
        &now_timestamp()?,
    )
    .await?;
    read_workspace(&app, workspace)
}

async fn pick_and_bind_workspace(
    app: &tauri::AppHandle,
    db_url: &str,
    working_folder_id: &ProjectWorkingFolderId,
    title: &str,
) -> ChatResult<Option<ProjectWorkingFolderRead>> {
    let pool = chat_pool(app.clone(), db_url.to_string()).await?;
    let workspace = repository::read_workspace(&pool, working_folder_id).await?;
    if workspace.kind == WorkingFolderKind::Managed {
        return Err(ChatError::validation(
            "workingFolderId",
            "Managed project working folders do not use external bindings",
        ));
    }
    let start_directory = workspace_picker_start_directory(app, working_folder_id);
    let Some(selection) = pick_workspace_folder(app, title, start_directory).await? else {
        return Ok(None);
    };
    bind_workspace(app, &pool, workspace, &selection)
        .await
        .map(Some)
}

/// Share binding validation between immediate workflows and explicit settings Save.
async fn bind_workspace(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    workspace: ProjectWorkingFolder,
    selection: &Path,
) -> ChatResult<ProjectWorkingFolderRead> {
    if workspace.kind != WorkingFolderKind::External || workspace.archived_at.is_some() {
        return Err(ChatError::validation(
            "workingFolderId",
            "Only active external folders can be rebound",
        ));
    }
    let working_folder_id = &workspace.id;
    validate_external_folder_outside_vault(app, selection)?;
    ensure_unique_project_path(
        pool,
        app,
        &workspace.project_id,
        Some(&workspace.id),
        selection,
    )
    .await?;
    app.state::<crate::chat::terminal::ChatTerminalRegistry>()
        .shutdown_workspace(working_folder_id)?;
    let (probe, binding) = prepare_workspace_binding(&workspace, selection)?;
    let workspace = if workspace.repository_kind
        == ganbaru_chat_contracts::models::RepositoryKind::None
        && probe.kind == ganbaru_chat_contracts::models::RepositoryKind::Git
    {
        repository::set_workspace_repository(
            pool,
            working_folder_id,
            probe.kind,
            probe.compatibility_identity.as_deref(),
            &now_timestamp()?,
        )
        .await?
    } else {
        workspace
    };
    store_active_device_binding(app, &workspace.id, binding)?;
    read_workspace(app, workspace)
}

async fn ensure_unique_project_path(
    pool: &SqlitePool,
    app: &tauri::AppHandle,
    project_id: &str,
    except_id: Option<&ProjectWorkingFolderId>,
    selected_path: &Path,
) -> ChatResult<()> {
    let selected = std::fs::canonicalize(selected_path).map_err(|_| {
        ChatError::validation(
            "workingFolderPath",
            "Selected working folder is unavailable",
        )
    })?;
    let scope = read_active_working_folder_scope(app).map_err(device_state_error)?;
    let siblings = repository::list_workspaces(pool).await?;
    let duplicate = siblings.iter().any(|candidate| {
        candidate.project_id == project_id
            && except_id != Some(&candidate.id)
            && scope.bindings.get(&candidate.id).is_some_and(|binding| {
                std::fs::canonicalize(&binding.canonical_path).is_ok_and(|bound| bound == selected)
            })
    });
    if duplicate {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "This folder is already assigned to the selected project",
            true,
        ));
    }
    Ok(())
}

async fn pick_workspace_folder(
    app: &tauri::AppHandle,
    title: &str,
    start_directory: Option<PathBuf>,
) -> ChatResult<Option<PathBuf>> {
    let title = title.trim();
    if title.is_empty() || title.len() > 160 || title.chars().any(char::is_control) {
        return Err(ChatError::validation(
            "title",
            "Working-folder picker title is invalid",
        ));
    }
    let (sender, mut receiver) = tauri::async_runtime::channel(1);
    let mut picker = app.dialog().file().set_title(title);
    if let Some(directory) = start_directory {
        picker = picker.set_directory(directory);
    }
    picker.pick_folder(move |selection| {
        let result = selection.map(dialog_path).transpose();
        let _ = sender.try_send(result);
    });
    receiver
        .recv()
        .await
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Internal,
                "Folder picker closed without a result",
                true,
            )
        })?
        .map_err(|_| {
            ChatError::validation(
                "workingFolderPath",
                "Selected project working folder is not a local folder",
            )
        })
}

fn workspace_picker_start_directory(
    app: &tauri::AppHandle,
    working_folder_id: &ProjectWorkingFolderId,
) -> Option<PathBuf> {
    let bound_path = read_active_working_folder_scope(app)
        .ok()
        .and_then(|scope| scope.bindings.get(working_folder_id).cloned())
        .map(|binding| PathBuf::from(binding.canonical_path));
    preferred_workspace_picker_directory(
        bound_path.as_deref(),
        app.path().document_dir().ok().as_deref(),
    )
}

fn preferred_workspace_picker_directory(
    bound_path: Option<&Path>,
    documents_path: Option<&Path>,
) -> Option<PathBuf> {
    bound_path
        .and_then(|path| path.ancestors().find(|candidate| candidate.is_dir()))
        .or_else(|| documents_path.filter(|path| path.is_dir()))
        .map(Path::to_path_buf)
}

fn read_workspace(
    app: &tauri::AppHandle,
    workspace: super::ProjectWorkingFolder,
) -> ChatResult<ProjectWorkingFolderRead> {
    let scope = read_active_working_folder_scope(app).map_err(device_state_error)?;
    workspace_read(workspace, &scope)
}

async fn chat_pool(app: tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app, db_url)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))
}

fn now_timestamp() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "create Chat timestamp", false))
}

fn dialog_path(path: FilePath) -> Result<PathBuf, String> {
    path.into_path()
        .map_err(|_| "selected path is not local".to_string())
}

fn device_state_error(_error: String) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Project working-folder device state could not be read",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        FolderSelectionReceipt, MAX_PENDING_FOLDER_SELECTIONS, ProjectWorkingFolderSelections,
        preferred_workspace_picker_directory,
    };
    use ganbaru_chat_contracts::models::ProjectWorkingFolderId;
    use std::path::Path;

    #[test]
    fn staged_folder_selections_are_bound_to_the_project_target_and_vault() {
        let selections = ProjectWorkingFolderSelections::default();
        let target = ProjectWorkingFolderId::new("external-folder").unwrap();
        selections
            .insert(FolderSelectionReceipt {
                id: "selection".to_string(),
                project_id: "project".to_string(),
                working_folder_id: Some(target.clone()),
                vault_root: "vault".into(),
                path: "chosen".into(),
                filesystem_identity: "identity".to_string(),
            })
            .unwrap();
        assert!(
            selections
                .read("selection", "project", Some(&target), Path::new("vault"))
                .is_ok()
        );
        assert!(
            selections
                .read("unknown", "project", Some(&target), Path::new("vault"))
                .is_err()
        );
        assert!(
            selections
                .read(
                    "selection",
                    "another-project",
                    Some(&target),
                    Path::new("vault")
                )
                .is_err()
        );
        assert!(
            selections
                .read("selection", "project", None, Path::new("vault"))
                .is_err()
        );
        assert!(
            selections
                .read(
                    "selection",
                    "project",
                    Some(&ProjectWorkingFolderId::new("other").unwrap()),
                    Path::new("vault")
                )
                .is_err()
        );
        assert!(
            selections
                .read(
                    "selection",
                    "project",
                    Some(&target),
                    Path::new("another-vault")
                )
                .is_err()
        );
        assert!(
            selections
                .read("selection", "project", Some(&target), Path::new("vault"))
                .is_ok()
        );
    }

    #[test]
    fn native_selection_receipts_are_bounded_and_oldest_selections_expire() {
        let selections = ProjectWorkingFolderSelections::default();
        for index in 0..=MAX_PENDING_FOLDER_SELECTIONS {
            selections
                .insert(FolderSelectionReceipt {
                    id: index.to_string(),
                    project_id: "project".to_string(),
                    working_folder_id: None,
                    vault_root: "vault".into(),
                    path: "chosen".into(),
                    filesystem_identity: "identity".to_string(),
                })
                .unwrap();
        }
        assert!(
            selections
                .read("0", "project", None, Path::new("vault"))
                .is_err()
        );
        assert!(
            selections
                .read(
                    &MAX_PENDING_FOLDER_SELECTIONS.to_string(),
                    "project",
                    None,
                    Path::new("vault")
                )
                .is_ok()
        );
    }

    #[test]
    fn workspace_picker_prefers_the_existing_bound_folder() {
        let current = std::env::current_dir().expect("read current directory");
        let documents = std::env::temp_dir();

        assert_eq!(
            preferred_workspace_picker_directory(Some(&current), Some(&documents)),
            Some(current),
        );
    }

    #[test]
    fn workspace_picker_falls_back_to_documents_without_a_binding() {
        let documents = std::env::temp_dir();

        assert_eq!(
            preferred_workspace_picker_directory(None, Some(&documents)),
            Some(documents),
        );
        assert_eq!(
            preferred_workspace_picker_directory(None, Some(Path::new("missing-documents"))),
            None,
        );
    }
}
