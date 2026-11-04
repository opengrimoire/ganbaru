//! Tauri command adapters for bounded core workspace-file services.

use super::{AuthorizedWorkingFolder, ExternalOpenConfirmation, resolve_workspace_relative_path};
use crate::db;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId,
};
use serde::Deserialize;
use sqlx::SqlitePool;
use tauri_plugin_dialog::{
    DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};

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

/// Localized text for the native confirmation shown before a file that could run as a program
/// opens. Rust appends the file path itself, so the webview cannot hide what is opened.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalOpenPrompt {
    pub title: String,
    pub message: String,
    pub allow_label: String,
    pub cancel_label: String,
}

/// Upper bounds, in bytes, for the localized title, message, and button labels.
const EXTERNAL_OPEN_PROMPT_LIMITS: (usize, usize, usize) = (160, 1_000, 80);

/// Opens a workspace file with the default handler. Documents open directly; anything that
/// could run as a program asks first in a native dialog naming the file, and declining returns
/// a `cancelled` error.
#[tauri::command]
pub async fn projects_open_working_folder_file(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    relative_path: String,
    execution_environment_id: Option<String>,
    prompt: ExternalOpenPrompt,
) -> ChatResult<()> {
    validate_external_open_prompt(&prompt)?;
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
    if super::external_open_kind(&path)? == super::ExternalOpenKind::Document {
        return super::open_authorized_document(&authorized, &path);
    }
    let confirmation = ask_external_open(&app, &prompt, &relative_path).await?;
    super::open_authorized_path_confirmed(&authorized, &path, confirmation)
}

fn validate_external_open_prompt(prompt: &ExternalOpenPrompt) -> ChatResult<()> {
    let (title_limit, message_limit, label_limit) = EXTERNAL_OPEN_PROMPT_LIMITS;
    let fields = [
        ("title", prompt.title.as_str(), title_limit),
        ("message", prompt.message.as_str(), message_limit),
        ("allowLabel", prompt.allow_label.as_str(), label_limit),
        ("cancelLabel", prompt.cancel_label.as_str(), label_limit),
    ];
    for (field, value, limit) in fields {
        let text = value.trim();
        if text.is_empty() || text.len() > limit || text.chars().any(is_hidden_character) {
            return Err(ChatError::validation(field, "Open prompt text is invalid"));
        }
    }
    if prompt.allow_label.trim() == prompt.cancel_label.trim() {
        return Err(ChatError::validation(
            "allowLabel",
            "Open prompt buttons need distinct labels",
        ));
    }
    Ok(())
}

/// Control characters and bidirectional formatting characters, which can reorder how a file
/// name renders so that `exe` reads as `pdf`.
fn is_hidden_character(character: char) -> bool {
    character.is_control()
        || matches!(character, '\u{200e}' | '\u{200f}' | '\u{061c}')
        || ('\u{202a}'..='\u{202e}').contains(&character)
        || ('\u{2066}'..='\u{2069}').contains(&character)
}

/// The file path as the dialog shows it, with hidden characters made visible as `?`.
fn visible_open_path(relative_path: &str) -> String {
    relative_path
        .chars()
        .map(|character| {
            if is_hidden_character(character) {
                '?'
            } else {
                character
            }
        })
        .collect()
}

async fn ask_external_open(
    app: &tauri::AppHandle,
    prompt: &ExternalOpenPrompt,
    relative_path: &str,
) -> ChatResult<ExternalOpenConfirmation> {
    let allow_label = prompt.allow_label.trim().to_string();
    let (sender, mut receiver) = tauri::async_runtime::channel(1);
    app.dialog()
        .message(format!(
            "{}\n\n{}",
            prompt.message.trim(),
            visible_open_path(relative_path)
        ))
        .title(prompt.title.trim())
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            allow_label.clone(),
            prompt.cancel_label.trim().to_string(),
        ))
        .show_with_result(move |result| {
            let allowed = match result {
                MessageDialogResult::Ok | MessageDialogResult::Yes => true,
                MessageDialogResult::Custom(label) => label == allow_label,
                _ => false,
            };
            let _ = sender.try_send(allowed);
        });
    let allowed = receiver.recv().await.ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::Internal,
            "Open confirmation closed without a result",
            true,
        )
    })?;
    if !allowed {
        return Err(ChatError::new(
            ChatErrorCode::Cancelled,
            "Opening the file was cancelled",
            true,
        ));
    }
    Ok(ExternalOpenConfirmation::from_native_dialog())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt() -> ExternalOpenPrompt {
        ExternalOpenPrompt {
            title: "Open this file?".into(),
            message: "It may run as a program.".into(),
            allow_label: "Open".into(),
            cancel_label: "Cancel".into(),
        }
    }

    #[test]
    fn open_prompts_reject_empty_oversized_hidden_or_ambiguous_text() {
        assert!(validate_external_open_prompt(&prompt()).is_ok());
        let (title_limit, ..) = EXTERNAL_OPEN_PROMPT_LIMITS;
        let invalid = [
            ExternalOpenPrompt {
                title: " ".into(),
                ..prompt()
            },
            ExternalOpenPrompt {
                title: "x".repeat(title_limit + 1),
                ..prompt()
            },
            ExternalOpenPrompt {
                message: "Safe\u{202e}fdp.exe".into(),
                ..prompt()
            },
            ExternalOpenPrompt {
                allow_label: "Op\nen".into(),
                ..prompt()
            },
            ExternalOpenPrompt {
                cancel_label: "Open".into(),
                ..prompt()
            },
        ];
        for prompt in invalid {
            assert!(
                validate_external_open_prompt(&prompt).is_err(),
                "{prompt:?}"
            );
        }
    }

    #[test]
    fn the_dialog_path_shows_bidirectional_and_control_characters() {
        assert_eq!(
            visible_open_path("docs/report\u{202e}fdp.exe"),
            "docs/report?fdp.exe"
        );
        assert_eq!(visible_open_path("a\u{2066}b\u{1b}c"), "a?b?c");
        assert_eq!(visible_open_path("src/main.rs"), "src/main.rs");
    }
}
