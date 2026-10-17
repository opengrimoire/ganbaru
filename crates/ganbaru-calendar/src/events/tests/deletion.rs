use super::fixtures::*;
use crate::events::deletion::{PreparedDelete, prepare};
use crate::events::scope::{ScopeRequest, read_snapshot};
use crate::recurrence::canonical::{EditScope, ScopeClock, parse_date};
use serde_json::json;

async fn prepare_write(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    selected: &str,
    scope: EditScope,
    clock: ScopeClock,
    command_id: &str,
    stop_active: bool,
) -> (
    crate::events::commit::CommitRequest,
    crate::events::commit::PreparedCommit,
) {
    let review = prepare(
        read_snapshot(tx, id).await.unwrap(),
        parse_date(selected).unwrap(),
        scope,
        clock,
    )
    .unwrap();
    let request = serde_json::from_value(json!({
        "vaultId":"vault", "vaultGeneration":1, "commandId":command_id, "reviewRevision":review.review_revision,
        "edit":{"kind":"delete", "selection":{"templateId":id, "recurrenceDate":selected, "scope":scope}, "stopActive":stop_active}
    })).unwrap();
    let rows = crate::events::deletion::prepare_commit(
        read_snapshot(tx, id).await.unwrap(),
        parse_date(selected).unwrap(),
        scope,
        clock,
        command_id,
        &review.review_revision,
        stop_active,
    )
    .unwrap();
    (request, rows)
}

#[test]
fn semantic_deletion_archives_complete_task_referenced_source_and_replays_without_live_rows() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let clock = now("2099-05-08T00:00:00Z");
        let mut tx = pool.begin().await.unwrap();
        let (request, rows) = prepare_write(
            &mut tx,
            "source",
            "2099-05-10",
            EditScope::All,
            clock,
            "delete-source",
            false,
        )
        .await;
        let receipt = rows.write(&mut tx).await.unwrap();
        request
            .record_receipt(&mut tx, &receipt, clock.epoch_ms)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let archive: (String, String, Option<String>) = sqlx::query_as("SELECT id, original_occurrence_id, recurrence_date FROM calendar_event_archives WHERE source_event_id='source'")
            .fetch_one(&pool).await.unwrap();
        assert_ne!(archive.0, "source");
        assert_eq!(archive.1, "source");
        assert!(archive.2.is_none());
        let counts: (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT
            (SELECT COUNT(*) FROM calendar_events WHERE id='source'),
            (SELECT COUNT(*) FROM calendar_event_archive_task_links),
            (SELECT COUNT(*) FROM calendar_event_archive_music_assignments),
            (SELECT COUNT(*) FROM calendar_event_archive_overrides)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(counts, (0, 1, 1, 1));
        let mut retry = serde_json::to_value(&request).unwrap();
        retry["vaultGeneration"] = json!(999);
        let retry: crate::events::commit::CommitRequest = serde_json::from_value(retry).unwrap();
        let replay = retry.read_only_retry(&pool).await.unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(replay).unwrap(),
            serde_json::to_value(receipt).unwrap()
        );
        let mut tx = pool.begin().await.unwrap();
        restore_archived_calendar_event_tx(&mut tx, &CalendarEventMutationTarget { id: archive.0 })
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let links: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_task_event_links WHERE event_id='source'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(links, 1);
    });
}

#[test]
fn future_wide_deletion_retains_started_prefix_and_exact_future_history_snapshot() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        sqlx::query("DELETE FROM project_task_event_links")
            .execute(&pool)
            .await
            .unwrap();
        insert_test_completed_pomodoro_history(&pool, "source", "source::2099-05-11", "2099-05-12")
            .await;
        sqlx::raw_sql(
            "UPDATE calendar_events SET icalendar_component_id=NULL;
            UPDATE calendar_event_alarms SET icalendar_component_id=NULL;
            UPDATE calendar_event_attendees SET icalendar_component_id=NULL;
            UPDATE calendar_event_overrides SET icalendar_component_id=NULL;
            DELETE FROM icalendar_objects;",
        )
        .execute(&pool)
        .await
        .unwrap();
        let clock = now("2099-05-10T12:00:00Z");
        let mut tx = pool.begin().await.unwrap();
        let (request, rows) = prepare_write(
            &mut tx,
            "source",
            "2099-05-11",
            EditScope::All,
            clock,
            "delete-mixed",
            false,
        )
        .await;
        let receipt = rows.write(&mut tx).await.unwrap();
        request
            .record_receipt(&mut tx, &receipt, clock.epoch_ms)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let rule: String =
            sqlx::query_scalar("SELECT rrule FROM calendar_events WHERE id='source'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(rule.contains("COUNT=2"), "{rule}");
        let mut tx = pool.begin().await.unwrap();
        let template = read_snapshot(&mut tx, "source")
            .await
            .unwrap()
            .geometry
            .template(true)
            .unwrap();
        for (date, exists) in [
            ("2099-05-09", true),
            ("2099-05-10", true),
            ("2099-05-11", false),
        ] {
            assert_eq!(
                template
                    .resolve_identity(Some(parse_date(date).unwrap()))
                    .unwrap()
                    .is_some(),
                exists,
                "{date}"
            );
        }
        tx.rollback().await.unwrap();
        let archived: (String, String) =
            sqlx::query_as("SELECT id, recurrence_date FROM calendar_event_archives")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(archived.1, "2099-05-11");
        let reference: (Option<String>, String, String, String) = sqlx::query_as("SELECT event_id, original_event_id, event_date, calendar_archive_id FROM pomodoro_runs WHERE id='run-1'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(
            reference,
            (
                None,
                "source::2099-05-11".into(),
                "2099-05-12".into(),
                archived.0.clone()
            )
        );
        let segments: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pomodoro_segments WHERE event_id IS NOT NULL AND run_id='run-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(segments, 0);
        // A capped source cannot revive this identity by removing EXDATE alone.
        let mut tx = pool.begin().await.unwrap();
        assert!(
            restore_archived_calendar_event_tx(
                &mut tx,
                &CalendarEventMutationTarget { id: archived.0 }
            )
            .await
            .is_err()
        );
        tx.rollback().await.unwrap();
        let archives: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_event_archives")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(archives, 1);
    });
}

#[test]
fn native_stop_archival_and_receipts_roll_back_together_and_retry_the_exact_run() {
    use ganbaru_pomodoro::*;
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let clock = now("2099-05-10T10:05:00Z");
        let mut tx = pool.begin().await.unwrap();
        let calendar = crate::reads::focus_context::resolve(
            &mut tx,
            clock.epoch_ms,
            Some("source::2099-05-10"),
        )
        .await
        .unwrap();
        let mut context = FocusExecutionContext {
            now_ms: clock.epoch_ms,
            platform: FocusPlatform::Desktop,
            foreground: true,
            commitment: calendar.commitment,
            planned_blocks: Vec::new(),
            local_time: None,
        };
        let before = focus_execute_command_tx(
            &mut tx,
            &FocusCommand {
                command_id: "start-delete".into(),
                expected_revision: 0,
                intent: FocusIntent::StartScheduled {
                    occurrence_id: Some("source::2099-05-10".into()),
                },
            },
            &context,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        let accepted = now("2099-05-10T10:06:00Z");
        context.now_ms = accepted.epoch_ms;
        for failure in [true, false] {
            if failure {
                sqlx::query("CREATE TRIGGER reject_delete_receipt BEFORE INSERT ON calendar_edit_receipts BEGIN SELECT RAISE(ABORT, 'delete receipt failed'); END")
                    .execute(&pool).await.unwrap();
            }
            let mut tx = pool.begin().await.unwrap();
            let review = prepare(
                read_snapshot(&mut tx, "source").await.unwrap(),
                parse_date("2099-05-10").unwrap(),
                EditScope::All,
                accepted,
            )
            .unwrap();
            let rejected = crate::events::deletion::prepare_commit(
                read_snapshot(&mut tx, "source").await.unwrap(),
                parse_date("2099-05-10").unwrap(),
                EditScope::All,
                accepted,
                "delete-active",
                &review.review_revision,
                false,
            );
            assert!(rejected.err().unwrap().contains("explicitly stop"));
            let (request, rows) = prepare_write(
                &mut tx,
                "source",
                "2099-05-10",
                EditScope::All,
                accepted,
                "delete-active",
                true,
            )
            .await;
            let command = rows.stop_focus_command(&request, &before).unwrap().unwrap();
            let stopped = focus_execute_command_tx(&mut tx, &command, &context)
                .await
                .unwrap();
            assert_eq!(stopped.mode, FocusMode::Stopped);
            let receipt = rows.write(&mut tx).await.unwrap();
            let recorded = request
                .record_receipt(&mut tx, &receipt, accepted.epoch_ms)
                .await;
            if failure {
                assert!(recorded.unwrap_err().contains("delete receipt failed"));
                tx.rollback().await.unwrap();
                let original = focus_read_execution_snapshot(&pool, accepted.epoch_ms)
                    .await
                    .unwrap();
                assert_eq!(original.run, before.run);
                assert_eq!(original.segment, before.segment);
                let archives: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM calendar_event_archives")
                        .fetch_one(&pool)
                        .await
                        .unwrap();
                assert_eq!(archives, 0);
                sqlx::query("DROP TRIGGER reject_delete_receipt")
                    .execute(&pool)
                    .await
                    .unwrap();
            } else {
                recorded.unwrap();
                tx.commit().await.unwrap();
                let run = stopped.run.unwrap();
                assert_eq!(run.ended_at_ms, Some(accepted.epoch_ms));
                assert_eq!(
                    run.planned_end_ms,
                    before.run.as_ref().unwrap().planned_end_ms
                );
                assert_eq!(run.occurrence_id, "source::2099-05-10");
                let projection: (Option<String>, String) = sqlx::query_as(
                    "SELECT event_id, calendar_archive_id FROM pomodoro_runs WHERE id=?",
                )
                .bind(&run.id)
                .fetch_one(&pool)
                .await
                .unwrap();
                assert!(projection.0.is_none());
                let original: String = sqlx::query_scalar(
                    "SELECT original_occurrence_id FROM calendar_event_archives WHERE id=?",
                )
                .bind(projection.1)
                .fetch_one(&pool)
                .await
                .unwrap();
                assert_eq!(original, run.occurrence_id);
                assert!(request.read_only_retry(&pool).await.unwrap().is_some());
            }
        }
    });
}

#[test]
fn duplicate_history_and_child_edits_invalidate_deletion_before_writes() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        insert_test_completed_pomodoro_history(&pool, "source", "source::2099-05-10", "2099-05-10")
            .await;
        let clock = now("2099-05-10T12:00:00Z");
        let mut tx = pool.begin().await.unwrap();
        let review = prepare(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::This,
            clock,
        )
        .unwrap();
        sqlx::query("UPDATE pomodoro_runs SET ended_at='2099-05-10T10:41:00Z' WHERE id='run-1'")
            .execute(&mut *tx)
            .await
            .unwrap();
        let prepared = crate::events::deletion::prepare_commit(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::This,
            clock,
            "delete-stale",
            &review.review_revision,
            false,
        );
        assert!(prepared.err().unwrap().contains("review deletion again"));
        let counts: (i64, i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM calendar_event_archives), (SELECT COUNT(*) FROM calendar_event_exdates)")
            .fetch_one(&mut *tx).await.unwrap();
        assert_eq!(counts, (0, 0));
        tx.rollback().await.unwrap();
    });
}

fn now(value: &str) -> ScopeClock {
    ScopeClock {
        epoch_ms: chrono::DateTime::parse_from_rfc3339(value)
            .unwrap()
            .timestamp_millis(),
        floating_today: None,
    }
}

async fn review(tx: &mut sqlx::SqliteConnection, id: &str, selected: &str) -> PreparedDelete {
    prepare(
        read_snapshot(tx, id).await.unwrap(),
        parse_date(selected).unwrap(),
        EditScope::All,
        now("2099-05-01T00:00:00Z"),
    )
    .unwrap()
}

#[test]
fn floating_deletion_uses_native_device_dates_and_preserves_inclusive_archive_geometry() {
    crate::test_support::block_on(async {
        for (native_today, epoch, archived) in [
            ("2099-05-10", "2099-05-09T23:00:00Z", true),
            ("2099-05-09", "2099-05-10T01:00:00Z", false),
        ] {
            let pool = in_memory_pool().await;
            insert_test_event_at(
                &pool,
                "floating",
                "2099-05-10",
                "2099-05-12",
                Some("FREQ=DAILY;COUNT=3"),
            )
            .await;
            sqlx::query("UPDATE calendar_events SET all_day=1 WHERE id='floating'")
                .execute(&pool)
                .await
                .unwrap();
            let clock = ScopeClock {
                floating_today: Some(parse_date(native_today).unwrap()),
                ..now(epoch)
            };
            let mut tx = pool.begin().await.unwrap();
            let (_, rows) = prepare_write(
                &mut tx,
                "floating",
                "2099-05-10",
                EditScope::This,
                clock,
                "delete-floating",
                false,
            )
            .await;
            rows.write(&mut tx).await.unwrap();
            tx.commit().await.unwrap();
            let archive: Option<(String, String, String, String)> = sqlx::query_as("SELECT start_time, end_time, original_occurrence_id, recurrence_date FROM calendar_event_archives")
                .fetch_optional(&pool).await.unwrap();
            if archived {
                assert_eq!(
                    archive.unwrap(),
                    (
                        "2099-05-10".into(),
                        "2099-05-12".into(),
                        "floating".into(),
                        "2099-05-10".into()
                    )
                );
            } else {
                assert!(archive.is_none());
            }
            let exceptions: Vec<String> = sqlx::query_scalar(
                "SELECT occurrence_date FROM calendar_event_exdates WHERE event_id='floating'",
            )
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(exceptions, vec!["2099-05-10"]);
        }
    });
}

#[test]
fn complete_archive_fanout_is_rejected_before_copying_or_writing_any_rows() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        sqlx::raw_sql("UPDATE calendar_events SET rrule='FREQ=DAILY;COUNT=90' WHERE id='source';
            WITH RECURSIVE item(value) AS (VALUES(1) UNION ALL SELECT value+1 FROM item WHERE value<300)
            INSERT INTO calendar_event_notifications (id, event_id, offset_minutes, sort_order)
            SELECT 'fanout-' || value, 'source', value, value FROM item;")
            .execute(&pool).await.unwrap();
        let clock = now("2099-07-01T12:00:00Z");
        let mut tx = pool.begin().await.unwrap();
        let review = prepare(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::All,
            clock,
        )
        .unwrap();
        let before: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let window = crate::recurrence::canonical::Window::new(
            "2099-05-09",
            "2099-05-15",
            &ganbaru_civil_time::zone("UTC").unwrap(),
        )
        .unwrap();
        let preview = crate::events::preview::project_delete(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::All,
            clock,
            "delete-too-large",
            &window,
        );
        assert!(
            preview
                .err()
                .unwrap()
                .contains("aggregate row or byte budget")
        );
        let result = crate::events::deletion::prepare_commit(
            read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::All,
            clock,
            "delete-too-large",
            &review.review_revision,
            false,
        );
        assert!(
            result
                .err()
                .unwrap()
                .contains("aggregate row or byte budget")
        );
        let after: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(after, before);
        tx.rollback().await.unwrap();
    });
}

#[test]
fn deletion_review_covers_complete_children_and_durable_task_links_without_writes() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let mut tx = pool.begin().await.unwrap();
        let before: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let original = review(&mut tx, "source", "2099-05-10").await;
        assert!(original.plan.archive_source);
        assert_eq!(
            serde_json::to_value(&original.plan).unwrap()["outcome"],
            "archive"
        );
        let after: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(before, after);
        for change in [
            "UPDATE calendar_event_alarms SET trigger_value='-PT11M'",
            "UPDATE calendar_event_override_extended_properties SET property_value='changed'",
            "UPDATE icalendar_value_nodes SET text_value='changed' WHERE id='parameter-value'",
            "UPDATE music_context_assignments SET behavior='pause-music'",
            "UPDATE calendar_event_pomodoro_config_sequence_steps SET focus_duration_minutes=26",
            "UPDATE project_task_event_links SET link_kind='reference'",
        ] {
            sqlx::query("SAVEPOINT change")
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query(change).execute(&mut *tx).await.unwrap();
            assert_ne!(
                original.review_revision,
                review(&mut tx, "source", "2099-05-10")
                    .await
                    .review_revision,
                "{change}"
            );
            sqlx::query("ROLLBACK TO change")
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query("RELEASE change")
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM project_task_event_links")
            .execute(&mut *tx)
            .await
            .unwrap();
        let imported = review(&mut tx, "source", "2099-05-10").await;
        assert!(imported.plan.archive_source);
        assert_ne!(imported.review_revision, original.review_revision);
        sqlx::raw_sql(
            "UPDATE calendar_events SET icalendar_component_id=NULL;
            UPDATE calendar_event_alarms SET icalendar_component_id=NULL;
            UPDATE calendar_event_attendees SET icalendar_component_id=NULL;
            UPDATE calendar_event_overrides SET icalendar_component_id=NULL;
            DELETE FROM icalendar_objects;",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        let unlinked = review(&mut tx, "source", "2099-05-10").await;
        assert!(!unlinked.plan.archive_source);
        assert_ne!(unlinked.review_revision, original.review_revision);
        assert_eq!(
            serde_json::to_value(unlinked.plan).unwrap()["outcome"],
            "delete"
        );
        tx.rollback().await.unwrap();
    });
}

#[test]
fn later_history_changes_native_review_using_original_home_identity_outside_the_selection() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2099-05-01T09:00:00Z",
            "2099-05-01T10:00:00Z",
            Some("FREQ=DAILY;COUNT=60"),
        )
        .await;
        let mut tx = pool.begin().await.unwrap();
        let captured = read_snapshot(&mut tx, "event-1").await.unwrap();
        tx.commit().await.unwrap();
        insert_test_completed_pomodoro_history(
            &pool,
            "event-1",
            "event-1::2099-06-10",
            "2099-06-09",
        )
        .await;
        let selected = parse_date("2099-05-15").unwrap();
        let clock = now("2099-05-01T00:00:00Z");
        let original = prepare(captured, selected, EditScope::All, clock).unwrap();
        assert!(original.plan.archive_occurrences.is_empty());
        let mut tx = pool.begin().await.unwrap();
        let current = prepare(
            read_snapshot(&mut tx, "event-1").await.unwrap(),
            selected,
            EditScope::All,
            clock,
        )
        .unwrap();
        assert_ne!(current.review_revision, original.review_revision);
        assert_eq!(current.plan.archive_occurrences.len(), 1);
        assert_eq!(
            current.plan.archive_occurrences[0].recurrence_date,
            "2099-06-10"
        );
        let day: String =
            sqlx::query_scalar("SELECT event_date FROM pomodoro_runs WHERE id='run-1'")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(day, "2099-06-09");
        tx.rollback().await.unwrap();
    });
}

#[test]
fn selecting_current_focus_alias_requires_that_exact_run_to_stop_without_changing_execution() {
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
        sqlx::query("UPDATE pomodoro_runs SET original_event_id='old-event::2026-05-09', current_occurrence_id='event-1::2026-05-10'").execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let prepared = prepare(
            read_snapshot(&mut tx, "event-1").await.unwrap(),
            parse_date("2026-05-10").unwrap(),
            EditScope::All,
            now("2026-05-10T09:30:00Z"),
        )
        .unwrap();
        assert_eq!(prepared.plan.effective_scope, EditScope::This);
        assert_eq!(prepared.plan.active_run_to_stop.as_deref(), Some("run-1"));
        let run: (String, Option<String>) = sqlx::query_as(
            "SELECT original_event_id, ended_at FROM pomodoro_runs WHERE id='run-1'",
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(run, ("old-event::2026-05-09".into(), None));
        tx.rollback().await.unwrap();
    });
}

#[test]
fn review_expires_at_the_selected_start_and_changed_protection_has_a_different_digest() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2099-05-01T09:00:00Z",
            "2099-05-01T10:00:00Z",
            None,
        )
        .await;
        let mut tx = pool.begin().await.unwrap();
        let selected = parse_date("2099-05-01").unwrap();
        let before = prepare(
            read_snapshot(&mut tx, "event-1").await.unwrap(),
            selected,
            EditScope::This,
            now("2099-05-01T08:59:59.999Z"),
        )
        .unwrap();
        assert_eq!(
            before.plan.valid_until_ms,
            Some(now("2099-05-01T09:00:00Z").epoch_ms)
        );
        let started = prepare(
            read_snapshot(&mut tx, "event-1").await.unwrap(),
            selected,
            EditScope::This,
            now("2099-05-01T09:00:00Z"),
        )
        .unwrap();
        assert!(started.plan.archive_source && started.plan.selected_started);
        assert_ne!(before.review_revision, started.review_revision);
        tx.rollback().await.unwrap();
    });
}

#[test]
fn deletion_input_cannot_supply_clocks_history_operations_or_replacement_recurrence() {
    for key in [
        "now",
        "activeRunId",
        "history",
        "operations",
        "rrule",
        "sourceAfter",
        "archiveSource",
    ] {
        let mut value = json!({"templateId":"event", "recurrenceDate":"2026-05-01", "scope":"all"});
        value[key] = json!("untrusted");
        assert!(
            serde_json::from_value::<ScopeRequest>(value).is_err(),
            "{key}"
        );
    }
}
