//! Authorized Chat-facing integration for working-folder Markdown files.

use crate::chat::workspace::commands::authorize_working_folder;
use crate::chat::workspace::{
    AuthorizedWorkingFolder, WorkingFolderAuthorizationOperation,
    ensure_managed_working_folder_binding, open_authorized_path,
};
use crate::db::connect_sqlite;
use ganbaru_chat::repository::workspaces;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId,
};
use ganbaru_notes::working_markdown::{
    self, WorkingMarkdownError, WorkingMarkdownFile, WorkingMarkdownNode, WorkingMarkdownNodeKind,
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
        let scan = working_markdown::scan_markdown(&authorized.canonical_path)
            .map_err(working_markdown_error)?;
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
    let authorized = require_folder(
        &app,
        db_url,
        &working_folder_id,
        WorkingFolderAuthorizationOperation::FileRead,
    )
    .await?;
    let file = working_markdown::read_markdown(&authorized.canonical_path, &relative_path)
        .map_err(working_markdown_error)?;
    Ok(markdown_file_dto(working_folder_id, file))
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
    let authorized = require_folder(
        &app,
        db_url,
        &working_folder_id,
        WorkingFolderAuthorizationOperation::FileWrite,
    )
    .await?;
    let file = working_markdown::save_markdown(
        &authorized.canonical_path,
        &relative_path,
        &content,
        &expected_revision,
    )
    .map_err(working_markdown_error)?;
    Ok(markdown_file_dto(working_folder_id, file))
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
    let path = working_markdown::resolve_markdown_file(&authorized.canonical_path, &relative_path)
        .map_err(working_markdown_error)?;
    open_authorized_path(&authorized, &path)
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

fn markdown_file_dto(
    working_folder_id: ProjectWorkingFolderId,
    file: WorkingMarkdownFile,
) -> NotesWorkingMarkdownFileRead {
    NotesWorkingMarkdownFileRead {
        working_folder_id,
        relative_path: file.relative_path,
        content: file.content,
        revision: file.revision,
        byte_size: file.byte_size,
    }
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
        WorkingMarkdownError::StaleRevision => ChatError::new(
            ChatErrorCode::StaleRevision,
            "The Markdown file changed outside Ganbaru AI",
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
