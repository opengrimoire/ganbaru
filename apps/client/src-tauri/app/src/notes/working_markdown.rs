//! Authorized Chat-facing integration for working-folder Markdown files.

use crate::chat::workspace::commands::authorize_working_folder;
use crate::chat::workspace::files::{
    ProjectWorkingFolderFilePreview, preview_workspace_file_within, save_workspace_file_within,
};
use crate::chat::workspace::{
    AuthorizedWorkingFolder, WorkingFolderAuthorizationOperation,
    ensure_managed_working_folder_binding, open_authorized_document,
};
use crate::db::connect_sqlite;
use ganbaru_chat::repository::workspaces;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId,
};
use ganbaru_notes::working_markdown::{
    self, MAX_MARKDOWN_FILE_BYTES, WorkingMarkdownError, WorkingMarkdownNode,
    WorkingMarkdownNodeKind,
};
use serde::Serialize;
use tauri::Runtime;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotesWorkingMarkdownNodeKind {
    Directory,
    File,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesWorkingMarkdownNode {
    pub kind: NotesWorkingMarkdownNodeKind,
    pub name: String,
    pub relative_path: String,
    pub children: Vec<NotesWorkingMarkdownNode>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesWorkingMarkdownRoot {
    pub working_folder_id: ProjectWorkingFolderId,
    pub display_name: String,
    pub source_kind: String,
    pub display_path: String,
    pub nodes: Vec<NotesWorkingMarkdownNode>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesWorkingMarkdownTreeRead {
    pub roots: Vec<NotesWorkingMarkdownRoot>,
    pub unavailable_working_folder_ids: Vec<ProjectWorkingFolderId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesWorkingMarkdownFileRead {
    pub working_folder_id: ProjectWorkingFolderId,
    pub relative_path: String,
    pub content: String,
    pub revision: String,
    pub byte_size: u64,
}

#[tauri::command]
pub async fn notes_list_working_markdown<R: Runtime>(
    app: tauri::AppHandle<R>,
    db_url: String,
    project_id: String,
) -> ChatResult<NotesWorkingMarkdownTreeRead> {
    validate_project_id(&project_id)?;
    let pool = connect_sqlite(app.clone(), db_url)
        .await
        .map_err(persistence_error)?;
    let folders = workspaces::list_workspaces(&pool).await?;
    for folder in folders
        .iter()
        .filter(|folder| folder.project_id == project_id && folder.archived_at.is_none())
    {
        ensure_managed_working_folder_binding(&app, folder)?;
    }
    let mut roots = Vec::new();
    let mut unavailable = Vec::new();
    for folder in folders
        .into_iter()
        .filter(|folder| folder.project_id == project_id && folder.archived_at.is_none())
    {
        let authorized = match authorize_working_folder(
            &app,
            &pool,
            &folder.id,
            WorkingFolderAuthorizationOperation::FileRead,
        )
        .await
        {
            Ok(authorized) => authorized,
            Err(_) => {
                unavailable.push(folder.id);
                continue;
            }
        };
        let root = authorized.canonical_path.clone();
        let scan = on_notes_worker(move || {
            working_markdown::scan_markdown(&root).map_err(working_markdown_error)
        })
        .await?;
        if scan.nodes.is_empty() {
            continue;
        }
        roots.push(NotesWorkingMarkdownRoot {
            working_folder_id: folder.id,
            display_name: folder.display_name,
            source_kind: match folder.kind {
                crate::chat::workspace::WorkingFolderKind::Managed => "managed".to_string(),
                crate::chat::workspace::WorkingFolderKind::External => "external".to_string(),
            },
            display_path: folder
                .managed_relative_path
                .unwrap_or_else(|| authorized.canonical_path.to_string_lossy().into_owned()),
            nodes: scan.nodes.into_iter().map(markdown_node_dto).collect(),
            truncated: scan.truncated,
        });
    }
    Ok(NotesWorkingMarkdownTreeRead {
        roots,
        unavailable_working_folder_ids: unavailable,
    })
}

#[tauri::command]
pub async fn notes_read_working_markdown<R: Runtime>(
    app: tauri::AppHandle<R>,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    relative_path: String,
) -> ChatResult<NotesWorkingMarkdownFileRead> {
    let relative_path =
        working_markdown::validate_markdown_path(&relative_path).map_err(working_markdown_error)?;
    let authorized = require_folder(
        &app,
        db_url,
        &working_folder_id,
        WorkingFolderAuthorizationOperation::FileRead,
    )
    .await?;
    let preview = on_notes_worker(move || {
        preview_workspace_file_within(&authorized, &relative_path, MAX_MARKDOWN_FILE_BYTES)
    })
    .await?;
    markdown_file_dto(working_folder_id, preview)
}

#[tauri::command]
pub async fn notes_save_working_markdown<R: Runtime>(
    app: tauri::AppHandle<R>,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    relative_path: String,
    content: String,
    expected_revision: String,
) -> ChatResult<NotesWorkingMarkdownFileRead> {
    let relative_path =
        working_markdown::validate_markdown_path(&relative_path).map_err(working_markdown_error)?;
    if content.len() as u64 > MAX_MARKDOWN_FILE_BYTES {
        return Err(working_markdown_error(WorkingMarkdownError::TooLarge));
    }
    let authorized = require_folder(
        &app,
        db_url,
        &working_folder_id,
        WorkingFolderAuthorizationOperation::FileWrite,
    )
    .await?;
    let saved = on_notes_worker(move || {
        save_workspace_file_within(
            &authorized,
            &relative_path,
            &content,
            &expected_revision,
            MAX_MARKDOWN_FILE_BYTES,
        )
    })
    .await?;
    markdown_file_dto(working_folder_id, saved)
}

#[tauri::command]
pub async fn notes_open_working_markdown<R: Runtime>(
    app: tauri::AppHandle<R>,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    relative_path: String,
) -> ChatResult<()> {
    let authorized = require_folder(
        &app,
        db_url,
        &working_folder_id,
        WorkingFolderAuthorizationOperation::FileRead,
    )
    .await?;
    let root = authorized.canonical_path.clone();
    let path = on_notes_worker(move || {
        working_markdown::resolve_markdown_file(&root, &relative_path)
            .map_err(working_markdown_error)
    })
    .await?;
    open_authorized_document(&authorized, &path)
}

/// Runs one working-Markdown filesystem operation on a Notes file worker.
async fn on_notes_worker<T, F>(work: F) -> ChatResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> ChatResult<T> + Send + 'static,
{
    ganbaru_notes::blocking::run_blocking(move || Ok(work()))
        .await
        .map_err(|_| {
            ChatError::new(
                ChatErrorCode::Internal,
                "The Markdown file worker stopped unexpectedly",
                true,
            )
        })?
}

async fn require_folder<R: Runtime>(
    app: &tauri::AppHandle<R>,
    db_url: String,
    working_folder_id: &ProjectWorkingFolderId,
    operation: WorkingFolderAuthorizationOperation,
) -> ChatResult<AuthorizedWorkingFolder> {
    let pool = connect_sqlite(app.clone(), db_url)
        .await
        .map_err(persistence_error)?;
    let folder = workspaces::read_workspace(&pool, working_folder_id).await?;
    if folder.archived_at.is_some() {
        return Err(ChatError::validation(
            "workingFolderId",
            "Archived project working folders cannot be edited",
        ));
    }
    ensure_managed_working_folder_binding(app, &folder)?;
    authorize_working_folder(app, &pool, working_folder_id, operation).await
}

fn markdown_node_dto(node: WorkingMarkdownNode) -> NotesWorkingMarkdownNode {
    NotesWorkingMarkdownNode {
        kind: match node.kind {
            WorkingMarkdownNodeKind::Directory => NotesWorkingMarkdownNodeKind::Directory,
            WorkingMarkdownNodeKind::File => NotesWorkingMarkdownNodeKind::File,
        },
        name: node.name,
        relative_path: node.relative_path,
        children: node.children.into_iter().map(markdown_node_dto).collect(),
    }
}

/// Projects a bounded workspace preview as editable Markdown, refusing files
/// over the Markdown limit and files that are not UTF-8 text.
fn markdown_file_dto(
    working_folder_id: ProjectWorkingFolderId,
    preview: ProjectWorkingFolderFilePreview,
) -> ChatResult<NotesWorkingMarkdownFileRead> {
    if preview.oversized {
        return Err(working_markdown_error(WorkingMarkdownError::TooLarge));
    }
    let (Some(content), Some(revision)) = (preview.text, preview.content_revision) else {
        return Err(working_markdown_error(WorkingMarkdownError::InvalidUtf8));
    };
    Ok(NotesWorkingMarkdownFileRead {
        working_folder_id,
        relative_path: preview.relative_path,
        content,
        revision,
        byte_size: preview.byte_size,
    })
}

fn validate_project_id(project_id: &str) -> ChatResult<()> {
    if project_id.trim().is_empty()
        || project_id.len() > 1_024
        || project_id.chars().any(char::is_control)
    {
        return Err(ChatError::validation("projectId", "project ID is invalid"));
    }
    Ok(())
}

fn working_markdown_error(error: WorkingMarkdownError) -> ChatError {
    match error {
        WorkingMarkdownError::InvalidPath => {
            ChatError::validation("relativePath", "Markdown relative path is invalid")
        }
        WorkingMarkdownError::InvalidExtension => ChatError::validation(
            "relativePath",
            "Only Markdown files can be opened in project Notes",
        ),
        WorkingMarkdownError::InvalidUtf8 => {
            ChatError::validation("relativePath", "Markdown file must contain UTF-8 text")
        }
        WorkingMarkdownError::TooLarge => ChatError::validation(
            "relativePath",
            "Markdown files larger than 4 MiB are not available in project Notes",
        ),
        WorkingMarkdownError::NotFile => {
            ChatError::validation("relativePath", "Markdown path must identify a file")
        }
        WorkingMarkdownError::OutsideRoot => ChatError::new(
            ChatErrorCode::Permission,
            "Markdown path resolves outside the working folder",
            false,
        ),
        WorkingMarkdownError::SymbolicLink => ChatError::new(
            ChatErrorCode::Permission,
            "Symbolic links are not available in project Notes",
            false,
        ),
        WorkingMarkdownError::Io => ChatError::new(
            ChatErrorCode::Permission,
            "The Markdown file or directory could not be accessed",
            true,
        ),
    }
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Project working-folder persistence failed",
        true,
    )
}
