use super::*;

const DEVICE: &str = "device-a";
const AGENT: &str = "participant:usage";
const ASSIGNMENT: &str = "assignment:usage";
const DATE: &str = "2026-11-12";
const NOW: &str = "2026-11-12T10:01:00.000Z";

async fn migrated_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "PRAGMA foreign_keys=OFF;
         UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         INSERT INTO chat_agents (participant_id, provider_family_id, created_at, updated_at)
         VALUES ('{AGENT}', 'codex', '{NOW}', '{NOW}');
         INSERT INTO chat_work_assignments
            (id, reply_thread_id, agent_id, triggering_message_item_id, state,
             model_selection_schema_version, model_selection_data, created_at, updated_at)
         VALUES ('{ASSIGNMENT}', 'reply-thread:usage', '{AGENT}', 'item:usage', 'working',
                 1, '{{}}', '{NOW}', '{NOW}');
         UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;"
    )))
    .execute(&pool)
    .await
    .unwrap();
    pool
}

/// Seeds run `run_id` of [`ASSIGNMENT`], started when `started`, with `tokens` counted,
/// `working_seconds` closed, and an open working interval from `working_since` when given.
async fn seed_run(
    pool: &SqlitePool,
    run_id: &str,
    ordinal: i64,
    started: bool,
    tokens: i64,
    working_seconds: i64,
    working_since: Option<&str>,
) {
    let started_at = if started {
        format!("'{NOW}'")
    } else {
        "NULL".to_owned()
    };
    let working_since = working_since.map_or("NULL".to_owned(), |since| format!("'{since}'"));
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         INSERT INTO chat_agent_runs
            (id, assignment_id, project_id,
             authorization_revision_id, authorization_scope_digest, provider_turn_id,
             provider_thread_id, state, run_ordinal, started_at, created_at, updated_at)
         VALUES ('{run_id}', '{ASSIGNMENT}', 'project-usage',
                 'authorization:usage', '{digest}', 'turn:{run_id}', 'thread:usage', 'working',
                 {ordinal}, {started_at}, '{NOW}', '{NOW}');
         UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;
         INSERT INTO chat_agent_run_usage
            (run_id, input_tokens, counted_tokens, working_seconds, working_since, updated_at)
         VALUES ('{run_id}', {tokens}, {tokens}, {working_seconds}, {working_since}, '{NOW}');",
        digest = "a".repeat(64),
    )))
    .execute(pool)
    .await
    .unwrap();
}

async fn set_usage(pool: &SqlitePool, run_id: &str, tokens: i64, working_seconds: i64) {
    sqlx::query(
        "UPDATE chat_agent_run_usage
         SET input_tokens = ?1, counted_tokens = ?1, working_seconds = ?2, working_since = NULL
         WHERE run_id = ?3",
    )
    .bind(tokens)
    .bind(working_seconds)
    .bind(run_id)
    .execute(pool)
    .await
    .unwrap();
}

async fn flush(pool: &SqlitePool, date: &str, now: &str, scope: FlushScope) -> usize {
    flush_agent_usage(pool, DEVICE, date, now, scope)
        .await
        .unwrap()
}

/// The stored counts of this device's summary of [`AGENT`] on `date`.
async fn summary(pool: &SqlitePool, date: &str) -> Option<(i64, i64, i64)> {
    sqlx::query_as(
        "SELECT counted_tokens, working_seconds, run_count FROM chat_agent_usage_daily
         WHERE id = ?",
    )
    .bind(agent_usage_key(DEVICE, AGENT, date).unwrap().to_string())
    .fetch_optional(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn flush_adds_new_usage_once_and_counts_each_run_once() {
    let pool = migrated_pool().await;
    seed_run(&pool, "run:1", 1, true, 100, 30, None).await;
    seed_run(&pool, "run:queued", 2, false, 0, 0, None).await;

    assert_eq!(flush(&pool, DATE, NOW, FlushScope::All).await, 1);
    assert_eq!(summary(&pool, DATE).await, Some((100, 30, 1)));
    assert_eq!(flush(&pool, DATE, NOW, FlushScope::All).await, 0);
    assert_eq!(summary(&pool, DATE).await, Some((100, 30, 1)));

    set_usage(&pool, "run:1", 150, 45).await;
    assert_eq!(flush(&pool, DATE, NOW, FlushScope::All).await, 1);
    assert_eq!(summary(&pool, DATE).await, Some((150, 45, 1)));
}

#[tokio::test]
async fn flush_counts_an_open_working_interval_only_when_asked_and_once() {
    let pool = migrated_pool().await;
    seed_run(
        &pool,
        "run:1",
        1,
        true,
        0,
        10,
        Some("2026-11-12T10:00:00.000Z"),
    )
    .await;
    assert_eq!(flush(&pool, DATE, NOW, FlushScope::NotWorking).await, 0);
    assert_eq!(summary(&pool, DATE).await, None);

    assert_eq!(flush(&pool, DATE, NOW, FlushScope::All).await, 1);
    assert_eq!(summary(&pool, DATE).await, Some((0, 70, 1)));

    // The interval closes at 61 seconds; only the second the summary has not seen is added.
    set_usage(&pool, "run:1", 0, 71).await;
    assert_eq!(flush(&pool, DATE, NOW, FlushScope::NotWorking).await, 1);
    assert_eq!(summary(&pool, DATE).await, Some((0, 71, 1)));
}

#[tokio::test]
async fn usage_after_midnight_lands_on_the_new_local_date() {
    let pool = migrated_pool().await;
    seed_run(&pool, "run:1", 1, true, 40, 5, None).await;
    flush(&pool, DATE, NOW, FlushScope::All).await;
    set_usage(&pool, "run:1", 100, 20).await;
    flush(&pool, "2026-11-13", NOW, FlushScope::All).await;
    assert_eq!(summary(&pool, DATE).await, Some((40, 5, 1)));
    assert_eq!(summary(&pool, "2026-11-13").await, Some((60, 15, 0)));
}

#[tokio::test]
async fn an_overflowing_count_keeps_its_last_value_and_is_not_retried() {
    let pool = migrated_pool().await;
    sqlx::query(
        "INSERT INTO chat_agent_usage_daily
            (id, device_id, agent_id, local_date, counted_tokens, working_seconds, run_count)
         VALUES (?, ?, ?, ?, ?, 0, 0)",
    )
    .bind(agent_usage_key(DEVICE, AGENT, DATE).unwrap().to_string())
    .bind(DEVICE)
    .bind(AGENT)
    .bind(DATE)
    .bind(MAX_DAILY_COUNTED_TOKENS - 10)
    .execute(&pool)
    .await
    .unwrap();
    seed_run(&pool, "run:1", 1, true, 11, 7, None).await;
    assert_eq!(flush(&pool, DATE, NOW, FlushScope::All).await, 1);
    assert_eq!(
        summary(&pool, DATE).await,
        Some((MAX_DAILY_COUNTED_TOKENS - 10, 7, 1))
    );
    assert_eq!(flush(&pool, DATE, NOW, FlushScope::All).await, 0);
}

#[tokio::test]
async fn flush_refuses_a_malformed_local_date() {
    let pool = migrated_pool().await;
    for date in ["2026-11-1", "2026/11/12", "2026-13-01", ""] {
        assert!(
            flush_agent_usage(&pool, DEVICE, date, NOW, FlushScope::All)
                .await
                .is_err(),
            "{date}"
        );
    }
}

#[tokio::test]
async fn read_sums_devices_per_day_inside_the_retention_window() {
    let pool = migrated_pool().await;
    for (device, date, tokens, seconds, runs) in [
        ("device-b", "2026-11-12", 30, 3, 1),
        ("device-a", "2026-11-12", 70, 7, 2),
        ("device-a", "2026-11-11", 5, 1, 1),
        ("device-a", "2026-08-14", 9, 9, 9),
    ] {
        sqlx::query(
            "INSERT INTO chat_agent_usage_daily
                (id, device_id, agent_id, local_date, counted_tokens, working_seconds,
                 run_count)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(agent_usage_key(device, AGENT, date).unwrap().to_string())
        .bind(device)
        .bind(AGENT)
        .bind(date)
        .bind(tokens)
        .bind(seconds)
        .bind(runs)
        .execute(&pool)
        .await
        .unwrap();
    }
    let days = read_agent_usage(&pool, AGENT, "2026-01-01", "2026-11-12", DATE)
        .await
        .unwrap();
    assert_eq!(
        days,
        [
            ChatAgentUsageDayRead {
                local_date: "2026-11-11".to_owned(),
                counted_tokens: 5,
                working_seconds: 1,
                run_count: 1,
                device_ids: vec!["device-a".to_owned()],
            },
            ChatAgentUsageDayRead {
                local_date: "2026-11-12".to_owned(),
                counted_tokens: 100,
                working_seconds: 10,
                run_count: 3,
                device_ids: vec!["device-a".to_owned(), "device-b".to_owned()],
            },
        ]
    );
    assert_eq!(
        read_agent_usage(&pool, AGENT, "2026-11-11", "2026-11-11", DATE)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        read_agent_usage(&pool, AGENT, "2026-11-12", "2026-11-11", DATE)
            .await
            .is_err()
    );
    assert!(
        read_agent_usage(&pool, AGENT, "2026-08-01", "2026-08-10", DATE)
            .await
            .unwrap()
            .is_empty()
    );
}
