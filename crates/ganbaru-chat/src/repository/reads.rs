use crate::events::{ChangedFileSummary, ThreadUsageUpdatedEvent};
use crate::models::{
    ChatActivityDetailRead, ChatActivityId, ChatChangeNotification, ChatError, ChatErrorCode,
    ChatExecutionEnvironmentId, ChatProjectionKey, ChatResult, ChatScratchGenerationId,
    ChatThreadId, ChatThreadShellRead, ChatThreadState, ChatTimelineDeltaRead,
    ChatTimelineItemRead, ChatTimelinePageRead, ChatTimelineTurnRead, ChatTurnId, ChatTurnState,
    InteractionMode, ModelId, ModelOptionSelection, ProjectWorkingFolderId, ProviderFamilyId,
    ProviderInstanceId, ProviderThreadId, SafetyMode, TurnModeSnapshot, UtcTimestamp,
    VersionedJson,
};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::{Row, SqliteConnection, SqlitePool};
use std::collections::BTreeSet;
use std::time::Duration;

const MAX_PAGE_SIZE: u32 = 200;
const MAX_SEARCH_LENGTH: usize = 240;
/// Body bytes after which a since-read stops returning rows.
pub const MAX_SINCE_READ_BYTES: u64 = 1024 * 1024;
/// How long removal records stay, so a reader that falls further behind reloads the timeline.
pub const TIMELINE_REMOVAL_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);
/// Bytes of an activity detail that timeline reads carry; the whole detail loads through
/// [`read_activity_detail`] when it is opened.
pub const ACTIVITY_DETAIL_PREVIEW_BYTES: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatProjectShellRead {
    pub project_id: String,
    pub working_folder_id: ProjectWorkingFolderId,
    pub workspace_name: String,
    pub workspace_archived_at: Option<UtcTimestamp>,
    pub active_thread_count: u64,
    pub archived_thread_count: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredModelSelection {
    #[serde(deserialize_with = "required_nullable")]
    model_id: Option<ModelId>,
    #[serde(rename = "options")]
    model_options: Vec<ModelOptionSelection>,
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTimelineCursor {
    sequence: u64,
    row_id: Option<String>,
}

pub fn parse_timeline_cursor(value: &str) -> ChatResult<ChatTimelineCursor> {
    if value.len() > 1_200 || value.chars().any(char::is_control) {
        return Err(invalid_cursor_error());
    }
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(ChatTimelineCursor {
            sequence: value.parse().map_err(|_| invalid_cursor_error())?,
            row_id: None,
        });
    }
    let cursor: ChatTimelineCursor =
        serde_json::from_str(value).map_err(|_| invalid_cursor_error())?;
    if cursor
        .row_id
        .as_ref()
        .is_some_and(|row_id| row_id.is_empty() || row_id.len() > 1_024)
    {
        return Err(invalid_cursor_error());
    }
    Ok(cursor)
}

pub async fn read_project_shells(pool: &SqlitePool) -> ChatResult<Vec<ChatProjectShellRead>> {
    let rows = sqlx::query(
        "SELECT w.project_id, w.id, w.display_name, w.archived_at,
                SUM(CASE WHEN t.id IS NOT NULL AND t.archived_at IS NULL AND t.state != 'closed' THEN 1 ELSE 0 END) AS active_count,
                SUM(CASE WHEN t.archived_at IS NOT NULL THEN 1 ELSE 0 END) AS archived_count
         FROM project_working_folders w
         LEFT JOIN chat_threads t ON t.working_folder_id = w.id
         GROUP BY w.id
         ORDER BY w.project_id, w.sort_order, w.display_name COLLATE NOCASE, w.id",
    )
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    rows.into_iter()
        .map(|row| {
            Ok(ChatProjectShellRead {
                project_id: row.try_get("project_id").map_err(persistence_error)?,
                working_folder_id: id(row.try_get("id").map_err(persistence_error)?)?,
                workspace_name: row.try_get("display_name").map_err(persistence_error)?,
                workspace_archived_at: timestamp(
                    row.try_get("archived_at").map_err(persistence_error)?,
                )?,
                active_thread_count: unsigned(
                    row.try_get("active_count").map_err(persistence_error)?,
                )?,
                archived_thread_count: unsigned(
                    row.try_get("archived_count").map_err(persistence_error)?,
                )?,
            })
        })
        .collect()
}

pub async fn read_thread_shells(
    pool: &SqlitePool,
    working_folder_id: Option<&ProjectWorkingFolderId>,
    archived: bool,
) -> ChatResult<Vec<ChatThreadShellRead>> {
    read_thread_shell_query(pool, working_folder_id, archived, None).await
}

pub async fn read_thread_shell_window(
    pool: &SqlitePool,
    working_folder_id: Option<&ProjectWorkingFolderId>,
    archived: bool,
    limit: u32,
) -> ChatResult<Vec<ChatThreadShellRead>> {
    read_thread_shell_query(
        pool,
        working_folder_id,
        archived,
        Some(limit.clamp(1, MAX_PAGE_SIZE)),
    )
    .await
}

async fn read_thread_shell_query(
    pool: &SqlitePool,
    working_folder_id: Option<&ProjectWorkingFolderId>,
    archived: bool,
    limit: Option<u32>,
) -> ChatResult<Vec<ChatThreadShellRead>> {
    let rows = sqlx::query(
        "SELECT id, working_folder_id, execution_environment_id, scratch_generation_id,
                project_id, title, provider_family_id,
                provider_instance_id, provider_thread_id, model_selection_data,
                safety_mode, interaction_mode, state, latest_turn_state,
                latest_preview, message_count, revision, last_event_sequence,
                last_activity_at, unread_at, archived_at
         FROM chat_threads
         WHERE (? IS NULL OR working_folder_id = ?)
           AND scratch_generation_id IS NULL
           AND ((? = 1 AND archived_at IS NOT NULL) OR (? = 0 AND archived_at IS NULL AND state != 'closed'))
         ORDER BY CASE WHEN archived_at IS NULL THEN last_activity_at ELSE archived_at END DESC, id
         LIMIT COALESCE(?, -1)",
    )
    .bind(working_folder_id.map(ProjectWorkingFolderId::as_str))
    .bind(working_folder_id.map(ProjectWorkingFolderId::as_str))
    .bind(archived)
    .bind(archived)
    .bind(limit.map(i64::from))
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    rows.into_iter().map(row_to_thread_shell).collect()
}

pub async fn read_thread_shell(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<ChatThreadShellRead> {
    sqlx::query(
        "SELECT id, working_folder_id, execution_environment_id, scratch_generation_id,
                project_id, title, provider_family_id,
                provider_instance_id, provider_thread_id, model_selection_data,
                safety_mode, interaction_mode, state, latest_turn_state,
                latest_preview, message_count, revision, last_event_sequence,
                last_activity_at, unread_at, archived_at
         FROM chat_threads WHERE id = ?",
    )
    .bind(thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .map(row_to_thread_shell)
    .transpose()?
    .ok_or_else(not_found_error)
}

pub async fn search_thread_titles(
    pool: &SqlitePool,
    query: &str,
    archived: Option<bool>,
    limit: u32,
) -> ChatResult<Vec<ChatThreadShellRead>> {
    let normalized = query.trim().to_lowercase();
    if normalized.is_empty() || normalized.len() > MAX_SEARCH_LENGTH {
        return Err(ChatError::validation(
            "query",
            "Chat title search query is invalid",
        ));
    }
    let pattern = format!("%{}%", escape_like(&normalized));
    let rows = sqlx::query(
        "SELECT id, working_folder_id, execution_environment_id, scratch_generation_id,
                project_id, title, provider_family_id,
                provider_instance_id, provider_thread_id, model_selection_data,
                safety_mode, interaction_mode, state, latest_turn_state,
                latest_preview, message_count, revision, last_event_sequence,
                last_activity_at, unread_at, archived_at
         FROM chat_threads
         WHERE title_search LIKE ? ESCAPE '\\'
           AND scratch_generation_id IS NULL
           AND (? IS NULL OR (? = 1 AND archived_at IS NOT NULL) OR (? = 0 AND archived_at IS NULL AND state != 'closed'))
         ORDER BY CASE WHEN title_search = ? THEN 0 WHEN title_search LIKE ? ESCAPE '\\' THEN 1 ELSE 2 END,
                  last_activity_at DESC, id
         LIMIT ?",
    )
    .bind(pattern)
    .bind(archived)
    .bind(archived)
    .bind(archived)
    .bind(&normalized)
    .bind(format!("{}%", escape_like(&normalized)))
    .bind(i64::from(limit.clamp(1, MAX_PAGE_SIZE)))
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    rows.into_iter().map(row_to_thread_shell).collect()
}

/// The visible timeline rows of one thread as `(row_id, turn_id, sequence_anchor, change_sequence,
/// item_kind, schema_version, item_data)`. Rows of an invalidated turn are hidden. `$filter` adds
/// a condition to every branch, binding one value per branch.
macro_rules! visible_timeline_rows {
    ($filter:literal) => {
        concat!(
            "SELECT id AS row_id, turn_id, sequence_anchor, change_sequence, 'message' AS item_kind,
                    content_metadata_schema_version AS schema_version,
                    json_object('role', role, 'markdown', normalized_markdown,
                                'streamingState', streaming_state,
                                'providerItemId', provider_item_id,
                                'metadata', json(content_metadata_data),
                                'createdAt', created_at, 'updatedAt', updated_at) AS item_data
             FROM chat_messages m WHERE thread_id = ? ",
            $filter,
            "
               AND (turn_id IS NULL OR EXISTS (
                 SELECT 1 FROM chat_turns t WHERE t.id = m.turn_id AND t.invalidated_at IS NULL
               ))
             UNION ALL
             SELECT id, turn_id, sequence_anchor, change_sequence, 'activity', safe_metadata_schema_version,
                    json_object('activityKind', item_kind, 'status', status, 'title', title,
                                'detail', detail, 'providerItemId', provider_item_id,
                                'metadata', json(safe_metadata_data),
                                'createdAt', created_at, 'updatedAt', updated_at)
             FROM chat_activities a WHERE thread_id = ? ",
            $filter,
            "
               AND (turn_id IS NULL OR EXISTS (
                 SELECT 1 FROM chat_turns t WHERE t.id = a.turn_id AND t.invalidated_at IS NULL
               ))
             UNION ALL
             SELECT id, origin_turn_id, sequence_anchor, change_sequence, 'plan', steps_schema_version,
                    json_object('markdown', markdown, 'steps', json(steps_data), 'state', state,
                                'createdAt', created_at, 'updatedAt', updated_at)
             FROM chat_plans p WHERE thread_id = ? ",
            $filter,
            "
               AND (origin_turn_id IS NULL OR EXISTS (
                 SELECT 1 FROM chat_turns t
                 WHERE t.id = p.origin_turn_id AND t.invalidated_at IS NULL
               ))"
        )
    };
}

/// Reads `(last_change, removals_pruned_through)` of a thread's timeline change clock.
pub(crate) async fn read_timeline_clock(
    connection: &mut SqliteConnection,
    thread_id: &ChatThreadId,
) -> ChatResult<(u64, u64)> {
    let clock = sqlx::query_as::<_, (i64, i64)>(
        "SELECT last_change, removals_pruned_through
         FROM chat_timeline_change_clocks WHERE thread_id = ?",
    )
    .bind(thread_id.as_str())
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?
    .unwrap_or((0, 0));
    Ok((unsigned(clock.0)?, unsigned(clock.1)?))
}

/// Reads one page of a thread's visible timeline, newest first from `cursor`, together with the
/// change clock it was read at, in one read transaction.
pub async fn read_timeline_page(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    cursor: Option<&ChatTimelineCursor>,
    limit: u32,
) -> ChatResult<ChatTimelinePageRead> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let thread = sqlx::query("SELECT revision, last_event_sequence FROM chat_threads WHERE id = ?")
        .bind(thread_id.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(persistence_error)?
        .ok_or_else(not_found_error)?;
    let revision = unsigned(thread.try_get("revision").map_err(persistence_error)?)?;
    let last_sequence = unsigned(
        thread
            .try_get("last_event_sequence")
            .map_err(persistence_error)?,
    )?;
    let (change_cursor, _) = read_timeline_clock(&mut transaction, thread_id).await?;
    let anchor = cursor
        .map_or(last_sequence, |value| value.sequence)
        .min(last_sequence);
    let anchor_row_id = cursor.and_then(|value| value.row_id.as_deref());
    let page_limit = limit.clamp(1, MAX_PAGE_SIZE);
    let mut rows = sqlx::query(concat!(
        "SELECT row_id, turn_id, sequence_anchor, change_sequence, item_kind, schema_version, item_data
         FROM (",
        visible_timeline_rows!(""),
        ")
         WHERE sequence_anchor < ?
            OR (sequence_anchor = ? AND (? IS NULL OR row_id < ?))
         ORDER BY sequence_anchor DESC, row_id DESC
         LIMIT ?"
    ))
    .bind(thread_id.as_str())
    .bind(thread_id.as_str())
    .bind(thread_id.as_str())
    .bind(i64_value(anchor)?)
    .bind(i64_value(anchor)?)
    .bind(anchor_row_id)
    .bind(anchor_row_id)
    .bind(i64::from(page_limit) + 1)
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let has_older = rows.len() > page_limit as usize;
    rows.truncate(page_limit as usize);
    let mut items = rows
        .into_iter()
        .map(|row| row_to_timeline_item(row, None))
        .collect::<ChatResult<Vec<_>>>()?;
    items.reverse();
    let turns = read_timeline_turns_in(&mut transaction, &item_turn_ids(&items)).await?;
    transaction.commit().await.map_err(persistence_error)?;
    let last = items.last().map(|item| item.sequence_anchor);
    Ok(ChatTimelinePageRead {
        thread_id: thread_id.clone(),
        previous_cursor: has_older
            .then(|| {
                items.first().map(|item| {
                    serde_json::to_string(&ChatTimelineCursor {
                        sequence: item.sequence_anchor,
                        row_id: Some(item.activity_id.as_str().to_string()),
                    })
                })
            })
            .flatten()
            .transpose()
            .map_err(serialization_error)?,
        next_cursor: last
            .filter(|value| *value < last_sequence)
            .map(|value| (value + 1).to_string()),
        items,
        turns,
        thread_revision: revision,
        change_cursor,
    })
}

/// Reads the visible timeline rows, turns, and removals of a thread written after `after_change`,
/// in one read transaction. The read returns at most `limit` rows and stops once their bodies
/// pass [`MAX_SINCE_READ_BYTES`], always returning at least one row; either cap sets `has_more`
/// and leaves `change_cursor` at the last returned change.
pub async fn read_timeline_since(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    after_change: u64,
    limit: u32,
) -> ChatResult<ChatTimelineDeltaRead> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let revision = sqlx::query_scalar::<_, i64>("SELECT revision FROM chat_threads WHERE id = ?")
        .bind(thread_id.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(persistence_error)?
        .ok_or_else(not_found_error)
        .and_then(unsigned)?;
    let (last_change, pruned_through) = read_timeline_clock(&mut transaction, thread_id).await?;
    let mut delta = ChatTimelineDeltaRead {
        thread_id: thread_id.clone(),
        items: Vec::new(),
        turns: Vec::new(),
        removed_item_ids: Vec::new(),
        change_cursor: last_change,
        has_more: false,
        reset_required: false,
        thread_revision: revision,
    };
    if after_change < pruned_through || after_change > last_change {
        delta.reset_required = true;
        return Ok(delta);
    }
    if after_change == last_change {
        return Ok(delta);
    }
    let after = i64_value(after_change)?;
    let row_limit = limit.clamp(1, MAX_PAGE_SIZE);
    let sizes = sqlx::query_as::<_, (i64, i64)>(
        "SELECT change_sequence, body_bytes FROM (
           SELECT change_sequence,
                  length(CAST(normalized_markdown AS BLOB))
                    + length(CAST(content_metadata_data AS BLOB)) AS body_bytes
           FROM chat_messages WHERE thread_id = ? AND change_sequence > ?
           UNION ALL
           SELECT change_sequence,
                  coalesce(length(CAST(detail AS BLOB)), 0)
                    + length(CAST(safe_metadata_data AS BLOB))
           FROM chat_activities WHERE thread_id = ? AND change_sequence > ?
           UNION ALL
           SELECT change_sequence,
                  length(CAST(markdown AS BLOB)) + length(CAST(steps_data AS BLOB))
           FROM chat_plans WHERE thread_id = ? AND change_sequence > ?
         )
         ORDER BY change_sequence
         LIMIT ?",
    )
    .bind(thread_id.as_str())
    .bind(after)
    .bind(thread_id.as_str())
    .bind(after)
    .bind(thread_id.as_str())
    .bind(after)
    .bind(i64::from(row_limit) + 1)
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let cutoff = since_read_cutoff(&sizes, row_limit as usize)?;
    if let Some(cutoff) = cutoff {
        delta.has_more = true;
        delta.change_cursor = cutoff;
    }
    let through = i64_value(delta.change_cursor)?;
    let rows = sqlx::query(concat!(
        "SELECT row_id, turn_id, sequence_anchor, change_sequence, item_kind, schema_version, item_data
         FROM (",
        visible_timeline_rows!("AND change_sequence > ? AND change_sequence <= ?"),
        ")
         ORDER BY change_sequence"
    ))
    .bind(thread_id.as_str())
    .bind(after)
    .bind(through)
    .bind(thread_id.as_str())
    .bind(after)
    .bind(through)
    .bind(thread_id.as_str())
    .bind(after)
    .bind(through)
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    delta.items = rows
        .into_iter()
        .map(|row| row_to_timeline_item(row, None))
        .collect::<ChatResult<Vec<_>>>()?;
    let changed_turns = sqlx::query_scalar::<_, String>(
        "SELECT id FROM chat_turns
         WHERE thread_id = ? AND change_sequence > ? AND change_sequence <= ?
           AND invalidated_at IS NULL",
    )
    .bind(thread_id.as_str())
    .bind(after)
    .bind(through)
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let mut turn_ids = item_turn_ids(&delta.items);
    turn_ids.extend(changed_turns);
    delta.turns = read_timeline_turns_in(&mut transaction, &turn_ids).await?;
    let removed = sqlx::query_scalar::<_, String>(
        "SELECT item_id FROM chat_timeline_removals
         WHERE thread_id = ? AND change_sequence > ? AND change_sequence <= ?
         UNION
         SELECT item.id FROM chat_turns t
         JOIN (
           SELECT id, turn_id FROM chat_messages WHERE thread_id = ?
           UNION ALL SELECT id, turn_id FROM chat_activities WHERE thread_id = ?
           UNION ALL SELECT id, origin_turn_id FROM chat_plans WHERE thread_id = ?
         ) item ON item.turn_id = t.id
         WHERE t.thread_id = ? AND t.invalidated_at IS NOT NULL
           AND t.change_sequence > ? AND t.change_sequence <= ?",
    )
    .bind(thread_id.as_str())
    .bind(after)
    .bind(through)
    .bind(thread_id.as_str())
    .bind(thread_id.as_str())
    .bind(thread_id.as_str())
    .bind(thread_id.as_str())
    .bind(after)
    .bind(through)
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    let returned = delta
        .items
        .iter()
        .map(|item| item.activity_id.as_str())
        .collect::<BTreeSet<_>>();
    delta.removed_item_ids = removed
        .into_iter()
        .filter(|id| !returned.contains(id.as_str()))
        .map(ChatActivityId::new)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| corrupt_data_error())?;
    Ok(delta)
}

/// Returns the last change a since-read may return when the row or byte cap cuts it short, given
/// `(change_sequence, body_bytes)` pairs in change order, or `None` when every change fits.
fn since_read_cutoff(sizes: &[(i64, i64)], row_limit: usize) -> ChatResult<Option<u64>> {
    let mut total = 0_u64;
    for (index, (_, bytes)) in sizes.iter().enumerate() {
        total = total.saturating_add(unsigned(*bytes)?);
        if index > 0 && (index >= row_limit || total > MAX_SINCE_READ_BYTES) {
            return unsigned(sizes[index - 1].0).map(Some);
        }
    }
    Ok(None)
}

/// Removes timeline removal records older than [`TIMELINE_REMOVAL_RETENTION`] before `now` and
/// records the highest pruned change per thread, so a since-read from before it asks for a reload.
pub async fn prune_timeline_removals(pool: &SqlitePool, now: &UtcTimestamp) -> ChatResult<u64> {
    let retention = format!("-{} seconds", TIMELINE_REMOVAL_RETENTION.as_secs());
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_timeline_change_clocks
         SET removals_pruned_through = max(removals_pruned_through, (
             SELECT max(change_sequence) FROM chat_timeline_removals r
             WHERE r.thread_id = chat_timeline_change_clocks.thread_id
               AND r.removed_at < strftime('%Y-%m-%dT%H:%M:%fZ', ?, ?)
         ))
         WHERE EXISTS (
             SELECT 1 FROM chat_timeline_removals r
             WHERE r.thread_id = chat_timeline_change_clocks.thread_id
               AND r.removed_at < strftime('%Y-%m-%dT%H:%M:%fZ', ?, ?)
         )",
    )
    .bind(now.as_str())
    .bind(&retention)
    .bind(now.as_str())
    .bind(&retention)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let pruned = sqlx::query(
        "DELETE FROM chat_timeline_removals
         WHERE removed_at < strftime('%Y-%m-%dT%H:%M:%fZ', ?, ?)",
    )
    .bind(now.as_str())
    .bind(&retention)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    transaction.commit().await.map_err(persistence_error)?;
    Ok(pruned)
}

/// Reads the complete projected timeline for one exact turn.
pub async fn read_timeline_turn(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    turn_id: &ChatTurnId,
) -> ChatResult<ChatTimelinePageRead> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let revision = sqlx::query_scalar::<_, i64>(
        "SELECT t.revision
         FROM chat_threads t
         JOIN chat_turns turn ON turn.thread_id = t.id
         WHERE t.id = ? AND turn.id = ? AND turn.invalidated_at IS NULL",
    )
    .bind(thread_id.as_str())
    .bind(turn_id.as_str())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(persistence_error)?
    .ok_or_else(turn_not_found_error)
    .and_then(unsigned)?;
    let (change_cursor, _) = read_timeline_clock(&mut transaction, thread_id).await?;
    let rows = sqlx::query(
        "SELECT row_id, turn_id, sequence_anchor, change_sequence, item_kind, schema_version, item_data
         FROM (
           SELECT id AS row_id, turn_id, sequence_anchor, change_sequence, 'message' AS item_kind,
                  content_metadata_schema_version AS schema_version,
                  json_object('role', role, 'markdown', normalized_markdown,
                              'streamingState', streaming_state,
                              'providerItemId', provider_item_id,
                              'metadata', json(content_metadata_data),
                              'createdAt', created_at, 'updatedAt', updated_at) AS item_data
           FROM chat_messages WHERE thread_id = ?
           UNION ALL
           SELECT id, turn_id, sequence_anchor, change_sequence, 'activity', safe_metadata_schema_version,
                  json_object('activityKind', item_kind, 'status', status, 'title', title,
                              'detail', detail, 'providerItemId', provider_item_id,
                              'metadata', json(safe_metadata_data),
                              'createdAt', created_at, 'updatedAt', updated_at)
           FROM chat_activities WHERE thread_id = ?
           UNION ALL
           SELECT id, origin_turn_id, sequence_anchor, change_sequence, 'plan', steps_schema_version,
                  json_object('markdown', markdown, 'steps', json(steps_data), 'state', state,
                              'createdAt', created_at, 'updatedAt', updated_at)
           FROM chat_plans WHERE thread_id = ?
         )
         WHERE turn_id = ?
         ORDER BY sequence_anchor, row_id",
    )
    .bind(thread_id.as_str())
    .bind(thread_id.as_str())
    .bind(thread_id.as_str())
    .bind(turn_id.as_str())
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let items = rows
        .into_iter()
        .map(|row| row_to_timeline_item(row, Some(thread_id.clone())))
        .collect::<ChatResult<Vec<_>>>()?;
    let turn_ids = BTreeSet::from([turn_id.as_str().to_string()]);
    let turns = read_timeline_turns_in(&mut transaction, &turn_ids).await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(ChatTimelinePageRead {
        thread_id: thread_id.clone(),
        items,
        turns,
        previous_cursor: None,
        next_cursor: None,
        thread_revision: revision,
        change_cursor,
    })
}

fn item_turn_ids(items: &[ChatTimelineItemRead]) -> BTreeSet<String> {
    items
        .iter()
        .filter_map(|item| {
            item.turn_id
                .as_ref()
                .map(|turn_id| turn_id.as_str().to_string())
        })
        .collect()
}

fn row_to_timeline_item(
    row: sqlx::sqlite::SqliteRow,
    source_thread_id: Option<ChatThreadId>,
) -> ChatResult<ChatTimelineItemRead> {
    let row_id: String = row.try_get("row_id").map_err(persistence_error)?;
    let data: String = row.try_get("item_data").map_err(persistence_error)?;
    let kind: String = row.try_get("item_kind").map_err(persistence_error)?;
    let mut value: serde_json::Value = serde_json::from_str(&data).map_err(serialization_error)?;
    if kind == "activity" {
        shorten_activity_detail(&mut value)?;
    }
    Ok(ChatTimelineItemRead {
        activity_id: ChatActivityId::new(row_id).map_err(|_| corrupt_data_error())?,
        turn_id: row
            .try_get::<Option<String>, _>("turn_id")
            .map_err(persistence_error)?
            .map(ChatTurnId::new)
            .transpose()
            .map_err(|_| corrupt_data_error())?,
        sequence_anchor: unsigned(row.try_get("sequence_anchor").map_err(persistence_error)?)?,
        change_sequence: unsigned(row.try_get("change_sequence").map_err(persistence_error)?)?,
        kind,
        data: VersionedJson {
            schema_version: u32::try_from(
                row.try_get::<i64, _>("schema_version")
                    .map_err(persistence_error)?,
            )
            .map_err(|_| corrupt_data_error())?,
            value,
        },
        source_thread_id,
    })
}

/// Activity kinds whose readers want the end of the detail: command output and reasoning
/// summaries. Every other kind previews the start.
pub(crate) const TAIL_PREVIEW_ACTIVITY_KINDS: [&str; 3] =
    ["command_execution", "command_output", "reasoning_summary"];

/// Replaces an activity row's `detail` with `detailPreview`, `detailBytes` (the UTF-8 length of
/// the whole detail), and `detailTruncated`.
fn shorten_activity_detail(value: &mut serde_json::Value) -> ChatResult<()> {
    let object = value.as_object_mut().ok_or_else(corrupt_data_error)?;
    let tail = object
        .get("activityKind")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| TAIL_PREVIEW_ACTIVITY_KINDS.contains(&kind));
    let detail = match object.remove("detail") {
        Some(serde_json::Value::String(detail)) => Some(detail),
        Some(serde_json::Value::Null) | None => None,
        Some(_) => return Err(corrupt_data_error()),
    };
    let bytes = detail.as_deref().map_or(0, str::len);
    let preview = detail
        .as_deref()
        .map(|detail| activity_detail_preview(detail, ACTIVITY_DETAIL_PREVIEW_BYTES, tail));
    let truncated = preview.is_some_and(|preview| preview.len() < bytes);
    object.insert(
        "detailPreview".to_string(),
        preview.map_or(serde_json::Value::Null, |preview| preview.into()),
    );
    object.insert("detailBytes".to_string(), bytes.into());
    object.insert("detailTruncated".to_string(), truncated.into());
    Ok(())
}

/// Returns at most `limit` bytes of `detail`, from its end when `tail` is set and from its start
/// otherwise, cut on a character boundary so no code point splits.
pub fn activity_detail_preview(detail: &str, limit: usize, tail: bool) -> &str {
    if detail.len() <= limit {
        return detail;
    }
    if tail {
        let mut start = detail.len() - limit;
        while !detail.is_char_boundary(start) {
            start += 1;
        }
        &detail[start..]
    } else {
        let mut end = limit;
        while !detail.is_char_boundary(end) {
            end -= 1;
        }
        &detail[..end]
    }
}

/// Reads the whole detail of one activity of a thread. An activity of another thread, or of an
/// invalidated turn, is not found.
pub async fn read_activity_detail(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    activity_id: &ChatActivityId,
) -> ChatResult<ChatActivityDetailRead> {
    let (detail, updated_at) = sqlx::query_as::<_, (Option<String>, String)>(
        "SELECT a.detail, a.updated_at
         FROM chat_activities a
         WHERE a.id = ? AND a.thread_id = ?
           AND (a.turn_id IS NULL OR EXISTS (
             SELECT 1 FROM chat_turns t WHERE t.id = a.turn_id AND t.invalidated_at IS NULL
           ))",
    )
    .bind(activity_id.as_str())
    .bind(thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(activity_not_found_error)?;
    Ok(ChatActivityDetailRead {
        activity_id: activity_id.clone(),
        detail,
        updated_at: UtcTimestamp::new(updated_at).map_err(|_| corrupt_data_error())?,
    })
}
pub async fn read_timeline_turns_by_ids(
    pool: &SqlitePool,
    turn_ids: &BTreeSet<String>,
) -> ChatResult<Vec<ChatTimelineTurnRead>> {
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    read_timeline_turns_in(&mut connection, turn_ids).await
}

pub(crate) async fn read_timeline_turns_in(
    connection: &mut SqliteConnection,
    turn_ids: &BTreeSet<String>,
) -> ChatResult<Vec<ChatTimelineTurnRead>> {
    if turn_ids.is_empty() {
        return Ok(Vec::new());
    }
    let encoded_turn_ids = serde_json::to_string(turn_ids).map_err(serialization_error)?;
    sqlx::query(
        "SELECT id, state, started_at, completed_at, stop_reason, error_data,
                model_selection_data, safety_mode, interaction_mode,
                usage_data, changed_file_summary_data
         FROM chat_turns
         WHERE invalidated_at IS NULL
           AND id IN (SELECT value FROM json_each(?))
         ORDER BY started_at, id",
    )
    .bind(encoded_turn_ids)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?
    .into_iter()
    .map(row_to_timeline_turn)
    .collect()
}

fn invalid_cursor_error() -> ChatError {
    ChatError::validation("cursor", "Chat timeline cursor is invalid")
}

fn row_to_timeline_turn(row: sqlx::sqlite::SqliteRow) -> ChatResult<ChatTimelineTurnRead> {
    let model: StoredModelSelection = serde_json::from_str(
        &row.try_get::<String, _>("model_selection_data")
            .map_err(persistence_error)?,
    )
    .map_err(serialization_error)?;
    let usage = row
        .try_get::<Option<String>, _>("usage_data")
        .map_err(persistence_error)?
        .map(|value| serde_json::from_str::<ThreadUsageUpdatedEvent>(&value))
        .transpose()
        .map_err(serialization_error)?;
    let changed_files = row
        .try_get::<Option<String>, _>("changed_file_summary_data")
        .map_err(persistence_error)?
        .map(|value| serde_json::from_str::<Vec<ChangedFileSummary>>(&value))
        .transpose()
        .map_err(serialization_error)?
        .unwrap_or_default();
    Ok(ChatTimelineTurnRead {
        turn_id: ChatTurnId::new(row.try_get::<String, _>("id").map_err(persistence_error)?)
            .map_err(|_| corrupt_data_error())?,
        state: parse_turn_state(
            &row.try_get::<String, _>("state")
                .map_err(persistence_error)?,
        )?,
        started_at: timestamp(row.try_get("started_at").map_err(persistence_error)?)?,
        completed_at: timestamp(row.try_get("completed_at").map_err(persistence_error)?)?,
        stop_reason: row.try_get("stop_reason").map_err(persistence_error)?,
        error_code: stored_error_code(row.try_get("error_data").map_err(persistence_error)?),
        model_id: model.model_id,
        model_options: model.model_options,
        modes: TurnModeSnapshot {
            safety_mode: parse_safety(
                &row.try_get::<String, _>("safety_mode")
                    .map_err(persistence_error)?,
            )?,
            interaction_mode: parse_interaction(
                &row.try_get::<String, _>("interaction_mode")
                    .map_err(persistence_error)?,
            )?,
        },
        usage,
        changed_files,
    })
}

/// Reads the code of a turn's stored structured error.
///
/// The code only chooses the localized explanation a reader shows, so a stored error without a
/// recognized code reads as `None` instead of failing the timeline read.
fn stored_error_code(error_data: Option<String>) -> Option<ChatErrorCode> {
    #[derive(Deserialize)]
    struct StoredErrorCode {
        code: ChatErrorCode,
    }
    serde_json::from_str::<StoredErrorCode>(&error_data?)
        .ok()
        .map(|stored| stored.code)
}

fn row_to_thread_shell(row: sqlx::sqlite::SqliteRow) -> ChatResult<ChatThreadShellRead> {
    let model_data: String = row
        .try_get("model_selection_data")
        .map_err(persistence_error)?;
    let model: StoredModelSelection =
        serde_json::from_str(&model_data).map_err(serialization_error)?;
    let working_folder_id = row
        .try_get::<Option<String>, _>("working_folder_id")
        .map_err(persistence_error)?
        .map(id)
        .transpose()?;
    let execution_environment_id = ChatExecutionEnvironmentId::new(
        row.try_get::<String, _>("execution_environment_id")
            .map_err(persistence_error)?,
    )
    .map_err(|_| corrupt_data_error())?;
    let scratch_generation_id = row
        .try_get::<Option<String>, _>("scratch_generation_id")
        .map_err(persistence_error)?
        .map(ChatScratchGenerationId::new)
        .transpose()
        .map_err(|_| corrupt_data_error())?;
    if !matches!(
        (&working_folder_id, &scratch_generation_id),
        (Some(_), None) | (None, Some(_))
    ) {
        return Err(corrupt_data_error());
    }
    Ok(ChatThreadShellRead {
        id: ChatThreadId::new(row.try_get::<String, _>("id").map_err(persistence_error)?)
            .map_err(|_| corrupt_data_error())?,
        working_folder_id,
        execution_environment_id,
        scratch_generation_id,
        project_id: row.try_get("project_id").map_err(persistence_error)?,
        title: row.try_get("title").map_err(persistence_error)?,
        provider_family_id: ProviderFamilyId::new(
            row.try_get::<String, _>("provider_family_id")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        provider_instance_id: ProviderInstanceId::new(
            row.try_get::<String, _>("provider_instance_id")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        provider_thread_id: row
            .try_get::<Option<String>, _>("provider_thread_id")
            .map_err(persistence_error)?
            .map(ProviderThreadId::new)
            .transpose()
            .map_err(|_| corrupt_data_error())?,
        model_id: model.model_id,
        model_options: model.model_options,
        modes: TurnModeSnapshot {
            safety_mode: parse_safety(
                &row.try_get::<String, _>("safety_mode")
                    .map_err(persistence_error)?,
            )?,
            interaction_mode: parse_interaction(
                &row.try_get::<String, _>("interaction_mode")
                    .map_err(persistence_error)?,
            )?,
        },
        state: parse_thread_state(
            &row.try_get::<String, _>("state")
                .map_err(persistence_error)?,
        )?,
        latest_turn_state: row
            .try_get::<Option<String>, _>("latest_turn_state")
            .map_err(persistence_error)?
            .map(|value| parse_turn_state(&value))
            .transpose()?,
        latest_preview: row.try_get("latest_preview").map_err(persistence_error)?,
        message_count: unsigned(row.try_get("message_count").map_err(persistence_error)?)?,
        revision: unsigned(row.try_get("revision").map_err(persistence_error)?)?,
        last_event_sequence: unsigned(
            row.try_get("last_event_sequence")
                .map_err(persistence_error)?,
        )?,
        last_activity_at: UtcTimestamp::new(
            row.try_get::<String, _>("last_activity_at")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        unread_at: timestamp(row.try_get("unread_at").map_err(persistence_error)?)?,
        archived_at: timestamp(row.try_get("archived_at").map_err(persistence_error)?)?,
    })
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
fn id(value: String) -> ChatResult<ProjectWorkingFolderId> {
    ProjectWorkingFolderId::new(value).map_err(|_| corrupt_data_error())
}
fn timestamp(value: Option<String>) -> ChatResult<Option<UtcTimestamp>> {
    value
        .map(UtcTimestamp::new)
        .transpose()
        .map_err(|_| corrupt_data_error())
}
fn unsigned(value: i64) -> ChatResult<u64> {
    u64::try_from(value).map_err(|_| corrupt_data_error())
}
fn i64_value(value: u64) -> ChatResult<i64> {
    i64::try_from(value)
        .map_err(|_| ChatError::validation("sequence", "Chat sequence is too large"))
}
fn parse_safety(value: &str) -> ChatResult<SafetyMode> {
    match value {
        "ask_for_approval" => Ok(SafetyMode::AskForApproval),
        "approve_for_me" => Ok(SafetyMode::ApproveForMe),
        "full_access" => Ok(SafetyMode::FullAccess),
        "custom" => Ok(SafetyMode::Custom),
        _ => Err(corrupt_data_error()),
    }
}
fn parse_interaction(value: &str) -> ChatResult<InteractionMode> {
    match value {
        "build" => Ok(InteractionMode::Build),
        "plan" => Ok(InteractionMode::Plan),
        _ => Err(corrupt_data_error()),
    }
}
fn parse_thread_state(value: &str) -> ChatResult<ChatThreadState> {
    match value {
        "draft" => Ok(ChatThreadState::Draft),
        "active" => Ok(ChatThreadState::Active),
        "waiting" => Ok(ChatThreadState::Waiting),
        "idle" => Ok(ChatThreadState::Idle),
        "error" => Ok(ChatThreadState::Error),
        "archived" => Ok(ChatThreadState::Archived),
        "closed" => Ok(ChatThreadState::Closed),
        _ => Err(corrupt_data_error()),
    }
}
fn parse_turn_state(value: &str) -> ChatResult<ChatTurnState> {
    match value {
        "pending" => Ok(ChatTurnState::Pending),
        "dispatching" => Ok(ChatTurnState::Dispatching),
        "active" => Ok(ChatTurnState::Active),
        "waiting_for_approval" => Ok(ChatTurnState::WaitingForApproval),
        "waiting_for_user_input" => Ok(ChatTurnState::WaitingForUserInput),
        "completed" => Ok(ChatTurnState::Completed),
        "interrupted" => Ok(ChatTurnState::Interrupted),
        "failed" => Ok(ChatTurnState::Failed),
        _ => Err(corrupt_data_error()),
    }
}
fn not_found_error() -> ChatError {
    ChatError::new(ChatErrorCode::NotFound, "Chat thread was not found", true)
}
fn turn_not_found_error() -> ChatError {
    ChatError::new(ChatErrorCode::NotFound, "Chat turn was not found", true)
}
fn activity_not_found_error() -> ChatError {
    ChatError::new(ChatErrorCode::NotFound, "Chat activity was not found", true)
}
fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat read persistence failed",
        true,
    )
}
fn serialization_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat JSON is invalid",
        false,
    )
}
fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat record is invalid",
        false,
    )
}

/// Builds the change notification for a write that changed a thread's rows without an event. It
/// carries the thread's last event sequence as both `sequence` and `since_sequence`, so a reader
/// sees no skipped event.
pub async fn read_change_notification(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    changed_projection_keys: Vec<ChatProjectionKey>,
) -> ChatResult<ChatChangeNotification> {
    let (revision, sequence) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT revision, last_event_sequence FROM chat_threads WHERE id = ?",
    )
    .bind(thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(not_found_error)?;
    let sequence = unsigned(sequence)?;
    Ok(ChatChangeNotification {
        thread_id: thread_id.clone(),
        sequence,
        since_sequence: sequence,
        revision: unsigned(revision)?,
        changed_projection_keys,
    })
}

#[cfg(test)]
mod tests {
    use super::StoredModelSelection;

    #[test]
    fn stored_model_selection_requires_current_nullable_id_and_options_fields() {
        let current = serde_json::json!({ "modelId": null, "options": [] });
        assert!(serde_json::from_value::<StoredModelSelection>(current.clone()).is_ok());

        let mut missing_model_id = current.clone();
        missing_model_id.as_object_mut().unwrap().remove("modelId");
        assert!(serde_json::from_value::<StoredModelSelection>(missing_model_id).is_err());

        let mut missing_options = current.clone();
        missing_options.as_object_mut().unwrap().remove("options");
        assert!(serde_json::from_value::<StoredModelSelection>(missing_options).is_err());

        assert!(
            serde_json::from_value::<StoredModelSelection>(serde_json::json!({
                "modelId": null,
                "modelOptions": []
            }))
            .is_err()
        );
    }

    #[test]
    fn activity_detail_preview_cuts_on_character_boundaries_from_either_end() {
        let detail = "aé✓b";
        assert_eq!(super::activity_detail_preview(detail, 16, true), detail);
        assert_eq!(super::activity_detail_preview(detail, 2, false), "a");
        assert_eq!(super::activity_detail_preview(detail, 3, false), "aé");
        assert_eq!(super::activity_detail_preview(detail, 3, true), "b");
        assert_eq!(super::activity_detail_preview(detail, 4, true), "✓b");
    }

    #[test]
    fn shortened_activity_detail_keeps_the_command_tail_and_the_tool_head() {
        let long = format!(
            "{}{}",
            "h".repeat(super::ACTIVITY_DETAIL_PREVIEW_BYTES),
            "tail"
        );
        let mut command =
            serde_json::json!({ "activityKind": "command_execution", "detail": long });
        super::shorten_activity_detail(&mut command).unwrap();
        let preview = command["detailPreview"].as_str().unwrap();
        assert!(preview.ends_with("tail"));
        assert_eq!(preview.len(), super::ACTIVITY_DETAIL_PREVIEW_BYTES);
        assert_eq!(command["detailBytes"], long.len());
        assert_eq!(command["detailTruncated"], true);
        assert!(command.get("detail").is_none());

        let mut tool = serde_json::json!({ "activityKind": "dynamic_tool_call", "detail": long });
        super::shorten_activity_detail(&mut tool).unwrap();
        assert!(!tool["detailPreview"].as_str().unwrap().contains("tail"));

        let mut short = serde_json::json!({ "activityKind": "notice", "detail": "done" });
        super::shorten_activity_detail(&mut short).unwrap();
        assert_eq!(short["detailPreview"], "done");
        assert_eq!(short["detailTruncated"], false);

        let mut empty = serde_json::json!({ "activityKind": "notice", "detail": null });
        super::shorten_activity_detail(&mut empty).unwrap();
        assert!(empty["detailPreview"].is_null());
        assert_eq!(empty["detailBytes"], 0);
    }
}
