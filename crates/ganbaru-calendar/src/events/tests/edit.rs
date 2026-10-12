use super::fixtures::*;
use crate::events::edit::{EventDraft, prepare};
use crate::events::scope::read_snapshot;
use crate::recurrence::canonical::{EditScope, ScopeClock, parse_date};
use serde_json::json;

fn clock() -> ScopeClock {
    ScopeClock {
        epoch_ms: chrono::DateTime::parse_from_rfc3339("2026-05-09T09:30:00Z")
            .unwrap()
            .timestamp_millis(),
        floating_today: Some(parse_date("2026-05-09").unwrap()),
    }
}

#[test]
fn draft_preparation_uses_durable_run_identity_and_performs_no_writes() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2026-05-01T09:00:00Z",
            "2026-05-01T10:00:00Z",
            Some("FREQ=DAILY;COUNT=30"),
        )
        .await;
        insert_test_open_pomodoro_run(&pool).await;
        sqlx::query("UPDATE pomodoro_runs SET original_event_id = 'event-1::2026-05-09'")
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let changes_before: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let draft: EventDraft = serde_json::from_value(json!({"fields":[{"field":"title","value":"new title"}],"timing":{"endTime":"2026-05-09T11:00:00Z"}})).unwrap();
        let prepared = prepare(
            read_snapshot(&mut tx, "event-1").await.unwrap(),
            parse_date("2026-05-09").unwrap(),
            EditScope::All,
            clock(),
            draft,
        )
        .unwrap();
        let value = serde_json::to_value(prepared).unwrap();
        assert_eq!(value["scope"]["effectiveScope"], "this");
        assert_eq!(value["scope"]["activeRunId"], "run-1");
        assert_eq!(value["plan"]["kind"], "detach");
        assert_eq!(value["plan"]["activeTransfer"]["runId"], "run-1");
        assert_eq!(value["plan"]["activeTransfer"]["target"]["kind"], "edited");
        assert_eq!(
            value["selectedAfter"]["endTime"],
            "2026-05-09T11:00:00.000Z"
        );
        let draft: EventDraft =
            serde_json::from_value(json!({"timing":{"startTime":"2026-05-09T09:01:00Z"}})).unwrap();
        let error = prepare(
            read_snapshot(&mut tx, "event-1").await.unwrap(),
            parse_date("2026-05-09").unwrap(),
            EditScope::All,
            clock(),
            draft,
        )
        .err()
        .unwrap();
        assert!(error.contains("recorded start"), "{error}");
        let changes_after: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(changes_before, changes_after);
        let title: String =
            sqlx::query_scalar("SELECT title FROM calendar_events WHERE id = 'event-1'")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_ne!(title, "new title");
        tx.rollback().await.unwrap();
    });
}

#[test]
fn native_plan_materializes_persisted_history_outside_the_selected_occurrence() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2099-05-01T09:00:00Z",
            "2099-05-01T10:00:00Z",
            Some("FREQ=DAILY;COUNT=5"),
        )
        .await;
        insert_test_completed_pomodoro_history(
            &pool,
            "event-1",
            "event-1::2099-05-04",
            "2099-05-04",
        )
        .await;
        let mut tx = pool.begin().await.unwrap();
        let draft =
            serde_json::from_value(json!({"fields":[{"field":"title","value":"future title"}]}))
                .unwrap();
        let prepared = prepare(
            read_snapshot(&mut tx, "event-1").await.unwrap(),
            parse_date("2099-05-02").unwrap(),
            EditScope::All,
            clock(),
            draft,
        )
        .unwrap();
        let value = serde_json::to_value(prepared).unwrap();
        assert_eq!(value["plan"]["kind"], "split");
        assert_eq!(
            value["plan"]["materialize"][0]["recurrenceDate"],
            "2099-05-04"
        );
        assert_eq!(
            value["plan"]["edited"]["fields"]["exceptions"],
            json!(["2099-05-04"])
        );
        let original: String = sqlx::query_scalar(
            "SELECT original_event_id FROM pomodoro_runs WHERE event_date = '2099-05-04'",
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(original, "event-1::2099-05-04");
        tx.rollback().await.unwrap();
    });
}

#[test]
fn floating_conversion_clears_inherited_focus_and_rejects_an_explicit_configuration() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2026-05-01T09:00:00Z",
            "2026-05-01T10:00:00Z",
            Some("FREQ=DAILY;COUNT=30"),
        )
        .await;
        let mut tx = pool.begin().await.unwrap();
        let mut value =
            json!({"timing":{"allDay":true,"startTime":"2026-05-15","endTime":"2026-05-15"}});
        let prepared = prepare(
            read_snapshot(&mut tx, "event-1").await.unwrap(),
            parse_date("2026-05-15").unwrap(),
            EditScope::This,
            clock(),
            serde_json::from_value(value.clone()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(prepared).unwrap()["draft"]["pomodoroConfig"]["action"],
            "clear"
        );
        value["pomodoroConfig"] = json!({"action":"set","value":{"rhythm":{"kind":"count","focusDurationMinutes":25,"shortBreakMinutes":5,"longBreakMinutes":15,"longBreakAfterFocusCount":4},"rhythmSource":"preset","presetKey":"balanced","idleTimeoutMinutes":null}});
        let error = prepare(
            read_snapshot(&mut tx, "event-1").await.unwrap(),
            parse_date("2026-05-15").unwrap(),
            EditScope::This,
            clock(),
            serde_json::from_value(value).unwrap(),
        )
        .err()
        .unwrap();
        assert!(error.contains("cannot enable"), "{error}");
        tx.rollback().await.unwrap();
    });
}
