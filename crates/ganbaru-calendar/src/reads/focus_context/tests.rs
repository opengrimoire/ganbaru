use super::*;

async fn pool() -> sqlx::SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ganbaru_db::run_migrations(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO calendars(id, name, source) VALUES('focus-calendar', 'Focus', 'local');
        INSERT INTO calendar_events(id, title, start_time, end_time, timezone, calendar_id, created_at)
        VALUES('long', 'Long', '2024-03-11T09:00:00Z', '2024-03-11T12:00:00Z', 'UTC', 'focus-calendar', '2024-01-01'),
              ('short', 'Short', '2024-03-11T10:00:00Z', '2024-03-11T11:00:00Z', 'UTC', 'focus-calendar', '2024-01-02');
        INSERT INTO calendar_event_pomodoro_configs(event_id, rhythm_kind, rhythm_source) VALUES('long', 'count', 'custom'), ('short', 'count', 'custom');
        INSERT INTO calendar_event_pomodoro_config_count_rhythms(event_id, focus_duration_minutes, short_break_minutes, long_break_minutes, long_break_after_focus_count)
        VALUES('long', 40, 5, 10, 4), ('short', 40, 5, 10, 4);")
        .execute(&pool).await.unwrap();
    pool
}

fn now(value: &str) -> i64 {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp_millis()
}

#[test]
fn native_focus_selects_the_earliest_end_and_exact_end_boundaries() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let mut tx = pool.begin().await.unwrap();
        let zone = TimeZone::UTC;
        let context = resolve_in_zone(&mut tx, now("2024-03-11T10:15:00Z"), None, &zone)
            .await
            .unwrap();
        assert_eq!(context.commitment.unwrap().occurrence_id, "short");
        assert_eq!(context.next_boundary_ms, Some(now("2024-03-11T11:00:00Z")));
        assert_eq!(context.planned_blocks.len(), 2);
        let ended = resolve_in_zone(&mut tx, now("2024-03-11T11:00:00Z"), None, &zone)
            .await
            .unwrap();
        assert_eq!(ended.commitment.unwrap().occurrence_id, "long");
        let gap = resolve_in_zone(&mut tx, now("2024-03-11T12:00:00Z"), None, &zone)
            .await
            .unwrap();
        assert!(gap.commitment.is_none());
    });
}

#[test]
fn native_focus_keeps_home_zone_recurrence_identity_and_uses_the_device_day_for_its_plan() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        sqlx::query("UPDATE calendar_events SET start_time='2024-03-11T00:15:00Z', end_time='2024-03-11T02:15:00Z', rrule='FREQ=DAILY;COUNT=2' WHERE id='long'")
            .execute(&mut *tx).await.unwrap();
        let context = resolve_in_zone(
            &mut tx,
            now("2024-03-12T00:30:00Z"),
            Some("long::2024-03-12"),
            &civil_time::zone("America/Monterrey").unwrap(),
        )
        .await
        .unwrap();
        let commitment = context.commitment.unwrap();
        assert_eq!(commitment.occurrence_id, "long::2024-03-12");
        assert_eq!(commitment.event_date, "2024-03-11");
        assert!(
            context
                .planned_blocks
                .iter()
                .any(|block| block.event_id == commitment.occurrence_id)
        );
        assert!(
            context
                .planned_blocks
                .iter()
                .all(|block| block.event_date == commitment.event_date)
        );
    });
}

#[test]
fn native_focus_late_acceptance_captures_the_complete_original_day_plan_for_a_multi_day_owner() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        sqlx::raw_sql("UPDATE calendar_events SET start_time='2024-03-09T23:00:00Z', end_time='2024-03-12T01:00:00Z' WHERE id='long';
            UPDATE calendar_events SET start_time='2024-03-09T12:00:00Z', end_time='2024-03-09T13:00:00Z' WHERE id='short';")
            .execute(&mut *tx).await.unwrap();
        let context = resolve_in_zone(
            &mut tx,
            now("2024-03-12T00:30:00Z"),
            Some("long"),
            &TimeZone::UTC,
        )
        .await
        .unwrap();
        assert_eq!(context.commitment.unwrap().event_date, "2024-03-09");
        assert_eq!(context.planned_blocks.len(), 2);
        assert!(
            context
                .planned_blocks
                .iter()
                .any(|block| block.event_id == "short")
        );
        assert!(
            context
                .planned_blocks
                .iter()
                .all(|block| block.event_date == "2024-03-09")
        );
    });
}

#[test]
fn native_focus_resolves_explicit_selection_without_falling_back_to_another_owner() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let mut tx = pool.begin().await.unwrap();
        let time = now("2024-03-11T10:15:00Z");
        let explicit = resolve_in_zone(&mut tx, time, Some("long"), &TimeZone::UTC)
            .await
            .unwrap();
        assert_eq!(explicit.commitment.unwrap().occurrence_id, "long");
        let missing = resolve_in_zone(&mut tx, time, Some("not-current"), &TimeZone::UTC)
            .await
            .unwrap();
        assert!(missing.commitment.is_none());
    });
}

#[test]
fn native_focus_reads_its_existing_owner_from_committed_execution_in_the_same_transaction() {
    crate::test_support::block_on(async {
        for recurring in [false, true] {
            let pool = pool().await;
            if recurring {
                sqlx::query(
                    "UPDATE calendar_events SET start_time = '2024-03-10T09:00:00Z', end_time = '2024-03-10T12:00:00Z', rrule = 'FREQ=DAILY;COUNT=3' WHERE id = 'long'",
                )
                .execute(&pool)
                .await
                .unwrap();
            }
            let expected = if recurring {
                "long::2024-03-11"
            } else {
                "long"
            };
            let time = now("2024-03-11T10:15:00Z");
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
            let chosen = resolve_in_zone(&mut tx, time, Some(expected), &TimeZone::UTC)
                .await
                .unwrap();
            let context = ganbaru_pomodoro::FocusExecutionContext {
                now_ms: time,
                platform: ganbaru_pomodoro::FocusPlatform::Desktop,
                foreground: true,
                local_time: None,
                planned_blocks: Vec::new(),
                commitment: chosen.commitment,
            };
            ganbaru_pomodoro::focus_execute_command_tx(
                &mut tx,
                &ganbaru_pomodoro::FocusCommand {
                    command_id: "start-long".into(),
                    expected_revision: 0,
                    intent: ganbaru_pomodoro::FocusIntent::StartScheduled {
                        occurrence_id: Some(expected.into()),
                    },
                },
                &context,
            )
            .await
            .unwrap();
            let retained = resolve_in_zone(&mut tx, time + 1, None, &TimeZone::UTC)
                .await
                .unwrap();
            assert_eq!(retained.commitment.unwrap().occurrence_id, expected);
            tx.commit().await.unwrap();
            let mut restarted = pool.begin().await.unwrap();
            let retained = resolve_in_zone(&mut restarted, time + 2, None, &TimeZone::UTC)
                .await
                .unwrap();
            assert_eq!(retained.commitment.unwrap().occurrence_id, expected);
        }
    });
}

#[test]
fn native_focus_canonical_edit_changes_the_fingerprint_and_cancelled_events_are_ineligible() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let time = now("2024-03-11T10:15:00Z");
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let before = resolve_in_zone(&mut tx, time, Some("short"), &TimeZone::UTC)
            .await
            .unwrap()
            .commitment
            .unwrap();
        sqlx::query("UPDATE calendar_event_pomodoro_config_count_rhythms SET focus_duration_minutes=25 WHERE event_id='short'")
            .execute(&mut *tx).await.unwrap();
        let after = resolve_in_zone(&mut tx, time, Some("short"), &TimeZone::UTC)
            .await
            .unwrap()
            .commitment
            .unwrap();
        assert_ne!(before.calendar_revision, after.calendar_revision);
        sqlx::query("UPDATE calendar_events SET status='cancelled' WHERE id='short'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            resolve_in_zone(&mut tx, time, Some("short"), &TimeZone::UTC)
                .await
                .unwrap()
                .commitment
                .is_none()
        );
    });
}
