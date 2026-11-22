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

/// States of an assignment that a channel row marks with its attention state. Failures are not
/// among them: a channel marks a failure only until it is seen, through `unseen_failure`.
const CHANNEL_ATTENTION_STATES_SQL: &str =
    "'queued', 'working', 'waiting_for_answer', 'waiting_for_approval', 'ready_for_review'";

/// Whether the reply thread aliased `thread` ends in a failure the local person has not read in
/// that thread: its current assignment failed, nothing continues it, delegated work's open parent
/// is not deciding what follows, and the failure message, aliased `failure_item`, lies after the
/// thread's read cursor. `seen_elsewhere` adds further conditions on `failure_item`, starting
/// with `AND`, or is empty.
pub(crate) fn unseen_thread_failure_sql(seen_elsewhere: &str) -> String {
    format!(
        "EXISTS (
            SELECT 1 FROM chat_work_assignments assignment
            JOIN chat_work_semantic_updates failure
              ON failure.assignment_id = assignment.id AND failure.update_kind = 'failure'
            JOIN chat_conversation_items failure_item ON failure_item.id = failure.item_id
            WHERE assignment.reply_thread_id = thread.id
              AND assignment.state = 'failed'
              AND NOT EXISTS (
                  SELECT 1 FROM chat_work_assignments successor
                  WHERE successor.previous_assignment_id = assignment.id
              )
              AND NOT EXISTS (
                  SELECT 1 FROM chat_work_assignments parent
                  WHERE parent.id = assignment.parent_assignment_id
                    AND parent.state IN (
                        'queued', 'working', 'waiting_for_answer', 'waiting_for_approval'
                    )
              )
              AND NOT EXISTS (
                  SELECT 1 FROM chat_reply_thread_read_cursors thread_cursor
                  WHERE thread_cursor.reply_thread_id = thread.id
                    AND thread_cursor.participant_id = '{LOCAL_PARTICIPANT_ID}'
                    AND (thread_cursor.last_read_order_key, thread_cursor.last_read_item_id)
                        >= (failure_item.order_key, failure_item.id)
              )
              {seen_elsewhere}
        )"
    )
}

/// The time the local person last opened the conversation aliased `channel`, or an empty string
/// when they never did. Opening a channel stamps its read cursor even when no top-level message
/// arrived, so replies and failures posted before then count as seen.
fn channel_seen_at_sql() -> String {
    format!(
        "coalesce((
            SELECT seen.updated_at FROM chat_conversation_read_cursors seen
            WHERE seen.conversation_id = channel.conversation_id
              AND seen.participant_id = '{LOCAL_PARTICIPANT_ID}'
        ), '')"
    )
}

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

/// Advances a participant's read cursor of a conversation to its last root item and stamps it
/// with `now`, the time the conversation was last seen.
///
/// The cursor never moves backward, because sync merges read cursors as a maximum. The row is
/// rewritten only when the position grows or a reply arrived after the last stamp, so opening a
/// quiet channel writes nothing.
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
            last_read_order_key = CASE
                WHEN (excluded.last_read_order_key, excluded.last_read_item_id)
                    > (last_read_order_key, last_read_item_id)
                THEN excluded.last_read_order_key ELSE last_read_order_key END,
            last_read_item_id = CASE
                WHEN (excluded.last_read_order_key, excluded.last_read_item_id)
                    > (last_read_order_key, last_read_item_id)
                THEN excluded.last_read_item_id ELSE last_read_item_id END,
            updated_at = max(updated_at, excluded.updated_at)
         WHERE (excluded.last_read_order_key, excluded.last_read_item_id)
             > (last_read_order_key, last_read_item_id)
            OR EXISTS (
                SELECT 1 FROM chat_conversation_items reply
                WHERE reply.conversation_id = excluded.conversation_id
                  AND reply.reply_thread_id IS NOT NULL
                  AND reply.created_at > chat_conversation_read_cursors.updated_at
            )",
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
    let seen_at = channel_seen_at_sql();
    let unseen_failure =
        unseen_thread_failure_sql(&format!("AND failure_item.created_at > {seen_at}"));
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
            (SELECT count(*) FROM chat_conversation_items item
             JOIN chat_communication_messages message ON message.item_id = item.id
             JOIN chat_reply_threads thread ON thread.id = item.reply_thread_id
             WHERE item.conversation_id = channel.conversation_id
               AND item.item_kind = 'message'
               AND message.deleted_at IS NULL
               AND message.author_participant_id IS NOT '{LOCAL_PARTICIPANT_ID}'
               AND item.created_at > {seen_at}
               AND NOT EXISTS (
                    SELECT 1 FROM chat_reply_thread_read_cursors thread_cursor
                    WHERE thread_cursor.reply_thread_id = thread.id
                      AND thread_cursor.participant_id = '{LOCAL_PARTICIPANT_ID}'
                      AND (thread_cursor.last_read_order_key, thread_cursor.last_read_item_id)
                          >= (item.order_key, item.id)
               )
               AND EXISTS (
                    SELECT 1 FROM chat_conversation_items own
                    JOIN chat_communication_messages own_message ON own_message.item_id = own.id
                    WHERE (own.id = thread.root_item_id OR own.reply_thread_id = thread.id)
                      AND own_message.author_participant_id = '{LOCAL_PARTICIPANT_ID}'
               )) AS unread_reply_count,
            EXISTS (
                SELECT 1 FROM chat_reply_threads thread
                WHERE thread.conversation_id = channel.conversation_id
                  AND {unseen_failure}
            ) AS unseen_failure,
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
                WHERE assignment.state IN ({CHANNEL_ATTENTION_STATES_SQL})
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
        unread_reply_count: u64_value(
            row.try_get("unread_reply_count")
                .map_err(persistence_error)?,
        )?,
        unseen_failure: row
            .try_get::<i64, _>("unseen_failure")
            .map_err(persistence_error)?
            != 0,
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
    ChatWorkAssignmentState::parse(value).ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::Persistence,
            "Stored assignment state is invalid",
            false,
        )
    })
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
mod tests;
