//! Chat timeline benchmark datasets. The long thread is written through the Chat event
//! repository, and the stream flood is replayed through the production event ingestor, so both
//! measure the same rows and notifications a provider session produces.

use chrono::{DateTime, Duration as TimeDelta, SecondsFormat, Utc};
use ganbaru_chat::events::{
    CanonicalEvent, CanonicalRuntimeEvent, ContentDeltaEvent, ItemLifecycleEvent,
    ProposedPlanCompletedEvent, ProposedPlanDeltaEvent, TurnCompletedEvent, TurnStartedEvent,
};
use ganbaru_chat::ingestion::{ChatChangeEmitter, ChatEventIngestor};
use ganbaru_chat::models::{
    ActivityStatus, CanonicalItemKind, ChatChangeNotification, ChatEventId, ChatResult,
    ChatThreadId, ChatTurnId, ChatTurnState, ContentStreamKind, InteractionMode, ProviderFamilyId,
    ProviderInstanceId, SafetyMode, TurnModeSnapshot, UtcTimestamp,
};
use ganbaru_chat::repository::events::{AppendCanonicalEventRequest, append_canonical_event};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Thread IDs the harness may seed; anything else is refused so a fixture never lands in a
/// thread the user could have created.
pub(crate) const CHAT_BENCHMARK_THREAD_PREFIX: &str = "benchmark-chat-";
const PROJECT_GROUP_ID: &str = "benchmark-chat";
const PROJECT_ID: &str = "benchmark-chat";
const WORKING_FOLDER_ID: &str = "benchmark-chat-folder";
const PROVIDER_FAMILY_ID: &str = "codex";
const PROVIDER_INSTANCE_ID: &str = "benchmark";
/// Fixed start of the seeded long thread, so every run stores the same timestamps.
const LONG_THREAD_START: &str = "2026-01-01T09:00:00Z";

/// Turns in the long thread; each turn carries the row mix below.
const LONG_THREAD_TURNS: usize = 500;
/// Assistant messages per turn (40 percent of the rows).
const MESSAGES_PER_TURN: usize = 4;
/// Tool activities per turn (50 percent of the rows).
const ACTIVITIES_PER_TURN: usize = 5;
/// Proposed plans per turn (10 percent of the rows).
const PLANS_PER_TURN: usize = 1;

/// Size of one streamed command output delta in the flood.
pub(crate) const FLOOD_COMMAND_DELTA_BYTES: usize = 4 * 1024;
/// Command output the flood streams in total, exactly the per-command artifact limit.
pub(crate) const FLOOD_COMMAND_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
/// Assistant text deltas the flood streams after the command.
pub(crate) const FLOOD_TEXT_DELTAS: usize = 2_000;
/// Wait between command output deltas: output arrives faster than a reader refreshes.
const FLOOD_COMMAND_DELTA_INTERVAL: Duration = Duration::from_millis(1);
/// Wait between assistant text deltas, about 200 deltas a second, a fast model's token rate.
const FLOOD_TEXT_DELTA_INTERVAL: Duration = Duration::from_millis(5);

/// Markdown bodies the long thread cycles through, covering the blocks the renderer handles.
const MESSAGE_BODIES: [&str; 4] = [
    "## Findings\n\nThe parser keeps **every** block and the `render` step stays pure.\n\n- First check passed\n- Second check passed\n- Third check needs a retry",
    "Here is the change:\n\n```ts\nexport function sum(values: number[]): number {\n  return values.reduce((total, value) => total + value, 0);\n}\n```\n\nIt keeps the old signature.",
    "| File | Lines | State |\n| --- | --- | --- |\n| `model.ts` | 120 | updated |\n| `view.svelte` | 48 | unchanged |\n\nSee [the guide](https://example.com/guide) for the table rules.",
    "1. Read the failing test\n2. Reproduce it with a smaller input\n3. Fix the boundary\n\n> The boundary was off by one when the list was empty.",
];

/// Tool activity kinds the long thread cycles through, with the title each one shows.
const ACTIVITY_KINDS: [(CanonicalItemKind, &str); 5] = [
    (CanonicalItemKind::CommandExecution, "pnpm test"),
    (CanonicalItemKind::FileChange, "Edit src/lib/model.ts"),
    (CanonicalItemKind::DynamicToolCall, "Read"),
    (CanonicalItemKind::WebSearch, "Search the documentation"),
    (CanonicalItemKind::McpToolCall, "List project tasks"),
];

/// A Chat benchmark dataset shape, matching the harness dataset profiles.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChatTimelineDatasetShape {
    /// One settled thread with 5000 timeline rows.
    T5000,
    /// One empty thread the stream flood replays into.
    Flood,
}

/// What a Chat dataset seed wrote.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTimelineSeedSummary {
    pub thread_id: String,
    /// Timeline rows the seed wrote: messages, activities, and plans.
    pub row_count: usize,
}

/// What one stream flood replay did.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStreamReplaySummary {
    /// Change notifications the ingestor emitted, held deltas included.
    pub emitted_notifications: u64,
    /// Canonical events offered to the ingestor.
    pub ingested_events: usize,
    /// Wall time from the first ingested event to the final flush.
    pub duration_ms: f64,
}

/// Waits between flood deltas; tests use [`FloodPacing::IMMEDIATE`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct FloodPacing {
    pub command_delta_interval: Duration,
    pub text_delta_interval: Duration,
}

impl FloodPacing {
    /// The pacing the benchmark command replays at.
    pub(crate) const BENCHMARK: Self = Self {
        command_delta_interval: FLOOD_COMMAND_DELTA_INTERVAL,
        text_delta_interval: FLOOD_TEXT_DELTA_INTERVAL,
    };
    /// No waits, so a test replays the flood as fast as the ingestor stores it.
    #[cfg(test)]
    pub(crate) const IMMEDIATE: Self = Self {
        command_delta_interval: Duration::ZERO,
        text_delta_interval: Duration::ZERO,
    };
}

/// Counts notifications on their way to another emitter.
pub(crate) struct CountingEmitter {
    inner: Arc<dyn ChatChangeEmitter>,
    count: AtomicU64,
}

impl CountingEmitter {
    pub(crate) fn new(inner: Arc<dyn ChatChangeEmitter>) -> Self {
        Self {
            inner,
            count: AtomicU64::new(0),
        }
    }

    pub(crate) fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }
}

impl ChatChangeEmitter for CountingEmitter {
    fn emit(&self, notification: &ChatChangeNotification) -> ChatResult<()> {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.inner.emit(notification)
    }
}

/// Validates a harness thread ID against [`CHAT_BENCHMARK_THREAD_PREFIX`].
pub(crate) fn benchmark_thread_id(value: &str) -> Result<ChatThreadId, String> {
    if !value.starts_with(CHAT_BENCHMARK_THREAD_PREFIX) {
        return Err(format!(
            "Chat benchmark thread IDs start with {CHAT_BENCHMARK_THREAD_PREFIX}"
        ));
    }
    ChatThreadId::new(value).map_err(|_| "Chat benchmark thread ID is not valid".to_string())
}

/// Writes one Chat dataset into `pool`: the project, working folder, and thread it needs, and
/// for [`ChatTimelineDatasetShape::T5000`] the whole long thread.
pub(crate) async fn seed_chat_timeline(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    shape: ChatTimelineDatasetShape,
) -> Result<ChatTimelineSeedSummary, String> {
    let start = parse_start()?;
    seed_thread_parents(pool, thread_id, &timestamp(start)?).await?;
    let row_count = match shape {
        ChatTimelineDatasetShape::T5000 => seed_long_thread(pool, thread_id, start).await?,
        ChatTimelineDatasetShape::Flood => 0,
    };
    Ok(ChatTimelineSeedSummary {
        thread_id: thread_id.as_str().to_string(),
        row_count,
    })
}

async fn seed_thread_parents(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    now: &UtcTimestamp,
) -> Result<(), String> {
    let mut transaction = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    sqlx::query(
        "INSERT OR IGNORE INTO project_groups (id, name, order_key) VALUES (?, 'Benchmark', 'a1')",
    )
    .bind(PROJECT_GROUP_ID)
    .execute(&mut *transaction)
    .await
    .map_err(|e| format!("seed benchmark project group: {e}"))?;
    sqlx::query(
        "INSERT OR IGNORE INTO projects (id, group_id, name, order_key)
         VALUES (?, ?, 'Chat benchmark', 'a1')",
    )
    .bind(PROJECT_ID)
    .bind(PROJECT_GROUP_ID)
    .execute(&mut *transaction)
    .await
    .map_err(|e| format!("seed benchmark project: {e}"))?;
    sqlx::query(
        "INSERT OR IGNORE INTO project_working_folders
            (id, project_id, display_name, kind, managed_relative_path,
             repository_kind, sort_order, created_at, updated_at)
         VALUES (?, ?, 'Benchmark files', 'managed', 'projects/benchmark-chat', 'none', 0, ?, ?)",
    )
    .bind(WORKING_FOLDER_ID)
    .bind(PROJECT_ID)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(|e| format!("seed benchmark working folder: {e}"))?;
    ganbaru_chat::project_defaults::ensure_project_chat_defaults(&mut transaction, PROJECT_ID, now)
        .await
        .map_err(|error| format!("seed benchmark Chat defaults: {}", error.message))?;
    sqlx::query(
        "INSERT INTO chat_threads
            (id, project_id, working_folder_id, title, provider_family_id, provider_instance_id,
             continuation_group_id, safety_mode, interaction_mode, state,
             last_activity_at, created_at, updated_at)
         VALUES (?, ?, ?, 'Benchmark thread', ?, ?, ?, 'ask_for_approval', 'build', 'idle', ?, ?, ?)",
    )
    .bind(thread_id.as_str())
    .bind(PROJECT_ID)
    .bind(WORKING_FOLDER_ID)
    .bind(PROVIDER_FAMILY_ID)
    .bind(PROVIDER_INSTANCE_ID)
    .bind(format!("{}-continuation", thread_id.as_str()))
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(|e| format!("seed benchmark Chat thread: {e}"))?;
    transaction
        .commit()
        .await
        .map_err(|e| format!("commit: {e}"))
}

/// Writes [`LONG_THREAD_TURNS`] settled turns, one second of event time apart per event.
async fn seed_long_thread(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    start: DateTime<Utc>,
) -> Result<usize, String> {
    let mut writer = EventWriter::new(thread_id.clone(), start);
    let mut rows = 0;
    for turn in 0..LONG_THREAD_TURNS {
        let turn_id = format!("turn-{turn:04}");
        writer.append(pool, &turn_id, turn_started()).await?;
        for slot in 0..MESSAGES_PER_TURN + ACTIVITIES_PER_TURN + PLANS_PER_TURN {
            let event = long_thread_row(turn, slot);
            writer.append(pool, &turn_id, event).await?;
            rows += 1;
            if slot >= MESSAGES_PER_TURN + ACTIVITIES_PER_TURN {
                let plan_id = plan_id(turn, slot);
                let completed = CanonicalEvent::ProposedPlanCompleted(ProposedPlanCompletedEvent {
                    markdown: plan_markdown(turn),
                    plan_id,
                });
                writer.append(pool, &turn_id, completed).await?;
            }
        }
        writer.append(pool, &turn_id, turn_completed()).await?;
    }
    Ok(rows)
}

/// The row at `slot` of `turn`: messages and activities alternate, and the plan ends the turn.
fn long_thread_row(turn: usize, slot: usize) -> CanonicalEvent {
    let row_slots = MESSAGES_PER_TURN + ACTIVITIES_PER_TURN;
    if slot >= row_slots {
        return CanonicalEvent::ProposedPlanDelta(ProposedPlanDeltaEvent {
            plan_id: plan_id(turn, slot),
            delta: plan_markdown(turn),
            content_index: 0,
        });
    }
    // Even slots below twice the message count are messages, so a turn reads message, tool,
    // message, tool, and ends with its remaining tools.
    let message_index = slot / 2;
    if slot.is_multiple_of(2) && message_index < MESSAGES_PER_TURN {
        return CanonicalEvent::ItemCompleted(ItemLifecycleEvent {
            item_id: format!("message-{turn:04}-{slot}"),
            kind: CanonicalItemKind::AssistantMessage,
            status: ActivityStatus::Completed,
            title: Some("Assistant message".to_string()),
            detail: Some(MESSAGE_BODIES[(turn + message_index) % MESSAGE_BODIES.len()].to_string()),
            safe_metadata: None,
        });
    }
    let (kind, title) = ACTIVITY_KINDS[(turn + slot) % ACTIVITY_KINDS.len()];
    CanonicalEvent::ItemCompleted(ItemLifecycleEvent {
        item_id: format!("activity-{turn:04}-{slot}"),
        kind,
        status: ActivityStatus::Completed,
        title: Some(title.to_string()),
        detail: Some(format!("{title} finished for step {turn}")),
        safe_metadata: None,
    })
}

fn plan_id(turn: usize, slot: usize) -> String {
    format!("plan-{turn:04}-{slot}")
}

fn plan_markdown(turn: usize) -> String {
    format!("1. Inspect step {turn}\n2. Apply the change\n3. Run the checks")
}

/// Replays the stream flood into `thread_id`: one turn whose command streams
/// [`FLOOD_COMMAND_OUTPUT_BYTES`] of output in [`FLOOD_COMMAND_DELTA_BYTES`] deltas, then
/// [`FLOOD_TEXT_DELTAS`] assistant text deltas. The caller owns the ingestor and its idle flush,
/// as a provider session does.
pub(crate) async fn replay_chat_stream(
    ingestor: &Mutex<ChatEventIngestor>,
    thread_id: &ChatThreadId,
    pacing: FloodPacing,
) -> Result<usize, String> {
    let mut writer = EventWriter::new(thread_id.clone(), Utc::now());
    let turn_id = "turn-flood";
    let command_id = "flood-command";
    let message_id = "flood-message";
    writer.ingest(ingestor, turn_id, turn_started()).await?;
    writer
        .ingest(
            ingestor,
            turn_id,
            item(
                command_id,
                CanonicalItemKind::CommandExecution,
                ActivityStatus::Active,
            ),
        )
        .await?;
    let output_chunk = command_output_chunk();
    for _ in 0..FLOOD_COMMAND_OUTPUT_BYTES / FLOOD_COMMAND_DELTA_BYTES {
        pause(pacing.command_delta_interval).await;
        let delta = content_delta(
            command_id,
            ContentStreamKind::CommandOutput,
            output_chunk.clone(),
        );
        writer.ingest(ingestor, turn_id, delta).await?;
    }
    writer
        .ingest(
            ingestor,
            turn_id,
            item(
                command_id,
                CanonicalItemKind::CommandExecution,
                ActivityStatus::Completed,
            ),
        )
        .await?;
    writer
        .ingest(
            ingestor,
            turn_id,
            item(
                message_id,
                CanonicalItemKind::AssistantMessage,
                ActivityStatus::Active,
            ),
        )
        .await?;
    for index in 0..FLOOD_TEXT_DELTAS {
        pause(pacing.text_delta_interval).await;
        let delta = content_delta(
            message_id,
            ContentStreamKind::AssistantText,
            text_delta(index),
        );
        writer.ingest(ingestor, turn_id, delta).await?;
    }
    writer
        .ingest(
            ingestor,
            turn_id,
            item(
                message_id,
                CanonicalItemKind::AssistantMessage,
                ActivityStatus::Completed,
            ),
        )
        .await?;
    writer.ingest(ingestor, turn_id, turn_completed()).await?;
    ingestor
        .lock()
        .await
        .flush()
        .await
        .map_err(|error| format!("flush the Chat flood: {}", error.message))?;
    Ok(writer.count)
}

/// Replays the flood with the production idle flush running beside it and reports its timing.
pub(crate) async fn replay_chat_stream_with_idle_flush(
    pool: SqlitePool,
    emitter: Arc<dyn ChatChangeEmitter>,
    thread_id: &ChatThreadId,
) -> Result<ChatStreamReplaySummary, String> {
    let counting = Arc::new(CountingEmitter::new(emitter));
    let ingestor = Arc::new(Mutex::new(ChatEventIngestor::new(pool, counting.clone())));
    let idle_flush = tauri::async_runtime::spawn(ganbaru_chat::ingestion::run_idle_flush(
        Arc::downgrade(&ingestor),
    ));
    let started = Instant::now();
    let replayed = replay_chat_stream(&ingestor, thread_id, FloodPacing::BENCHMARK).await;
    let duration_ms = started.elapsed().as_secs_f64() * 1_000.0;
    idle_flush.abort();
    Ok(ChatStreamReplaySummary {
        emitted_notifications: counting.count(),
        ingested_events: replayed?,
        duration_ms,
    })
}

async fn pause(interval: Duration) {
    if !interval.is_zero() {
        tokio::time::sleep(interval).await;
    }
}

/// One [`FLOOD_COMMAND_DELTA_BYTES`] chunk of numbered ASCII output lines.
fn command_output_chunk() -> String {
    let mut chunk = String::with_capacity(FLOOD_COMMAND_DELTA_BYTES);
    let mut line = 0;
    while chunk.len() < FLOOD_COMMAND_DELTA_BYTES {
        chunk.push_str(&format!("test {line:05} passed in the benchmark suite\n"));
        line += 1;
    }
    chunk.truncate(FLOOD_COMMAND_DELTA_BYTES - 1);
    chunk.push('\n');
    chunk
}

fn text_delta(index: usize) -> String {
    if index % 40 == 39 {
        format!("token{index}.\n\n")
    } else {
        format!("token{index} ")
    }
}

fn item(item_id: &str, kind: CanonicalItemKind, status: ActivityStatus) -> CanonicalEvent {
    let event = ItemLifecycleEvent {
        item_id: item_id.to_string(),
        kind,
        status,
        title: Some(match kind {
            CanonicalItemKind::CommandExecution => "pnpm test".to_string(),
            _ => "Assistant message".to_string(),
        }),
        detail: None,
        safe_metadata: None,
    };
    if status == ActivityStatus::Completed {
        CanonicalEvent::ItemCompleted(event)
    } else {
        CanonicalEvent::ItemStarted(event)
    }
}

fn content_delta(item_id: &str, stream_kind: ContentStreamKind, delta: String) -> CanonicalEvent {
    CanonicalEvent::ContentDelta(ContentDeltaEvent {
        item_id: item_id.to_string(),
        stream_kind,
        content_index: 0,
        delta,
    })
}

fn turn_started() -> CanonicalEvent {
    CanonicalEvent::TurnStarted(TurnStartedEvent {
        provider_turn_id: None,
        state: ChatTurnState::Active,
        modes: TurnModeSnapshot {
            safety_mode: SafetyMode::AskForApproval,
            interaction_mode: InteractionMode::Build,
        },
        model_id: None,
        model_options: Vec::new(),
    })
}

fn turn_completed() -> CanonicalEvent {
    CanonicalEvent::TurnCompleted(TurnCompletedEvent {
        state: ChatTurnState::Completed,
        stop_reason: None,
        usage: None,
        changed_files: Vec::new(),
    })
}

/// Builds canonical events with unique IDs and increasing event times for one thread.
struct EventWriter {
    thread_id: ChatThreadId,
    start: DateTime<Utc>,
    count: usize,
}

impl EventWriter {
    fn new(thread_id: ChatThreadId, start: DateTime<Utc>) -> Self {
        Self {
            thread_id,
            start,
            count: 0,
        }
    }

    fn request(
        &mut self,
        turn_id: &str,
        event: CanonicalEvent,
    ) -> Result<AppendCanonicalEventRequest, String> {
        let offset = i64::try_from(self.count).map_err(|_| "too many benchmark events")?;
        let created_at = timestamp(self.start + TimeDelta::milliseconds(offset * 1_000))?;
        let event_id = ChatEventId::new(format!(
            "{}-event-{:06}",
            self.thread_id.as_str(),
            self.count
        ))
        .map_err(|_| "benchmark Chat event ID is not valid".to_string())?;
        let turn_id = ChatTurnId::new(turn_id)
            .map_err(|_| "benchmark Chat turn ID is not valid".to_string())?;
        self.count += 1;
        Ok(AppendCanonicalEventRequest {
            runtime: CanonicalRuntimeEvent {
                schema_version: 1,
                event_id,
                provider_family_id: ProviderFamilyId::new(PROVIDER_FAMILY_ID)
                    .map_err(|_| "benchmark provider family is not valid".to_string())?,
                provider_instance_id: ProviderInstanceId::new(PROVIDER_INSTANCE_ID)
                    .map_err(|_| "benchmark provider instance is not valid".to_string())?,
                thread_id: self.thread_id.clone(),
                created_at: created_at.clone(),
                turn_id: Some(turn_id),
                provider_turn_id: None,
                provider_item_id: None,
                provider_request_id: None,
                provider_task_id: None,
                provider_reference: None,
                event,
                redacted_diagnostic: None,
            },
            ingested_at: created_at,
            diagnostic_expires_at: None,
        })
    }

    async fn append(
        &mut self,
        pool: &SqlitePool,
        turn_id: &str,
        event: CanonicalEvent,
    ) -> Result<(), String> {
        let request = self.request(turn_id, event)?;
        append_canonical_event(pool, request)
            .await
            .map(|_| ())
            .map_err(|error| format!("seed benchmark Chat event: {}", error.message))
    }

    async fn ingest(
        &mut self,
        ingestor: &Mutex<ChatEventIngestor>,
        turn_id: &str,
        event: CanonicalEvent,
    ) -> Result<(), String> {
        let request = self.request(turn_id, event)?;
        ingestor
            .lock()
            .await
            .ingest(request)
            .await
            .map_err(|error| format!("replay benchmark Chat event: {}", error.message))
    }
}

fn parse_start() -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(LONG_THREAD_START)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|e| format!("benchmark Chat start time: {e}"))
}

fn timestamp(value: DateTime<Utc>) -> Result<UtcTimestamp, String> {
    UtcTimestamp::new(value.to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| "benchmark Chat timestamp is not valid".to_string())
}

#[cfg(test)]
mod tests;
