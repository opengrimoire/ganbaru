use super::helpers::{
    insert_event, insert_open_run, memory_pool_migrated_through, migrate_through,
    migrated_memory_pool,
};

#[test]
fn schema_creates_distractions_usage_samples() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;

        sqlx::query(
            "INSERT INTO distractions_usage_samples
                (id, source_type, source_key, display_name, started_at_ms, elapsed_seconds, local_date, created_at_ms)
             VALUES ('sample-1', 'website', 'youtube.com', 'youtube.com', 1779923600000, 30, '2026-05-28', 1779923630000)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let invalid = sqlx::query(
            "INSERT INTO distractions_usage_samples
                (id, source_type, source_key, started_at_ms, elapsed_seconds, local_date, created_at_ms)
             VALUES ('sample-2', 'website', 'youtube.com', 1779923600000, 0, '2026-05-28', 1779923630000)",
        )
        .execute(&pool)
        .await;

        assert!(invalid.is_err());
    });
}

#[test]
fn schema_records_redacted_distractions_block_events() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_event(&pool).await;
        insert_open_run(&pool, "run-1").await.unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_segments
                (id, event_id, event_date, run_id, rhythm_position, phase,
                 planned_start, planned_end, actual_start, status)
             VALUES ('segment-1', 'event-1', '2026-05-23', 'run-1', 1, 'focus',
                     '2026-05-23T09:00:00Z', '2026-05-23T09:40:00Z',
                     '2026-05-23T09:00:00Z', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO distractions_block_events
                (id, run_id, segment_id, occurred_at, source_type,
                 source_key, display_name, phase, decision, rule_id, category_id)
             VALUES ('block-1', 'run-1', 'segment-1', '2026-05-23T09:10:00Z',
                     'browser', 'reddit.com', 'reddit.com', 'focus',
                     'blocked', 'rule-1', 'social')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO distractions_block_event_rule_snapshots
                (block_event_id, rule_id, rule_kind, rule_label,
                 environment_id, blocker_mode)
             VALUES ('block-1', 'rule-1', 'category', 'Social',
                     'env-1', 'blacklist')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let full_url = sqlx::query(
            "INSERT INTO distractions_block_events
                (id, occurred_at, source_type, source_key, phase, decision)
             VALUES ('block-bad', '2026-05-23T09:11:00Z',
                     'browser', 'https://reddit.com/r/all', 'focus', 'blocked')",
        )
        .execute(&pool)
        .await;
        assert!(full_url.is_err());
    });
}

#[test]
fn schema_accepts_mobile_app_rule_snapshots() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;

        sqlx::query(
            "INSERT INTO distractions_block_events
                (id, occurred_at, source_type, source_key, display_name, phase, decision, rule_id)
             VALUES ('mobile-block-1', '2026-08-29T18:00:00Z', 'mobile_app',
                     'com.example.video', 'Example video', 'focus', 'blocked', 'mobile-rule-1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO distractions_block_event_rule_snapshots
                (block_event_id, rule_id, rule_kind, rule_label, blocker_mode)
             VALUES ('mobile-block-1', 'mobile-rule-1', 'mobile_app',
                     'Example video', 'blacklist')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let second = sqlx::query(
            "INSERT INTO distractions_block_event_rule_snapshots
                (block_event_id, ordinal, rule_kind, blocker_mode)
             VALUES ('mobile-block-1', 1, 'mobile_app', 'blacklist')",
        )
        .execute(&pool)
        .await;
        assert!(second.is_err(), "an event keeps one rule snapshot");

        sqlx::query("DELETE FROM distractions_block_events WHERE id = 'mobile-block-1'")
            .execute(&pool)
            .await
            .unwrap();
        let snapshots: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM distractions_block_event_rule_snapshots")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(snapshots, 0);
    });
}

#[test]
fn deleting_a_focus_run_deletes_its_block_events() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_event(&pool).await;
        insert_open_run(&pool, "run-1").await.unwrap();
        sqlx::query(
            "INSERT INTO distractions_block_events
                (id, run_id, occurred_at, source_type, source_key, phase, decision)
             VALUES ('block-run', 'run-1', '2026-05-23T09:10:00Z', 'browser',
                     'reddit.com', 'focus', 'blocked'),
                    ('block-free', NULL, '2026-05-23T09:11:00Z', 'browser',
                     'reddit.com', NULL, 'blocked')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("DELETE FROM pomodoro_runs WHERE id = 'run-1'")
            .execute(&pool)
            .await
            .unwrap();
        let remaining: Vec<String> =
            sqlx::query_scalar("SELECT id FROM distractions_block_events ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, ["block-free"]);
    });
}

/// Migration that makes block history replicate; the one before it is the last old shape.
const BEFORE_BLOCK_HISTORY: i64 = 20261024142304;
const BLOCK_HISTORY: i64 = 20261024182105;

#[test]
fn block_history_migration_keeps_events_and_rule_snapshots() {
    super::block_on(async {
        let pool = memory_pool_migrated_through(BEFORE_BLOCK_HISTORY).await;
        insert_event(&pool).await;
        insert_open_run(&pool, "run-1").await.unwrap();
        sqlx::raw_sql(
            "UPDATE pomodoro_runs SET ended_at = '2026-05-23T10:00:00Z', end_reason = 'completed'
             WHERE id = 'run-1';
             INSERT INTO distractions_block_events
                (id, run_id, occurred_at, source_type, source_key, phase, decision)
             VALUES ('block-run', 'run-1', '2026-05-23T09:10:00Z', 'browser',
                     'reddit.com', 'focus', 'blocked'),
                    ('block-free', NULL, '2026-05-23T11:00:00Z', 'mobile_app',
                     'com.example.video', NULL, 'limit_exhausted');
             INSERT INTO distractions_block_event_rule_snapshots
                (block_event_id, rule_kind, rule_label, blocker_mode)
             VALUES ('block-run', 'domain', 'Social', 'blacklist'),
                    ('block-free', 'mobile_app', 'Video', 'limit');",
        )
        .execute(&pool)
        .await
        .unwrap();

        migrate_through(&pool, BLOCK_HISTORY).await.unwrap();

        let events: Vec<(String, Option<String>, String, String)> = sqlx::query_as(
            "SELECT id, run_id, source_key, decision FROM distractions_block_events ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            events,
            [
                (
                    "block-free".to_owned(),
                    None,
                    "com.example.video".to_owned(),
                    "limit_exhausted".to_owned()
                ),
                (
                    "block-run".to_owned(),
                    Some("run-1".to_owned()),
                    "reddit.com".to_owned(),
                    "blocked".to_owned()
                ),
            ]
        );
        let snapshots: Vec<(String, i64, String, String)> = sqlx::query_as(
            "SELECT block_event_id, ordinal, rule_label, blocker_mode
             FROM distractions_block_event_rule_snapshots ORDER BY block_event_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            snapshots,
            [
                (
                    "block-free".to_owned(),
                    0,
                    "Video".to_owned(),
                    "limit".to_owned()
                ),
                (
                    "block-run".to_owned(),
                    0,
                    "Social".to_owned(),
                    "blacklist".to_owned()
                ),
            ]
        );
        let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(enabled, 1);

        sqlx::query("DELETE FROM pomodoro_runs WHERE id = 'run-1'")
            .execute(&pool)
            .await
            .unwrap();
        let remaining: Vec<String> = sqlx::query_scalar(
            "SELECT block_event_id FROM distractions_block_event_rule_snapshots",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            remaining,
            ["block-free"],
            "an upgraded run takes its block events and their snapshots with it"
        );
    });
}
