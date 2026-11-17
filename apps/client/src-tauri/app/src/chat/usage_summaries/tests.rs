use super::*;
use ganbaru_sync_contracts::chat_usage::agent_usage_key;

const AGENT: &str = "participant:usage";
const ASSIGNMENT: &str = "assignment:usage";
/// 2026-11-12T23:30:00Z, which is already 2026-11-13 in Tokyo.
const EPOCH_MS: i64 = 1_794_526_200_000;

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
         INSERT INTO chat_agents (participant_id, role, created_at, updated_at)
         VALUES ('{AGENT}', 'Counts', '2026-11-12T23:00:00.000Z', '2026-11-12T23:00:00.000Z');
         INSERT INTO chat_work_assignments
            (id, reply_thread_id, agent_id, triggering_message_item_id, state, created_at,
             updated_at)
         VALUES ('{ASSIGNMENT}', 'reply-thread:usage', '{AGENT}', 'item:usage', 'working',
                 '2026-11-12T23:00:00.000Z', '2026-11-12T23:00:00.000Z');
         INSERT INTO chat_agent_runs
            (id, assignment_id, project_id, agent_policy_revision_id,
             authorization_revision_id, authorization_scope_digest, provider_turn_id,
             provider_thread_id, state, run_ordinal, started_at, created_at, updated_at)
         VALUES ('run:usage', '{ASSIGNMENT}', 'project-usage', 'policy:usage',
                 'authorization:usage', '{digest}', 'turn:usage', 'thread:usage', 'completed',
                 1, '2026-11-12T23:00:00.000Z', '2026-11-12T23:00:00.000Z',
                 '2026-11-12T23:00:00.000Z');
         UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;
         INSERT INTO chat_agent_run_usage
            (run_id, input_tokens, counted_tokens, working_seconds, updated_at)
         VALUES ('run:usage', 400, 400, 90, '2026-11-12T23:20:00.000Z');",
        digest = "a".repeat(64),
    )))
    .execute(&pool)
    .await
    .unwrap();
    pool
}

#[tokio::test]
async fn agent_usage_flushes_on_the_local_date_and_reads_summed_across_devices() {
    let pool = migrated_pool().await;
    let tokyo = TimeZone::get("Asia/Tokyo").unwrap();
    assert_eq!(
        flush_at(&pool, "device-a", EPOCH_MS, &tokyo, FlushScope::All)
            .await
            .unwrap(),
        1
    );
    sqlx::query(
        "INSERT INTO chat_agent_usage_daily
            (id, device_id, agent_id, local_date, counted_tokens, working_seconds, run_count)
         VALUES (?, 'device-b', ?, '2026-11-13', 100, 10, 1)",
    )
    .bind(
        agent_usage_key("device-b", AGENT, "2026-11-13")
            .unwrap()
            .to_string(),
    )
    .bind(AGENT)
    .execute(&pool)
    .await
    .unwrap();

    let agent = ChatParticipantId::new(AGENT).unwrap();
    let days = read_usage_at(&pool, &agent, "2026-11-01", "2026-11-30", EPOCH_MS, &tokyo)
        .await
        .unwrap();
    assert_eq!(
        days,
        [ChatAgentUsageDayRead {
            local_date: "2026-11-13".to_owned(),
            counted_tokens: 500,
            working_seconds: 100,
            run_count: 2,
            device_ids: vec!["device-a".to_owned(), "device-b".to_owned()],
        }]
    );

    // In UTC the same instant is still 2026-11-12, so new usage lands on that date.
    sqlx::query("UPDATE chat_agent_run_usage SET input_tokens = 450, counted_tokens = 450 WHERE run_id = 'run:usage'")
        .execute(&pool)
        .await
        .unwrap();
    flush_at(&pool, "device-a", EPOCH_MS, &TimeZone::UTC, FlushScope::All)
        .await
        .unwrap();
    let utc_days = read_usage_at(
        &pool,
        &agent,
        "2026-11-12",
        "2026-11-12",
        EPOCH_MS,
        &TimeZone::UTC,
    )
    .await
    .unwrap();
    assert_eq!(
        utc_days
            .iter()
            .map(|day| (day.local_date.as_str(), day.counted_tokens, day.run_count))
            .collect::<Vec<_>>(),
        [("2026-11-12", 50, 0)]
    );
}
