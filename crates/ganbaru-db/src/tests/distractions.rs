use super::helpers::{insert_event, insert_open_run, migrated_memory_pool};

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

        let invalid = sqlx::query(
            "UPDATE distractions_block_event_rule_snapshots
             SET rule_kind = 'screen_content' WHERE block_event_id = 'mobile-block-1'",
        )
        .execute(&pool)
        .await;
        assert!(invalid.is_err());
    });
}
