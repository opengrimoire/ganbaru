use sqlx::{Row, SqlitePool};

pub(super) async fn migrated_memory_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    crate::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    pool
}

pub(super) async fn insert_event(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO calendar_events (id, title, start_time, end_time)
         VALUES ('event-1', 'Focus block', '2026-05-23T09:00:00Z', '2026-05-23T10:00:00Z')",
    )
    .execute(pool)
    .await
    .unwrap();
}

pub(super) async fn insert_open_run(
    pool: &SqlitePool,
    id: &str,
) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO pomodoro_runs
            (id, event_id, original_event_id, event_date, planned_start, planned_end,
             started_at, rhythm_kind, rhythm_source, preset_key, last_heartbeat, start_trigger)
         VALUES (?, 'event-1', 'event-1', '2026-05-23',
                 '2026-05-23T09:00:00Z', '2026-05-23T10:00:00Z',
                 '2026-05-23T09:00:00Z', 'count', 'preset', 'adaptive',
                 '2026-05-23T09:00:00Z', 'manual')",
    )
    .bind(id)
    .execute(pool)
    .await
}

pub(super) async fn query_plan(pool: &SqlitePool, sql: &str) -> String {
    sqlx::query(sqlx::AssertSqlSafe(format!("EXPLAIN QUERY PLAN {sql}")))
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn assert_plan_uses(plan: &str, expected: &str) {
    assert!(
        plan.contains(expected),
        "expected query plan to use {expected}, got:\n{plan}",
    );
}

/// One project with tasks, an archived external folder, soft-deleted statuses and priorities,
/// and a Chat graph of teammates, assignments, authorizations, runs, and scratch that references
/// the project and both working folders.
const PROJECT_CHAT_GRAPH: &str = include_str!("project_chat_graph.sql");

/// Loads the project and Chat graph.
pub(super) async fn load_project_chat_graph(pool: &SqlitePool) {
    sqlx::raw_sql(PROJECT_CHAT_GRAPH)
        .execute(pool)
        .await
        .unwrap();
}

/// Creates the default channel of `project-routine-learning` with its owner membership, as
/// Chat does on first use, since fresh vaults seed no conversations. Returns the conversation
/// and channel ids.
pub(super) async fn insert_default_channel(pool: &SqlitePool) -> (String, String) {
    sqlx::raw_sql(
        "INSERT INTO chat_conversations
            (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
         VALUES ('conversation-default-learning', 'project-routine-learning', 'channel',
                 '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z',
                 '2026-10-23T00:00:00.000Z');
         INSERT INTO chat_channels
            (id, project_id, conversation_id, name, is_default, created_at, updated_at)
         VALUES ('channel-default-learning', 'project-routine-learning',
                 'conversation-default-learning', 'general', 1, '2026-10-23T00:00:00.000Z',
                 '2026-10-23T00:00:00.000Z');
         INSERT INTO chat_conversation_memberships
            (conversation_id, participant_id, sync_key, membership_role, created_at, updated_at)
         VALUES ('conversation-default-learning', 'participant:local-owner',
                 'membership-default-learning-owner', 'owner', '2026-10-23T00:00:00.000Z',
                 '2026-10-23T00:00:00.000Z');",
    )
    .execute(pool)
    .await
    .unwrap();
    (
        "conversation-default-learning".to_owned(),
        "channel-default-learning".to_owned(),
    )
}
