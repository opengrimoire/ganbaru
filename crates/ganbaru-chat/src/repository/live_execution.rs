//! The live execution read a linked phone pulls from the device that executes a work assignment.
//!
//! The read covers only the turn of the assignment's newest run. Rows are built from an allowlist
//! of presentation fields with every variable field capped, so request, approval, provider,
//! environment, folder, and file change data never leave the executing device, and a page stays
//! under a byte budget measured the way the transport encodes it.

use super::reads::{
    ACTIVITY_DETAIL_PREVIEW_BYTES, TAIL_PREVIEW_ACTIVITY_KINDS, activity_detail_preview,
    read_timeline_clock, read_timeline_turns_in,
};
use crate::agent_runs::SETTLED_RUN_STATES;
use crate::models::{
    ChatActivityId, ChatAgentRunId, ChatError, ChatErrorCode, ChatLiveExecutionRead, ChatResult,
    ChatThreadId, ChatTimelineItemRead, ChatTurnId, ModelId, ProviderFamilyId, VersionedJson,
};
use serde_json::{Map, Value, json};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeSet;

/// Rows and removals one live read returns at most.
pub const MAX_LIVE_ENTRIES: usize = 200;
/// Bytes of a message's Markdown a live read carries, taken from its end.
pub const LIVE_MESSAGE_TAIL_BYTES: usize = 64 * 1024;
/// Bytes of a plan's Markdown a live read carries, taken from its start.
pub const LIVE_PLAN_MARKDOWN_BYTES: usize = 32 * 1024;
/// Steps of a plan a live read carries.
pub const LIVE_PLAN_STEPS: usize = 64;
/// Bytes of one plan step's text a live read carries.
const LIVE_PLAN_STEP_TEXT_BYTES: usize = 512;
/// Bytes of a short identifier or label a live read carries, such as a tool name or step id.
const LIVE_LABEL_BYTES: usize = 256;
/// Bytes of an activity title a live read carries.
const LIVE_TITLE_BYTES: usize = 2_000;
/// Characters SQLite cuts a message to before the byte tail is taken, so a long message is never
/// read whole. Four bytes per character at most keeps the byte tail inside it.
const MESSAGE_TAIL_CHARACTERS: i64 = LIVE_MESSAGE_TAIL_BYTES as i64;
/// Characters SQLite cuts a plan's Markdown to before the byte cap is taken.
const PLAN_HEAD_CHARACTERS: i64 = LIVE_PLAN_MARKDOWN_BYTES as i64;
/// Characters SQLite cuts an activity detail to before the byte preview is taken.
const DETAIL_WINDOW_CHARACTERS: i64 = ACTIVITY_DETAIL_PREVIEW_BYTES as i64;

/// Assignment states in which work can still write the run's timeline.
const RUNNING_ASSIGNMENT_STATES: [&str; 4] = [
    "queued",
    "working",
    "waiting_for_answer",
    "waiting_for_approval",
];
/// Activity metadata keys whose short text a live read keeps.
const LIVE_METADATA_TEXT_KEYS: [&str; 4] = ["toolKind", "tool", "toolName", "name"];
/// Byte limit of a web search query or page URL a live read keeps.
const LIVE_WEB_FIELD_BYTES: usize = 2048;
/// Most web search result links a live read keeps.
const LIVE_WEB_LINKS: usize = 24;
/// Byte limit of one web search result title a live read keeps.
const LIVE_WEB_LINK_TITLE_BYTES: usize = 256;

/// What a live read asks for.
#[derive(Clone, Copy, Debug)]
pub struct LiveExecutionQuery<'a> {
    /// The work assignment whose newest run is read.
    pub assignment_id: &'a str,
    /// This device's stable id; only the device that executes the assignment serves its run.
    pub own_device_id: &'a str,
    /// The change clock value the reader already has, or `None` for the whole run.
    pub after_change: Option<u64>,
}

/// How large a live read may grow.
#[derive(Clone, Copy, Debug)]
pub struct LiveExecutionBudget {
    /// Bytes the encoded read may take, as `measure` counts them.
    pub payload_bytes: usize,
    /// Counts the bytes a serialized JSON text takes once the transport encodes it.
    pub measure: fn(&str) -> usize,
}

/// The result of a live read.
#[derive(Clone, Debug, PartialEq)]
pub enum LiveExecutionOutcome {
    /// The assignment is unknown or another device executes it.
    NotHere,
    /// The live rows this device serves.
    Read(Box<ChatLiveExecutionRead>),
}

/// The newest run of an assignment and its turn.
struct NewestRun {
    id: String,
    state: String,
    turn_id: String,
    thread_id: String,
    invalidated: bool,
    provider_family_id: String,
    model_id: Option<String>,
}

/// One change a live read returns: a row written or a row removed.
enum LiveEntry {
    Item(ChatTimelineItemRead),
    Removed(ChatActivityId),
}

/// Reads the live execution of `query.assignment_id` from this device, which must execute it.
///
/// Rows of the newest run's turn written after `query.after_change` return in change order,
/// together with that turn and the rows removed after the cursor. The read stops at
/// [`MAX_LIVE_ENTRIES`] changes or once the encoded read would pass `budget.payload_bytes`,
/// always returning at least one change; either cap sets `has_more` and leaves `change_cursor`
/// at the last returned change. A cursor outside the thread's kept change range sets
/// `reset_required`.
pub async fn read_live_execution(
    pool: &SqlitePool,
    query: LiveExecutionQuery<'_>,
    budget: LiveExecutionBudget,
) -> ChatResult<LiveExecutionOutcome> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let assignment = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT state, executing_device_id FROM chat_work_assignments WHERE id = ?",
    )
    .bind(query.assignment_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let Some((assignment_state, executing_device_id)) = assignment else {
        return Ok(LiveExecutionOutcome::NotHere);
    };
    if executing_device_id.as_deref() != Some(query.own_device_id) {
        return Ok(LiveExecutionOutcome::NotHere);
    }
    let assignment_settled = !RUNNING_ASSIGNMENT_STATES.contains(&assignment_state.as_str());
    let mut read = ChatLiveExecutionRead {
        assignment_id: query.assignment_id.to_string(),
        run_id: None,
        thread_id: None,
        turn_id: None,
        provider_family_id: None,
        model_id: None,
        items: Vec::new(),
        turns: Vec::new(),
        removed_item_ids: Vec::new(),
        change_cursor: 0,
        has_more: false,
        reset_required: false,
        settled: assignment_settled,
    };
    let run = sqlx::query(
        "SELECT r.id, r.state, r.provider_turn_id, t.thread_id, t.invalidated_at IS NOT NULL AS invalidated,
                th.provider_family_id,
                CASE json_type(t.model_selection_data, '$.modelId')
                  WHEN 'text' THEN json_extract(t.model_selection_data, '$.modelId')
                END AS model_id
         FROM chat_agent_runs r
         JOIN chat_turns t ON t.id = r.provider_turn_id
         JOIN chat_threads th ON th.id = t.thread_id
         WHERE r.assignment_id = ?
         ORDER BY r.run_ordinal DESC
         LIMIT 1",
    )
    .bind(query.assignment_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(persistence_error)?
    .map(|row| -> ChatResult<NewestRun> {
        Ok(NewestRun {
            id: row.try_get("id").map_err(persistence_error)?,
            state: row.try_get("state").map_err(persistence_error)?,
            turn_id: row.try_get("provider_turn_id").map_err(persistence_error)?,
            thread_id: row.try_get("thread_id").map_err(persistence_error)?,
            invalidated: row.try_get("invalidated").map_err(persistence_error)?,
            provider_family_id: row.try_get("provider_family_id").map_err(persistence_error)?,
            model_id: row.try_get("model_id").map_err(persistence_error)?,
        })
    })
    .transpose()?;
    let Some(run) = run else {
        transaction.commit().await.map_err(persistence_error)?;
        return Ok(LiveExecutionOutcome::Read(Box::new(read)));
    };
    read.settled = assignment_settled || SETTLED_RUN_STATES.contains(&run.state.as_str());
    let thread_id = ChatThreadId::new(run.thread_id).map_err(|_| corrupt_data_error())?;
    let turn_id = ChatTurnId::new(run.turn_id).map_err(|_| corrupt_data_error())?;
    read.provider_family_id =
        Some(ProviderFamilyId::new(run.provider_family_id).map_err(|_| corrupt_data_error())?);
    read.model_id = run
        .model_id
        .map(ModelId::new)
        .transpose()
        .map_err(|_| corrupt_data_error())?;
    read.run_id = Some(ChatAgentRunId::new(run.id).map_err(|_| corrupt_data_error())?);
    read.thread_id = Some(thread_id.clone());
    read.turn_id = Some(turn_id.clone());
    let (last_change, pruned_through) = read_timeline_clock(&mut transaction, &thread_id).await?;
    read.change_cursor = last_change;
    if let Some(after) = query.after_change
        && (after < pruned_through || after > last_change)
    {
        read.reset_required = true;
        transaction.commit().await.map_err(persistence_error)?;
        return Ok(LiveExecutionOutcome::Read(Box::new(read)));
    }
    if run.invalidated {
        // A rolled back turn shows nothing; its rows are hidden on every timeline.
        transaction.commit().await.map_err(persistence_error)?;
        return Ok(LiveExecutionOutcome::Read(Box::new(read)));
    }
    let turn_ids = BTreeSet::from([turn_id.as_str().to_string()]);
    read.turns = read_timeline_turns_in(&mut transaction, &turn_ids).await?;
    for turn in &mut read.turns {
        turn.changed_files.clear();
    }
    let after = query.after_change.unwrap_or(0);
    if after == last_change {
        transaction.commit().await.map_err(persistence_error)?;
        return Ok(LiveExecutionOutcome::Read(Box::new(read)));
    }
    let after = i64::try_from(after).map_err(|_| corrupt_data_error())?;
    let items = read_live_items(&mut transaction, &thread_id, &turn_id, after).await?;
    let removals = if query.after_change.is_some() {
        read_live_removals(&mut transaction, &thread_id, after).await?
    } else {
        Vec::new()
    };
    transaction.commit().await.map_err(persistence_error)?;
    fill_entries(&mut read, items, removals, budget)?;
    Ok(LiveExecutionOutcome::Read(Box::new(read)))
}

/// Merges `items` and `removals`, each in change order and fetched one past
/// [`MAX_LIVE_ENTRIES`], into `read` until a cap is reached.
fn fill_entries(
    read: &mut ChatLiveExecutionRead,
    items: Vec<(u64, ChatTimelineItemRead)>,
    removals: Vec<(u64, ChatActivityId)>,
    budget: LiveExecutionBudget,
) -> ChatResult<()> {
    // A list cut at its fetch limit may hide changes after its last kept entry, so no entry past
    // that change is returned.
    let mut limit_change = u64::MAX;
    if items.len() > MAX_LIVE_ENTRIES {
        limit_change = limit_change.min(items[MAX_LIVE_ENTRIES - 1].0);
    }
    if removals.len() > MAX_LIVE_ENTRIES {
        limit_change = limit_change.min(removals[MAX_LIVE_ENTRIES - 1].0);
    }
    let mut entries = items
        .into_iter()
        .map(|(change, item)| (change, LiveEntry::Item(item)))
        .chain(
            removals
                .into_iter()
                .map(|(change, id)| (change, LiveEntry::Removed(id))),
        )
        .collect::<Vec<_>>();
    entries.sort_by_key(|(change, _)| *change);
    let mut used = measured(&*read, budget)?;
    let mut cutoff = None;
    for (kept, (change, entry)) in entries.into_iter().enumerate() {
        let cost = match &entry {
            LiveEntry::Item(item) => measured(item, budget)?,
            LiveEntry::Removed(id) => measured(id, budget)?,
        }
        .saturating_add(1);
        let over_budget = used.saturating_add(cost) > budget.payload_bytes;
        if change > limit_change || (kept > 0 && (kept >= MAX_LIVE_ENTRIES || over_budget)) {
            break;
        }
        used = used.saturating_add(cost);
        cutoff = Some(change);
        match entry {
            LiveEntry::Item(item) => read.items.push(item),
            LiveEntry::Removed(id) => read.removed_item_ids.push(id),
        }
    }
    if let Some(cutoff) = cutoff
        && cutoff < read.change_cursor
    {
        read.has_more = true;
        read.change_cursor = cutoff;
    }
    // A row removed and written again reads as written.
    let returned = read
        .items
        .iter()
        .map(|item| item.activity_id.clone())
        .collect::<BTreeSet<_>>();
    read.removed_item_ids.retain(|id| !returned.contains(id));
    Ok(())
}

/// The encoded size of `value` serialized as JSON.
fn measured<T: serde::Serialize + ?Sized>(
    value: &T,
    budget: LiveExecutionBudget,
) -> ChatResult<usize> {
    serde_json::to_string(value)
        .map(|text| (budget.measure)(&text))
        .map_err(serialization_error)
}

/// Reads the shortened rows of `turn_id` written after `after`, in change order, at most one
/// past [`MAX_LIVE_ENTRIES`]. User messages and user message activities are withheld.
async fn read_live_items(
    connection: &mut sqlx::SqliteConnection,
    thread_id: &ChatThreadId,
    turn_id: &ChatTurnId,
    after: i64,
) -> ChatResult<Vec<(u64, ChatTimelineItemRead)>> {
    let tail_kinds =
        serde_json::to_string(&TAIL_PREVIEW_ACTIVITY_KINDS).map_err(serialization_error)?;
    let rows = sqlx::query(
        "SELECT * FROM (
           SELECT id, sequence_anchor, change_sequence, 'message' AS kind,
                  content_metadata_schema_version AS schema_version,
                  NULL AS item_kind, streaming_state AS status, NULL AS title,
                  CASE WHEN length(normalized_markdown) > ?
                    THEN substr(normalized_markdown, -?) ELSE normalized_markdown END AS text,
                  length(CAST(normalized_markdown AS BLOB)) AS text_bytes,
                  CASE json_type(content_metadata_data, '$.phase')
                    WHEN 'text' THEN substr(json_extract(content_metadata_data, '$.phase'), 1, ?)
                  END AS phase,
                  NULL AS tool_kind, NULL AS tool, NULL AS tool_name, NULL AS name,
                  NULL AS exit_code, NULL AS duration_ms, NULL AS steps,
                  NULL AS web_query, NULL AS web_url, NULL AS web_result,
                  created_at, updated_at
           FROM chat_messages
           WHERE thread_id = ? AND turn_id = ? AND role = 'assistant' AND change_sequence > ?
           UNION ALL
           SELECT id, sequence_anchor, change_sequence, 'activity', safe_metadata_schema_version,
                  item_kind, status, title,
                  CASE
                    WHEN detail IS NULL OR length(detail) <= ? THEN detail
                    WHEN item_kind IN (SELECT value FROM json_each(?)) THEN substr(detail, -?)
                    ELSE substr(detail, 1, ?)
                  END,
                  length(CAST(detail AS BLOB)),
                  NULL,
                  CASE json_type(safe_metadata_data, '$.toolKind')
                    WHEN 'text' THEN substr(json_extract(safe_metadata_data, '$.toolKind'), 1, ?)
                  END,
                  CASE json_type(safe_metadata_data, '$.tool')
                    WHEN 'text' THEN substr(json_extract(safe_metadata_data, '$.tool'), 1, ?)
                  END,
                  CASE json_type(safe_metadata_data, '$.toolName')
                    WHEN 'text' THEN substr(json_extract(safe_metadata_data, '$.toolName'), 1, ?)
                  END,
                  CASE json_type(safe_metadata_data, '$.name')
                    WHEN 'text' THEN substr(json_extract(safe_metadata_data, '$.name'), 1, ?)
                  END,
                  CASE json_type(safe_metadata_data, '$.exitCode')
                    WHEN 'integer' THEN json_extract(safe_metadata_data, '$.exitCode')
                  END,
                  CASE json_type(safe_metadata_data, '$.durationMs')
                    WHEN 'integer' THEN json_extract(safe_metadata_data, '$.durationMs')
                    WHEN 'real' THEN CAST(json_extract(safe_metadata_data, '$.durationMs') AS INTEGER)
                  END,
                  NULL,
                  CASE WHEN item_kind = 'web_search'
                    AND json_type(safe_metadata_data, '$.toolInput.query') = 'text'
                    THEN substr(json_extract(safe_metadata_data, '$.toolInput.query'), 1, ?)
                  END,
                  CASE WHEN item_kind = 'web_search'
                    AND json_type(safe_metadata_data, '$.toolInput.url') = 'text'
                    THEN substr(json_extract(safe_metadata_data, '$.toolInput.url'), 1, ?)
                  END,
                  CASE WHEN item_kind = 'web_search'
                    AND json_type(safe_metadata_data, '$.webResult') = 'object'
                    THEN json_extract(safe_metadata_data, '$.webResult')
                  END,
                  created_at, updated_at
           FROM chat_activities
           WHERE thread_id = ? AND turn_id = ? AND item_kind != 'user_message'
             AND change_sequence > ?
           UNION ALL
           SELECT id, sequence_anchor, change_sequence, 'plan', steps_schema_version,
                  NULL, state, NULL,
                  CASE WHEN length(markdown) > ? THEN substr(markdown, 1, ?) ELSE markdown END,
                  length(CAST(markdown AS BLOB)),
                  NULL, NULL, NULL, NULL, NULL, NULL, NULL,
                  steps_data, NULL, NULL, NULL,
                  created_at, updated_at
           FROM chat_plans
           WHERE thread_id = ? AND origin_turn_id = ? AND change_sequence > ?
         )
         ORDER BY change_sequence
         LIMIT ?",
    )
    .bind(MESSAGE_TAIL_CHARACTERS)
    .bind(MESSAGE_TAIL_CHARACTERS)
    .bind(LIVE_LABEL_BYTES as i64)
    .bind(thread_id.as_str())
    .bind(turn_id.as_str())
    .bind(after)
    .bind(DETAIL_WINDOW_CHARACTERS)
    .bind(tail_kinds)
    .bind(DETAIL_WINDOW_CHARACTERS)
    .bind(DETAIL_WINDOW_CHARACTERS)
    .bind(LIVE_LABEL_BYTES as i64)
    .bind(LIVE_LABEL_BYTES as i64)
    .bind(LIVE_LABEL_BYTES as i64)
    .bind(LIVE_LABEL_BYTES as i64)
    .bind(LIVE_WEB_FIELD_BYTES as i64)
    .bind(LIVE_WEB_FIELD_BYTES as i64)
    .bind(thread_id.as_str())
    .bind(turn_id.as_str())
    .bind(after)
    .bind(PLAN_HEAD_CHARACTERS)
    .bind(PLAN_HEAD_CHARACTERS)
    .bind(thread_id.as_str())
    .bind(turn_id.as_str())
    .bind(after)
    .bind((MAX_LIVE_ENTRIES + 1) as i64)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    rows.into_iter()
        .map(|row| live_item(row, turn_id))
        .collect()
}

/// Reads the ids of rows removed from `thread_id` after `after`, in change order, at most one
/// past [`MAX_LIVE_ENTRIES`].
async fn read_live_removals(
    connection: &mut sqlx::SqliteConnection,
    thread_id: &ChatThreadId,
    after: i64,
) -> ChatResult<Vec<(u64, ChatActivityId)>> {
    sqlx::query_as::<_, (i64, String)>(
        "SELECT change_sequence, item_id FROM chat_timeline_removals
         WHERE thread_id = ? AND change_sequence > ?
         ORDER BY change_sequence
         LIMIT ?",
    )
    .bind(thread_id.as_str())
    .bind(after)
    .bind((MAX_LIVE_ENTRIES + 1) as i64)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?
    .into_iter()
    .map(|(change, id)| {
        Ok((
            unsigned(change)?,
            ChatActivityId::new(id).map_err(|_| corrupt_data_error())?,
        ))
    })
    .collect()
}

/// Builds one live row from the allowlisted columns of `row`.
fn live_item(
    row: sqlx::sqlite::SqliteRow,
    turn_id: &ChatTurnId,
) -> ChatResult<(u64, ChatTimelineItemRead)> {
    let kind: String = row.try_get("kind").map_err(persistence_error)?;
    let text: Option<String> = row.try_get("text").map_err(persistence_error)?;
    let text_bytes = row
        .try_get::<Option<i64>, _>("text_bytes")
        .map_err(persistence_error)?
        .map(unsigned)
        .transpose()?;
    let status: String = row.try_get("status").map_err(persistence_error)?;
    let created_at: String = row.try_get("created_at").map_err(persistence_error)?;
    let updated_at: String = row.try_get("updated_at").map_err(persistence_error)?;
    let mut value = Map::new();
    match kind.as_str() {
        "message" => {
            let text = text.unwrap_or_default();
            let markdown = activity_detail_preview(&text, LIVE_MESSAGE_TAIL_BYTES, true);
            value.insert("role".into(), json!("assistant"));
            value.insert("markdown".into(), json!(markdown));
            value.insert(
                "markdownTruncated".into(),
                json!(text_bytes.unwrap_or(0) > markdown.len() as u64),
            );
            value.insert("streamingState".into(), json!(status));
            let phase: Option<String> = row.try_get("phase").map_err(persistence_error)?;
            let mut metadata = Map::new();
            if let Some(phase) = phase {
                metadata.insert("phase".into(), json!(phase));
            }
            value.insert("metadata".into(), Value::Object(metadata));
        }
        "activity" => {
            let activity_kind: String = row.try_get("item_kind").map_err(persistence_error)?;
            let title: String = row.try_get("title").map_err(persistence_error)?;
            let tail = TAIL_PREVIEW_ACTIVITY_KINDS.contains(&activity_kind.as_str());
            let preview = text
                .as_deref()
                .map(|detail| activity_detail_preview(detail, ACTIVITY_DETAIL_PREVIEW_BYTES, tail));
            let detail_bytes = text_bytes.unwrap_or(0);
            value.insert(
                "activityKind".into(),
                json!(capped(&activity_kind, LIVE_LABEL_BYTES)),
            );
            value.insert("status".into(), json!(capped(&status, LIVE_LABEL_BYTES)));
            value.insert("title".into(), json!(capped(&title, LIVE_TITLE_BYTES)));
            value.insert("detailPreview".into(), json!(preview));
            value.insert("detailBytes".into(), json!(detail_bytes));
            value.insert(
                "detailTruncated".into(),
                json!(preview.is_some_and(|preview| (preview.len() as u64) < detail_bytes)),
            );
            value.insert("metadata".into(), Value::Object(activity_metadata(&row)?));
        }
        "plan" => {
            let text = text.unwrap_or_default();
            let markdown = activity_detail_preview(&text, LIVE_PLAN_MARKDOWN_BYTES, false);
            let steps: Option<String> = row.try_get("steps").map_err(persistence_error)?;
            value.insert("markdown".into(), json!(markdown));
            value.insert(
                "markdownTruncated".into(),
                json!(text_bytes.unwrap_or(0) > markdown.len() as u64),
            );
            value.insert("steps".into(), plan_steps(steps.as_deref())?);
            value.insert("state".into(), json!(status));
        }
        _ => return Err(corrupt_data_error()),
    }
    value.insert("createdAt".into(), json!(created_at));
    value.insert("updatedAt".into(), json!(updated_at));
    let change = unsigned(row.try_get("change_sequence").map_err(persistence_error)?)?;
    let item = ChatTimelineItemRead {
        activity_id: ChatActivityId::new(
            row.try_get::<String, _>("id").map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        turn_id: Some(turn_id.clone()),
        sequence_anchor: unsigned(row.try_get("sequence_anchor").map_err(persistence_error)?)?,
        change_sequence: change,
        kind,
        data: VersionedJson {
            schema_version: u32::try_from(
                row.try_get::<i64, _>("schema_version")
                    .map_err(persistence_error)?,
            )
            .map_err(|_| corrupt_data_error())?,
            value: Value::Object(value),
        },
        source_thread_id: None,
    };
    Ok((change, item))
}

/// The allowlisted metadata of an activity row: short tool labels, the exit code, and the
/// duration. Working directories, paths, inputs, and file changes are withheld.
fn activity_metadata(row: &sqlx::sqlite::SqliteRow) -> ChatResult<Map<String, Value>> {
    let mut metadata = Map::new();
    for (key, column) in
        LIVE_METADATA_TEXT_KEYS
            .into_iter()
            .zip(["tool_kind", "tool", "tool_name", "name"])
    {
        if let Some(text) = row
            .try_get::<Option<String>, _>(column)
            .map_err(persistence_error)?
        {
            metadata.insert(key.into(), json!(capped(&text, LIVE_LABEL_BYTES)));
        }
    }
    if let Some(exit_code) = row
        .try_get::<Option<i64>, _>("exit_code")
        .map_err(persistence_error)?
    {
        metadata.insert("exitCode".into(), json!(exit_code));
    }
    if let Some(duration) = row
        .try_get::<Option<i64>, _>("duration_ms")
        .map_err(persistence_error)?
        .filter(|duration| *duration >= 0)
    {
        metadata.insert("durationMs".into(), json!(duration));
    }
    let mut tool_input = Map::new();
    for (key, column) in [("query", "web_query"), ("url", "web_url")] {
        if let Some(text) = row
            .try_get::<Option<String>, _>(column)
            .map_err(persistence_error)?
        {
            tool_input.insert(key.into(), json!(capped(&text, LIVE_WEB_FIELD_BYTES)));
        }
    }
    if !tool_input.is_empty() {
        metadata.insert("toolInput".into(), Value::Object(tool_input));
    }
    if let Some(result) = row
        .try_get::<Option<String>, _>("web_result")
        .map_err(persistence_error)?
    {
        let result: Value = serde_json::from_str(&result).map_err(serialization_error)?;
        metadata.insert("webResult".into(), web_result(&result));
    }
    Ok(metadata)
}

/// The fields of a stored web search or fetch result that a live read keeps: at most
/// [`LIVE_WEB_LINKS`] result links with an http or https URL and a capped title, and a fetch's
/// URL, status code, and size.
fn web_result(result: &Value) -> Value {
    let mut kept = Map::new();
    if let Some(links) = result.get("links").and_then(Value::as_array) {
        let links = links
            .iter()
            .filter_map(|link| {
                let url = web_url(link.get("url"))?;
                let mut entry = Map::from_iter([("url".to_string(), json!(url))]);
                if let Some(title) = link.get("title").and_then(Value::as_str) {
                    entry.insert(
                        "title".into(),
                        json!(capped(title, LIVE_WEB_LINK_TITLE_BYTES)),
                    );
                }
                Some(Value::Object(entry))
            })
            .take(LIVE_WEB_LINKS)
            .collect();
        kept.insert("links".into(), Value::Array(links));
    }
    if let Some(url) = web_url(result.get("url")) {
        kept.insert("url".into(), json!(url));
    }
    for key in ["code", "bytes"] {
        if let Some(number) = result.get(key).and_then(Value::as_u64) {
            kept.insert(key.into(), json!(number));
        }
    }
    Value::Object(kept)
}

/// `value` when it is an http or https URL no longer than [`LIVE_WEB_FIELD_BYTES`].
fn web_url(value: Option<&Value>) -> Option<&str> {
    let url = value?.as_str()?;
    let scheme = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    (url.len() <= LIVE_WEB_FIELD_BYTES
        && (scheme.starts_with("https://") || scheme.starts_with("http://")))
    .then_some(url)
}

/// The first [`LIVE_PLAN_STEPS`] well-formed steps of a stored plan, each with only its id,
/// capped text, and status.
fn plan_steps(steps: Option<&str>) -> ChatResult<Value> {
    let Some(steps) = steps else {
        return Ok(Value::Array(Vec::new()));
    };
    let parsed: Value = serde_json::from_str(steps).map_err(serialization_error)?;
    let Value::Array(candidates) = parsed else {
        return Err(corrupt_data_error());
    };
    let steps = candidates
        .iter()
        .filter_map(|candidate| {
            let id = candidate.get("id")?.as_str()?;
            let text = candidate.get("text")?.as_str()?;
            let status = candidate.get("status")?.as_str()?;
            Some(json!({
                "id": capped(id, LIVE_LABEL_BYTES),
                "text": capped(text, LIVE_PLAN_STEP_TEXT_BYTES),
                "status": capped(status, LIVE_LABEL_BYTES),
            }))
        })
        .take(LIVE_PLAN_STEPS)
        .collect();
    Ok(Value::Array(steps))
}

/// At most `limit` bytes from the start of `text`, cut on a character boundary.
fn capped(text: &str, limit: usize) -> &str {
    activity_detail_preview(text, limit, false)
}

fn unsigned(value: i64) -> ChatResult<u64> {
    u64::try_from(value).map_err(|_| corrupt_data_error())
}
fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat live read persistence failed",
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
mod tests;
