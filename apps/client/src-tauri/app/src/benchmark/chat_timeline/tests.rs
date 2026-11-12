use super::*;
use ganbaru_chat::repository::reads::read_timeline_page;
use std::sync::Mutex as StdMutex;

struct RecordingEmitter {
    notifications: StdMutex<Vec<ChatChangeNotification>>,
}

impl ChatChangeEmitter for RecordingEmitter {
    fn emit(&self, notification: &ChatChangeNotification) -> ChatResult<()> {
        self.notifications
            .lock()
            .expect("recorded notifications")
            .push(notification.clone());
        Ok(())
    }
}

async fn benchmark_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    pool
}

async fn count(pool: &SqlitePool, table: &str, thread_id: &ChatThreadId) -> i64 {
    let sql = match table {
        "chat_activities" => "SELECT COUNT(*) FROM chat_activities WHERE thread_id = ?",
        "chat_messages" => "SELECT COUNT(*) FROM chat_messages WHERE thread_id = ?",
        "chat_plans" => "SELECT COUNT(*) FROM chat_plans WHERE thread_id = ?",
        "chat_turns" => "SELECT COUNT(*) FROM chat_turns WHERE thread_id = ?",
        other => panic!("no row count query for {other}"),
    };
    sqlx::query_scalar(sql)
        .bind(thread_id.as_str())
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn benchmark_thread_ids_must_use_the_benchmark_prefix() {
    assert!(benchmark_thread_id("benchmark-chat-v1-t5000").is_ok());
    assert!(benchmark_thread_id("thread-1").is_err());
    assert!(benchmark_thread_id("benchmark-chat-").is_ok());
}

#[tokio::test]
async fn long_thread_seed_writes_five_thousand_rows_in_the_planned_mix() {
    let pool = benchmark_pool().await;
    let thread_id = benchmark_thread_id("benchmark-chat-v1-t5000").unwrap();

    let summary = seed_chat_timeline(&pool, &thread_id, ChatTimelineDatasetShape::T5000)
        .await
        .unwrap();

    assert_eq!(summary.row_count, 5_000);
    assert_eq!(count(&pool, "chat_messages", &thread_id).await, 2_000);
    assert_eq!(count(&pool, "chat_activities", &thread_id).await, 2_500);
    assert_eq!(count(&pool, "chat_plans", &thread_id).await, 500);
    assert_eq!(count(&pool, "chat_turns", &thread_id).await, 500);
    let page = read_timeline_page(&pool, &thread_id, None, 200)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 200);
    assert!(page.previous_cursor.is_some());
}

#[tokio::test]
async fn flood_seed_writes_only_the_empty_thread() {
    let pool = benchmark_pool().await;
    let thread_id = benchmark_thread_id("benchmark-chat-v1-flood").unwrap();

    let summary = seed_chat_timeline(&pool, &thread_id, ChatTimelineDatasetShape::Flood)
        .await
        .unwrap();

    assert_eq!(summary.row_count, 0);
    assert_eq!(count(&pool, "chat_messages", &thread_id).await, 0);
    assert_eq!(count(&pool, "chat_activities", &thread_id).await, 0);
}

#[tokio::test]
async fn flood_replay_stores_the_whole_command_output_and_batches_notifications() {
    let pool = benchmark_pool().await;
    let thread_id = benchmark_thread_id("benchmark-chat-v1-flood").unwrap();
    seed_chat_timeline(&pool, &thread_id, ChatTimelineDatasetShape::Flood)
        .await
        .unwrap();
    let recorder = Arc::new(RecordingEmitter {
        notifications: StdMutex::new(Vec::new()),
    });
    let counting = Arc::new(CountingEmitter::new(recorder.clone()));
    let frozen = Instant::now();
    let ingestor = Mutex::new(
        ChatEventIngestor::new(pool.clone(), counting.clone()).with_clock(Arc::new(move || frozen)),
    );

    let ingested = replay_chat_stream(&ingestor, &thread_id, FloodPacing::IMMEDIATE)
        .await
        .unwrap();

    let command_deltas = FLOOD_COMMAND_OUTPUT_BYTES / FLOOD_COMMAND_DELTA_BYTES;
    assert_eq!(ingested, command_deltas + FLOOD_TEXT_DELTAS + 6);
    let detail: String = sqlx::query_scalar(
        "SELECT detail FROM chat_activities WHERE thread_id = ? AND item_kind = 'command_execution'",
    )
    .bind(thread_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(detail.len(), FLOOD_COMMAND_OUTPUT_BYTES);
    assert!(!detail.contains("truncated"));
    let message: String = sqlx::query_scalar(
        "SELECT normalized_markdown FROM chat_messages WHERE thread_id = ? AND role = 'assistant'",
    )
    .bind(thread_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(message.starts_with("token0 token1 "));
    assert!(message.contains(&format!("token{}", FLOOD_TEXT_DELTAS - 1)));
    let emitted = recorder.notifications.lock().unwrap().len() as u64;
    assert_eq!(counting.count(), emitted);
    assert!(emitted > 0);
    assert!(
        emitted < ingested as u64 / 10,
        "held deltas merge instead of announcing every delta, emitted {emitted}"
    );
}

#[test]
fn command_output_chunks_fill_the_delta_size_exactly() {
    let chunk = command_output_chunk();
    assert_eq!(chunk.len(), FLOOD_COMMAND_DELTA_BYTES);
    assert!(chunk.ends_with('\n'));
    assert_eq!(FLOOD_COMMAND_OUTPUT_BYTES % FLOOD_COMMAND_DELTA_BYTES, 0);
}
