use super::helpers::{
    insert_event, insert_open_run, memory_pool_migrated_through, migrated_memory_pool,
};
use crate::run_migrations;

/// Last migration before Calendar tables replicate.
const BEFORE_CALENDAR_REPLICATION: i64 = 20261023120329;

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
                VALUES ('older', 'event-override', 'short-break', 'play-automatically', 'removed-playlist', 'copied-project', 'removed-project', 9, 7, 'play-selected', 'removed-preset');")
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

async fn insert_legacy_calendar_graph(pool: &sqlx::SqlitePool) {
    sqlx::raw_sql(
        "INSERT INTO calendars (id, name, source, created_at, updated_at)
         VALUES ('imported', 'Imported', 'ics', '2026-05-23T00:00:00Z', '2026-05-23T00:00:00Z');
         INSERT INTO calendar_events (id, title, start_time, end_time, calendar_id, geo_lat, geo_lng)
         VALUES ('event-geo', 'Geo', '2026-05-23T09:00:00Z', '2026-05-23T10:00:00Z', 'imported', 45.1234567, -93.5),
                ('event-plain', 'Plain', '2026-05-23T11:00:00Z', '2026-05-23T12:00:00Z', 'imported', NULL, NULL);
         INSERT INTO icalendar_objects (id, calendar_id, source_kind, source_name, source_fingerprint, created_at, updated_at)
         VALUES ('object-a', 'imported', 'import-file', 'a.ics', 'fingerprint-a', '2026-05-23T00:00:00Z', '2026-05-23T00:00:00Z'),
                ('object-b', 'imported', 'import-file', 'a.ics', 'fingerprint-b', '2026-05-23T00:00:00Z', '2026-05-23T00:00:00Z'),
                ('object-c', 'imported', 'import-file', 'a.ics', 'fingerprint-c', '2026-05-24T00:00:00Z', '2026-05-24T00:00:00Z');
         INSERT INTO icalendar_components (id, object_id, calendar_id, component_type, preservation_status, created_at, updated_at)
         VALUES ('component-a', 'object-a', 'imported', 'vevent', 'lossless', 'now', 'now'),
                ('component-c', 'object-c', 'imported', 'vevent', 'lossless', 'now', 'now');
         INSERT INTO icalendar_component_properties (id, component_id, name, value_type)
         VALUES ('property-a', 'component-a', 'x-count', 'integer');
         INSERT INTO icalendar_property_parameters (id, property_id, name)
         VALUES ('parameter-a', 'property-a', 'x-unit');
         INSERT INTO icalendar_value_nodes (id, property_id, parameter_id, value_kind, number_value)
         VALUES ('node-a', 'property-a', NULL, 'number', 1.5),
                ('node-b', 'property-a', 'parameter-a', 'text', NULL);
         INSERT INTO calendar_event_archives
             (id, source_event_id, archived_at, start_time, end_time, calendar_id, geo_lat, geo_lng, created_at, updated_at)
         VALUES ('archive-a', 'event-gone', '2026-05-23T00:00:00Z', '2026-05-23T09:00:00Z',
                 '2026-05-23T10:00:00Z', 'imported', -0.0000004, 179.9999996, 'now', 'now');
         INSERT INTO calendar_event_archive_import_objects (archive_event_id, object_id)
         VALUES ('archive-a', 'object-a');
         INSERT INTO project_groups (id, name, icon, order_key) VALUES ('group-1', 'Work', 'lucide:folder', 'a0');
         INSERT INTO projects (id, group_id, name, icon, order_key)
         VALUES ('project-1', 'group-1', 'Launch', 'lucide:folder', 'a0');
         INSERT INTO project_sections (id, project_id, name, order_key)
         VALUES ('section-1', 'project-1', 'Now', 'a0');
         INSERT INTO project_statuses (id, project_id, name, category, order_key)
         VALUES ('status-1', 'project-1', 'To do', 'not_started', 'a0');
         INSERT INTO project_tasks (id, project_id, section_id, status_id, title, section_order_key, status_order_key)
         VALUES ('task-1', 'project-1', 'section-1', 'status-1', 'Plan', 'a0', 'a0');
         INSERT INTO project_task_event_links (task_id, event_id) VALUES ('task-1', 'event-plain');",
    )
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn calendar_replication_migration_keeps_rows_and_converts_reals() {
    super::block_on(async {
        let pool = memory_pool_migrated_through(BEFORE_CALENDAR_REPLICATION).await;
        insert_legacy_calendar_graph(&pool).await;

        run_migrations(&pool).await.unwrap();

        let counts: (i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM calendar_events),
                    (SELECT COUNT(*) FROM icalendar_objects),
                    (SELECT COUNT(*) FROM icalendar_components),
                    (SELECT COUNT(*) FROM icalendar_component_properties),
                    (SELECT COUNT(*) FROM icalendar_property_parameters),
                    (SELECT COUNT(*) FROM icalendar_value_nodes),
                    (SELECT COUNT(*) FROM calendar_event_archive_import_objects)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(counts, (2, 3, 2, 1, 1, 2, 1));

        let geo: Vec<(String, Option<i64>, Option<i64>)> = sqlx::query_as(
            "SELECT id, geo_lat, geo_lng FROM calendar_events
             UNION ALL SELECT id, geo_lat, geo_lng FROM calendar_event_archives ORDER BY 1",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            geo,
            vec![
                ("archive-a".into(), Some(0), Some(180_000_000)),
                ("event-geo".into(), Some(45_123_457), Some(-93_500_000)),
                ("event-plain".into(), None, None),
            ],
            "coordinates round to the nearest millionth of a degree"
        );

        let nodes: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
            "SELECT id, component_id, typeof(number_value), number_value
             FROM icalendar_value_nodes ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            nodes,
            vec![
                (
                    "node-a".into(),
                    "component-a".into(),
                    "text".into(),
                    Some("1.5".into())
                ),
                ("node-b".into(), "component-a".into(), "null".into(), None),
            ]
        );
        let parameter_component: String =
            sqlx::query_scalar("SELECT component_id FROM icalendar_property_parameters")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(parameter_component, "component-a");

        let imports: Vec<String> =
            sqlx::query_scalar("SELECT import_id FROM icalendar_objects ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(imports[0], imports[1], "one import id per earlier batch");
        assert_ne!(
            imports[0], imports[2],
            "a later batch takes its own import id"
        );

        let placeholders: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM calendar_event_archive_import_objects
                     WHERE sync_key GLOB 'pending:*')
                  + (SELECT COUNT(*) FROM project_task_event_links WHERE sync_key GLOB 'pending:*')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            placeholders, 2,
            "the conversion pass rewrites placeholder keys"
        );

        sqlx::query("DELETE FROM calendars WHERE id = 'imported'")
            .execute(&pool)
            .await
            .unwrap();
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 0, "events cascade with their calendar");
    });
}

#[test]
fn calendar_replication_migration_refuses_a_dangling_reference() {
    super::block_on(async {
        let pool = memory_pool_migrated_through(BEFORE_CALENDAR_REPLICATION).await;
        sqlx::raw_sql(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO calendar_events (id, title, start_time, end_time, calendar_id)
             VALUES ('orphan', 'Orphan', '2026-05-23T09:00:00Z', '2026-05-23T10:00:00Z', 'missing');
             PRAGMA foreign_keys = ON;",
        )
        .execute(&pool)
        .await
        .unwrap();

        let error = run_migrations(&pool)
            .await
            .expect_err("the foreign key guard refuses a dangling calendar");
        assert!(
            error.to_string().contains("CHECK constraint failed"),
            "{error}"
        );
    });
}
