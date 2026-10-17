use crate::events::{ChangedFileSummary, ThreadUsageUpdatedEvent};
use crate::models::{
    ChatActivityId, ChatError, ChatErrorCode, ChatExecutionEnvironmentId, ChatResult,
    ChatScratchGenerationId, ChatThreadId, ChatThreadShellRead, ChatThreadState,
    ChatTimelineItemRead, ChatTimelinePageRead, ChatTimelineTurnRead, ChatTurnId, ChatTurnState,
    InteractionMode, ModelId, ModelOptionSelection, ProjectWorkingFolderId, ProviderFamilyId,
    ProviderInstanceId, ProviderThreadId, SafetyMode, TurnModeSnapshot, UtcTimestamp,
    VersionedJson,
};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeSet;

const MAX_PAGE_SIZE: u32 = 200;
const MAX_SEARCH_LENGTH: usize = 240;

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

pub async fn read_timeline_page(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    cursor: Option<&ChatTimelineCursor>,
    limit: u32,
) -> ChatResult<ChatTimelinePageRead> {
    let thread = sqlx::query("SELECT revision, last_event_sequence FROM chat_threads WHERE id = ?")
        .bind(thread_id.as_str())
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?
        .ok_or_else(not_found_error)?;
    let revision = unsigned(thread.try_get("revision").map_err(persistence_error)?)?;
    let last_sequence = unsigned(
        thread
            .try_get("last_event_sequence")
            .map_err(persistence_error)?,
    )?;
    let anchor = cursor
        .map_or(last_sequence, |value| value.sequence)
        .min(last_sequence);
    let anchor_row_id = cursor.and_then(|value| value.row_id.as_deref());
    let page_limit = limit.clamp(1, MAX_PAGE_SIZE);
    let mut rows = sqlx::query(
        "SELECT row_id, turn_id, sequence_anchor, item_kind, schema_version, item_data
         FROM (
           SELECT id AS row_id, turn_id, sequence_anchor, 'message' AS item_kind,
                  content_metadata_schema_version AS schema_version,
                  json_object('role', role, 'markdown', normalized_markdown,
                              'streamingState', streaming_state,
                              'providerItemId', provider_item_id,
                              'metadata', json(content_metadata_data),
                              'createdAt', created_at, 'updatedAt', updated_at) AS item_data
           FROM chat_messages m WHERE thread_id = ?
             AND (turn_id IS NULL OR EXISTS (
               SELECT 1 FROM chat_turns t WHERE t.id = m.turn_id AND t.invalidated_at IS NULL
             ))
           UNION ALL
           SELECT id, turn_id, sequence_anchor, 'activity', safe_metadata_schema_version,
                  json_object('activityKind', item_kind, 'status', status, 'title', title,
                              'detail', detail, 'providerItemId', provider_item_id,
                              'metadata', json(safe_metadata_data),
                              'createdAt', created_at, 'updatedAt', updated_at)
           FROM chat_activities a WHERE thread_id = ?
             AND (turn_id IS NULL OR EXISTS (
               SELECT 1 FROM chat_turns t WHERE t.id = a.turn_id AND t.invalidated_at IS NULL
             ))
           UNION ALL
           SELECT id, origin_turn_id, sequence_anchor, 'plan', steps_schema_version,
                  json_object('markdown', markdown, 'steps', json(steps_data), 'state', state,
                              'createdAt', created_at, 'updatedAt', updated_at)
           FROM chat_plans p WHERE thread_id = ?
             AND (origin_turn_id IS NULL OR EXISTS (
               SELECT 1 FROM chat_turns t
               WHERE t.id = p.origin_turn_id AND t.invalidated_at IS NULL
             ))
         )
         WHERE sequence_anchor < ?
            OR (sequence_anchor = ? AND (? IS NULL OR row_id < ?))
         ORDER BY sequence_anchor DESC, row_id DESC
         LIMIT ?",
    )
    .bind(thread_id.as_str())
    .bind(thread_id.as_str())
    .bind(thread_id.as_str())
    .bind(i64_value(anchor)?)
    .bind(i64_value(anchor)?)
    .bind(anchor_row_id)
    .bind(anchor_row_id)
    .bind(i64::from(page_limit) + 1)
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let has_older = rows.len() > page_limit as usize;
    rows.truncate(page_limit as usize);
    let mut items = rows
        .into_iter()
        .map(|row| row_to_timeline_item(row, None))
        .collect::<ChatResult<Vec<_>>>()?;
    items.reverse();
    let turn_ids = items
        .iter()
        .filter_map(|item| {
            item.turn_id
                .as_ref()
                .map(|turn_id| turn_id.as_str().to_string())
        })
        .collect::<BTreeSet<_>>();
    let turns = read_timeline_turns_by_ids(pool, &turn_ids).await?;
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
    })
}

/// Reads the complete projected timeline for one exact turn.
pub async fn read_timeline_turn(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    turn_id: &ChatTurnId,
) -> ChatResult<ChatTimelinePageRead> {
    let revision = sqlx::query_scalar::<_, i64>(
        "SELECT t.revision
         FROM chat_threads t
         JOIN chat_turns turn ON turn.thread_id = t.id
         WHERE t.id = ? AND turn.id = ? AND turn.invalidated_at IS NULL",
    )
    .bind(thread_id.as_str())
    .bind(turn_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(turn_not_found_error)
    .and_then(unsigned)?;
    let rows = sqlx::query(
        "SELECT row_id, turn_id, sequence_anchor, item_kind, schema_version, item_data
         FROM (
           SELECT id AS row_id, turn_id, sequence_anchor, 'message' AS item_kind,
                  content_metadata_schema_version AS schema_version,
                  json_object('role', role, 'markdown', normalized_markdown,
                              'streamingState', streaming_state,
                              'providerItemId', provider_item_id,
                              'metadata', json(content_metadata_data),
                              'createdAt', created_at, 'updatedAt', updated_at) AS item_data
           FROM chat_messages WHERE thread_id = ?
           UNION ALL
           SELECT id, turn_id, sequence_anchor, 'activity', safe_metadata_schema_version,
                  json_object('activityKind', item_kind, 'status', status, 'title', title,
                              'detail', detail, 'providerItemId', provider_item_id,
                              'metadata', json(safe_metadata_data),
                              'createdAt', created_at, 'updatedAt', updated_at)
           FROM chat_activities WHERE thread_id = ?
           UNION ALL
           SELECT id, origin_turn_id, sequence_anchor, 'plan', steps_schema_version,
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
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let items = rows
        .into_iter()
        .map(|row| row_to_timeline_item(row, Some(thread_id.clone())))
        .collect::<ChatResult<Vec<_>>>()?;
    let turn_ids = BTreeSet::from([turn_id.as_str().to_string()]);
    let turns = read_timeline_turns_by_ids(pool, &turn_ids).await?;
    Ok(ChatTimelinePageRead {
        thread_id: thread_id.clone(),
        items,
        turns,
        previous_cursor: None,
        next_cursor: None,
        thread_revision: revision,
    })
}

fn row_to_timeline_item(
    row: sqlx::sqlite::SqliteRow,
    source_thread_id: Option<ChatThreadId>,
) -> ChatResult<ChatTimelineItemRead> {
    let row_id: String = row.try_get("row_id").map_err(persistence_error)?;
    let data: String = row.try_get("item_data").map_err(persistence_error)?;
    Ok(ChatTimelineItemRead {
        activity_id: ChatActivityId::new(row_id).map_err(|_| corrupt_data_error())?,
        turn_id: row
            .try_get::<Option<String>, _>("turn_id")
            .map_err(persistence_error)?
            .map(ChatTurnId::new)
            .transpose()
            .map_err(|_| corrupt_data_error())?,
        sequence_anchor: unsigned(row.try_get("sequence_anchor").map_err(persistence_error)?)?,
        kind: row.try_get("item_kind").map_err(persistence_error)?,
        data: VersionedJson {
            schema_version: u32::try_from(
                row.try_get::<i64, _>("schema_version")
                    .map_err(persistence_error)?,
            )
            .map_err(|_| corrupt_data_error())?,
            value: serde_json::from_str(&data).map_err(serialization_error)?,
        },
        source_thread_id,
    })
}

pub async fn read_timeline_turns_by_ids(
    pool: &SqlitePool,
    turn_ids: &BTreeSet<String>,
) -> ChatResult<Vec<ChatTimelineTurnRead>> {
    if turn_ids.is_empty() {
        return Ok(Vec::new());
    }
    let encoded_turn_ids = serde_json::to_string(turn_ids).map_err(serialization_error)?;
    sqlx::query(
        "SELECT id, state, started_at, completed_at, stop_reason,
                model_selection_data, safety_mode, interaction_mode,
                usage_data, changed_file_summary_data
         FROM chat_turns
         WHERE invalidated_at IS NULL
           AND id IN (SELECT value FROM json_each(?))
         ORDER BY started_at, id",
    )
    .bind(encoded_turn_ids)
    .fetch_all(pool)
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
}
