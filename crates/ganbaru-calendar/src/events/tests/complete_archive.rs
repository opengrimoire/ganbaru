use super::fixtures::*;
use crate::events::metadata::Metadata;
use crate::events::occurrence::{ReadBudget, load_context};

fn target(id: &str) -> CalendarEventMutationTarget {
    CalendarEventMutationTarget { id: id.into() }
}

#[test]
fn archive_and_restore_preserve_task_links_music_versions_and_independent_imported_metadata() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        sqlx::raw_sql("UPDATE project_task_event_links SET link_kind='reference';
            INSERT INTO music_context_assignments (owner_kind, owner_id, phase, behavior, provenance_kind, updated_at_ms, version, soundscape_behavior)
                VALUES ('event-override', 'source', 'short-break', 'pause-music', 'explicit', 9, 7, 'keep-current-soundscape');")
            .execute(&pool).await.unwrap();
        let link: (String, String) = sqlx::query_as(
            "SELECT link_kind, created_at FROM project_task_event_links WHERE event_id='source'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        archive_calendar_event_tx(&mut tx, &target("source"))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let archive_component: String = sqlx::query_scalar(
            "SELECT icalendar_component_id FROM calendar_event_archives WHERE id='source'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_ne!(archive_component, "event");
        let assignments: Vec<(String, String, i64, String)> = sqlx::query_as("SELECT owner_kind, phase, version, soundscape_behavior FROM calendar_event_archive_music_assignments WHERE archive_event_id='source' ORDER BY owner_kind").fetch_all(&pool).await.unwrap();
        assert_eq!(
            assignments,
            vec![
                (
                    "event-override".into(),
                    "short-break".into(),
                    7,
                    "keep-current-soundscape".into()
                ),
                ("event-snapshot".into(), "focus".into(), 1, "inherit".into())
            ]
        );
        let live_assignments: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM music_context_assignments WHERE owner_id='source'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(live_assignments, 0);
        // Changing the old envelope cannot modify its archived independent copy.
        sqlx::query(
            "UPDATE icalendar_value_nodes SET text_value='changed' WHERE id='parameter-value'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let copied_value: String = sqlx::query_scalar("SELECT node.text_value FROM icalendar_value_nodes node JOIN icalendar_property_parameters parameter ON parameter.id=node.parameter_id JOIN icalendar_component_properties property ON property.id=parameter.property_id WHERE property.component_id=? AND parameter.name='x-param'")
            .bind(&archive_component).fetch_one(&pool).await.unwrap();
        assert_eq!(copied_value, "kept");
        let mut tx = pool.begin().await.unwrap();
        restore_archived_calendar_event_tx(&mut tx, &target("source"))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let restored_link: (String, String) = sqlx::query_as(
            "SELECT link_kind, created_at FROM project_task_event_links WHERE event_id='source'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(restored_link, link);
        let restored: Vec<(String, String, i64, String)> = sqlx::query_as("SELECT owner_kind, phase, version, soundscape_behavior FROM music_context_assignments WHERE owner_id='source' ORDER BY owner_kind").fetch_all(&pool).await.unwrap();
        assert_eq!(restored, assignments);
        let provenance: (String, String, String) = sqlx::query_as("SELECT attendee.id, alarm.id, override.id FROM calendar_event_attendees attendee JOIN calendar_event_alarms alarm ON alarm.event_id=attendee.event_id JOIN calendar_event_overrides override ON override.parent_event_id=attendee.event_id WHERE attendee.event_id='source'").fetch_one(&pool).await.unwrap();
        assert_eq!(
            provenance,
            ("attendee".into(), "alarm".into(), "override".into())
        );
        let config: i64 = sqlx::query_scalar("SELECT focus_duration_minutes FROM calendar_event_pomodoro_config_sequence_steps WHERE event_id='source'").fetch_one(&pool).await.unwrap();
        assert_eq!(config, 25);
        let extension: String = sqlx::query_scalar("SELECT property_value FROM calendar_event_override_extended_properties WHERE override_id='override'").fetch_one(&pool).await.unwrap();
        assert_eq!(extension, "override data");
        let restored_component: String = sqlx::query_scalar(
            "SELECT icalendar_component_id FROM calendar_events WHERE id='source'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(restored_component, archive_component);
        let retained: String = sqlx::query_scalar("SELECT node.text_value FROM icalendar_value_nodes node JOIN icalendar_property_parameters parameter ON parameter.id=node.parameter_id JOIN icalendar_component_properties property ON property.id=parameter.property_id WHERE property.component_id=? AND parameter.name='x-param'")
            .bind(&restored_component).fetch_one(&pool).await.unwrap();
        assert_eq!(retained, "kept");
    });
}

#[test]
fn repeated_scoped_archive_restore_releases_only_its_unused_import_copy() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        for _ in 0..2 {
            let mut tx = pool.begin().await.unwrap();
            archive_calendar_event_tx(&mut tx, &target("source::2099-05-10"))
                .await
                .unwrap();
            tx.commit().await.unwrap();
            let copies: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM icalendar_objects")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(copies, 2);
            let mut tx = pool.begin().await.unwrap();
            restore_archived_calendar_event_tx(&mut tx, &target("source::2099-05-10"))
                .await
                .unwrap();
            tx.commit().await.unwrap();
            let remaining: (i64, i64, i64) = sqlx::query_as(
                "SELECT
                (SELECT COUNT(*) FROM icalendar_objects),
                (SELECT COUNT(*) FROM calendar_event_archive_import_objects),
                (SELECT COUNT(*) FROM calendar_event_exdates WHERE event_id='source')",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(remaining, (1, 0, 0));
            let original: String = sqlx::query_scalar(
                "SELECT text_value FROM icalendar_value_nodes WHERE id='parameter-value'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(original, "kept");
        }
    });
}

#[test]
fn duplicate_archive_identity_never_overwrites_the_earlier_snapshot() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let mut tx = pool.begin().await.unwrap();
        let context = load_context(&mut tx, "source::2099-05-10").await.unwrap();
        let original = Metadata::read(&mut tx, "source", &mut ReadBudget::default())
            .await
            .unwrap()
            .prepare_archive("immutable-archive", &context)
            .unwrap();
        original
            .write(&mut tx, "2099-05-10T12:00:00Z")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        sqlx::query("UPDATE calendar_events SET title='Later edit' WHERE id='source'")
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let replacement = Metadata::read(&mut tx, "source", &mut ReadBudget::default())
            .await
            .unwrap()
            .prepare_archive("immutable-archive", &context)
            .unwrap();
        assert!(
            replacement
                .write(&mut tx, "2099-05-10T13:00:00Z")
                .await
                .is_err()
        );
        tx.rollback().await.unwrap();
        let title: String = sqlx::query_scalar(
            "SELECT title FROM calendar_event_archives WHERE id='immutable-archive'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_ne!(title, "Later edit");
        let objects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM icalendar_objects")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(objects, 2);
    });
}

#[test]
fn calendar_removal_protects_floating_dates_using_the_native_device_day() {
    crate::test_support::block_on(async {
        for (instant, today, archived) in [
            ("2026-05-10T01:00:00Z", "2026-05-09", false),
            ("2026-05-09T23:00:00Z", "2026-05-10", true),
        ] {
            let pool = super::metadata::seed().await;
            sqlx::raw_sql("INSERT INTO calendars (id, name, source) VALUES ('floating-calendar', 'Floating', 'local');
                INSERT INTO calendar_events (id, title, start_time, end_time, timezone, all_day, calendar_id)
                VALUES ('floating-event', 'Floating', '2026-05-10', '2026-05-10', 'UTC', 1, 'floating-calendar');")
                .execute(&pool).await.unwrap();
            let mut tx = pool.begin().await.unwrap();
            let epoch_ms = chrono::DateTime::parse_from_rfc3339(instant)
                .unwrap()
                .timestamp_millis();
            crate::events::archive_or_delete_calendar_events_for_calendar(
                &mut tx,
                Some("floating-calendar"),
                crate::recurrence::canonical::ScopeClock {
                    epoch_ms,
                    floating_today: Some(crate::recurrence::canonical::parse_date(today).unwrap()),
                },
            )
            .await
            .unwrap();
            tx.commit().await.unwrap();
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM calendar_event_archives WHERE id='floating-event'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(count != 0, archived, "{instant}, device day {today}");
            let live: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM calendar_events WHERE id='floating-event'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(live, 0);
        }
    });
}

#[test]
fn removing_the_original_calendar_preserves_archived_import_envelopes_and_components() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        sqlx::raw_sql("INSERT INTO calendars (id, name, source) VALUES ('removed-calendar', 'Imported', 'ics');
            UPDATE calendar_events SET calendar_id='removed-calendar' WHERE id='source';
            UPDATE icalendar_objects SET calendar_id='removed-calendar';
            UPDATE icalendar_components SET calendar_id='removed-calendar';")
            .execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        crate::events::archive_or_delete_calendar_events_for_calendar(
            &mut tx,
            Some("removed-calendar"),
            crate::recurrence::canonical::ScopeClock {
                epoch_ms: chrono::Utc::now().timestamp_millis(),
                floating_today: Some(chrono::Utc::now().date_naive()),
            },
        )
        .await
        .unwrap();
        crate::calendars::retain_archived_imports_tx(&mut tx, "removed-calendar")
            .await
            .unwrap();
        sqlx::query("DELETE FROM calendars WHERE id='removed-calendar'")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let archive: (String, String, String, String) = sqlx::query_as("SELECT archive.calendar_id, object.calendar_id, component.calendar_id, node.text_value
            FROM calendar_event_archives archive
            JOIN icalendar_components component ON component.id=archive.icalendar_component_id
            JOIN icalendar_objects object ON object.id=component.object_id
            JOIN icalendar_component_properties property ON property.component_id=component.id
            JOIN icalendar_property_parameters parameter ON parameter.property_id=property.id
            JOIN icalendar_value_nodes node ON node.parameter_id=parameter.id WHERE archive.id='source' AND parameter.name='x-param'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(
            archive,
            (
                "removed-calendar".into(),
                "local".into(),
                "local".into(),
                "kept".into()
            )
        );
        let owners: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_event_archive_import_objects WHERE archive_event_id='source'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(owners, 1);
    });
}

#[test]
fn moved_occurrence_archive_uses_its_override_metadata_and_current_focus_alias() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2099-05-01T09:00:00Z",
            "2099-05-01T10:00:00Z",
            Some("FREQ=DAILY;COUNT=30"),
        )
        .await;
        insert_test_completed_pomodoro_history(
            &pool,
            "event-1",
            "old-event::2099-05-19",
            "2099-05-19",
        )
        .await;
        sqlx::raw_sql("UPDATE pomodoro_runs SET current_occurrence_id='event-1::2099-05-20';
            INSERT INTO calendar_event_overrides (id, parent_event_id, recurrence_id, title, description, location, start_time, end_time)
                VALUES ('moved', 'event-1', '2099-05-20', 'Override title', '', 'Moved location', '2099-06-01T09:00:00Z', '2099-06-01T11:00:00Z');
            INSERT INTO calendar_event_extended_properties (id, event_id, property_key, property_value) VALUES ('base', 'event-1', 'X-EXTRA', 'base');
            INSERT INTO calendar_event_override_extended_properties (id, override_id, property_key, property_value) VALUES ('delta', 'moved', 'X-EXTRA', 'override');")
            .execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let error = delete_calendar_event_tx(&mut tx, &target("event-1::2099-05-20"))
            .await
            .unwrap_err();
        assert!(error.contains("archive it instead"), "{error}");
        tx.rollback().await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        archive_calendar_event_tx(&mut tx, &target("event-1::2099-05-20"))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let snapshot: (String, String, String, String, String) = sqlx::query_as("SELECT title, description, location, start_time, original_occurrence_id FROM calendar_event_archives WHERE id='event-1::2099-05-20'").fetch_one(&pool).await.unwrap();
        assert_eq!(
            snapshot,
            (
                "Override title".into(),
                "".into(),
                "Moved location".into(),
                "2099-06-01T09:00:00.000Z".into(),
                "event-1::2099-05-20".into()
            )
        );
        let property: String = sqlx::query_scalar("SELECT property_value FROM calendar_event_archive_extended_properties WHERE archive_event_id='event-1::2099-05-20'").fetch_one(&pool).await.unwrap();
        assert_eq!(property, "override");
        let run: (Option<String>, String, String) = sqlx::query_as(
            "SELECT event_id, original_event_id, event_date FROM pomodoro_runs WHERE id='run-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            run,
            (None, "old-event::2099-05-19".into(), "2099-05-19".into())
        );
        let mut tx = pool.begin().await.unwrap();
        restore_archived_calendar_event_tx(&mut tx, &target("event-1::2099-05-20"))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let restored: Option<String> =
            sqlx::query_scalar("SELECT event_id FROM pomodoro_runs WHERE id='run-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(restored.as_deref(), Some("event-1"));
    });
}

#[test]
fn an_opaque_archive_of_the_explicit_anchor_restores_only_that_occurrence() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let mut tx = pool.begin().await.unwrap();
        let context = load_context(&mut tx, "source::2099-05-09").await.unwrap();
        assert_eq!(context.canonical_id, "source");
        let prepared = Metadata::read(&mut tx, "source", &mut ReadBudget::default())
            .await
            .unwrap()
            .prepare_archive("opaque-archive", &context)
            .unwrap();
        prepared
            .write(&mut tx, "2099-05-09T12:00:00Z")
            .await
            .unwrap();
        crate::events::archive::add_exdate(&mut tx, "source", "2099-05-09", "2099-05-09T12:00:00Z")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let kind: (String, String) = sqlx::query_as("SELECT original_occurrence_id, recurrence_date FROM calendar_event_archives WHERE id='opaque-archive'").fetch_one(&pool).await.unwrap();
        assert_eq!(kind, ("source".into(), "2099-05-09".into()));
        let mut tx = pool.begin().await.unwrap();
        restore_archived_calendar_event_tx(&mut tx, &target("opaque-archive"))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let restored: (String, i64) = sqlx::query_as("SELECT rrule, (SELECT COUNT(*) FROM calendar_event_exdates WHERE event_id='source') FROM calendar_events WHERE id='source'").fetch_one(&pool).await.unwrap();
        assert_eq!(restored, ("FREQ=DAILY;COUNT=3".into(), 0));
    });
}

#[test]
fn late_archive_write_failure_rolls_back_copied_graph_children_and_live_references() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        sqlx::raw_sql("CREATE TRIGGER fail_archive_music BEFORE INSERT ON calendar_event_archive_music_assignments BEGIN SELECT RAISE(ABORT, 'injected late archive failure'); END;")
            .execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let error = archive_calendar_event_tx(&mut tx, &target("source"))
            .await
            .unwrap_err();
        assert!(error.contains("injected late archive failure"), "{error}");
        tx.rollback().await.unwrap();
        for (sql, expected) in [
            ("SELECT COUNT(*) FROM calendar_events WHERE id='source'", 1),
            ("SELECT COUNT(*) FROM calendar_event_archives", 0),
            ("SELECT COUNT(*) FROM icalendar_objects", 1),
            ("SELECT COUNT(*) FROM calendar_event_archive_alarms", 0),
            (
                "SELECT COUNT(*) FROM project_task_event_links WHERE event_id='source'",
                1,
            ),
            (
                "SELECT COUNT(*) FROM music_context_assignments WHERE owner_id='source'",
                1,
            ),
        ] {
            let count: i64 = sqlx::query_scalar(sql).fetch_one(&pool).await.unwrap();
            assert_eq!(count, expected, "{sql}");
        }
        sqlx::query("DROP TRIGGER fail_archive_music")
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        archive_calendar_event_tx(&mut tx, &target("source"))
            .await
            .unwrap();
        tx.commit().await.unwrap();
    });
}

#[test]
fn changed_task_ownership_prevents_restore_and_keeps_the_complete_archive() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let mut tx = pool.begin().await.unwrap();
        archive_calendar_event_tx(&mut tx, &target("source"))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        sqlx::query("DELETE FROM project_tasks WHERE id='task'")
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let error = restore_archived_calendar_event_tx(&mut tx, &target("source"))
            .await
            .unwrap_err();
        assert!(error.contains("task is missing"), "{error}");
        tx.rollback().await.unwrap();
        let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM calendar_events WHERE id='source'), (SELECT COUNT(*) FROM calendar_event_archives WHERE id='source'), (SELECT COUNT(*) FROM calendar_event_archive_task_links WHERE archive_event_id='source')").fetch_one(&pool).await.unwrap();
        assert_eq!(counts, (0, 1, 1));
    });
}
