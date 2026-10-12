use super::helpers::{insert_event, insert_open_run, migrated_memory_pool};

#[test]
fn schema_creates_normalized_calendar_archive_tables() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let archive_tables = [
            "calendar_event_archives",
            "calendar_event_archive_pomodoro_configs",
            "calendar_event_archive_pomodoro_config_count_rhythms",
            "calendar_event_archive_pomodoro_config_sequence_steps",
            "calendar_event_archive_notifications",
            "calendar_event_archive_exdates",
            "calendar_event_archive_rdates",
            "calendar_event_archive_categories",
            "calendar_event_archive_extended_properties",
            "calendar_event_archive_organizers",
            "calendar_event_archive_attendees",
            "calendar_event_archive_alarms",
            "calendar_event_archive_overrides",
            "calendar_event_archive_override_extended_properties",
            "calendar_event_archive_task_links",
            "calendar_event_archive_music_assignments",
            "calendar_event_archive_import_objects",
        ];
        for table in archive_tables {
            let exists: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?")
                    .bind(table)
                    .fetch_optional(&pool)
                    .await
                    .unwrap();
            assert_eq!(exists, Some(1), "{table} should exist");
        }
    });
}

#[test]
fn archive_metadata_keeps_historical_reference_values_and_enforces_constraints() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query("INSERT INTO calendar_event_archives (id, source_event_id, archived_at, title, start_time, end_time, calendar_id, created_at, updated_at)
            VALUES ('older', 'removed-source', '2026-05-23T11:00:00Z', 'Historical event', '2026-05-23T09:00:00Z', '2026-05-23T10:00:00Z', 'local', '2026-05-23T08:00:00Z', '2026-05-23T08:00:00Z')")
            .execute(&pool).await.unwrap();
        let older: (String, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT title, original_occurrence_id, recurrence_date FROM calendar_event_archives WHERE id='older'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(older, ("Historical event".into(), None, None));
        // Historical identities survive without their former live task/library.
        sqlx::raw_sql("INSERT INTO calendar_event_archive_task_links VALUES ('older', 'removed-task', 'reference', '2026-05-23T08:00:00Z');
            INSERT INTO calendar_event_archive_music_assignments
                VALUES ('older', 'event-override', 'short-break', 'play-automatically', 'removed-playlist', 'removed-soundscape', 'copied-project', 'removed-project', 9, 7, 'play-selected');")
            .execute(&pool).await.unwrap();
        let historical: (String, i64) = sqlx::query_as("SELECT playlist_id, version FROM calendar_event_archive_music_assignments WHERE archive_event_id='older'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(historical, ("removed-playlist".into(), 7));
        for sql in [
            "UPDATE calendar_event_archive_task_links SET link_kind='unknown'",
            "UPDATE calendar_event_archive_music_assignments SET version=0",
            "UPDATE calendar_event_archive_music_assignments SET phase='short_break'",
            "UPDATE calendar_event_archives SET recurrence_date='invalid'",
            "INSERT INTO calendar_event_archive_import_objects VALUES ('older', 'missing-object')",
        ] {
            assert!(sqlx::query(sql).execute(&pool).await.is_err(), "{sql}");
        }
        sqlx::query("DELETE FROM calendar_event_archives WHERE id='older'")
            .execute(&pool)
            .await
            .unwrap();
        let children: (i64, i64) = sqlx::query_as(
            "SELECT
            (SELECT COUNT(*) FROM calendar_event_archive_task_links),
            (SELECT COUNT(*) FROM calendar_event_archive_music_assignments)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(children, (0, 0));
    });
}

#[test]
fn schema_rejects_invalid_calendar_values() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;

        assert!(
            sqlx::query(
                "INSERT INTO calendars (id, name, source, created_at, updated_at)
             VALUES ('bad-source', 'Bad', 'web', '2026-05-23T00:00:00Z', '2026-05-23T00:00:00Z')",
            )
            .execute(&pool)
            .await
            .is_err()
        );

        assert!(
            sqlx::query(
                "INSERT INTO calendar_events
                (id, title, start_time, end_time, timezone, calendar_id, all_day)
             VALUES ('bad-bool', 'Bad', '2026-05-23T09:00:00Z',
                     '2026-05-23T10:00:00Z', 'UTC', 'local', 2)",
            )
            .execute(&pool)
            .await
            .is_err()
        );

        assert!(
            sqlx::query(
                "INSERT INTO calendar_events
                (id, title, start_time, end_time, timezone, calendar_id, color)
             VALUES ('bad-color', 'Bad', '2026-05-23T09:00:00Z',
                     '2026-05-23T10:00:00Z', 'UTC', 'local', 32)",
            )
            .execute(&pool)
            .await
            .is_err()
        );

        assert!(
            sqlx::query(
                "INSERT INTO calendar_events
                (id, title, start_time, end_time, timezone, calendar_id, priority)
             VALUES ('bad-priority', 'Bad', '2026-05-23T09:00:00Z',
                     '2026-05-23T10:00:00Z', 'UTC', 'local', 10)",
            )
            .execute(&pool)
            .await
            .is_err()
        );

        assert!(
            sqlx::query(
                "INSERT INTO calendar_events
                (id, title, start_time, end_time, timezone, calendar_id, geo_lat)
             VALUES ('bad-geo', 'Bad', '2026-05-23T09:00:00Z',
                     '2026-05-23T10:00:00Z', 'UTC', 'local', 25.0)",
            )
            .execute(&pool)
            .await
            .is_err()
        );

        assert!(
            sqlx::query(
                "INSERT INTO calendar_events
                (id, title, start_time, end_time, timezone, calendar_id)
             VALUES ('bad-fk', 'Bad', '2026-05-23T09:00:00Z',
                     '2026-05-23T10:00:00Z', 'UTC', 'missing')",
            )
            .execute(&pool)
            .await
            .is_err()
        );
    });
}

#[test]
fn deleting_calendar_event_preserves_pomodoro_segments() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_event(&pool).await;
        insert_open_run(&pool, "run-1").await.unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_segments
                (id, event_id, event_date, run_id, rhythm_position, phase,
                 planned_start, planned_end, actual_start, actual_end, status, end_reason)
             VALUES ('segment-1', 'event-1', '2026-05-23', 'run-1', 1, 'focus',
                     '2026-05-23T09:00:00Z', '2026-05-23T09:40:00Z',
                     '2026-05-23T09:00:00Z', '2026-05-23T09:20:00Z',
                     'interrupted', 'stopped')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("DELETE FROM calendar_events WHERE id = 'event-1'")
            .execute(&pool)
            .await
            .unwrap();

        let segment_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_segments WHERE id = 'segment-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let event_id: Option<String> =
            sqlx::query_scalar("SELECT event_id FROM pomodoro_segments WHERE id = 'segment-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(segment_count, 1);
        assert_eq!(event_id, None);
    });
}
