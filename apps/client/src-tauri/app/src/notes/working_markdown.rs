//! Authorized Chat-facing integration for working-folder Markdown files.

use crate::chat::workspace::commands::{
    authorize_working_folder, bind_managed_folders_for_listing,
};
use crate::chat::workspace::files::{
    ProjectWorkingFolderFilePreview, preview_workspace_file_within, save_workspace_file_within,
};
use crate::chat::workspace::{
    AuthorizedWorkingFolder, ProjectWorkingFolder, WorkingFolderAuthorizationOperation,
    WorkingFolderKind, ensure_managed_working_folder_binding, open_authorized_document,
};
use crate::db::connect_sqlite;
use ganbaru_chat::repository::workspaces;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId,
};
use ganbaru_notes::working_markdown::{
    self, MAX_MARKDOWN_FILE_BYTES, WorkingMarkdownError, WorkingMarkdownNode,
    WorkingMarkdownNodeKind, WorkingMarkdownScan,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
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
    bind_managed_folders_for_listing(
        &app,
        folders
            .iter()
            .filter(|folder| folder.project_id == project_id && folder.archived_at.is_none()),
    );
    let mut scans = Vec::new();
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
                scans.push(FolderMarkdownScan::Unauthorized(folder));
                continue;
            }
        };
        let root = authorized.canonical_path.clone();
        let scan = on_notes_worker(move || Ok(working_markdown::scan_markdown(&root))).await?;
        scans.push(FolderMarkdownScan::Scanned {
            folder,
            canonical_path: authorized.canonical_path,
            scan,
        });
    }
    markdown_tree(scans)
}

/// The outcome of authorizing and scanning one active working folder.
enum FolderMarkdownScan {
    Unauthorized(ProjectWorkingFolder),
    Scanned {
        folder: ProjectWorkingFolder,
        canonical_path: PathBuf,
        scan: Result<WorkingMarkdownScan, WorkingMarkdownError>,
    },
}

/// Builds the working Markdown tree from per-folder scans.
fn markdown_tree(scans: Vec<FolderMarkdownScan>) -> ChatResult<NotesWorkingMarkdownTreeRead> {
    let mut roots = Vec::new();
    let mut unavailable = Vec::new();
    for entry in scans {
        let (folder, canonical_path, scan) = match entry {
            FolderMarkdownScan::Unauthorized(folder) => {
                unavailable.push(folder.id);
                continue;
            }
            FolderMarkdownScan::Scanned {
                folder,
                canonical_path,
                scan,
            } => (folder, canonical_path, scan),
        };
        let scan = scan.map_err(working_markdown_error)?;
        if scan.nodes.is_empty() {
            continue;
        }
        roots.push(markdown_root(folder, &canonical_path, scan));
    }
    Ok(NotesWorkingMarkdownTreeRead {
        roots,
        unavailable_working_folder_ids: unavailable,
    })
}

fn markdown_root(
    folder: ProjectWorkingFolder,
    canonical_path: &Path,
    scan: WorkingMarkdownScan,
) -> NotesWorkingMarkdownRoot {
    NotesWorkingMarkdownRoot {
        working_folder_id: folder.id,
        display_name: folder.display_name,
        source_kind: match folder.kind {
            WorkingFolderKind::Managed => "managed".to_string(),
            WorkingFolderKind::External => "external".to_string(),
        },
        display_path: folder
            .managed_relative_path
            .unwrap_or_else(|| canonical_path.to_string_lossy().into_owned()),
        nodes: scan.nodes.into_iter().map(markdown_node_dto).collect(),
        truncated: scan.truncated,
    }
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

#[cfg(test)]
mod tests {
    use crate::chat::workspace::commands::bind_active_folders;
    use crate::chat::workspace::{ProjectWorkingFolder, WorkingFolderKind};
    use ganbaru_chat_contracts::models::{ChatError, ProjectWorkingFolderId, UtcTimestamp};
    use ganbaru_working_folders::RepositoryKind;
    use std::cell::RefCell;

    fn managed_folder(id: &str, path: &str) -> ProjectWorkingFolder {
        let timestamp = UtcTimestamp::new("2027-01-02T00:00:00.000Z").unwrap();
        ProjectWorkingFolder {
            id: ProjectWorkingFolderId::new(id).unwrap(),
            project_id: "project".to_string(),
            display_name: id.to_string(),
            kind: WorkingFolderKind::Managed,
            managed_relative_path: Some(path.to_string()),
            sort_order: 0,
            repository_kind: RepositoryKind::None,
            repository_identity: None,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            archived_at: None,
            revision: 1,
        }
    }

    #[test]
    fn working_markdown_binding_failure_leaves_other_folders() {
        let folders = [
            managed_folder("working-folder:unbound", "x"),
            managed_folder("working-folder:project", "projects/project"),
            managed_folder("working-folder:extra", "projects/project"),
        ];
        let bound = RefCell::new(Vec::new());

        bind_active_folders(&folders, |folder| {
            if folder.managed_relative_path.as_deref() == Some("x") {
                return Err(ChatError::validation("workingFolderPath", "unbound path"));
            }
            bound.borrow_mut().push(folder.id.as_str().to_string());
            Ok(())
        });

        assert_eq!(
            bound.into_inner(),
            ["working-folder:project", "working-folder:extra"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_folder_is_unavailable_and_other_folders_stay() {
        use super::{FolderMarkdownScan, markdown_tree};
        use ganbaru_notes::working_markdown::scan_markdown;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let base = std::env::temp_dir().join(format!(
            "ganbaru-app-working-markdown-unreadable-{}",
            std::process::id()
        ));
        let readable = base.join("readable");
        let broken = base.join("broken");
        let locked = broken.join("locked");
        fs::create_dir_all(&readable).unwrap();
        fs::create_dir_all(&locked).unwrap();
        fs::write(readable.join("plan.md"), "# Plan").unwrap();
        fs::write(broken.join("top.md"), "# Top").unwrap();
        fs::write(locked.join("hidden.md"), "# Hidden").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let locked_is_unreadable = fs::read_dir(&locked).is_err();

        let scans = vec![
            FolderMarkdownScan::Scanned {
                folder: managed_folder("working-folder:broken", "projects/broken"),
                canonical_path: broken.clone(),
                scan: scan_markdown(&broken),
            },
            FolderMarkdownScan::Scanned {
                folder: managed_folder("working-folder:readable", "projects/readable"),
                canonical_path: readable.clone(),
                scan: scan_markdown(&readable),
            },
        ];
        let tree = markdown_tree(scans);

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        fs::remove_dir_all(&base).unwrap();
        if !locked_is_unreadable {
            eprintln!("skipped: the process can read a directory with no permissions");
            return;
        }
        assert!(
            tree.is_ok(),
            "one unreadable folder failed the whole working Markdown tree"
        );
        let tree = tree.unwrap();
        let roots: Vec<&str> = tree
            .roots
            .iter()
            .map(|root| root.working_folder_id.as_str())
            .collect();
        assert_eq!(roots, ["working-folder:readable"]);
        let unavailable: Vec<&str> = tree
            .unavailable_working_folder_ids
            .iter()
            .map(ProjectWorkingFolderId::as_str)
            .collect();
        assert_eq!(unavailable, ["working-folder:broken"]);
    }
}
