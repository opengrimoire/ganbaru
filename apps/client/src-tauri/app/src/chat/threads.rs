//! Projection-backed commands for Chat thread navigation and lifecycle actions.

use super::workspace::WorkingFolderAuthorizationOperation;
use super::workspace::commands::authorize_working_folder;
use super::{credentials::PlatformCredentialStore, credentials::materialize_provider_environment};
use crate::db;
use chrono::{SecondsFormat, Utc};
use ganbaru_chat::repository::provider_lifecycle::{
    self, EnqueueProviderLifecycleFailure, ProviderLifecycleOperation,
};
use ganbaru_chat::repository::{lifecycle, reads};
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ChatThreadId, ChatThreadShellRead, ChatTimelinePageRead,
    ProjectWorkingFolderId, ProviderThreadLifecycleRequest, UtcTimestamp,
};
use ganbaru_chat_providers::{
    DriverCancellation, DriverOperationContext, ProviderDriverFactory, ProviderDriverRegistry,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::time::{Duration, Instant, SystemTime};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

const PERMANENT_DELETE_CLEANUP_GRACE: Duration = Duration::from_secs(24 * 60 * 60);
const PROVIDER_LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(30);
const THREAD_DELETE_STOP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkChatThreadCommand {
    pub source_thread_id: ChatThreadId,
    pub new_thread_id: ChatThreadId,
    pub title: String,
    pub last_provider_turn_id: Option<String>,
}

#[tauri::command]
pub async fn chat_list_thread_window(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: Option<ProjectWorkingFolderId>,
    archived: bool,
    limit: u32,
) -> ChatResult<Vec<ChatThreadShellRead>> {
    reads::read_thread_shell_window(
        &chat_pool(app, db_url).await?,
        working_folder_id.as_ref(),
        archived,
        limit,
    )
    .await
}

#[tauri::command]
pub async fn chat_read_thread_shell(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
) -> ChatResult<ChatThreadShellRead> {
    reads::read_thread_shell(&chat_pool(app, db_url).await?, &thread_id).await
}

#[tauri::command]
pub async fn chat_read_timeline_page(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    cursor: Option<String>,
    limit: u32,
) -> ChatResult<ChatTimelinePageRead> {
    let cursor = cursor
        .as_deref()
        .map(reads::parse_timeline_cursor)
        .transpose()?;
    reads::read_timeline_page(
        &chat_pool(app, db_url).await?,
        &thread_id,
        cursor.as_ref(),
        limit,
    )
    .await
}

#[tauri::command]
pub async fn chat_read_timeline_turn(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    turn_id: ganbaru_chat_contracts::models::ChatTurnId,
) -> ChatResult<ChatTimelinePageRead> {
    reads::read_timeline_turn(&chat_pool(app, db_url).await?, &thread_id, &turn_id).await
}

#[tauri::command]
pub async fn chat_fork_thread(
    app: tauri::AppHandle,
    db_url: String,
    request: ForkChatThreadCommand,
) -> ChatResult<ChatThreadShellRead> {
    let title = request.title.trim();
    if title.is_empty() || title.len() > 1_000 || title.chars().any(char::is_control) {
        return Err(ChatError::validation(
            "title",
            "Forked thread title is invalid",
        ));
    }
    if request.source_thread_id == request.new_thread_id {
        return Err(ChatError::validation(
            "newThreadId",
            "Forked thread ID must be new",
        ));
    }
    if let Some(turn_id) = request.last_provider_turn_id.as_deref() {
        if turn_id.is_empty() || turn_id.len() > 1_024 || turn_id.chars().any(char::is_control) {
            return Err(ChatError::validation(
                "lastProviderTurnId",
                "Provider turn ID is invalid",
            ));
        }
    }
    let pool = chat_pool(app.clone(), db_url).await?;
    let source = reads::read_thread_shell(&pool, &request.source_thread_id).await?;
    if source.archived_at.is_some()
        || source.state == ganbaru_chat_contracts::models::ChatThreadState::Closed
    {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Archived or closed threads cannot be forked",
            true,
        ));
    }
    let working_folder_id = source.working_folder_id.as_ref().ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::Conflict,
            "Private scratch work cannot be forked into direct agent history",
            true,
        )
    })?;
    let execution_environment_id: Option<String> =
        sqlx::query_scalar("SELECT execution_environment_id FROM chat_threads WHERE id = ?")
            .bind(request.source_thread_id.as_str())
            .fetch_one(&pool)
            .await
            .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "read Chat thread", true))?;
    let (provider_thread_id, resume_cursor) = match source.provider_thread_id.clone() {
        Some(provider_thread_id)
            if matches!(source.provider_family_id.as_str(), "codex" | "opencode") =>
        {
            let authorized = authorize_working_folder(
                &app,
                &pool,
                working_folder_id,
                WorkingFolderAuthorizationOperation::ProviderStart,
            )
            .await?;
            let authorized = super::execution_environment::resolve_environment_workspace(
                &app,
                &pool,
                authorized,
                execution_environment_id.as_deref(),
            )
            .await?;
            let verified = ganbaru_chat_contracts::models::VerifiedWorkspaceContext {
                working_folder_id: working_folder_id.clone(),
                canonical_path: authorized
                    .canonical_path
                    .to_str()
                    .ok_or_else(|| {
                        ChatError::validation("workspace", "Workspace path is unsupported")
                    })?
                    .to_string(),
                repository_kind: authorized.repository_kind,
                repository_identity: authorized.repository_identity,
            };
            let provider =
                super::settings_commands::read_provider(&app, &source.provider_instance_id)?;
            let configuration = materialize_provider_environment(
                &provider.configuration,
                &PlatformCredentialStore::default(),
            )?;
            let mut driver = ProviderDriverRegistry.create_driver(configuration)?;
            let context = DriverOperationContext {
                operation_id: format!("provider-thread-fork:{}", request.new_thread_id.as_str()),
                deadline: Instant::now() + PROVIDER_LIFECYCLE_TIMEOUT,
                cancellation: DriverCancellation::default(),
            };
            let forked = driver
                .fork_thread(
                    ganbaru_chat_contracts::models::ProviderForkThreadRequest {
                        provider_thread_id,
                        workspace: verified,
                        last_provider_turn_id: request.last_provider_turn_id.clone(),
                    },
                    &context,
                )
                .await?;
            let cursor = provider_resume_cursor(source.provider_family_id.as_str(), &forked)?;
            (Some(forked), Some(cursor))
        }
        _ => (None, None),
    };
    let now = now_timestamp()?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "create Chat thread fork", true))?;
    let inserted = sqlx::query(
        "INSERT INTO chat_threads
            (id, working_folder_id, execution_environment_id, project_id, title, title_source,
             provider_family_id, provider_instance_id, continuation_group_id, provider_thread_id,
             resume_cursor_schema_version, resume_cursor_data, model_selection_schema_version,
             model_selection_data, safety_mode, interaction_mode, state, latest_turn_state,
             last_activity_at, created_at, updated_at)
         SELECT ?, working_folder_id, execution_environment_id, project_id, ?, 'user',
                provider_family_id, provider_instance_id, continuation_group_id, ?, ?, ?,
                model_selection_schema_version, model_selection_data, safety_mode, interaction_mode,
                'idle', NULL, ?, ?, ?
         FROM chat_threads WHERE id = ? AND state != 'closed'",
    )
    .bind(request.new_thread_id.as_str())
    .bind(title)
    .bind(provider_thread_id.as_ref().map(|value| value.as_str()))
    .bind(
        resume_cursor
            .as_ref()
            .map(|value| i64::from(value.schema_version)),
    )
    .bind(resume_cursor.as_ref().map(|value| value.value.to_string()))
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(request.source_thread_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "create Chat thread fork", true))?;
    if inserted.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::NotFound,
            "Source Chat thread was not found",
            true,
        ));
    }
    sqlx::query(
        "INSERT INTO chat_thread_relations
            (child_thread_id, parent_thread_id, source_turn_id, relation_kind, created_at)
         VALUES (?, ?, NULL, 'fork', ?)",
    )
    .bind(request.new_thread_id.as_str())
    .bind(request.source_thread_id.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "record Chat thread fork", true))?;
    transaction
        .commit()
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "create Chat thread fork", true))?;
    reads::read_thread_shell(&pool, &request.new_thread_id).await
}

fn provider_resume_cursor(
    provider_family_id: &str,
    provider_thread_id: &ganbaru_chat_contracts::models::ProviderThreadId,
) -> ChatResult<ganbaru_chat_contracts::models::VersionedJson> {
    let value = match provider_family_id {
        "codex" => json!({ "threadId": provider_thread_id.as_str() }),
        "opencode" => json!({ "sessionId": provider_thread_id.as_str() }),
        _ => {
            return Err(ChatError::unsupported(
                "Provider does not expose a resumable native fork",
            ));
        }
    };
    Ok(ganbaru_chat_contracts::models::VersionedJson {
        schema_version: 1,
        value,
    })
}

#[tauri::command]
pub fn chat_open_external_url(app: tauri::AppHandle, url: String) -> ChatResult<()> {
    let parsed = validate_external_url(&url)?;
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|_| {
            ChatError::new(
                ChatErrorCode::Permission,
                "Chat link could not be opened",
                true,
            )
        })
}

fn validate_external_url(url: &str) -> ChatResult<reqwest::Url> {
    if url.len() > 2_048 || url.chars().any(char::is_control) {
        return Err(ChatError::validation("url", "Chat link is invalid"));
    }
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| ChatError::validation("url", "Chat link is invalid"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(ChatError::validation("url", "Chat link is not allowed"));
    }
    Ok(parsed)
}

#[tauri::command]
pub async fn chat_rename_thread(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    title: String,
    expected_revision: u64,
) -> ChatResult<ChatThreadShellRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    lifecycle::rename_thread(
        &pool,
        &thread_id,
        &title,
        expected_revision,
        &now_timestamp()?,
    )
    .await?;
    let shell = reads::read_thread_shell(&pool, &thread_id).await?;
    synchronize_provider_lifecycle(
        &app,
        &pool,
        &shell,
        ProviderLifecycleOperation::Rename,
        json!({ "title": shell.title }),
        Some(&thread_id),
    )
    .await?;
    Ok(shell)
}

#[tauri::command]
pub async fn chat_set_thread_read(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    read: bool,
    expected_revision: u64,
) -> ChatResult<ChatThreadShellRead> {
    let pool = chat_pool(app, db_url).await?;
    lifecycle::set_thread_read(
        &pool,
        &thread_id,
        read,
        expected_revision,
        &now_timestamp()?,
    )
    .await?;
    reads::read_thread_shell(&pool, &thread_id).await
}

#[tauri::command]
pub async fn chat_archive_thread(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    expected_revision: u64,
) -> ChatResult<ChatThreadShellRead> {
    set_thread_archived(app, db_url, thread_id, expected_revision, true).await
}

#[tauri::command]
pub async fn chat_restore_thread(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    expected_revision: u64,
) -> ChatResult<ChatThreadShellRead> {
    set_thread_archived(app, db_url, thread_id, expected_revision, false).await
}

#[tauri::command]
pub async fn chat_delete_thread_permanently(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    expected_revision: u64,
    confirmed_title: String,
) -> ChatResult<()> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let shell = reads::read_thread_shell(&pool, &thread_id).await?;
    if shell.title != confirmed_title {
        return Err(ChatError::validation(
            "confirmedTitle",
            "The confirmation title does not match the Chat thread",
        ));
    }
    let runtimes = app.state::<ganbaru_chat::runtime::ChatRuntimeRegistry>();
    let mutations = app.state::<super::workspace::mutation::ChatWorkspaceMutationRegistry>();
    runtimes
        .shutdown_thread_and_remove(&thread_id, THREAD_DELETE_STOP_TIMEOUT, &mutations)
        .await?;
    let now = SystemTime::now();
    let cleanup = now
        .checked_add(PERMANENT_DELETE_CLEANUP_GRACE)
        .ok_or_else(|| ChatError::new(ChatErrorCode::Internal, "create cleanup deadline", false))?;
    lifecycle::permanently_delete_thread(
        &pool,
        &thread_id,
        expected_revision,
        &timestamp(cleanup)?,
        &timestamp(now)?,
    )
    .await?;
    app.state::<super::terminal::ChatTerminalRegistry>()
        .shutdown_thread(&thread_id)?;
    app.state::<super::internal_mcp::InternalMcpRegistry>()
        .stop_thread_endpoint(&thread_id)
        .await;
    app.state::<super::preview::ChatBrowserManager>()
        .close_thread(&app, &thread_id);
    synchronize_provider_lifecycle(
        &app,
        &pool,
        &shell,
        ProviderLifecycleOperation::Delete,
        json!({}),
        None,
    )
    .await?;
    Ok(())
}

async fn set_thread_archived(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    expected_revision: u64,
    archived: bool,
) -> ChatResult<ChatThreadShellRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    lifecycle::set_thread_archived(
        &pool,
        &thread_id,
        archived,
        expected_revision,
        &now_timestamp()?,
    )
    .await?;
    let shell = reads::read_thread_shell(&pool, &thread_id).await?;
    if archived {
        synchronize_provider_lifecycle(
            &app,
            &pool,
            &shell,
            ProviderLifecycleOperation::Archive,
            json!({}),
            Some(&thread_id),
        )
        .await?;
    }
    Ok(shell)
}

async fn synchronize_provider_lifecycle(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    shell: &ChatThreadShellRead,
    operation: ProviderLifecycleOperation,
    payload: Value,
    retained_thread_id: Option<&ChatThreadId>,
) -> ChatResult<()> {
    let Some(provider_thread_id) = shell.provider_thread_id.clone() else {
        return Ok(());
    };
    let result = async {
        let provider = super::settings_commands::read_provider(app, &shell.provider_instance_id)?;
        let configuration = materialize_provider_environment(
            &provider.configuration,
            &PlatformCredentialStore::default(),
        )?;
        let mut driver = ProviderDriverRegistry.create_driver(configuration)?;
        let request = ProviderThreadLifecycleRequest {
            provider_thread_id: provider_thread_id.clone(),
            title: payload
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_string),
        };
        let context = DriverOperationContext {
            operation_id: format!("provider-thread-{}", operation.as_str()),
            deadline: Instant::now() + PROVIDER_LIFECYCLE_TIMEOUT,
            cancellation: DriverCancellation::default(),
        };
        match operation {
            ProviderLifecycleOperation::Rename => driver.rename_thread(request, &context).await,
            ProviderLifecycleOperation::Archive => driver.archive_thread(request, &context).await,
            ProviderLifecycleOperation::Delete => driver.delete_thread(request, &context).await,
            ProviderLifecycleOperation::Unsubscribe => {
                driver.unsubscribe_thread(request, &context).await
            }
            ProviderLifecycleOperation::Cleanup => driver.cleanup_thread(request, &context).await,
        }
        .map(|_| ())
    }
    .await;
    if let Err(error) = result {
        provider_lifecycle::enqueue_failure(
            pool,
            EnqueueProviderLifecycleFailure {
                thread_id: retained_thread_id,
                provider_family_id: &shell.provider_family_id,
                provider_instance_id: &shell.provider_instance_id,
                provider_thread_id: &provider_thread_id,
                operation,
                payload: &payload,
                error: &error,
                now: &now_timestamp()?,
            },
        )
        .await?;
    }
    Ok(())
}

async fn chat_pool(app: tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app, db_url)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))
}

fn now_timestamp() -> ChatResult<UtcTimestamp> {
    timestamp(SystemTime::now())
}

fn timestamp(value: SystemTime) -> ChatResult<UtcTimestamp> {
    let value: chrono::DateTime<Utc> = value.into();
    UtcTimestamp::new(value.to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "create Chat timestamp", false))
}

#[cfg(test)]
mod tests {
    use super::validate_external_url;

    #[test]
    fn external_chat_links_allow_only_uncredentialed_http_urls() {
        assert!(validate_external_url("https://example.com/docs").is_ok());
        assert!(validate_external_url("http://localhost:3000/path").is_ok());
        assert!(validate_external_url("file:///etc/passwd").is_err());
        assert!(validate_external_url("javascript:alert(1)").is_err());
        assert!(validate_external_url("https://token@example.com").is_err());
        assert!(validate_external_url("https://example.com/\nheader").is_err());
    }
}
