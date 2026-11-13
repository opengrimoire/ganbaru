//! Organizational project channels.

use crate::db;
use chrono::{DateTime, SecondsFormat, Utc};
use ganbaru_chat::conversion::{conversation_cursor_key, membership_key};
use ganbaru_chat::ids::new_id;
use ganbaru_chat::project_defaults;
use ganbaru_chat_contracts::models::{
    ChatChannelId, ChatChannelRead, ChatConversationId, ChatError, ChatErrorCode, ChatResult,
    ChatWorkAssignmentState, UtcTimestamp,
};
use serde::Deserialize;
use sqlx::{AssertSqlSafe, Row, SqlitePool};
use std::time::SystemTime;

const LOCAL_PARTICIPANT_ID: &str = "participant:local-owner";
const MAX_CHANNEL_NAME_CHARS: usize = 80;
const MAX_CHANNEL_TOPIC_CHARS: usize = 250;
const MAX_CHANNEL_RESULTS: u32 = 500;

/// Days a failed assignment keeps asking for attention after it settled. Failures have no
/// review outcome, so they age out instead; Retry and a successor end them sooner, and the
/// reply thread keeps the failure in its history.
pub(crate) const REVIEW_QUEUE_FAILURE_WINDOW_DAYS: i64 = 7;

/// States of an assignment that waits for the person, in attention priority order.
pub(crate) const PERSON_ATTENTION_STATES_SQL: &str =
    "'waiting_for_answer', 'waiting_for_approval', 'ready_for_review', 'failed'";

/// Whether the assignment aliased `assignment` still asks for attention: no later assignment
/// continues it, and a failure settled at or after the failure window start, the one bound
/// parameter, unless it is delegated work whose parent is still open, since that parent
/// receives the failure as its outcome and decides what follows.
pub(crate) const ASSIGNMENT_ATTENTION_PREDICATE_SQL: &str = "NOT EXISTS (
        SELECT 1 FROM chat_work_assignments successor
        WHERE successor.previous_assignment_id = assignment.id
    )
    AND (
        assignment.state <> 'failed'
        OR (
            coalesce(assignment.settled_at, assignment.updated_at) >= ?
            AND NOT EXISTS (
                SELECT 1 FROM chat_work_assignments parent
                WHERE parent.id = assignment.parent_assignment_id
                  AND parent.state IN (
                      'queued', 'working', 'waiting_for_answer', 'waiting_for_approval'
                  )
            )
        )
    )";

/// Attention priority of the assignment aliased `assignment`: answer, approval, review,
/// failure, working, then queued.
pub(crate) const ASSIGNMENT_ATTENTION_PRIORITY_SQL: &str = "CASE assignment.state
        WHEN 'waiting_for_answer' THEN 1
        WHEN 'waiting_for_approval' THEN 2
        WHEN 'ready_for_review' THEN 3
        WHEN 'failed' THEN 4
        WHEN 'working' THEN 5
        ELSE 6
    END";

/// The earliest settle time at which a failure still asks for attention at `now`, in the stored
/// timestamp format so it compares as text.
pub(crate) fn failure_window_start(now: DateTime<Utc>) -> String {
    (now - chrono::Duration::days(REVIEW_QUEUE_FAILURE_WINDOW_DAYS))
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// The failure window start at the current time.
pub(crate) fn current_failure_window_start() -> String {
    failure_window_start(DateTime::<Utc>::from(SystemTime::now()))
}

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
    let pool = chat_pool(app, db_url).await?;
    ensure_chat_defaults(&pool, ChatDefaultsScope::Project(&project_id)).await?;
    read_channels(&pool, Some(&project_id), archived).await
}

#[tauri::command]
pub async fn chat_list_navigation_channels(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<Vec<ChatChannelRead>> {
    let pool = chat_pool(app, db_url).await?;
    ensure_chat_defaults(&pool, ChatDefaultsScope::AllProjects).await?;
    read_channels(&pool, None, false).await
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
        ChatConversationId::new(new_id("conversation")?).map_err(identifier_error)?;
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
            (conversation_id, participant_id, sync_key, membership_role, created_at, updated_at)
         VALUES (?, ?, ?, 'owner', ?, ?)",
    )
    .bind(conversation_id.as_str())
    .bind(LOCAL_PARTICIPANT_ID)
    .bind(sync_key(membership_key(
        conversation_id.as_str(),
        LOCAL_PARTICIPANT_ID,
    ))?)
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
    if read {
        mark_conversation_read(&pool, &channel.conversation_id, LOCAL_PARTICIPANT_ID, &now).await?;
    } else {
        mark_conversation_unread(&pool, &channel.conversation_id, LOCAL_PARTICIPANT_ID, &now)
            .await?;
    }
    read_channel(&pool, &channel_id).await
}

/// Advances a participant's read cursor of a conversation to its last root item.
///
/// The cursor never moves backward and the row is not rewritten when the position does not
/// grow, because sync merges read cursors as a maximum.
pub(crate) async fn mark_conversation_read(
    pool: &SqlitePool,
    conversation_id: &ChatConversationId,
    participant_id: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let last = sqlx::query_as::<_, (String, String)>(
        "SELECT order_key, id FROM chat_conversation_items
         WHERE conversation_id = ? AND reply_thread_id IS NULL
         ORDER BY order_key DESC, id DESC LIMIT 1",
    )
    .bind(conversation_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    let Some((order_key, item_id)) = last else {
        return Ok(());
    };
    sqlx::query(
        "INSERT INTO chat_conversation_read_cursors
            (conversation_id, participant_id, sync_key, last_read_order_key,
             last_read_item_id, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(conversation_id, participant_id) DO UPDATE SET
            last_read_order_key = excluded.last_read_order_key,
            last_read_item_id = excluded.last_read_item_id,
            updated_at = excluded.updated_at
         WHERE (excluded.last_read_order_key, excluded.last_read_item_id)
             > (last_read_order_key, last_read_item_id)",
    )
    .bind(conversation_id.as_str())
    .bind(participant_id)
    .bind(sync_key(conversation_cursor_key(
        conversation_id.as_str(),
        participant_id,
    ))?)
    .bind(order_key)
    .bind(item_id)
    .bind(now.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// Resets a participant's read cursor of a conversation to nothing read.
///
/// This is the one write that moves a cursor backward. Sync merges read cursors as a maximum,
/// so a device that has read further keeps its position, and a later read on any device
/// replaces the reset.
async fn mark_conversation_unread(
    pool: &SqlitePool,
    conversation_id: &ChatConversationId,
    participant_id: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    sqlx::query(
        "INSERT INTO chat_conversation_read_cursors
            (conversation_id, participant_id, sync_key, last_read_order_key,
             last_read_item_id, updated_at)
         VALUES (?, ?, ?, '', '', ?)
         ON CONFLICT(conversation_id, participant_id) DO UPDATE SET
            last_read_order_key = '',
            last_read_item_id = '',
            updated_at = excluded.updated_at",
    )
    .bind(conversation_id.as_str())
    .bind(participant_id)
    .bind(sync_key(conversation_cursor_key(
        conversation_id.as_str(),
        participant_id,
    ))?)
    .bind(now.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

pub(crate) async fn read_channel(
    pool: &SqlitePool,
    channel_id: &ChatChannelId,
) -> ChatResult<ChatChannelRead> {
    let row = sqlx::query(AssertSqlSafe(channel_read_query("channel.id = ?", None)))
        .bind(current_failure_window_start())
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
    let query = channel_read_query(
        &format!("{archive_clause}{project_clause}"),
        Some("channel.is_default DESC, channel.name COLLATE NOCASE, channel.id LIMIT ?"),
    );
    let mut prepared =
        sqlx::query(AssertSqlSafe(query.as_str())).bind(current_failure_window_start());
    if let Some(project_id) = project_id {
        prepared = prepared.bind(project_id);
    }
    let rows = prepared
        .bind(i64::from(MAX_CHANNEL_RESULTS))
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
        "{}
         SELECT
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
               AND item.item_kind = 'message'
               AND NOT EXISTS (
                    SELECT 1 FROM chat_communication_messages message
                    WHERE message.item_id = item.id AND message.deleted_at IS NOT NULL
               )) AS message_count,
            (SELECT count(*) FROM chat_conversation_items item
             JOIN chat_communication_messages message ON message.item_id = item.id
             WHERE item.conversation_id = channel.conversation_id
               AND item.reply_thread_id IS NULL
               AND item.item_kind = 'message'
               AND message.deleted_at IS NULL
               AND message.author_participant_id IS NOT '{LOCAL_PARTICIPANT_ID}'
               AND NOT EXISTS (
                    SELECT 1 FROM chat_conversation_read_cursors cursor
                    WHERE cursor.conversation_id = channel.conversation_id
                      AND cursor.participant_id = '{LOCAL_PARTICIPANT_ID}'
                      AND (cursor.last_read_order_key, cursor.last_read_item_id)
                          >= (item.order_key, item.id)
               )) AS unread_count,
            (SELECT revision.normalized_markdown
             FROM chat_conversation_items item
             JOIN chat_communication_messages message ON message.item_id = item.id
             JOIN chat_communication_message_revisions revision
               ON revision.id = message.current_revision_id
             WHERE item.conversation_id = channel.conversation_id
               AND item.reply_thread_id IS NULL
               AND message.deleted_at IS NULL
             ORDER BY item.order_key DESC, item.id DESC LIMIT 1) AS latest_preview,
            attention.state AS attention_state
         FROM chat_channels channel
         JOIN chat_conversations conversation ON conversation.id = channel.conversation_id
         LEFT JOIN open_assignment_attention attention
           ON attention.conversation_id = channel.conversation_id AND attention.attention_rank = 1
         WHERE {predicate}{}",
        open_assignment_attention_cte(),
        order
            .map(|value| format!(" ORDER BY {value}"))
            .unwrap_or_default(),
    )
}

/// The `open_assignment_attention` common table expression: every open assignment that still
/// asks for attention, ranked per conversation by priority, then newest update, then id, so
/// rank 1 is the channel's attention state. It binds the failure window start.
fn open_assignment_attention_cte() -> String {
    format!(
        "WITH open_assignment_attention AS (
            SELECT conversation_id, state,
                   ROW_NUMBER() OVER (
                       PARTITION BY conversation_id
                       ORDER BY priority, updated_at DESC, id
                   ) AS attention_rank
            FROM (
                SELECT thread.conversation_id, assignment.state, assignment.updated_at,
                       assignment.id, {ASSIGNMENT_ATTENTION_PRIORITY_SQL} AS priority
                FROM chat_work_assignments assignment
                JOIN chat_reply_threads thread ON thread.id = assignment.reply_thread_id
                WHERE assignment.state IN ('queued', 'working', {PERSON_ATTENTION_STATES_SQL})
                  AND {ASSIGNMENT_ATTENTION_PREDICATE_SQL}
            )
         )"
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

/// Rejects a project id that is empty, too long, or holds control characters.
pub(crate) fn validate_project_id(value: &str) -> ChatResult<()> {
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

/// Projects whose Chat defaults a command creates before it reads. A missing project or working
/// folder leaves nothing to create.
pub(crate) enum ChatDefaultsScope<'a> {
    /// One project.
    Project(&'a str),
    /// The project that owns a working folder.
    #[cfg(desktop)]
    WorkingFolder(&'a str),
    /// Every project.
    AllProjects,
}

/// Creates missing default channels and current-folder execution environments for `scope`.
pub(crate) async fn ensure_chat_defaults(
    pool: &SqlitePool,
    scope: ChatDefaultsScope<'_>,
) -> ChatResult<()> {
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let ensured = match scope {
        ChatDefaultsScope::Project(project_id) => {
            project_defaults::ensure_project_chat_defaults(&mut transaction, project_id, &now).await
        }
        #[cfg(desktop)]
        ChatDefaultsScope::WorkingFolder(working_folder_id) => {
            project_defaults::ensure_working_folder_chat_defaults(
                &mut transaction,
                working_folder_id,
                &now,
            )
            .await
        }
        ChatDefaultsScope::AllProjects => {
            project_defaults::ensure_all_project_chat_defaults(&mut transaction, &now).await
        }
    };
    match ensured {
        Ok(()) => transaction.commit().await.map_err(persistence_error),
        Err(error) if error.code == ChatErrorCode::NotFound => Ok(()),
        Err(error) => Err(error),
    }
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

/// Maps a failed `sync_key` derivation, which only an id outside the row key bounds causes.
pub(crate) fn sync_key(derived: Result<String, String>) -> ChatResult<String> {
    derived.map_err(|_| {
        ChatError::new(
            ChatErrorCode::Validation,
            "Chat identifiers are too long to save",
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

#[cfg(test)]
mod tests {
    use super::{LOCAL_PARTICIPANT_ID, mark_conversation_read, read_channel, timestamp};
    use ganbaru_chat_contracts::models::{ChatChannelId, ChatConversationId};

    const NOW: &str = "2026-10-20T12:00:00.000Z";
    const LATER: &str = "2026-10-20T12:05:00.000Z";

    async fn project_channel(pool: &sqlx::SqlitePool) -> (ChatChannelId, ChatConversationId) {
        let (channel_id, conversation_id): (String, String) = sqlx::query_as(
            "SELECT id, conversation_id FROM chat_channels WHERE project_id = 'project-chat'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        (
            ChatChannelId::new(channel_id).unwrap(),
            ChatConversationId::new(conversation_id).unwrap(),
        )
    }

    async fn seed_teammate_messages(
        pool: &sqlx::SqlitePool,
        conversation_id: &ChatConversationId,
        messages: &[(&str, &str, bool)],
    ) {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "INSERT OR IGNORE INTO chat_participants
                (id, participant_kind, display_name, created_at, updated_at)
             VALUES ('participant:teammate', 'ai_teammate', 'Teammate', '{NOW}', '{NOW}');"
        )))
        .execute(pool)
        .await
        .unwrap();
        for (item_id, order_key, deleted) in messages {
            let deleted_at = deleted.then_some(NOW);
            let revision_id = format!("revision:{item_id}");
            sqlx::query(
                "INSERT INTO chat_conversation_items
                    (id, conversation_id, item_kind, order_key, created_at)
                 VALUES (?, ?, 'message', ?, ?)",
            )
            .bind(item_id)
            .bind(conversation_id.as_str())
            .bind(order_key)
            .bind(NOW)
            .execute(pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO chat_communication_messages
                    (item_id, author_participant_id, deleted_at, created_at)
                 VALUES (?, 'participant:teammate', ?, ?)",
            )
            .bind(item_id)
            .bind(deleted_at)
            .bind(NOW)
            .execute(pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO chat_communication_message_revisions
                    (id, message_item_id, normalized_markdown, created_at)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(&revision_id)
            .bind(item_id)
            .bind(format!("Text of {item_id}"))
            .bind(NOW)
            .execute(pool)
            .await
            .unwrap();
            sqlx::query(
                "UPDATE chat_communication_messages SET current_revision_id = ? WHERE item_id = ?",
            )
            .bind(&revision_id)
            .bind(item_id)
            .execute(pool)
            .await
            .unwrap();
        }
    }

    async fn local_cursor(
        pool: &sqlx::SqlitePool,
        conversation_id: &ChatConversationId,
    ) -> (String, String, String) {
        sqlx::query_as(
            "SELECT last_read_order_key, last_read_item_id, updated_at
             FROM chat_conversation_read_cursors
             WHERE conversation_id = ? AND participant_id = ?",
        )
        .bind(conversation_id.as_str())
        .bind(LOCAL_PARTICIPANT_ID)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn channel_summaries_leave_out_deleted_messages() {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        let (channel_id, conversation_id) = project_channel(&pool).await;
        seed_teammate_messages(
            &pool,
            &conversation_id,
            &[("item:kept", "c000", false), ("item:deleted", "c001", true)],
        )
        .await;

        let channel = read_channel(&pool, &channel_id).await.unwrap();

        assert_eq!(channel.latest_preview.as_deref(), Some("Text of item:kept"));
        assert_eq!(channel.message_count, 1);
        assert_eq!(channel.unread_count, 1);
    }

    #[tokio::test]
    async fn unread_counts_break_equal_order_keys_by_item_id() {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        let (channel_id, conversation_id) = project_channel(&pool).await;
        seed_teammate_messages(
            &pool,
            &conversation_id,
            &[("item:a", "c000", false), ("item:b", "c000", false)],
        )
        .await;
        sqlx::query(
            "INSERT INTO chat_conversation_read_cursors
                (conversation_id, participant_id, sync_key, last_read_order_key,
                 last_read_item_id, updated_at)
             VALUES (?, ?, 'cursor-test', 'c000', 'item:a', ?)",
        )
        .bind(conversation_id.as_str())
        .bind(LOCAL_PARTICIPANT_ID)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();

        let channel = read_channel(&pool, &channel_id).await.unwrap();

        assert_eq!(channel.unread_count, 1);
        assert_eq!(channel.latest_preview.as_deref(), Some("Text of item:b"));
    }

    #[tokio::test]
    async fn read_cursor_never_moves_backward_or_rewrites_an_unchanged_position() {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        let (channel_id, conversation_id) = project_channel(&pool).await;
        seed_teammate_messages(
            &pool,
            &conversation_id,
            &[("item:a", "c000", false), ("item:b", "c000", false)],
        )
        .await;
        let now = timestamp(NOW.to_owned()).unwrap();
        let later = timestamp(LATER.to_owned()).unwrap();

        mark_conversation_read(&pool, &conversation_id, LOCAL_PARTICIPANT_ID, &now)
            .await
            .unwrap();
        assert_eq!(
            local_cursor(&pool, &conversation_id).await,
            ("c000".to_owned(), "item:b".to_owned(), NOW.to_owned())
        );

        mark_conversation_read(&pool, &conversation_id, LOCAL_PARTICIPANT_ID, &later)
            .await
            .unwrap();
        assert_eq!(
            local_cursor(&pool, &conversation_id).await,
            ("c000".to_owned(), "item:b".to_owned(), NOW.to_owned())
        );

        sqlx::query(
            "UPDATE chat_conversation_read_cursors
             SET last_read_order_key = 'c900', last_read_item_id = 'item:remote'
             WHERE conversation_id = ?",
        )
        .bind(conversation_id.as_str())
        .execute(&pool)
        .await
        .unwrap();
        mark_conversation_read(&pool, &conversation_id, LOCAL_PARTICIPANT_ID, &later)
            .await
            .unwrap();
        assert_eq!(
            local_cursor(&pool, &conversation_id).await,
            ("c900".to_owned(), "item:remote".to_owned(), NOW.to_owned())
        );
        assert_eq!(
            read_channel(&pool, &channel_id).await.unwrap().unread_count,
            0
        );
    }

    mod attention {
        use super::super::{read_channel, read_channels};
        use crate::chat::tests::attention::{
            SeedAssignment, days_ago, seed_assignment, seed_channel, seed_reply_thread,
            seed_teammate,
        };
        use ganbaru_chat_contracts::models::{ChatChannelId, ChatWorkAssignmentState};

        const TEAMMATE: &str = "participant:attention-agent";

        /// A channel of the seeded project with `threads` reply threads and a teammate.
        async fn channel_with_threads(
            pool: &sqlx::SqlitePool,
            channel_id: &str,
            threads: &[&str],
        ) -> ChatChannelId {
            let conversation_id =
                seed_channel(pool, "project-chat", channel_id, channel_id, false).await;
            for thread in threads {
                seed_reply_thread(pool, &conversation_id, thread).await;
            }
            ChatChannelId::new(channel_id).unwrap()
        }

        async fn attention_of(
            pool: &sqlx::SqlitePool,
            channel_id: &ChatChannelId,
        ) -> Option<ChatWorkAssignmentState> {
            read_channel(pool, channel_id)
                .await
                .unwrap()
                .attention_state
        }

        #[tokio::test]
        async fn attention_prefers_answer_over_approval_over_review_over_failure() {
            let pool = crate::chat::tests::repository::pool_with_thread().await;
            seed_teammate(&pool, TEAMMATE, "Attention agent").await;
            let channel = channel_with_threads(
                &pool,
                "channel:priority",
                &["thread:a", "thread:b", "thread:c", "thread:d"],
            )
            .await;
            let mut failed =
                SeedAssignment::new("assignment:d", "thread:d", TEAMMATE, "failed", 40);
            failed.settled_at = Some(days_ago(1));
            seed_assignment(&pool, &failed).await;
            assert_eq!(
                attention_of(&pool, &channel).await,
                Some(ChatWorkAssignmentState::Failed)
            );

            let expected = [
                (
                    "assignment:c",
                    "thread:c",
                    "ready_for_review",
                    ChatWorkAssignmentState::ReadyForReview,
                ),
                (
                    "assignment:b",
                    "thread:b",
                    "waiting_for_approval",
                    ChatWorkAssignmentState::WaitingForApproval,
                ),
                (
                    "assignment:a",
                    "thread:a",
                    "waiting_for_answer",
                    ChatWorkAssignmentState::WaitingForAnswer,
                ),
            ];
            for (id, thread, state, attention) in expected {
                seed_assignment(&pool, &SeedAssignment::new(id, thread, TEAMMATE, state, 1)).await;
                assert_eq!(attention_of(&pool, &channel).await, Some(attention));
            }
        }

        #[tokio::test]
        async fn a_failure_continued_by_a_later_assignment_asks_for_nothing() {
            let pool = crate::chat::tests::repository::pool_with_thread().await;
            seed_teammate(&pool, TEAMMATE, "Attention agent").await;
            let channel = channel_with_threads(&pool, "channel:successor", &["thread:a"]).await;
            let mut failed =
                SeedAssignment::new("assignment:failed", "thread:a", TEAMMATE, "failed", 1);
            failed.settled_at = Some(days_ago(1));
            seed_assignment(&pool, &failed).await;
            let mut successor =
                SeedAssignment::new("assignment:successor", "thread:a", TEAMMATE, "completed", 2);
            successor.previous_assignment_id = Some("assignment:failed");
            seed_assignment(&pool, &successor).await;

            assert_eq!(attention_of(&pool, &channel).await, None);
        }

        #[tokio::test]
        async fn a_failure_stops_asking_for_attention_after_the_window() {
            let pool = crate::chat::tests::repository::pool_with_thread().await;
            seed_teammate(&pool, TEAMMATE, "Attention agent").await;
            let old = channel_with_threads(&pool, "channel:old", &["thread:old"]).await;
            let recent = channel_with_threads(&pool, "channel:recent", &["thread:recent"]).await;
            let mut eight_days =
                SeedAssignment::new("assignment:old", "thread:old", TEAMMATE, "failed", 1);
            eight_days.settled_at = Some(days_ago(8));
            seed_assignment(&pool, &eight_days).await;
            let mut six_days =
                SeedAssignment::new("assignment:recent", "thread:recent", TEAMMATE, "failed", 1);
            six_days.settled_at = Some(days_ago(6));
            seed_assignment(&pool, &six_days).await;

            assert_eq!(attention_of(&pool, &old).await, None);
            assert_eq!(
                attention_of(&pool, &recent).await,
                Some(ChatWorkAssignmentState::Failed)
            );
        }

        #[tokio::test]
        async fn a_delegated_failure_waits_on_its_open_parent() {
            let pool = crate::chat::tests::repository::pool_with_thread().await;
            seed_teammate(&pool, TEAMMATE, "Attention agent").await;
            let parent_channel =
                channel_with_threads(&pool, "channel:parent", &["thread:parent"]).await;
            let child_channel =
                channel_with_threads(&pool, "channel:child", &["thread:child"]).await;
            seed_assignment(
                &pool,
                &SeedAssignment::new("assignment:parent", "thread:parent", TEAMMATE, "working", 1),
            )
            .await;
            let mut child =
                SeedAssignment::new("assignment:child", "thread:child", TEAMMATE, "failed", 2);
            child.settled_at = Some(days_ago(1));
            child.parent_assignment_id = Some("assignment:parent");
            seed_assignment(&pool, &child).await;

            assert_eq!(
                attention_of(&pool, &parent_channel).await,
                Some(ChatWorkAssignmentState::Working)
            );
            assert_eq!(
                attention_of(&pool, &child_channel).await,
                None,
                "the parent receives the failure as its outcome"
            );

            sqlx::query(
                "UPDATE chat_work_assignments SET state = 'completed', settled_at = ?
                 WHERE id = 'assignment:parent'",
            )
            .bind(days_ago(0))
            .execute(&pool)
            .await
            .unwrap();
            assert_eq!(
                attention_of(&pool, &child_channel).await,
                Some(ChatWorkAssignmentState::Failed),
                "a settled parent no longer handles the failure"
            );
        }

        #[tokio::test]
        async fn many_assignments_read_as_one_row_per_channel() {
            let pool = crate::chat::tests::repository::pool_with_thread().await;
            seed_teammate(&pool, TEAMMATE, "Attention agent").await;
            for (channel, state) in [
                ("channel:busy-a", "ready_for_review"),
                ("channel:busy-b", "working"),
            ] {
                let conversation_id = sqlx::query_scalar::<_, String>(
                    "SELECT conversation_id FROM chat_channels WHERE id = ?",
                )
                .bind(channel_with_threads(&pool, channel, &[]).await.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
                for index in 0..300 {
                    let thread = format!("thread:{channel}:{index}");
                    seed_reply_thread(&pool, &conversation_id, &thread).await;
                    let id = format!("assignment:{channel}:{index}");
                    let state = if index == 150 { state } else { "completed" };
                    seed_assignment(
                        &pool,
                        &SeedAssignment::new(&id, &thread, TEAMMATE, state, index),
                    )
                    .await;
                }
            }

            let channels = read_channels(&pool, Some("project-chat"), false)
                .await
                .unwrap();

            let busy = channels
                .iter()
                .filter(|channel| channel.id.as_str().starts_with("channel:busy"))
                .map(|channel| (channel.id.as_str(), channel.attention_state))
                .collect::<Vec<_>>();
            assert_eq!(
                busy,
                [
                    (
                        "channel:busy-a",
                        Some(ChatWorkAssignmentState::ReadyForReview)
                    ),
                    ("channel:busy-b", Some(ChatWorkAssignmentState::Working)),
                ]
            );
            assert_eq!(
                channels.len(),
                3,
                "the default channel and the two busy channels"
            );
        }
    }
}
