use super::fixtures::*;
use crate::events::scope::read_snapshot;
use crate::recurrence::canonical::{EditScope, ScopeClock, parse_date};

#[test]
fn active_scope_uses_the_current_alias_date_without_rewriting_original_history() {
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
        sqlx::query("UPDATE pomodoro_runs SET original_event_id = 'old-event::2026-05-09', current_occurrence_id = 'event-1::2026-05-10', current_event_date = '2026-05-09'")
            .execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let plan = read_snapshot(&mut tx, "event-1")
            .await
            .unwrap()
            .plan(
                parse_date("2026-05-10").unwrap(),
                EditScope::All,
                now("2026-05-09T09:30:00Z"),
            )
            .unwrap();
        assert!(plan.selected_active);
        assert_eq!(plan.active_recurrence_date.as_deref(), Some("2026-05-10"));
        assert_eq!(plan.effective_scope, EditScope::This);
        let date: String =
            sqlx::query_scalar("SELECT event_date FROM pomodoro_runs WHERE id = 'run-1'")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(date, "2026-05-09");
        tx.rollback().await.unwrap();
    });
}

fn now(value: &str) -> ScopeClock {
    let epoch_ms = chrono::DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp_millis();
    ScopeClock {
        epoch_ms,
        floating_today: None,
    }
}

#[test]
fn historical_protection_uses_the_original_identity_instead_of_the_device_day() {
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
            "event-1::2099-05-20",
            "2099-05-19",
        )
        .await;
        let mut tx = pool.begin().await.unwrap();
        let plan = read_snapshot(&mut tx, "event-1")
            .await
            .unwrap()
            .plan(
                parse_date("2099-05-15").unwrap(),
                EditScope::All,
                now("2099-05-10T12:00:00Z"),
            )
            .unwrap();
        assert_eq!(plan.preserve.len(), 1);
        assert_eq!(plan.preserve[0].recurrence_date, "2099-05-20");
        tx.rollback().await.unwrap();
    });
}

#[test]
fn scope_snapshot_retains_its_history_and_a_fresh_read_detects_later_execution() {
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
        let mut tx = pool.begin().await.unwrap();
        let before: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let snapshot = read_snapshot(&mut tx, "event-1").await.unwrap();
        let after: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(before, after);
        tx.commit().await.unwrap();
        insert_test_completed_pomodoro_history(
            &pool,
            "event-1",
            "event-1::2099-05-20",
            "2099-05-20",
        )
        .await;
        let selected = parse_date("2099-05-15").unwrap();
        let clock = now("2099-05-10T12:00:00Z");
        let earlier = snapshot.plan(selected, EditScope::All, clock).unwrap();
        assert_eq!(earlier.first_mutable.unwrap().recurrence_date, "2099-05-11");
        assert!(earlier.preserve.is_empty());
        let mut tx = pool.begin().await.unwrap();
        let current = read_snapshot(&mut tx, "event-1")
            .await
            .unwrap()
            .plan(selected, EditScope::All, clock)
            .unwrap();
        assert_eq!(current.protected_through.as_deref(), Some("2099-05-10"));
        assert_eq!(current.first_mutable.unwrap().recurrence_date, "2099-05-11");
        assert_eq!(current.preserve.len(), 1);
        assert_eq!(current.preserve[0].recurrence_date, "2099-05-20");
        tx.rollback().await.unwrap();
    });
}

#[test]
fn active_scope_is_derived_from_the_persisted_run() {
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
        let plan = read_snapshot(&mut tx, "event-1")
            .await
            .unwrap()
            .plan(
                parse_date("2026-05-09").unwrap(),
                EditScope::All,
                now("2026-05-09T09:30:00Z"),
            )
            .unwrap();
        assert_eq!(plan.effective_scope, EditScope::This);
        assert!(plan.selected_active && plan.selected_has_history);
        assert_eq!(plan.active_run_id.as_deref(), Some("run-1"));
        tx.rollback().await.unwrap();
    });
}

#[test]
fn history_and_recurrence_children_share_the_same_allocation_allowance() {
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
        sqlx::query("WITH RECURSIVE n(value) AS (SELECT 1 UNION ALL SELECT value + 1 FROM n WHERE value < 9999)
            INSERT INTO calendar_event_rdates (id, event_id, occurrence_start)
            SELECT 'rdate-' || value, 'event-1', date('2026-05-20', '+' || value || ' days') FROM n").execute(&pool).await.unwrap();
        insert_test_open_pomodoro_run(&pool).await;
        let mut tx = pool.begin().await.unwrap();
        let error = read_snapshot(&mut tx, "event-1").await.err().unwrap();
        assert!(error.contains("record or byte budget"), "{error}");
        tx.rollback().await.unwrap();
    });
}

struct FixedDeviceDate;

impl crate::events::scope::DeviceDate for FixedDeviceDate {
    fn local_date(&self, _epoch_ms: i64) -> Result<chrono::NaiveDate, String> {
        parse_date("2026-05-01")
    }
}

#[test]
fn concurrent_scope_previews_wait_for_each_other_instead_of_failing() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2026-05-01T09:00:00Z",
            "2026-05-01T10:00:00Z",
            None,
        )
        .await;
        let preview = |label: &'static str| {
            crate::events::scope::prepare_request_with_clock(
                &pool,
                std::sync::Arc::new(FixedDeviceDate),
                crate::events::scope::ScopeRequest {
                    template_id: "event-1".into(),
                    recurrence_date: "2026-05-01".into(),
                    scope: EditScope::This,
                },
                0,
                move |_, _, _, _| {
                    // Hold the gate long enough that the other preview arrives while it is taken.
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    Ok(label)
                },
            )
        };
        let (edit, deletion) = tokio::join!(preview("edit"), preview("deletion"));
        assert_eq!(edit, Ok("edit"));
        assert_eq!(deletion, Ok("deletion"));
    });
}

#[test]
fn preview_waiting_for_the_connection_leaves_the_gate_to_the_save_holding_it() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2026-05-01T09:00:00Z",
            "2026-05-01T10:00:00Z",
            None,
        )
        .await;
        // A Save holds the vault's only connection and needs the gate inside its transaction.
        let save = pool.begin().await.unwrap();
        let preview = crate::events::scope::prepare_request_with_clock(
            &pool,
            std::sync::Arc::new(FixedDeviceDate),
            crate::events::scope::ScopeRequest {
                template_id: "event-1".into(),
                recurrence_date: "2026-05-01".into(),
                scope: EditScope::This,
            },
            0,
            |_, _, _, _| Ok("edit"),
        );
        tokio::pin!(preview);
        let waiting =
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut preview).await;
        assert!(waiting.is_err(), "preview should wait for the connection");
        // Other tests may briefly hold the process-wide gate, so wait instead of requiring it free.
        assert!(
            crate::events::scope::SCOPE_GATE
                .acquire_within(std::time::Duration::from_secs(5))
                .await
                .is_some()
        );
        save.rollback().await.unwrap();
        assert_eq!(preview.await, Ok("edit"));
    });
}
