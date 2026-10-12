use super::*;

#[test]
fn native_window_selects_civil_labels_across_render_dates_and_filters_exact_instants() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        sqlx::raw_sql("INSERT INTO calendar_events(id, title, start_time, end_time, timezone, calendar_id)
            VALUES ('east', 'East', '2024-07-01 00:15', '2024-07-01 00:45', 'Pacific/Kiritimati', 'native-calendar'),
                   ('west', 'West', '2024-06-29 23:00', '2024-06-29 23:30', 'America/Adak', 'native-calendar'),
                   ('outside', 'Outside', '2024-06-30 12:00', '2024-06-30 12:30', 'Pacific/Kiritimati', 'native-calendar');")
            .execute(&pool).await.unwrap();
        let window = Window::new(
            "2024-06-30",
            "2024-06-30",
            &civil_time::zone("UTC").unwrap(),
        )
        .unwrap();
        let result = snapshot(&pool, &window, false).await;
        assert!(result.diagnostics.is_empty());
        assert_eq!(result.occurrences.len(), 2);
        for (id, date, start, end) in [
            (
                "east",
                "2024-07-01",
                "2024-06-30T10:15:00.000Z",
                "2024-06-30T10:45:00.000Z",
            ),
            (
                "west",
                "2024-06-29",
                "2024-06-30T08:00:00.000Z",
                "2024-06-30T08:30:00.000Z",
            ),
        ] {
            let row = result.occurrences.iter().find(|row| row.id == id).unwrap();
            assert_eq!(row.recurrence_date, date);
            assert_eq!(row.start_time, start);
            assert_eq!(row.end_time, end);
        }
    });
}

#[test]
fn native_window_keeps_civil_gap_and_legacy_home_labels_for_every_consumer() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        sqlx::raw_sql("INSERT INTO calendar_events(id, title, start_time, end_time, timezone, calendar_id)
            VALUES ('gap', 'Explicit gap', '2024-03-10T02:00:00', '2024-03-10T02:30:00', 'America/New_York', 'native-calendar'),
                   ('civil', 'Legacy civil', '2024-03-10 00:15', '2024-03-10 00:45', 'America/New_York', 'native-calendar');
            INSERT INTO calendar_event_pomodoro_configs(event_id, rhythm_kind, rhythm_source)
                SELECT id, 'count', 'custom' FROM calendar_events WHERE id IN ('gap', 'civil');
            INSERT INTO calendar_event_pomodoro_config_count_rhythms(event_id, focus_duration_minutes, short_break_minutes, long_break_minutes, long_break_after_focus_count)
                SELECT id, 25, 5, 15, 4 FROM calendar_events WHERE id IN ('gap', 'civil');
            INSERT INTO calendar_event_notifications(id, event_id, offset_minutes)
                VALUES ('gap-notice', 'gap', 10), ('civil-notice', 'civil', 10);")
            .execute(&pool).await.unwrap();
        let window = Window::new(
            "2024-03-10",
            "2024-03-10",
            &civil_time::zone("America/New_York").unwrap(),
        )
        .unwrap();
        for purpose in [
            WindowPurpose::Render,
            WindowPurpose::Focus,
            WindowPurpose::Notifications,
        ] {
            let mut tx = pool.begin().await.unwrap();
            let source = read(&mut tx, &window, purpose, false).await.unwrap();
            tx.commit().await.unwrap();
            let result = source.expand(&window).unwrap();
            assert!(result.diagnostics.is_empty());
            for (id, start, end) in [
                (
                    "gap",
                    "2024-03-10T07:00:00.000Z",
                    "2024-03-10T07:30:00.000Z",
                ),
                (
                    "civil",
                    "2024-03-10T05:15:00.000Z",
                    "2024-03-10T05:45:00.000Z",
                ),
            ] {
                let row = result
                    .occurrences
                    .iter()
                    .find(|row| row.id == id)
                    .unwrap_or_else(|| panic!("Native window lost home-zone occurrence {id}"));
                assert_eq!(row.start_time, start);
                assert_eq!(row.end_time, end);
            }
        }
    });
}

async fn pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ganbaru_db::run_migrations(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO calendars(id, name, source) VALUES('native-calendar', 'Native', 'local');
        INSERT INTO calendar_events(id, title, start_time, end_time, timezone, calendar_id, rrule)
        VALUES('series', 'Home-zone series', '2024-03-09T14:00:00Z', '2024-03-09T15:00:00Z', 'America/New_York', 'native-calendar', 'FREQ=DAILY;COUNT=3');")
        .execute(&pool).await.unwrap();
    pool
}

#[test]
fn empty_native_windows_do_not_prepare_child_queries_and_preserve_requested_totals() {
    use sqlx::Connection;
    crate::test_support::block_on(async {
        let pool = pool().await;
        let window = window("2024-03-09", "2024-03-15");
        let mut tx = pool.begin().await.unwrap();
        tx.clear_cached_statements().await.unwrap();
        // The fixture has a Calendar event, but no Focus configuration.
        let source = read(&mut tx, &window, WindowPurpose::Focus, false)
            .await
            .unwrap();
        assert!(source.events.is_empty());
        assert_eq!(source.total_event_count, None);
        assert_eq!(tx.cached_statements_size(), 1);
        let source = read(&mut tx, &window, WindowPurpose::Focus, true)
            .await
            .unwrap();
        assert_eq!(source.total_event_count, Some(1));
        assert_eq!(tx.cached_statements_size(), 2);
        let result = source.expand(&window).unwrap();
        assert!(result.occurrences.is_empty());
        assert!(result.overrides.is_empty());
        assert!(result.attendees.is_empty());
        tx.rollback().await.unwrap();
        pool.close().await;
    });
}

fn window(start: &str, end: &str) -> Window {
    Window::new(start, end, &civil_time::zone("UTC").unwrap()).unwrap()
}

async fn snapshot(
    pool: &SqlitePool,
    window: &Window,
    scheduler_only: bool,
) -> NativeCalendarWindow {
    let mut tx = pool.begin().await.unwrap();
    let purpose = if scheduler_only {
        WindowPurpose::Focus
    } else {
        WindowPurpose::Render
    };
    let source = read(&mut tx, window, purpose, true).await.unwrap();
    tx.commit().await.unwrap();
    source.expand(window).unwrap()
}

#[test]
fn native_notification_window_filters_before_expansion_and_retains_reminder_offsets() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        sqlx::raw_sql("INSERT INTO calendar_events(id, title, start_time, end_time, timezone, calendar_id, rrule)
            VALUES('silent-series', 'No reminders', '2024-03-09T14:00:00Z', '2024-03-09T15:00:00Z', 'UTC', 'native-calendar', 'FREQ=DAILY');
            INSERT INTO calendar_event_notifications(id, event_id, offset_minutes) VALUES('notice', 'series', 10);
            INSERT INTO calendar_event_attendees(id, event_id, email, role, status) VALUES('guest', 'series', 'guest@example.test', 'req-participant', 'accepted');")
            .execute(&pool).await.unwrap();
        let window = window("2024-03-09", "2024-03-15");
        let mut tx = pool.begin().await.unwrap();
        let source = read(&mut tx, &window, WindowPurpose::Notifications, false)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let result = source.expand(&window).unwrap();
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].id, "series");
        assert_eq!(result.events[0].notifications.as_deref(), Some("[10]"));
        assert_eq!(result.occurrences.len(), 3);
        assert_eq!(result.occurrences[1].start_time, "2024-03-10T13:00:00.000Z");
        assert!(result.attendees.is_empty());
    });
}

#[test]
fn native_window_expands_canonical_home_zone_with_all_children_in_one_snapshot() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        sqlx::raw_sql("INSERT INTO calendar_event_exdates(id, event_id, occurrence_date) VALUES('excluded', 'series', '2024-03-10');
            INSERT INTO calendar_event_rdates(id, event_id, occurrence_start) VALUES('added', 'series', '2024-03-20T13:00:00Z');
            INSERT INTO calendar_event_notifications(id, event_id, offset_minutes) VALUES('notice', 'series', 10);
            INSERT INTO calendar_event_attendees(id, event_id, email, role, status) VALUES('guest', 'series', 'guest@example.test', 'req-participant', 'accepted');")
            .execute(&pool).await.unwrap();
        let result = snapshot(&pool, &window("2024-03-09", "2024-03-20"), false).await;
        assert!(result.diagnostics.is_empty());
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.attendees.len(), 1);
        assert_eq!(result.events[0].notifications.as_deref(), Some("[10]"));
        assert_eq!(result.total_event_count, Some(1));
        assert_eq!(
            result
                .occurrences
                .iter()
                .map(|value| value.id.as_str())
                .collect::<Vec<_>>(),
            ["series", "series::2024-03-11", "series::2024-03-20"]
        );
        assert_eq!(result.occurrences[1].start_time, "2024-03-11T13:00:00.000Z");
    });
}

#[test]
fn native_window_applies_off_window_override_without_losing_recurrence_provenance() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        sqlx::query("INSERT INTO calendar_event_overrides(id, parent_event_id, recurrence_id, title, start_time, end_time)
            VALUES('moved', 'series', '2024-03-10T13:00:00Z', 'Moved', '2024-03-20T13:00:00Z', '2024-03-20T14:00:00Z')")
            .execute(&pool).await.unwrap();
        let result = snapshot(&pool, &window("2024-03-20", "2024-03-20"), false).await;
        assert_eq!(result.occurrences.len(), 1);
        assert_eq!(result.occurrences[0].id, "series::2024-03-10");
        assert_eq!(result.occurrences[0].recurrence_date, "2024-03-10");
        assert_eq!(result.occurrences[0].override_id.as_deref(), Some("moved"));
        assert_eq!(result.overrides[0].title.as_deref(), Some("Moved"));
    });
}

#[test]
fn native_window_releases_sqlite_before_expansion_and_keeps_its_committed_snapshot() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let window = window("2024-03-09", "2024-03-15");
        let mut tx = pool.begin().await.unwrap();
        let source = read(&mut tx, &window, WindowPurpose::Render, false)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        sqlx::query("UPDATE calendar_events SET title='Later edit', rrule='FREQ=DAILY;COUNT=5' WHERE id='series'")
            .execute(&pool).await.unwrap();
        let before = source.expand(&window).unwrap();
        assert_eq!(before.events[0].title, "Home-zone series");
        assert_eq!(before.occurrences.len(), 3);
        let after = snapshot(&pool, &window, false).await;
        assert_eq!(after.events[0].title, "Later edit");
        assert_eq!(after.occurrences.len(), 5);
    });
}

#[test]
fn native_window_reports_preservation_only_rules_without_silently_reinterpreting_them() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        sqlx::query("UPDATE calendar_events SET rrule='FREQ=DAILY;BYHOUR=9,15' WHERE id='series'")
            .execute(&pool)
            .await
            .unwrap();
        let result = snapshot(&pool, &window("2024-03-09", "2024-03-15"), false).await;
        assert_eq!(
            result.events[0].rrule.as_deref(),
            Some("FREQ=DAILY;BYHOUR=9,15")
        );
        assert!(result.occurrences.is_empty());
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].event_id, "series");
        assert!(result.diagnostics[0].message.contains("BYHOUR"));
    });
}

#[test]
fn native_window_rejects_excess_templates_and_bad_windows_without_truncating() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        sqlx::query("WITH RECURSIVE n(value) AS (VALUES(1) UNION ALL SELECT value+1 FROM n WHERE value<10000)
            INSERT INTO calendar_events(id, title, start_time, end_time, timezone, calendar_id, rrule)
            SELECT 'extra-' || value, 'Extra', '2024-03-09T09:00:00Z', '2024-03-09T10:00:00Z', 'UTC', 'native-calendar', 'FREQ=DAILY' FROM n")
            .execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert!(
            read(
                &mut tx,
                &window("2024-03-09", "2024-03-09"),
                WindowPurpose::Render,
                false
            )
            .await
            .err()
            .unwrap()
            .contains("10000")
        );
        assert!(
            Window::new(
                "2024-03-10",
                "2024-03-09",
                &civil_time::zone("UTC").unwrap()
            )
            .is_err()
        );
        assert!(
            Window::new(
                "1900-01-01",
                "2026-01-01",
                &civil_time::zone("UTC").unwrap()
            )
            .is_err()
        );
    });
}
