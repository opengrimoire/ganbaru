//! Authorized commands for runtime terminals.

use super::terminal::{
    ChatTerminalCloseResult, ChatTerminalCreateInput, ChatTerminalId, ChatTerminalOutputChunk,
    ChatTerminalRead, ChatTerminalRegistry, ChatTerminalSnapshotRead,
};
use super::workspace::commands::authorize_working_folder;
use super::workspace::{AuthorizedWorkingFolder, WorkingFolderAuthorizationOperation};
use crate::db;
use chrono::{SecondsFormat, Utc};
use ganbaru_chat_contracts::models::{
    ChatChannelId, ChatError, ChatErrorCode, ChatReplyThreadId, ChatResult, ProjectWorkingFolderId,
    UtcTimestamp,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Row, SqlitePool};
use tauri::Manager;
use tauri::ipc::Channel;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChatTerminalRequest {
    pub terminal_id: ChatTerminalId,
    pub channel_id: ChatChannelId,
    /// Set when a reply thread tab opens the terminal, so the tab keeps panels of its own.
    #[serde(default)]
    pub reply_thread_id: Option<ChatReplyThreadId>,
    pub working_folder_id: ProjectWorkingFolderId,
    pub columns: u16,
    pub rows: u16,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTerminalInputRequest {
    pub terminal_id: ChatTerminalId,
    pub channel_id: ChatChannelId,
    pub working_folder_id: ProjectWorkingFolderId,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTerminalResizeRequest {
    pub terminal_id: ChatTerminalId,
    pub channel_id: ChatChannelId,
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
    pub channel_id: ChatChannelId,
    pub working_folder_id: ProjectWorkingFolderId,
    pub schema_version: u32,
    pub groups: Vec<ChatTerminalPanelLayout>,
    pub updated_at: Option<UtcTimestamp>,
}

#[tauri::command]
pub async fn chat_list_terminals(
    app: tauri::AppHandle,
    db_url: String,
    channel_id: ChatChannelId,
    working_folder_id: ProjectWorkingFolderId,
    reply_thread_id: Option<ChatReplyThreadId>,
) -> ChatResult<Vec<ChatTerminalRead>> {
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_channel_workspace(&app, &pool, &channel_id, &working_folder_id).await?;
    app.state::<ChatTerminalRegistry>().list_for_panel(
        &channel_id,
        &working_folder_id,
        reply_thread_id.as_ref(),
    )
}

#[tauri::command]
pub async fn chat_read_terminal_layout(
    app: tauri::AppHandle,
    db_url: String,
    channel_id: ChatChannelId,
    working_folder_id: ProjectWorkingFolderId,
) -> ChatResult<ChatTerminalLayoutRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_channel_workspace(&app, &pool, &channel_id, &working_folder_id).await?;
    let row = sqlx::query(
        "SELECT layout_schema_version, layout_data, updated_at
         FROM chat_terminal_layouts WHERE channel_id = ? AND working_folder_id = ?",
    )
    .bind(channel_id.as_str())
    .bind(working_folder_id.as_str())
    .fetch_optional(&pool)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(ChatTerminalLayoutRead {
            channel_id,
            working_folder_id,
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
        channel_id,
        working_folder_id,
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
    channel_id: ChatChannelId,
    working_folder_id: ProjectWorkingFolderId,
    panel: ChatTerminalPanelLayout,
) -> ChatResult<ChatTerminalLayoutRead> {
    validate_terminal_layout_groups(std::slice::from_ref(&panel))?;
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_channel_workspace(&app, &pool, &channel_id, &working_folder_id).await?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT layout_data FROM chat_terminal_layouts
         WHERE channel_id = ? AND working_folder_id = ?",
    )
    .bind(channel_id.as_str())
    .bind(working_folder_id.as_str())
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
            (channel_id, working_folder_id, layout_schema_version, layout_data, updated_at)
         VALUES (?, ?, 1, ?, ?)
         ON CONFLICT(channel_id, working_folder_id) DO UPDATE SET
            layout_schema_version = excluded.layout_schema_version,
            layout_data = excluded.layout_data,
            updated_at = excluded.updated_at",
    )
    .bind(channel_id.as_str())
    .bind(working_folder_id.as_str())
    .bind(data)
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(ChatTerminalLayoutRead {
        channel_id,
        working_folder_id,
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
        authorize_channel_scope(&app, &pool, &request.channel_id, &request.working_folder_id)
            .await?;
    validate_terminal_start(scope.channel_archived, scope.folder_archived)?;
    app.state::<ChatTerminalRegistry>().create(
        app.clone(),
        ChatTerminalCreateInput {
            terminal_id: request.terminal_id.into_inner(),
            channel_id: request.channel_id,
            reply_thread_id: request.reply_thread_id,
            working_folder_id: request.working_folder_id,
            workspace_path: scope.authorized.canonical_path,
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
    channel_id: ChatChannelId,
    working_folder_id: ProjectWorkingFolderId,
) -> ChatResult<ChatTerminalSnapshotRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_channel_workspace(&app, &pool, &channel_id, &working_folder_id).await?;
    let registry = app.state::<ChatTerminalRegistry>();
    registry.require_scope(terminal_id.as_str(), &channel_id, &working_folder_id)?;
    registry.snapshot(terminal_id.as_str())
}

/// Subscribes the calling view to live output through its own channel and returns the replay to render first.
#[tauri::command]
pub async fn chat_terminal_attach(
    app: tauri::AppHandle,
    db_url: String,
    terminal_id: ChatTerminalId,
    channel_id: ChatChannelId,
    working_folder_id: ProjectWorkingFolderId,
    subscription_id: String,
    channel: Channel<ChatTerminalOutputChunk>,
) -> ChatResult<ChatTerminalSnapshotRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_channel_workspace(&app, &pool, &channel_id, &working_folder_id).await?;
    let registry = app.state::<ChatTerminalRegistry>();
    registry.require_scope(terminal_id.as_str(), &channel_id, &working_folder_id)?;
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
    authorize_channel_workspace(&app, &pool, &request.channel_id, &request.working_folder_id)
        .await?;
    let registry = app.state::<ChatTerminalRegistry>();
    registry.require_scope(
        request.terminal_id.as_str(),
        &request.channel_id,
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
    authorize_channel_workspace(&app, &pool, &request.channel_id, &request.working_folder_id)
        .await?;
    let registry = app.state::<ChatTerminalRegistry>();
    registry.require_scope(
        request.terminal_id.as_str(),
        &request.channel_id,
        &request.working_folder_id,
    )?;
    registry.resize(request.terminal_id.as_str(), request.columns, request.rows)
}

#[tauri::command]
pub async fn chat_terminal_close(
    app: tauri::AppHandle,
    db_url: String,
    terminal_id: ChatTerminalId,
    channel_id: ChatChannelId,
    working_folder_id: ProjectWorkingFolderId,
    confirmed: bool,
) -> ChatResult<ChatTerminalCloseResult> {
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_channel_workspace(&app, &pool, &channel_id, &working_folder_id).await?;
    let registry = app.state::<ChatTerminalRegistry>();
    registry.require_scope(terminal_id.as_str(), &channel_id, &working_folder_id)?;
    registry.close(terminal_id.as_str(), confirmed)
}

/// A channel's authorized terminal working folder and the archive states that gate new shells.
struct ChannelTerminalScope {
    channel_archived: bool,
    folder_archived: bool,
    authorized: AuthorizedWorkingFolder,
}

/// The stored channel fields that decide terminal access.
struct StoredChannelScope {
    project_id: String,
    archived: bool,
}

/// Authorizes a channel of the active vault in one of its project's working folders for terminal access on
/// this device.
pub(crate) async fn authorize_channel_workspace(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    channel_id: &ChatChannelId,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatResult<AuthorizedWorkingFolder> {
    Ok(
        authorize_channel_scope(app, pool, channel_id, working_folder_id)
            .await?
            .authorized,
    )
}

async fn authorize_channel_scope(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    channel_id: &ChatChannelId,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatResult<ChannelTerminalScope> {
    let channel = read_channel_scope(pool, channel_id).await?;
    let folder =
        ganbaru_chat::repository::workspaces::read_workspace(pool, working_folder_id).await?;
    validate_terminal_channel_scope(channel.as_ref(), &folder.project_id)?;
    let authorized = authorize_working_folder(
        app,
        pool,
        working_folder_id,
        WorkingFolderAuthorizationOperation::TerminalStart,
    )
    .await?;
    Ok(ChannelTerminalScope {
        channel_archived: channel.is_some_and(|channel| channel.archived),
        folder_archived: folder.archived_at.is_some(),
        authorized,
    })
}

async fn read_channel_scope(
    pool: &SqlitePool,
    channel_id: &ChatChannelId,
) -> ChatResult<Option<StoredChannelScope>> {
    let row = sqlx::query("SELECT project_id, archived_at FROM chat_channels WHERE id = ?")
        .bind(channel_id.as_str())
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?;
    row.as_ref()
        .map(|row| -> ChatResult<StoredChannelScope> {
            Ok(StoredChannelScope {
                project_id: row
                    .try_get::<String, _>("project_id")
                    .map_err(persistence_error)?,
                archived: row
                    .try_get::<Option<String>, _>("archived_at")
                    .map_err(persistence_error)?
                    .is_some(),
            })
        })
        .transpose()
}

/// Accepts a stored channel whose project owns the working folder. Unknown channel IDs are rejected, and a
/// folder of another project never opens a terminal for the channel.
fn validate_terminal_channel_scope(
    channel: Option<&StoredChannelScope>,
    folder_project_id: &str,
) -> ChatResult<()> {
    let Some(channel) = channel else {
        return Err(ChatError::new(
            ChatErrorCode::NotFound,
            "Chat channel was not found",
            true,
        ));
    };
    if channel.project_id != folder_project_id {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "The working folder belongs to another project than the channel",
            false,
        ));
    }
    Ok(())
}

/// Refuses a new shell in an archived channel or working folder. Terminals that
/// already run there stay listable, usable, and closable.
fn validate_terminal_start(channel_archived: bool, folder_archived: bool) -> ChatResult<()> {
    let message = if folder_archived {
        "Unarchive the working folder to start a terminal in it"
    } else if channel_archived {
        "Restore the channel to start a terminal in it"
    } else {
        return Ok(());
    };
    Err(ChatError::new(
        ChatErrorCode::InvalidStateTransition,
        message,
        true,
    ))
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

    const TIMESTAMP: &str = "2026-11-14T00:00:00.000Z";

    fn channel(id: &str) -> ChatChannelId {
        ChatChannelId::new(id).expect("channel ID should be valid")
    }

    fn stored(project_id: &str, archived: bool) -> StoredChannelScope {
        StoredChannelScope {
            project_id: project_id.to_string(),
            archived,
        }
    }

    async fn pool_with_channels() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("memory database should open");
        ganbaru_db::test_support::apply_migrated_template(&pool)
            .await
            .expect("migrations should apply");
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&pool)
            .await
            .expect("foreign keys should turn off");
        for (id, project_id, archived_at) in [
            ("channel:open", "project:a", None),
            ("channel:archived", "project:a", Some(TIMESTAMP)),
        ] {
            sqlx::query(
                "INSERT INTO chat_conversations
                    (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
                 VALUES (?, ?, 'channel', ?, ?, ?)",
            )
            .bind(format!("conversation:{id}"))
            .bind(project_id)
            .bind(TIMESTAMP)
            .bind(TIMESTAMP)
            .bind(TIMESTAMP)
            .execute(&pool)
            .await
            .expect("conversation should insert");
            sqlx::query(
                "INSERT INTO chat_channels
                    (id, project_id, conversation_id, name, archived_at, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(project_id)
            .bind(format!("conversation:{id}"))
            .bind(id)
            .bind(archived_at)
            .bind(TIMESTAMP)
            .bind(TIMESTAMP)
            .execute(&pool)
            .await
            .expect("channel should insert");
        }
        pool
    }

    #[test]
    fn terminal_commands_accept_only_canonical_uuid_terminal_ids() {
        let request = |terminal_id: &str| {
            serde_json::from_value::<ChatTerminalInputRequest>(json!({
                "terminalId": terminal_id,
                "channelId": "channel",
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
    fn terminal_creation_names_a_reply_thread_only_for_thread_tabs() {
        let request = |reply_thread_id: Option<&str>| {
            let mut value = json!({
                "terminalId": "0f8fad5b-d9cb-469f-a165-70867728950e",
                "channelId": "channel",
                "workingFolderId": "folder",
                "columns": 80,
                "rows": 24,
            });
            if let Some(id) = reply_thread_id {
                value["replyThreadId"] = json!(id);
            }
            serde_json::from_value::<CreateChatTerminalRequest>(value)
        };
        let channel_panel = request(None).expect("a channel panel request is valid");
        assert!(channel_panel.reply_thread_id.is_none());
        let thread_panel = request(Some("reply")).expect("a thread tab request is valid");
        assert_eq!(
            thread_panel
                .reply_thread_id
                .as_ref()
                .map(ChatReplyThreadId::as_str),
            Some("reply")
        );
        assert!(
            request(Some("")).is_err(),
            "an empty reply thread ID is rejected"
        );
    }

    #[test]
    fn terminal_requests_name_a_channel_instead_of_a_thread() {
        let legacy = serde_json::from_value::<ChatTerminalInputRequest>(json!({
            "terminalId": "0f8fad5b-d9cb-469f-a165-70867728950e",
            "threadId": "thread",
            "workingFolderId": "folder",
            "text": "ls\r",
        }));
        assert!(legacy.is_err(), "a thread-scoped request has no channel");
    }

    #[test]
    fn terminal_scope_rejects_unknown_channels_and_folders_of_other_projects() {
        let unknown = validate_terminal_channel_scope(None, "project:a")
            .expect_err("an unknown channel ID must not authorize a terminal");
        assert_eq!(unknown.code, ChatErrorCode::NotFound);

        assert!(
            validate_terminal_channel_scope(Some(&stored("project:a", false)), "project:a").is_ok()
        );
        let foreign =
            validate_terminal_channel_scope(Some(&stored("project:a", false)), "project:b")
                .expect_err("a folder of another project must not open a terminal for the channel");
        assert_eq!(foreign.code, ChatErrorCode::Permission);
    }

    #[test]
    fn archived_channels_keep_running_terminals_and_archived_scopes_start_none() {
        assert!(
            validate_terminal_channel_scope(Some(&stored("project:a", true)), "project:a").is_ok(),
            "surviving terminals of an archived channel stay reachable"
        );
        assert!(validate_terminal_start(false, false).is_ok());
        for (channel_archived, folder_archived) in [(true, false), (false, true), (true, true)] {
            let refused = validate_terminal_start(channel_archived, folder_archived)
                .expect_err("archived scopes must not start terminals");
            assert_eq!(refused.code, ChatErrorCode::InvalidStateTransition);
        }
    }

    #[tokio::test]
    async fn channel_scope_reads_the_stored_project_and_archive_state() {
        let pool = pool_with_channels().await;
        let open = read_channel_scope(&pool, &channel("channel:open"))
            .await
            .expect("channel should read")
            .expect("channel should exist");
        assert_eq!(open.project_id, "project:a");
        assert!(!open.archived);
        let archived = read_channel_scope(&pool, &channel("channel:archived"))
            .await
            .expect("channel should read")
            .expect("channel should exist");
        assert!(archived.archived);
        assert!(
            read_channel_scope(&pool, &channel("channel:missing"))
                .await
                .expect("missing channel should read")
                .is_none()
        );
    }

    #[test]
    fn layout_and_preview_discovery_authorize_the_channel_before_reading() {
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
                .find("authorize_channel_workspace(")
                .unwrap_or_else(|| panic!("{command} authorizes the channel"));
            let read = body
                .find(first_read)
                .unwrap_or_else(|| panic!("{command} reads {first_read}"));
            assert!(
                authorized < read,
                "{command} must authorize the channel before {first_read}"
            );
        }
    }
}
