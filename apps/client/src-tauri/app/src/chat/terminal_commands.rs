//! Authorized commands for runtime terminals.

use super::terminal::{
    ChatTerminalCloseResult, ChatTerminalCreateInput, ChatTerminalId, ChatTerminalOutputChunk,
    ChatTerminalRead, ChatTerminalRegistry, ChatTerminalSnapshotRead, default_shell_label,
};
use super::terminal_access::{ChatTerminalAccess, TerminalStartScope, reservation_mismatch_error};
use super::workspace::commands::authorize_working_folder;
use super::workspace::{AuthorizedWorkingFolder, WorkingFolderAuthorizationOperation};
use crate::db;
use chrono::{SecondsFormat, Utc};
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ChatThreadId, ProjectWorkingFolderId, UtcTimestamp,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Row, SqlitePool};
use std::future::Future;
use std::path::Path;
use tauri::Manager;
use tauri::ipc::Channel;
use tauri_plugin_dialog::{
    DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChatTerminalRequest {
    pub terminal_id: ChatTerminalId,
    pub thread_id: ChatThreadId,
    pub working_folder_id: ProjectWorkingFolderId,
    pub columns: u16,
    pub rows: u16,
    pub start_prompt: TerminalStartPrompt,
}

/// Localized text for the native terminal start confirmation. Rust appends the
/// folder path and shell itself, so the webview cannot hide what is approved.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalStartPrompt {
    pub title: String,
    pub message: String,
    pub allow_label: String,
    pub cancel_label: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTerminalInputRequest {
    pub terminal_id: ChatTerminalId,
    pub thread_id: ChatThreadId,
    pub working_folder_id: ProjectWorkingFolderId,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTerminalResizeRequest {
    pub terminal_id: ChatTerminalId,
    pub thread_id: ChatThreadId,
    pub working_folder_id: ProjectWorkingFolderId,
    pub columns: u16,
    pub rows: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTerminalPanelLayout {
    pub placement: String,
    pub terminal_names: Vec<String>,
    pub selected_index: Option<u64>,
    pub split_direction: String,
    pub split_sizes: Vec<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTerminalLayoutRead {
    pub thread_id: ChatThreadId,
    pub schema_version: u32,
    pub groups: Vec<ChatTerminalPanelLayout>,
    pub updated_at: Option<UtcTimestamp>,
}

#[tauri::command]
pub async fn chat_list_terminals(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    working_folder_id: ProjectWorkingFolderId,
) -> ChatResult<Vec<ChatTerminalRead>> {
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_thread_workspace(&app, &pool, &thread_id, &working_folder_id).await?;
    app.state::<ChatTerminalRegistry>()
        .list(&thread_id, &working_folder_id)
}

#[tauri::command]
pub async fn chat_read_terminal_layout(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    working_folder_id: ProjectWorkingFolderId,
) -> ChatResult<ChatTerminalLayoutRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_thread_workspace(&app, &pool, &thread_id, &working_folder_id).await?;
    let row = sqlx::query(
        "SELECT layout_schema_version, layout_data, updated_at
         FROM chat_terminal_layouts WHERE thread_id = ?",
    )
    .bind(thread_id.as_str())
    .fetch_optional(&pool)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(ChatTerminalLayoutRead {
            thread_id,
            schema_version: 1,
            groups: Vec::new(),
            updated_at: None,
        });
    };
    let schema_version = u32::try_from(
        row.try_get::<i64, _>("layout_schema_version")
            .map_err(persistence_error)?,
    )
    .map_err(|_| persistence_error("invalid terminal layout schema"))?;
    let value: serde_json::Value = serde_json::from_str(
        &row.try_get::<String, _>("layout_data")
            .map_err(persistence_error)?,
    )
    .map_err(persistence_error)?;
    let groups: Vec<ChatTerminalPanelLayout> =
        serde_json::from_value(value.get("groups").cloned().unwrap_or_else(|| json!([])))
            .map_err(persistence_error)?;
    validate_terminal_layout_groups(&groups)?;
    Ok(ChatTerminalLayoutRead {
        thread_id,
        schema_version,
        groups,
        updated_at: Some(
            UtcTimestamp::new(
                row.try_get::<String, _>("updated_at")
                    .map_err(persistence_error)?,
            )
            .map_err(persistence_error)?,
        ),
    })
}

#[tauri::command]
pub async fn chat_save_terminal_panel_layout(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    working_folder_id: ProjectWorkingFolderId,
    panel: ChatTerminalPanelLayout,
) -> ChatResult<ChatTerminalLayoutRead> {
    validate_terminal_layout_groups(std::slice::from_ref(&panel))?;
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_thread_workspace(&app, &pool, &thread_id, &working_folder_id).await?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let existing: Option<String> =
        sqlx::query_scalar("SELECT layout_data FROM chat_terminal_layouts WHERE thread_id = ?")
            .bind(thread_id.as_str())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(persistence_error)?;
    let mut groups = existing
        .map(|data| -> ChatResult<Vec<ChatTerminalPanelLayout>> {
            let value: serde_json::Value =
                serde_json::from_str(&data).map_err(persistence_error)?;
            serde_json::from_value(value.get("groups").cloned().unwrap_or_else(|| json!([])))
                .map_err(persistence_error)
        })
        .transpose()?
        .unwrap_or_default();
    validate_terminal_layout_groups(&groups)?;
    groups.retain(|group| group.placement != panel.placement);
    groups.push(panel);
    groups.sort_by(|left, right| left.placement.cmp(&right.placement));
    let now = now_timestamp()?;
    let data = serde_json::to_string(&json!({ "groups": groups })).map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_terminal_layouts
            (thread_id, layout_schema_version, layout_data, updated_at)
         VALUES (?, 1, ?, ?)
         ON CONFLICT(thread_id) DO UPDATE SET
            layout_schema_version = excluded.layout_schema_version,
            layout_data = excluded.layout_data,
            updated_at = excluded.updated_at",
    )
    .bind(thread_id.as_str())
    .bind(data)
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(ChatTerminalLayoutRead {
        thread_id,
        schema_version: 1,
        groups,
        updated_at: Some(now),
    })
}

fn validate_terminal_layout_groups(groups: &[ChatTerminalPanelLayout]) -> ChatResult<()> {
    if groups.len() > 2 {
        return Err(ChatError::validation(
            "groups",
            "Terminal layout has too many groups",
        ));
    }
    for group in groups {
        if !matches!(group.placement.as_str(), "inspector" | "bottom")
            || !matches!(group.split_direction.as_str(), "horizontal" | "vertical")
            || group.terminal_names.len() > 8
            || group.terminal_names.iter().any(|name| {
                name.trim().is_empty() || name.len() > 100 || name.chars().any(char::is_control)
            })
            || (!group.split_sizes.is_empty()
                && group.split_sizes.len() != group.terminal_names.len())
            || group
                .split_sizes
                .iter()
                .any(|size| *size == 0 || *size > 10_000)
            || group
                .selected_index
                .is_some_and(|index| index as usize >= group.terminal_names.len())
        {
            return Err(ChatError::validation(
                "layout",
                "Terminal layout is invalid",
            ));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn chat_terminal_create(
    app: tauri::AppHandle,
    db_url: String,
    request: CreateChatTerminalRequest,
) -> ChatResult<ChatTerminalSnapshotRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let scope =
        authorize_thread_scope(&app, &pool, &request.thread_id, &request.working_folder_id).await?;
    let folder =
        ganbaru_chat::repository::workspaces::read_workspace(&pool, &request.working_folder_id)
            .await?;
    validate_terminal_start(scope.thread_archived, folder.archived_at.is_some())?;
    let shell = default_shell_label();
    let path = scope.authorized.canonical_path.clone();
    ensure_start_grant(
        &app.state::<ChatTerminalAccess>(),
        scope.start_scope(),
        || ask_terminal_start(&app, &request.start_prompt, &path, &shell),
    )
    .await?;
    let authorized = scope.authorized;
    app.state::<ChatTerminalRegistry>().create(
        app.clone(),
        ChatTerminalCreateInput {
            terminal_id: request.terminal_id.into_inner(),
            thread_id: request.thread_id,
            working_folder_id: request.working_folder_id,
            environment_id: scope.environment_id,
            workspace_path: authorized.canonical_path,
            columns: request.columns,
            rows: request.rows,
        },
    )
}

#[tauri::command]
pub async fn chat_terminal_snapshot(
    app: tauri::AppHandle,
    db_url: String,
    terminal_id: ChatTerminalId,
    thread_id: ChatThreadId,
    working_folder_id: ProjectWorkingFolderId,
) -> ChatResult<ChatTerminalSnapshotRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_thread_workspace(&app, &pool, &thread_id, &working_folder_id).await?;
    let registry = app.state::<ChatTerminalRegistry>();
    registry.require_scope(terminal_id.as_str(), &thread_id, &working_folder_id)?;
    registry.snapshot(terminal_id.as_str())
}

/// Subscribes the calling view to live output through its own channel and returns the replay to render first.
#[tauri::command]
pub async fn chat_terminal_attach(
    app: tauri::AppHandle,
    db_url: String,
    terminal_id: ChatTerminalId,
    thread_id: ChatThreadId,
    working_folder_id: ProjectWorkingFolderId,
    subscription_id: String,
    channel: Channel<ChatTerminalOutputChunk>,
) -> ChatResult<ChatTerminalSnapshotRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_thread_workspace(&app, &pool, &thread_id, &working_folder_id).await?;
    let registry = app.state::<ChatTerminalRegistry>();
    registry.require_scope(terminal_id.as_str(), &thread_id, &working_folder_id)?;
    registry.attach(terminal_id.as_str(), subscription_id, Box::new(channel))
}

/// Flow control for an existing subscription. The random subscription ID, issued to the attached view, is the
/// capability, so these frequent calls skip the database authorization that attaching already passed.
#[tauri::command]
pub fn chat_terminal_acknowledge(
    app: tauri::AppHandle,
    terminal_id: ChatTerminalId,
    subscription_id: String,
    bytes: u64,
) -> ChatResult<()> {
    app.state::<ChatTerminalRegistry>()
        .acknowledge(terminal_id.as_str(), &subscription_id, bytes)
}

#[tauri::command]
pub fn chat_terminal_detach(
    app: tauri::AppHandle,
    terminal_id: ChatTerminalId,
    subscription_id: String,
) -> ChatResult<()> {
    app.state::<ChatTerminalRegistry>()
        .detach(terminal_id.as_str(), &subscription_id)
}

#[tauri::command]
pub async fn chat_terminal_input(
    app: tauri::AppHandle,
    db_url: String,
    request: ChatTerminalInputRequest,
) -> ChatResult<()> {
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_thread_workspace(&app, &pool, &request.thread_id, &request.working_folder_id).await?;
    let registry = app.state::<ChatTerminalRegistry>();
    registry.require_scope(
        request.terminal_id.as_str(),
        &request.thread_id,
        &request.working_folder_id,
    )?;
    registry.input(request.terminal_id.as_str(), &request.text)
}

#[tauri::command]
pub async fn chat_terminal_resize(
    app: tauri::AppHandle,
    db_url: String,
    request: ChatTerminalResizeRequest,
) -> ChatResult<()> {
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_thread_workspace(&app, &pool, &request.thread_id, &request.working_folder_id).await?;
    let registry = app.state::<ChatTerminalRegistry>();
    registry.require_scope(
        request.terminal_id.as_str(),
        &request.thread_id,
        &request.working_folder_id,
    )?;
    registry.resize(request.terminal_id.as_str(), request.columns, request.rows)
}

#[tauri::command]
pub async fn chat_terminal_close(
    app: tauri::AppHandle,
    db_url: String,
    terminal_id: ChatTerminalId,
    thread_id: ChatThreadId,
    working_folder_id: ProjectWorkingFolderId,
    confirmed: bool,
) -> ChatResult<ChatTerminalCloseResult> {
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_thread_workspace(&app, &pool, &thread_id, &working_folder_id).await?;
    let registry = app.state::<ChatTerminalRegistry>();
    registry.require_scope(terminal_id.as_str(), &thread_id, &working_folder_id)?;
    registry.close(terminal_id.as_str(), confirmed)
}

/// Binds an unsaved draft thread to its working folder so it may open terminals.
/// A persisted thread needs no reservation and is only checked against the folder.
#[tauri::command]
pub async fn chat_reserve_draft_thread(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    working_folder_id: ProjectWorkingFolderId,
) -> ChatResult<()> {
    let pool = chat_pool(app.clone(), db_url).await?;
    if let Some(stored) = read_thread_scope(&pool, &thread_id).await? {
        return validate_terminal_thread_scope(Some(&stored), None, &working_folder_id);
    }
    authorize_working_folder(
        &app,
        &pool,
        &working_folder_id,
        WorkingFolderAuthorizationOperation::TerminalStart,
    )
    .await?;
    let live_threads = app.state::<ChatTerminalRegistry>().thread_ids()?;
    app.state::<ChatTerminalAccess>().reserve(
        &active_vault_id(&app)?,
        &thread_id,
        &working_folder_id,
        |thread| live_threads.contains(thread),
    )
}

/// A thread's authorized terminal workspace and the grant key that covers it.
struct ThreadTerminalScope {
    vault_id: String,
    environment_id: Option<String>,
    thread_archived: bool,
    authorized: AuthorizedWorkingFolder,
}

impl ThreadTerminalScope {
    fn start_scope(&self) -> TerminalStartScope {
        TerminalStartScope::new(
            self.vault_id.clone(),
            self.authorized.working_folder_id.clone(),
            self.environment_id.as_deref(),
        )
    }
}

/// Runs `ask` only when this launch holds no start grant for `scope`, and
/// records the grant when the user allows it. The prompt lock keeps concurrent
/// starts for one folder from showing more than one dialog.
async fn ensure_start_grant<F, Fut>(
    access: &ChatTerminalAccess,
    scope: TerminalStartScope,
    ask: F,
) -> ChatResult<()>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ChatResult<bool>>,
{
    let _prompt = access.lock_prompt().await;
    if access.is_granted(&scope)? {
        return Ok(());
    }
    if !ask().await? {
        return Err(ChatError::new(
            ChatErrorCode::Cancelled,
            "Terminal start was not allowed",
            true,
        ));
    }
    access.grant(scope)
}

/// Shows the native terminal start confirmation naming the folder and shell.
async fn ask_terminal_start(
    app: &tauri::AppHandle,
    prompt: &TerminalStartPrompt,
    folder: &Path,
    shell: &str,
) -> ChatResult<bool> {
    let allow_label = prompt.allow_label.trim().to_string();
    let (sender, mut receiver) = tauri::async_runtime::channel(1);
    app.dialog()
        .message(format!(
            "{}\n\n{}\n{}",
            prompt.message.trim(),
            folder.to_string_lossy(),
            shell
                .chars()
                .filter(|c| !c.is_control())
                .collect::<String>(),
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
    receiver.recv().await.ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::Internal,
            "Terminal confirmation closed without a result",
            true,
        )
    })
}

/// Authorizes a persisted or reserved thread in its working folder for terminal access on this device.
pub(crate) async fn authorize_thread_workspace(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatResult<AuthorizedWorkingFolder> {
    Ok(
        authorize_thread_scope(app, pool, thread_id, working_folder_id)
            .await?
            .authorized,
    )
}

async fn authorize_thread_scope(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatResult<ThreadTerminalScope> {
    let vault_id = active_vault_id(app)?;
    let access = app.state::<ChatTerminalAccess>();
    let stored_scope = read_thread_scope(pool, thread_id).await?;
    let reserved = match stored_scope {
        Some(_) => {
            access.release(&vault_id, thread_id)?;
            None
        }
        None => access.reserved_folder(&vault_id, thread_id)?,
    };
    validate_terminal_thread_scope(stored_scope.as_ref(), reserved.as_ref(), working_folder_id)?;
    let authorized = authorize_working_folder(
        app,
        pool,
        working_folder_id,
        WorkingFolderAuthorizationOperation::TerminalStart,
    )
    .await?;
    let thread_archived = stored_scope
        .as_ref()
        .is_some_and(|(_, state, _)| state == "archived");
    let environment_id = stored_scope.and_then(|(_, _, environment_id)| environment_id);
    let authorized = super::execution_environment::resolve_environment_workspace(
        app,
        pool,
        authorized,
        environment_id.as_deref(),
    )
    .await?;
    Ok(ThreadTerminalScope {
        vault_id,
        environment_id,
        thread_archived,
        authorized,
    })
}

async fn read_thread_scope(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<Option<(String, String, Option<String>)>> {
    let row = sqlx::query(
        "SELECT working_folder_id, state, execution_environment_id
         FROM chat_threads WHERE id = ?",
    )
    .bind(thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    row.as_ref()
        .map(|row| -> ChatResult<(String, String, Option<String>)> {
            Ok((
                row.try_get::<String, _>("working_folder_id")
                    .map_err(persistence_error)?,
                row.try_get::<String, _>("state")
                    .map_err(persistence_error)?,
                row.try_get::<Option<String>, _>("execution_environment_id")
                    .map_err(persistence_error)?,
            ))
        })
        .transpose()
}

/// Accepts a persisted open thread in the requested folder, or an unsaved
/// draft that Rust reserved for that folder. Unknown thread IDs are rejected.
fn validate_terminal_thread_scope(
    stored_scope: Option<&(String, String, Option<String>)>,
    reserved_folder: Option<&ProjectWorkingFolderId>,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatResult<()> {
    let Some((stored_workspace, state, _)) = stored_scope else {
        return match reserved_folder {
            Some(folder) if folder == working_folder_id => Ok(()),
            Some(_) => Err(reservation_mismatch_error()),
            None => Err(thread_not_found_error()),
        };
    };
    if state == "closed" {
        return Err(thread_not_found_error());
    }
    if stored_workspace != working_folder_id.as_str() {
        return Err(reservation_mismatch_error());
    }
    Ok(())
}

/// Refuses a new shell in an archived thread or working folder. Terminals that
/// already run there stay listable, usable, and closable.
fn validate_terminal_start(thread_archived: bool, folder_archived: bool) -> ChatResult<()> {
    let message = if folder_archived {
        "Unarchive the working folder to start a terminal in it"
    } else if thread_archived {
        "Unarchive the Chat thread to start a terminal in it"
    } else {
        return Ok(());
    };
    Err(ChatError::new(
        ChatErrorCode::InvalidStateTransition,
        message,
        true,
    ))
}

fn thread_not_found_error() -> ChatError {
    ChatError::new(ChatErrorCode::NotFound, "Chat thread was not found", true)
}

fn active_vault_id(app: &tauri::AppHandle) -> ChatResult<String> {
    crate::vault::active_vault_id(app).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Persistence,
            "Active vault is unavailable",
            true,
        )
    })
}

fn now_timestamp() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "create Chat timestamp", false))
}

async fn chat_pool(app: tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app, db_url)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat terminal context persistence failed",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn workspace(id: &str) -> ProjectWorkingFolderId {
        ProjectWorkingFolderId::new(id).expect("workspace ID should be valid")
    }

    fn stored(folder: &str, state: &str) -> (String, String, Option<String>) {
        (folder.to_string(), state.to_string(), None)
    }

    #[test]
    fn terminal_commands_accept_only_canonical_uuid_terminal_ids() {
        let request = |terminal_id: &str| {
            serde_json::from_value::<ChatTerminalInputRequest>(json!({
                "terminalId": terminal_id,
                "threadId": "thread",
                "workingFolderId": "folder",
                "text": "ls\r",
            }))
        };
        let valid = request("0f8fad5b-d9cb-469f-a165-70867728950e").expect("UUID is accepted");
        assert_eq!(
            valid.terminal_id.as_str(),
            "0f8fad5b-d9cb-469f-a165-70867728950e"
        );
        for invalid in [
            "",
            "terminal:test",
            "0F8FAD5B-D9CB-469F-A165-70867728950E",
            "{0f8fad5b-d9cb-469f-a165-70867728950e}",
            "0f8fad5bd9cb469fa16570867728950e",
            "0f8fad5b-d9cb-469f-a165-70867728950e\n",
            "0f8fad5b-d9cb-469f-a165_70867728950e",
        ] {
            assert!(request(invalid).is_err(), "{invalid:?} is rejected");
        }
    }
    #[test]
    fn terminal_create_rejects_unknown_thread_without_reservation() {
        let folder = workspace("workspace:test");
        let unknown = validate_terminal_thread_scope(None, None, &folder)
            .expect_err("an unknown thread ID must not authorize a terminal");
        assert_eq!(unknown.code, ChatErrorCode::NotFound);

        assert!(validate_terminal_thread_scope(None, Some(&folder), &folder).is_ok());
        let elsewhere =
            validate_terminal_thread_scope(None, Some(&workspace("workspace:other")), &folder)
                .expect_err("a draft reserved for another folder must not authorize this one");
        assert_eq!(elsewhere.code, ChatErrorCode::Permission);
    }

    #[test]
    fn terminal_scope_rejects_closed_or_cross_workspace_threads() {
        let folder = workspace("workspace:test");
        assert!(
            validate_terminal_thread_scope(
                Some(&stored("workspace:test", "active")),
                None,
                &folder
            )
            .is_ok()
        );

        let closed = validate_terminal_thread_scope(
            Some(&stored("workspace:test", "closed")),
            None,
            &folder,
        )
        .expect_err("closed threads must not authorize terminals");
        assert_eq!(closed.code, ChatErrorCode::NotFound);

        let mismatched = validate_terminal_thread_scope(
            Some(&stored("workspace:other", "active")),
            Some(&folder),
            &folder,
        )
        .expect_err("cross-workspace threads must not authorize terminals");
        assert_eq!(mismatched.code, ChatErrorCode::Permission);
    }

    #[test]
    fn archived_threads_and_folders_keep_running_terminals_but_start_no_new_ones() {
        let folder = workspace("workspace:test");
        assert!(
            validate_terminal_thread_scope(
                Some(&stored("workspace:test", "archived")),
                None,
                &folder
            )
            .is_ok(),
            "surviving terminals of an archived thread stay reachable"
        );
        assert!(validate_terminal_start(false, false).is_ok());
        for (thread_archived, folder_archived) in [(true, false), (false, true), (true, true)] {
            let refused = validate_terminal_start(thread_archived, folder_archived)
                .expect_err("archived scopes must not start terminals");
            assert_eq!(refused.code, ChatErrorCode::InvalidStateTransition);
        }
    }

    #[test]
    fn terminal_create_requires_start_grant() {
        let access = ChatTerminalAccess::default();
        let scope = TerminalStartScope::new("vault", workspace("workspace:test"), None);
        let asked = Cell::new(0);

        let declined =
            tauri::async_runtime::block_on(ensure_start_grant(&access, scope.clone(), || {
                asked.set(asked.get() + 1);
                async { Ok(false) }
            }))
            .expect_err("a declined confirmation must not start a terminal");
        assert_eq!(declined.code, ChatErrorCode::Cancelled);
        assert!(!access.is_granted(&scope).expect("read"));

        for _ in 0..2 {
            tauri::async_runtime::block_on(ensure_start_grant(&access, scope.clone(), || {
                asked.set(asked.get() + 1);
                async { Ok(true) }
            }))
            .expect("an allowed or granted folder starts terminals");
        }
        assert_eq!(
            asked.get(),
            2,
            "one grant covers later terminals in the folder"
        );

        let worktree = TerminalStartScope::new("vault", workspace("workspace:test"), Some("tree"));
        let failed = tauri::async_runtime::block_on(ensure_start_grant(&access, worktree, || {
            asked.set(asked.get() + 1);
            async {
                Err(ChatError::new(
                    ChatErrorCode::Internal,
                    "dialog failed",
                    true,
                ))
            }
        }))
        .expect_err("another environment asks again and dialog failures propagate");
        assert_eq!(failed.code, ChatErrorCode::Internal);
        assert_eq!(asked.get(), 3);

        access
            .clear_folder(&workspace("workspace:test"))
            .expect("clear");
        assert!(!access.is_granted(&scope).expect("read"));
    }

    #[test]
    fn layout_and_preview_discovery_authorize_the_thread_before_reading() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/chat");
        for (file, command, first_read) in [
            (
                "terminal_commands.rs",
                "chat_read_terminal_layout",
                "sqlx::query(",
            ),
            (
                "terminal_commands.rs",
                "chat_save_terminal_panel_layout",
                "pool.begin()",
            ),
            (
                "preview.rs",
                "chat_browser_discover_servers",
                "registry.snapshot(",
            ),
        ] {
            let source = std::fs::read_to_string(root.join(file)).expect("read Chat source");
            let start = source
                .find(&format!("pub async fn {command}("))
                .unwrap_or_else(|| panic!("{command} is defined"));
            let end = start + source[start..].find("\n}\n").expect("command ends");
            let body = &source[start..end];
            let authorized = body
                .find("authorize_thread_workspace(")
                .unwrap_or_else(|| panic!("{command} authorizes the thread"));
            let read = body
                .find(first_read)
                .unwrap_or_else(|| panic!("{command} reads {first_read}"));
            assert!(
                authorized < read,
                "{command} must authorize the thread before {first_read}"
            );
        }
    }
}
