//! Organizational project channels.

use crate::db;
use chrono::{DateTime, SecondsFormat, Utc};
use ganbaru_chat_contracts::models::{
    ChatChannelId, ChatChannelRead, ChatConversationId, ChatError, ChatErrorCode, ChatResult,
    ChatWorkAssignmentState, UtcTimestamp,
};
use serde::Deserialize;
use sqlx::{Row, SqlitePool};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const LOCAL_PARTICIPANT_ID: &str = "participant:local-owner";
const MAX_CHANNEL_NAME_CHARS: usize = 80;
const MAX_CHANNEL_TOPIC_CHARS: usize = 250;
const MAX_CHANNEL_RESULTS: u32 = 500;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChatChannelCommand {
    pub id: ChatChannelId,
    pub project_id: String,
    pub name: String,
    #[serde(default)]
    pub topic: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateChatChannelDetailsCommand {
    pub channel_id: ChatChannelId,
    pub name: String,
    #[serde(default)]
    pub topic: String,
    pub expected_revision: u64,
}

#[tauri::command]
pub async fn chat_list_channels(
    app: tauri::AppHandle,
    db_url: String,
    project_id: String,
    archived: bool,
) -> ChatResult<Vec<ChatChannelRead>> {
    validate_project_id(&project_id)?;
    read_channels(
        &chat_pool(app, db_url).await?,
        Some(&project_id),
        archived,
        None,
    )
    .await
}

#[tauri::command]
pub async fn chat_list_navigation_channels(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<Vec<ChatChannelRead>> {
    read_channels(&chat_pool(app, db_url).await?, None, false, None).await
}

#[tauri::command]
pub async fn chat_read_channel(
    app: tauri::AppHandle,
    db_url: String,
    channel_id: ChatChannelId,
) -> ChatResult<ChatChannelRead> {
    read_channel(&chat_pool(app, db_url).await?, &channel_id).await
}

#[tauri::command]
pub async fn chat_create_channel(
    app: tauri::AppHandle,
    db_url: String,
    request: CreateChatChannelCommand,
) -> ChatResult<ChatChannelRead> {
    validate_project_id(&request.project_id)?;
    let name = normalized_channel_name(&request.name)?;
    let topic = normalized_topic(&request.topic)?;
    let pool = chat_pool(app, db_url).await?;
    require_unique_name(&pool, &request.project_id, &name, None).await?;
    require_active_project(&pool, &request.project_id).await?;
    let now = now_timestamp()?;
    let conversation_id =
        ChatConversationId::new(new_id("conversation")).map_err(identifier_error)?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_conversations
            (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
         VALUES (?, ?, 'channel', ?, ?, ?)",
    )
    .bind(conversation_id.as_str())
    .bind(&request.project_id)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_channels
            (id, project_id, conversation_id, name, topic, is_default, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, 0, ?, ?)",
    )
    .bind(request.id.as_str())
    .bind(&request.project_id)
    .bind(conversation_id.as_str())
    .bind(name)
    .bind(topic)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_conversation_memberships
            (conversation_id, participant_id, membership_role, created_at, updated_at)
         VALUES (?, ?, 'owner', ?, ?)",
    )
    .bind(conversation_id.as_str())
    .bind(LOCAL_PARTICIPANT_ID)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    read_channel(&pool, &request.id).await
}

#[tauri::command]
pub async fn chat_update_channel_details(
    app: tauri::AppHandle,
    db_url: String,
    request: UpdateChatChannelDetailsCommand,
) -> ChatResult<ChatChannelRead> {
    let pool = chat_pool(app, db_url).await?;
    let current = read_channel(&pool, &request.channel_id).await?;
    let name = normalized_channel_name(&request.name)?;
    if current.is_default && name != "general" {
        return Err(ChatError::validation(
            "name",
            "The general channel name is fixed",
        ));
    }
    require_unique_name(&pool, &current.project_id, &name, Some(&request.channel_id)).await?;
    let topic = normalized_topic(&request.topic)?;
    let now = now_timestamp()?;
    let updated = sqlx::query(
        "UPDATE chat_channels
         SET name = ?, topic = ?, revision = revision + 1, updated_at = ?
         WHERE id = ? AND revision = ?",
    )
    .bind(name)
    .bind(topic)
    .bind(now.as_str())
    .bind(request.channel_id.as_str())
    .bind(i64_value(request.expected_revision)?)
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    require_updated(&pool, &request.channel_id, updated.rows_affected()).await?;
    read_channel(&pool, &request.channel_id).await
}

#[tauri::command]
pub async fn chat_archive_channel(
    app: tauri::AppHandle,
    db_url: String,
    channel_id: ChatChannelId,
    expected_revision: u64,
) -> ChatResult<ChatChannelRead> {
    set_channel_archived(app, db_url, channel_id, expected_revision, true).await
}

#[tauri::command]
pub async fn chat_restore_channel(
    app: tauri::AppHandle,
    db_url: String,
    channel_id: ChatChannelId,
    expected_revision: u64,
) -> ChatResult<ChatChannelRead> {
    set_channel_archived(app, db_url, channel_id, expected_revision, false).await
}

#[tauri::command]
pub async fn chat_set_channel_read(
    app: tauri::AppHandle,
    db_url: String,
    channel_id: ChatChannelId,
    read: bool,
) -> ChatResult<ChatChannelRead> {
    let pool = chat_pool(app, db_url).await?;
    let channel = read_channel(&pool, &channel_id).await?;
    let now = now_timestamp()?;
    let ordinal = if read {
        sqlx::query_scalar::<_, i64>(
            "SELECT coalesce(max(ordinal), 0)
             FROM chat_conversation_items
             WHERE conversation_id = ? AND reply_thread_id IS NULL",
        )
        .bind(channel.conversation_id.as_str())
        .fetch_one(&pool)
        .await
        .map_err(persistence_error)?
    } else {
        0
    };
    sqlx::query(
        "INSERT INTO chat_conversation_read_cursors
            (conversation_id, participant_id, last_read_root_ordinal, updated_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(conversation_id, participant_id) DO UPDATE SET
            last_read_root_ordinal = excluded.last_read_root_ordinal,
            updated_at = excluded.updated_at",
    )
    .bind(channel.conversation_id.as_str())
    .bind(LOCAL_PARTICIPANT_ID)
    .bind(ordinal)
    .bind(now.as_str())
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    read_channel(&pool, &channel_id).await
}

pub(crate) async fn read_channel(
    pool: &SqlitePool,
    channel_id: &ChatChannelId,
) -> ChatResult<ChatChannelRead> {
    let row = sqlx::query(&channel_read_query("channel.id = ?", None))
        .bind(channel_id.as_str())
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?
        .ok_or_else(|| {
            ChatError::new(ChatErrorCode::NotFound, "Chat channel was not found", true)
        })?;
    channel_from_row(pool, &row).await
}

async fn read_channels(
    pool: &SqlitePool,
    project_id: Option<&str>,
    archived: bool,
    search: Option<(&str, u32)>,
) -> ChatResult<Vec<ChatChannelRead>> {
    let archive_clause = if archived {
        "channel.archived_at IS NOT NULL"
    } else {
        "channel.archived_at IS NULL"
    };
    let project_clause = if project_id.is_some() {
        " AND channel.project_id = ?"
    } else {
        " AND EXISTS (
            SELECT 1 FROM projects project
            WHERE project.id = channel.project_id AND project.status != 'archived'
          )"
    };
    let search_clause = if search.is_some() {
        " AND (lower(channel.name) LIKE '%' || ? || '%' OR lower(channel.topic) LIKE '%' || ? || '%')"
    } else {
        ""
    };
    let limit = search
        .map(|(_, limit)| limit)
        .unwrap_or(MAX_CHANNEL_RESULTS);
    let query = channel_read_query(
        &format!("{archive_clause}{project_clause}{search_clause}"),
        Some("channel.is_default DESC, channel.name COLLATE NOCASE, channel.id LIMIT ?"),
    );
    let mut prepared = sqlx::query(&query);
    if let Some(project_id) = project_id {
        prepared = prepared.bind(project_id);
    }
    if let Some((search, _)) = search {
        prepared = prepared.bind(search).bind(search);
    }
    let rows = prepared
        .bind(i64::from(limit))
        .fetch_all(pool)
        .await
        .map_err(persistence_error)?;
    let mut channels = Vec::with_capacity(rows.len());
    for row in rows {
        channels.push(channel_from_row(pool, &row).await?);
    }
    Ok(channels)
}

fn channel_read_query(predicate: &str, order: Option<&str>) -> String {
    format!(
        "SELECT
            channel.id,
            channel.conversation_id,
            channel.project_id,
            channel.name,
            channel.topic,
            channel.is_default,
            channel.revision,
            channel.archived_at,
            channel.created_at,
            channel.updated_at,
            conversation.last_activity_at,
            (SELECT count(*) FROM chat_conversation_items item
             WHERE item.conversation_id = channel.conversation_id
               AND item.reply_thread_id IS NULL
               AND item.item_kind = 'message') AS message_count,
            (SELECT count(*) FROM chat_conversation_items item
             JOIN chat_communication_messages message ON message.item_id = item.id
             WHERE item.conversation_id = channel.conversation_id
               AND item.reply_thread_id IS NULL
               AND item.item_kind = 'message'
               AND message.author_participant_id != '{LOCAL_PARTICIPANT_ID}'
               AND item.ordinal > coalesce((
                    SELECT cursor.last_read_root_ordinal
                    FROM chat_conversation_read_cursors cursor
                    WHERE cursor.conversation_id = channel.conversation_id
                      AND cursor.participant_id = '{LOCAL_PARTICIPANT_ID}'
               ), 0)) AS unread_count,
            (SELECT revision.normalized_markdown
             FROM chat_conversation_items item
             JOIN chat_communication_messages message ON message.item_id = item.id
             JOIN chat_communication_message_revisions revision
               ON revision.id = message.current_revision_id
             WHERE item.conversation_id = channel.conversation_id
               AND item.reply_thread_id IS NULL
             ORDER BY item.ordinal DESC LIMIT 1) AS latest_preview,
            (SELECT assignment.state
             FROM chat_work_assignments assignment
             JOIN chat_reply_threads thread ON thread.id = assignment.reply_thread_id
             WHERE thread.conversation_id = channel.conversation_id
               AND assignment.state IN (
                    'queued', 'working', 'waiting_for_answer',
                    'waiting_for_approval', 'ready_for_review', 'failed'
               )
             ORDER BY
               CASE assignment.state
                 WHEN 'waiting_for_answer' THEN 1
                 WHEN 'waiting_for_approval' THEN 2
                 WHEN 'ready_for_review' THEN 3
                 WHEN 'failed' THEN 4
                 WHEN 'working' THEN 5
                 ELSE 6
               END,
               assignment.updated_at DESC
             LIMIT 1) AS attention_state
         FROM chat_channels channel
         JOIN chat_conversations conversation ON conversation.id = channel.conversation_id
         WHERE {predicate}{}",
        order
            .map(|value| format!(" ORDER BY {value}"))
            .unwrap_or_default(),
    )
}

async fn channel_from_row(
    pool: &SqlitePool,
    row: &sqlx::sqlite::SqliteRow,
) -> ChatResult<ChatChannelRead> {
    let conversation_id = ChatConversationId::new(
        row.try_get::<String, _>("conversation_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let attention_state = row
        .try_get::<Option<String>, _>("attention_state")
        .map_err(persistence_error)?
        .map(|value| parse_work_state(&value))
        .transpose()?;
    Ok(ChatChannelRead {
        id: ChatChannelId::new(row.try_get::<String, _>("id").map_err(persistence_error)?)
            .map_err(identifier_error)?,
        conversation_id: conversation_id.clone(),
        project_id: row.try_get("project_id").map_err(persistence_error)?,
        name: row.try_get("name").map_err(persistence_error)?,
        topic: row.try_get("topic").map_err(persistence_error)?,
        is_default: row
            .try_get::<i64, _>("is_default")
            .map_err(persistence_error)?
            != 0,
        memberships: super::organization::read_memberships_for_conversation(
            pool,
            &conversation_id,
            false,
        )
        .await?,
        message_count: u64_value(row.try_get("message_count").map_err(persistence_error)?)?,
        unread_count: u64_value(row.try_get("unread_count").map_err(persistence_error)?)?,
        latest_preview: row.try_get("latest_preview").map_err(persistence_error)?,
        last_activity_at: timestamp(row.try_get("last_activity_at").map_err(persistence_error)?)?,
        attention_state,
        revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
        archived_at: optional_timestamp(row.try_get("archived_at").map_err(persistence_error)?)?,
        created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
        updated_at: timestamp(row.try_get("updated_at").map_err(persistence_error)?)?,
    })
}

async fn set_channel_archived(
    app: tauri::AppHandle,
    db_url: String,
    channel_id: ChatChannelId,
    expected_revision: u64,
    archived: bool,
) -> ChatResult<ChatChannelRead> {
    let pool = chat_pool(app, db_url).await?;
    let current = read_channel(&pool, &channel_id).await?;
    if archived && current.is_default {
        return Err(ChatError::validation(
            "channelId",
            "The general channel cannot be archived",
        ));
    }
    let now = now_timestamp()?;
    let archived_at = archived.then(|| now.as_str());
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let updated = sqlx::query(
        "UPDATE chat_channels
         SET archived_at = ?, revision = revision + 1, updated_at = ?
         WHERE id = ? AND revision = ?",
    )
    .bind(archived_at)
    .bind(now.as_str())
    .bind(channel_id.as_str())
    .bind(i64_value(expected_revision)?)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The channel changed before the update",
            true,
        ));
    }
    sqlx::query(
        "UPDATE chat_conversations
         SET archived_at = ?, revision = revision + 1, updated_at = ?
         WHERE id = ?",
    )
    .bind(archived_at)
    .bind(now.as_str())
    .bind(current.conversation_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    read_channel(&pool, &channel_id).await
}

async fn require_active_project(pool: &SqlitePool, project_id: &str) -> ChatResult<()> {
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?;
    match status.as_deref() {
        Some("archived") => Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Restore the project before creating a channel",
            true,
        )),
        Some(_) => Ok(()),
        None => Err(ChatError::new(
            ChatErrorCode::NotFound,
            "The Chat project was not found",
            true,
        )),
    }
}

async fn require_unique_name(
    pool: &SqlitePool,
    project_id: &str,
    name: &str,
    except: Option<&ChatChannelId>,
) -> ChatResult<()> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM chat_channels
            WHERE project_id = ? AND name = ? COLLATE NOCASE
              AND (? IS NULL OR id != ?)
         )",
    )
    .bind(project_id)
    .bind(name)
    .bind(except.map(ChatChannelId::as_str))
    .bind(except.map(ChatChannelId::as_str))
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    if exists != 0 {
        return Err(ChatError::validation(
            "name",
            "A channel with this name already exists in the project",
        ));
    }
    Ok(())
}

fn normalized_channel_name(value: &str) -> ChatResult<String> {
    let normalized = value.trim().trim_start_matches('#').trim().to_lowercase();
    if normalized.is_empty() || normalized.chars().count() > MAX_CHANNEL_NAME_CHARS {
        return Err(ChatError::validation(
            "name",
            "Channel name must contain 1 to 80 characters",
        ));
    }
    if normalized.chars().any(|character| {
        character.is_control() || character.is_whitespace() || matches!(character, '/' | '\\')
    }) {
        return Err(ChatError::validation(
            "name",
            "Channel name cannot contain spaces, control characters, or path separators",
        ));
    }
    Ok(normalized)
}

fn normalized_topic(value: &str) -> ChatResult<String> {
    let normalized = value.trim().to_string();
    if normalized.chars().count() > MAX_CHANNEL_TOPIC_CHARS {
        return Err(ChatError::validation(
            "topic",
            "Channel topic cannot exceed 250 characters",
        ));
    }
    Ok(normalized)
}

fn validate_project_id(value: &str) -> ChatResult<()> {
    if value.trim().is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
        return Err(ChatError::validation("projectId", "Project ID is invalid"));
    }
    Ok(())
}

async fn require_updated(
    pool: &SqlitePool,
    channel_id: &ChatChannelId,
    rows_affected: u64,
) -> ChatResult<()> {
    if rows_affected == 1 {
        return Ok(());
    }
    let exists: i64 = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_channels WHERE id = ?)")
        .bind(channel_id.as_str())
        .fetch_one(pool)
        .await
        .map_err(persistence_error)?;
    Err(if exists == 0 {
        ChatError::new(ChatErrorCode::NotFound, "Chat channel was not found", true)
    } else {
        ChatError::new(
            ChatErrorCode::StaleRevision,
            "The channel changed before the update",
            true,
        )
    })
}

fn parse_work_state(value: &str) -> ChatResult<ChatWorkAssignmentState> {
    match value {
        "queued" => Ok(ChatWorkAssignmentState::Queued),
        "working" => Ok(ChatWorkAssignmentState::Working),
        "waiting_for_answer" => Ok(ChatWorkAssignmentState::WaitingForAnswer),
        "waiting_for_approval" => Ok(ChatWorkAssignmentState::WaitingForApproval),
        "ready_for_review" => Ok(ChatWorkAssignmentState::ReadyForReview),
        "completed" => Ok(ChatWorkAssignmentState::Completed),
        "failed" => Ok(ChatWorkAssignmentState::Failed),
        "cancelled" => Ok(ChatWorkAssignmentState::Cancelled),
        _ => Err(ChatError::new(
            ChatErrorCode::Persistence,
            "Stored assignment state is invalid",
            false,
        )),
    }
}

pub(crate) async fn chat_pool(app: tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app, db_url).await.map_err(|_| {
        ChatError::new(
            ChatErrorCode::Persistence,
            "Chat database is unavailable",
            true,
        )
    })
}

pub(crate) fn now_timestamp() -> ChatResult<UtcTimestamp> {
    timestamp(DateTime::<Utc>::from(SystemTime::now()).to_rfc3339_opts(SecondsFormat::Millis, true))
}

pub(crate) fn timestamp(value: String) -> ChatResult<UtcTimestamp> {
    UtcTimestamp::new(value).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Persistence,
            "Stored Chat timestamp is invalid",
            false,
        )
    })
}

pub(crate) fn optional_timestamp(value: Option<String>) -> ChatResult<Option<UtcTimestamp>> {
    value.map(timestamp).transpose()
}

pub(crate) fn i64_value(value: u64) -> ChatResult<i64> {
    i64::try_from(value).map_err(|_| ChatError::validation("revision", "Revision is too large"))
}

pub(crate) fn u64_value(value: i64) -> ChatResult<u64> {
    u64::try_from(value).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Persistence,
            "Stored Chat number is invalid",
            false,
        )
    })
}

pub(crate) fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat data could not be saved or read",
        true,
    )
}

pub(crate) fn identifier_error(_error: String) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat identifier is invalid",
        false,
    )
}

fn new_id(prefix: &str) -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}:{nanos:032x}{sequence:016x}")
}
