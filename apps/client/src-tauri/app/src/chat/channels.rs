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

    async fn seed_agent_messages(
        pool: &sqlx::SqlitePool,
        conversation_id: &ChatConversationId,
        messages: &[(&str, &str, bool)],
    ) {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "INSERT OR IGNORE INTO chat_participants
                (id, participant_kind, display_name, created_at, updated_at)
             VALUES ('participant:agent', 'ai_agent', 'Agent', '{NOW}', '{NOW}');"
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
                 VALUES (?, 'participant:agent', ?, ?)",
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
        seed_agent_messages(
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
        seed_agent_messages(
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
        seed_agent_messages(
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
        use super::super::{
            LOCAL_PARTICIPANT_ID, mark_conversation_read, read_channel, read_channels, timestamp,
        };
        use crate::chat::tests::attention::{
            SeedAssignment, days_ago, minutes_after_seed, seed_agent, seed_assignment,
            seed_channel, seed_failure_update, seed_reply, seed_reply_thread,
            seed_reply_thread_cursor,
        };
        use ganbaru_chat_contracts::models::{
            ChatChannelId, ChatConversationId, ChatReplyThreadId, ChatReplyThreadSummaryRead,
            ChatWorkAssignmentState,
        };

        const AGENT: &str = "participant:attention-agent";

        /// Opens the channel `channel_id` at `minutes` after the seed time.
        async fn open_channel_at(
            pool: &sqlx::SqlitePool,
            channel_id: &ChatChannelId,
            minutes: i64,
        ) {
            let conversation_id = read_channel(pool, channel_id)
                .await
                .unwrap()
                .conversation_id;
            mark_conversation_read(
                pool,
                &conversation_id,
                LOCAL_PARTICIPANT_ID,
                &timestamp(minutes_after_seed(minutes)).unwrap(),
            )
            .await
            .unwrap();
        }

        async fn unseen_failure_of(pool: &sqlx::SqlitePool, channel_id: &ChatChannelId) -> bool {
            read_channel(pool, channel_id).await.unwrap().unseen_failure
        }

        async fn unread_replies_of(pool: &sqlx::SqlitePool, channel_id: &ChatChannelId) -> u64 {
            read_channel(pool, channel_id)
                .await
                .unwrap()
                .unread_reply_count
        }

        /// A failed assignment in `thread` with its failure message `item_id`, posted
        /// `minutes` after the seed time.
        async fn seed_failure(
            pool: &sqlx::SqlitePool,
            assignment: &SeedAssignment<'_>,
            item_id: &str,
            order_key: &str,
            minutes: i64,
        ) {
            seed_assignment(pool, assignment).await;
            seed_failure_update(
                pool,
                assignment.reply_thread_id,
                assignment.id,
                AGENT,
                item_id,
                order_key,
                &minutes_after_seed(minutes),
            )
            .await;
        }

        /// A channel of the seeded project with `threads` reply threads and an agent.
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

        async fn thread_summary(
            pool: &sqlx::SqlitePool,
            reply_thread_id: &str,
        ) -> ChatReplyThreadSummaryRead {
            crate::chat::organization::read_reply_thread_summary(
                pool,
                &ChatReplyThreadId::new(reply_thread_id).unwrap(),
            )
            .await
            .unwrap()
        }

        /// The time the local read cursor of `conversation_id` was last stamped.
        async fn cursor_stamp(
            pool: &sqlx::SqlitePool,
            conversation_id: &ChatConversationId,
        ) -> String {
            sqlx::query_scalar(
                "SELECT updated_at FROM chat_conversation_read_cursors
                 WHERE conversation_id = ? AND participant_id = ?",
            )
            .bind(conversation_id.as_str())
            .bind(LOCAL_PARTICIPANT_ID)
            .fetch_one(pool)
            .await
            .unwrap()
        }

        #[tokio::test]
        async fn attention_prefers_answer_over_approval_over_review_and_leaves_out_failures() {
            let pool = crate::chat::tests::repository::pool_with_thread().await;
            seed_agent(&pool, AGENT, "Attention agent").await;
            let channel = channel_with_threads(
                &pool,
                "channel:priority",
                &["thread:a", "thread:b", "thread:c", "thread:d"],
            )
            .await;
            let mut failed = SeedAssignment::new("assignment:d", "thread:d", AGENT, "failed", 40);
            failed.settled_at = Some(days_ago(1));
            seed_failure(&pool, &failed, "item:failure:d", "a1", 40).await;
            assert_eq!(attention_of(&pool, &channel).await, None);
            assert!(unseen_failure_of(&pool, &channel).await);

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
                seed_assignment(&pool, &SeedAssignment::new(id, thread, AGENT, state, 1)).await;
                assert_eq!(attention_of(&pool, &channel).await, Some(attention));
            }
        }

        #[tokio::test]
        async fn a_failure_continued_by_a_later_assignment_is_not_marked() {
            let pool = crate::chat::tests::repository::pool_with_thread().await;
            seed_agent(&pool, AGENT, "Attention agent").await;
            let channel = channel_with_threads(&pool, "channel:successor", &["thread:a"]).await;
            let failed = SeedAssignment::new("assignment:failed", "thread:a", AGENT, "failed", 1);
            seed_failure(&pool, &failed, "item:failure", "a1", 1).await;
            assert!(unseen_failure_of(&pool, &channel).await);

            let mut successor =
                SeedAssignment::new("assignment:successor", "thread:a", AGENT, "completed", 2);
            successor.previous_assignment_id = Some("assignment:failed");
            seed_assignment(&pool, &successor).await;

            assert!(!unseen_failure_of(&pool, &channel).await);
        }

        #[tokio::test]
        async fn a_retried_failure_is_not_marked() {
            let pool = crate::chat::tests::repository::pool_with_thread().await;
            seed_agent(&pool, AGENT, "Attention agent").await;
            let channel = channel_with_threads(&pool, "channel:retried", &["thread:a"]).await;
            let failed = SeedAssignment::new("assignment:a", "thread:a", AGENT, "failed", 1);
            seed_failure(&pool, &failed, "item:failure", "a1", 1).await;

            sqlx::query(
                "UPDATE chat_work_assignments SET state = 'queued', settled_at = NULL
                 WHERE id = 'assignment:a'",
            )
            .execute(&pool)
            .await
            .unwrap();

            assert!(!unseen_failure_of(&pool, &channel).await);
            assert_eq!(
                attention_of(&pool, &channel).await,
                Some(ChatWorkAssignmentState::Queued)
            );
        }

        #[tokio::test]
        async fn an_unseen_failure_clears_when_the_channel_is_opened_after_it() {
            let pool = crate::chat::tests::repository::pool_with_thread().await;
            seed_agent(&pool, AGENT, "Attention agent").await;
            let channel = channel_with_threads(&pool, "channel:opened", &["thread:a"]).await;
            open_channel_at(&pool, &channel, 1).await;
            let failed = SeedAssignment::new("assignment:a", "thread:a", AGENT, "failed", 5);
            seed_failure(&pool, &failed, "item:failure", "a1", 5).await;
            assert!(
                unseen_failure_of(&pool, &channel).await,
                "a failure after the last visit is unseen"
            );

            open_channel_at(&pool, &channel, 10).await;

            assert!(!unseen_failure_of(&pool, &channel).await);
            let thread = thread_summary(&pool, "thread:a").await;
            assert!(
                thread.unseen_failure,
                "the thread keeps its marker until the thread itself is read"
            );
        }

        #[tokio::test]
        async fn an_unseen_failure_clears_when_its_thread_is_read() {
            let pool = crate::chat::tests::repository::pool_with_thread().await;
            seed_agent(&pool, AGENT, "Attention agent").await;
            let channel = channel_with_threads(&pool, "channel:thread-read", &["thread:a"]).await;
            let failed = SeedAssignment::new("assignment:a", "thread:a", AGENT, "failed", 5);
            seed_failure(&pool, &failed, "item:failure", "a1", 5).await;

            seed_reply_thread_cursor(&pool, "thread:a", "a1", "item:failure").await;

            assert!(!unseen_failure_of(&pool, &channel).await);
            let thread = thread_summary(&pool, "thread:a").await;
            assert!(!thread.unseen_failure);
        }

        #[tokio::test]
        async fn a_delegated_failure_waits_on_its_open_parent() {
            let pool = crate::chat::tests::repository::pool_with_thread().await;
            seed_agent(&pool, AGENT, "Attention agent").await;
            let parent_channel =
                channel_with_threads(&pool, "channel:parent", &["thread:parent"]).await;
            let child_channel =
                channel_with_threads(&pool, "channel:child", &["thread:child"]).await;
            seed_assignment(
                &pool,
                &SeedAssignment::new("assignment:parent", "thread:parent", AGENT, "working", 1),
            )
            .await;
            let mut child =
                SeedAssignment::new("assignment:child", "thread:child", AGENT, "failed", 2);
            child.parent_assignment_id = Some("assignment:parent");
            seed_failure(&pool, &child, "item:failure:child", "a1", 2).await;

            assert_eq!(
                attention_of(&pool, &parent_channel).await,
                Some(ChatWorkAssignmentState::Working)
            );
            assert!(
                !unseen_failure_of(&pool, &child_channel).await,
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
            assert!(
                unseen_failure_of(&pool, &child_channel).await,
                "a settled parent no longer handles the failure"
            );
        }

        #[tokio::test]
        async fn unread_replies_count_only_threads_the_person_took_part_in() {
            let pool = crate::chat::tests::repository::pool_with_thread().await;
            seed_agent(&pool, AGENT, "Attention agent").await;
            let channel = channel_with_threads(
                &pool,
                "channel:replies",
                &["thread:own", "thread:joined", "thread:other"],
            )
            .await;
            // `thread:joined` and `thread:other` start from an agent's message.
            sqlx::query(
                "UPDATE chat_communication_messages SET author_participant_id = ?
                 WHERE item_id IN ('item:root:thread:joined', 'item:root:thread:other')",
            )
            .bind(AGENT)
            .execute(&pool)
            .await
            .unwrap();
            let unread_roots = read_channel(&pool, &channel).await.unwrap().unread_count;
            let at = minutes_after_seed(5);
            seed_reply(&pool, "thread:own", "item:own:1", AGENT, "a1", &at).await;
            seed_reply(&pool, "thread:own", "item:own:2", AGENT, "a2", &at).await;
            seed_reply(
                &pool,
                "thread:own",
                "item:own:mine",
                LOCAL_PARTICIPANT_ID,
                "a3",
                &at,
            )
            .await;
            seed_reply(
                &pool,
                "thread:joined",
                "item:joined:mine",
                LOCAL_PARTICIPANT_ID,
                "a1",
                &at,
            )
            .await;
            seed_reply(&pool, "thread:joined", "item:joined:1", AGENT, "a2", &at).await;
            seed_reply(&pool, "thread:other", "item:other:1", AGENT, "a1", &at).await;

            assert_eq!(unread_replies_of(&pool, &channel).await, 3);
            assert_eq!(
                read_channel(&pool, &channel).await.unwrap().unread_count,
                unread_roots,
                "replies never count as top-level messages"
            );

            seed_reply_thread_cursor(&pool, "thread:own", "a1", "item:own:1").await;
            assert_eq!(
                unread_replies_of(&pool, &channel).await,
                2,
                "a reply read in its thread no longer counts"
            );

            open_channel_at(&pool, &channel, 10).await;
            assert_eq!(unread_replies_of(&pool, &channel).await, 0);

            seed_reply(
                &pool,
                "thread:joined",
                "item:joined:2",
                AGENT,
                "a3",
                &minutes_after_seed(20),
            )
            .await;
            assert_eq!(
                unread_replies_of(&pool, &channel).await,
                1,
                "a reply after the visit counts again"
            );
        }

        #[tokio::test]
        async fn opening_a_channel_stamps_its_cursor_only_when_replies_arrived() {
            let pool = crate::chat::tests::repository::pool_with_thread().await;
            seed_agent(&pool, AGENT, "Attention agent").await;
            let channel = channel_with_threads(&pool, "channel:stamp", &["thread:a"]).await;
            open_channel_at(&pool, &channel, 1).await;
            let conversation_id = read_channel(&pool, &channel).await.unwrap().conversation_id;
            assert_eq!(
                cursor_stamp(&pool, &conversation_id).await,
                minutes_after_seed(1)
            );

            open_channel_at(&pool, &channel, 2).await;
            assert_eq!(
                cursor_stamp(&pool, &conversation_id).await,
                minutes_after_seed(1),
                "a quiet channel writes nothing"
            );

            seed_reply(
                &pool,
                "thread:a",
                "item:reply",
                AGENT,
                "a1",
                &minutes_after_seed(3),
            )
            .await;
            open_channel_at(&pool, &channel, 4).await;
            assert_eq!(
                cursor_stamp(&pool, &conversation_id).await,
                minutes_after_seed(4)
            );
        }

        #[tokio::test]
        async fn many_assignments_read_as_one_row_per_channel() {
            let pool = crate::chat::tests::repository::pool_with_thread().await;
            seed_agent(&pool, AGENT, "Attention agent").await;
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
                        &SeedAssignment::new(&id, &thread, AGENT, state, index),
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
